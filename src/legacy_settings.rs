//! Read-only legacy settings inspection. Import never executes stored tools or changes ACDSee.
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{
    collections::{BTreeMap, HashSet},
    fs,
    path::{Path, PathBuf},
};

const MAX_IMPORT: usize = 8 * 1024 * 1024;
const MAX_BINDINGS: usize = 4096;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub struct ShortcutBinding {
    pub table: u16,
    pub virtual_key: bool,
    pub modifiers: u8,
    pub key: u16,
    pub command: u16,
}
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct ImportReport {
    pub bindings: Vec<ShortcutBinding>,
    /// A legacy accelerator export is a complete table. Do not silently restore removed defaults.
    pub replace_tables: Vec<u16>,
    pub settings: BTreeMap<String, Value>,
    pub sources: Vec<PathBuf>,
    pub ignored: Vec<String>,
}

#[derive(Clone)]
enum RawValue {
    Text(String),
    Dword(u32),
    Binary(Vec<u8>),
}
type Sections = BTreeMap<String, BTreeMap<String, RawValue>>;

fn known_commands() -> HashSet<u16> {
    include_str!("../docs/retained-command-audit.csv")
        .lines()
        .skip(1)
        .filter_map(|l| l.split(',').next()?.parse().ok())
        .collect()
}
fn allowed_table(table: u16) -> bool {
    matches!(table, 2 | 552 | 553)
}
fn validate(bindings: &[ShortcutBinding]) -> Result<(), String> {
    if bindings.len() > MAX_BINDINGS {
        return Err("自定义按键超过4096项".into());
    }
    let known = known_commands();
    let mut keys = HashSet::new();
    for binding in bindings {
        if !allowed_table(binding.table)
            || binding.modifiers & !7 != 0
            || binding.key == 0
            || (binding.virtual_key && binding.key > 255)
            || !known.contains(&binding.command)
        {
            return Err("按键包含未知模式、修饰键或命令ID，未应用该导入".into());
        }
        if !keys.insert((
            binding.table,
            binding.virtual_key,
            binding.modifiers,
            binding.key,
        )) {
            return Err("自定义按键存在同模式冲突，未应用该导入".into());
        }
    }
    Ok(())
}

fn decode_text(bytes: &[u8]) -> Result<String, String> {
    if bytes.starts_with(&[0xff, 0xfe]) {
        if bytes.len() % 2 != 0 {
            return Err("UTF-16配置文件长度无效".into());
        }
        let words = bytes[2..]
            .chunks_exact(2)
            .map(|b| u16::from_le_bytes([b[0], b[1]]))
            .collect::<Vec<_>>();
        String::from_utf16(&words).map_err(|_| "配置文件包含无效UTF-16".into())
    } else {
        String::from_utf8(
            bytes
                .strip_prefix(&[0xef, 0xbb, 0xbf])
                .unwrap_or(bytes)
                .to_vec(),
        )
        .map_err(|_| "请提供UTF-8 JSON或Windows Registry Editor 5.00导出的UTF-16 REG文件".into())
    }
}

fn quoted(text: &str) -> Option<(String, &str)> {
    let mut chars = text.strip_prefix('"')?.char_indices();
    let mut output = String::new();
    while let Some((index, c)) = chars.next() {
        match c {
            '"' => return Some((output, &text[index + 2..])),
            '\\' => {
                let (_, next) = chars.next()?;
                if !matches!(next, '\\' | '"') {
                    return None;
                }
                output.push(next);
            }
            _ => output.push(c),
        }
    }
    None
}

fn reg_sections(text: &str) -> Result<Sections, String> {
    if !text
        .trim_start_matches('\u{feff}')
        .starts_with("Windows Registry Editor Version 5.00")
        && !text.starts_with("REGEDIT4")
    {
        return Err("不是Windows REG导出文件".into());
    }
    let mut lines = Vec::new();
    let mut logical = String::new();
    for line in text.lines() {
        let line = line.trim();
        logical.push_str(line.trim_end_matches('\\'));
        if !line.ends_with('\\') {
            lines.push(std::mem::take(&mut logical));
        }
    }
    if !logical.is_empty() {
        return Err("REG文件结尾的续行不完整".into());
    }
    let mut sections = Sections::new();
    let mut current = None;
    for line in lines {
        if line.is_empty() || line.starts_with(';') {
            continue;
        }
        if line.starts_with('[') && line.ends_with(']') {
            let path = &line[1..line.len() - 1];
            let lower = path.to_ascii_lowercase();
            current = (lower.starts_with("hkey_current_user\\software\\acd systems\\")
                || lower.starts_with("hkey_current_user\\software\\wow6432node\\acd systems\\")
                || lower == "hkey_current_user\\software\\acd systems")
                .then(|| path.to_owned());
            if let Some(path) = &current {
                if sections.len() >= 1024 {
                    return Err("旧设置超过1024个键上限".into());
                }
                sections.entry(path.clone()).or_default();
            }
            continue;
        }
        let Some(path) = &current else {
            continue;
        };
        let Some((name, rest)) = quoted(&line) else {
            continue;
        };
        let Some(raw) = rest.trim_start().strip_prefix('=') else {
            continue;
        };
        let raw = raw.trim();
        let value = if let Some(hex) = raw.strip_prefix("dword:") {
            RawValue::Dword(u32::from_str_radix(hex, 16).map_err(|_| "REG DWORD无效")?)
        } else if let Some(hex) = raw.strip_prefix("hex:") {
            let bytes = hex
                .split(',')
                .filter(|x| !x.trim().is_empty())
                .map(|v| u8::from_str_radix(v.trim(), 16).map_err(|_| "REG二进制无效"))
                .collect::<Result<Vec<_>, _>>()?;
            if bytes.len() > MAX_IMPORT {
                return Err("REG二进制超过8 MiB".into());
            }
            RawValue::Binary(bytes)
        } else if let Some((value, rest)) = quoted(raw) {
            if !rest.trim().is_empty() {
                return Err("REG字符串结尾无效".into());
            }
            RawValue::Text(value)
        } else {
            continue;
        };
        sections.get_mut(path).unwrap().insert(name, value);
    }
    Ok(sections)
}

/// Only complete, structurally valid ACCEL tables with known command IDs are recognized.
/// Conflicting 6-/8-byte interpretations are rejected instead of selecting a convenient guess.
pub fn decode_accelerators(table: u16, bytes: &[u8]) -> Result<Vec<ShortcutBinding>, String> {
    if allowed_table(table) && (bytes.is_empty() || bytes == [0, 0, 0, 0]) {
        return Ok(Vec::new());
    }
    if !allowed_table(table) {
        return Err("此加速键表上下文不属于管理、查看或比较模式".into());
    }
    let known = known_commands();
    let mut results = Vec::<Vec<ShortcutBinding>>::new();
    for stride in [6usize, 8] {
        for header in [0usize, 4] {
            if bytes.len() < header || (bytes.len() - header) % stride != 0 {
                continue;
            }
            let count = (bytes.len() - header) / stride;
            if count == 0 || count > MAX_BINDINGS {
                continue;
            }
            if header == 4 && u32::from_le_bytes(bytes[..4].try_into().unwrap()) as usize != count {
                continue;
            }
            let mut output = Vec::new();
            let mut valid = true;
            for (index, entry) in bytes[header..].chunks_exact(stride).enumerate() {
                let flags = entry[0];
                let key = u16::from_le_bytes([entry[2], entry[3]]);
                let command = u16::from_le_bytes([entry[4], entry[5]]);
                if entry[1] != 0
                    || flags & !0x9f != 0
                    || (flags & 0x80 != 0 && index + 1 != count)
                    || key == 0
                    || !known.contains(&command)
                    || (stride == 8 && (entry[6] != 0 || entry[7] != 0))
                {
                    valid = false;
                    break;
                }
                output.push(ShortcutBinding {
                    table,
                    virtual_key: flags & 1 != 0,
                    modifiers: u8::from(flags & 8 != 0)
                        | (u8::from(flags & 4 != 0) << 1)
                        | (u8::from(flags & 16 != 0) << 2),
                    key,
                    command,
                });
            }
            if valid && validate(&output).is_ok() && !results.contains(&output) {
                results.push(output);
            }
        }
    }
    if results.len() != 1 {
        return Err("旧加速键二进制结构／上下文不能唯一确认，未导入".into());
    }
    Ok(results.remove(0))
}

fn section_table(path: &str, values: &BTreeMap<String, RawValue>) -> Option<u16> {
    let suffix = path.rsplit('\\').next()?.to_ascii_lowercase();
    let explicit = suffix
        .strip_prefix("keyboard-")
        .or_else(|| suffix.strip_prefix("keyboardtable-"))
        .and_then(|s| s.parse::<u16>().ok());
    if let Some(table) = explicit.filter(|t| allowed_table(*t)) {
        return Some(table);
    }
    if suffix.starts_with("keyboard") {
        for name in ["ResourceID", "ResourceId", "TableID"] {
            if let Some(RawValue::Dword(n)) = values.get(name) {
                let table = u16::try_from(*n).ok()?;
                if allowed_table(table) {
                    return Some(table);
                }
            }
        }
    }
    None
}
fn from_sections(sections: Sections) -> ImportReport {
    let mut report = ImportReport::default();
    for (path, values) in sections {
        let table = section_table(&path, &values);
        for (name, value) in &values {
            let key = format!("{path}\\{name}");
            let lower = name.to_ascii_lowercase();
            if [
                "password",
                "passwd",
                "token",
                "credential",
                "secret",
                "cookie",
            ]
            .iter()
            .any(|part| lower.contains(part))
            {
                report.ignored.push(format!("{key}：未导入认证字段"));
                continue;
            }
            match value {
                RawValue::Text(value) => {
                    report.settings.insert(key, Value::String(value.clone()));
                }
                RawValue::Dword(value) => {
                    report.settings.insert(key, serde_json::json!(value));
                }
                RawValue::Binary(bytes) => {
                    if name.eq_ignore_ascii_case("Accelerators") {
                        match table
                            .ok_or_else(|| "缺少已确认的表上下文".to_string())
                            .and_then(|t| decode_accelerators(t, bytes))
                        {
                            Ok(bindings) => {
                                report.bindings.extend(bindings);
                                if let Some(table) = table {
                                    if !report.replace_tables.contains(&table) {
                                        report.replace_tables.push(table);
                                    }
                                }
                            }
                            Err(error) => report
                                .ignored
                                .push(format!("{key}：{error}（{}字节）", bytes.len())),
                        }
                    } else {
                        report
                            .ignored
                            .push(format!("{key}：未解释的二进制（{}字节）", bytes.len()));
                    }
                }
            }
        }
    }
    report
}

pub fn import_file(path: &Path) -> Result<ImportReport, String> {
    if !path.is_file() || path.metadata().map_err(|e| e.to_string())?.len() > MAX_IMPORT as u64 {
        return Err("设置导入只接受8 MiB以内的文件".into());
    }
    let text = decode_text(&fs::read(path).map_err(|e| e.to_string())?)?;
    let mut report = if path
        .extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| e.eq_ignore_ascii_case("reg"))
    {
        from_sections(reg_sections(&text)?)
    } else {
        serde_json::from_str::<ImportReport>(&text)
            .map_err(|_| "不是RustView支持的快捷键JSON导出格式".to_string())?
    };
    validate(&report.bindings)?;
    if report.replace_tables.iter().any(|t| !allowed_table(*t)) {
        return Err("导入包含被排除的旧模式".into());
    }
    report.sources.push(path.to_owned());
    Ok(report)
}

pub fn export_file(path: &Path, report: &ImportReport) -> Result<(), String> {
    validate(&report.bindings)?;
    let bytes = serde_json::to_vec_pretty(report).map_err(|e| e.to_string())?;
    if bytes.len() > MAX_IMPORT {
        return Err("设置超过8 MiB".into());
    }
    let parent = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let mut temp = tempfile::Builder::new()
        .prefix(".rustview-keys-")
        .tempfile_in(parent)
        .map_err(|e| e.to_string())?;
    use std::io::Write;
    temp.write_all(&bytes)
        .and_then(|_| temp.as_file().sync_all())
        .map_err(|e| e.to_string())?;
    temp.persist(path).map_err(|e| e.to_string())?;
    Ok(())
}

#[cfg(windows)]
pub fn inspect_current_user() -> Result<ImportReport, String> {
    use windows_sys::Win32::{
        Foundation::{ERROR_NO_MORE_ITEMS, ERROR_SUCCESS},
        System::Registry::*,
    };
    struct Key(HKEY);
    impl Drop for Key {
        fn drop(&mut self) {
            unsafe {
                RegCloseKey(self.0);
            }
        }
    }
    fn wide(s: &str) -> Vec<u16> {
        s.encode_utf16().chain(Some(0)).collect()
    }
    unsafe fn visit(
        key: HKEY,
        path: String,
        sections: &mut Sections,
        depth: usize,
        budget: &mut usize,
    ) -> Result<(), String> {
        if depth > 12 || sections.len() >= 1024 {
            return Err("旧设置树超过读取上限".into());
        }
        let mut values = BTreeMap::new();
        for index in 0..4096 {
            let mut name = vec![0u16; 16384];
            let mut name_len = name.len() as u32;
            let mut ty = 0;
            let mut data = vec![0u8; 1024 * 1024];
            let mut len = data.len() as u32;
            let error = RegEnumValueW(
                key,
                index,
                name.as_mut_ptr(),
                &mut name_len,
                std::ptr::null_mut(),
                &mut ty,
                data.as_mut_ptr(),
                &mut len,
            );
            if error == ERROR_NO_MORE_ITEMS {
                break;
            }
            if error != ERROR_SUCCESS {
                continue;
            }
            data.truncate(len as usize);
            let needed = data.len() + name_len as usize * 2;
            if needed > *budget {
                return Err("旧设置数据超过8 MiB读取预算".into());
            }
            *budget -= needed;
            let name = String::from_utf16_lossy(&name[..name_len as usize]);
            let value = match ty {
                REG_DWORD if data.len() == 4 => Some(RawValue::Dword(u32::from_le_bytes(
                    data.try_into().unwrap(),
                ))),
                REG_SZ | REG_EXPAND_SZ if data.len() % 2 == 0 => {
                    Some(RawValue::Text(String::from_utf16_lossy(
                        &data
                            .chunks_exact(2)
                            .map(|b| u16::from_le_bytes([b[0], b[1]]))
                            .take_while(|c| *c != 0)
                            .collect::<Vec<_>>(),
                    )))
                }
                REG_BINARY => Some(RawValue::Binary(data)),
                _ => None,
            };
            if let Some(value) = value {
                values.insert(name, value);
            }
        }
        sections.insert(path.clone(), values);
        for index in 0..1024 {
            let mut name = [0u16; 512];
            let mut len = name.len() as u32;
            let error = RegEnumKeyExW(
                key,
                index,
                name.as_mut_ptr(),
                &mut len,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
            );
            if error == ERROR_NO_MORE_ITEMS {
                break;
            }
            if error != ERROR_SUCCESS {
                continue;
            }
            let name = String::from_utf16_lossy(&name[..len as usize]);
            let mut child = std::ptr::null_mut();
            if RegOpenKeyExW(key, wide(&name).as_ptr(), 0, KEY_READ, &mut child) == ERROR_SUCCESS {
                let child = Key(child);
                visit(
                    child.0,
                    format!("{path}\\{name}"),
                    sections,
                    depth + 1,
                    budget,
                )?;
            }
        }
        Ok(())
    }
    let mut sections = Sections::new();
    let mut budget = MAX_IMPORT;
    for view in [KEY_WOW64_32KEY, KEY_WOW64_64KEY] {
        unsafe {
            let path = "Software\\ACD Systems";
            let mut key = std::ptr::null_mut();
            if RegOpenKeyExW(
                HKEY_CURRENT_USER,
                wide(path).as_ptr(),
                0,
                KEY_READ | view,
                &mut key,
            ) == ERROR_SUCCESS
            {
                let key = Key(key);
                visit(
                    key.0,
                    format!("HKEY_CURRENT_USER\\{path}"),
                    &mut sections,
                    0,
                    &mut budget,
                )?;
            }
        }
    }
    let mut report = from_sections(sections);
    if report.settings.is_empty() && report.bindings.is_empty() {
        report.ignored.push("当前用户未找到可读取的ACD Systems设置；这份复制目录的XML不是自定义按键文件。可导入原用户导出的REG或本程序JSON配置。".into());
    }
    Ok(report)
}
#[cfg(not(windows))]
pub fn inspect_current_user() -> Result<ImportReport, String> {
    Ok(ImportReport {
        ignored: vec!["当前系统没有Windows用户注册表".into()],
        ..Default::default()
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn accel_layout_count_modifiers_and_unknown_ids_are_checked() {
        let data = [0x09, 0, 0x4f, 0, 0x36, 0x08, 0x11, 0, 0x25, 0, 0x19, 0x09];
        let bindings = decode_accelerators(2, &data).unwrap();
        assert_eq!(bindings[0].command, 2102);
        assert_eq!(bindings[0].modifiers, 1);
        assert_eq!(bindings[1].modifiers, 4);
        let mut bad = data;
        bad[4] = 0xff;
        bad[5] = 0xff;
        assert!(decode_accelerators(2, &bad).is_err());
        assert!(decode_accelerators(21800, &data).is_err());
        let mut counted = 2u32.to_le_bytes().to_vec();
        counted.extend(data);
        assert_eq!(decode_accelerators(2, &counted).unwrap(), bindings);
    }
    #[test]
    fn reg_export_is_read_only_and_unidentified_binary_is_not_guessed() {
        let text="Windows Registry Editor Version 5.00\n[HKEY_CURRENT_USER\\Software\\ACD Systems\\ACDSee Pro\\50\\Keyboard-2]\n\"Accelerators\"=hex:09,00,4f,00,36,08\n\"ZoomLock\"=dword:00000001\n[HKEY_CURRENT_USER\\Software\\Other]\n\"Accelerators\"=hex:09,00,4f,00,36,08\n";
        let report = from_sections(reg_sections(text).unwrap());
        assert_eq!(report.bindings.len(), 1);
        assert_eq!(report.replace_tables, [2]);
        assert_eq!(report.settings.len(), 1);
        let unknown = text.replace("Keyboard-2", "Keyboard-0");
        let report = from_sections(reg_sections(&unknown).unwrap());
        assert!(report.bindings.is_empty());
        assert_eq!(report.ignored.len(), 1);
    }
    #[test]
    fn json_roundtrip_rejects_duplicate_keys_and_excluded_context() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("shortcuts.json");
        let binding = ShortcutBinding {
            table: 552,
            virtual_key: true,
            modifiers: 1,
            key: 0x4f,
            command: 2258,
        };
        let report = ImportReport {
            bindings: vec![binding.clone()],
            replace_tables: vec![552],
            ..Default::default()
        };
        export_file(&path, &report).unwrap();
        assert_eq!(import_file(&path).unwrap().bindings, [binding.clone()]);
        assert!(validate(&[binding.clone(), binding]).is_err());
    }
}

//! Versioned, non-destructive processing records and reusable export recipes.
use crate::{
    editing::Edit,
    precision::{ExportSettings, RawSettings},
};
use serde::{Deserialize, Serialize};
use std::{
    fs,
    io::{Read, Write},
    path::{Path, PathBuf},
    time::UNIX_EPOCH,
};

const MAX_RECORD: u64 = 1024 * 1024;
const MAX_PRESETS: usize = 128;

pub fn data_root() -> Result<PathBuf, String> {
    let executable = std::env::current_exe().map_err(|e| e.to_string())?;
    let directory = executable.parent().ok_or("无法确定程序目录")?;
    Ok(directory
        .ancestors()
        .take(4)
        .map(|p| p.join("RustView-data"))
        .find(|p| p.is_dir())
        .unwrap_or_else(|| directory.join("RustView-data")))
}

/// Hashes only the canonical container/file identity and entry name, never a password.
pub fn record_path(root: &Path, source: &Path, entry: Option<&str>) -> Result<PathBuf, String> {
    let source = source.canonicalize().map_err(|e| e.to_string())?;
    let identity = serde_json::to_vec(&(source, entry)).map_err(|e| e.to_string())?;
    let hash = identity.iter().fold(0xcbf29ce484222325_u64, |hash, byte| {
        (hash ^ u64::from(*byte)).wrapping_mul(0x100000001b3)
    });
    Ok(root.join("edits").join(format!("{hash:016x}.json")))
}

pub fn read_for(source: &Path, entry: Option<&str>) -> Option<EditRecord> {
    read_for_in(&data_root().ok()?, source, entry)
}

pub fn read_for_in(root: &Path, source: &Path, entry: Option<&str>) -> Option<EditRecord> {
    let canonical = source.canonicalize().ok()?;
    let record = EditRecord::load(&record_path(root, &canonical, entry).ok()?).ok()?;
    // A hash is a filename, not proof of identity; a collision must never apply another recipe.
    (record.source == canonical && record.entry.as_deref() == entry).then_some(record)
}

fn normal_path(path: &Path) -> String {
    let path = path.to_string_lossy().replace('\\', "/");
    let path = if let Some(rest) = path.strip_prefix("//?/UNC/") {
        format!("//{rest}")
    } else {
        path.strip_prefix("//?/").unwrap_or(&path).to_owned()
    };
    path.trim_end_matches('/').to_ascii_lowercase()
}

/// Files have already moved. Read one bounded record at a time, without validating the obsolete path.
pub fn relocate_keys(root: &Path, moves: &[(String, String)]) -> Result<(), String> {
    relocate_keys_with_updates(root, moves, &[])
}

#[derive(Clone, Debug)]
pub struct ContainerUpdate {
    pub path: PathBuf,
    pub before: SourceStamp,
    pub after: SourceStamp,
    pub removed: Vec<String>,
}

/// `containers` comes only from a verified archive operation, with its before/after fingerprints.
pub fn relocate_keys_with_updates(
    root: &Path,
    moves: &[(String, String)],
    containers: &[ContainerUpdate],
) -> Result<(), String> {
    use std::collections::{HashMap, HashSet};
    if moves.is_empty() && containers.is_empty() {
        return Ok(());
    }
    if moves.len() > 100_000
        || moves
            .iter()
            .map(|(a, b)| a.len().saturating_add(b.len()))
            .sum::<usize>()
            > 32 * 1024 * 1024
    {
        return Err("无损记录迁移计划超过十万条或32 MiB".into());
    }
    let mut direct = HashMap::new();
    let mut prefixes = HashMap::new();
    if containers.len() > 100_000
        || containers.iter().map(|u| u.removed.len()).sum::<usize>() > 100_000
        || containers
            .iter()
            .flat_map(|u| &u.removed)
            .map(String::len)
            .sum::<usize>()
            > 32 * 1024 * 1024
    {
        return Err("压缩包记录迁移超过十万个条目或32 MiB".into());
    }
    let mut updates = HashMap::new();
    for update in containers {
        if SourceStamp::read(&update.path)? != update.after {
            return Err("压缩包在操作后再次改变，未刷新无损记录".into());
        }
        updates.insert(normal_path(&update.path), update);
    }
    for (old, new) in moves {
        let (old_path, old_entry): (PathBuf, Option<String>) =
            serde_json::from_str(old).map_err(|_| "无损记录旧身份键无效")?;
        let (new_path, new_entry): (PathBuf, Option<String>) =
            serde_json::from_str(new).map_err(|_| "无损记录新身份键无效")?;
        let normalized = normal_path(&old_path);
        if direct
            .insert(
                (normalized.clone(), old_entry.clone()),
                (new_path.clone(), new_entry.clone()),
            )
            .is_some()
        {
            return Err("无损记录迁移源键重复".into());
        }
        if old_entry.is_none() && new_entry.is_none() {
            prefixes.insert(normalized, new_path);
        }
    }
    let directory = root.join("edits");
    if !directory.exists() {
        return Ok(());
    }
    // Stage bounded JSON on disk; only filenames stay in memory. Old files are
    // retained until every source identity and destination collision is checked.
    let transaction = tempfile::Builder::new()
        .prefix(".relocate-")
        .tempdir_in(&directory)
        .map_err(|e| e.to_string())?;
    let mut plans = Vec::new();
    let mut staged_bytes = 0u64;
    let mut count = 0usize;
    for item in fs::read_dir(&directory).map_err(|e| e.to_string())? {
        let item = item.map_err(|e| e.to_string())?;
        let old_file = item.path();
        if old_file.extension().and_then(|e| e.to_str()) != Some("json") {
            continue;
        }
        count += 1;
        if count > 100_000 {
            return Err("无损记录扫描超过十万条，未删除剩余记录".into());
        }
        let mut record: EditRecord = read_json(&old_file, MAX_RECORD)?;
        let normalized = normal_path(&record.source);
        let target = if let Some(target) = direct.get(&(normalized.clone(), record.entry.clone())) {
            Some(target.clone())
        } else {
            let mut prefix = normalized.as_str();
            let mut found = None;
            loop {
                if let Some(target) = prefixes.get(prefix) {
                    // Preserve original component spelling; comparison alone is case folded.
                    let original = record.source.to_string_lossy().replace('\\', "/");
                    let original = if let Some(rest) = original.strip_prefix("//?/UNC/") {
                        format!("//{rest}")
                    } else {
                        original
                            .strip_prefix("//?/")
                            .unwrap_or(&original)
                            .to_owned()
                    };
                    let mut path = target.clone();
                    for component in original[prefix.len()..]
                        .split('/')
                        .filter(|s| !s.is_empty())
                    {
                        path.push(component);
                    }
                    found = Some((path, record.entry.clone()));
                    break;
                }
                let Some(end) = prefix.rfind('/') else {
                    break;
                };
                prefix = &prefix[..end];
            }
            found.or_else(|| {
                updates
                    .get(&normalized)
                    .filter(|_| record.entry.is_some())
                    .map(|update| (update.path.clone(), record.entry.clone()))
            })
        };
        let Some((source, entry)) = target else {
            continue;
        };
        if record.version != 1 {
            return Err("待迁移无损记录版本不受支持；旧记录保留".into());
        }
        record.recipe.validate()?;
        let source = source
            .canonicalize()
            .map_err(|e| format!("无损记录新源不存在；旧记录保留：{e}"))?;
        let current_stamp = SourceStamp::read(&source)?;
        let trusted_update = updates.get(&normal_path(&source)).filter(|update| {
            record.entry.is_some()
                && entry.is_some()
                && record.stamp == update.before
                && current_stamp == update.after
        });
        if updates.contains_key(&normalized) && trusted_update.is_none() {
            continue;
        }
        if trusted_update.is_some_and(|update| {
            record.entry == entry
                && entry
                    .as_ref()
                    .is_some_and(|entry| update.removed.contains(entry))
        }) {
            continue;
        }
        if current_stamp != record.stamp && trusted_update.is_none() {
            return Err(
                "迁移后原文件大小或修改时间不一致，旧无损记录保留；包内重写需重新保存编辑记录"
                    .into(),
            );
        }
        record.source = source;
        record.entry = entry;
        if trusted_update.is_some() {
            record.stamp = current_stamp;
        }
        record.verify_source()?;
        let target_file = record_path(root, &record.source, record.entry.as_deref())?;
        let bytes = serde_json::to_vec_pretty(&record).map_err(|e| e.to_string())?;
        if bytes.len() > MAX_RECORD as usize {
            return Err("迁移后的记录超过1 MiB，旧记录保留".into());
        }
        staged_bytes = staged_bytes
            .checked_add(bytes.len() as u64 + old_file.metadata().map_err(|e| e.to_string())?.len())
            .filter(|n| *n <= 1024 * 1024 * 1024)
            .ok_or("记录迁移临时数据超过1 GiB，旧记录保留")?;
        let mut stage =
            fs::File::create(transaction.path().join(format!("new-{}.json", plans.len())))
                .map_err(|e| e.to_string())?;
        stage
            .write_all(&bytes)
            .and_then(|_| stage.sync_all())
            .map_err(|e| e.to_string())?;
        plans.push((
            item.file_name(),
            target_file
                .file_name()
                .ok_or("无损记录目标没有文件名")?
                .to_owned(),
        ));
    }
    let old_names: HashSet<_> = plans.iter().map(|(old, _)| old.clone()).collect();
    let mut targets = HashSet::new();
    for (_, target) in &plans {
        if !targets.insert(target.clone())
            || (directory.join(target).exists() && !old_names.contains(target))
        {
            return Err("目标无损记录已存在，所有旧记录保留".into());
        }
    }
    let (mut backed_up, mut published) = (0usize, 0usize);
    let result = (|| -> Result<(), String> {
        for (index, (old, _)) in plans.iter().enumerate() {
            fs::rename(
                directory.join(old),
                transaction.path().join(format!("old-{index}.json")),
            )
            .map_err(|e| e.to_string())?;
            backed_up += 1;
        }
        for (index, (_, target)) in plans.iter().enumerate() {
            let record: EditRecord = read_json(
                &transaction.path().join(format!("new-{index}.json")),
                MAX_RECORD,
            )?;
            record.verify_source()?;
            atomic_json_mode(&directory.join(target), &record, MAX_RECORD, false)?;
            published += 1;
        }
        Ok(())
    })();
    if let Err(error) = result {
        let mut restored = true;
        for (_, target) in plans.iter().take(published).rev() {
            if fs::remove_file(directory.join(target)).is_err() {
                restored = false;
            }
        }
        for (index, (old, _)) in plans.iter().take(backed_up).enumerate().rev() {
            if directory.join(old).exists()
                || fs::rename(
                    transaction.path().join(format!("old-{index}.json")),
                    directory.join(old),
                )
                .is_err()
            {
                restored = false;
            }
        }
        if !restored {
            let backup = transaction.keep();
            return Err(format!(
                "无损记录迁移失败：{error}；恢复副本保留在{}",
                backup.display()
            ));
        }
        return Err(format!("无损记录迁移失败，旧记录已恢复：{error}"));
    }
    Ok(())
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct Recipe {
    pub version: u32,
    pub name: String,
    pub edits: Vec<Edit>,
    pub export: ExportSettings,
    pub raw: RawSettings,
}
impl Default for Recipe {
    fn default() -> Self {
        Self {
            version: 1,
            name: "默认处理".into(),
            edits: Vec::new(),
            export: ExportSettings::default(),
            raw: RawSettings::default(),
        }
    }
}
impl Recipe {
    pub fn validate(&self) -> Result<(), String> {
        if self.version != 1
            || self.name.trim().is_empty()
            || self.name.len() > 1024
            || self.name.contains('\0')
            || self.edits.len() > 128
        {
            return Err("处理预设版本、名称或步骤数量无效".into());
        }
        self.raw.validate()?;
        if !(1..=100).contains(&self.export.quality) || !matches!(self.export.depth, 8 | 16) {
            return Err("预设中的输出质量或位深无效".into());
        }
        let data = serde_json::to_vec(self).map_err(|e| e.to_string())?;
        if data.len() > MAX_RECORD as usize {
            return Err("单个处理预设超过1 MiB".into());
        }
        // JSON cannot represent NaN/infinity; a round trip must preserve every required numeric field.
        serde_json::from_slice::<Recipe>(&data).map_err(|_| "预设包含无效数值")?;
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct SourceStamp {
    pub length: u64,
    pub modified_ns: u64,
}
impl SourceStamp {
    pub fn read(path: &Path) -> Result<Self, String> {
        let m = path.metadata().map_err(|e| e.to_string())?;
        if !m.is_file() {
            return Err("编辑记录的源必须是普通文件".into());
        }
        Ok(Self {
            length: m.len(),
            modified_ns: m
                .modified()
                .map_err(|e| e.to_string())?
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos()
                .try_into()
                .map_err(|_| "文件修改时间超出记录范围")?,
        })
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct EditRecord {
    pub version: u32,
    pub source: PathBuf,
    pub entry: Option<String>,
    pub stamp: SourceStamp,
    pub recipe: Recipe,
}
impl EditRecord {
    /// Archive entry identities never include an archive password.
    pub fn capture(source: &Path, entry: Option<String>, recipe: Recipe) -> Result<Self, String> {
        recipe.validate()?;
        if entry
            .as_ref()
            .is_some_and(|e| e.is_empty() || e.len() > 32768 || e.contains('\0'))
        {
            return Err("编辑记录中的包内路径无效".into());
        }
        let source = source.canonicalize().map_err(|e| e.to_string())?;
        let stamp = SourceStamp::read(&source)?;
        Ok(Self {
            version: 1,
            source,
            entry,
            stamp,
            recipe,
        })
    }
    pub fn verify_source(&self) -> Result<(), String> {
        if self.version != 1 {
            return Err("编辑记录版本不受支持".into());
        }
        self.recipe.validate()?;
        if self
            .entry
            .as_ref()
            .is_some_and(|e| e.is_empty() || e.len() > 32768 || e.contains('\0'))
        {
            return Err("编辑记录中的包内路径无效".into());
        }
        if SourceStamp::read(&self.source)? != self.stamp {
            return Err("原文件的大小或修改时间已改变，未自动套用旧编辑记录".into());
        }
        Ok(())
    }
    pub fn save(&self, path: &Path) -> Result<(), String> {
        self.verify_source()?;
        atomic_json(path, self, MAX_RECORD)
    }
    pub fn load(path: &Path) -> Result<Self, String> {
        let record: Self = read_json(path, MAX_RECORD)?;
        record.verify_source()?;
        Ok(record)
    }
}

#[derive(Default, Serialize, Deserialize)]
struct Presets {
    version: u32,
    recipes: Vec<Recipe>,
}

/// One bounded file; no recursive scans or unbounded preset history.
pub fn load_presets(path: &Path) -> Result<Vec<Recipe>, String> {
    if !path.exists() {
        return Ok(Vec::new());
    }
    let presets: Presets = read_json(path, 16 * MAX_RECORD)?;
    if presets.version != 1 || presets.recipes.len() > MAX_PRESETS {
        return Err("预设库版本无效或超过128个预设".into());
    }
    for recipe in &presets.recipes {
        recipe.validate()?;
    }
    Ok(presets.recipes)
}
pub fn save_preset(path: &Path, recipe: Recipe) -> Result<(), String> {
    recipe.validate()?;
    let mut recipes = load_presets(path)?;
    if let Some(found) = recipes.iter_mut().find(|r| r.name == recipe.name) {
        *found = recipe;
    } else {
        if recipes.len() == MAX_PRESETS {
            return Err("最多保存128个处理预设".into());
        }
        recipes.push(recipe);
    }
    atomic_json(
        path,
        &Presets {
            version: 1,
            recipes,
        },
        16 * MAX_RECORD,
    )
}
pub fn remove_preset(path: &Path, name: &str) -> Result<(), String> {
    let mut recipes = load_presets(path)?;
    recipes.retain(|r| r.name != name);
    atomic_json(
        path,
        &Presets {
            version: 1,
            recipes,
        },
        16 * MAX_RECORD,
    )
}

fn read_json<T: serde::de::DeserializeOwned>(path: &Path, max: u64) -> Result<T, String> {
    let file = fs::File::open(path).map_err(|e| e.to_string())?;
    if file.metadata().map_err(|e| e.to_string())?.len() > max {
        return Err("处理记录超过读取上限".into());
    }
    let mut bytes = Vec::new();
    file.take(max + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    if bytes.len() > max as usize {
        return Err("处理记录超过读取上限".into());
    }
    serde_json::from_slice(&bytes).map_err(|_| "处理记录JSON无效或字段不完整".into())
}
fn atomic_json(path: &Path, value: &impl Serialize, max: u64) -> Result<(), String> {
    atomic_json_mode(path, value, max, true)
}
fn atomic_json_mode(
    path: &Path,
    value: &impl Serialize,
    max: u64,
    replace: bool,
) -> Result<(), String> {
    let bytes = serde_json::to_vec_pretty(value).map_err(|e| e.to_string())?;
    if bytes.len() > max as usize {
        return Err("处理记录超过写入上限".into());
    }
    let parent = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    let mut stage = tempfile::Builder::new()
        .prefix(".rustview-recipe-")
        .tempfile_in(parent)
        .map_err(|e| e.to_string())?;
    stage
        .write_all(&bytes)
        .and_then(|_| stage.as_file().sync_all())
        .map_err(|e| e.to_string())?;
    if replace {
        stage.persist(path).map_err(|e| e.to_string())?;
    } else {
        stage
            .persist_noclobber(path)
            .map_err(|e| format!("目标无损记录已存在或无法发布，旧记录保留：{e}"))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn record_preserves_original_and_rejects_changed_source() {
        let d = tempfile::tempdir().unwrap();
        let source = d.path().join("photo.raw");
        fs::write(&source, [1, 2, 3]).unwrap();
        let path = d.path().join("photo.edits.json");
        let mut recipe = Recipe::default();
        recipe.edits.push(Edit::Exposure { ev: 1.25 });
        let record = EditRecord::capture(&source, Some("folder/photo.dng".into()), recipe).unwrap();
        record.save(&path).unwrap();
        let loaded = EditRecord::load(&path).unwrap();
        assert_eq!(loaded.recipe.edits, record.recipe.edits);
        assert_eq!(fs::read(&source).unwrap(), [1, 2, 3]);
        assert_eq!(loaded.entry.as_deref(), Some("folder/photo.dng"));
        fs::write(source, [1, 2, 3, 4]).unwrap();
        assert!(EditRecord::load(&path).is_err());
    }
    #[test]
    fn presets_replace_by_name_and_validate_before_writing() {
        let d = tempfile::tempdir().unwrap();
        let path = d.path().join("presets.json");
        let mut r = Recipe::default();
        save_preset(&path, r.clone()).unwrap();
        r.edits.push(Edit::Blur { sigma: 1.5 });
        save_preset(&path, r.clone()).unwrap();
        assert_eq!(load_presets(&path).unwrap().len(), 1);
        assert_eq!(load_presets(&path).unwrap()[0].edits, r.edits);
        let before = fs::read(&path).unwrap();
        r.raw.exposure_ev = f32::NAN;
        assert!(save_preset(&path, r).is_err());
        assert_eq!(fs::read(&path).unwrap(), before);
        remove_preset(&path, "默认处理").unwrap();
        assert!(load_presets(&path).unwrap().is_empty());
    }
    #[test]
    fn stable_record_location_checks_full_identity_and_archive_entry() {
        let d = tempfile::tempdir().unwrap();
        let source = d.path().join("images.zip");
        fs::write(&source, [1, 2, 3]).unwrap();
        let record =
            EditRecord::capture(&source, Some("one/a.png".into()), Recipe::default()).unwrap();
        let path = record_path(d.path(), &source, Some("one/a.png")).unwrap();
        record.save(&path).unwrap();
        assert!(read_for_in(d.path(), &source, Some("one/a.png")).is_some());
        assert!(read_for_in(d.path(), &source, Some("two/a.png")).is_none());
        let collision = record_path(d.path(), &source, Some("two/a.png")).unwrap();
        fs::copy(&path, collision).unwrap();
        assert!(read_for_in(d.path(), &source, Some("two/a.png")).is_none());
        fs::write(&source, [1, 2, 3, 4]).unwrap();
        assert!(read_for_in(d.path(), &source, Some("one/a.png")).is_none());
    }
    #[test]
    fn records_follow_file_directory_and_container_moves_after_old_paths_disappear() {
        let d = tempfile::tempdir().unwrap();
        let root = d.path().join("data");
        let old = d.path().join("old");
        let renamed = d.path().join("new");
        fs::create_dir(&old).unwrap();
        let photo = old.join("a.raw");
        let archive = old.join("images.zip");
        fs::write(&photo, [1, 2, 3]).unwrap();
        fs::write(&archive, [4, 5]).unwrap();
        for (source, entry) in [(&photo, None), (&archive, Some("folder/image.png"))] {
            let record =
                EditRecord::capture(source, entry.map(str::to_owned), Recipe::default()).unwrap();
            record
                .save(&record_path(&root, source, entry).unwrap())
                .unwrap();
        }
        let old_identity = old.canonicalize().unwrap();
        fs::rename(&old, &renamed).unwrap();
        let key = |path: &Path| serde_json::to_string(&(path, None::<String>)).unwrap();
        relocate_keys(&root, &[(key(&old_identity), key(&renamed))]).unwrap();
        let photo = renamed.join("a.raw");
        let archive = renamed.join("images.zip");
        assert!(read_for_in(&root, &photo, None).is_some());
        assert!(read_for_in(&root, &archive, Some("folder/image.png")).is_some());
        let old_identity = photo.canonicalize().unwrap();
        let target = renamed.join("b.raw");
        fs::rename(&photo, &target).unwrap();
        relocate_keys(&root, &[(key(&old_identity), key(&target))]).unwrap();
        assert!(read_for_in(&root, &target, None).is_some());
        assert_eq!(fs::read_dir(root.join("edits")).unwrap().count(), 2);
    }
    #[test]
    fn trusted_entry_rename_refreshes_container_stamp_but_unknown_changes_do_not() {
        let d = tempfile::tempdir().unwrap();
        let source = d.path().join("images.zip");
        fs::write(&source, [1, 2, 3]).unwrap();
        let record = EditRecord::capture(&source, Some("a.png".into()), Recipe::default()).unwrap();
        record
            .save(&record_path(d.path(), &source, Some("a.png")).unwrap())
            .unwrap();
        fs::write(&source, [1, 2, 3, 4]).unwrap();
        assert!(read_for_in(d.path(), &source, Some("a.png")).is_none());
        let key =
            |entry: &str| serde_json::to_string(&(record.source.clone(), Some(entry))).unwrap();
        relocate_keys_with_updates(
            d.path(),
            &[(key("a.png"), key("b.png"))],
            &[ContainerUpdate {
                path: source.clone(),
                before: record.stamp,
                after: SourceStamp::read(&source).unwrap(),
                removed: Vec::new(),
            }],
        )
        .unwrap();
        assert!(read_for_in(d.path(), &source, Some("b.png")).is_some());
        assert!(read_for_in(d.path(), &source, Some("a.png")).is_none());
    }
    #[test]
    fn record_exchange_keeps_each_recipe_with_its_original_content() {
        let d = tempfile::tempdir().unwrap();
        let a = d.path().join("a.raw");
        let b = d.path().join("b.raw");
        fs::write(&a, [1]).unwrap();
        fs::write(&b, [2, 3]).unwrap();
        for (path, name) in [(&a, "A"), (&b, "B")] {
            let mut recipe = Recipe::default();
            recipe.name = name.into();
            let record = EditRecord::capture(path, None, recipe).unwrap();
            record
                .save(&record_path(d.path(), path, None).unwrap())
                .unwrap();
        }
        let key = |path: &Path| serde_json::to_string(&(path, None::<String>)).unwrap();
        let old_a = a.canonicalize().unwrap();
        let old_b = b.canonicalize().unwrap();
        let temp = d.path().join("swap");
        fs::rename(&a, &temp).unwrap();
        fs::rename(&b, &a).unwrap();
        fs::rename(temp, &b).unwrap();
        relocate_keys(d.path(), &[(key(&old_a), key(&b)), (key(&old_b), key(&a))]).unwrap();
        assert_eq!(read_for_in(d.path(), &a, None).unwrap().recipe.name, "B");
        assert_eq!(read_for_in(d.path(), &b, None).unwrap().recipe.name, "A");
    }
    #[test]
    fn trusted_container_refreshes_siblings_but_never_resurrects_deleted_or_stale_entry() {
        let d = tempfile::tempdir().unwrap();
        let source = d.path().join("images.zip");
        fs::write(&source, [1, 2, 3]).unwrap();
        for entry in ["a.png", "b.png", "deleted.png"] {
            let record =
                EditRecord::capture(&source, Some(entry.into()), Recipe::default()).unwrap();
            record
                .save(&record_path(d.path(), &source, Some(entry)).unwrap())
                .unwrap();
        }
        let before = SourceStamp::read(&source).unwrap();
        fs::write(&source, [1, 2, 3, 4]).unwrap();
        let after = SourceStamp::read(&source).unwrap();
        relocate_keys_with_updates(
            d.path(),
            &[],
            &[ContainerUpdate {
                path: source.clone(),
                before,
                after,
                removed: vec!["deleted.png".into()],
            }],
        )
        .unwrap();
        assert!(read_for_in(d.path(), &source, Some("b.png")).is_some());
        assert!(read_for_in(d.path(), &source, Some("deleted.png")).is_none());
        fs::write(&source, [1, 2, 3, 4, 5]).unwrap();
        let newest = SourceStamp::read(&source).unwrap();
        relocate_keys_with_updates(
            d.path(),
            &[],
            &[ContainerUpdate {
                path: source.clone(),
                before: after,
                after: newest,
                removed: Vec::new(),
            }],
        )
        .unwrap();
        assert!(read_for_in(d.path(), &source, Some("a.png")).is_some());
        assert!(read_for_in(d.path(), &source, Some("deleted.png")).is_none());
    }
}

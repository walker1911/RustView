use serde::{Deserialize, Serialize};
use std::{
    collections::{HashMap, HashSet},
    fs::{self, File},
    io::{Read, Write},
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::atomic::{AtomicBool, Ordering},
    thread,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

#[derive(Clone, Serialize, Deserialize)]
pub enum Source {
    Local(PathBuf),
    Archive {
        path: PathBuf,
        entry: String,
        password: Option<String>,
    },
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct BatchResult {
    pub completed: Vec<PathBuf>,
    pub failures: Vec<(String, String)>,
    #[serde(default)]
    pub backups: Vec<PathBuf>,
    #[serde(skip)]
    pub containers: Vec<crate::recipes::ContainerUpdate>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum ArchiveEdit {
    Delete(Vec<String>),
    Rename { entry: String, new_name: String },
    RenameMany(Vec<(String, String)>),
    Add(Vec<PathBuf>),
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ArchiveResult {
    pub backup: PathBuf,
    pub renamed_entry: Option<String>,
    #[serde(skip)]
    pub container_update: Option<crate::recipes::ContainerUpdate>,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct RenameTemplate {
    pub pattern: String,
    pub start: u64,
    pub step: u64,
    pub padding: usize,
}
impl Default for RenameTemplate {
    fn default() -> Self {
        Self {
            pattern: "{name}_{n}{ext}".into(),
            start: 1,
            step: 1,
            padding: 3,
        }
    }
}

#[derive(Clone, Serialize, Deserialize)]
pub struct RenameItem {
    pub source: Source,
    pub new_name: String,
    expected_size: u64,
    expected_modified: Option<SystemTime>,
}

pub struct RenameResult {
    pub moved: Vec<(Source, Source)>,
    pub backups: Vec<PathBuf>,
    pub containers: Vec<crate::recipes::ContainerUpdate>,
}

const MAX_TREE_ENTRIES: usize = 100_000;
const MAX_TREE_DEPTH: usize = 256;
pub const CLIPBOARD_CACHE_BYTES: u64 = 2 * 1024 * 1024 * 1024;

pub struct ClipboardExport {
    pub directory: PathBuf,
    pub paths: Vec<PathBuf>,
}
#[derive(Default)]
pub struct CacheCleanup {
    pub removed: usize,
    pub bytes: u64,
    pub retained_bytes: u64,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Times {
    pub modified: Option<i64>,
    pub accessed: Option<i64>,
    pub created: Option<i64>,
}

#[cfg(windows)]
pub fn parse_local_datetime(value: &str) -> Result<i64, String> {
    use windows_sys::Win32::{
        Foundation::{FILETIME, SYSTEMTIME},
        System::Time::{SystemTimeToFileTime, TzSpecificLocalTimeToSystemTimeEx},
    };
    let value = value.trim();
    let bytes = value.as_bytes();
    if bytes.len() != 19
        || bytes[4] != b'-'
        || bytes[7] != b'-'
        || bytes[10] != b' '
        || bytes[13] != b':'
        || bytes[16] != b':'
        || bytes
            .iter()
            .enumerate()
            .any(|(index, byte)| !matches!(index, 4 | 7 | 10 | 13 | 16) && !byte.is_ascii_digit())
    {
        return Err("请输入 YYYY-MM-DD HH:MM:SS 格式的本地日期时间".into());
    }
    let number = |range: std::ops::Range<usize>| {
        value[range]
            .parse::<u16>()
            .map_err(|_| "日期数字无效".to_owned())
    };
    let local = SYSTEMTIME {
        wYear: number(0..4)?,
        wMonth: number(5..7)?,
        wDay: number(8..10)?,
        wHour: number(11..13)?,
        wMinute: number(14..16)?,
        wSecond: number(17..19)?,
        wDayOfWeek: 0,
        wMilliseconds: 0,
    };
    let mut utc: SYSTEMTIME = unsafe { std::mem::zeroed() };
    let mut filetime: FILETIME = unsafe { std::mem::zeroed() };
    // The initial conversion validates the calendar date; the second uses the current Windows time zone.
    if unsafe { SystemTimeToFileTime(&local, &mut filetime) } == 0
        || unsafe { TzSpecificLocalTimeToSystemTimeEx(std::ptr::null(), &local, &mut utc) } == 0
        || unsafe { SystemTimeToFileTime(&utc, &mut filetime) } == 0
    {
        return Err("日期无效或超出系统支持范围".into());
    }
    let ticks = ((filetime.dwHighDateTime as u64) << 32) | filetime.dwLowDateTime as u64;
    let seconds = (ticks as i128 - 116_444_736_000_000_000_i128) / 10_000_000;
    let seconds = i64::try_from(seconds).map_err(|_| "日期超出支持范围")?;
    let time = if seconds >= 0 {
        UNIX_EPOCH.checked_add(Duration::from_secs(seconds as u64))
    } else {
        UNIX_EPOCH.checked_sub(Duration::from_secs(seconds.unsigned_abs()))
    }
    .ok_or("日期超出支持范围")?;
    if format_local_datetime(time) != value {
        return Err("该本地时间无效，可能处于夏令时切换区间".into());
    }
    Ok(seconds)
}

#[cfg(not(windows))]
pub fn parse_local_datetime(_value: &str) -> Result<i64, String> {
    Err("本地日期转换仅支持Windows".into())
}

#[cfg(windows)]
pub fn format_local_datetime(value: SystemTime) -> String {
    use windows_sys::Win32::{
        Foundation::{FILETIME, SYSTEMTIME},
        System::Time::{FileTimeToSystemTime, SystemTimeToTzSpecificLocalTimeEx},
    };
    let nanoseconds = match value.duration_since(UNIX_EPOCH) {
        Ok(duration) => duration.as_nanos() as i128,
        Err(error) => -(error.duration().as_nanos() as i128),
    };
    let ticks = 116_444_736_000_000_000_i128 + nanoseconds / 100;
    let Ok(ticks) = u64::try_from(ticks) else {
        return "日期超出支持范围".into();
    };
    let filetime = FILETIME {
        dwLowDateTime: ticks as u32,
        dwHighDateTime: (ticks >> 32) as u32,
    };
    let mut utc: SYSTEMTIME = unsafe { std::mem::zeroed() };
    let mut local: SYSTEMTIME = unsafe { std::mem::zeroed() };
    if unsafe { FileTimeToSystemTime(&filetime, &mut utc) } == 0
        || unsafe { SystemTimeToTzSpecificLocalTimeEx(std::ptr::null(), &utc, &mut local) } == 0
    {
        return "日期超出支持范围".into();
    }
    format!(
        "{:04}-{:02}-{:02} {:02}:{:02}:{:02}",
        local.wYear, local.wMonth, local.wDay, local.wHour, local.wMinute, local.wSecond
    )
}

#[cfg(not(windows))]
pub fn format_local_datetime(_value: SystemTime) -> String {
    "本地日期转换仅支持Windows".into()
}

fn valid_name(name: &str) -> Result<(), String> {
    let stem = name.split('.').next().unwrap_or("").to_ascii_uppercase();
    let reserved = matches!(stem.as_str(), "CON" | "PRN" | "AUX" | "NUL")
        || stem
            .strip_prefix("COM")
            .or_else(|| stem.strip_prefix("LPT"))
            .is_some_and(|n| n.len() == 1 && matches!(n.as_bytes()[0], b'1'..=b'9'));
    if name.is_empty()
        || name.encode_utf16().count() > 255
        || name == "."
        || name == ".."
        || name.ends_with([' ', '.'])
        || reserved
        || name
            .chars()
            .any(|c| c.is_control() || "<>:\"/\\|?*".contains(c))
    {
        return Err("文件名为空或包含 Windows 不支持的字符".into());
    }
    Ok(())
}

fn source_name(source: &Source) -> Result<String, String> {
    let name = match source {
        Source::Local(path) => path.file_name().and_then(|name| name.to_str()),
        Source::Archive { entry, .. } => entry.rsplit(['/', '\\']).next(),
    }
    .ok_or("无法取得源文件名")?;
    valid_name(name)?;
    Ok(name.to_owned())
}

fn describe(source: &Source) -> String {
    match source {
        Source::Local(path) => path.display().to_string(),
        Source::Archive { path, entry, .. } => format!("{} :: {entry}", path.display()),
    }
}

fn same_file(a: &Path, b: &Path) -> bool {
    matches!((a.canonicalize(), b.canonicalize()), (Ok(a), Ok(b)) if a == b)
}

fn regular_file(path: &Path) -> Result<(), String> {
    let metadata = fs::symlink_metadata(path).map_err(|error| error.to_string())?;
    if reparse(&metadata) {
        return Err("不跟随符号链接或重解析点".into());
    }
    if !metadata.is_file() {
        return Err("此操作需要普通文件".into());
    }
    Ok(())
}

fn reparse(metadata: &fs::Metadata) -> bool {
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        if metadata.file_attributes() & 0x400 != 0 {
            return true;
        }
    }
    metadata.file_type().is_symlink()
}

fn operation_path(path: &Path) -> Result<PathBuf, String> {
    let metadata = fs::symlink_metadata(path).map_err(|e| e.to_string())?;
    if reparse(&metadata) {
        return Err("不跟随符号链接或重解析点".into());
    }
    let path = path.canonicalize().map_err(|e| e.to_string())?;
    if path.parent().is_none() || path.file_name().is_none() {
        return Err("不能对磁盘或共享根目录执行此操作".into());
    }
    Ok(path)
}

fn rename_no_replace(source: &Path, destination: &Path) -> Result<(), String> {
    #[cfg(windows)]
    {
        use std::os::windows::ffi::OsStrExt;
        let wide = |p: &Path| {
            p.as_os_str()
                .encode_wide()
                .chain(Some(0))
                .collect::<Vec<_>>()
        };
        let (source, destination) = (wide(source), wide(destination));
        if unsafe {
            windows_sys::Win32::Storage::FileSystem::MoveFileExW(
                source.as_ptr(),
                destination.as_ptr(),
                0,
            )
        } == 0
        {
            return Err(std::io::Error::last_os_error().to_string());
        }
        Ok(())
    }
    #[cfg(not(windows))]
    {
        if destination.exists() {
            return Err("目标已存在，未覆盖".into());
        }
        fs::rename(source, destination).map_err(|e| e.to_string())
    }
}

fn cancelled(cancel: &AtomicBool) -> Result<(), String> {
    if cancel.load(Ordering::Relaxed) {
        Err("已取消，未提交当前项目".into())
    } else {
        Ok(())
    }
}

fn inspect_tree(root: &Path, cancel: &AtomicBool) -> Result<(), String> {
    let root = operation_path(root)?;
    if !root.is_dir() {
        return Ok(());
    }
    let mut stack = vec![(fs::read_dir(&root).map_err(|e| e.to_string())?, 0usize)];
    let mut count = 1;
    while let Some((reader, depth)) = stack.last_mut() {
        cancelled(cancel)?;
        let depth = *depth;
        let Some(entry) = reader.next() else {
            stack.pop();
            continue;
        };
        let entry = entry.map_err(|e| e.to_string())?;
        count += 1;
        if count > MAX_TREE_ENTRIES {
            return Err("文件夹超过十万项上限".into());
        }
        let metadata = fs::symlink_metadata(entry.path()).map_err(|e| e.to_string())?;
        if reparse(&metadata) {
            return Err(format!(
                "文件夹含符号链接或重解析点，未跟随：{}",
                entry.path().display()
            ));
        }
        if metadata.is_dir() {
            if depth + 1 > MAX_TREE_DEPTH {
                return Err("文件夹超过256层深度上限".into());
            }
            stack.push((
                fs::read_dir(entry.path()).map_err(|e| e.to_string())?,
                depth + 1,
            ));
        } else if !metadata.is_file() {
            return Err("文件夹包含非普通文件".into());
        }
    }
    Ok(())
}

/// A directory is copied to staging, then published as one directory. Cancellation
/// never exposes a partial destination and never changes the source tree.
pub fn copy_local_tree(
    source: &Path,
    destination: &Path,
    max_file_bytes: u64,
    cancel: &AtomicBool,
) -> Result<(), String> {
    let source = operation_path(source)?;
    if !source.is_dir() {
        return Err("源路径不是文件夹".into());
    }
    let parent = destination
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."))
        .canonicalize()
        .map_err(|e| e.to_string())?;
    let name = destination
        .file_name()
        .and_then(|n| n.to_str())
        .ok_or("目标名称无效")?;
    valid_name(name)?;
    let destination = parent.join(name);
    if destination.starts_with(&source) || same_file(&source, &destination) {
        return Err("不能把文件夹复制或移动到自身及其子目录".into());
    }
    if destination.exists() {
        return Err("目标文件夹已存在，未合并或覆盖；请选择空目标位置".into());
    }
    let staging = tempfile::Builder::new()
        .prefix(".rustview-tree-")
        .tempdir_in(&parent)
        .map_err(|e| e.to_string())?;
    let payload = staging.path().join("payload");
    fs::create_dir(&payload).map_err(|e| e.to_string())?;
    let mut stack = vec![(
        fs::read_dir(&source).map_err(|e| e.to_string())?,
        payload.clone(),
        0usize,
    )];
    let mut count = 1;
    while let Some((reader, to, depth)) = stack.last_mut() {
        cancelled(cancel)?;
        let depth = *depth;
        let Some(entry) = reader.next() else {
            stack.pop();
            continue;
        };
        let entry = entry.map_err(|e| e.to_string())?;
        count += 1;
        if count > MAX_TREE_ENTRIES {
            return Err("文件夹超过十万项上限".into());
        }
        let metadata = fs::symlink_metadata(entry.path()).map_err(|e| e.to_string())?;
        if reparse(&metadata) {
            return Err(format!(
                "不跟随符号链接或重解析点：{}",
                entry.path().display()
            ));
        }
        let target = to.join(entry.file_name());
        if metadata.is_dir() {
            if depth + 1 > MAX_TREE_DEPTH {
                return Err("文件夹超过256层深度上限".into());
            }
            fs::create_dir(&target).map_err(|e| e.to_string())?;
            stack.push((
                fs::read_dir(entry.path()).map_err(|e| e.to_string())?,
                target,
                depth + 1,
            ));
        } else if metadata.is_file() {
            copy_regular_cancellable(
                &entry.path(),
                &target,
                max_file_bytes,
                cancel,
                Some(&source),
            )?;
        } else {
            return Err("文件夹包含非普通文件".into());
        }
    }
    cancelled(cancel)?;
    rename_no_replace(&payload, &destination)
}

fn copy_regular_cancellable(
    source: &Path,
    destination: &Path,
    max_bytes: u64,
    cancel: &AtomicBool,
    root: Option<&Path>,
) -> Result<(), String> {
    use std::io::ErrorKind;
    let mut options = fs::OpenOptions::new();
    options.read(true);
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        options.custom_flags(0x0020_0000).share_mode(1);
    }
    let mut input = options.open(source).map_err(|e| e.to_string())?;
    let metadata = input.metadata().map_err(|e| e.to_string())?;
    if !metadata.is_file() || reparse(&metadata) {
        return Err("源文件不是普通文件或是重解析点".into());
    }
    if metadata.len() > max_bytes {
        return Err("源文件超过读取上限".into());
    }
    if let Some(root) = root {
        let actual = source.canonicalize().map_err(|e| e.to_string())?;
        if !actual.starts_with(root) {
            return Err("源文件已移出当前文件夹，未跟随".into());
        }
    }
    let mut output = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(destination)
        .map_err(|e| e.to_string())?;
    let mut buffer = [0u8; 64 * 1024];
    let mut bytes = 0u64;
    loop {
        cancelled(cancel)?;
        let count = match input.read(&mut buffer) {
            Err(e) if e.kind() == ErrorKind::Interrupted => continue,
            other => other.map_err(|e| e.to_string())?,
        };
        if count == 0 {
            break;
        }
        bytes = bytes
            .checked_add(count as u64)
            .filter(|b| *b <= max_bytes)
            .ok_or("源文件读取超过上限")?;
        output
            .write_all(&buffer[..count])
            .map_err(|e| e.to_string())?;
    }
    let after = input.metadata().map_err(|e| e.to_string())?;
    if after.len() != metadata.len() || after.modified().ok() != metadata.modified().ok() {
        return Err("源文件在复制期间发生变化，未提交目标".into());
    }
    output.sync_all().map_err(|e| e.to_string())?;
    if let Ok(time) = metadata.modified() {
        output
            .set_times(fs::FileTimes::new().set_modified(time))
            .map_err(|e| e.to_string())?;
    }
    Ok(())
}

/// Copy original file bytes (or one original archive entry), never the rendered pixels.
pub fn copy_to(
    source: &Source,
    destination: &Path,
    overwrite: bool,
    max_bytes: u64,
) -> Result<(), String> {
    copy_to_cancellable(
        source,
        destination,
        overwrite,
        max_bytes,
        &AtomicBool::new(false),
    )
}

pub fn copy_to_cancellable(
    source: &Source,
    destination: &Path,
    overwrite: bool,
    max_bytes: u64,
    cancel: &AtomicBool,
) -> Result<(), String> {
    cancelled(cancel)?;
    if let Source::Local(path) = source {
        if fs::symlink_metadata(path)
            .map_err(|e| e.to_string())?
            .is_dir()
        {
            return copy_local_tree(path, destination, max_bytes, cancel);
        }
    }
    let parent = destination
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."))
        .canonicalize()
        .map_err(|e| e.to_string())?;
    let name = destination
        .file_name()
        .and_then(|n| n.to_str())
        .ok_or("目标名称无效")?;
    valid_name(name)?;
    let destination = parent.join(name);
    let original = match source {
        Source::Local(path) | Source::Archive { path, .. } => path,
    };
    if same_file(original, &destination) {
        return Err("不能覆盖源文件或源压缩包".into());
    }
    if destination.exists() && !overwrite {
        return Err("目标文件已存在，需要先确认覆盖".into());
    }
    if let Ok(metadata) = fs::symlink_metadata(&destination) {
        if !metadata.is_file() || reparse(&metadata) {
            return Err("目标不是普通文件，未覆盖".into());
        }
    }
    let staging = tempfile::Builder::new()
        .prefix(".rustview-copy-")
        .tempdir_in(&parent)
        .map_err(|e| e.to_string())?;
    let payload = staging.path().join("payload");
    match source {
        Source::Local(path) => copy_regular_cancellable(path, &payload, max_bytes, cancel, None)?,
        Source::Archive {
            path,
            entry,
            password,
        } => crate::archive::extract(path, entry, password.as_deref(), &payload, max_bytes)
            .map_err(|e| e.to_string())?,
    }
    cancelled(cancel)?;
    if overwrite {
        fs::rename(&payload, &destination).map_err(|e| e.to_string())
    } else {
        rename_no_replace(&payload, &destination)
    }
}

pub fn copy_into(
    sources: &[Source],
    directory: &Path,
    overwrite: bool,
    max_bytes: u64,
) -> BatchResult {
    copy_into_cancellable(
        sources,
        directory,
        overwrite,
        max_bytes,
        &AtomicBool::new(false),
    )
}

pub fn copy_into_cancellable(
    sources: &[Source],
    directory: &Path,
    overwrite: bool,
    max_bytes: u64,
    cancel: &AtomicBool,
) -> BatchResult {
    let mut result = BatchResult::default();
    let mut targets = HashSet::new();
    for source in sources {
        if cancel.load(Ordering::Relaxed) {
            result
                .failures
                .push((describe(source), "已取消，后续项目未处理".into()));
            break;
        }
        let target = source_name(source).and_then(|name| {
            let target = directory.join(name);
            if !targets.insert(target.to_string_lossy().to_lowercase()) {
                return Err("所选条目含多个同名目标，请分别复制或先重命名".into());
            }
            Ok(target)
        });
        match target.and_then(|target| {
            copy_to_cancellable(source, &target, overwrite, max_bytes, cancel).map(|()| target)
        }) {
            Ok(path) => result.completed.push(path),
            Err(error) => result.failures.push((describe(source), error)),
        }
    }
    result
}

pub fn rename_local(path: &Path, new_name: &str) -> Result<PathBuf, String> {
    let path = operation_path(path)?;
    valid_name(new_name)?;
    let destination = path.with_file_name(new_name);
    if path.file_name().and_then(|s| s.to_str()) == Some(new_name) {
        return Ok(path);
    }
    if destination.exists() && !same_file(&path, &destination) {
        return Err("目标已存在，未覆盖".into());
    }
    let metadata = fs::metadata(&path).map_err(|e| e.to_string())?;
    rename_many(&[RenameItem {
        source: Source::Local(path),
        new_name: new_name.to_owned(),
        expected_size: metadata.len(),
        expected_modified: metadata.modified().ok(),
    }])?;
    Ok(destination)
}

fn associated_sidecar(path: &Path) -> Result<Option<PathBuf>, String> {
    if !path
        .file_name()
        .and_then(|s| s.to_str())
        .is_some_and(crate::imaging::is_supported_image)
        || !path.is_file()
    {
        return Ok(None);
    }
    let sidecar = path.with_extension("xmp");
    match fs::symlink_metadata(&sidecar) {
        Ok(metadata) => {
            if reparse(&metadata) || !metadata.is_file() {
                return Err(format!("关联XMP不是普通文件：{}", sidecar.display()));
            }
            Ok(Some(operation_path(&sidecar)?))
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e.to_string()),
    }
}

fn expand_rename_sidecars(plan: &[RenameItem]) -> Result<Vec<RenameItem>, String> {
    let mut expanded = plan.to_vec();
    let mut associations: HashMap<PathBuf, PathBuf> = HashMap::new();
    let explicit = plan
        .iter()
        .filter_map(|item| match &item.source {
            Source::Local(path) => Some((
                path.canonicalize()
                    .unwrap_or_else(|_| path.clone())
                    .to_string_lossy()
                    .to_lowercase(),
                item,
            )),
            _ => None,
        })
        .collect::<HashMap<_, _>>();
    for item in plan {
        let Source::Local(path) = &item.source else {
            continue;
        };
        let path = operation_path(path)?;
        let Some(sidecar) = associated_sidecar(&path)? else {
            continue;
        };
        let target = path.with_file_name(&item.new_name).with_extension("xmp");
        if let Some(previous) = associations.insert(sidecar.clone(), target.clone()) {
            if previous != target {
                return Err(
                    "所选原片共用一个XMP，却会改为不同名称；请分批处理或保持相同的新文件名主体"
                        .into(),
                );
            }
            continue;
        }
        if let Some(existing) = explicit.get(&sidecar.to_string_lossy().to_lowercase()) {
            if sidecar.with_file_name(&existing.new_name) != target {
                return Err("关联XMP与显式重命名计划冲突".into());
            }
            continue;
        }
        let metadata = fs::metadata(&sidecar).map_err(|e| e.to_string())?;
        let name = target
            .file_name()
            .and_then(|s| s.to_str())
            .ok_or("XMP目标文件名无效")?
            .to_owned();
        expanded.push(RenameItem {
            source: Source::Local(sidecar),
            new_name: name,
            expected_size: metadata.len(),
            expected_modified: metadata.modified().ok(),
        });
    }
    if expanded.len() > MAX_TREE_ENTRIES {
        return Err("含关联XMP的计划超过十万项".into());
    }
    Ok(expanded)
}

/// A photo and its XMP are preflighted together. Cross-volume moves copy both
/// before deleting either original; rollback never overwrites another file.
fn move_photo_pair(
    source: &Path,
    destination: &Path,
    sidecar: &Path,
    cancel: &AtomicBool,
) -> Result<PathBuf, String> {
    let side_target = destination.with_extension("xmp");
    let pairs = [(source, destination), (sidecar, side_target.as_path())];
    for (_, target) in pairs {
        if target.exists() {
            return Err(format!(
                "原片或关联XMP目标已存在，未覆盖：{}",
                target.display()
            ));
        }
    }
    let mut moved = 0;
    let mut direct = true;
    for (old, new) in pairs {
        if cancelled(cancel).is_err() || rename_no_replace(old, new).is_err() {
            direct = false;
            break;
        }
        moved += 1;
    }
    if direct {
        return Ok(destination.to_owned());
    }
    for (old, new) in pairs[..moved].iter().rev() {
        rename_no_replace(new, old)
            .map_err(|e| format!("移动未完成，原件保留在{}，回滚失败：{e}", new.display()))?;
    }
    cancelled(cancel)?;
    let before = pairs
        .iter()
        .map(|(old, _)| source_signature(old, cancel))
        .collect::<Result<Vec<_>, _>>()?;
    let mut copied = 0;
    let operation = (|| -> Result<(), String> {
        for (old, new) in pairs {
            copy_to_cancellable(&Source::Local(old.to_owned()), new, false, u64::MAX, cancel)?;
            copied += 1;
        }
        cancelled(cancel)?;
        for ((old, _), signature) in pairs.iter().zip(before) {
            if source_signature(old, cancel)? != signature {
                return Err("源原片或XMP在复制时变化，原件保留".into());
            }
        }
        Ok(())
    })();
    if let Err(error) = operation {
        let mut cleanup = Vec::new();
        for (_, new) in pairs[..copied].iter().rev() {
            if let Err(e) = fs::remove_file(new) {
                cleanup.push(format!("{}：{e}", new.display()));
            }
        }
        return Err(if cleanup.is_empty() {
            error
        } else {
            format!("{error}；部分副本保留：{}", cleanup.join("；"))
        });
    }
    // Keep both destination copies if source deletion is denied. No original
    // content is lost, and the caller receives the precise retained location.
    for (old, _) in pairs.iter().rev() {
        if let Err(e) = remove_local(old) {
            return Err(format!(
                "原片与XMP已复制到{}；源项目{}未删除：{e}",
                destination.parent().unwrap_or(destination).display(),
                old.display()
            ));
        }
    }
    Ok(destination.to_owned())
}

pub fn paste_local(paths: &[PathBuf], directory: &Path, cut: bool, overwrite: bool) -> BatchResult {
    paste_local_cancellable(paths, directory, cut, overwrite, &AtomicBool::new(false))
}

pub fn paste_local_cancellable(
    paths: &[PathBuf],
    directory: &Path,
    cut: bool,
    overwrite: bool,
    cancel: &AtomicBool,
) -> BatchResult {
    let mut result = BatchResult::default();
    let mut targets = HashSet::new();
    for path in paths {
        if cancel.load(Ordering::Relaxed) {
            result
                .failures
                .push((path.display().to_string(), "已取消，后续项目未处理".into()));
            break;
        }
        let outcome = (|| {
            let source = operation_path(path)?;
            let directory = directory.canonicalize().map_err(|e| e.to_string())?;
            if !directory.is_dir() {
                return Err("目标不是文件夹".into());
            }
            let destination = directory.join(source_name(&Source::Local(source.clone()))?);
            if !targets.insert(destination.to_string_lossy().to_lowercase()) {
                return Err("所选项目含多个同名目标，未覆盖".into());
            }
            if same_file(&source, &destination) || destination.starts_with(&source) {
                return Err("不能把项目移动到自身及其子目录".into());
            }
            if destination.exists() && !overwrite {
                return Err("目标已存在，需要先确认覆盖".into());
            }
            if cut {
                if let Some(sidecar) = associated_sidecar(&source)? {
                    return move_photo_pair(&source, &destination, &sidecar, cancel);
                }
            }
            if cut && !destination.exists() && rename_no_replace(&source, &destination).is_ok() {
                return Ok(destination);
            }
            let before = source_signature(&source, cancel)?;
            copy_to_cancellable(
                &Source::Local(source.clone()),
                &destination,
                overwrite,
                u64::MAX,
                cancel,
            )?;
            if cut {
                if cancelled(cancel).is_err() {
                    return Err(format!(
                        "已复制到{}，移动已取消，源项目保留",
                        destination.display()
                    ));
                }
                if source_signature(&source, cancel)? != before {
                    return Err(format!(
                        "已复制到{}，但源项目变化，未删除原件",
                        destination.display()
                    ));
                }
                remove_local(&source).map_err(|e| {
                    format!("已复制到{}，但源项目未删除：{e}", destination.display())
                })?;
            }
            Ok(destination)
        })();
        match outcome {
            Ok(path) => result.completed.push(path),
            Err(e) => result.failures.push((path.display().to_string(), e)),
        }
    }
    result
}

fn source_signature(path: &Path, cancel: &AtomicBool) -> Result<(u64, u64, u128), String> {
    use std::hash::{Hash, Hasher};
    let root = operation_path(path)?;
    let metadata = fs::metadata(&root).map_err(|e| e.to_string())?;
    let mut count = 0u64;
    let mut bytes = 0u64;
    let mut digest = 0u128;
    let mut record = |path: &Path, metadata: &fs::Metadata| -> Result<(), String> {
        count += 1;
        if count > MAX_TREE_ENTRIES as u64 {
            return Err("文件夹超过十万项上限".into());
        }
        let mut hash = std::collections::hash_map::DefaultHasher::new();
        path.strip_prefix(&root).unwrap_or(path).hash(&mut hash);
        metadata.len().hash(&mut hash);
        metadata.modified().ok().hash(&mut hash);
        metadata.is_dir().hash(&mut hash);
        digest = digest.wrapping_add(hash.finish() as u128);
        if metadata.is_file() {
            bytes = bytes.checked_add(metadata.len()).ok_or("文件大小溢出")?;
        }
        Ok(())
    };
    record(&root, &metadata)?;
    if metadata.is_dir() {
        let mut stack = vec![(fs::read_dir(&root).map_err(|e| e.to_string())?, 0usize)];
        while let Some((reader, depth)) = stack.last_mut() {
            cancelled(cancel)?;
            let depth = *depth;
            let Some(entry) = reader.next() else {
                stack.pop();
                continue;
            };
            let entry = entry.map_err(|e| e.to_string())?;
            let metadata = fs::symlink_metadata(entry.path()).map_err(|e| e.to_string())?;
            if reparse(&metadata) {
                return Err("源文件夹含重解析点，未跟随".into());
            }
            record(&entry.path(), &metadata)?;
            if metadata.is_dir() {
                if depth + 1 > MAX_TREE_DEPTH {
                    return Err("文件夹超过256层深度上限".into());
                }
                stack.push((
                    fs::read_dir(entry.path()).map_err(|e| e.to_string())?,
                    depth + 1,
                ));
            }
        }
    }
    Ok((count, bytes, digest))
}

fn remove_local(path: &Path) -> Result<(), String> {
    let path = operation_path(path)?;
    if path.is_dir() {
        fs::remove_dir_all(path).map_err(|e| e.to_string())
    } else {
        fs::remove_file(path).map_err(|e| e.to_string())
    }
}

pub fn move_into(
    sources: &[Source],
    directory: &Path,
    overwrite: bool,
    max_bytes: u64,
) -> BatchResult {
    move_into_cancellable(
        sources,
        directory,
        overwrite,
        max_bytes,
        &AtomicBool::new(false),
    )
}

pub fn move_into_cancellable(
    sources: &[Source],
    directory: &Path,
    overwrite: bool,
    max_bytes: u64,
    cancel: &AtomicBool,
) -> BatchResult {
    let mut result = BatchResult::default();
    let mut targets = HashSet::new();
    let mut archives: HashMap<(PathBuf, Option<String>), Vec<(String, PathBuf)>> = HashMap::new();
    for source in sources {
        if cancel.load(Ordering::Relaxed) {
            result
                .failures
                .push((describe(source), "已取消，后续项目未处理".into()));
            break;
        }
        let target_name = match source_name(source) {
            Ok(name) => name,
            Err(error) => {
                result.failures.push((describe(source), error));
                continue;
            }
        };
        if !targets.insert(directory.join(target_name).to_string_lossy().to_lowercase()) {
            result.failures.push((
                describe(source),
                "所选文件含多个同名目标，请分别移动或先重命名".into(),
            ));
            continue;
        }
        match source {
            Source::Local(path) => {
                let moved =
                    paste_local_cancellable(&[path.clone()], directory, true, overwrite, cancel);
                result.completed.extend(moved.completed);
                result.failures.extend(moved.failures);
            }
            Source::Archive {
                path,
                entry,
                password,
            } => {
                let outcome = (|| {
                    write_format(path)?;
                    let target = directory.join(source_name(source)?);
                    copy_to_cancellable(source, &target, overwrite, max_bytes, cancel)?;
                    Ok(target)
                })();
                match outcome {
                    Ok(target) => archives
                        .entry((path.clone(), password.clone()))
                        .or_default()
                        .push((entry.clone(), target)),
                    Err(error) => result.failures.push((describe(source), error)),
                }
            }
        }
    }
    for ((path, password), copied) in archives {
        if cancel.load(Ordering::Relaxed) {
            for (entry, target) in copied {
                result.failures.push((
                    format!("{} :: {entry}", path.display()),
                    format!("已复制到{}，但移动已取消，包内原条目保留", target.display()),
                ));
            }
            continue;
        }
        let entries = copied.iter().map(|(entry, _)| entry.clone()).collect();
        match edit_archive_cancellable(
            &path,
            &ArchiveEdit::Delete(entries),
            password.as_deref(),
            cancel,
        ) {
            Ok(edited) => {
                result.containers.extend(edited.container_update);
                result.backups.push(edited.backup);
                result
                    .completed
                    .extend(copied.into_iter().map(|(_, target)| target));
            }
            Err(error) => {
                for (entry, target) in copied {
                    result.failures.push((
                        format!("{} :: {entry}", path.display()),
                        format!("已复制到 {}，但未移除包内原条目：{error}", target.display()),
                    ));
                }
            }
        }
    }
    result
}

/// {name}=stem, {ext}=extension including its dot, {n}=sequence. Legacy * and
/// runs of # are also supported. {date}/{time} use each file's modified time.
pub fn plan_rename(
    sources: &[Source],
    template: &RenameTemplate,
) -> Result<Vec<RenameItem>, String> {
    if sources.is_empty() || sources.len() > MAX_TREE_ENTRIES {
        return Err("请选择1至十万个项目".into());
    }
    if template.pattern.is_empty() || template.pattern.len() > 4096 || template.padding > 20 {
        return Err("重命名模板为空、过长或数字位数超过20".into());
    }
    let needs_date = template.pattern.contains("{date}") || template.pattern.contains("{time}");
    let mut dates = HashMap::new();
    let mut source_bytes = 0usize;
    for source in sources {
        source_bytes = source_bytes.saturating_add(match source {
            Source::Local(path) => path.as_os_str().len(),
            Source::Archive { path, entry, .. } => {
                path.as_os_str().len().saturating_add(entry.len())
            }
        });
        if source_bytes > 32 * 1024 * 1024 {
            return Err("重命名源路径超过32MiB计划上限，请分批处理".into());
        }
    }
    if needs_date {
        if let Some(Source::Archive { path, password, .. }) = sources.first() {
            let mut command = engine_command("l", write_format(path)?)?;
            command.arg("-slt").arg("--").arg(path);
            let listing =
                String::from_utf8(run_engine(command, password.as_deref(), 16 * 1024 * 1024)?)
                    .map_err(|_| "压缩包日期索引编码无效")?
                    .replace("\r\n", "\n");
            if let Some((_, records)) = listing.split_once("\n----------\n") {
                for record in records.split("\n\n") {
                    let name = record.lines().find_map(|l| l.strip_prefix("Path = "));
                    let date = record
                        .lines()
                        .find_map(|l| l.strip_prefix("Modified = "))
                        .and_then(|s| s.get(..19))
                        .and_then(|s| parse_local_datetime(s).ok())
                        .and_then(|s| {
                            if s >= 0 {
                                UNIX_EPOCH.checked_add(Duration::from_secs(s as u64))
                            } else {
                                UNIX_EPOCH.checked_sub(Duration::from_secs(s.unsigned_abs()))
                            }
                        });
                    if let (Some(name), Some(date)) = (name, date) {
                        dates.insert(name.to_owned(), date);
                    }
                }
            }
        }
    }
    let archive_metadata = if let Some(Source::Archive { path, .. }) = sources.first() {
        Some(fs::symlink_metadata(path).map_err(|e| e.to_string())?)
    } else {
        None
    };
    let mut plan = Vec::with_capacity(sources.len());
    for (index, source) in sources.iter().enumerate() {
        let original = source_name(source)?;
        let path = match source {
            Source::Local(p) | Source::Archive { path: p, .. } => p,
        };
        let metadata = match (&archive_metadata, source) {
            (Some(metadata), Source::Archive { .. }) => metadata.clone(),
            _ => fs::symlink_metadata(path).map_err(|e| e.to_string())?,
        };
        if reparse(&metadata) {
            return Err("不重命名符号链接或重解析点".into());
        }
        let stem = if metadata.is_dir() {
            original.as_str()
        } else {
            Path::new(&original)
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or(&original)
        };
        let extension = if metadata.is_dir() {
            String::new()
        } else {
            Path::new(&original)
                .extension()
                .and_then(|s| s.to_str())
                .map(|s| format!(".{s}"))
                .unwrap_or_default()
        };
        let date = if needs_date {
            Some(match source {
                Source::Local(_) => metadata.modified().map_err(|e| e.to_string())?,
                Source::Archive { entry, .. } => *dates
                    .get(entry)
                    .ok_or("包内条目没有可用修改日期，请使用不含日期的模板")?,
            })
        } else {
            None
        };
        let number = template
            .step
            .checked_mul(index as u64)
            .and_then(|v| template.start.checked_add(v))
            .ok_or("序号超出支持范围")?;
        let new_name = render_rename(
            &template.pattern,
            stem,
            &extension,
            number,
            template.padding,
            date,
        )?;
        valid_name(&new_name)?;
        plan.push(RenameItem {
            source: source.clone(),
            new_name,
            expected_size: metadata.len(),
            expected_modified: metadata.modified().ok(),
        });
    }
    let plan = expand_rename_sidecars(&plan)?;
    validate_rename_plan(&plan)?;
    Ok(plan)
}

fn render_rename(
    pattern: &str,
    name: &str,
    extension: &str,
    number: u64,
    padding: usize,
    date: Option<SystemTime>,
) -> Result<String, String> {
    let formatted = date.map(format_local_datetime);
    let mut output = String::new();
    let mut chars = pattern.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '*' => output.push_str(name),
            '#' => {
                let mut width = 1;
                while chars.peek() == Some(&'#') {
                    chars.next();
                    width += 1;
                }
                if width > 20 {
                    return Err("序号位数超过20".into());
                }
                output.push_str(&format!("{number:0width$}"));
            }
            '{' => {
                let mut token = String::new();
                loop {
                    match chars.next() {
                        Some('}') => break,
                        Some(c) => token.push(c),
                        None => return Err("模板占位符缺少右花括号".into()),
                    }
                }
                match token.as_str() {
                    "name" => output.push_str(name),
                    "ext" => output.push_str(extension),
                    "n" => output.push_str(&format!("{number:0padding$}")),
                    "date" => output.push_str(
                        &formatted
                            .as_ref()
                            .and_then(|d| d.get(..10))
                            .ok_or("该项目没有有效日期")?
                            .replace('-', ""),
                    ),
                    "time" => output.push_str(
                        &formatted
                            .as_ref()
                            .and_then(|d| d.get(11..19))
                            .ok_or("该项目没有有效时间")?
                            .replace(':', ""),
                    ),
                    _ => return Err(format!("未知重命名占位符：{{{token}}}")),
                }
            }
            c => output.push(c),
        }
        if output.len() > 4096 {
            return Err("生成的文件名过长".into());
        }
    }
    Ok(output)
}

fn validate_rename_plan(plan: &[RenameItem]) -> Result<(), String> {
    if plan.is_empty() || plan.len() > MAX_TREE_ENTRIES {
        return Err("重命名计划为空或超过十万项".into());
    }
    let first = &plan[0].source;
    let mut originals = HashSet::new();
    let mut targets = HashSet::new();
    for item in plan {
        valid_name(&item.new_name)?;
        match (&item.source, first) {
            (Source::Local(path), Source::Local(_)) => {
                let source = operation_path(path)?;
                let key = source.to_string_lossy().to_lowercase();
                if !originals.insert(key) {
                    return Err("重命名源项目重复".into());
                }
                let target = source.with_file_name(&item.new_name);
                if !targets.insert(target.to_string_lossy().to_lowercase()) {
                    return Err("重命名会产生重复目标".into());
                }
            }
            (Source::Archive { path, entry, .. }, Source::Archive { path: origin, .. })
                if same_file(path, origin) =>
            {
                if !originals.insert(entry.to_lowercase()) {
                    return Err("重命名源条目重复".into());
                }
                let parent = entry.rfind(['/', '\\']).map(|i| &entry[..=i]).unwrap_or("");
                if !targets.insert(format!("{parent}{}", item.new_name).to_lowercase()) {
                    return Err("重命名会产生重复包内目标".into());
                }
            }
            _ => return Err("一次重命名须来自本地项目或同一个压缩包".into()),
        }
    }
    for item in plan {
        if let Source::Local(path) = &item.source {
            let source = operation_path(path)?;
            let target = source.with_file_name(&item.new_name);
            if target.exists() && !originals.contains(&target.to_string_lossy().to_lowercase()) {
                return Err(format!("目标已存在，未覆盖：{}", target.display()));
            }
        }
    }
    if let Source::Archive { path, password, .. } = first {
        write_format(path)?;
        let entries = crate::archive::list(path, password.as_deref()).map_err(|e| e.to_string())?;
        let present: HashSet<_> = entries.iter().map(|e| e.name.to_lowercase()).collect();
        if !originals.is_subset(&present) {
            return Err("一个或多个包内源条目不存在".into());
        }
        if targets
            .iter()
            .any(|target| present.contains(target) && !originals.contains(target))
        {
            return Err("包内目标已有其它条目，未覆盖".into());
        }
    }
    Ok(())
}

pub fn rename_many(plan: &[RenameItem]) -> Result<RenameResult, String> {
    rename_many_cancellable(plan, &AtomicBool::new(false))
}

pub fn rename_many_cancellable(
    plan: &[RenameItem],
    cancel: &AtomicBool,
) -> Result<RenameResult, String> {
    let expanded = expand_rename_sidecars(plan)?;
    let plan = expanded.as_slice();
    validate_rename_plan(plan)?;
    cancelled(cancel)?;
    for item in plan {
        let path = match &item.source {
            Source::Local(p) | Source::Archive { path: p, .. } => p,
        };
        let m = fs::metadata(path).map_err(|e| e.to_string())?;
        if m.len() != item.expected_size || m.modified().ok() != item.expected_modified {
            return Err("源项目在预览后发生变化，请重新生成计划".into());
        }
    }
    if let Source::Archive { path, password, .. } = &plan[0].source {
        let pairs = plan
            .iter()
            .filter_map(|item| {
                if let Source::Archive { entry, .. } = &item.source {
                    Some((entry.clone(), item.new_name.clone()))
                } else {
                    None
                }
            })
            .collect::<Vec<_>>();
        if pairs
            .iter()
            .all(|(entry, new)| entry.rsplit(['/', '\\']).next() == Some(new.as_str()))
        {
            return Ok(RenameResult {
                moved: Vec::new(),
                backups: Vec::new(),
                containers: Vec::new(),
            });
        }
        let result = edit_archive_cancellable(
            path,
            &ArchiveEdit::RenameMany(pairs),
            password.as_deref(),
            cancel,
        )?;
        let moved = plan
            .iter()
            .map(|item| {
                let Source::Archive {
                    path,
                    entry,
                    password,
                } = &item.source
                else {
                    unreachable!()
                };
                let parent = entry.rfind(['/', '\\']).map(|i| &entry[..=i]).unwrap_or("");
                (
                    item.source.clone(),
                    Source::Archive {
                        path: path.clone(),
                        entry: format!("{parent}{}", item.new_name),
                        password: password.clone(),
                    },
                )
            })
            .collect();
        return Ok(RenameResult {
            moved,
            backups: vec![result.backup],
            containers: result.container_update.into_iter().collect(),
        });
    }
    let mut staged: Vec<(PathBuf, PathBuf, PathBuf)> = Vec::new();
    let mut rollback_failures = Vec::new();
    let operation = (|| -> Result<(), String> {
        for item in plan {
            cancelled(cancel)?;
            let Source::Local(path) = &item.source else {
                unreachable!()
            };
            let path = operation_path(path)?;
            let target = path.with_file_name(&item.new_name);
            if path == target {
                continue;
            }
            let reserved = tempfile::Builder::new()
                .prefix(".rustview-rename-")
                .tempfile_in(path.parent().ok_or("源目录无效")?)
                .map_err(|e| e.to_string())?
                .into_temp_path()
                .keep()
                .map_err(|e| e.to_string())?;
            fs::remove_file(&reserved).map_err(|e| e.to_string())?;
            rename_no_replace(&path, &reserved)?;
            staged.push((path, target, reserved));
        }
        cancelled(cancel)?;
        let mut committed = 0;
        for (_, target, stage) in &staged {
            if let Err(error) = rename_no_replace(stage, target) {
                for (_, target, stage) in staged[..committed].iter().rev() {
                    if let Err(e) = rename_no_replace(target, stage) {
                        rollback_failures.push(format!("{}：{e}", target.display()));
                    }
                }
                return Err(error);
            }
            committed += 1;
        }
        Ok(())
    })();
    if let Err(error) = operation {
        for (old, _, stage) in staged.iter().rev() {
            if stage.exists() {
                if let Err(e) = rename_no_replace(stage, old) {
                    rollback_failures.push(format!("{} → {}：{e}", stage.display(), old.display()));
                }
            }
        }
        return Err(if rollback_failures.is_empty() {
            format!("重命名未完成，已回滚：{error}")
        } else {
            format!(
                "{error}；以下原件保留在暂存路径或目标路径，请手动恢复：{}",
                rollback_failures.join("；")
            )
        });
    }
    Ok(RenameResult {
        moved: staged
            .into_iter()
            .map(|(old, new, _)| (Source::Local(old), Source::Local(new)))
            .collect(),
        backups: Vec::new(),
        containers: Vec::new(),
    })
}

#[derive(Serialize, Deserialize)]
struct CacheMarker {
    version: u8,
    created: u64,
    bytes: u64,
    complete: bool,
    #[serde(default)]
    owner: u32,
}
struct CacheEntry {
    path: PathBuf,
    marker: CacheMarker,
    pinned: bool,
}

fn cache_root(root: &Path) -> Result<PathBuf, String> {
    fs::create_dir_all(root).map_err(|e| e.to_string())?;
    let metadata = fs::symlink_metadata(root).map_err(|e| e.to_string())?;
    if reparse(&metadata) || !metadata.is_dir() {
        return Err("剪贴板缓存根目录不是普通目录".into());
    }
    let root = root.canonicalize().map_err(|e| e.to_string())?;
    if root.parent().is_none() {
        return Err("不能把磁盘根目录用作剪贴板缓存".into());
    }
    Ok(root)
}

fn cache_entries(root: &Path, pinned: &[PathBuf]) -> Result<Vec<CacheEntry>, String> {
    let pinned = pinned
        .iter()
        .filter_map(|p| p.canonicalize().ok())
        .collect::<Vec<_>>();
    let mut entries = Vec::new();
    for entry in fs::read_dir(root).map_err(|e| e.to_string())? {
        let entry = entry.map_err(|e| e.to_string())?;
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if !name.starts_with("export-") && !name.starts_with(".rustview-export-") {
            continue;
        }
        let metadata = fs::symlink_metadata(entry.path()).map_err(|e| e.to_string())?;
        if !metadata.is_dir() || reparse(&metadata) {
            continue;
        }
        let marker_path = entry.path().join(".rustview-cache.json");
        let marker = match File::open(marker_path).and_then(|mut f| {
            let mut b = Vec::new();
            Read::by_ref(&mut f).take(1025).read_to_end(&mut b)?;
            Ok(b)
        }) {
            Ok(b) if b.len() <= 1024 => serde_json::from_slice::<CacheMarker>(&b).ok(),
            _ => None,
        };
        let Some(marker) = marker.filter(|m| m.version == 1 && m.bytes <= CLIPBOARD_CACHE_BYTES)
        else {
            continue;
        };
        let path = entry.path().canonicalize().map_err(|e| e.to_string())?;
        if path.parent() != Some(root) {
            return Err("缓存路径已移出缓存根目录，未删除".into());
        }
        let in_use = pinned
            .iter()
            .any(|p| p.starts_with(&path) || path.starts_with(p));
        entries.push(CacheEntry {
            path,
            marker,
            pinned: in_use,
        });
        if entries.len() > 10_000 {
            return Err("剪贴板缓存目录超过一万项，请先手动检查缓存".into());
        }
    }
    entries.sort_by_key(|e| e.marker.created);
    Ok(entries)
}

fn prune_cache_to(
    root: &Path,
    pinned: &[PathBuf],
    budget: u64,
    clear: bool,
) -> Result<CacheCleanup, String> {
    let root = cache_root(root)?;
    let entries = cache_entries(&root, pinned)?;
    let mut total = entries
        .iter()
        .map(|e| e.marker.bytes)
        .fold(0u64, u64::saturating_add);
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let mut result = CacheCleanup::default();
    for entry in entries {
        let stale_stage = !entry.marker.complete
            && (!process_alive(entry.marker.owner)
                || now.saturating_sub(entry.marker.created) > 24 * 60 * 60);
        if entry.pinned
            || (!entry.marker.complete && !stale_stage)
            || (!clear && !stale_stage && total <= budget)
        {
            continue;
        }
        let metadata = fs::symlink_metadata(&entry.path).map_err(|e| e.to_string())?;
        let resolved = entry.path.canonicalize().map_err(|e| e.to_string())?;
        if reparse(&metadata) || resolved.parent() != Some(root.as_path()) {
            return Err("缓存目录位置已变化，未执行删除".into());
        }
        fs::remove_dir_all(&resolved).map_err(|e| e.to_string())?;
        total = total.saturating_sub(entry.marker.bytes);
        result.bytes = result.bytes.saturating_add(entry.marker.bytes);
        result.removed += 1;
    }
    result.retained_bytes = total;
    if !clear && total > budget {
        return Err(
            "剪贴板缓存预算不足；当前剪贴板引用的文件会保留，请先完成粘贴或复制其它文件后清理缓存"
                .into(),
        );
    }
    Ok(result)
}

fn process_alive(pid: u32) -> bool {
    if pid == 0 {
        return false;
    }
    if pid == std::process::id() {
        return true;
    }
    #[cfg(windows)]
    unsafe {
        use windows_sys::Win32::{
            Foundation::{CloseHandle, GetLastError},
            System::Threading::{OpenProcess, WaitForSingleObject, PROCESS_SYNCHRONIZE},
        };
        let process = OpenProcess(PROCESS_SYNCHRONIZE, 0, pid);
        if process.is_null() {
            return GetLastError() != 87;
        }
        let running = WaitForSingleObject(process, 0) != 0;
        CloseHandle(process);
        running
    }
    #[cfg(not(windows))]
    {
        false
    }
}

fn cache_lock(root: &Path) -> Result<File, String> {
    let file = fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(root.join(".rustview-cache.lock"))
        .map_err(|e| e.to_string())?;
    file.try_lock()
        .map_err(|_| "另一个窗口正在处理剪贴板缓存，请稍后重试".to_owned())?;
    Ok(file)
}

pub fn prune_clipboard_cache(root: &Path, pinned: &[PathBuf]) -> Result<CacheCleanup, String> {
    let root = cache_root(root)?;
    let _lock = cache_lock(&root)?;
    prune_cache_to(&root, pinned, CLIPBOARD_CACHE_BYTES, false)
}
pub fn clear_clipboard_cache(root: &Path, pinned: &[PathBuf]) -> Result<CacheCleanup, String> {
    let root = cache_root(root)?;
    let _lock = cache_lock(&root)?;
    prune_cache_to(&root, pinned, 0, true)
}

/// Exported files survive viewer exit. The caller pins current clipboard paths
/// during eviction, publishes the returned references only after success, and
/// offers explicit cache cleanup. No password is persisted in this cache.
pub fn export_clipboard_cache(
    sources: &[Source],
    root: &Path,
    pinned: &[PathBuf],
) -> Result<ClipboardExport, String> {
    export_clipboard_cache_cancellable(sources, root, pinned, &AtomicBool::new(false))
}

pub fn export_clipboard_cache_cancellable(
    sources: &[Source],
    root: &Path,
    pinned: &[PathBuf],
    cancel: &AtomicBool,
) -> Result<ClipboardExport, String> {
    if sources.is_empty() || sources.len() > MAX_TREE_ENTRIES {
        return Err("剪贴板导出项目为空或超过十万项".into());
    }
    let root = cache_root(root)?;
    let _lock = cache_lock(&root)?;
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default();
    let final_path = root.join(format!(
        "export-{}-{}",
        stamp.as_nanos(),
        std::process::id()
    ));
    let mut groups: HashMap<PathBuf, Vec<usize>> = HashMap::new();
    let mut sizes = vec![0u64; sources.len()];
    let mut names = HashSet::new();
    let mut paths = Vec::with_capacity(sources.len());
    let mut units = 1usize;
    for (index, source) in sources.iter().enumerate() {
        cancelled(cancel)?;
        let name = source_name(source)?;
        if !names.insert(name.to_lowercase()) {
            return Err("剪贴板导出存在同名文件，请分别复制或先重命名".into());
        }
        let path = final_path.join(name);
        units = units.saturating_add(path.as_os_str().to_string_lossy().encode_utf16().count() + 1);
        if units > 8 * 1024 * 1024 {
            return Err("剪贴板路径总长度超过16MiB上限，请分批复制".into());
        }
        paths.push(path);
        match source {
            Source::Local(path) => {
                regular_file(path)?;
                sizes[index] = fs::metadata(path).map_err(|e| e.to_string())?.len();
            }
            Source::Archive { path, .. } => groups.entry(path.clone()).or_default().push(index),
        }
    }
    for (path, indices) in groups {
        cancelled(cancel)?;
        let Source::Archive { password, .. } = &sources[indices[0]] else {
            unreachable!()
        };
        let listing = crate::archive::list(&path, password.as_deref())
            .map_err(|e| e.to_string())?
            .into_iter()
            .map(|e| (e.name, e.size))
            .collect::<HashMap<_, _>>();
        for index in indices {
            let Source::Archive { entry, .. } = &sources[index] else {
                unreachable!()
            };
            sizes[index] = *listing.get(entry).ok_or("包内源条目不存在")?;
        }
    }
    let total = sizes.iter().try_fold(0u64, |sum, size| {
        if *size > crate::worker::MAX_INPUT_BYTES {
            return Err("单个剪贴板导出文件超过512MiB".to_owned());
        }
        sum.checked_add(*size)
            .filter(|sum| *sum <= CLIPBOARD_CACHE_BYTES)
            .ok_or_else(|| "导出总量超过2GiB剪贴板磁盘缓存预算，请分批复制".into())
    })?;
    prune_cache_to(&root, pinned, CLIPBOARD_CACHE_BYTES - total, false)?;
    let staging = tempfile::Builder::new()
        .prefix(".rustview-export-")
        .tempdir_in(&root)
        .map_err(|e| e.to_string())?;
    let marker = CacheMarker {
        version: 1,
        created: stamp.as_secs(),
        bytes: total,
        complete: false,
        owner: std::process::id(),
    };
    write_cache_marker(staging.path(), &marker)?;
    let mut copied = 0u64;
    for (index, source) in sources.iter().enumerate() {
        let destination = staging
            .path()
            .join(paths[index].file_name().ok_or("缓存文件名无效")?);
        copy_to_cancellable(source, &destination, false, sizes[index], cancel)?;
        copied = copied
            .checked_add(fs::metadata(destination).map_err(|e| e.to_string())?.len())
            .filter(|sum| *sum <= CLIPBOARD_CACHE_BYTES)
            .ok_or("缓存总量超过预算")?;
    }
    cancelled(cancel)?;
    write_cache_marker(
        staging.path(),
        &CacheMarker {
            version: 1,
            created: stamp.as_secs(),
            bytes: copied,
            complete: true,
            owner: std::process::id(),
        },
    )?;
    rename_no_replace(staging.path(), &final_path)?;
    let _ = staging.keep();
    Ok(ClipboardExport {
        directory: final_path,
        paths,
    })
}

fn write_cache_marker(path: &Path, marker: &CacheMarker) -> Result<(), String> {
    let mut file = File::create(path.join(".rustview-cache.json")).map_err(|e| e.to_string())?;
    file.write_all(&serde_json::to_vec(marker).map_err(|e| e.to_string())?)
        .map_err(|e| e.to_string())?;
    file.sync_all().map_err(|e| e.to_string())
}

pub fn write_file_list(sources: &[Source], destination: &Path) -> Result<(), String> {
    if sources.iter().any(|source| {
        same_file(
            match source {
                Source::Local(path) | Source::Archive { path, .. } => path,
            },
            destination,
        )
    }) {
        return Err("不能用文件列表覆盖源图片或压缩包".into());
    }
    let parent = destination
        .parent()
        .filter(|path| !path.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let mut temporary =
        tempfile::NamedTempFile::new_in(parent).map_err(|error| error.to_string())?;
    for source in sources {
        writeln!(temporary, "{}", describe(source)).map_err(|error| error.to_string())?;
    }
    temporary
        .as_file()
        .sync_all()
        .map_err(|error| error.to_string())?;
    temporary
        .persist(destination)
        .map_err(|error| error.to_string())?;
    Ok(())
}

pub fn set_times(paths: &[PathBuf], change: &Times) -> BatchResult {
    fn time(seconds: i64) -> Result<SystemTime, String> {
        if seconds >= 0 {
            UNIX_EPOCH.checked_add(Duration::from_secs(seconds as u64))
        } else {
            UNIX_EPOCH.checked_sub(Duration::from_secs(seconds.unsigned_abs()))
        }
        .ok_or_else(|| "日期超出系统支持范围".into())
    }
    let mut result = BatchResult::default();
    for path in paths {
        let outcome = (|| {
            regular_file(path)?;
            let mut times = fs::FileTimes::new();
            if let Some(value) = change.modified {
                times = times.set_modified(time(value)?);
            }
            if let Some(value) = change.accessed {
                times = times.set_accessed(time(value)?);
            }
            if let Some(value) = change.created {
                #[cfg(windows)]
                {
                    use std::os::windows::fs::FileTimesExt;
                    times = times.set_created(time(value)?);
                }
                #[cfg(not(windows))]
                {
                    let _ = value;
                    return Err("创建日期修改仅支持Windows".to_owned());
                }
            }
            File::options()
                .write(true)
                .open(path)
                .map_err(|error| error.to_string())?
                .set_times(times)
                .map_err(|error| error.to_string())
        })();
        match outcome {
            Ok(()) => result.completed.push(path.clone()),
            Err(error) => result.failures.push((path.display().to_string(), error)),
        }
    }
    result
}

/// The GUI must obtain confirmation before calling either destructive mode.
pub fn delete_local(paths: &[PathBuf], permanent: bool) -> BatchResult {
    delete_local_cancellable(paths, permanent, &AtomicBool::new(false))
}

/// Cancellation is observed between complete items, never halfway through a tree delete.
pub fn delete_local_cancellable(
    paths: &[PathBuf],
    permanent: bool,
    cancel: &AtomicBool,
) -> BatchResult {
    let mut result = BatchResult::default();
    for path in paths {
        if cancel.load(Ordering::Relaxed) {
            result
                .failures
                .push((path.display().to_string(), "已取消，后续项目未删除".into()));
            break;
        }
        let outcome = (|| {
            let checked = operation_path(path)?;
            if permanent {
                inspect_tree(&checked, cancel)?;
                remove_local(&checked)
            } else {
                trash::delete(&checked).map_err(|e| e.to_string())
            }
        })();
        match outcome {
            Ok(()) => result.completed.push(path.clone()),
            Err(e) => result.failures.push((path.display().to_string(), e)),
        }
    }
    result
}

fn write_format(path: &Path) -> Result<&'static str, String> {
    let mut magic = [0; 8];
    let mut file = File::open(path).map_err(|error| error.to_string())?;
    let count = file.read(&mut magic).map_err(|error| error.to_string())?;
    let magic = &magic[..count];
    if magic.starts_with(b"PK\x03\x04") || magic.starts_with(b"PK\x05\x06") {
        Ok("-tzip")
    } else if magic.starts_with(b"7z\xbc\xaf\x27\x1c") {
        Ok("-t7z")
    } else {
        Err("只支持修改常规单卷 ZIP 或 7z；RAR 可以浏览和复制条目，但不支持写入，原包未改变".into())
    }
}

fn collect_output(
    mut stream: impl Read + Send + 'static,
    limit: usize,
) -> thread::JoinHandle<(Vec<u8>, bool)> {
    thread::spawn(move || {
        let mut saved = Vec::new();
        let mut truncated = false;
        let mut buffer = [0; 8192];
        while let Ok(count) = stream.read(&mut buffer) {
            if count == 0 {
                break;
            }
            let keep = count.min(limit.saturating_sub(saved.len()));
            saved.extend_from_slice(&buffer[..keep]);
            truncated |= keep != count;
        }
        (saved, truncated)
    })
}

fn run_engine(
    command: Command,
    password: Option<&str>,
    output_limit: usize,
) -> Result<Vec<u8>, String> {
    run_engine_cancellable(command, password, output_limit, &AtomicBool::new(false))
}

fn run_engine_cancellable(
    mut command: Command,
    password: Option<&str>,
    output_limit: usize,
    cancel: &AtomicBool,
) -> Result<Vec<u8>, String> {
    cancelled(cancel)?;
    if password.is_some_and(|value| value.len() > 1024 || value.contains(['\0', '\r', '\n'])) {
        return Err("密码过长或包含换行字符".into());
    }
    command
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x0800_0000);
    }
    let mut child = command.spawn().map_err(|error| error.to_string())?;
    #[cfg(windows)]
    let _job = match crate::worker::Job::attach(&child) {
        Ok(job) => job,
        Err(e) => {
            let _ = child.kill();
            let _ = child.wait();
            return Err(e);
        }
    };
    let output = collect_output(child.stdout.take().unwrap(), output_limit);
    let errors = collect_output(child.stderr.take().unwrap(), 64 * 1024);
    if let Some(mut stdin) = child.stdin.take() {
        if let Some(password) = password {
            // Update operations can ask separately for the old password and encryption confirmation.
            for _ in 0..3 {
                if stdin
                    .write_all(password.as_bytes())
                    .and_then(|()| stdin.write_all(b"\n"))
                    .is_err()
                {
                    break;
                }
            }
        }
    }
    let started = Instant::now();
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break Ok(status),
            Ok(None)
                if started.elapsed() < Duration::from_secs(110)
                    && !cancel.load(Ordering::Relaxed) =>
            {
                thread::sleep(Duration::from_millis(20))
            }
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                break Err(if cancel.load(Ordering::Relaxed) {
                    "压缩包操作已取消，原包未替换".to_owned()
                } else {
                    "压缩包修改超时或进程已停止，原包未替换".to_owned()
                });
            }
        }
    };
    let (output, truncated) = output.join().unwrap_or_default();
    let (errors, _) = errors.join().unwrap_or_default();
    let status = status?;
    if !status.success() {
        let detail = String::from_utf8_lossy(&errors);
        if detail.to_ascii_lowercase().contains("password") || status.code() == Some(255) {
            return Err("压缩包需要正确密码才能修改，原包未替换".into());
        }
        return Err(format!(
            "7-Zip 修改或校验失败，原包未替换：{}",
            detail.trim()
        ));
    }
    if truncated && output_limit > 64 * 1024 {
        return Err("压缩包索引超过修改操作的读取上限".into());
    }
    Ok(output)
}

fn engine_command(operation: &str, format: &str) -> Result<Command, String> {
    let mut command =
        Command::new(crate::archive::engine_path().map_err(|error| error.to_string())?);
    command.args([
        operation,
        format,
        "-sccUTF-8",
        "-bd",
        "-bsp0",
        "-bse2",
        "-spd",
        "-ssc",
        "-r-",
    ]);
    Ok(command)
}

fn encrypted_entries(
    path: &Path,
    format: &str,
    password: Option<&str>,
) -> Result<HashSet<String>, String> {
    let mut command = engine_command("l", format)?;
    command.arg("-slt").arg("--").arg(path);
    let bytes = run_engine(command, password, 16 * 1024 * 1024)?;
    let listing = String::from_utf8(bytes)
        .map_err(|_| "压缩包索引编码无效")?
        .replace("\r\n", "\n");
    let records = listing
        .split_once("\n----------\n")
        .ok_or("压缩包索引无效")?
        .1;
    Ok(records
        .split("\n\n")
        .filter_map(|record| {
            let name = record
                .lines()
                .find_map(|line| line.strip_prefix("Path = "))?;
            record
                .lines()
                .any(|line| line == "Encrypted = +")
                .then(|| name.to_owned())
        })
        .collect())
}

fn replace_archive(original: &Path, replacement: &Path, backup: &Path) -> Result<(), String> {
    #[cfg(windows)]
    {
        use std::os::windows::ffi::OsStrExt;
        use windows_sys::Win32::Storage::FileSystem::ReplaceFileW;
        let wide = |path: &Path| {
            path.as_os_str()
                .encode_wide()
                .chain(Some(0))
                .collect::<Vec<_>>()
        };
        let original = wide(original);
        let replacement = wide(replacement);
        let backup = wide(backup);
        if unsafe {
            ReplaceFileW(
                original.as_ptr(),
                replacement.as_ptr(),
                backup.as_ptr(),
                0,
                std::ptr::null(),
                std::ptr::null(),
            )
        } == 0
        {
            return Err(format!(
                "无法安全替换压缩包：{}",
                std::io::Error::last_os_error()
            ));
        }
    }
    #[cfg(not(windows))]
    {
        fs::copy(original, backup).map_err(|error| error.to_string())?;
        fs::rename(replacement, original).map_err(|error| error.to_string())?;
    }
    Ok(())
}

fn prepare_archive_renames(
    pairs: &[(String, String)],
    expected: &mut HashMap<String, u64>,
    encrypted: &mut HashSet<String>,
) -> Result<Vec<(String, String)>, String> {
    if pairs.is_empty() || pairs.len() > MAX_TREE_ENTRIES {
        return Err("重命名计划为空或过大".into());
    }
    let mut seen = HashSet::new();
    let mut removed = Vec::with_capacity(pairs.len());
    for (entry, name) in pairs {
        valid_name(name)?;
        if !seen.insert(entry.as_str()) {
            return Err("包内重命名源条目重复".into());
        }
        let size = expected.remove(entry).ok_or("包内源条目不存在")?;
        let protected = encrypted.remove(entry);
        let parent = entry.rfind(['/', '\\']).map(|i| &entry[..=i]).unwrap_or("");
        removed.push((entry.clone(), format!("{parent}{name}"), size, protected));
    }
    let mut occupied = expected
        .keys()
        .map(|p| p.to_lowercase())
        .collect::<HashSet<_>>();
    for (_, target, size, protected) in &removed {
        if !occupied.insert(target.to_lowercase()) {
            return Err("包内目标名称冲突，原包未改变".into());
        }
        expected.insert(target.clone(), *size);
        if *protected {
            encrypted.insert(target.clone());
        }
    }
    Ok(removed
        .into_iter()
        .filter_map(|(old, new, _, _)| (old != new).then_some((old, new)))
        .collect())
}

fn run_rename_pairs(
    path: &Path,
    format: &str,
    password: Option<&str>,
    pairs: &[(String, String)],
    cancel: &AtomicBool,
) -> Result<(), String> {
    let mut start = 0;
    while start < pairs.len() {
        cancelled(cancel)?;
        let mut end = start;
        let mut characters = path.as_os_str().len() + 2048;
        while end < pairs.len() {
            let pair = &pairs[end];
            let next = pair.0.encode_utf16().count() + pair.1.encode_utf16().count() + 8;
            if characters + next > 24_000 {
                break;
            }
            characters += next;
            end += 1;
        }
        if end == start {
            return Err("包内名称或压缩包路径过长，原包未修改".into());
        }
        let mut command = engine_command("rn", format)?;
        command.arg("--").arg(path);
        for (old, new) in &pairs[start..end] {
            command.arg(old).arg(new);
        }
        run_engine_cancellable(command, password, 64 * 1024, cancel)?;
        start = end;
    }
    Ok(())
}

/// Edits a same-directory copy, verifies it, then atomically replaces the archive with a backup.
/// The caller must confirm deletion/renaming before calling. RAR is never modified.
pub fn edit_archive(
    path: &Path,
    edit: &ArchiveEdit,
    password: Option<&str>,
) -> Result<ArchiveResult, String> {
    edit_archive_cancellable(path, edit, password, &AtomicBool::new(false))
}

pub fn edit_archive_cancellable(
    path: &Path,
    edit: &ArchiveEdit,
    password: Option<&str>,
    cancel: &AtomicBool,
) -> Result<ArchiveResult, String> {
    cancelled(cancel)?;
    let path = path.canonicalize().map_err(|error| error.to_string())?;
    let format = write_format(&path)?;
    let before_metadata = fs::metadata(&path).map_err(|error| error.to_string())?;
    let before_stamp = crate::recipes::SourceStamp::read(&path)?;
    let before = crate::archive::list(&path, password).map_err(|error| error.to_string())?;
    let sizes: HashMap<_, _> = before
        .iter()
        .map(|entry| (entry.name.clone(), entry.size))
        .collect();
    let encrypted = encrypted_entries(&path, format, password)?;
    if !encrypted.is_empty() && password.is_none() {
        return Err("修改密码包前需要输入正确密码".into());
    }
    let mut expected = sizes.clone();
    let mut expected_encrypted = encrypted.clone();
    let mut renamed_entry = None;
    let mut rename_pairs = Vec::new();
    let operation = match edit {
        ArchiveEdit::Delete(_) => "d",
        ArchiveEdit::Rename { .. } | ArchiveEdit::RenameMany(_) => "rn",
        ArchiveEdit::Add(_) => "u",
    };
    match edit {
        ArchiveEdit::Delete(entries) => {
            if entries.is_empty() {
                return Err("没有选择压缩包条目".into());
            }
            for entry in entries {
                if expected.remove(entry).is_none() {
                    return Err(format!("压缩包条目不存在：{entry}"));
                }
                expected_encrypted.remove(entry);
            }
        }
        ArchiveEdit::Rename { entry, new_name } => {
            rename_pairs = prepare_archive_renames(
                &[(entry.clone(), new_name.clone())],
                &mut expected,
                &mut expected_encrypted,
            )?;
            renamed_entry = rename_pairs.first().map(|(_, new)| new.clone());
        }
        ArchiveEdit::RenameMany(pairs) => {
            rename_pairs = prepare_archive_renames(pairs, &mut expected, &mut expected_encrypted)?
        }
        ArchiveEdit::Add(files) => {
            if files.is_empty() {
                return Err("没有可添加的文件".into());
            }
            for file in files {
                regular_file(file)?;
                let name = source_name(&Source::Local(file.clone()))?;
                if expected.contains_key(&name) {
                    return Err(format!(
                        "压缩包已有同名条目 {name}，请先重命名或删除后再添加"
                    ));
                }
                expected.insert(
                    name.clone(),
                    fs::metadata(file).map_err(|error| error.to_string())?.len(),
                );
                if !encrypted.is_empty() {
                    expected_encrypted.insert(name);
                }
            }
        }
    }
    let parent = path.parent().ok_or("压缩包路径无效")?;
    let suffix = if format == "-tzip" { ".zip" } else { ".7z" };
    let temporary = tempfile::Builder::new()
        .prefix(".rustview-edit-")
        .suffix(suffix)
        .tempfile_in(parent)
        .map_err(|error| error.to_string())?
        .into_temp_path();
    fs::copy(&path, &temporary).map_err(|error| error.to_string())?;
    cancelled(cancel)?;
    let mut command = engine_command(operation, format)?;
    if let ArchiveEdit::Delete(entries) = edit {
        for entry in entries {
            command.arg(format!("-i!{entry}"));
        }
    }
    // For ZIP updates, -p requests a stdin encryption password; without it new entries become plaintext.
    if matches!(edit, ArchiveEdit::Add(_)) && !encrypted.is_empty() {
        command.arg("-p");
    }
    command.arg("--").arg(&temporary);
    match edit {
        ArchiveEdit::Rename { .. } | ArchiveEdit::RenameMany(_) => {}
        ArchiveEdit::Add(files) => {
            command.args(files);
        }
        ArchiveEdit::Delete(_) => {}
    }
    if matches!(
        edit,
        ArchiveEdit::Rename { .. } | ArchiveEdit::RenameMany(_)
    ) {
        let token = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        let mut occupied = sizes
            .keys()
            .map(|s| s.to_lowercase())
            .collect::<HashSet<_>>();
        let mut stages = Vec::with_capacity(rename_pairs.len());
        for (index, (old, new)) in rename_pairs.iter().enumerate() {
            let parent = old.rfind(['/', '\\']).map(|i| &old[..=i]).unwrap_or("");
            let stage = format!("{parent}.rustview-rename-{token}-{index}");
            if !occupied.insert(stage.to_lowercase()) {
                return Err("包内暂存名称冲突，原包未改变".into());
            }
            stages.push((old.clone(), stage.clone(), new.clone()));
        }
        run_rename_pairs(
            &temporary,
            format,
            password,
            &stages
                .iter()
                .map(|(old, stage, _)| (old.clone(), stage.clone()))
                .collect::<Vec<_>>(),
            cancel,
        )?;
        run_rename_pairs(
            &temporary,
            format,
            password,
            &stages
                .iter()
                .map(|(_, stage, new)| (stage.clone(), new.clone()))
                .collect::<Vec<_>>(),
            cancel,
        )?;
    } else {
        run_engine_cancellable(command, password, 64 * 1024, cancel)?;
    }
    let mut test = engine_command("t", format)?;
    test.arg("--").arg(&temporary);
    run_engine_cancellable(test, password, 64 * 1024, cancel)?;
    let after = crate::archive::list(&temporary, password).map_err(|error| error.to_string())?;
    let after: HashMap<_, _> = after
        .into_iter()
        .map(|entry| (entry.name, entry.size))
        .collect();
    if after != expected {
        return Err("修改后条目列表与预期不一致，原包未替换".into());
    }
    if !expected_encrypted.is_subset(&encrypted_entries(&temporary, format, password)?) {
        return Err("修改会丢失原有密码保护，原包未替换".into());
    }
    let now = fs::metadata(&path).map_err(|error| error.to_string())?;
    if now.len() != before_metadata.len() || now.modified().ok() != before_metadata.modified().ok()
    {
        return Err("源压缩包在操作期间发生变化，未覆盖".into());
    }
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let backup = parent.join(format!(
        "{}.rustview-backup-{stamp}",
        path.file_name().unwrap().to_string_lossy()
    ));
    cancelled(cancel)?;
    replace_archive(&path, &temporary, &backup)?;
    let after = crate::recipes::SourceStamp::read(&path).map_err(|e| {
        format!(
            "压缩包已修改，备份在{}；无法读取新时间戳：{e}",
            backup.display()
        )
    })?;
    let removed = match edit {
        ArchiveEdit::Delete(entries) => entries.clone(),
        _ => Vec::new(),
    };
    Ok(ArchiveResult {
        backup,
        renamed_entry,
        container_update: Some(crate::recipes::ContainerUpdate {
            path,
            before: before_stamp,
            after,
            removed,
        }),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn photo_rename_and_move_preserve_xmp_and_refuse_sidecar_conflicts() {
        let root = tempfile::tempdir().unwrap();
        let photo = root.path().join("camera.nef");
        let side = root.path().join("camera.xmp");
        let live = root.path().join("camera.mov");
        fs::write(&photo, b"raw").unwrap();
        fs::write(&side, b"metadata").unwrap();
        fs::write(&live, b"excluded").unwrap();
        let plan = plan_rename(
            &[Source::Local(photo.clone())],
            &RenameTemplate {
                pattern: "renamed{ext}".into(),
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(plan.len(), 2);
        let renamed = rename_many(&plan).unwrap();
        assert_eq!(renamed.moved.len(), 2);
        assert_eq!(
            fs::read(root.path().join("renamed.xmp")).unwrap(),
            b"metadata"
        );
        assert_eq!(fs::read(&live).unwrap(), b"excluded");
        let target = root.path().join("target");
        fs::create_dir(&target).unwrap();
        fs::write(target.join("renamed.xmp"), b"existing").unwrap();
        let photo = root.path().join("renamed.nef");
        let failed = paste_local(&[photo.clone()], &target, true, false);
        assert_eq!(failed.failures.len(), 1);
        assert!(photo.exists());
        assert_eq!(fs::read(target.join("renamed.xmp")).unwrap(), b"existing");
        fs::remove_file(target.join("renamed.xmp")).unwrap();
        let moved = paste_local(&[photo.clone()], &target, true, false);
        assert!(moved.failures.is_empty());
        assert!(!photo.exists());
        assert!(!root.path().join("renamed.xmp").exists());
        assert_eq!(fs::read(target.join("renamed.xmp")).unwrap(), b"metadata");
    }

    #[test]
    fn sidecar_swap_is_transactional_and_ambiguous_shared_names_are_rejected() {
        let root = tempfile::tempdir().unwrap();
        for (name, data) in [
            ("a.jpg", b"aaa".as_slice()),
            ("b.jpg", b"bbb".as_slice()),
            ("a.xmp", b"a-meta".as_slice()),
            ("b.xmp", b"b-meta".as_slice()),
        ] {
            fs::write(root.path().join(name), data).unwrap();
        }
        let make = |old: &str, new: &str| {
            let path = root.path().join(old);
            let m = path.metadata().unwrap();
            RenameItem {
                source: Source::Local(path),
                new_name: new.into(),
                expected_size: m.len(),
                expected_modified: m.modified().ok(),
            }
        };
        rename_many(&[make("a.jpg", "b.jpg"), make("b.jpg", "a.jpg")]).unwrap();
        assert_eq!(fs::read(root.path().join("a.jpg")).unwrap(), b"bbb");
        assert_eq!(fs::read(root.path().join("a.xmp")).unwrap(), b"b-meta");
        fs::write(root.path().join("a.nef"), b"raw").unwrap();
        let ambiguous = plan_rename(
            &[
                Source::Local(root.path().join("a.jpg")),
                Source::Local(root.path().join("a.nef")),
            ],
            &RenameTemplate::default(),
        );
        assert!(ambiguous.is_err());
        assert!(root.path().join("a.xmp").exists());
        fs::write(root.path().join("occupied.xmp"), b"existing").unwrap();
        assert!(rename_many(&[make("a.jpg", "occupied.jpg")]).is_err());
        assert!(root.path().join("a.jpg").exists());
        assert_eq!(
            fs::read(root.path().join("occupied.xmp")).unwrap(),
            b"existing"
        );
    }

    #[test]
    fn batch_rename_templates_swaps_and_preview_conflicts_keep_data() {
        let root = tempfile::tempdir().unwrap();
        let a = root.path().join("a.jpg");
        let b = root.path().join("b.jpg");
        fs::write(&a, b"AAA").unwrap();
        fs::write(&b, b"BBB").unwrap();
        let sources = vec![Source::Local(a.clone()), Source::Local(b.clone())];
        let mut plan = plan_rename(
            &sources,
            &RenameTemplate {
                pattern: "{name}_{n}{ext}".into(),
                start: 8,
                step: 2,
                padding: 3,
            },
        )
        .unwrap();
        assert_eq!(plan[0].new_name, "a_008.jpg");
        assert_eq!(plan[1].new_name, "b_010.jpg");
        plan[0].new_name = "b.jpg".into();
        plan[1].new_name = "a.jpg".into();
        let result = rename_many(&plan).unwrap();
        assert_eq!(result.moved.len(), 2);
        assert_eq!(fs::read(&a).unwrap(), b"BBB");
        assert_eq!(fs::read(&b).unwrap(), b"AAA");
        let mut collision = plan_rename(&sources, &RenameTemplate::default()).unwrap();
        collision[0].new_name = "same.jpg".into();
        collision[1].new_name = "same.jpg".into();
        assert!(rename_many(&collision).is_err());
        assert_eq!(fs::read(&a).unwrap(), b"BBB");
        assert_eq!(
            render_rename(
                "*_##_{date}_{time}{ext}",
                "photo",
                ".png",
                7,
                2,
                Some(UNIX_EPOCH + Duration::from_secs(1_700_000_000))
            )
            .unwrap()
            .starts_with("photo_07_"),
            true
        );
        let cancelled = AtomicBool::new(true);
        assert!(rename_many_cancellable(&plan, &cancelled).is_err());
    }

    #[test]
    fn real_archive_swaps_and_delete_add_keep_only_valid_processing_records() {
        use crate::recipes::{self, EditRecord, Recipe};
        let root = tempfile::tempdir().unwrap();
        for name in ["a.png", "b.png", "keep.png", "stale.png"] {
            fs::write(root.path().join(name), name.as_bytes()).unwrap();
        }
        for (ext, format) in [("zip", "-tzip"), ("7z", "-t7z")] {
            let path = root.path().join(format!("recipes.{ext}"));
            let data = root.path().join(format!("records-{ext}"));
            let mut command = engine_command("a", format).unwrap();
            command.arg("--").arg(&path);
            for name in ["a.png", "b.png", "keep.png", "stale.png"] {
                command.arg(root.path().join(name));
            }
            run_engine(command, None, 65536).unwrap();
            for name in ["a.png", "b.png", "keep.png", "stale.png"] {
                let mut recipe = Recipe::default();
                recipe.name = name.into();
                let mut record = EditRecord::capture(&path, Some(name.into()), recipe).unwrap();
                let file = recipes::record_path(&data, &path, Some(name)).unwrap();
                record.save(&file).unwrap();
                if name == "stale.png" {
                    record.stamp.length += 1;
                    fs::write(file, serde_json::to_vec(&record).unwrap()).unwrap();
                }
            }
            let sources = [
                Source::Archive {
                    path: path.clone(),
                    entry: "a.png".into(),
                    password: None,
                },
                Source::Archive {
                    path: path.clone(),
                    entry: "b.png".into(),
                    password: None,
                },
            ];
            let mut plan = plan_rename(&sources, &RenameTemplate::default()).unwrap();
            plan[0].new_name = "b.png".into();
            plan[1].new_name = "a.png".into();
            let result = rename_many(&plan).unwrap();
            let key = |entry: &str| {
                serde_json::to_string(&(path.canonicalize().unwrap(), Some(entry))).unwrap()
            };
            let moves = vec![(key("a.png"), key("b.png")), (key("b.png"), key("a.png"))];
            recipes::relocate_keys_with_updates(&data, &moves, &result.containers).unwrap();
            assert_eq!(
                recipes::read_for_in(&data, &path, Some("a.png"))
                    .unwrap()
                    .recipe
                    .name,
                "b.png"
            );
            assert_eq!(
                recipes::read_for_in(&data, &path, Some("b.png"))
                    .unwrap()
                    .recipe
                    .name,
                "a.png"
            );
            assert!(recipes::read_for_in(&data, &path, Some("keep.png")).is_some());
            assert!(recipes::read_for_in(&data, &path, Some("stale.png")).is_none());
            let deleted =
                edit_archive(&path, &ArchiveEdit::Delete(vec!["a.png".into()]), None).unwrap();
            let update = deleted.container_update.unwrap();
            assert_eq!(update.removed, ["a.png"]);
            recipes::relocate_keys_with_updates(&data, &[], &[update]).unwrap();
            assert!(recipes::read_for_in(&data, &path, Some("a.png")).is_none());
            assert!(recipes::read_for_in(&data, &path, Some("keep.png")).is_some());
            let added = edit_archive(
                &path,
                &ArchiveEdit::Add(vec![root.path().join("a.png")]),
                None,
            )
            .unwrap();
            recipes::relocate_keys_with_updates(&data, &[], &[added.container_update.unwrap()])
                .unwrap();
            assert!(recipes::read_for_in(&data, &path, Some("a.png")).is_none());
            assert!(recipes::read_for_in(&data, &path, Some("keep.png")).is_some());
            assert!(recipes::read_for_in(&data, &path, Some("stale.png")).is_none());
        }
    }

    #[test]
    fn batch_archive_rename_swaps_in_one_backup_and_preserves_encryption() {
        let root = tempfile::tempdir().unwrap();
        let first = root.path().join("a.png");
        let second = root.path().join("b.png");
        fs::write(&first, b"first").unwrap();
        fs::write(&second, b"second").unwrap();
        for (ext, format) in [("zip", "-tzip"), ("7z", "-t7z")] {
            let path = root.path().join(format!("batch.{ext}"));
            let mut command = engine_command("a", format).unwrap();
            command
                .arg("-p")
                .arg("--")
                .arg(&path)
                .arg(&first)
                .arg(&second);
            run_engine(command, Some("secret-fixture"), 65536).unwrap();
            let sources = [
                Source::Archive {
                    path: path.clone(),
                    entry: "a.png".into(),
                    password: Some("secret-fixture".into()),
                },
                Source::Archive {
                    path: path.clone(),
                    entry: "b.png".into(),
                    password: Some("secret-fixture".into()),
                },
            ];
            let mut plan = plan_rename(&sources, &RenameTemplate::default()).unwrap();
            plan[0].new_name = "b.png".into();
            plan[1].new_name = "a.png".into();
            let renamed = rename_many(&plan).unwrap();
            assert_eq!(renamed.backups.len(), 1);
            assert!(renamed.backups[0].exists());
            let output = root.path().join(format!("out-{ext}"));
            crate::archive::extract(&path, "a.png", Some("secret-fixture"), &output, 1024).unwrap();
            assert_eq!(fs::read(output).unwrap(), b"second");
            assert_eq!(
                encrypted_entries(&path, format, Some("secret-fixture"))
                    .unwrap()
                    .len(),
                2
            );
        }
    }

    #[test]
    fn recursive_directory_copy_move_cancel_and_delete_keep_other_paths() {
        let root = tempfile::tempdir().unwrap();
        let source = root.path().join("source");
        fs::create_dir_all(source.join("nested/empty")).unwrap();
        fs::write(source.join("nested/photo.jpg"), b"pixels").unwrap();
        let target = root.path().join("copy");
        copy_local_tree(&source, &target, 1024, &AtomicBool::new(false)).unwrap();
        assert_eq!(
            fs::read(target.join("nested/photo.jpg")).unwrap(),
            b"pixels"
        );
        assert!(target.join("nested/empty").is_dir());
        assert!(copy_local_tree(
            &source,
            &source.join("inside"),
            1024,
            &AtomicBool::new(false)
        )
        .is_err());
        let cancelled = root.path().join("cancelled");
        assert!(copy_local_tree(&source, &cancelled, 1024, &AtomicBool::new(true)).is_err());
        assert!(!cancelled.exists());
        let destination = root.path().join("destination");
        fs::create_dir(&destination).unwrap();
        let moved = paste_local(&[target], &destination, true, false);
        assert!(moved.failures.is_empty());
        assert!(destination.join("copy/nested/empty").is_dir());
        assert!(delete_local(&[destination.join("copy")], true)
            .failures
            .is_empty());
        assert_eq!(
            fs::read(source.join("nested/photo.jpg")).unwrap(),
            b"pixels"
        );
        #[cfg(windows)]
        {
            assert!(operation_path(Path::new("C:\\")).is_err());
        }
    }

    #[test]
    fn clipboard_cache_survives_export_scope_and_respects_pins_and_budget() {
        let root = tempfile::tempdir().unwrap();
        let source = root.path().join("source.png");
        fs::write(&source, b"original bytes").unwrap();
        let cache = root.path().join("clipboard");
        let exported = export_clipboard_cache(&[Source::Local(source)], &cache, &[]).unwrap();
        let paths = exported.paths.clone();
        drop(exported);
        assert_eq!(fs::read(&paths[0]).unwrap(), b"original bytes");
        let cleaned = clear_clipboard_cache(&cache, &paths).unwrap();
        assert_eq!(cleaned.removed, 0);
        assert!(paths[0].exists());
        assert!(prune_cache_to(&cache, &paths, 0, false).is_err());
        assert!(paths[0].exists());
        let cleaned = clear_clipboard_cache(&cache, &[]).unwrap();
        assert_eq!(cleaned.removed, 1);
        assert!(!paths[0].exists());
        let stage = cache.join(".rustview-export-abandoned");
        fs::create_dir(&stage).unwrap();
        write_cache_marker(
            &stage,
            &CacheMarker {
                version: 1,
                created: 0,
                bytes: 1,
                complete: false,
                owner: 0,
            },
        )
        .unwrap();
        prune_clipboard_cache(&cache, &[]).unwrap();
        assert!(!stage.exists());
        let unknown = cache.join("export-personal");
        fs::create_dir(&unknown).unwrap();
        fs::write(unknown.join("personal.txt"), b"keep").unwrap();
        clear_clipboard_cache(&cache, &[]).unwrap();
        assert!(unknown.join("personal.txt").exists());
    }

    #[test]
    fn file_copy_rename_move_and_conflict_preserve_originals() {
        let root = tempfile::tempdir().unwrap();
        let source = root.path().join("source.png");
        fs::write(&source, b"original").unwrap();
        let folder = root.path().join("target");
        fs::create_dir(&folder).unwrap();
        let copy = folder.join("source.png");
        copy_to(&Source::Local(source.clone()), &copy, false, 1024).unwrap();
        assert!(copy_to(&Source::Local(source.clone()), &copy, false, 1024).is_err());
        assert_eq!(fs::read(&source).unwrap(), b"original");
        let renamed = rename_local(&copy, "renamed.png").unwrap();
        assert!(renamed.exists());
        let result = paste_local(&[source.clone()], &folder, true, false);
        assert!(result.failures.is_empty());
        assert!(!source.exists());
        assert_eq!(fs::read(copy).unwrap(), b"original");
    }

    #[test]
    fn archive_copy_is_original_entry_bytes_and_rar_write_is_rejected() {
        let root = tempfile::tempdir().unwrap();
        let fixtures = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
        let source = Source::Archive {
            path: fixtures.join("encrypted.zip"),
            entry: "image.png".into(),
            password: Some("fixture-password".into()),
        };
        let output = root.path().join("image.png");
        copy_to(&source, &output, false, 1024).unwrap();
        assert_eq!(
            fs::read(output).unwrap(),
            fs::read(fixtures.join("image.png")).unwrap()
        );
        let rar = root.path().join("test.rar");
        fs::copy(fixtures.join("image.rar"), &rar).unwrap();
        let before = fs::read(&rar).unwrap();
        assert!(edit_archive(&rar, &ArchiveEdit::Delete(vec!["test.tga".into()]), None).is_err());
        assert_eq!(fs::read(rar).unwrap(), before);
    }

    #[test]
    fn zip_and_7z_edits_have_backups_and_keep_encryption() {
        let root = tempfile::tempdir().unwrap();
        let fixtures = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
        for extension in ["zip", "7z"] {
            let archive = root.path().join(format!("book.{extension}"));
            fs::copy(fixtures.join(format!("encrypted.{extension}")), &archive).unwrap();
            let original = fs::read(&archive).unwrap();
            let result = edit_archive(
                &archive,
                &ArchiveEdit::Rename {
                    entry: "image.png".into(),
                    new_name: "renamed.png".into(),
                },
                Some("fixture-password"),
            )
            .unwrap();
            assert_eq!(fs::read(result.backup).unwrap(), original);
            let extra = root.path().join("extra.png");
            fs::copy(fixtures.join("image.png"), &extra).unwrap();
            edit_archive(
                &archive,
                &ArchiveEdit::Add(vec![extra]),
                Some("fixture-password"),
            )
            .unwrap();
            let output = root.path().join("extracted");
            assert!(matches!(
                crate::archive::extract(&archive, "extra.png", None, &output, 1024),
                Err(crate::archive::ArchiveError::PasswordRequired)
            ));
            crate::archive::extract(
                &archive,
                "extra.png",
                Some("fixture-password"),
                &output,
                1024,
            )
            .unwrap();
            edit_archive(
                &archive,
                &ArchiveEdit::Delete(vec!["renamed.png".into()]),
                Some("fixture-password"),
            )
            .unwrap();
            assert_eq!(
                crate::archive::list(&archive, Some("fixture-password"))
                    .unwrap()
                    .len(),
                1
            );
        }
    }

    #[test]
    fn failed_archive_edit_does_not_change_original() {
        let root = tempfile::tempdir().unwrap();
        let archive = root.path().join("book.zip");
        fs::copy(
            Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/encrypted.zip"),
            &archive,
        )
        .unwrap();
        let original = fs::read(&archive).unwrap();
        assert!(edit_archive(
            &archive,
            &ArchiveEdit::Rename {
                entry: "image.png".into(),
                new_name: "other.png".into()
            },
            Some("wrong")
        )
        .is_err());
        assert_eq!(fs::read(archive).unwrap(), original);
    }

    #[test]
    fn moving_archive_entry_keeps_backup_and_rar_is_not_partially_moved() {
        let root = tempfile::tempdir().unwrap();
        let destination = root.path().join("out");
        fs::create_dir(&destination).unwrap();
        let fixtures = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
        let archive = root.path().join("book.zip");
        fs::copy(fixtures.join("plain.zip"), &archive).unwrap();
        let original = fs::read(&archive).unwrap();
        let result = move_into(
            &[Source::Archive {
                path: archive.clone(),
                entry: "image.png".into(),
                password: None,
            }],
            &destination,
            false,
            1024,
        );
        assert!(result.failures.is_empty(), "{:?}", result.failures);
        assert_eq!(result.completed.len(), 1);
        assert_eq!(result.backups.len(), 1);
        assert_eq!(fs::read(&result.backups[0]).unwrap(), original);
        assert!(crate::archive::list(&archive, None).unwrap().is_empty());
        let rar = root.path().join("book.rar");
        fs::copy(fixtures.join("image.rar"), &rar).unwrap();
        let result = move_into(
            &[Source::Archive {
                path: rar,
                entry: "test.tga".into(),
                password: None,
            }],
            &destination,
            false,
            1024,
        );
        assert_eq!(result.failures.len(), 1);
        assert!(!destination.join("test.tga").exists());
    }

    #[test]
    fn multi_move_never_overwrites_another_selected_file() {
        let root = tempfile::tempdir().unwrap();
        for name in ["a", "b", "out"] {
            fs::create_dir(root.path().join(name)).unwrap();
        }
        let first = root.path().join("a/same.png");
        let second = root.path().join("b/same.png");
        fs::write(&first, b"first").unwrap();
        fs::write(&second, b"second").unwrap();
        let result = move_into(
            &[Source::Local(first), Source::Local(second.clone())],
            &root.path().join("out"),
            true,
            1024,
        );
        assert_eq!(result.completed.len(), 1);
        assert_eq!(result.failures.len(), 1);
        assert!(second.exists());
        assert_eq!(
            fs::read(root.path().join("out/same.png")).unwrap(),
            b"first"
        );
    }

    #[test]
    fn file_list_never_contains_archive_password() {
        let root = tempfile::tempdir().unwrap();
        let output = root.path().join("files.txt");
        write_file_list(
            &[Source::Archive {
                path: root.path().join("book.zip"),
                entry: "folder/image.png".into(),
                password: Some("never-save-this-password".into()),
            }],
            &output,
        )
        .unwrap();
        let text = fs::read_to_string(output).unwrap();
        assert!(text.contains("folder/image.png"));
        assert!(!text.contains("never-save-this-password"));
    }

    #[cfg(windows)]
    #[test]
    fn local_datetime_roundtrip_and_only_selected_timestamp_changes() {
        let text = "2024-02-29 12:34:56";
        let seconds = parse_local_datetime(text).unwrap();
        let time = UNIX_EPOCH + Duration::from_secs(seconds as u64);
        assert_eq!(format_local_datetime(time), text);
        assert!(parse_local_datetime("2024-02-30 12:34:56").is_err());
        assert!(parse_local_datetime("2024-02-29 25:00:00").is_err());
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("time.png");
        fs::write(&path, b"data").unwrap();
        let created = path.metadata().unwrap().created().unwrap();
        let result = set_times(
            &[path.clone()],
            &Times {
                modified: Some(seconds),
                ..Default::default()
            },
        );
        assert!(result.failures.is_empty());
        let metadata = path.metadata().unwrap();
        assert_eq!(format_local_datetime(metadata.modified().unwrap()), text);
        assert_eq!(metadata.created().unwrap(), created);
    }
}

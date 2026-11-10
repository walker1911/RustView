use std::collections::HashSet;
use std::fmt;
use std::fs::{self, File};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitStatus, Stdio};
use std::thread::{self, JoinHandle};

const MAX_INDEX_BYTES: u64 = 16 * 1024 * 1024;
const MAX_ENTRIES: usize = 100_000;
const MAX_NAME_BYTES: usize = 4096;
const MAX_ERROR_BYTES: usize = 64 * 1024;
const MAX_PASSWORD_BYTES: usize = 1024;

#[derive(Debug, Clone)]
pub struct ArchiveEntry {
    pub name: String,
    pub size: u64,
    pub modified:Option<i64>,
}

#[derive(Debug)]
pub enum ArchiveError {
    PasswordRequired,
    WrongPassword,
    Unsupported(String),
    Other(String),
}

impl fmt::Display for ArchiveError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::PasswordRequired => write!(f, "压缩包需要密码"),
            Self::WrongPassword => write!(f, "压缩包密码不正确"),
            Self::Unsupported(message) | Self::Other(message) => f.write_str(message),
        }
    }
}

impl std::error::Error for ArchiveError {}

impl From<io::Error> for ArchiveError {
    fn from(error: io::Error) -> Self {
        Self::Other(error.to_string())
    }
}

pub fn engine_path() -> Result<PathBuf, ArchiveError> {
    let executable = std::env::current_exe()?;
    let executable_dir = executable.parent().unwrap_or(Path::new("."));
    let candidates = [
        Some(executable_dir.join("native/7zip/7z.exe")),
        Some(executable_dir.join("7zip/7z.exe")),
        cfg!(debug_assertions)
            .then(|| Path::new(env!("CARGO_MANIFEST_DIR")).join("native/7zip/7z.exe")),
    ];
    candidates
        .into_iter()
        .flatten()
        .find(|path| path.is_file() && path.with_file_name("7z.dll").is_file())
        .ok_or_else(|| ArchiveError::Other("缺少随程序提供的 7-Zip 引擎".into()))
}

fn archive_path(path: &Path) -> Result<PathBuf, ArchiveError> {
    let name = path
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| ArchiveError::Unsupported("压缩包文件名无效".into()))?
        .to_ascii_lowercase();
    let (stem, extension) = name.rsplit_once('.').unwrap_or((name.as_str(), ""));
    if !matches!(extension, "zip" | "cbz" | "rar" | "cbr" | "7z" | "cb7") {
        return Err(ArchiveError::Unsupported(
            "当前仅支持 ZIP、RAR 和 7z 单卷压缩包".into(),
        ));
    }
    if matches!(extension, "rar" | "cbr") {
        if let Some((_, number)) = stem.rsplit_once(".part") {
            if !number.is_empty() && number.bytes().all(|byte| byte.is_ascii_digit()) {
                return Err(ArchiveError::Unsupported("暂不支持分卷压缩包".into()));
            }
        }
    }
    let path = path.canonicalize()?;
    if !path.is_file() {
        return Err(ArchiveError::Unsupported("压缩包路径不是文件".into()));
    }
    Ok(path)
}

fn validate_entry(name: &str) -> Result<(), ArchiveError> {
    if name.is_empty()
        || name.len() > MAX_NAME_BYTES
        || name.contains(['\0', '\r', '\n', ':'])
        || name
            .split(['/', '\\'])
            .any(|part| part.is_empty() || part == "." || part == "..")
    {
        return Err(ArchiveError::Unsupported("不支持该压缩包条目路径".into()));
    }
    Ok(())
}

fn format_switch(path: &Path) -> Result<&'static str, ArchiveError> {
    let mut signature = [0_u8; 8];
    let mut file = File::open(path)?;
    let count = file.read(&mut signature)?;
    let signature = &signature[..count];
    if signature.starts_with(b"PK\x03\x04") || signature.starts_with(b"PK\x05\x06") {
        Ok("-tzip")
    } else if signature.starts_with(b"Rar!\x1a\x07\x00") {
        Ok("-tRar")
    } else if signature.starts_with(b"Rar!\x1a\x07\x01\x00") {
        Ok("-tRar5")
    } else if signature.starts_with(b"7z\xbc\xaf\x27\x1c") {
        Ok("-t7z")
    } else if signature.starts_with(b"PK\x07\x08") {
        Err(ArchiveError::Unsupported("暂不支持分卷压缩包".into()))
    } else {
        Err(ArchiveError::Unsupported(
            "文件内容不是常规 ZIP、RAR 或 7z 压缩包；暂不支持自解压包（SFX）".into(),
        ))
    }
}

struct EngineProcess {
    child: Child,
    errors: Option<JoinHandle<Vec<u8>>>,
    #[cfg(windows)]
    _job:crate::worker::Job,
}

impl EngineProcess {
    fn spawn(
        operation: &str,
        path: &Path,
        entry: Option<&str>,
        password: Option<&str>,
    ) -> Result<Self, ArchiveError> {
        if let Some(password) = password {
            if password.len() > MAX_PASSWORD_BYTES || password.contains(['\0', '\r', '\n']) {
                return Err(ArchiveError::Unsupported(
                    "密码过长或包含不支持的换行字符".into(),
                ));
            }
        }
        let format = format_switch(path)?;
        let mut command = Command::new(engine_path()?);
        command.args([operation, format, "-sccUTF-8", "-bd", "-bsp0", "-bse2"]);
        if operation == "l" {
            command.arg("-slt");
        } else {
            command.args(["-so", "-spd", "-ssc", "-r-"]);
            // -i! uses an include rule, so a filename starting with @ is not a list file.
            // -spd and -ssc make it a literal, case-sensitive name rather than a wildcard.
            command.arg(format!("-i!{}", entry.unwrap_or_default()));
        }
        command.arg("--").arg(path);
        command
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            command.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
        }
        let mut child = command.spawn()?;
        #[cfg(windows)]
        let job=match crate::worker::Job::attach(&child){Ok(j)=>j,Err(e)=>{let _=child.kill();let _=child.wait();return Err(ArchiveError::Other(e));}};
        let mut stderr = child.stderr.take().unwrap();
        let errors = thread::spawn(move || {
            let mut saved = Vec::new();
            let mut buffer = [0_u8; 4096];
            while let Ok(count) = stderr.read(&mut buffer) {
                if count == 0 {
                    break;
                }
                let keep = count.min(MAX_ERROR_BYTES.saturating_sub(saved.len()));
                saved.extend_from_slice(&buffer[..keep]);
            }
            saved
        });
        let mut process = Self {
            child,
            errors: Some(errors),
            #[cfg(windows)]
            _job:job,
        };
        if let Some(mut stdin) = process.child.stdin.take() {
            if let Some(password) = password {
                // Do not use -p: the password stays out of command lines and process listings.
                // A bare -p means an empty password in 7-Zip and disables its stdin prompt.
                let result = stdin
                    .write_all(password.as_bytes())
                    .and_then(|()| stdin.write_all(b"\n"));
                if let Err(error) = result {
                    if error.kind() != io::ErrorKind::BrokenPipe {
                        return Err(error.into());
                    }
                }
            }
        }
        Ok(process)
    }

    fn finish(mut self) -> Result<(ExitStatus, Vec<u8>), ArchiveError> {
        let status = self.child.wait()?;
        let errors = self.errors.take().unwrap().join().unwrap_or_default();
        Ok((status, errors))
    }
}

impl Drop for EngineProcess {
    fn drop(&mut self) {
        if !matches!(self.child.try_wait(), Ok(Some(_))) {
            let _ = self.child.kill();
            let _ = self.child.wait();
        }
        if let Some(errors) = self.errors.take() {
            let _ = errors.join();
        }
    }
}

fn command_error(status: ExitStatus, errors: &[u8], password: Option<&str>) -> ArchiveError {
    let message = String::from_utf8_lossy(errors);
    let lower = message.to_ascii_lowercase();
    if lower.contains("password") || (status.code() == Some(255) && password.is_none()) {
        return if password.is_some() {
            ArchiveError::WrongPassword
        } else {
            ArchiveError::PasswordRequired
        };
    }
    ArchiveError::Other(if message.trim().is_empty() {
        format!("7-Zip 操作失败（退出码 {:?}）", status.code())
    } else {
        message.trim().to_owned()
    })
}

fn copy_bounded(
    input: &mut impl Read,
    output: &mut impl Write,
    limit: u64,
) -> Result<u64, ArchiveError> {
    let mut buffer = [0_u8; 64 * 1024];
    let mut total = 0_u64;
    loop {
        let count = input.read(&mut buffer)?;
        if count == 0 {
            return Ok(total);
        }
        total = total
            .checked_add(count as u64)
            .ok_or_else(|| ArchiveError::Unsupported("条目大小溢出".into()))?;
        if total > limit {
            return Err(ArchiveError::Unsupported(format!(
                "压缩包读取超过允许的 {} MiB 上限",
                limit / (1024 * 1024)
            )));
        }
        output.write_all(&buffer[..count])?;
    }
}

pub fn list(path: &Path, password: Option<&str>) -> Result<Vec<ArchiveEntry>, ArchiveError> {
    let path = archive_path(path)?;
    let mut process = EngineProcess::spawn("l", &path, None, password)?;
    let mut output = Vec::new();
    copy_bounded(
        process.child.stdout.as_mut().unwrap(),
        &mut output,
        MAX_INDEX_BYTES,
    )?;
    let (status, errors) = process.finish()?;
    if !status.success() {
        return Err(command_error(status, &errors, password));
    }
    let text = String::from_utf8(output)
        .map_err(|_| ArchiveError::Unsupported("压缩包索引不是有效 UTF-8".into()))?;
    parse_listing(&text)
}

fn parse_listing(text: &str) -> Result<Vec<ArchiveEntry>, ArchiveError> {
    let normalized = text.replace("\r\n", "\n");
    let (header, files) = normalized
        .split_once("\n----------\n")
        .ok_or_else(|| ArchiveError::Other("7-Zip 没有返回有效的条目索引".into()))?;
    let mut supported = false;
    for line in header.lines() {
        if let Some(kind) = line.strip_prefix("Type = ") {
            supported = matches!(
                kind.trim().to_ascii_lowercase().as_str(),
                "zip" | "7z" | "rar" | "rar5"
            );
        }
        if let Some((key, value)) = line.split_once(" = ") {
            if matches!(key, "Volume" | "Multivolume") && value.trim() == "+"
                || key == "Volumes" && value.trim().parse::<u64>().is_ok_and(|count| count > 1)
            {
                return Err(ArchiveError::Unsupported("暂不支持分卷压缩包".into()));
            }
        }
    }
    if !supported {
        return Err(ArchiveError::Unsupported(
            "文件内容不是受支持的 ZIP、RAR 或 7z 压缩包".into(),
        ));
    }
    let mut entries = Vec::new();
    let mut names = HashSet::new();
    for record in files.split("\n\n") {
        let mut name = None;
        let mut size = None;
        let mut modified=None;
        let mut directory = false;
        let mut link = false;
        let mut keys = HashSet::new();
        for line in record.lines().filter(|line| !line.trim().is_empty()) {
            let (key, value) = line
                .split_once(" = ")
                .ok_or_else(|| ArchiveError::Unsupported("不支持含换行符的压缩包条目".into()))?;
            if !keys.insert(key) {
                return Err(ArchiveError::Unsupported("压缩包条目索引有重复字段".into()));
            }
            match key {
                "Modified"=>modified=value.get(..19).and_then(|v|crate::fileops::parse_local_datetime(v).ok()),
                "Path" => name = Some(value),
                "Size" => {
                    size = Some(
                        value
                            .parse::<u64>()
                            .map_err(|_| ArchiveError::Unsupported("压缩包条目大小无效".into()))?,
                    )
                }
                "Folder" => directory = value == "+",
                "Attributes" => {
                    directory |= value.starts_with('D');
                    link |= value
                        .split_whitespace()
                        .any(|part| part.starts_with('l') && part.len() == 10);
                }
                "Symbolic Link" | "Hard Link" => link = !value.is_empty(),
                _ => {}
            }
        }
        if directory || link {
            continue;
        }
        if let Some(name) = name {
            validate_entry(name)?;
            if !names.insert(name.to_owned()) {
                return Err(ArchiveError::Unsupported(
                    "压缩包含同名条目，无法唯一选择".into(),
                ));
            }
            if entries.len() == MAX_ENTRIES {
                return Err(ArchiveError::Unsupported(format!(
                    "压缩包条目超过 {MAX_ENTRIES} 个上限"
                )));
            }
            entries.push(ArchiveEntry {
                name: name.to_owned(),
                size: size.ok_or_else(|| ArchiveError::Unsupported("压缩包条目缺少大小".into()))?,
                modified,
            });
        }
    }
    Ok(entries)
}

pub fn extract(
    path: &Path,
    entry: &str,
    password: Option<&str>,
    destination: &Path,
    max_bytes: u64,
) -> Result<(), ArchiveError> {
    validate_entry(entry)?;
    let path = archive_path(path)?;
    let mut process = EngineProcess::spawn("x", &path, Some(entry), password)?;
    let result = (|| {
        // The caller supplies a dedicated temporary destination. Archive paths never become disk paths.
        let mut output = File::create(destination)?;
        copy_bounded(
            process.child.stdout.as_mut().unwrap(),
            &mut output,
            max_bytes,
        )?;
        output.flush()?;
        let (status, errors) = process.finish()?;
        if !status.success() {
            return Err(command_error(status, &errors, password));
        }
        Ok(())
    })();
    if result.is_err() {
        let _ = fs::remove_file(destination);
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    struct Fixture(PathBuf);
    impl Fixture {
        fn new() -> Self {
            static COUNT: AtomicU64 = AtomicU64::new(0);
            let root = std::env::temp_dir().canonicalize().unwrap().join(format!(
                "rustview-archive-test-{}-{}-{}",
                std::process::id(),
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_nanos(),
                COUNT.fetch_add(1, Ordering::Relaxed)
            ));
            fs::create_dir(&root).unwrap();
            Self(root)
        }
        fn archive(&self, extension: &str, password: Option<&str>) -> PathBuf {
            let name = "@中文图像.txt";
            let file = self.0.join(name);
            fs::write(&file, b"archive payload\nsecond line").unwrap();
            let archive = self.0.join(format!("fixture.{extension}"));
            let mut command = Command::new(engine_path().unwrap());
            command.args(["a", &format!("-t{extension}")]);
            if let Some(password) = password {
                command.arg(format!("-p{password}"));
                if extension == "7z" {
                    command.arg("-mhe=on");
                }
            }
            command.arg("--").arg(&archive).arg(&file);
            #[cfg(windows)]
            {
                use std::os::windows::process::CommandExt;
                command.creation_flags(0x0800_0000);
            }
            let result = command.output().unwrap();
            assert!(
                result.status.success(),
                "{}",
                String::from_utf8_lossy(&result.stderr)
            );
            archive
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn rejects_escape_paths_and_volume_names() {
        for name in [
            "../x.png",
            "/x.png",
            "C:\\x.png",
            "a\\..\\x.png",
            "a\nb.png",
            "x:stream",
        ] {
            assert!(validate_entry(name).is_err(), "{name}");
        }
        assert!(validate_entry("nested/@[1]*?.png").is_ok());
        assert!(matches!(
            archive_path(Path::new("a.part01.rar")),
            Err(ArchiveError::Unsupported(_))
        ));
        assert!(matches!(
            archive_path(Path::new("a.7z.001")),
            Err(ArchiveError::Unsupported(_))
        ));
    }

    #[test]
    fn bounded_copy_never_writes_past_limit() {
        let mut result = Vec::new();
        assert!(copy_bounded(&mut io::Cursor::new(vec![1; 200_000]), &mut result, 70_000).is_err());
        assert!(result.len() <= 70_000);
    }

    #[test]
    fn plain_zip_extracts_exact_at_name_and_enforces_limit() {
        let fixture = Fixture::new();
        let archive = fixture.archive("zip", None);
        let entries = list(&archive, None).unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].name, "@中文图像.txt");
        let output = fixture.0.join("output");
        extract(&archive, &entries[0].name, None, &output, 1024).unwrap();
        assert_eq!(fs::read(&output).unwrap(), b"archive payload\nsecond line");
        assert!(extract(&archive, &entries[0].name, None, &output, 4).is_err());
        assert!(!output.exists());
    }

    #[test]
    fn encrypted_zip_and_header_encrypted_7z_use_stdin_password() {
        for extension in ["zip", "7z"] {
            let fixture = Fixture::new();
            // 7-Zip's ZIP writer intentionally restricts new passwords to ASCII.
            let password = if extension == "zip" {
                "fixture-password"
            } else {
                "密码🔑test"
            };
            let archive = fixture.archive(extension, Some(password));
            if extension == "7z" {
                assert!(matches!(
                    list(&archive, None),
                    Err(ArchiveError::PasswordRequired)
                ));
                assert!(matches!(
                    list(&archive, Some("wrong")),
                    Err(ArchiveError::WrongPassword)
                ));
            }
            let entries = list(&archive, Some(password)).unwrap();
            let output = fixture.0.join("output");
            assert!(matches!(
                extract(&archive, &entries[0].name, None, &output, 1024),
                Err(ArchiveError::PasswordRequired)
            ));
            assert!(matches!(
                extract(&archive, &entries[0].name, Some("wrong"), &output, 1024),
                Err(ArchiveError::WrongPassword)
            ));
            extract(&archive, &entries[0].name, Some(password), &output, 1024).unwrap();
            assert_eq!(fs::read(output).unwrap(), b"archive payload\nsecond line");
        }
    }

    #[test]
    fn upstream_rar4_rar5_and_solid_password_fixtures() {
        let fixture = Fixture::new();
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
        let plain = root.join("basic.rar");
        let entries = list(&plain, None).unwrap();
        assert_eq!(entries.len(), 2); // Directories and the symbolic link are excluded.
        extract(&plain, "test.txt", None, &fixture.0.join("plain"), 1024).unwrap();
        assert_eq!(
            fs::read(fixture.0.join("plain")).unwrap(),
            b"test text document\r\n"
        );
        for archive_name in [
            "test_read_format_rar4_encrypted_filenames.rar",
            "test_read_format_rar5_encrypted_filenames.rar",
            "test_read_format_rar5_solid_encrypted_filenames.rar",
        ] {
            let archive = root.join(archive_name);
            assert!(matches!(
                list(&archive, None),
                Err(ArchiveError::PasswordRequired)
            ));
            assert!(matches!(
                list(&archive, Some("wrong")),
                Err(ArchiveError::WrongPassword)
            ));
            let entries = list(&archive, Some("password")).unwrap();
            assert_eq!(entries.len(), 4);
            let output = fixture.0.join("rar-output");
            extract(&archive, "b.txt", Some("password"), &output, 1024).unwrap();
            assert_eq!(fs::read(output).unwrap(), b"This is from b.txt");
        }
    }

    #[test]
    fn wildcard_and_case_sensitive_names_are_selected_literally() {
        let fixture = Fixture::new();
        let archive = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/wildcards.zip");
        assert_eq!(list(&archive, None).unwrap().len(), 5);
        let output = fixture.0.join("literal");
        for (name, expected) in [
            ("literal*.txt", b"wanted wildcard".as_slice()),
            ("@file.txt", b"at entry".as_slice()),
            ("Case.txt", b"upper case".as_slice()),
            ("case.txt", b"lower case".as_slice()),
        ] {
            extract(&archive, name, None, &output, 1024).unwrap();
            assert_eq!(fs::read(&output).unwrap(), expected);
        }
    }

    #[test]
    fn rejects_renamed_tar_and_accepts_empty_zip() {
        let fixture = Fixture::new();
        let disguised = fixture.0.join("renamed-tar.zip");
        let mut tar = [0_u8; 1024];
        tar[257..263].copy_from_slice(b"ustar\0");
        fs::write(&disguised, tar).unwrap();
        assert!(matches!(
            list(&disguised, None),
            Err(ArchiveError::Unsupported(_))
        ));
        assert!(matches!(
            extract(
                &disguised,
                "image.png",
                None,
                &fixture.0.join("output"),
                1024
            ),
            Err(ArchiveError::Unsupported(_))
        ));
        let empty_zip = fixture.0.join("empty.zip");
        let mut eocd = [0_u8; 22];
        eocd[..4].copy_from_slice(b"PK\x05\x06");
        fs::write(&empty_zip, eocd).unwrap();
        assert!(list(&empty_zip, None).unwrap().is_empty());
    }

    #[test]
    fn rejects_multivolume_and_ambiguous_listing() {
        assert!(matches!(
            parse_listing("Type = Rar\nVolumes = 2\n----------\n"),
            Err(ArchiveError::Unsupported(_))
        ));
        assert!(parse_listing(
            "Path = a----------b.zip\nType = zip\n----------\nPath = a.png\nSize = 1\n\n"
        )
        .is_ok());
        assert!(parse_listing(
            "Type = zip\n----------\nPath = a.png\nSize = 1\n\nPath = a.png\nSize = 1\n"
        )
        .is_err());
    }
}

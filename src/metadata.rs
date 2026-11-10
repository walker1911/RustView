//! Metadata parsing and container updates are delegated to the bundled ExifTool.
//! Run these calls in the existing memory-limited worker, not the UI thread.
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{
    collections::BTreeMap,
    fs::{self, File},
    io::{Read, Write},
    path::{Path, PathBuf},
    process::{Command, Stdio},
    time::{Duration, Instant, SystemTime},
};

const MAX_INPUT: u64 = 512 * 1024 * 1024;
const MAX_JSON: usize = 8 * 1024 * 1024;
const MAX_TEXT: usize = 1024 * 1024;

#[derive(Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct Values {
    pub title: String,
    pub description: String,
    pub keywords: Vec<String>,
    pub iptc_keywords: Vec<String>,
    pub rating: Option<u8>,
    pub label: Option<String>,
    pub creator: String,
    pub copyright: String,
    pub datetime_original: String,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct Metadata {
    pub fields: BTreeMap<String, Value>,
    pub values: Values,
    pub sidecar: Option<PathBuf>,
}

#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub enum Destination {
    #[default]
    Auto,
    Embedded,
    Sidecar,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct WriteResult {
    pub destination: PathBuf,
    pub backup: Option<PathBuf>,
}

pub fn sidecar_path(path: &Path) -> PathBuf {
    path.with_extension("xmp")
}

fn engine() -> Result<PathBuf, String> {
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    for ancestor in exe.ancestors().skip(1).take(4) {
        let path = ancestor.join("native/ExifTool/exiftool.exe");
        if path.is_file() {
            return Ok(path);
        }
    }
    Err("缺少 native/ExifTool/exiftool.exe 及 exiftool_files，请完整解压便携包".into())
}

fn checked_path(path: &Path) -> Result<PathBuf, String> {
    let path = path.canonicalize().map_err(|e| e.to_string())?;
    let metadata = fs::metadata(&path).map_err(|e| e.to_string())?;
    if !metadata.is_file() || metadata.len() > MAX_INPUT {
        return Err("元数据读取只接受512 MiB以内的图片文件".into());
    }
    let name = path
        .file_name()
        .and_then(|x| x.to_str())
        .ok_or("路径不是有效Unicode文件名")?;
    let extension = path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    if !crate::imaging::is_supported_image(name)
        && !matches!(
            extension.as_str(),
            "xmp" | "heic" | "heif" | "avif" | "jxl" | "psd" | "psb"
        )
    {
        return Err("元数据工具只接受图像或XMP，不包含音视频支持".into());
    }
    Ok(path)
}

fn read_bounded(mut reader: impl Read, limit: usize) -> Result<Vec<u8>, String> {
    let mut bytes = Vec::new();
    reader
        .by_ref()
        .take(limit as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    if bytes.len() > limit {
        return Err("ExifTool输出超过预算".into());
    }
    Ok(bytes)
}

fn run(arguments: &[String]) -> Result<Vec<u8>, String> {
    if arguments.iter().any(|a| a.contains(['\r', '\n', '\0'])) {
        return Err("元数据参数包含无效的换行或空字符".into());
    }
    let mut command = Command::new(engine()?);
    command
        .args(["-config", "", "-charset", "filename=utf8", "-@", "-"])
        .env("LC_ALL", "C")
        .env("LC_CTYPE", "C")
        .env("LANG", "C")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x08000000);
    }
    let mut child = command
        .spawn()
        .map_err(|e| format!("无法启动ExifTool：{e}"))?;
    let stdout = child.stdout.take().ok_or("元数据输出管道未建立")?;
    let stderr = child.stderr.take().ok_or("元数据错误管道未建立")?;
    let out = std::thread::spawn(move || read_bounded(stdout, MAX_JSON));
    let err = std::thread::spawn(move || read_bounded(stderr, 64 * 1024));
    let input = arguments.join("\n") + "\n";
    if child
        .stdin
        .take()
        .ok_or("元数据输入管道未建立")?
        .write_all(input.as_bytes())
        .is_err()
    {
        let _ = child.kill();
        let _ = child.wait();
        return Err("元数据输入管道已关闭".into());
    }
    let started = Instant::now();
    let status = loop {
        if started.elapsed() > Duration::from_secs(120) {
            let _ = child.kill();
            let _ = child.wait();
            return Err("元数据处理超过120秒，已停止".into());
        }
        match child.try_wait() {
            Ok(Some(s)) => break s,
            Ok(None) => std::thread::sleep(Duration::from_millis(10)),
            Err(e) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(e.to_string());
            }
        }
    };
    let output = out.join().map_err(|_| "元数据输出线程失败")??;
    let errors = err.join().map_err(|_| "元数据错误线程失败")??;
    if !status.success() {
        return Err(format!(
            "ExifTool处理失败：{}",
            String::from_utf8_lossy(&errors)
                .chars()
                .take(1500)
                .collect::<String>()
        ));
    }
    Ok(output)
}

fn read_fields(path: &Path) -> Result<BTreeMap<String, Value>, String> {
    let bytes = run(&[
        "-j".into(),
        "-G1".into(),
        "-s".into(),
        "-struct".into(),
        "-a".into(),
        "--".into(),
        path.to_string_lossy().into_owned(),
    ])?;
    let mut rows: Vec<BTreeMap<String, Value>> =
        serde_json::from_slice(&bytes).map_err(|_| "ExifTool返回了无效JSON")?;
    if rows.len() != 1 {
        return Err("元数据结果数量错误".into());
    }
    let fields = rows.remove(0);
    if fields.keys().any(|k| k.ends_with(":Error") || k == "Error") {
        return Err("ExifTool无法解析此图像的元数据".into());
    }
    if fields.len() > 8192 {
        return Err("元数据字段超过8192项".into());
    }
    Ok(fields)
}

fn text(value: &Value) -> String {
    match value {
        Value::String(s) => s.clone(),
        Value::Number(n) => n.to_string(),
        Value::Object(o) => o
            .get("x-default")
            .or_else(|| o.get("val"))
            .map(text)
            .unwrap_or_default(),
        Value::Array(a) => a.first().map(text).unwrap_or_default(),
        _ => String::new(),
    }
}
fn first(fields: &BTreeMap<String, Value>, keys: &[&str]) -> String {
    keys.iter()
        .find_map(|k| fields.get(*k).map(text).filter(|s| !s.is_empty()))
        .unwrap_or_default()
}
fn list(fields: &BTreeMap<String, Value>, keys: &[&str]) -> Vec<String> {
    keys.iter()
        .find_map(|k| {
            fields.get(*k).map(|v| match v {
                Value::Array(a) => a.iter().map(text).filter(|s| !s.is_empty()).collect(),
                _ => {
                    let s = text(v);
                    if s.is_empty() {
                        Vec::new()
                    } else {
                        vec![s]
                    }
                }
            })
        })
        .unwrap_or_default()
}
fn values(fields: &BTreeMap<String, Value>) -> Values {
    let rating = first(fields, &["XMP-xmp:Rating", "XMP-acdsee:Rating"]);
    let label = first(fields, &["XMP-xmp:Label"]);
    Values {
        title: first(
            fields,
            &[
                "XMP-dc:Title",
                "XMP-acdsee:Caption",
                "IPTC:ObjectName",
                "IFD0:DocumentName",
            ],
        ),
        description: first(
            fields,
            &[
                "XMP-dc:Description",
                "XMP-acdsee:Notes",
                "IPTC:Caption-Abstract",
                "IFD0:ImageDescription",
            ],
        ),
        keywords: list(fields, &["XMP-acdsee:Keywords"]),
        iptc_keywords: list(fields, &["XMP-dc:Subject", "IPTC:Keywords"]),
        rating: rating.parse::<u8>().ok().filter(|v| *v <= 5),
        label: (!label.is_empty()).then_some(label),
        creator: first(
            fields,
            &[
                "XMP-dc:Creator",
                "XMP-acdsee:Author",
                "IPTC:By-line",
                "IFD0:Artist",
            ],
        ),
        copyright: first(
            fields,
            &["XMP-dc:Rights", "IPTC:CopyrightNotice", "IFD0:Copyright"],
        ),
        datetime_original: first(
            fields,
            &["XMP-exif:DateTimeOriginal", "ExifIFD:DateTimeOriginal"],
        ),
    }
}

pub fn read(path: &Path) -> Result<Metadata, String> {
    let path = checked_path(path)?;
    let mut fields = read_fields(&path)?;
    let sidecar = sidecar_path(&path);
    let mut snapshot = None;
    let sidecar = if sidecar != path && sidecar.is_file() {
        let side = read_fields(&checked_path(&sidecar)?)?;
        // Our sidecars contain a complete editable-field snapshot, including explicit clears.
        if first(&side, &["XMP-xmp:CreatorTool"]) == "RustView" {
            snapshot = Some(values(&side));
        }
        for (key, value) in side {
            if key.starts_with("XMP-") {
                fields.insert(key, value);
            }
        }
        Some(sidecar)
    } else {
        None
    };
    let values = snapshot.unwrap_or_else(|| values(&fields));
    Ok(Metadata {
        fields,
        values,
        sidecar,
    })
}

fn validate(values: &Values) -> Result<(), String> {
    let strings = [
        &values.title,
        &values.description,
        &values.creator,
        &values.copyright,
        &values.datetime_original,
    ];
    if strings
        .iter()
        .any(|s| s.len() > MAX_TEXT || s.contains('\0'))
        || values.keywords.len() > 4096
        || values.iptc_keywords.len() > 4096
        || values
            .keywords
            .iter()
            .chain(&values.iptc_keywords)
            .any(|s| s.len() > 65536 || s.contains('\0'))
        || values
            .label
            .as_ref()
            .is_some_and(|s| s.len() > 65536 || s.contains('\0'))
    {
        return Err("元数据文字过长、关键词过多或包含空字符".into());
    }
    if values.rating.is_some_and(|r| r > 5) {
        return Err("评级必须在0–5之间".into());
    }
    if !values.datetime_original.is_empty() {
        let date = &values.datetime_original;
        let bytes = date.as_bytes();
        if bytes.len() < 19
            || !date.is_ascii()
            || !matches!(bytes[4], b':' | b'-')
            || !matches!(bytes[7], b':' | b'-')
            || !matches!(bytes[10], b' ' | b'T')
            || bytes[13] != b':'
            || bytes[16] != b':'
        {
            return Err("拍摄时间请使用YYYY:MM:DD HH:MM:SS格式，可带时区".into());
        }
        let n = |a, b| date[a..b].parse::<u32>().unwrap_or(u32::MAX);
        let (y, m, d, h, minute, sec) =
            (n(0, 4), n(5, 7), n(8, 10), n(11, 13), n(14, 16), n(17, 19));
        let leap = y % 4 == 0 && (y % 100 != 0 || y % 400 == 0);
        let days = match m {
            1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
            4 | 6 | 9 | 11 => 30,
            2 => {
                if leap {
                    29
                } else {
                    28
                }
            }
            _ => 0,
        };
        if y == 0 || m == 0 || d == 0 || d > days || h > 23 || minute > 59 || sec > 59 {
            return Err("拍摄日期时间无效".into());
        }
    }
    Ok(())
}

fn json_tags(values: &Values, path: &Path, embedded: bool) -> Value {
    let mut tags = serde_json::Map::new();
    tags.insert(
        "SourceFile".into(),
        Value::String(path.to_string_lossy().into_owned()),
    );
    if !embedded {
        tags.insert(
            "XMP-xmp:CreatorTool".into(),
            Value::String("RustView".into()),
        );
    }
    for (tag, value) in [
        ("XMP-dc:Title", &values.title),
        ("XMP-dc:Description", &values.description),
        ("XMP-dc:Rights", &values.copyright),
    ] {
        tags.insert(tag.into(), Value::String(value.clone()));
    }
    let as_list = |values: &Vec<String>| {
        if values.is_empty() {
            Value::String(String::new())
        } else {
            serde_json::json!(values)
        }
    };
    tags.insert("XMP-acdsee:Keywords".into(), as_list(&values.keywords));
    tags.insert("XMP-dc:Subject".into(), as_list(&values.iptc_keywords));
    tags.insert(
        "XMP-acdsee:Caption".into(),
        Value::String(values.title.clone()),
    );
    tags.insert(
        "XMP-acdsee:Notes".into(),
        Value::String(values.description.clone()),
    );
    tags.insert(
        "XMP-acdsee:Author".into(),
        Value::String(values.creator.clone()),
    );
    tags.insert(
        "XMP-acdsee:Rating".into(),
        values
            .rating
            .map(|r| serde_json::json!(r))
            .unwrap_or_else(|| Value::String(String::new())),
    );
    tags.insert(
        "XMP-dc:Creator".into(),
        if values.creator.is_empty() {
            Value::String(String::new())
        } else {
            serde_json::json!([values.creator])
        },
    );
    tags.insert(
        "XMP-xmp:Rating".into(),
        values
            .rating
            .map(|r| serde_json::json!(r))
            .unwrap_or_else(|| Value::String(String::new())),
    );
    tags.insert(
        "XMP-xmp:Label".into(),
        Value::String(values.label.clone().unwrap_or_default()),
    );
    tags.insert(
        "XMP-exif:DateTimeOriginal".into(),
        Value::String(values.datetime_original.clone()),
    );
    if embedded {
        for (tag, value) in [
            ("IPTC:ObjectName", &values.title),
            ("IPTC:Caption-Abstract", &values.description),
            ("IPTC:By-line", &values.creator),
            ("IPTC:CopyrightNotice", &values.copyright),
        ] {
            tags.insert(tag.into(), Value::String(value.clone()));
        }
        tags.insert("IPTC:Keywords".into(), as_list(&values.iptc_keywords));
        tags.insert(
            "IPTC:CodedCharacterSet".into(),
            Value::String("UTF8".into()),
        );
        // JPEG/TIFF support actual EXIF tags. PNG/WebP still preserve the XMP fields above.
        if matches!(
            path.extension()
                .and_then(|e| e.to_str())
                .unwrap_or("")
                .to_ascii_lowercase()
                .as_str(),
            "jpg" | "jpeg" | "tif" | "tiff"
        ) {
            for (tag, value) in [
                ("IFD0:DocumentName", &values.title),
                ("IFD0:ImageDescription", &values.description),
                ("IFD0:Artist", &values.creator),
                ("IFD0:Copyright", &values.copyright),
                ("ExifIFD:DateTimeOriginal", &values.datetime_original),
            ] {
                tags.insert(tag.into(), Value::String(value.clone()));
            }
        }
    }
    Value::Array(vec![Value::Object(tags)])
}

fn stamp(path: &Path) -> Result<(u64, Option<SystemTime>), String> {
    let m = fs::metadata(path).map_err(|e| e.to_string())?;
    Ok((m.len(), m.modified().ok()))
}

fn backup(path: &Path) -> Result<PathBuf, String> {
    let folder = path
        .parent()
        .ok_or("文件缺少父目录")?
        .join(".rustview-originals");
    fs::create_dir_all(&folder).map_err(|e| e.to_string())?;
    let target = folder.join(path.file_name().ok_or("文件名为空")?);
    if target.exists() {
        if !target.is_file() {
            return Err("原图备份路径被文件夹占用".into());
        }
        return Ok(target);
    }
    let mut temporary = tempfile::Builder::new()
        .prefix(".metadata-backup-")
        .tempfile_in(&folder)
        .map_err(|e| e.to_string())?;
    std::io::copy(
        &mut File::open(path).map_err(|e| e.to_string())?,
        &mut temporary,
    )
    .map_err(|e| e.to_string())?;
    temporary.as_file().sync_all().map_err(|e| e.to_string())?;
    match temporary.persist_noclobber(&target) {
        Ok(_) => Ok(target),
        Err(_e) if target.is_file() => Ok(target),
        Err(e) => Err(e.to_string()),
    }
}

pub fn write(
    path: &Path,
    values: &Values,
    destination: Destination,
) -> Result<WriteResult, String> {
    validate(values)?;
    let mut normalized = values.clone();
    if !normalized.datetime_original.is_empty() {
        let date = &normalized.datetime_original;
        normalized.datetime_original = format!(
            "{}:{}:{} {}",
            &date[..4],
            &date[5..7],
            &date[8..10],
            &date[11..]
        );
    }
    let values = &normalized;
    let path = checked_path(path)?;
    let mut signature = Vec::with_capacity(4);
    File::open(&path)
        .map_err(|e| e.to_string())?
        .take(4)
        .read_to_end(&mut signature)
        .map_err(|e| e.to_string())?;
    if crate::imaging::is_photoshop(&path.to_string_lossy()) || signature == b"8BPS" {
        return Err("PSD/PSB只读，不写入文档属性；评级等整理信息可保存在本地目录库".into());
    }
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    let embedded = match destination {
        Destination::Sidecar => false,
        Destination::Auto => {
            !crate::imaging::is_raw(&path.to_string_lossy())
                && matches!(
                    ext.as_str(),
                    "jpg" | "jpeg" | "png" | "webp" | "tif" | "tiff" | "gif"
                )
        }
        Destination::Embedded => {
            if crate::imaging::is_raw(&path.to_string_lossy())
                || !matches!(
                    ext.as_str(),
                    "jpg" | "jpeg" | "png" | "webp" | "tif" | "tiff" | "gif"
                )
            {
                return Err(
                    "此格式不原位写入元数据，请使用XMP sidecar；RAW原始数据保持不变".into(),
                );
            }
            true
        }
    };
    let destination = if embedded {
        path.clone()
    } else {
        sidecar_path(&path)
    };
    let before = destination
        .exists()
        .then(|| stamp(&destination))
        .transpose()?;
    if destination.exists() && !destination.is_file() {
        return Err("元数据目标不是文件".into());
    }
    let parent = destination.parent().ok_or("目标缺少父目录")?;
    let suffix = if embedded {
        format!(".{ext}")
    } else {
        ".xmp".to_owned()
    };
    let temporary = tempfile::Builder::new()
        .prefix(".rustview-metadata-")
        .suffix(&suffix)
        .tempfile_in(parent)
        .map_err(|e| e.to_string())?
        .into_temp_path();
    if destination.is_file() {
        fs::copy(&destination, &temporary).map_err(|e| e.to_string())?;
    } else {
        fs::write(&temporary,b"<?xpacket begin='' id='W5M0MpCehiHzreSzNTczkc9d'?><x:xmpmeta xmlns:x='adobe:ns:meta/'><rdf:RDF xmlns:rdf='http://www.w3.org/1999/02/22-rdf-syntax-ns#'><rdf:Description rdf:about=''/></rdf:RDF></x:xmpmeta><?xpacket end='w'?>").map_err(|e|e.to_string())?;
    }
    let instructions = tempfile::Builder::new()
        .prefix("rustview-tags-")
        .suffix(".json")
        .tempfile()
        .map_err(|e| e.to_string())?;
    let json =
        serde_json::to_vec(&json_tags(values, &temporary, embedded)).map_err(|e| e.to_string())?;
    if json.len() > MAX_JSON {
        return Err("写入元数据超过8 MiB预算".into());
    }
    fs::write(instructions.path(), json).map_err(|e| e.to_string())?;
    run(&[
        "-overwrite_original".into(),
        "-P".into(),
        "-f".into(),
        "-api".into(),
        "MissingTagValue^=".into(),
        format!("-j={}", instructions.path().display()),
        "--".into(),
        temporary.to_string_lossy().into_owned(),
    ])?;
    let read_back = values_from_file(&temporary)?;
    if read_back.title != values.title
        || read_back.description != values.description
        || read_back.keywords != values.keywords
        || read_back.rating != values.rating
        || read_back.label != values.label
        || read_back.creator != values.creator
        || read_back.copyright != values.copyright
        || read_back.iptc_keywords != values.iptc_keywords
        || read_back.datetime_original != values.datetime_original
    {
        return Err("ExifTool写入结果与输入不一致，原文件未替换".into());
    }
    fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(&temporary)
        .and_then(|f| f.sync_all())
        .map_err(|e| e.to_string())?;
    if before
        != destination
            .exists()
            .then(|| stamp(&destination))
            .transpose()?
    {
        return Err("文件在写入期间被其它程序修改，已保留原文件".into());
    }
    let backup = if before.is_some() {
        Some(backup(&destination)?)
    } else {
        None
    };
    if before.is_some() {
        temporary.persist(&destination).map_err(|e| e.to_string())?;
    } else {
        temporary
            .persist_noclobber(&destination)
            .map_err(|e| e.to_string())?;
    }
    Ok(WriteResult {
        destination,
        backup,
    })
}
fn values_from_file(path: &Path) -> Result<Values, String> {
    Ok(values(&read_fields(path)?))
}

#[cfg(test)]
mod tests {
    use super::*;
    fn sample() -> Values {
        Values {
            title: "中文标题\nsecond line".into(),
            description: "说明\n多行".into(),
            keywords: vec!["风景".into(), "night".into()],
            iptc_keywords: vec!["相机".into()],
            rating: Some(4),
            label: Some("Red".into()),
            creator: "摄影师".into(),
            copyright: "Copyright 2026".into(),
            datetime_original: "2026:11:10 18:23:04".into(),
        }
    }
    #[test]
    fn embedded_jpeg_roundtrip_preserves_pixels_and_first_backup() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("中文照片.jpg");
        image::RgbImage::from_pixel(3, 2, image::Rgb([20, 60, 90]))
            .save(&path)
            .unwrap();
        let original = fs::read(&path).unwrap();
        let decoded = image::open(&path).unwrap().into_rgb8();
        let first = write(&path, &sample(), Destination::Auto).unwrap();
        assert_eq!(fs::read(first.backup.unwrap()).unwrap(), original);
        let result = read(&path).unwrap();
        assert!(result.values.title == sample().title);
        assert!(result.values.iptc_keywords == sample().iptc_keywords);
        assert_eq!(result.values.datetime_original, "2026:11:10 18:23:04");
        assert_eq!(image::open(&path).unwrap().into_rgb8(), decoded);
        let mut second = sample();
        second.title = "第二次".into();
        second.datetime_original = "2026-11-10T18:23:04".into();
        write(&path, &second, Destination::Embedded).unwrap();
        assert_eq!(
            read(&path).unwrap().values.datetime_original,
            "2026:11:10 18:23:04"
        );
        assert_eq!(
            fs::read(dir.path().join(".rustview-originals/中文照片.jpg")).unwrap(),
            original
        );
    }
    #[test]
    fn raw_uses_xmp_and_never_modifies_original() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("拍摄.dng");
        fs::copy(
            Path::new(env!("CARGO_MANIFEST_DIR")).join("native/LibRaw/testdata/synthetic.dng"),
            &path,
        )
        .unwrap();
        let original = fs::read(&path).unwrap();
        let result = write(&path, &sample(), Destination::Auto).unwrap();
        assert_eq!(
            result.destination,
            path.with_extension("xmp").canonicalize().unwrap()
        );
        assert!(result.backup.is_none());
        assert_eq!(fs::read(&path).unwrap(), original);
        assert!(read(&path).unwrap().values.title == sample().title);
        write(&path, &Values::default(), Destination::Auto).unwrap();
        assert!(read(&path).unwrap().values == Values::default());
        assert_eq!(fs::read(&path).unwrap(), original);
        assert!(write(&path, &sample(), Destination::Embedded).is_err());
    }
    #[test]
    fn invalid_values_leave_source_untouched() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("test.png");
        image::RgbaImage::from_pixel(1, 1, image::Rgba([1, 2, 3, 4]))
            .save(&path)
            .unwrap();
        let original = fs::read(&path).unwrap();
        let mut values = sample();
        values.datetime_original = "2026:02:30 10:20:30".into();
        assert!(write(&path, &values, Destination::Auto).is_err());
        assert_eq!(fs::read(path).unwrap(), original);
    }
    #[test]
    fn photoshop_metadata_write_is_rejected_but_reading_remains_available() {
        let fixtures = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/psd");
        for (fixture, name) in [
            ("layered.psd", "document.PSD"),
            ("layered.psb", "document.PSB"),
            ("layered.psd", "renamed.jpg"),
        ] {
            let directory = tempfile::tempdir().unwrap();
            let path = directory.path().join(name);
            fs::copy(fixtures.join(fixture), &path).unwrap();
            let original = fs::read(&path).unwrap();
            assert!(!read(&path).unwrap().fields.is_empty(), "{name}");
            for destination in [
                Destination::Auto,
                Destination::Embedded,
                Destination::Sidecar,
            ] {
                let error = write(&path, &sample(), destination).unwrap_err();
                assert!(error.contains("PSD/PSB只读"), "{name}: {error}");
                assert_eq!(fs::read(&path).unwrap(), original, "{name}");
                assert!(!sidecar_path(&path).exists(), "{name}");
                assert!(
                    !directory.path().join(".rustview-originals").exists(),
                    "{name}"
                );
            }
        }
    }
    #[test]
    fn source_fields_can_be_explicitly_cleared() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("clear.jpg");
        image::RgbImage::from_pixel(2, 2, image::Rgb([50, 60, 70]))
            .save(&path)
            .unwrap();
        write(&path, &sample(), Destination::Embedded).unwrap();
        write(&path, &Values::default(), Destination::Embedded).unwrap();
        assert!(read(&path).unwrap().values == Values::default());
    }
}

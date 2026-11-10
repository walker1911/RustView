use serde::{Deserialize, Serialize};
use std::{
    fs,
    io::{Read, Write},
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    time::{Duration, Instant},
};

pub const MAX_IMAGE_BYTES: usize = 512 * 1024 * 1024;
pub const MAX_INPUT_BYTES: u64 = 512 * 1024 * 1024;
pub const WORKER_MEMORY: usize = 2 * 1024 * 1024 * 1024;
pub const METADATA_MEMORY: usize = 256 * 1024 * 1024;
pub const MAX_ENTRIES: usize = 100_000;

#[derive(Clone, Serialize, Deserialize)]
#[serde(tag = "operation")]
pub enum Request {
    Unedited {
        source: Box<Request>,
    },
    MetadataRead {
        source: Box<Request>,
    },
    MetadataWrite {
        path: PathBuf,
        values: crate::metadata::Values,
        destination: crate::metadata::Destination,
    },
    Render {
        source: Box<Request>,
        edits: Vec<crate::editing::Edit>,
        settings: crate::precision::ExportSettings,
        raw: crate::precision::RawSettings,
        save: Option<PathBuf>,
    },
    Preview {
        source: Box<Request>,
        max_side: u32,
        region: Option<[u32; 4]>,
    },
    Clipboard {
        destination: PathBuf,
    },
    Page {
        source: Box<Request>,
        index: u32,
        last: bool,
    },
    ColorPreview {
        source: Box<Request>,
        managed: bool,
    },
    Thumbnail {
        source: Box<Request>,
    },
    Edit {
        source: Box<Request>,
        edits: Vec<crate::editing::Edit>,
        save: Option<PathBuf>,
    },
    List {
        path: PathBuf,
        password: Option<String>,
    },
    Load {
        path: PathBuf,
        entry: Option<String>,
        password: Option<String>,
        full_raw: bool,
    },
}

impl Request {
    pub fn set_password(&mut self, value: Option<String>) {
        match self {
            Self::List { password, .. } | Self::Load { password, .. } => *password = value,
            Self::Page { source, .. }
            | Self::Unedited { source }
            | Self::ColorPreview { source, .. }
            | Self::Thumbnail { source }
            | Self::Preview { source, .. }
            | Self::Edit { source, .. }
            | Self::MetadataRead { source }
            | Self::Render { source, .. } => source.set_password(value),
            Self::Clipboard { .. } | Self::MetadataWrite { .. } => {}
        }
    }
}

#[derive(Clone, Serialize, Deserialize)]
pub struct Entry {
    pub name: String,
    pub size: u64,
    pub directory: bool,
    pub archive: bool,
    #[serde(default)]
    pub modified: Option<i64>,
}

#[derive(Default, Serialize, Deserialize)]
pub struct Reply {
    #[serde(default)]
    pub editing_record: Option<crate::recipes::EditRecord>,
    pub metadata: Option<crate::metadata::Metadata>,
    pub metadata_write: Option<crate::metadata::WriteResult>,
    pub rendered: Option<crate::precision::RenderSummary>,
    #[serde(default)]
    pub source_width: u32,
    #[serde(default)]
    pub source_height: u32,
    #[serde(default)]
    pub page_index: u32,
    #[serde(default)]
    pub page_total: u32,
    pub entries: Option<Vec<Entry>>,
    pub width: u32,
    pub height: u32,
    pub preview_width: u32,
    pub preview_height: u32,
    pub note: String,
    pub error: Option<String>,
    pub password_required: bool,
}

pub struct Output {
    pub reply: Reply,
    pub rgba: Vec<u8>,
    pub preview: Vec<u8>,
}

pub fn is_archive(path: &Path) -> bool {
    matches!(
        path.extension()
            .and_then(|x| x.to_str())
            .unwrap_or("")
            .to_ascii_lowercase()
            .as_str(),
        "zip" | "cbz" | "rar" | "cbr" | "7z" | "cb7"
    )
}

pub(crate) fn archive_error(error: crate::archive::ArchiveError) -> Reply {
    use crate::archive::ArchiveError::*;
    let (password_required, message) = match error {
        PasswordRequired => (true, "压缩包需要密码".to_owned()),
        WrongPassword => (true, "密码不正确，请重新输入".to_owned()),
        Unsupported(s) | Other(s) => (false, s),
    };
    Reply {
        error: Some(message),
        password_required,
        ..Default::default()
    }
}

pub(crate) fn prepare_image(request: &Request, output: &Path) -> Result<PathBuf, Reply> {
    if let Request::ColorPreview { source, .. }
    | Request::Page { source, .. }
    | Request::Unedited { source } = request
    {
        return prepare_image(source, output);
    }
    let Request::Load {
        path,
        entry,
        password,
        ..
    } = request
    else {
        return Err(Reply {
            error: Some("读取图像需要 Load 请求".into()),
            ..Default::default()
        });
    };
    let input = if let Some(entry) = entry {
        let extension = Path::new(entry)
            .extension()
            .and_then(|x| x.to_str())
            .unwrap_or("img");
        let extension: String = extension
            .chars()
            .filter(|c| c.is_ascii_alphanumeric())
            .take(12)
            .collect();
        let input = output.join(format!("input.{extension}"));
        crate::archive::extract(path, entry, password.as_deref(), &input, MAX_INPUT_BYTES)
            .map_err(archive_error)?;
        input
    } else {
        path.clone()
    };
    let input_size = fs::metadata(&input).map(|m| m.len());
    if !matches!(input_size, Ok(size) if size <= MAX_INPUT_BYTES) {
        return Err(Reply {
            error: Some("文件不存在或超过当前单文件 512 MiB 读取限额".into()),
            ..Default::default()
        });
    }
    Ok(input)
}

fn decode_source(
    request: &Request,
    output: &Path,
) -> Result<(crate::imaging::DecodedImage, u32, u32), Reply> {
    fn settings(request: &Request) -> Result<(bool, bool, Option<(u32, bool)>), String> {
        match request {
            Request::Load { full_raw, .. } => Ok((*full_raw, true, None)),
            Request::Unedited { source } => settings(source),
            Request::ColorPreview { source, managed } => {
                let (full_raw, _, page) = settings(source)?;
                Ok((full_raw, *managed, page))
            }
            Request::Page {
                source,
                index,
                last,
            } => {
                let (full_raw, managed, _) = settings(source)?;
                Ok((full_raw, managed, Some((*index, *last))))
            }
            _ => Err("图像来源需要 Load、Page 或 ColorPreview 请求".into()),
        }
    }
    let (full_raw, managed, page) = settings(request).map_err(|error| Reply {
        error: Some(error),
        ..Default::default()
    })?;
    let input = prepare_image(request, output)?;
    let decoded = (|| -> Result<_, String> {
        let (mut image, index, total) = if let Some((index, last)) = page {
            let page = crate::pages::decode_with_options(
                &input,
                index,
                last,
                MAX_IMAGE_BYTES,
                managed,
                full_raw,
            )?;
            (
                crate::imaging::DecodedImage {
                    width: page.width,
                    height: page.height,
                    rgba: page.rgba,
                    note: format!("第 {}／{} 页", page.index + 1, page.total),
                },
                page.index,
                page.total,
            )
        } else {
            let decode = if managed {
                crate::imaging::decode
            } else {
                crate::imaging::decode_unmanaged
            };
            (
                decode(&input, 200_000_000, MAX_IMAGE_BYTES, full_raw)?,
                0,
                1,
            )
        };
        if !managed {
            image.note.push_str("；ICC颜色管理已关闭");
        }
        Ok((image, index, total))
    })();
    decoded.map_err(|error| Reply {
        error: Some(error),
        ..Default::default()
    })
}

fn write_source(
    output: &Path,
    decoded: (crate::imaging::DecodedImage, u32, u32),
) -> Result<Reply, String> {
    let (image, index, total) = decoded;
    let mut reply = write_image(output, image.width, image.height, image.rgba, image.note)?;
    reply.page_index = index;
    reply.page_total = total;
    Ok(reply)
}

fn source_record(request: &Request) -> Option<crate::recipes::EditRecord> {
    fn identity(request: &Request) -> Option<(&Path, Option<&str>)> {
        match request {
            Request::Load { path, entry, .. } => Some((path, entry.as_deref())),
            Request::Page { source, .. } | Request::ColorPreview { source, .. } => identity(source),
            _ => None,
        }
    }
    identity(request).and_then(|(path, entry)| crate::recipes::read_for(path, entry))
}

fn render_record_preview(
    input: &Path,
    index: u32,
    record: &crate::recipes::EditRecord,
    output: &Path,
    all_frames: bool,
) -> Result<PathBuf, String> {
    let mut settings = record.recipe.export.clone();
    settings.depth = 8;
    settings.animation = if all_frames {
        crate::precision::AnimationMode::AllFrames
    } else {
        crate::precision::AnimationMode::CurrentFrame
    };
    settings.format = if all_frames {
        crate::precision::OutputFormat::WebP
    } else {
        crate::precision::OutputFormat::Png
    };
    let mut edits = record.recipe.edits.clone();
    if edits
        .iter()
        .any(|edit| matches!(edit, crate::editing::Edit::ColorDepth { .. }))
    {
        edits.push(crate::editing::Edit::ColorDepth {
            mode: crate::editing::ColorDepth::Rgba8,
        });
    }
    let path = output.join(format!("record-preview.{}", settings.format.extension()));
    crate::precision::render(
        input,
        Some(index),
        &edits,
        &settings,
        &record.recipe.raw,
        output,
        &path,
    )?;
    Ok(path)
}

/// Stream preparation renders a saved recipe once, leaving frame iteration bounded.
pub(crate) fn prepare_recorded_input(
    request: &Request,
    output: &Path,
) -> Result<(PathBuf, Option<crate::recipes::EditRecord>), Reply> {
    let input = prepare_image(request, output)?;
    let record = source_record(request);
    let Some(record) = record else {
        return Ok((input, None));
    };
    let result = (|| -> Result<_, String> {
        let animated = crate::animation::open(&input, 128 * 1024 * 1024)?.is_some();
        let path = render_record_preview(&input, 0, &record, output, animated)?;
        Ok((path, Some(record)))
    })();
    result.map_err(|error| Reply {
        error: Some(error),
        ..Default::default()
    })
}

/// Normal browsing restores a record; explicit Edit/Render always start from the original.
fn decode_recorded_source(
    request: &Request,
    output: &Path,
) -> Result<
    (
        (crate::imaging::DecodedImage, u32, u32),
        Option<crate::recipes::EditRecord>,
    ),
    Reply,
> {
    let record = source_record(request);
    let Some(record) = record else {
        return decode_source(request, output).map(|image| (image, None));
    };
    let input = prepare_image(request, output)?;
    let result = (|| -> Result<_, String> {
        fn has_page(request: &Request) -> bool {
            match request {
                Request::Page { .. } => true,
                Request::ColorPreview { source, .. } => has_page(source),
                _ => false,
            }
        }
        fn managed(request: &Request) -> bool {
            match request {
                Request::ColorPreview { managed, .. } => *managed,
                Request::Page { source, .. } => managed(source),
                _ => true,
            }
        }
        let (index, total) = if has_page(request) {
            let (_, index, total) = decode_source(request, output)
                .map_err(|r| r.error.unwrap_or_else(|| "读取编辑记录所在页失败".into()))?;
            (index, total)
        } else {
            (0, 1)
        };
        let path = render_record_preview(&input, index, &record, output, false)?;
        let mut image = if managed(request) {
            crate::imaging::decode(&path, 200_000_000, MAX_IMAGE_BYTES, false)?
        } else {
            crate::imaging::decode_unmanaged(&path, 200_000_000, MAX_IMAGE_BYTES, false)?
        };
        image.note.push_str("；已恢复无损编辑记录（源文件未改变）");
        Ok(((image, index, total), Some(record)))
    })();
    result.map_err(|error| Reply {
        error: Some(error),
        ..Default::default()
    })
}

fn handle(request: Request, output: &Path) -> Reply {
    match request {
        Request::MetadataRead { source } => {
            let input = match prepare_image(&source, output) {
                Ok(p) => p,
                Err(r) => return r,
            };
            match crate::metadata::read(&input) {
                Ok(metadata) => Reply {
                    metadata: Some(metadata),
                    ..Default::default()
                },
                Err(e) => Reply {
                    error: Some(e),
                    ..Default::default()
                },
            }
        }
        Request::MetadataWrite {
            path,
            values,
            destination,
        } => {
            let record = crate::recipes::read_for(&path, None);
            let result = (|| -> Result<_, String> {
                let result = crate::metadata::write(&path, &values, destination)?;
                if let Some(record) = record {
                    let updated = crate::recipes::EditRecord::capture(&path, None, record.recipe)?;
                    updated.save(&crate::recipes::record_path(
                        &crate::recipes::data_root()?,
                        &path,
                        None,
                    )?)?;
                }
                Ok(result)
            })();
            match result {
                Ok(result) => Reply {
                    metadata_write: Some(result),
                    note: "源图元数据已保存".into(),
                    ..Default::default()
                },
                Err(e) => Reply {
                    error: Some(e),
                    ..Default::default()
                },
            }
        }
        Request::Render {
            source,
            mut edits,
            mut settings,
            raw,
            save,
        } => {
            let input = match prepare_image(&source, output) {
                Ok(p) => p,
                Err(r) => return r,
            };
            let result = (|| -> Result<Reply, String> {
                fn page(request: &Request) -> Option<u32> {
                    match request {
                        Request::Page { index, .. } => Some(*index),
                        Request::ColorPreview { source, .. } => page(source),
                        _ => None,
                    }
                }
                let exporting = save.is_some();
                let path = save.unwrap_or_else(|| output.join("render-preview.png"));
                if !exporting {
                    settings.format = crate::precision::OutputFormat::Png;
                    settings.depth = 8;
                    settings.animation = crate::precision::AnimationMode::CurrentFrame;
                }
                if !exporting
                    && edits
                        .iter()
                        .any(|e| matches!(e, crate::editing::Edit::ColorDepth { .. }))
                {
                    edits.push(crate::editing::Edit::ColorDepth {
                        mode: crate::editing::ColorDepth::Rgba8,
                    });
                }
                let summary = crate::precision::render(
                    &input,
                    page(&source),
                    &edits,
                    &settings,
                    &raw,
                    output,
                    &path,
                )?;
                if exporting {
                    return Ok(Reply {
                        note: summary.note.clone(),
                        rendered: Some(summary),
                        ..Default::default()
                    });
                }
                fn managed(request: &Request) -> bool {
                    match request {
                        Request::ColorPreview { managed, .. } => *managed,
                        Request::Page { source, .. } => managed(source),
                        _ => true,
                    }
                }
                let d = if managed(&source) {
                    crate::imaging::decode(&path, 200_000_000, MAX_IMAGE_BYTES, false)?
                } else {
                    crate::imaging::decode_unmanaged(&path, 200_000_000, MAX_IMAGE_BYTES, false)?
                };
                let mut reply =
                    write_image(output, d.width, d.height, d.rgba, summary.note.clone())?;
                reply.rendered = Some(summary);
                reply.page_index = page(&source).unwrap_or(0);
                reply.page_total = reply.page_index + 1;
                Ok(reply)
            })();
            result.unwrap_or_else(|e| Reply {
                error: Some(e),
                ..Default::default()
            })
        }
        Request::Preview {
            source,
            max_side,
            region,
        } => {
            if !(1..=4096).contains(&max_side) {
                return Reply {
                    error: Some("预览边长必须在1–4096之间".into()),
                    ..Default::default()
                };
            }
            let (d, index, total, record) =
                if matches!(&*source, Request::Render { save: None, .. }) {
                    let r = handle(*source, output);
                    if r.error.is_some() {
                        return r;
                    }
                    let rgba = match fs::read(output.join("pixels.rgba")) {
                        Ok(v) => v,
                        Err(e) => {
                            return Reply {
                                error: Some(e.to_string()),
                                ..Default::default()
                            }
                        }
                    };
                    (
                        crate::imaging::DecodedImage {
                            width: r.width,
                            height: r.height,
                            rgba,
                            note: r.note,
                        },
                        r.page_index,
                        r.page_total,
                        None,
                    )
                } else {
                    match decode_recorded_source(&source, output) {
                        Ok(((d, i, t), r)) => (d, i, t, r),
                        Err(r) => return r,
                    }
                };
            let result = (|| -> Result<Reply, String> {
                let (source_width, source_height) = (d.width, d.height);
                let image = image::RgbaImage::from_raw(d.width, d.height, d.rgba)
                    .ok_or("预览图尺寸无效")?;
                let [x, y, w, h] = region.unwrap_or([0, 0, source_width, source_height]);
                if x >= source_width || y >= source_height || w == 0 || h == 0 {
                    return Err("预览区域超出图片范围".into());
                }
                let (w, h) = (w.min(source_width - x), h.min(source_height - y));
                let ratio = (max_side as f64 / w as f64)
                    .min(max_side as f64 / h as f64)
                    .min(1.0);
                let (tw, th) = (
                    (w as f64 * ratio).round().max(1.0) as u32,
                    (h as f64 * ratio).round().max(1.0) as u32,
                );
                let view = image::imageops::crop_imm(&image, x, y, w, h);
                let thumb = image::imageops::thumbnail(&*view, tw, th);
                let mut reply = write_image(output, tw, th, thumb.into_raw(), d.note)?;
                reply.source_width = source_width;
                reply.source_height = source_height;
                reply.page_index = index;
                reply.page_total = total;
                reply.editing_record = record;
                Ok(reply)
            })();
            result.unwrap_or_else(|e| Reply {
                error: Some(e),
                ..Default::default()
            })
        }
        Request::Clipboard { destination } => {
            let result = (|| -> Result<Reply, String> {
                let image = crate::clipboard::read_image(MAX_IMAGE_BYTES, 200_000_000)?
                    .ok_or("剪贴板中没有可读取的图像")?;
                image::save_buffer_with_format(
                    destination,
                    &image.rgba,
                    image.width,
                    image.height,
                    image::ColorType::Rgba8,
                    image::ImageFormat::Png,
                )
                .map_err(|e| e.to_string())?;
                write_image(
                    output,
                    image.width,
                    image.height,
                    image.rgba,
                    "来自剪贴板的图像".into(),
                )
            })();
            result.unwrap_or_else(|e| Reply {
                error: Some(e),
                ..Default::default()
            })
        }
        Request::Thumbnail { source } => {
            let ((d, index, total), record) = match decode_recorded_source(&source, output) {
                Ok(d) => d,
                Err(r) => return r,
            };
            let result = (|| -> Result<Reply, String> {
                let image = image::RgbaImage::from_raw(d.width, d.height, d.rgba)
                    .ok_or("缩略图尺寸无效")?;
                let thumbnail = image::DynamicImage::ImageRgba8(image)
                    .thumbnail(d.width.min(128), d.height.min(128))
                    .into_rgba8();
                let mut reply = write_image(
                    output,
                    thumbnail.width(),
                    thumbnail.height(),
                    thumbnail.into_raw(),
                    d.note,
                )?;
                reply.page_index = index;
                reply.page_total = total;
                reply.editing_record = record;
                Ok(reply)
            })();
            result.unwrap_or_else(|e| Reply {
                error: Some(e),
                ..Default::default()
            })
        }
        Request::Edit {
            source,
            edits,
            save,
        } => {
            if edits.len() > 128 {
                return Reply {
                    error: Some("编辑步骤超过128步，请先保存副本".into()),
                    ..Default::default()
                };
            }
            let (decoded, index, total) = match decode_source(&source, output) {
                Ok(d) => d,
                Err(r) => return r,
            };
            let result = (|| -> Result<Reply, String> {
                let mut image = crate::editing::EditedImage {
                    width: decoded.width,
                    height: decoded.height,
                    rgba: decoded.rgba,
                    color_depth: crate::editing::ColorDepth::Rgba8,
                };
                for edit in &edits {
                    image = crate::editing::apply(image, edit)?;
                }
                if let Some(path) = save {
                    crate::editing::save_to(&image, &path)?;
                }
                let precision = image.color_depth.precision_note();
                let mut reply = write_image(
                    output,
                    image.width,
                    image.height,
                    image.rgba,
                    format!(
                        "{}；已应用 {} 步编辑；{}",
                        decoded.note,
                        edits.len(),
                        precision
                    ),
                )?;
                reply.page_index = index;
                reply.page_total = total;
                Ok(reply)
            })();
            result.unwrap_or_else(|e| Reply {
                error: Some(e),
                ..Default::default()
            })
        }
        Request::List { path, password } => {
            let mut entries = Vec::new();
            if path.is_dir() {
                let items = match fs::read_dir(&path) {
                    Ok(items) => items,
                    Err(e) => {
                        return Reply {
                            error: Some(e.to_string()),
                            ..Default::default()
                        }
                    }
                };
                for item in items {
                    let item = match item {
                        Ok(item) => item,
                        Err(_) => continue,
                    };
                    let metadata = match item.metadata() {
                        Ok(m) => m,
                        Err(_) => continue,
                    };
                    let name = item.file_name().to_string_lossy().into_owned();
                    let directory = metadata.is_dir();
                    let archive = is_archive(&item.path());
                    if directory || archive || crate::imaging::is_supported_image(&name) {
                        if entries.len() == MAX_ENTRIES {
                            return Reply {
                                error: Some("目录超过十万条，当前版本请打开更小的子目录".into()),
                                ..Default::default()
                            };
                        }
                        entries.push(Entry {
                            name,
                            size: metadata.len(),
                            directory,
                            archive,
                            modified: metadata
                                .modified()
                                .ok()
                                .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                                .and_then(|d| i64::try_from(d.as_secs()).ok()),
                        });
                    }
                }
            } else if is_archive(&path) {
                let items = match crate::archive::list(&path, password.as_deref()) {
                    Ok(items) => items,
                    Err(error) => return archive_error(error),
                };
                for item in items {
                    if crate::imaging::is_supported_image(&item.name) {
                        entries.push(Entry {
                            name: item.name,
                            size: item.size,
                            directory: false,
                            archive: false,
                            modified: item.modified,
                        });
                    }
                }
            } else {
                return Reply {
                    error: Some("请选择文件夹或 ZIP／RAR／7z 压缩包".into()),
                    ..Default::default()
                };
            }
            entries.sort_by(|a, b| {
                b.directory
                    .cmp(&a.directory)
                    .then_with(|| natord::compare(&a.name, &b.name))
            });
            Reply {
                entries: Some(entries),
                ..Default::default()
            }
        }
        Request::Load { .. }
        | Request::Page { .. }
        | Request::ColorPreview { .. }
        | Request::Unedited { .. } => {
            let (decoded, record) = match decode_recorded_source(&request, output) {
                Ok(d) => d,
                Err(r) => return r,
            };
            write_source(output, decoded)
                .map(|mut reply| {
                    reply.editing_record = record;
                    reply
                })
                .unwrap_or_else(|error| Reply {
                    error: Some(error),
                    ..Default::default()
                })
        }
    }
}

fn write_image(
    output: &Path,
    width: u32,
    height: u32,
    rgba: Vec<u8>,
    note: String,
) -> Result<Reply, String> {
    if rgba.len() > MAX_IMAGE_BYTES {
        return Err("编辑结果超过512 MiB预算".into());
    }
    fs::write(output.join("pixels.rgba"), &rgba).map_err(|e| e.to_string())?;
    let image = image::RgbaImage::from_raw(width, height, rgba).ok_or("图片尺寸错误")?;
    let preview = image::DynamicImage::ImageRgba8(image)
        .thumbnail(width.min(2048), height.min(2048))
        .into_rgba8();
    fs::write(output.join("preview.rgba"), preview.as_raw()).map_err(|e| e.to_string())?;
    Ok(Reply {
        width,
        height,
        preview_width: preview.width(),
        preview_height: preview.height(),
        note,
        ..Default::default()
    })
}

pub fn worker_main() {
    let Some(output) = std::env::args_os().nth(2).map(PathBuf::from) else {
        return;
    };
    let mut input = String::new();
    let reply = match std::io::stdin()
        .take(1024 * 1024)
        .read_to_string(&mut input)
    {
        Ok(_) => match serde_json::from_str::<Request>(&input) {
            Ok(request) => handle(request, &output),
            Err(e) => Reply {
                error: Some(format!("无效的读取请求：{e}")),
                ..Default::default()
            },
        },
        Err(e) => Reply {
            error: Some(e.to_string()),
            ..Default::default()
        },
    };
    if let Ok(json) = serde_json::to_vec(&reply) {
        let _ = fs::write(output.join("reply.json"), json);
    }
}

#[cfg(windows)]
pub(crate) struct Job(windows_sys::Win32::Foundation::HANDLE);

#[cfg(windows)]
impl Job {
    pub(crate) fn attach(child: &std::process::Child) -> Result<Self, String> {
        Self::attach_with_limit(child, WORKER_MEMORY)
    }
    pub(crate) fn attach_with_limit(
        child: &std::process::Child,
        memory: usize,
    ) -> Result<Self, String> {
        use std::os::windows::io::AsRawHandle;
        use windows_sys::Win32::{
            Foundation::{CloseHandle, HANDLE},
            System::JobObjects::*,
        };
        unsafe {
            let job = CreateJobObjectW(std::ptr::null(), std::ptr::null());
            if job.is_null() {
                return Err(std::io::Error::last_os_error().to_string());
            }
            let mut limits: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = std::mem::zeroed();
            limits.BasicLimitInformation.LimitFlags =
                JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE | JOB_OBJECT_LIMIT_JOB_MEMORY;
            limits.JobMemoryLimit = memory;
            if SetInformationJobObject(
                job,
                JobObjectExtendedLimitInformation,
                &limits as *const _ as *const _,
                std::mem::size_of_val(&limits) as u32,
            ) == 0
                || AssignProcessToJobObject(job, child.as_raw_handle() as HANDLE) == 0
            {
                let e = std::io::Error::last_os_error();
                CloseHandle(job);
                return Err(format!("无法设置解码进程内存限额：{e}"));
            }
            Ok(Self(job))
        }
    }
}

#[cfg(windows)]
impl Drop for Job {
    fn drop(&mut self) {
        unsafe {
            windows_sys::Win32::Foundation::CloseHandle(self.0);
        }
    }
}

pub fn execute(request: Request, cancel: Arc<AtomicBool>) -> Result<Output, String> {
    let directory = tempfile::Builder::new()
        .prefix("rustview-")
        .tempdir()
        .map_err(|e| e.to_string())?;
    let error_file =
        fs::File::create(directory.path().join("stderr.txt")).map_err(|e| e.to_string())?;
    let mut command = Command::new(std::env::current_exe().map_err(|e| e.to_string())?);
    command
        .arg("--worker")
        .arg(directory.path())
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::from(error_file));
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x08000000);
    }
    let mut child = command
        .spawn()
        .map_err(|e| format!("无法启动读取进程：{e}"))?;
    // The child blocks on stdin until it has been placed in the memory-limited job.
    let memory_limit = if matches!(
        &request,
        Request::MetadataRead { .. } | Request::MetadataWrite { .. }
    ) {
        METADATA_MEMORY
    } else {
        WORKER_MEMORY
    };
    #[cfg(windows)]
    let _job = match Job::attach_with_limit(&child, memory_limit) {
        Ok(job) => job,
        Err(e) => {
            let _ = child.kill();
            let _ = child.wait();
            return Err(e);
        }
    };
    let json = serde_json::to_vec(&request).map_err(|e| e.to_string())?;
    if let Some(mut stdin) = child.stdin.take() {
        if let Err(e) = stdin.write_all(&json) {
            let _ = child.kill();
            let _ = child.wait();
            return Err(e.to_string());
        }
    }
    let started = Instant::now();
    let exit = loop {
        if cancel.load(Ordering::Relaxed) || started.elapsed() > Duration::from_secs(120) {
            let _ = child.kill();
            let _ = child.wait();
            return Err(if cancel.load(Ordering::Relaxed) {
                "已取消".into()
            } else {
                "读取超时，已停止后台任务".into()
            });
        }
        match child.try_wait() {
            Ok(Some(exit)) => break exit,
            Ok(None) => std::thread::sleep(Duration::from_millis(15)),
            Err(e) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(e.to_string());
            }
        }
    };
    if !exit.success() {
        return Err(format!(
            "读取进程已停止，可能超过 {} MiB 工作内存限额或解码器发生错误；主窗口可继续使用",
            memory_limit / 1048576
        ));
    }
    let reply_file = directory.path().join("reply.json");
    if fs::metadata(&reply_file)
        .map(|m| m.len())
        .unwrap_or(u64::MAX)
        > 32 * 1024 * 1024
    {
        return Err("读取结果索引过大或未完整生成".into());
    }
    let reply: Reply = serde_json::from_slice(&fs::read(reply_file).map_err(|e| e.to_string())?)
        .map_err(|e| e.to_string())?;
    let rgba = read_pixels(
        directory.path().join("pixels.rgba"),
        reply.width,
        reply.height,
        MAX_IMAGE_BYTES,
    )?;
    let preview = read_pixels(
        directory.path().join("preview.rgba"),
        reply.preview_width,
        reply.preview_height,
        2048 * 2048 * 4,
    )?;
    Ok(Output {
        reply,
        rgba,
        preview,
    })
}

fn read_pixels(
    path: PathBuf,
    width: u32,
    height: u32,
    max_bytes: usize,
) -> Result<Vec<u8>, String> {
    if width == 0 || height == 0 {
        return Ok(Vec::new());
    }
    let expected = (width as usize)
        .checked_mul(height as usize)
        .and_then(|x| x.checked_mul(4))
        .ok_or("图片尺寸溢出")?;
    if expected > max_bytes
        || fs::metadata(&path).map_err(|e| e.to_string())?.len() != expected as u64
    {
        return Err("图片输出超过预算或数据长度错误".into());
    }
    fs::read(path).map_err(|e| e.to_string())
}

pub fn probe_main() {
    let args: Vec<String> = std::env::args().collect();
    let Some(path) = args.get(2).map(PathBuf::from) else {
        eprintln!("Usage: RustView --probe PATH [--entry NAME] [--password-stdin] [--full-raw]");
        std::process::exit(2)
    };
    let password = if args.iter().any(|a| a == "--password-stdin") {
        let mut s = String::new();
        let _ = std::io::stdin().take(16384).read_to_string(&mut s);
        Some(s.trim_end_matches(['\r', '\n']).to_owned())
    } else {
        None
    };
    let entry = args
        .iter()
        .position(|a| a == "--entry")
        .and_then(|i| args.get(i + 1))
        .cloned();
    let request = if path.is_dir() || (is_archive(&path) && entry.is_none()) {
        Request::List { path, password }
    } else {
        Request::Load {
            path,
            entry,
            password,
            full_raw: args.iter().any(|a| a == "--full-raw"),
        }
    };
    match execute(request, Arc::new(AtomicBool::new(false))) {
        Ok(output) => {
            println!("{}", serde_json::to_string(&output.reply).unwrap());
            if output.reply.error.is_some() {
                std::process::exit(1);
            }
        }
        Err(e) => {
            println!("{}", serde_json::json!({"error":e}));
            std::process::exit(1);
        }
    }
}

pub fn request_probe_main() {
    let mut json = Vec::new();
    let request = std::io::stdin()
        .take(1024 * 1024 + 1)
        .read_to_end(&mut json)
        .map_err(|e| e.to_string())
        .and_then(|_| {
            if json.len() > 1024 * 1024 {
                return Err("请求超过1 MiB".into());
            }
            serde_json::from_slice::<Request>(&json).map_err(|_| "无效的测试请求".to_owned())
        });
    let result = request.and_then(|request| execute(request, Arc::new(AtomicBool::new(false))));
    match result {
        Ok(output) => {
            println!("{}", serde_json::to_string(&output.reply).unwrap());
            if output.reply.error.is_some() {
                std::process::exit(1);
            }
        }
        Err(e) => {
            println!("{}", serde_json::json!({"error":e}));
            std::process::exit(1);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn edit_chain_saves_copy_with_actual_size_and_keeps_source() {
        let d = tempfile::tempdir().unwrap();
        let input = d.path().join("original.png");
        let saved = d.path().join("edited.png");
        image::RgbaImage::from_pixel(4, 2, image::Rgba([40, 80, 120, 200]))
            .save(&input)
            .unwrap();
        let before = fs::read(&input).unwrap();
        let r = handle(
            Request::Edit {
                source: Box::new(Request::Load {
                    path: input.clone(),
                    entry: None,
                    password: None,
                    full_raw: false,
                }),
                edits: vec![
                    crate::editing::Edit::Rotate90 { turns: 1 },
                    crate::editing::Edit::ColorDepth {
                        mode: crate::editing::ColorDepth::Gray4,
                    },
                ],
                save: Some(saved.clone()),
            },
            d.path(),
        );
        assert!(r.error.is_none(), "{:?}", r.error);
        assert_eq!((r.width, r.height), (2, 4));
        assert_eq!(before, fs::read(input).unwrap());
        let encoded = fs::read(saved).unwrap();
        assert_eq!((encoded[24], encoded[25]), (4, 0));
        assert!(r.note.contains("显式色深转换"));
    }
    #[test]
    fn page_request_does_not_change_to_neighbor_file() {
        use tiff::encoder::{colortype, TiffEncoder};
        let d = tempfile::tempdir().unwrap();
        let path = d.path().join("pages.tif");
        let mut e = TiffEncoder::new(fs::File::create(&path).unwrap()).unwrap();
        e.write_image::<colortype::RGB8>(1, 1, &[255, 0, 0])
            .unwrap();
        e.write_image::<colortype::RGB8>(1, 1, &[0, 255, 0])
            .unwrap();
        drop(e);
        let r = handle(
            Request::Page {
                source: Box::new(Request::Load {
                    path,
                    entry: None,
                    password: None,
                    full_raw: false,
                }),
                index: 1,
                last: false,
            },
            d.path(),
        );
        assert!(r.error.is_none(), "{:?}", r.error);
        assert_eq!((r.page_index, r.page_total), (1, 2));
        assert_eq!(
            fs::read(d.path().join("pixels.rgba")).unwrap(),
            [0, 255, 0, 255]
        );
    }
    #[test]
    fn small_images_are_not_upscaled_into_large_previews() {
        let folder = tempfile::tempdir().unwrap();
        let input = folder.path().join("small.png");
        image::RgbaImage::from_pixel(3, 2, image::Rgba([20, 40, 60, 128]))
            .save(&input)
            .unwrap();
        let reply = handle(
            Request::Load {
                path: input,
                entry: None,
                password: None,
                full_raw: false,
            },
            folder.path(),
        );
        assert!(reply.error.is_none(), "{:?}", reply.error);
        assert_eq!((reply.preview_width, reply.preview_height), (3, 2));
        assert_eq!(
            fs::metadata(folder.path().join("preview.rgba"))
                .unwrap()
                .len(),
            24
        );
    }
    #[test]
    fn preview_and_thumbnail_preserve_aspect_ratio_and_small_size() {
        let directory = tempfile::tempdir().unwrap();
        for (width, height, preview_size, thumbnail_size) in [
            (4096, 1024, (2048, 512), (128, 32)),
            (3, 20, (3, 20), (3, 20)),
        ] {
            let path = directory.path().join("aspect.png");
            image::RgbaImage::from_pixel(width, height, image::Rgba([30, 60, 90, 255]))
                .save(&path)
                .unwrap();
            let source = Request::Load {
                path,
                entry: None,
                password: None,
                full_raw: false,
            };
            let reply = handle(source.clone(), directory.path());
            assert!(reply.error.is_none(), "{:?}", reply.error);
            assert_eq!((reply.preview_width, reply.preview_height), preview_size);
            let reply = handle(
                Request::Thumbnail {
                    source: Box::new(source),
                },
                directory.path(),
            );
            assert!(reply.error.is_none(), "{:?}", reply.error);
            assert_eq!((reply.width, reply.height), thumbnail_size);
        }
    }
    #[test]
    fn edits_keep_selected_tiff_page_and_color_setting_in_either_wrapper_order() {
        use tiff::{
            encoder::{colortype, TiffEncoder},
            tags::Tag,
        };
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("profile-pages.tiff");
        let profile = moxcms::ColorProfile::new_adobe_rgb().encode().unwrap();
        let raw = [64, 120, 180, 255, 90, 150, 210, 255];
        let mut encoder = TiffEncoder::new(fs::File::create(&path).unwrap()).unwrap();
        encoder
            .write_image::<colortype::RGB8>(1, 1, &[255, 0, 0])
            .unwrap();
        let mut second = encoder.new_image::<colortype::RGB8>(2, 1).unwrap();
        second
            .encoder()
            .write_tag(Tag::IccProfile, profile.as_slice())
            .unwrap();
        second.write_data(&[64, 120, 180, 90, 150, 210]).unwrap();
        drop(encoder);
        let load = Request::Load {
            path,
            entry: None,
            password: None,
            full_raw: false,
        };
        let page = |source| Request::Page {
            source: Box::new(source),
            index: 1,
            last: false,
        };
        let unmanaged = |source| Request::ColorPreview {
            source: Box::new(source),
            managed: false,
        };
        for source in [page(unmanaged(load.clone())), unmanaged(page(load.clone()))] {
            let saved = directory.path().join("current-page.png");
            let reply = handle(
                Request::Edit {
                    source: Box::new(source),
                    edits: Vec::new(),
                    save: Some(saved.clone()),
                },
                directory.path(),
            );
            assert!(reply.error.is_none(), "{:?}", reply.error);
            assert_eq!(
                (
                    reply.width,
                    reply.height,
                    reply.page_index,
                    reply.page_total
                ),
                (2, 1, 1, 2)
            );
            assert_eq!(fs::read(directory.path().join("pixels.rgba")).unwrap(), raw);
            assert_eq!(image::open(saved).unwrap().into_rgba8().into_raw(), raw);
            assert!(reply.note.contains("ICC颜色管理已关闭"));
        }
        let reply = handle(page(load), directory.path());
        assert!(reply.error.is_none(), "{:?}", reply.error);
        let mut expected = raw;
        crate::imaging::apply_icc(&profile, &mut expected).unwrap();
        assert_ne!(expected, raw);
        assert_eq!(
            fs::read(directory.path().join("pixels.rgba")).unwrap(),
            expected
        );
    }
    #[test]
    fn wrapped_edit_and_thumbnail_preserve_archive_password_flag() {
        let directory = tempfile::tempdir().unwrap();
        let source = Request::Page {
            source: Box::new(Request::ColorPreview {
                managed: false,
                source: Box::new(Request::Load {
                    path: PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/aes.zip"),
                    entry: Some("image.png".into()),
                    password: None,
                    full_raw: false,
                }),
            }),
            index: 0,
            last: false,
        };
        for request in [
            Request::Edit {
                source: Box::new(source.clone()),
                edits: Vec::new(),
                save: None,
            },
            Request::Thumbnail {
                source: Box::new(source),
            },
        ] {
            let reply = handle(request, directory.path());
            assert!(reply.error.is_some());
            assert!(reply.password_required);
        }
    }
    #[test]
    fn media_are_not_archives() {
        assert!(is_archive(Path::new("a.CBZ")));
        assert!(!is_archive(Path::new("a.mp4")));
    }
    #[test]
    fn load_restores_record_thumbnail_uses_it_and_explicit_render_does_not_double_apply() {
        let directory = tempfile::tempdir().unwrap();
        let source = directory.path().join("source.png");
        image::RgbaImage::from_pixel(6, 4, image::Rgba([64, 64, 64, 255]))
            .save(&source)
            .unwrap();
        let mut recipe = crate::recipes::Recipe::default();
        recipe
            .edits
            .push(crate::editing::Edit::Exposure { ev: 1.0 });
        let record = crate::recipes::EditRecord::capture(&source, None, recipe.clone()).unwrap();
        let record_path =
            crate::recipes::record_path(&crate::recipes::data_root().unwrap(), &source, None)
                .unwrap();
        record.save(&record_path).unwrap();
        let request = Request::Load {
            path: source.clone(),
            entry: None,
            password: None,
            full_raw: false,
        };
        let reply = handle(request.clone(), directory.path());
        assert!(reply.error.is_none(), "{:?}", reply.error);
        assert!(reply.editing_record.is_some());
        let restored = fs::read(directory.path().join("pixels.rgba")).unwrap();
        assert!(restored[0] > 80 && restored[0] < 110);
        let reply = handle(
            Request::Thumbnail {
                source: Box::new(request.clone()),
            },
            directory.path(),
        );
        assert!(reply.error.is_none(), "{:?}", reply.error);
        assert_eq!(
            fs::read(directory.path().join("pixels.rgba")).unwrap(),
            restored
        );
        let reply = handle(
            Request::Render {
                source: Box::new(request.clone()),
                edits: recipe.edits,
                settings: recipe.export,
                raw: recipe.raw,
                save: None,
            },
            directory.path(),
        );
        assert!(reply.error.is_none(), "{:?}", reply.error);
        assert!(reply.editing_record.is_none());
        assert_eq!(
            fs::read(directory.path().join("pixels.rgba")).unwrap(),
            restored
        );
        let mut changed = fs::read(&source).unwrap();
        changed.push(0);
        fs::write(&source, changed).unwrap();
        let reply = handle(request, directory.path());
        assert!(reply.error.is_none(), "{:?}", reply.error);
        assert!(reply.editing_record.is_none());
        assert_eq!(
            fs::read(directory.path().join("pixels.rgba")).unwrap()[0],
            64
        );
        fs::remove_file(record_path).unwrap();
    }
    #[test]
    fn reject_inconsistent_pixel_output() {
        let t = tempfile::tempdir().unwrap();
        let p = t.path().join("p");
        fs::write(&p, [1, 2, 3]).unwrap();
        assert!(read_pixels(p, 1, 1, 4).is_err());
    }
    #[test]
    fn nested_render_preview_is_bounded_and_preserves_original_geometry_and_alpha() {
        let directory = tempfile::tempdir().unwrap();
        let input = directory.path().join("transparent.png");
        image::RgbaImage::from_pixel(12, 8, image::Rgba([30, 60, 90, 96]))
            .save(&input)
            .unwrap();
        let before = fs::read(&input).unwrap();
        let source = Request::Load {
            path: input.clone(),
            entry: None,
            password: None,
            full_raw: false,
        };
        let render = Request::Render {
            source: Box::new(source),
            edits: vec![crate::editing::Edit::Rotate90 { turns: 1 }],
            settings: Default::default(),
            raw: Default::default(),
            save: None,
        };
        let reply = handle(
            Request::Preview {
                source: Box::new(render),
                max_side: 3,
                region: None,
            },
            directory.path(),
        );
        assert!(reply.error.is_none(), "{:?}", reply.error);
        assert_eq!((reply.source_width, reply.source_height), (8, 12));
        assert_eq!((reply.width, reply.height), (2, 3));
        assert_eq!(
            fs::read(directory.path().join("pixels.rgba")).unwrap(),
            [30, 60, 90, 96].repeat(6)
        );
        assert_eq!(fs::read(input).unwrap(), before);
    }
    #[test]
    fn original_view_can_recover_from_an_over_budget_record_without_deleting_it() {
        let directory = tempfile::tempdir().unwrap();
        let input = directory.path().join("source.png");
        image::RgbaImage::from_pixel(2, 2, image::Rgba([30, 60, 90, 96]))
            .save(&input)
            .unwrap();
        let mut recipe = crate::recipes::Recipe::default();
        recipe.edits.push(crate::editing::Edit::Resize {
            width: 10000,
            height: 10000,
        });
        let path = crate::recipes::record_path(&crate::recipes::data_root().unwrap(), &input, None)
            .unwrap();
        crate::recipes::EditRecord::capture(&input, None, recipe)
            .unwrap()
            .save(&path)
            .unwrap();
        let source = Request::Load {
            path: input,
            entry: None,
            password: None,
            full_raw: false,
        };
        assert!(handle(source.clone(), directory.path()).error.is_some());
        let reply = handle(
            Request::Unedited {
                source: Box::new(source),
            },
            directory.path(),
        );
        assert!(reply.error.is_none(), "{:?}", reply.error);
        assert!(reply.editing_record.is_none());
        assert_eq!((reply.width, reply.height), (2, 2));
        assert!(path.exists());
        assert_eq!(
            fs::read(directory.path().join("pixels.rgba")).unwrap(),
            [30, 60, 90, 96].repeat(4)
        );
        fs::remove_file(path).unwrap();
    }
    #[test]
    fn metadata_only_write_keeps_valid_saved_processing_record() {
        let directory = tempfile::tempdir().unwrap();
        let input = directory.path().join("source.jpg");
        image::RgbImage::from_pixel(8, 6, image::Rgb([50, 80, 100]))
            .save(&input)
            .unwrap();
        let root = crate::recipes::data_root().unwrap();
        let mut recipe = crate::recipes::Recipe::default();
        recipe
            .edits
            .push(crate::editing::Edit::Exposure { ev: 1.0 });
        let record_path = crate::recipes::record_path(&root, &input, None).unwrap();
        crate::recipes::EditRecord::capture(&input, None, recipe.clone())
            .unwrap()
            .save(&record_path)
            .unwrap();
        let reply = handle(
            Request::MetadataWrite {
                path: input.clone(),
                values: crate::metadata::Values {
                    title: "保留处理记录".into(),
                    ..Default::default()
                },
                destination: crate::metadata::Destination::Embedded,
            },
            directory.path(),
        );
        assert!(reply.error.is_none(), "{:?}", reply.error);
        let record = crate::recipes::read_for(&input, None)
            .expect("metadata write must refresh a valid source stamp");
        assert_eq!(
            serde_json::to_value(record.recipe).unwrap(),
            serde_json::to_value(recipe).unwrap()
        );
        assert_eq!(
            crate::metadata::read(&input).unwrap().values.title,
            "保留处理记录"
        );
        fs::remove_file(record_path).unwrap();
    }
}

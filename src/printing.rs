//! One printer DC, moved to one background thread; decoding stays with the caller.
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, path::Path};
const STRIP_BYTES: usize = 16 * 1024 * 1024;
pub const TEXT_BYTES: usize = 16 * 1024;

#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub enum TextAlignment {
    Left,
    #[default]
    Center,
    Right,
}
#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub enum ImageFit {
    #[default]
    Fit,
    Fill,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct PrintSettings {
    pub rows: u32,
    pub columns: u32,
    pub margin_mm: f32,
    pub gap_mm: f32,
    pub header: String,
    pub footer: String,
    pub caption: String,
    pub font_points: f32,
    pub caption_lines: u32,
    pub alignment: TextAlignment,
    pub fit: ImageFit,
    pub copies_per_image: u32,
    pub image_width_mm: Option<f32>,
    pub image_height_mm: Option<f32>,
}
impl Default for PrintSettings {
    fn default() -> Self {
        Self {
            rows: 1,
            columns: 1,
            margin_mm: 10.0,
            gap_mm: 4.0,
            header: String::new(),
            footer: String::new(),
            caption: String::new(),
            font_points: 10.0,
            caption_lines: 2,
            alignment: TextAlignment::Center,
            fit: ImageFit::Fit,
            copies_per_image: 1,
            image_width_mm: None,
            image_height_mm: None,
        }
    }
}
impl PrintSettings {
    pub fn validate(&self) -> Result<(), String> {
        if self.rows == 0
            || self.columns == 0
            || self.rows > 20
            || self.columns > 20
            || self.copies_per_image == 0
            || self.copies_per_image > 1000
            || self.caption_lines > 20
            || !self.margin_mm.is_finite()
            || !(0.0..=100.0).contains(&self.margin_mm)
            || !self.gap_mm.is_finite()
            || !(0.0..=100.0).contains(&self.gap_mm)
            || !self.font_points.is_finite()
            || !(4.0..=120.0).contains(&self.font_points)
            || [&self.header, &self.footer, &self.caption]
                .iter()
                .any(|s| s.len() > 16384 || s.contains('\0'))
            || [self.image_width_mm, self.image_height_mm]
                .into_iter()
                .flatten()
                .any(|v| !v.is_finite() || !(1.0..=2000.0).contains(&v))
        {
            return Err("打印行列、边距、文字或尺寸设置无效".into());
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Fields {
    pub name: String,
    pub path: String,
    pub width: u32,
    pub height: u32,
    pub page: u32,
    pub pages: u32,
    pub date: String,
    pub time: String,
    pub title: String,
    pub description: String,
    pub extra: BTreeMap<String, String>,
}
impl Fields {
    pub fn from_path(path: &Path, width: u32, height: u32, page: u32, pages: u32) -> Self {
        let date = path
            .metadata()
            .ok()
            .and_then(|m| m.modified().ok())
            .map(crate::fileops::format_local_datetime)
            .unwrap_or_default();
        let (date, time) = date.split_once(' ').unwrap_or((&date, ""));
        Self {
            name: path
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default(),
            path: path.to_string_lossy().into_owned(),
            width,
            height,
            page,
            pages,
            date: date.to_owned(),
            time: time.to_owned(),
            ..Default::default()
        }
    }
    pub fn apply_metadata(&mut self, metadata: &crate::metadata::Metadata) {
        self.title = metadata.values.title.clone();
        self.description = metadata.values.description.clone();
        if !metadata.values.datetime_original.is_empty() {
            let value = &metadata.values.datetime_original;
            let (d, t) = value.split_once([' ', 'T']).unwrap_or((value, ""));
            self.date = d.to_owned();
            self.time = t.to_owned();
        }
        self.extra = metadata
            .fields
            .iter()
            .map(|(k, v)| {
                (
                    k.clone(),
                    match v {
                        serde_json::Value::String(s) => s.clone(),
                        _ => v.to_string(),
                    },
                )
            })
            .collect();
    }
}

pub fn field_token(command: u16) -> Option<&'static str> {
    Some(match command {
        13023 => "{page}",
        13024 => "{pages}",
        13026 => "{name}",
        13029 => "{width}",
        13030 => "{height}",
        13036 => "{date}",
        13037 => "{time}",
        _ => return None,
    })
}

pub fn expand_fields(template: &str, fields: &Fields) -> String {
    use std::borrow::Cow;
    fn append(output: &mut String, text: &str) -> bool {
        let available = TEXT_BYTES - output.len();
        if text.len() <= available {
            output.push_str(text);
            return true;
        }
        let mut end = available;
        while !text.is_char_boundary(end) {
            end -= 1;
        }
        output.push_str(&text[..end]);
        false
    }
    if template.is_empty() {
        return String::new();
    }
    // Bound capacity before expansion: truncating an 8 MiB String afterwards
    // would retain its allocation in every contact-sheet preview cell.
    let mut output = String::with_capacity(TEXT_BYTES);
    let mut rest = template;
    while let Some(start) = rest.find('{') {
        if !append(&mut output, &rest[..start]) {
            return output;
        }
        let after = &rest[start + 1..];
        let Some(end) = after.find('}') else {
            append(&mut output, &rest[start..]);
            return output;
        };
        let name = &after[..end];
        let value: Option<Cow<'_, str>> = match name {
            "name" | "filename" => Some(Cow::Borrowed(&fields.name)),
            "path" => Some(Cow::Borrowed(&fields.path)),
            "width" => Some(Cow::Owned(fields.width.to_string())),
            "height" => Some(Cow::Owned(fields.height.to_string())),
            "page" => Some(Cow::Owned(fields.page.to_string())),
            "pages" => Some(Cow::Owned(fields.pages.to_string())),
            "date" => Some(Cow::Borrowed(&fields.date)),
            "time" => Some(Cow::Borrowed(&fields.time)),
            "title" => Some(Cow::Borrowed(&fields.title)),
            "description" => Some(Cow::Borrowed(&fields.description)),
            _ => fields
                .extra
                .get(name.strip_prefix("tag:").unwrap_or(name))
                .map(|s| Cow::Borrowed(s.as_str())),
        };
        let value = value.as_deref().unwrap_or(&rest[start..start + end + 2]);
        if !append(&mut output, value) {
            return output;
        }
        rest = &after[end + 1..];
        if output.len() == TEXT_BYTES {
            return output;
        }
    }
    append(&mut output, rest);
    output
}

pub fn page_count(images: usize, settings: &PrintSettings) -> Result<u32, String> {
    settings.validate()?;
    let cells = u64::from(settings.rows) * u64::from(settings.columns);
    let count = (images as u64)
        .checked_mul(u64::from(settings.copies_per_image))
        .ok_or("打印数量溢出")?
        .div_ceil(cells);
    if count == 0 || count > 65535 {
        return Err("打印页数须为1至65535页，请缩小范围或增加接触表行列".into());
    }
    Ok(count as u32)
}
pub fn slot_image_index(
    page: u32,
    cell: u32,
    images: usize,
    settings: &PrintSettings,
) -> Option<usize> {
    if page == 0 || settings.validate().is_err() {
        return None;
    }
    let cells = u64::from(settings.rows) * u64::from(settings.columns);
    if u64::from(cell) >= cells {
        return None;
    }
    let slot = u64::from(page - 1)
        .checked_mul(cells)?
        .checked_add(u64::from(cell))?;
    let index = slot / u64::from(settings.copies_per_image);
    (index < images as u64).then_some(index as usize)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PixelRect {
    pub left: i32,
    pub top: i32,
    pub right: i32,
    pub bottom: i32,
}
impl PixelRect {
    fn width(self) -> i32 {
        self.right - self.left
    }
    fn height(self) -> i32 {
        self.bottom - self.top
    }
}
#[derive(Clone, Debug)]
pub struct PrintCell {
    pub image: PixelRect,
    pub caption: PixelRect,
}
#[derive(Clone, Debug)]
pub struct PageLayout {
    pub header: PixelRect,
    pub footer: PixelRect,
    pub cells: Vec<PrintCell>,
}

pub fn layout(
    page_width: i32,
    page_height: i32,
    dpi_x: f32,
    dpi_y: f32,
    settings: &PrintSettings,
) -> Result<PageLayout, String> {
    settings.validate()?;
    if page_width <= 0
        || page_height <= 0
        || !dpi_x.is_finite()
        || !dpi_y.is_finite()
        || !(1.0..=24000.0).contains(&dpi_x)
        || !(1.0..=24000.0).contains(&dpi_y)
    {
        return Err("打印机纸张或分辨率无效".into());
    }
    let mx = (settings.margin_mm * dpi_x / 25.4).round() as i32;
    let my = (settings.margin_mm * dpi_y / 25.4).round() as i32;
    let gx = (settings.gap_mm * dpi_x / 25.4).round() as i32;
    let gy = (settings.gap_mm * dpi_y / 25.4).round() as i32;
    let line = (settings.font_points * dpi_y / 72.0 * 1.4).ceil() as i32;
    let header_h = if settings.header.is_empty() {
        0
    } else {
        line * settings.header.lines().count().clamp(1, 5) as i32
    };
    let footer_h = if settings.footer.is_empty() {
        0
    } else {
        line * settings.footer.lines().count().clamp(1, 5) as i32
    };
    let top = my + header_h + if header_h > 0 { gy } else { 0 };
    let bottom = page_height - my - footer_h - if footer_h > 0 { gy } else { 0 };
    let cw = (page_width - 2 * mx - gx * (settings.columns as i32 - 1)) / settings.columns as i32;
    let ch = (bottom - top - gy * (settings.rows as i32 - 1)) / settings.rows as i32;
    let caption_h = if settings.caption.is_empty() {
        0
    } else {
        line * settings.caption_lines.max(1) as i32
    };
    if cw < 2 || ch - caption_h < 2 {
        return Err("纸张不足以容纳这些行列、文字与边距".into());
    }
    let mut cells = Vec::new();
    for row in 0..settings.rows as i32 {
        for col in 0..settings.columns as i32 {
            let left = mx + col * (cw + gx);
            let top = top + row * (ch + gy);
            cells.push(PrintCell {
                image: PixelRect {
                    left,
                    top,
                    right: left + cw,
                    bottom: top + ch - caption_h,
                },
                caption: PixelRect {
                    left,
                    top: top + ch - caption_h,
                    right: left + cw,
                    bottom: top + ch,
                },
            });
        }
    }
    Ok(PageLayout {
        header: PixelRect {
            left: mx,
            top: my,
            right: page_width - mx,
            bottom: my + header_h,
        },
        footer: PixelRect {
            left: mx,
            top: page_height - my - footer_h,
            right: page_width - mx,
            bottom: page_height - my,
        },
        cells,
    })
}

#[derive(Debug, PartialEq, Eq)]
struct Placement {
    x: i32,
    y: i32,
    width: i32,
    height: i32,
}

fn fit_geometry(
    width: u32,
    height: u32,
    target: PixelRect,
    dpi_x: i32,
    dpi_y: i32,
    fit: ImageFit,
) -> Result<(PixelRect, PixelRect), String> {
    if width == 0
        || height == 0
        || width > i32::MAX as u32
        || height > i32::MAX as u32
        || target.width() <= 0
        || target.height() <= 0
        || dpi_x <= 0
        || dpi_y <= 0
    {
        return Err("打印图像或区域尺寸无效".into());
    }
    let mut source = PixelRect {
        left: 0,
        top: 0,
        right: width as i32,
        bottom: height as i32,
    };
    match fit {
        ImageFit::Fit => {
            let w = width as f64 * dpi_x as f64;
            let h = height as f64 * dpi_y as f64;
            let scale = (target.width() as f64 / w).min(target.height() as f64 / h);
            let dw = (w * scale).round().clamp(1.0, target.width() as f64) as i32;
            let dh = (h * scale).round().clamp(1.0, target.height() as f64) as i32;
            let left = target.left + (target.width() - dw) / 2;
            let top = target.top + (target.height() - dh) / 2;
            Ok((
                source,
                PixelRect {
                    left,
                    top,
                    right: left + dw,
                    bottom: top + dh,
                },
            ))
        }
        ImageFit::Fill => {
            let physical_aspect =
                target.width() as f64 * dpi_y as f64 / (target.height() as f64 * dpi_x as f64);
            if width as f64 / height as f64 > physical_aspect {
                let w = (height as f64 * physical_aspect)
                    .round()
                    .clamp(1.0, width as f64) as i32;
                source.left = (width as i32 - w) / 2;
                source.right = source.left + w;
            } else {
                let h = (width as f64 / physical_aspect)
                    .round()
                    .clamp(1.0, height as f64) as i32;
                source.top = (height as i32 - h) / 2;
                source.bottom = source.top + h;
            }
            Ok((source, target))
        }
    }
}

fn placement(
    width: u32,
    height: u32,
    page_width: i32,
    page_height: i32,
) -> Result<Placement, String> {
    if width == 0 || height == 0 || page_width <= 0 || page_height <= 0 {
        return Err("图片或打印区域尺寸无效".into());
    }
    let scale = (page_width as f64 / width as f64).min(page_height as f64 / height as f64);
    let w = (width as f64 * scale).round().clamp(1.0, page_width as f64) as i32;
    let h = (height as f64 * scale)
        .round()
        .clamp(1.0, page_height as f64) as i32;
    Ok(Placement {
        x: (page_width - w) / 2,
        y: (page_height - h) / 2,
        width: w,
        height: h,
    })
}

fn convert_strip(
    rgba: &[u8],
    image_width: usize,
    x: usize,
    y: usize,
    width: usize,
    height: usize,
    output: &mut Vec<u8>,
) {
    output.resize(width * height * 4, 0);
    for row in 0..height {
        let start = ((y + row) * image_width + x) * 4;
        for (source, target) in rgba[start..start + width * 4]
            .chunks_exact(4)
            .zip(output[row * width * 4..(row + 1) * width * 4].chunks_exact_mut(4))
        {
            let alpha = source[3] as u32;
            for channel in 0..3 {
                target[channel] =
                    ((source[2 - channel] as u32 * alpha + 255 * (255 - alpha) + 127) / 255) as u8;
            }
            target[3] = 0; // BI_RGB is BGRX: composite alpha explicitly onto white.
        }
    }
}

#[cfg(windows)]
mod platform {
    use super::*;
    use std::{cell::Cell, marker::PhantomData, mem::size_of, ptr};
    use windows_sys::Win32::{
        Foundation::{GlobalFree, HWND},
        Graphics::Gdi::{
            CreateFontW, DeleteDC, DeleteObject, DrawTextW, GetDeviceCaps, SelectObject, SetBkMode,
            SetBrushOrgEx, SetStretchBltMode, SetTextColor, StretchDIBits, BITMAPINFO,
            BITMAPINFOHEADER, BI_RGB, DEFAULT_CHARSET, DIB_RGB_COLORS, DT_CENTER, DT_END_ELLIPSIS,
            DT_LEFT, DT_NOPREFIX, DT_RIGHT, DT_WORDBREAK, FW_NORMAL, HALFTONE, HDC, HORZRES,
            LOGPIXELSX, LOGPIXELSY, SRCCOPY, TRANSPARENT, VERTRES,
        },
        Storage::Xps::{AbortDoc, EndDoc, EndPage, StartDocW, StartPage, DOCINFOW},
        UI::Controls::Dialogs::{
            CommDlgExtendedError, PrintDlgW, PD_HIDEPRINTTOFILE, PD_NOSELECTION, PD_PAGENUMS,
            PD_RETURNDC, PD_USEDEVMODECOPIESANDCOLLATE, PRINTDLGW,
        },
    };

    /// Send but not Sync/Clone. All printing calls use &mut self on its owner thread.
    pub struct PrintJob {
        dc: usize,
        document: bool,
        page: bool,
        complete: bool,
        _not_sync: PhantomData<Cell<()>>,
        first_page: u32,
        last_page: u32,
    }

    /// UI-thread native printer selection. Cancel returns Ok(None).
    pub fn choose_printer(owner: HWND) -> Result<Option<PrintJob>, String> {
        choose_printer_for_pages(owner, 65535)
    }
    pub fn choose_printer_for_pages(
        owner: HWND,
        total_pages: u32,
    ) -> Result<Option<PrintJob>, String> {
        if total_pages == 0 || total_pages > 65535 {
            return Err("原生打印范围支持1至65535页".into());
        }
        unsafe {
            let mut dialog: PRINTDLGW = std::mem::zeroed();
            dialog.lStructSize = size_of::<PRINTDLGW>() as u32;
            dialog.hwndOwner = owner;
            dialog.Flags =
                PD_RETURNDC | PD_NOSELECTION | PD_HIDEPRINTTOFILE | PD_USEDEVMODECOPIESANDCOLLATE;
            dialog.nMinPage = 1;
            dialog.nMaxPage = total_pages as u16;
            dialog.nFromPage = 1;
            dialog.nToPage = total_pages as u16;
            dialog.nCopies = 1;
            let accepted = PrintDlgW(&mut dialog) != 0;
            let error = if accepted { 0 } else { CommDlgExtendedError() };
            if !dialog.hDevMode.is_null() {
                GlobalFree(dialog.hDevMode);
            }
            if !dialog.hDevNames.is_null() {
                GlobalFree(dialog.hDevNames);
            }
            if !accepted {
                if !dialog.hDC.is_null() {
                    DeleteDC(dialog.hDC);
                }
                return if error == 0 {
                    Ok(None)
                } else {
                    Err(format!("无法打开打印设置（错误{error:#x}）"))
                };
            }
            if dialog.hDC.is_null() {
                return Err("打印机没有提供设备上下文".into());
            }
            Ok(Some(PrintJob {
                dc: dialog.hDC as usize,
                document: false,
                page: false,
                complete: false,
                _not_sync: PhantomData,
                first_page: if dialog.Flags & PD_PAGENUMS != 0 {
                    dialog.nFromPage as u32
                } else {
                    1
                },
                last_page: if dialog.Flags & PD_PAGENUMS != 0 {
                    dialog.nToPage as u32
                } else {
                    total_pages
                },
            }))
        }
    }

    impl PrintJob {
        fn failure(&mut self, message: impl Into<String>) -> String {
            self.abort();
            message.into()
        }

        pub fn start_document(&mut self, title: &str) -> Result<(), String> {
            if self.document || self.complete {
                return Err(self.failure("此打印任务已启动或已经结束"));
            }
            let mut name: Vec<u16> = title.encode_utf16().collect();
            if name.contains(&0) {
                return Err(self.failure("打印标题包含空字符"));
            }
            name.push(0);
            let info = DOCINFOW {
                cbSize: size_of::<DOCINFOW>() as i32,
                lpszDocName: name.as_ptr(),
                lpszOutput: ptr::null(),
                lpszDatatype: ptr::null(),
                fwType: 0,
            };
            let code = unsafe { StartDocW(self.dc as HDC, &info) };
            if code <= 0 {
                return Err(self.failure(format!("无法开始打印文档（GDI状态{code}）")));
            }
            self.document = true;
            Ok(())
        }

        pub fn start_page(&mut self) -> Result<(), String> {
            if !self.document || self.page {
                return Err(self.failure("请先开始文档并结束上一页"));
            }
            let code = unsafe { StartPage(self.dc as HDC) };
            if code <= 0 {
                return Err(self.failure(format!("无法开始打印页（GDI状态{code}）")));
            }
            self.page = true;
            Ok(())
        }

        /// Fit the complete image into the printable area (legacy single-image entry point).
        pub fn draw_rgba(&mut self, width: u32, height: u32, rgba: &[u8]) -> Result<(), String> {
            let dc = self.dc as HDC;
            let target = PixelRect {
                left: 0,
                top: 0,
                right: unsafe { GetDeviceCaps(dc, HORZRES as i32) },
                bottom: unsafe { GetDeviceCaps(dc, VERTRES as i32) },
            };
            self.draw_region(width, height, rgba, target, ImageFit::Fit)
        }

        pub fn page_range(&self) -> (u32, u32) {
            (self.first_page, self.last_page)
        }
        pub fn page_metrics(&self) -> (i32, i32, f32, f32) {
            let dc = self.dc as HDC;
            (
                unsafe { GetDeviceCaps(dc, HORZRES as i32) },
                unsafe { GetDeviceCaps(dc, VERTRES as i32) },
                unsafe { GetDeviceCaps(dc, LOGPIXELSX as i32) } as f32,
                unsafe { GetDeviceCaps(dc, LOGPIXELSY as i32) } as f32,
            )
        }
        pub fn page_layout(&self, settings: &PrintSettings) -> Result<PageLayout, String> {
            let dc = self.dc as HDC;
            layout(
                unsafe { GetDeviceCaps(dc, HORZRES as i32) },
                unsafe { GetDeviceCaps(dc, VERTRES as i32) },
                unsafe { GetDeviceCaps(dc, LOGPIXELSX as i32) } as f32,
                unsafe { GetDeviceCaps(dc, LOGPIXELSY as i32) } as f32,
                settings,
            )
        }
        pub fn draw_page_text(
            &mut self,
            settings: &PrintSettings,
            fields: &Fields,
        ) -> Result<(), String> {
            let layout = self.page_layout(settings)?;
            self.draw_text(
                &expand_fields(&settings.header, fields),
                layout.header,
                settings,
            )?;
            self.draw_text(
                &expand_fields(&settings.footer, fields),
                layout.footer,
                settings,
            )
        }
        pub fn draw_cell_rgba(
            &mut self,
            width: u32,
            height: u32,
            rgba: &[u8],
            cell_index: u32,
            settings: &PrintSettings,
            fields: &Fields,
        ) -> Result<(), String> {
            let layout = self.page_layout(settings)?;
            let cell = layout
                .cells
                .get(cell_index as usize)
                .ok_or("打印单元格超出范围")?;
            let mut target = cell.image;
            if settings.image_width_mm.is_some() || settings.image_height_mm.is_some() {
                let dc = self.dc as HDC;
                let dx = unsafe { GetDeviceCaps(dc, LOGPIXELSX as i32) } as f32;
                let dy = unsafe { GetDeviceCaps(dc, LOGPIXELSY as i32) } as f32;
                let physical_w = settings.image_width_mm.unwrap_or_else(|| {
                    settings.image_height_mm.unwrap() * width as f32 / height as f32
                });
                let physical_h = settings.image_height_mm.unwrap_or_else(|| {
                    settings.image_width_mm.unwrap() * height as f32 / width as f32
                });
                let w = (physical_w * dx / 25.4).round() as i32;
                let h = (physical_h * dy / 25.4).round() as i32;
                if w > target.width() || h > target.height() {
                    return Err(self.failure("指定物理尺寸超过纸张单元格，请调整纸张、行列或尺寸"));
                }
                let left = target.left + (target.width() - w) / 2;
                let top = target.top + (target.height() - h) / 2;
                target = PixelRect {
                    left,
                    top,
                    right: left + w,
                    bottom: top + h,
                };
            }
            self.draw_region(width, height, rgba, target, settings.fit)?;
            self.draw_text(
                &expand_fields(&settings.caption, fields),
                cell.caption,
                settings,
            )
        }
        pub fn draw_rgba_at(
            &mut self,
            width: u32,
            height: u32,
            rgba: &[u8],
            target: PixelRect,
            fit: ImageFit,
        ) -> Result<(), String> {
            self.draw_region(width, height, rgba, target, fit)
        }

        fn draw_text(
            &mut self,
            text: &str,
            rect: PixelRect,
            settings: &PrintSettings,
        ) -> Result<(), String> {
            if text.is_empty() || rect.height() <= 0 {
                return Ok(());
            }
            if !self.document || !self.page {
                return Err(self.failure("请先开始打印页"));
            }
            let dc = self.dc as HDC;
            let height = (settings.font_points
                * unsafe { GetDeviceCaps(dc, LOGPIXELSY as i32) } as f32
                / 72.0)
                .round()
                .max(1.0) as i32;
            let face: Vec<u16> = "Microsoft YaHei".encode_utf16().chain(Some(0)).collect();
            let font = unsafe {
                CreateFontW(
                    -height,
                    0,
                    0,
                    0,
                    FW_NORMAL as i32,
                    0,
                    0,
                    0,
                    DEFAULT_CHARSET as u32,
                    0,
                    0,
                    0,
                    0,
                    face.as_ptr(),
                )
            };
            if font.is_null() {
                return Err(self.failure("无法创建打印字体"));
            }
            let old = unsafe { SelectObject(dc, font as _) };
            let old_mode = unsafe { SetBkMode(dc, TRANSPARENT as i32) };
            let old_color = unsafe { SetTextColor(dc, 0) };
            let mut r = windows_sys::Win32::Foundation::RECT {
                left: rect.left,
                top: rect.top,
                right: rect.right,
                bottom: rect.bottom,
            };
            let text: Vec<u16> = text.encode_utf16().take(16384).collect();
            let alignment = match settings.alignment {
                TextAlignment::Left => DT_LEFT,
                TextAlignment::Center => DT_CENTER,
                TextAlignment::Right => DT_RIGHT,
            };
            let result = unsafe {
                DrawTextW(
                    dc,
                    text.as_ptr(),
                    text.len() as i32,
                    &mut r,
                    alignment | DT_WORDBREAK | DT_NOPREFIX | DT_END_ELLIPSIS,
                )
            };
            unsafe {
                SetTextColor(dc, old_color);
                SetBkMode(dc, old_mode);
                SelectObject(dc, old);
                DeleteObject(font as _);
            }
            if result == 0 {
                return Err(self.failure("打印机无法绘制页眉、页脚或标题"));
            }
            Ok(())
        }

        /// At most one <=16MiB BGRX tile; filling crops the source without creating a second image.
        fn draw_region(
            &mut self,
            width: u32,
            height: u32,
            rgba: &[u8],
            target: PixelRect,
            fit: ImageFit,
        ) -> Result<(), String> {
            if !self.document || !self.page {
                return Err(self.failure("请先开始打印页"));
            }
            let expected = (width as usize)
                .checked_mul(height as usize)
                .and_then(|n| n.checked_mul(4));
            if width == 0
                || height == 0
                || expected != Some(rgba.len())
                || rgba.len() > crate::editing::MAX_BYTES
            {
                return Err(self.failure("打印RGBA尺寸不一致或超过512MiB"));
            }
            let dc = self.dc as HDC;
            let dx = unsafe { GetDeviceCaps(dc, LOGPIXELSX as i32) };
            let dy = unsafe { GetDeviceCaps(dc, LOGPIXELSY as i32) };
            let (source, rect) =
                fit_geometry(width, height, target, dx, dy, fit).map_err(|e| self.failure(e))?;
            unsafe {
                if SetStretchBltMode(dc, HALFTONE) == 0
                    || SetBrushOrgEx(dc, 0, 0, ptr::null_mut()) == 0
                {
                    return Err(self.failure("打印机无法设置图像缩放模式"));
                }
            }
            let source_width = source.width() as usize;
            let source_height = source.height() as usize;
            let tile_width = source_width.min(STRIP_BYTES / 4);
            let rows = (STRIP_BYTES / (tile_width * 4)).max(1);
            let mut pixels = Vec::new();
            for y in (0..source_height).step_by(rows) {
                let h = rows.min(source_height - y);
                for x in (0..source_width).step_by(tile_width) {
                    let w = tile_width.min(source_width - x);
                    let left = rect.left
                        + (x as f64 * rect.width() as f64 / source_width as f64).round() as i32;
                    let right = rect.left
                        + ((x + w) as f64 * rect.width() as f64 / source_width as f64).round()
                            as i32;
                    let top = rect.top
                        + (y as f64 * rect.height() as f64 / source_height as f64).round() as i32;
                    let bottom = rect.top
                        + ((y + h) as f64 * rect.height() as f64 / source_height as f64).round()
                            as i32;
                    if right <= left || bottom <= top {
                        continue;
                    }
                    convert_strip(
                        rgba,
                        width as usize,
                        source.left as usize + x,
                        source.top as usize + y,
                        w,
                        h,
                        &mut pixels,
                    );
                    let mut info: BITMAPINFO = unsafe { std::mem::zeroed() };
                    info.bmiHeader.biSize = size_of::<BITMAPINFOHEADER>() as u32;
                    info.bmiHeader.biWidth = w as i32;
                    info.bmiHeader.biHeight = -(h as i32);
                    info.bmiHeader.biPlanes = 1;
                    info.bmiHeader.biBitCount = 32;
                    info.bmiHeader.biCompression = BI_RGB;
                    info.bmiHeader.biSizeImage = pixels.len() as u32;
                    let result = unsafe {
                        StretchDIBits(
                            dc,
                            left,
                            top,
                            right - left,
                            bottom - top,
                            0,
                            0,
                            w as i32,
                            h as i32,
                            pixels.as_ptr().cast(),
                            &info,
                            DIB_RGB_COLORS,
                            SRCCOPY,
                        )
                    };
                    if result <= 0 {
                        return Err(
                            self.failure(format!("打印机无法接收图像分片（GDI状态{result}）"))
                        );
                    }
                }
            }
            Ok(())
        }

        pub fn end_page(&mut self) -> Result<(), String> {
            if !self.document || !self.page {
                return Err(self.failure("没有正在打印的页面"));
            }
            let code = unsafe { EndPage(self.dc as HDC) };
            if code <= 0 {
                return Err(self.failure(format!("无法结束打印页（GDI状态{code}）")));
            }
            self.page = false;
            Ok(())
        }

        pub fn finish(&mut self) -> Result<(), String> {
            if !self.document || self.page {
                return Err(self.failure("请先结束最后一页"));
            }
            let code = unsafe { EndDoc(self.dc as HDC) };
            if code <= 0 {
                return Err(self.failure(format!("无法提交打印文档（GDI状态{code}）")));
            }
            self.document = false;
            self.complete = true;
            Ok(())
        }

        pub fn abort(&mut self) {
            if self.document {
                unsafe {
                    AbortDoc(self.dc as HDC);
                }
            }
            self.document = false;
            self.page = false;
            self.complete = true;
        }
    }
    impl Drop for PrintJob {
        fn drop(&mut self) {
            self.abort();
            unsafe {
                DeleteDC(self.dc as HDC);
            }
        }
    }
}

#[cfg(windows)]
pub use platform::{choose_printer_for_pages, PrintJob};

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn contact_sheet_layout_copies_ranges_and_fields_are_bounded() {
        let settings = PrintSettings {
            rows: 2,
            columns: 3,
            header: "{name} {page}/{pages}".into(),
            footer: "{date} {time}".into(),
            caption: "{title}".into(),
            copies_per_image: 2,
            ..Default::default()
        };
        assert_eq!(page_count(7, &settings).unwrap(), 3);
        assert_eq!(slot_image_index(1, 1, 7, &settings), Some(0));
        assert_eq!(slot_image_index(2, 0, 7, &settings), Some(3));
        assert_eq!(slot_image_index(3, 2, 7, &settings), None);
        let grid = layout(2400, 3200, 300.0, 300.0, &settings).unwrap();
        assert_eq!(grid.cells.len(), 6);
        assert!(grid.cells.iter().all(|c| c.image.width() > 0
            && c.image.height() > 0
            && c.image.bottom <= c.caption.top));
        let fields = Fields {
            name: "photo{pages}.jpg".into(),
            page: 2,
            pages: 3,
            width: 640,
            height: 480,
            title: "中文标题".into(),
            ..Default::default()
        };
        assert_eq!(
            expand_fields(
                "{name} {page}/{pages} {width}×{height} {title} {unknown}",
                &fields
            ),
            "photo{pages}.jpg 2/3 640×480 中文标题 {unknown}"
        );
        assert_eq!(field_token(13023), Some("{page}"));
        assert_eq!(field_token(13037), Some("{time}"));
        assert!(PrintSettings {
            rows: 0,
            ..Default::default()
        }
        .validate()
        .is_err());
        assert!(PrintSettings {
            gap_mm: 100.0,
            rows: 20,
            columns: 20,
            ..Default::default()
        }
        .validate()
        .is_ok());
        assert!(layout(100, 100, 300.0, 300.0, &settings).is_err());
    }
    #[test]
    fn physical_aspect_and_fill_crop_are_correct_without_second_image() {
        let target = PixelRect {
            left: 0,
            top: 0,
            right: 500,
            bottom: 500,
        };
        let (source, rect) = fit_geometry(200, 100, target, 300, 600, ImageFit::Fit).unwrap();
        assert_eq!(rect, target);
        assert_eq!(source.right, 200);
        let (source, rect) = fit_geometry(200, 100, target, 300, 300, ImageFit::Fill).unwrap();
        assert_eq!(rect, target);
        assert_eq!(
            source,
            PixelRect {
                left: 50,
                top: 0,
                right: 150,
                bottom: 100
            }
        );
        assert!(fit_geometry(0, 100, target, 300, 300, ImageFit::Fit).is_err());
    }
    #[test]
    fn image_is_centered_without_distortion() {
        assert_eq!(
            placement(200, 100, 1000, 1000).unwrap(),
            Placement {
                x: 0,
                y: 250,
                width: 1000,
                height: 500
            }
        );
        assert_eq!(
            placement(100, 200, 1000, 1000).unwrap(),
            Placement {
                x: 250,
                y: 0,
                width: 500,
                height: 1000
            }
        );
        assert!(placement(0, 200, 1000, 1000).is_err());
        assert!(placement(100, 200, 0, 1000).is_err());
    }
    #[test]
    fn strips_are_bgrx_composited_on_white() {
        let rgba = [255, 0, 0, 128, 0, 7, 9, 0, 1, 2, 3, 255];
        let mut output = Vec::new();
        convert_strip(&rgba, 3, 0, 0, 3, 1, &mut output);
        assert_eq!(output, [127, 127, 255, 0, 255, 255, 255, 0, 3, 2, 1, 0]);
        convert_strip(&rgba, 3, 2, 0, 1, 1, &mut output);
        assert_eq!(output, [3, 2, 1, 0]);
    }
}

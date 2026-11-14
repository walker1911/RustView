use std::{
    fs::File,
    io::{Cursor, Read},
    path::Path,
    sync::atomic::{AtomicBool, Ordering},
};

use image::{ImageFormat, ImageReader};
use resvg::{tiny_skia, usvg};

use crate::imaging::DecodedImage;

pub(crate) const INITIAL_VIEW: [u32; 2] = [1280, 840];
pub(crate) const MAX_SOURCE_BYTES: u64 = 8 * 1024 * 1024;
pub(crate) const MAX_RASTER_EDGE: u32 = 4096;
pub(crate) const MAX_RASTER_PIXELS: u64 = 8_000_000;
pub(crate) const MAX_RASTER_BYTES: usize = 32 * 1024 * 1024;
const MAX_EMBEDDED_PIXELS: u64 = 4 * 1024 * 1024;
const MAX_XML_NODES: u32 = 100_000;

pub(crate) fn is_svg(name: &str) -> bool {
    Path::new(name)
        .extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| extension.eq_ignore_ascii_case("svg"))
}

pub(crate) fn render(
    path: &Path,
    max_width: u32,
    max_height: u32,
    max_pixels: u64,
    max_output_bytes: usize,
) -> Result<(DecodedImage, [u32; 2]), String> {
    let mut data = Vec::new();
    File::open(path)
        .and_then(|file| file.take(MAX_SOURCE_BYTES + 1).read_to_end(&mut data))
        .map_err(|error| format!("无法读取 SVG：{error}"))?;
    if data.len() as u64 > MAX_SOURCE_BYTES {
        return Err("SVG 文件超过 8 MiB".into());
    }
    let text = std::str::from_utf8(&data).map_err(|_| "SVG 文件需要使用 UTF-8 编码")?;
    let xml = parse_svg_xml(text).map_err(|error| format!("无法解析 SVG：{error}"))?;

    let image_skipped = AtomicBool::new(false);
    let mut options = usvg::Options {
        max_depth: 128,
        languages: vec!["zh-CN".into(), "en".into()],
        image_href_resolver: usvg::ImageHrefResolver {
            // The default resolver opens arbitrary paths, including UNC shares.
            resolve_string: Box::new(|_, _| {
                image_skipped.store(true, Ordering::Relaxed);
                None
            }),
            resolve_data: Box::new(|_, data, _| {
                let image = embedded_raster(data);
                if image.is_none() {
                    image_skipped.store(true, Ordering::Relaxed);
                }
                image
            }),
        },
        ..Default::default()
    };
    if xml
        .descendants()
        .any(|node| node.has_tag_name(("http://www.w3.org/2000/svg", "text")))
    {
        load_local_fonts(options.fontdb_mut());
    }
    let tree = usvg::Tree::from_xmltree(&xml, &options)
        .map_err(|error| format!("无法解析 SVG：{error}"))?;
    drop(xml);
    drop(data);

    let size = tree.size();
    let intrinsic = [f64::from(size.width()), f64::from(size.height())];
    if intrinsic
        .iter()
        .any(|&edge| !edge.is_finite() || edge <= 0.0 || edge > f64::from(u32::MAX))
    {
        return Err("SVG 画布尺寸超出支持范围".into());
    }
    let budget = max_pixels
        .min(MAX_RASTER_PIXELS)
        .min((max_output_bytes.min(MAX_RASTER_BYTES) / 4) as u64);
    let [width, height] =
        raster_size(intrinsic, max_width, max_height, budget).ok_or("SVG 渲染尺寸超出像素预算")?;
    let mut pixmap = tiny_skia::Pixmap::new(width, height).ok_or("无法分配 SVG 渲染缓冲区")?;
    let scale_x = width as f32 / size.width();
    let scale_y = height as f32 / size.height();
    if !scale_x.is_finite() || !scale_y.is_finite() {
        return Err("SVG 画布比例超出支持范围".into());
    }
    let transform = tiny_skia::Transform::from_scale(scale_x, scale_y);
    resvg::render(&tree, transform, &mut pixmap.as_mut());

    let note = if image_skipped.load(Ordering::Relaxed) {
        "SVG · 已跳过外部或超限的嵌入图像"
    } else {
        "SVG · 静态图像"
    };
    Ok((
        DecodedImage {
            width,
            height,
            rgba: pixmap.take_demultiplied(),
            note: note.into(),
        },
        intrinsic.map(|edge| edge.round().max(1.0) as u32),
    ))
}

fn parse_svg_xml(text: &str) -> Result<usvg::roxmltree::Document<'_>, usvg::roxmltree::Error> {
    use usvg::roxmltree::{Document, Error, ParsingOptions};

    let parse = |allow_dtd| {
        Document::parse_with_options(
            text,
            ParsingOptions {
                allow_dtd,
                nodes_limit: MAX_XML_NODES,
                ..Default::default()
            },
        )
    };
    match parse(false) {
        Err(Error::DtdDetected) if doctype_has_no_internal_subset(text) => {
            // The parser validates but does not load the external identifier.
            // Require a declaration without an internal subset to disallow entities.
            parse(true)
        }
        result => result,
    }
}

fn doctype_has_no_internal_subset(text: &str) -> bool {
    let mut prolog = text.strip_prefix('\u{feff}').unwrap_or(text);
    let mut declaration = loop {
        prolog = prolog.trim_start_matches([' ', '\t', '\r', '\n']);
        if let Some(declaration) = prolog.strip_prefix("<!DOCTYPE") {
            break declaration;
        }
        let end_marker = if prolog.starts_with("<!--") {
            "-->"
        } else if prolog.starts_with("<?") {
            "?>"
        } else {
            return false;
        };
        let Some((_, rest)) = prolog.split_once(end_marker) else {
            return false;
        };
        prolog = rest;
    };
    while let Some(index) = declaration.find(['>', '[', '\'', '"']) {
        match declaration.as_bytes()[index] {
            b'>' => return true,
            b'[' => return false,
            quote => {
                let Some((_, rest)) = declaration[index + 1..].split_once(char::from(quote)) else {
                    return false;
                };
                declaration = rest;
            }
        }
    }
    false
}

fn raster_size(
    intrinsic: [f64; 2],
    max_width: u32,
    max_height: u32,
    budget: u64,
) -> Option<[u32; 2]> {
    let [width, height] = intrinsic;
    let max_edge = MAX_RASTER_EDGE.min(budget as u32);
    let scale = (f64::from(max_width.min(max_edge)) / width)
        .min(f64::from(max_height.min(max_edge)) / height)
        .min((budget as f64 / (width * height)).sqrt());
    if scale <= 0.0 {
        return None;
    }
    let [width, height] = intrinsic.map(|edge| (edge * scale).floor().max(1.0) as u32);
    (u64::from(width) * u64::from(height) <= budget).then_some([width, height])
}

fn embedded_raster(data: std::sync::Arc<Vec<u8>>) -> Option<usvg::ImageKind> {
    let reader = ImageReader::new(Cursor::new(data.as_slice()))
        .with_guessed_format()
        .ok()?;
    let format = reader.format()?;
    if !matches!(
        format,
        ImageFormat::Jpeg | ImageFormat::Png | ImageFormat::WebP
    ) {
        return None;
    }
    let (width, height) = reader.into_dimensions().ok()?;
    if u64::from(width) * u64::from(height) > MAX_EMBEDDED_PIXELS {
        return None;
    }
    match format {
        ImageFormat::Jpeg => Some(usvg::ImageKind::JPEG(data)),
        ImageFormat::Png => Some(usvg::ImageKind::PNG(data)),
        ImageFormat::WebP => Some(usvg::ImageKind::WEBP(data)),
        _ => None,
    }
}

fn load_local_fonts(db: &mut usvg::fontdb::Database) {
    #[cfg(windows)]
    {
        use std::path::PathBuf;
        let root = std::env::var_os("SystemRoot")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from(r"C:\Windows"))
            .join("Fonts");
        // Load a finite set of standard Windows fonts only for SVGs with text.
        // The fallback resolver can use these CJK faces for missing glyphs.
        for font in [
            "arial.ttf",
            "arialbd.ttf",
            "ariali.ttf",
            "times.ttf",
            "timesbd.ttf",
            "timesi.ttf",
            "consola.ttf",
            "msyh.ttc",
            "simsun.ttc",
        ] {
            const MAX_FONT_BYTES: u64 = 32 * 1024 * 1024;
            let mut data = Vec::new();
            if let Ok(file) = File::open(root.join(font)) {
                if file.take(MAX_FONT_BYTES + 1).read_to_end(&mut data).is_ok()
                    && data.len() as u64 <= MAX_FONT_BYTES
                {
                    db.load_font_data(data);
                }
            }
        }
        db.set_sans_serif_family("Arial");
        db.set_serif_family("Times New Roman");
        db.set_monospace_family("Consolas");
    }
    #[cfg(not(windows))]
    let _ = db;
}

#[cfg(test)]
mod tests {
    use super::*;

    fn render_fixture(
        xml: &str,
        width: u32,
        height: u32,
    ) -> Result<(DecodedImage, [u32; 2]), String> {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("image.SVG");
        std::fs::write(&path, xml).unwrap();
        render(&path, width, height, MAX_RASTER_PIXELS, MAX_RASTER_BYTES)
    }

    #[test]
    fn svg_preserves_intrinsic_geometry_and_straight_alpha_at_requested_scale() {
        let xml = r#"<svg xmlns="http://www.w3.org/2000/svg" width="20" height="10">
            <rect width="10" height="10" fill="red" opacity="0.5" />
        </svg>"#;
        assert!(is_svg("design/PHOTO.SVG"));
        assert!(!is_svg("design/PHOTO.SVGZ"));
        for (width, height) in [(20, 10), (128, 64), (1024, 512)] {
            let (image, intrinsic) = render_fixture(xml, width, height).unwrap();
            assert_eq!(intrinsic, [20, 10]);
            assert_eq!((image.width, image.height), (width, height));
            assert_eq!(&image.rgba[..4], &[255, 0, 0, 128]);
            assert_eq!(
                &image.rgba[(width * 4 - 4) as usize..(width * 4) as usize],
                &[0; 4]
            );
        }
    }

    #[test]
    fn huge_view_box_fits_the_render_budget() {
        let xml = r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 10000000 10000000">
            <rect width="10000000" height="10000000" fill="green" />
        </svg>"#;
        let (image, intrinsic) = render_fixture(xml, u32::MAX, u32::MAX).unwrap();
        assert_eq!(intrinsic, [10_000_000, 10_000_000]);
        assert_eq!((image.width, image.height), (2828, 2828));
        assert_eq!(image.rgba.len(), 2828 * 2828 * 4);
        assert_eq!(&image.rgba[..4], &[0, 128, 0, 255]);
        assert_eq!(raster_size([1e9, 1.0], 1000, 1000, 16), Some([16, 1]));
        assert_eq!(raster_size([1.0, 1.0], 1, 1, 0), None);
    }

    #[test]
    fn svg_ignores_local_remote_and_nested_svg_resources() {
        let dir = tempfile::tempdir().unwrap();
        let png = dir.path().join("neighbor.png");
        image::RgbaImage::from_pixel(4, 4, image::Rgba([255, 0, 0, 255]))
            .save(&png)
            .unwrap();
        let xml = format!(
            r#"<svg xmlns="http://www.w3.org/2000/svg" width="30" height="10">
            <image href="{}" width="10" height="10" />
            <image href="https://example.invalid/image.png" x="10" width="10" height="10" />
            <image href="data:image/svg+xml,%3Csvg%2F%3E" x="20" width="10" height="10" />
        </svg>"#,
            png.to_string_lossy()
        );
        let (image, _) = render_fixture(&xml, 30, 10).unwrap();
        assert!(image.rgba.iter().all(|&byte| byte == 0));
        assert!(image.note.contains("已跳过"));
    }

    #[test]
    fn svg_embedded_images_accept_small_rasters_and_skip_large_dimensions() {
        const SMALL: &str = "iVBORw0KGgoAAAANSUhEUgAAAAIAAAACCAYAAABytg0kAAAAFElEQVR4nGP8z8Dwn4GBgYGJAQoAHxcCAk+Uzr4AAAAASUVORK5CYII=";
        // A 4096×4096 PNG header with a valid CRC. The body is intentionally incomplete.
        const LARGE: &str = "iVBORw0KGgoAAAANSUhEUgAAEAAAABAACAYAAADyoyQXAAAAFElEQVR4nGP8z8Dwn4GBgYGJAQoAHxcCAk+Uzr4AAAAASUVORK5CYII=";
        let xml = format!(
            r#"<svg xmlns="http://www.w3.org/2000/svg" width="4" height="2">
            <image href="data:image/png;base64,{SMALL}" width="2" height="2" />
            <image href="data:image/png;base64,{LARGE}" x="2" width="2" height="2" />
        </svg>"#
        );
        let (image, _) = render_fixture(&xml, 4, 2).unwrap();
        assert_eq!(&image.rgba[..4], &[255, 0, 0, 255]);
        assert_eq!(&image.rgba[8..12], &[0; 4]);
        assert!(image.note.contains("已跳过"));
    }

    #[test]
    fn svg_renders_with_an_external_doctype_declaration() {
        let svg = r##"<svg xmlns="http://www.w3.org/2000/svg" xmlns:xlink="http://www.w3.org/1999/xlink" width="20pt" height="10pt">
            <defs><path id="mark" d="M0 0H10V10H0Z" fill="red"/></defs>
            <use xlink:href="#mark"/>
        </svg>"##;
        let prolog = "\u{feff}<?xml version=\"1.0\" encoding=\"utf-8\"?>\n<!-- <!DOCTYPE svg [ignored]> -->\n";
        for declaration in [
            "<!DOCTYPE svg PUBLIC \"-//W3C//DTD SVG 1.1//EN\"\n  \"http://www.w3.org/Graphics/SVG/1.1/DTD/svg11.dtd\">",
            "<!DOCTYPE svg SYSTEM 'https://example.invalid/[a]>b.dtd'>",
        ] {
            let xml = format!("{prolog}{declaration}{svg}");
            let (image, _) = render_fixture(&xml, 40, 20).unwrap();
            assert_eq!((image.width, image.height), (40, 20));
            assert_eq!(&image.rgba[..4], &[255, 0, 0, 255]);
        }
    }

    #[test]
    fn svg_rejects_custom_entities() {
        for declaration in [
            r#"<!DOCTYPE svg [<!ENTITY test "x">]>"#,
            r#"<!DOCTYPE svg SYSTEM "https://example.invalid/a>b.dtd" [<!ENTITY test "x">]>"#,
            r#"<!DOCTYPE svg SYSTEM "https://example.invalid/test.dtd">"#,
        ] {
            let xml = format!(
                r#"{declaration}<svg xmlns="http://www.w3.org/2000/svg" width="10" height="10">&test;</svg>"#
            );
            assert!(render_fixture(&xml, 20, 20).is_err());
        }
    }

    #[test]
    fn svg_bounds_the_file_read() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("large.svg");
        File::create(&path)
            .unwrap()
            .set_len(MAX_SOURCE_BYTES + 1)
            .unwrap();
        assert!(render(&path, 20, 20, 400, 1600)
            .err()
            .unwrap()
            .contains("8 MiB"));
    }

    #[cfg(windows)]
    #[test]
    fn svg_renders_chinese_text_with_installed_fonts() {
        let root = std::env::var_os("SystemRoot").expect("Windows root path");
        let fonts = Path::new(&root).join("Fonts");
        if !fonts.join("msyh.ttc").exists() && !fonts.join("simsun.ttc").exists() {
            // Some Windows installs omit the optional CJK fonts.
            return;
        }
        let xml = r#"<svg xmlns="http://www.w3.org/2000/svg" width="130" height="50">
            <text x="3" y="36" font-family="sans-serif" font-size="30">图片 A</text>
        </svg>"#;
        let (image, _) = render_fixture(xml, 130, 50).unwrap();
        assert!(image
            .rgba
            .chunks_exact(4)
            .enumerate()
            .any(|(index, pixel)| {
                (45..59).contains(&(index % image.width as usize)) && pixel[3] > 0
            }));
    }
}

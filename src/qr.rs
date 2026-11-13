use quircs::{Data, DataType, Eci, Quirc};
use std::{
    borrow::Cow,
    fmt::Write,
    sync::atomic::{AtomicBool, Ordering},
};
use url::Url;

const MAX_SIDE: u32 = 2048;
const MAX_HITS: usize = 8;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Hit {
    pub(crate) text: String,
    pub(crate) url: Option<Url>,
}

pub(crate) fn scan(width: u32, height: u32, gray: &[u8], cancel: &AtomicBool) -> Vec<Hit> {
    if !(21..=MAX_SIDE).contains(&width)
        || !(21..=MAX_SIDE).contains(&height)
        || gray.len() != width as usize * height as usize
        || cancel.load(Ordering::Relaxed)
    {
        return Vec::new();
    }

    let mut scanner = Quirc::default();
    let mut hits = decode(&mut scanner, width, height, gray, cancel);
    if hits.is_empty() && !cancel.load(Ordering::Relaxed) {
        hits = decode_separated(&mut scanner, width, height, gray, cancel);
    }
    if hits.is_empty() && !cancel.load(Ordering::Relaxed) {
        // Dark-mode screenshots can contain a light QR code on a dark ground.
        let inverted: Vec<_> = gray.iter().map(|value| 255 - value).collect();
        hits = decode(&mut scanner, width, height, &inverted, cancel);
    }
    if cancel.load(Ordering::Relaxed) {
        hits.clear();
    }
    hits
}

fn decode(
    scanner: &mut Quirc,
    width: u32,
    height: u32,
    gray: &[u8],
    cancel: &AtomicBool,
) -> Vec<Hit> {
    let mut hits = Vec::new();
    if cancel.load(Ordering::Relaxed) {
        return hits;
    }
    for code in scanner
        .identify(width as usize, height as usize, gray)
        .take(MAX_HITS)
    {
        if cancel.load(Ordering::Relaxed) {
            break;
        }
        let Some(data) = code.ok().and_then(|code| code.decode().ok()) else {
            continue;
        };
        if !data.payload.is_empty() {
            let hit = read_hit(&data);
            if !hits.contains(&hit) {
                hits.push(hit);
            }
        }
    }
    hits
}

fn decode_separated(
    scanner: &mut Quirc,
    width: u32,
    height: u32,
    gray: &[u8],
    cancel: &AtomicBool,
) -> Vec<Hit> {
    // A failed pass with at least six finder patterns can have grouped pieces of
    // different codes. Try the two sides of the widest gap once, not every crop.
    let centers = &scanner.capstones;
    if centers.len() < 6 {
        return Vec::new();
    }
    let gap = |axis: fn(&quircs::Point) -> i32| {
        let mut values: Vec<_> = centers
            .iter()
            .map(|pattern| axis(&pattern.center))
            .collect();
        values.sort_unstable();
        values[2..values.len() - 2]
            .windows(2)
            .map(|pair| (pair[1] - pair[0], (pair[0] + pair[1]) / 2))
            .max()
            .unwrap_or_default()
    };
    let horizontal = gap(|p| p.x);
    let vertical = gap(|p| p.y);
    let (along_x, (gap, split)) = if horizontal.0 >= vertical.0 {
        (true, horizontal)
    } else {
        (false, vertical)
    };
    if gap < 32 {
        return Vec::new();
    }

    let split = split as usize;
    let width = width as usize;
    let height = height as usize;
    let limit = if along_x { width } else { height };
    if split < 21 || limit.saturating_sub(split) < 21 {
        return Vec::new();
    }

    let mut hits = Vec::new();
    let overlap = (gap as usize / 4).min(64);
    for (start, end) in [(0, split + overlap), (split - overlap.min(split), limit)] {
        if cancel.load(Ordering::Relaxed) {
            break;
        }
        let end = end.min(limit);
        let size = (end - start) as u32;
        let part = if along_x {
            let crop: Vec<_> = gray
                .chunks_exact(width)
                .flat_map(|row| &row[start..end])
                .copied()
                .collect();
            decode(scanner, size, height as u32, &crop, cancel)
        } else {
            decode(
                scanner,
                width as u32,
                size,
                &gray[start * width..end * width],
                cancel,
            )
        };
        for hit in part {
            if !hits.contains(&hit) {
                hits.push(hit);
            }
        }
        if hits.len() >= MAX_HITS {
            hits.truncate(MAX_HITS);
            break;
        }
    }
    hits
}

fn read_hit(data: &Data) -> Hit {
    let Some(text) = read_text(data) else {
        let mut hex = String::with_capacity(data.payload.len() * 3 + 4);
        hex.push_str("HEX:");
        for byte in &data.payload {
            let _ = write!(hex, " {byte:02X}");
        }
        return Hit {
            text: hex,
            url: None,
        };
    };
    let url = web_url(&text);
    // Keep multiline text and show invisible controls literally, especially bidi overrides.
    let mut display = String::with_capacity(text.len());
    for c in text.chars() {
        if (c.is_control() && !matches!(c, '\n' | '\r' | '\t'))
            || matches!(c, '\u{061c}' | '\u{200e}'..='\u{200f}' | '\u{202a}'..='\u{202e}' | '\u{2066}'..='\u{2069}')
        {
            display.extend(c.escape_unicode());
        } else {
            display.push(c);
        }
    }
    Hit { text: display, url }
}

fn read_text(data: &Data) -> Option<Cow<'_, str>> {
    use encoding_rs::*;

    let bytes = &data.payload;
    let encoding = match data.eci {
        Some(Eci::Iso8859_1) => {
            return Some(
                bytes
                    .iter()
                    .copied()
                    .map(char::from)
                    .collect::<String>()
                    .into(),
            )
        }
        Some(Eci::Iso8859_2) => ISO_8859_2,
        Some(Eci::Iso8859_3) => ISO_8859_3,
        Some(Eci::Iso8859_4) => ISO_8859_4,
        Some(Eci::Iso8859_5) => ISO_8859_5,
        Some(Eci::Iso8859_6) => ISO_8859_6,
        Some(Eci::Iso8859_7) => ISO_8859_7,
        Some(Eci::Iso8859_8) => ISO_8859_8,
        Some(Eci::Iso8859_9) => WINDOWS_1254,
        Some(Eci::Windows874) => WINDOWS_874,
        Some(Eci::Iso8859_13) => ISO_8859_13,
        Some(Eci::Iso8859_15) => ISO_8859_15,
        Some(Eci::ShiftJis) => SHIFT_JIS,
        Some(Eci::Utf8) => UTF_8,
        Some(Eci::Ibm437) => return None,
        None if data.data_type == Some(DataType::Kanji) => SHIFT_JIS,
        None => UTF_8,
    };
    encoding.decode_without_bom_handling_and_without_replacement(bytes)
}

fn web_url(text: &str) -> Option<Url> {
    if text.contains('\\') || text.chars().any(|c| c.is_whitespace() || c.is_control()) {
        return None;
    }
    let (scheme, rest) = text.split_once("://")?;
    if !(scheme.eq_ignore_ascii_case("http") || scheme.eq_ignore_ascii_case("https"))
        || rest.starts_with('/')
    {
        return None;
    }
    let parsed = Url::parse(text).ok()?;
    (parsed.host().is_some() && parsed.username().is_empty() && parsed.password().is_none())
        .then_some(parsed)
}

#[cfg(test)]
mod tests {
    use super::*;

    // Synthetic 25×25 module matrices for an example URL and UTF-8 text.
    const WEB: &str = "\
#######.#..##.#...#######
#.....#..##.....#.#.....#
#.###.#...#.###.#.#.###.#
#.###.#.....##..#.#.###.#
#.###.#..#.#.##.#.#.###.#
#.....#...#.#.#.#.#.....#
#######.#.#.#.#.#.#######
........#.##.#.##........
##.##.#..###....#.#.....#
#.#.##.#.#..######.#####.
.#.#..##..##.#.###.###..#
...###.######.#..###.####
...##.#..#.##..##.##....#
#.##....##...#.##...#..#.
#######..#.###.##.#.#####
#..###.##..#.....###.##.#
##..#.#.##....#.#####.##.
........######..#...#.##.
#######..###....#.#.#...#
#.....#...#.#.###...#..##
#.###.#.#.#.#########..##
#.###.#.#.#...#.###....##
#.###.#..##.#.##.#..#####
#.....#.###...##...##.###
#######.##...##.#.#..#..#";

    const UTF8: &str = "\
#######..#..##..#.#######
#.....#.#..#####..#.....#
#.###.#.######..#.#.###.#
#.###.#..###.###..#.###.#
#.###.#.#....##.#.#.###.#
#.....#.##.#.##...#.....#
#######.#.#.#.#.#.#######
........##..#.#..........
##.#..##..#....##.###.##.
..#.##....##..##.##.##..#
.###..#####.....#########
#..#....#.....#.#..##..#.
###.#####...#...#....#.##
..#.#.....###..##.#...#.#
#...#.###...#..###.##.#.#
.#..##...##.###.##.....##
###..###.#.#.##.######..#
........#.#.....#...#..#.
#######.#....#..#.#.#..##
#.....#..###.#.##...#.#.#
#.###.#..#.##.#######....
#.###.#.#..###.####.####.
#.###.#..#######....##..#
#.....#.##.###.####.#....
#######.#..#..##.####..##";

    fn paint(gray: &mut [u8], stride: usize, x: usize, matrix: &str) {
        const SCALE: usize = 5;
        for (row, line) in matrix.lines().enumerate() {
            assert_eq!(line.len(), 25);
            for (column, module) in line.bytes().enumerate() {
                if module == b'#' {
                    for dy in 0..SCALE {
                        let offset = (20 + row * SCALE + dy) * stride + x + column * SCALE;
                        gray[offset..offset + SCALE].fill(0);
                    }
                }
            }
        }
    }

    #[test]
    fn detects_multiple_codes_and_utf8_on_normal_or_inverted_backgrounds() {
        let mut pixels = vec![255; 320 * 165];
        paint(&mut pixels, 320, 20, WEB);
        paint(&mut pixels, 320, 175, UTF8);
        for _ in 0..2 {
            let hits = scan(320, 165, &pixels, &AtomicBool::new(false));
            assert_eq!(hits.len(), 2);
            assert!(hits
                .iter()
                .any(|hit| hit.text == "RustView 中文" && hit.url.is_none()));
            assert!(hits
                .iter()
                .any(|hit| hit.url.as_ref().and_then(Url::host_str) == Some("example.com")));
            pixels.iter_mut().for_each(|pixel| *pixel = 255 - *pixel);
        }
    }

    #[test]
    fn rejects_invalid_buffers_and_cancelled_work() {
        assert!(scan(300, 300, &[], &AtomicBool::new(false)).is_empty());
        assert!(scan(21, 21, &[0; 441], &AtomicBool::new(true)).is_empty());
        assert!(scan(MAX_SIDE + 1, 21, &[], &AtomicBool::new(false)).is_empty());
    }

    #[test]
    fn preserves_non_ascii_text_and_limits_openable_urls() {
        let hit = read_hit(&Data {
            payload: vec![0x93, 0xfa, 0x96, 0x7b],
            eci: Some(Eci::ShiftJis),
            ..Default::default()
        });
        assert_eq!(hit.text, "日本");
        let hit = read_hit(&Data {
            payload: vec![0xff, 0x0a],
            ..Default::default()
        });
        assert_eq!(hit.text, "HEX: FF 0A");
        assert!(web_url("https://example.com/path?q=1").is_some());
        for untrusted in [
            "https://example.com@other.test/",
            "https://\\other.test/",
            "https:///example.com",
            "https://examp\nle.com/",
            "javascript:alert(1)",
            "file:///C:/Windows/",
        ] {
            assert!(web_url(untrusted).is_none(), "{untrusted:?}");
        }
    }
}

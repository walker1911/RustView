use super::{paint_quad, visible_image_bounds, Pixels, Tile, Viewer, GPU_BUDGET};
use crate::{
    pyramid::Level,
    pyramid_worker::{self, Key, TileReply, Tiles, IN_FLIGHT},
    worker::PyramidInfo,
};
use egui::{Pos2, Rect};
use std::{
    collections::VecDeque,
    path::PathBuf,
    sync::mpsc::TryRecvError,
    time::{Duration, Instant},
};

const MAX_VISIBLE: usize = 512;
const MAX_TEXTURES: usize = 1024;
const MAX_FAILED: usize = 512;
const UPLOAD_BUDGET: usize = 16 * 1024 * 1024;
const READ_TIMEOUT: Duration = Duration::from_secs(10);

pub(super) struct View {
    info: PyramidInfo,
    path: PathBuf,
    managed: bool,
    tiles: Option<Tiles>,
    in_flight: VecDeque<Key>,
    last_progress: Instant,
    failed_tiles: VecDeque<Key>,
    failed: bool,
}

impl View {
    pub(super) fn new(info: PyramidInfo, path: PathBuf, managed: bool) -> Self {
        Self {
            info,
            path,
            managed,
            tiles: None,
            in_flight: VecDeque::new(),
            last_progress: Instant::now(),
            failed_tiles: VecDeque::new(),
            failed: false,
        }
    }

    pub(super) fn orientation(&self) -> (u8, bool, bool) {
        match self.info.orientation {
            2 => (0, true, false),
            3 => (2, false, false),
            4 => (0, false, true),
            5 => (1, false, true),
            6 => (1, false, false),
            7 => (1, true, false),
            8 => (3, false, false),
            _ => (0, false, false),
        }
    }

    pub(super) fn cancel(&mut self) {
        self.tiles = None;
        self.in_flight.clear();
    }

    pub(super) fn freeze(&mut self) {
        // Existing tiles belong to the displayed revision. The file may already
        // contain a different pyramid, so keep them without fetching new tiles.
        self.failed = true;
        self.cancel();
    }

    pub(super) fn close(&self) {
        if let Some(tiles) = &self.tiles {
            tiles.cancel();
        }
    }

    pub(super) fn drained(&self) -> bool {
        self.tiles.as_ref().is_none_or(Tiles::finished)
    }

    fn fail(&mut self, status: &mut String, error: String) {
        self.failed = true;
        self.cancel();
        *status = format!("{error}，继续显示全景和已读取细节");
    }
}

fn image_rect(image: &Pixels) -> Rect {
    Rect::from_min_size(
        Pos2::ZERO,
        egui::vec2(image.width as f32, image.height as f32),
    )
}

fn tile_span(level: Level, bounds: Rect, image: &Pixels) -> (u32, u32, u32, u32) {
    let ratio_x = level.width as f64 / image.width as f64 / level.tile_width as f64;
    let ratio_y = level.height as f64 / image.height as f64 / level.tile_height as f64;
    let x0 = (bounds.min.x as f64 * ratio_x).floor() as u32;
    let y0 = (bounds.min.y as f64 * ratio_y).floor() as u32;
    let x1 =
        ((bounds.max.x as f64 * ratio_x).ceil() as u32).min(level.width.div_ceil(level.tile_width));
    let y1 = ((bounds.max.y as f64 * ratio_y).ceil() as u32)
        .min(level.height.div_ceil(level.tile_height));
    (x0, y0, x1, y1)
}

fn key_for_texture((level, x, y): Key) -> (u32, u32, u32) {
    (level as u32, x, y)
}

fn tile_rect(level: Level, x: u32, y: u32, full: Rect) -> Rect {
    let point = |x, y| {
        egui::pos2(
            (f64::from(x) * f64::from(full.width()) / f64::from(level.width)) as f32,
            (f64::from(y) * f64::from(full.height()) / f64::from(level.height)) as f32,
        )
    };
    let left = x * level.tile_width;
    let top = y * level.tile_height;
    Rect::from_min_max(
        point(left, top),
        point(
            left + level.tile_width.min(level.width - left),
            top + level.tile_height.min(level.height - top),
        ),
    )
}

fn texture_uv(region: Rect, texture: Rect) -> Rect {
    Rect::from_min_max(
        ((region.min - texture.min) / texture.size()).to_pos2(),
        ((region.max - texture.min) / texture.size()).to_pos2(),
    )
}

impl Viewer {
    pub(super) fn draw_pyramid(
        &mut self,
        painter: &egui::Painter,
        image: &Pixels,
        viewport: Rect,
        center: Pos2,
    ) {
        let Some(view) = &mut self.pyramid else {
            return;
        };
        let Some(overview) = &self.overview else {
            return;
        };
        let ctx = painter.ctx();
        let full = image_rect(image);
        let paint = |texture, rect, uv| {
            paint_quad(
                painter,
                texture,
                rect,
                uv,
                image,
                center,
                self.scale,
                self.rotation,
                self.flip_x,
                self.flip_y,
            )
        };
        let bounds = visible_image_bounds(
            viewport,
            center,
            self.scale,
            self.rotation,
            self.flip_x,
            self.flip_y,
            image.width,
            image.height,
        )
        .intersect(full);
        let scale = f64::from(self.scale * ctx.pixels_per_point());
        let requested_width = (scale * f64::from(image.width)).min(f64::from(image.width));
        let requested_height = (scale * f64::from(image.height)).min(f64::from(image.height));
        let target = bounds
            .is_positive()
            .then(|| {
                let prefer = view
                    .info
                    .levels
                    .iter()
                    .rposition(|level| {
                        f64::from(level.width) >= requested_width
                            && f64::from(level.height) >= requested_height
                    })
                    .unwrap_or(0);
                view.info
                    .levels
                    .iter()
                    .copied()
                    .enumerate()
                    .skip(prefer)
                    .take_while(|(_, level)| {
                        level.width > image.preview_width || level.height > image.preview_height
                    })
                    .find_map(|(index, level)| {
                        let (x0, y0, x1, y1) = tile_span(level, bounds, image);
                        let count =
                            u64::from(x1.saturating_sub(x0)) * u64::from(y1.saturating_sub(y0));
                        let bytes =
                            count * u64::from(level.tile_width) * u64::from(level.tile_height) * 4;
                        (level.tile_width.max(level.tile_height) <= self.extra.max_texture
                            && count <= MAX_VISIBLE as u64
                            && bytes <= GPU_BUDGET as u64)
                            .then_some((index, x0, y0, x1, y1))
                    })
            })
            .flatten();

        let mut wanted = Vec::new();
        if let Some((level, x0, y0, x1, y1)) = target {
            wanted.extend((y0..y1).flat_map(|y| (x0..x1).map(move |x| (level, x, y))));
            let middle_x = i64::from(x0) + i64::from(x1) - 1;
            let middle_y = i64::from(y0) + i64::from(y1) - 1;
            wanted.sort_unstable_by_key(|&(_, x, y)| {
                (2 * i64::from(x) - middle_x).pow(2) + (2 * i64::from(y) - middle_y).pow(2)
            });
        }
        if view.tiles.is_none() && !view.failed && !wanted.is_empty() {
            match Tiles::start(
                view.path.clone(),
                view.managed,
                view.info.clone(),
                ctx.clone(),
            ) {
                Ok(tiles) => view.tiles = Some(tiles),
                Err(error) => view.fail(&mut self.status, error),
            }
        }
        let mut failure = None;
        if let Some(worker) = &view.tiles {
            let mut latest = worker.wanted.lock().unwrap();
            latest.clear();
            latest.extend_from_slice(&wanted);
            drop(latest);

            let mut uploaded = 0;
            while uploaded < UPLOAD_BUDGET {
                let reply = match worker.replies.try_recv() {
                    Ok(Ok(reply)) => reply,
                    Ok(Err(error)) => {
                        failure = Some(error);
                        break;
                    }
                    Err(TryRecvError::Disconnected) => {
                        failure = Some("TIFF 细节读取已停止".into());
                        break;
                    }
                    Err(TryRecvError::Empty) => break,
                };
                let Some(key) = view.in_flight.pop_front() else {
                    break;
                };
                view.last_progress = Instant::now();
                match reply {
                    TileReply::Superseded => continue,
                    TileReply::Unreadable => {
                        if view.failed_tiles.len() == MAX_FAILED {
                            view.failed_tiles.pop_front();
                        }
                        view.failed_tiles.push_back(key);
                        self.status = "部分 TIFF 分块无法读取，继续显示较低分辨率".into();
                    }
                    TileReply::Pixels(rgba) if wanted.contains(&key) => {
                        let Some((width, height)) = pyramid_worker::tile_size(&view.info, key)
                        else {
                            continue;
                        };
                        let bytes = rgba.len();
                        while self.tile_bytes + bytes > GPU_BUDGET
                            || self.tiles.len() >= MAX_TEXTURES
                        {
                            let Some(old) = self
                                .tiles
                                .iter()
                                .min_by_key(|(_, tile)| tile.used)
                                .map(|(key, _)| *key)
                            else {
                                break;
                            };
                            self.tile_bytes -= self.tiles.remove(&old).unwrap().bytes;
                        }
                        let texture = ctx.load_texture(
                            "pyramid-tile",
                            egui::ColorImage::from_rgba_unmultiplied(
                                [width as usize, height as usize],
                                &rgba,
                            ),
                            egui::TextureOptions::LINEAR,
                        );
                        self.tick += 1;
                        self.tiles.insert(
                            key_for_texture(key),
                            Tile {
                                texture,
                                bytes,
                                used: self.tick,
                            },
                        );
                        self.tile_bytes += bytes;
                        uploaded += bytes;
                    }
                    TileReply::Pixels(_) => {}
                }
            }
            if failure.is_none()
                && !view.in_flight.is_empty()
                && view.last_progress.elapsed() >= READ_TIMEOUT
            {
                failure = Some("TIFF 细节读取超时".into());
            }
            if failure.is_none() {
                for key in wanted.iter().copied() {
                    if view.in_flight.len() == IN_FLIGHT {
                        break;
                    }
                    if self.tiles.contains_key(&key_for_texture(key))
                        || view.in_flight.contains(&key)
                        || view.failed_tiles.contains(&key)
                    {
                        continue;
                    }
                    if worker.requests.try_send(Some(key)).is_ok() {
                        if view.in_flight.is_empty() {
                            view.last_progress = Instant::now();
                        }
                        view.in_flight.push_back(key);
                    }
                }
                if !view.in_flight.is_empty() {
                    ctx.request_repaint_after(Duration::from_millis(40));
                }
            }
        }
        if let Some(error) = failure {
            view.fail(&mut self.status, error);
        }

        // Paint each region once so transparent detail never blends with itself.
        // Prefer a cached coarser tile, then use the overview for the same region.
        if let Some((index, x0, y0, x1, y1)) = target {
            let level = view.info.levels[index];
            for y in y0..y1 {
                for x in x0..x1 {
                    let rect = tile_rect(level, x, y, full);
                    if let Some(tile) = self.tiles.get_mut(&key_for_texture((index, x, y))) {
                        self.tick += 1;
                        tile.used = self.tick;
                        paint(tile.texture.id(), rect, texture_uv(rect, rect));
                    } else {
                        let fallback = view
                            .info
                            .levels
                            .iter()
                            .copied()
                            .enumerate()
                            .skip(index + 1)
                            .take_while(|(_, level)| {
                                level.width > image.preview_width
                                    || level.height > image.preview_height
                            })
                            .find_map(|(index, level)| {
                                let (x0, y0, x1, y1) = tile_span(level, rect, image);
                                if x1 != x0 + 1 || y1 != y0 + 1 {
                                    return None;
                                }
                                let tile = self.tiles.get_mut(&key_for_texture((index, x0, y0)))?;
                                self.tick += 1;
                                tile.used = self.tick;
                                Some((tile.texture.id(), tile_rect(level, x0, y0, full)))
                            })
                            .unwrap_or((overview.id(), full));
                        paint(fallback.0, rect, texture_uv(rect, fallback.1));
                    }
                }
            }
        } else {
            paint(overview.id(), full, texture_uv(full, full));
        }
    }
}

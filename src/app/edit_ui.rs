use super::*;
use crate::editing::{ColorDepth, Edit};

impl Viewer {
    pub(super) fn open_edit_dialog(&mut self, id: u16) {
        if self.current_key.map(|k| k.0) != self.selected {
            if let Some(index) = self.selected.filter(|i| {
                self.entries
                    .get(*i)
                    .is_some_and(|e| !e.directory && !e.archive)
            }) {
                self.extra.pending_edit_id = Some(id);
                self.load_index(index, self.full_raw);
            }
            return;
        }
        if self.current.is_none() {
            self.status = "请先打开图片".into();
            return;
        }
        if self.extra.animated {
            self.extra.animation_paused = true;
        }
        if let Some(image) = &self.current {
            (self.extra.width, self.extra.height) = if self.rotation % 2 == 0 {
                (image.width, image.height)
            } else {
                (image.height, image.width)
            };
        }
        self.extra.value = if matches!(id, 2046 | 21851) {
            0.0
        } else {
            100.0
        };
        self.extra.edit_dialog = Some(id);
        self.extra.workspace.batch_indices = if matches!(id, 2043..=2046 | 10576) {
            let selected = self
                .extra
                .visible
                .iter()
                .copied()
                .filter(|i| {
                    self.extra.selection.contains(i)
                        && self
                            .entries
                            .get(*i)
                            .is_some_and(|e| !e.directory && !e.archive)
                })
                .collect::<Vec<_>>();
            if selected.is_empty() {
                self.selected.into_iter().collect()
            } else {
                selected
            }
        } else {
            Vec::new()
        };
        self.extra.workspace.select_command(id);
        self.browser = false;
    }
    pub(super) fn make_view_load(&self, index: usize, full_raw: bool) -> Option<Request> {
        let source = self.make_load(index, full_raw)?;
        Some(
            if self.selected == Some(index) && self.extra.page_index > 0 {
                Request::Page {
                    source: Box::new(source),
                    index: self.extra.page_index,
                    last: false,
                }
            } else {
                source
            },
        )
    }
    pub(super) fn document_edits(&self) -> Vec<Edit> {
        let mut edits = self.extra.edits.clone();
        if self.flip_x {
            edits.push(Edit::FlipH);
        }
        if self.flip_y {
            edits.push(Edit::FlipV);
        }
        if self.rotation != 0 {
            edits.push(Edit::Rotate90 {
                turns: self.rotation,
            });
        }
        edits
    }
    pub(super) fn render_edits(&mut self) {
        self.extra.workspace.preview_due = None;
        if self.current_key.map(|k| k.0) != self.selected || self.current.is_none() {
            return;
        }
        self.extra.edits = self.document_edits();
        self.rotation = 0;
        self.flip_x = false;
        self.flip_y = false;
        let Some(index) = self.selected else {
            return;
        };
        let Some(source) = self.make_view_load(index, self.full_raw) else {
            return;
        };
        self.extra.edited = self.extra.workspace.is_dirty(&self.extra.edits);
        self.extra.workspace.preview_valid = false;
        self.cache.clear();
        self.stop_animation();
        self.queue(
            Request::Render {
                source: Box::new(source),
                edits: self.extra.edits.clone(),
                settings: self.extra.workspace.export.clone(),
                raw: self.extra.workspace.raw.clone(),
                save: None,
            },
            Operation::Rendered { index },
        );
    }
    pub(super) fn apply_edit(&mut self, edit: Edit) {
        let edits = self.document_edits();
        if edits.len() >= 128 {
            self.error = Some("编辑已达128步，请先保存副本".into());
            return;
        }
        self.extra.edits = edits;
        self.rotation = 0;
        self.flip_x = false;
        self.flip_y = false;
        self.extra.edits.push(edit);
        self.extra.redo_edits.clear();
        self.render_edits();
    }
    pub(super) fn undo_edit(&mut self, redo: bool) {
        if redo {
            if let Some(e) = self.extra.redo_edits.pop() {
                self.extra.edits.push(e);
            } else {
                return;
            }
        } else if let Some(e) = self.extra.edits.pop() {
            self.extra.redo_edits.push(e);
        } else {
            return;
        }
        self.render_edits();
    }
    pub(super) fn color_depth(&mut self, id: u16) {
        let mode = match id {
            2315 => ColorDepth::BlackWhite1,
            2316 => ColorDepth::Gray4,
            2317 => ColorDepth::Gray8,
            2318 => ColorDepth::Indexed4,
            2319 => ColorDepth::Indexed8,
            2320 => ColorDepth::Rgb15,
            2321 => ColorDepth::Rgb24,
            2322 => ColorDepth::Gray16,
            _ => ColorDepth::Rgb48,
        };
        self.apply_edit(Edit::ColorDepth { mode });
    }
    pub(super) fn draw_inspectors(&mut self, ctx: &egui::Context) {
        if self.extra.page_view {
            let mut open = true;
            let mut command = None;
            egui::Window::new("多页图像")
                .open(&mut open)
                .show(ctx, |ui| {
                    ui.label(format!(
                        "当前第 {}／{} 页",
                        self.extra.page_index + 1,
                        self.extra.page_total
                    ));
                    ui.horizontal(|ui| {
                        for (label, id) in [
                            ("第一页", 2248),
                            ("上一页", 2250),
                            ("下一页", 2249),
                            ("最后一页", 2251),
                        ] {
                            if ui.button(label).clicked() {
                                command = Some(id);
                            }
                        }
                    });
                });
            self.extra.page_view = open;
            if let Some(id) = command {
                self.legacy_command(id, ctx);
            }
        }
        if self.extra.navigator {
            egui::Window::new("导航")
                .open(&mut self.extra.navigator)
                .show(ctx, |ui| {
                    if let Some(t) = &self.overview {
                        let s = t.size_vec2();
                        ui.image((t.id(), s * (200.0 / s.max_elem())));
                    }
                    if ui.button("返回图像中心").clicked() {
                        self.pan = Vec2::ZERO;
                    }
                });
        }
        if self.extra.histogram {
            egui::Window::new("RGB 统计图")
                .open(&mut self.extra.histogram)
                .show(ctx, |ui| {
                    if let Some(i) = &self.current {
                        if let Ok(bins) = if i.preview.is_empty() {
                            crate::editing::histogram(i.width, i.height, &i.rgba)
                        } else {
                            crate::editing::histogram(i.preview_width, i.preview_height, &i.preview)
                        } {
                            let (r, _) =
                                ui.allocate_exact_size(egui::vec2(360.0, 160.0), Sense::hover());
                            for (channel, color) in
                                bins.iter()
                                    .zip([Color32::RED, Color32::GREEN, Color32::BLUE])
                            {
                                let max = channel.iter().copied().max().unwrap_or(1).max(1) as f32;
                                let points = channel
                                    .iter()
                                    .enumerate()
                                    .map(|(x, v)| {
                                        egui::pos2(
                                            r.left() + r.width() * x as f32 / 255.0,
                                            r.bottom() - r.height() * (*v as f32 / max),
                                        )
                                    })
                                    .collect::<Vec<_>>();
                                ui.painter().add(egui::Shape::line(
                                    points,
                                    egui::Stroke::new(1.0_f32, color),
                                ));
                            }
                            ui.small("基于当前预览的RGB分布");
                        }
                    }
                });
        }
    }
    pub(super) fn external_open(&mut self, edit: bool) {
        let Some(Source::Local(path)) = self.selected_sources().into_iter().next() else {
            self.error = Some("包内文件请先用 Alt+C 提取到文件夹后调用外部程序".into());
            return;
        };
        if let Err(e) = shell_verb(&path, if edit { "edit" } else { "open" }) {
            self.error = Some(e);
        }
    }
    pub(super) fn wallpaper(&mut self, id: u16) {
        let root = std::env::current_exe()
            .ok()
            .and_then(|p| p.parent().map(|p| p.join("RustView-data")));
        let Some(root) = root else {
            return;
        };
        let backup = root.join("desktop-backup.json");
        if matches!(id, 2180 | 2267) {
            self.write_job(Box::new(move || {
                crate::desktop::restore(&backup)?;
                Ok("已还原原有壁纸".into())
            }));
            return;
        }
        let Some(source) = self.selected.and_then(|i| self.make_load(i, self.full_raw)) else {
            return;
        };
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        let path = root.join(format!("wallpaper-{stamp}.bmp"));
        let mode = match id {
            2179 | 2266 => crate::desktop::WallpaperMode::Tile,
            21897 => crate::desktop::WallpaperMode::Stretch,
            _ => crate::desktop::WallpaperMode::Center,
        };
        self.write_job(Box::new(move || {
            std::fs::create_dir_all(&root).map_err(|e| e.to_string())?;
            let output = worker::execute(
                Request::Edit {
                    source: Box::new(source),
                    edits: Vec::new(),
                    save: Some(path.clone()),
                },
                Arc::new(AtomicBool::new(false)),
            )?;
            if let Some(e) = output.reply.error {
                return Err(e);
            }
            crate::desktop::set(&path, mode, &backup)?;
            Ok("已设置壁纸；Alt+W还原原有壁纸".into())
        }));
    }
}

use crate::fileops::Source;
#[cfg(windows)]
fn shell_verb(path: &Path, verb: &str) -> Result<(), String> {
    use std::os::windows::ffi::OsStrExt;
    let path: Vec<_> = path.as_os_str().encode_wide().chain(Some(0)).collect();
    let verb: Vec<_> = verb.encode_utf16().chain(Some(0)).collect();
    let result = unsafe {
        windows_sys::Win32::UI::Shell::ShellExecuteW(
            std::ptr::null_mut(),
            verb.as_ptr(),
            path.as_ptr(),
            std::ptr::null(),
            std::ptr::null(),
            1,
        )
    };
    if result as usize <= 32 {
        Err(format!(
            "Windows无法执行该文件操作（错误码 {}），请检查文件关联或默认程序",
            result as usize
        ))
    } else {
        Ok(())
    }
}
#[cfg(not(windows))]
fn shell_verb(_path: &Path, _verb: &str) -> Result<(), String> {
    Err("此操作仅支持Windows".into())
}

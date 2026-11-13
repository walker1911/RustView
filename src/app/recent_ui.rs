use super::Viewer;
use crate::catalog::{Recent, RecentKind};
use std::path::{Path, PathBuf};

#[derive(Default)]
pub(super) struct State {
    pub(super) open: bool,
    rows: Vec<Recent>,
    error: Option<String>,
    opening: Option<Opening>,
    opened: Option<Opening>,
}

struct Opening {
    path: PathBuf,
    kind: RecentKind,
    image_index: Option<usize>,
    listed: bool,
}

impl Viewer {
    pub(super) fn track_recent_open(&mut self, path: PathBuf, kind: RecentKind) {
        self.recent_ui.opening = Some(Opening {
            path,
            kind,
            image_index: (kind == RecentKind::Image).then_some(0),
            listed: false,
        });
    }

    pub(super) fn track_management_image(&mut self, index: usize) {
        if self.is_archive_source() {
            return;
        }
        if let Some((source, entry)) = self.source.as_ref().zip(self.entries.get(index)) {
            self.recent_ui.opening = Some(Opening {
                path: source.join(&entry.name),
                kind: RecentKind::Image,
                image_index: Some(index),
                listed: false,
            });
        }
    }

    pub(super) fn cancel_recent_open(&mut self) {
        self.recent_ui.opening = None;
    }

    pub(super) fn recent_source_listed(&mut self, ctx: &egui::Context) {
        let Some(opening) = &mut self.recent_ui.opening else {
            return;
        };
        if self.source.as_ref() != Some(&opening.path) {
            return;
        }
        opening.listed = true;
        if opening.kind == RecentKind::Folder {
            self.recent_opened(ctx);
        }
    }

    pub(super) fn recent_image_drawn(&mut self, index: usize, ctx: &egui::Context) {
        if self
            .recent_ui
            .opening
            .as_ref()
            .is_some_and(|opening| match opening.kind {
                RecentKind::Image => opening.image_index == Some(index) && !self.browser,
                RecentKind::Archive => {
                    opening.listed && self.source.as_ref() == Some(&opening.path) && !self.browser
                }
                RecentKind::Folder => false,
            })
        {
            self.recent_opened(ctx);
        }
    }

    fn recent_opened(&mut self, ctx: &egui::Context) {
        self.recent_ui.opened = self.recent_ui.opening.take();
        ctx.request_repaint();
    }

    pub(super) fn flush_recent_open(&mut self, force: bool) {
        if self.recent_ui.opened.is_none() {
            return;
        }
        // The initial image can already be drawn while the catalog is still deferred.
        if !force && self.startup_image.is_some() && self.extra.catalog.is_none() {
            return;
        }
        self.initialize_catalog();
        if let Some(opening) = self.recent_ui.opened.take() {
            if let Some(catalog) = self.extra.catalog.as_mut() {
                if let Err(error) = catalog.remember_recent_source(&opening.path, opening.kind) {
                    self.status = format!("无法保存最近打开记录：{error}");
                }
            }
        }
    }

    pub(super) fn open_recent_ui(&mut self) {
        self.initialize_catalog();
        self.flush_recent_open(false);
        self.save_archive_resume(true);
        self.recent_ui.open = true;
        self.recent_ui.rows.clear();
        self.recent_ui.error = None;
        match self
            .extra
            .catalog
            .as_ref()
            .map(|catalog| catalog.recent_sources())
        {
            Some(Ok(rows)) => self.recent_ui.rows = rows,
            Some(Err(error)) => {
                self.recent_ui.error = Some(format!("无法读取最近打开记录：{error}"))
            }
            None => self.recent_ui.error = Some("无法访问本地目录库。".into()),
        }
    }

    pub(super) fn draw_recent_ui(&mut self, ctx: &egui::Context) {
        if self.password_dialog {
            self.recent_ui.open = false;
        }
        if !self.recent_ui.open {
            return;
        }
        let mut close = false;
        let modal = egui::Modal::new(egui::Id::new("recent-open")).show(ctx, |ui| {
            ui.set_width((ctx.screen_rect().width() - 48.0).clamp(180.0, 520.0));
            ui.heading("最近打开");
            let mut target = None;
            if let Some(error) = &self.recent_ui.error {
                ui.label(error);
            }
            if self.recent_ui.rows.is_empty() && self.recent_ui.error.is_none() {
                ui.label("还没有最近打开的图片、文件夹或压缩包。");
            }
            egui::ScrollArea::vertical()
                .max_height(360.0)
                .show(ui, |ui| {
                    for recent in &self.recent_ui.rows {
                        let path = display_path(&recent.path);
                        let name = recent
                            .path
                            .file_name()
                            .and_then(|name| name.to_str())
                            .unwrap_or(&path);
                        let kind = match recent.kind {
                            RecentKind::Image => "图片",
                            RecentKind::Folder => "文件夹",
                            RecentKind::Archive => "压缩包",
                        };
                        let label = format!("{name}  ·  {kind}");
                        if ui
                            .add(egui::Button::new(label).truncate())
                            .on_hover_text(path)
                            .clicked()
                        {
                            target = Some((recent.path.clone(), recent.kind));
                        }
                        if let Some(entry) = &recent.entry {
                            ui.add(
                                egui::Label::new(
                                    egui::RichText::new(format!("上次看到 {entry}")).small(),
                                )
                                .truncate(),
                            )
                            .on_hover_text(entry);
                        }
                    }
                });
            close = ui.button("关闭").clicked();
            target
        });
        if close || modal.should_close() {
            self.recent_ui.open = false;
        }
        if let Some((path, kind)) = modal.inner {
            let available = match kind {
                RecentKind::Folder => path.is_dir(),
                RecentKind::Image | RecentKind::Archive => path.is_file(),
            };
            if available {
                self.recent_ui.open = false;
                self.open_path(path);
            } else {
                self.recent_ui.error = Some(format!("找不到 {}", display_path(&path)));
            }
        }
    }
}

fn display_path(path: &Path) -> String {
    let path = path.to_string_lossy();
    if let Some(network_path) = path.strip_prefix(r"\\?\UNC\") {
        format!(r"\\{network_path}")
    } else {
        path.strip_prefix(r"\\?\").unwrap_or(&path).into()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::catalog::Catalog;

    #[test]
    fn records_the_displayed_open_once_and_discards_a_cancelled_open() {
        let directory = tempfile::tempdir().unwrap();
        let first = directory.path().join("open.jpg");
        let cancelled = directory.path().join("cancelled.jpg");
        std::fs::write(&first, b"test image identity").unwrap();
        std::fs::write(&cancelled, b"test image identity").unwrap();
        let database = directory.path().join("catalog.sqlite");
        let ctx = egui::Context::default();
        let mut viewer = Viewer::empty();
        viewer.browser = false;
        viewer.startup_image = Some(super::super::startup::ImageStartup::new(0));
        viewer.track_recent_open(first.clone(), RecentKind::Image);
        viewer.recent_image_drawn(1, &ctx);
        viewer.flush_recent_open(false);
        assert!(viewer.recent_ui.opened.is_none());
        viewer.recent_image_drawn(0, &ctx);
        viewer.flush_recent_open(false);
        assert!(viewer.recent_ui.opened.is_some());

        viewer.extra.catalog = Some(Catalog::open(&database).unwrap());
        viewer.flush_recent_open(false);
        viewer.track_recent_open(cancelled, RecentKind::Image);
        viewer.cancel_recent_open();
        viewer.recent_image_drawn(0, &ctx);
        viewer.recent_image_drawn(1, &ctx);
        viewer.flush_recent_open(false);
        let rows = viewer
            .extra
            .catalog
            .as_ref()
            .unwrap()
            .recent_sources()
            .unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].path, first.canonicalize().unwrap());
        assert!(viewer.recent_ui.opened.is_none());
    }

    #[test]
    fn reopening_an_archive_requires_a_new_listing_before_recording() {
        let mut viewer = Viewer::empty();
        let archive = PathBuf::from("album.7z");
        viewer.source = Some(archive.clone());
        viewer.browser = false;
        viewer.track_recent_open(archive, RecentKind::Archive);
        let ctx = egui::Context::default();
        viewer.recent_image_drawn(0, &ctx);
        assert!(viewer.recent_ui.opened.is_none());
        viewer.recent_source_listed(&ctx);
        viewer.recent_image_drawn(0, &ctx);
        assert!(viewer.recent_ui.opened.is_some());
    }
}

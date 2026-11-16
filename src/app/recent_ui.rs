use super::Viewer;
use crate::catalog::{Catalog, Recent, RecentKind};
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    sync::mpsc::{self, Receiver, TryRecvError},
    time::{Duration, Instant, SystemTime},
};

const FOLDER_SAVE_DELAY: Duration = Duration::from_millis(350);
const MAX_PENDING_FOLDERS: usize = 20;

#[derive(Default)]
pub(super) struct State {
    pub(super) open: bool,
    rows: Vec<Recent>,
    pinned: Vec<Recent>,
    error: Option<String>,
    opening: Option<Opening>,
    opened: Option<Opening>,
    folder: Option<FolderPosition>,
    folder_writes: BTreeMap<PathBuf, FolderWrite>,
    folder_writer: Option<Receiver<Result<(), String>>>,
    loading_rows: bool,
}

struct Opening {
    path: PathBuf,
    kind: RecentKind,
    image_index: Option<usize>,
    listed: bool,
}

struct FolderPosition {
    path: PathBuf,
    last_drawn: Option<String>,
    recorded_open: bool,
}

struct FolderWrite {
    filename: String,
    viewed_at: SystemTime,
    due: Instant,
    record_open: bool,
}

enum PanelAction {
    Open(Recent, bool),
    Pin(PathBuf, RecentKind),
    Unpin(PathBuf),
    PickFolder,
    PickArchive,
}

impl State {
    pub(super) fn is_local_folder(&self) -> bool {
        self.folder.is_some()
    }
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
            if let Some(folder) = &mut self.recent_ui.folder {
                folder.recorded_open = true;
            }
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

    pub(super) fn change_recent_folder(&mut self, path: Option<&Path>) {
        if self
            .recent_ui
            .folder
            .as_ref()
            .map(|folder| folder.path.as_path())
            == path
        {
            return;
        }
        self.save_recent_folder(true);
        self.recent_ui.folder = path.map(|path| FolderPosition {
            path: path.to_owned(),
            last_drawn: None,
            recorded_open: false,
        });
    }

    pub(super) fn folder_image_drawn(&mut self, index: usize, ctx: &egui::Context) {
        if self.browser
            || self.selected != Some(index)
            || self.current_key.map(|key| key.0) != Some(index)
        {
            return;
        }
        let Some(folder) = &mut self.recent_ui.folder else {
            return;
        };
        let Some(entry) = self
            .entries
            .get(index)
            .filter(|entry| !entry.directory && !entry.archive)
        else {
            return;
        };
        if self.source.as_ref() != Some(&folder.path)
            || folder.last_drawn.as_ref() == Some(&entry.name)
        {
            return;
        }
        let now = Instant::now();
        folder.last_drawn = Some(entry.name.clone());
        let pending = self
            .recent_ui
            .folder_writes
            .entry(folder.path.clone())
            .or_insert_with(|| FolderWrite {
                filename: String::new(),
                viewed_at: SystemTime::now(),
                due: now + FOLDER_SAVE_DELAY,
                record_open: false,
            });
        entry.name.clone_into(&mut pending.filename);
        pending.viewed_at = SystemTime::now();
        pending.due = now + FOLDER_SAVE_DELAY;
        pending.record_open |= !folder.recorded_open;
        folder.recorded_open = true;
        if self.recent_ui.folder_writes.len() > MAX_PENDING_FOLDERS {
            if let Some(oldest) = self
                .recent_ui
                .folder_writes
                .iter()
                .min_by_key(|(_, job)| job.viewed_at)
                .map(|(path, _)| path.clone())
            {
                self.recent_ui.folder_writes.remove(&oldest);
            }
        }
        ctx.request_repaint_after(FOLDER_SAVE_DELAY);
    }

    pub(super) fn save_recent_folder(&mut self, force: bool) {
        if force {
            let now = Instant::now();
            for pending in self.recent_ui.folder_writes.values_mut() {
                pending.due = now;
            }
        }
    }

    pub(super) fn recent_folder_drained(&self) -> bool {
        self.recent_ui.folder_writes.is_empty() && self.recent_ui.folder_writer.is_none()
    }

    pub(super) fn poll_recent_folder(&mut self, ctx: &egui::Context) {
        if let Some(receiver) = &self.recent_ui.folder_writer {
            let result = match receiver.try_recv() {
                Ok(result) => result,
                Err(TryRecvError::Empty) => return,
                Err(TryRecvError::Disconnected) => Err("后台任务已结束".into()),
            };
            self.recent_ui.folder_writer = None;
            if let Err(error) = result {
                self.status = format!("无法保存文件夹续看位置：{error}");
            }
        }
        let now = Instant::now();
        let Some(due) = self
            .recent_ui
            .folder_writes
            .values()
            .map(|pending| pending.due)
            .min()
        else {
            return;
        };
        if due > now {
            ctx.request_repaint_after(due - now);
            return;
        }
        if !self.closing && self.startup_image.is_some() && self.extra.catalog.is_none() {
            return;
        }
        let Some(database) = self
            .extra
            .catalog
            .as_ref()
            .and_then(Catalog::path)
            .map(Path::to_owned)
        else {
            self.recent_ui.folder_writes.clear();
            return;
        };
        let pending = &mut self.recent_ui.folder_writes;
        let ready: Vec<_> = pending
            .iter()
            .filter(|(_, job)| job.due <= now)
            .map(|(path, _)| path.clone())
            .collect();
        let mut batch: Vec<_> = ready
            .into_iter()
            .filter_map(|path| pending.remove_entry(&path))
            .collect();
        batch.sort_by_key(|(_, job)| job.viewed_at);
        let (sender, receiver) = mpsc::sync_channel(1);
        self.recent_ui.folder_writer = Some(receiver);
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            let result = Catalog::open(&database).and_then(|mut catalog| {
                let mut result = Ok(());
                for (path, job) in batch {
                    let saved = catalog.remember_folder_resume(&path, &job.filename, job.viewed_at);
                    let recent = if job.record_open {
                        catalog.remember_recent_source(&path, RecentKind::Folder)
                    } else {
                        Ok(())
                    };
                    result = result.and(saved).and(recent);
                }
                result
            });
            let _ = sender.send(result);
            ctx.request_repaint();
        });
    }

    pub(super) fn open_recent_ui(&mut self) {
        self.initialize_catalog();
        self.flush_recent_open(false);
        self.save_archive_resume(true);
        self.save_recent_folder(true);
        self.recent_ui.open = true;
        self.recent_ui.rows.clear();
        self.recent_ui.pinned.clear();
        self.recent_ui.error = None;
        self.recent_ui.loading_rows = !self.recent_folder_drained();
        if !self.recent_ui.loading_rows {
            self.read_recent_rows();
        }
    }

    fn read_recent_rows(&mut self) {
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
        self.read_pinned_rows();
    }

    fn read_pinned_rows(&mut self) {
        if let Some(catalog) = self.extra.catalog.as_ref() {
            match catalog.pinned_sources() {
                Ok(pinned) => self.recent_ui.pinned = pinned,
                Err(error) => self.recent_ui.error = Some(format!("无法读取常用位置：{error}")),
            }
        }
    }

    fn change_pin(&mut self, path: &Path, kind: Option<RecentKind>) {
        let Some(catalog) = self.extra.catalog.as_mut() else {
            self.recent_ui.error = Some("无法访问本地目录库。".into());
            return;
        };
        let result = match kind {
            Some(kind) => catalog.pin_source(path, kind),
            None => catalog.unpin_source(path),
        };
        self.recent_ui.error = result.err();
        if self.recent_ui.error.is_none() {
            self.read_pinned_rows();
        }
    }

    pub(super) fn draw_recent_ui(&mut self, ctx: &egui::Context) {
        if self.password_dialog {
            self.recent_ui.open = false;
        }
        if !self.recent_ui.open {
            return;
        }
        if self.recent_ui.loading_rows && self.recent_folder_drained() {
            self.recent_ui.loading_rows = false;
            self.read_recent_rows();
        }
        let mut close = false;
        let modal = egui::Modal::new(egui::Id::new("recent-open")).show(ctx, |ui| {
            ui.set_width((ctx.screen_rect().width() - 48.0).clamp(180.0, 520.0));
            ui.heading("最近打开");
            let mut action = None;
            if let Some(error) = &self.recent_ui.error {
                ui.label(error);
            }
            if self.recent_ui.loading_rows {
                ui.label("正在读取最近打开记录…");
            } else {
                egui::ScrollArea::vertical()
                    .max_height(360.0)
                    .show(ui, |ui| {
                        ui.strong("常用位置");
                        ui.horizontal_wrapped(|ui| {
                            if ui.button("添加文件夹…").clicked() {
                                action = Some(PanelAction::PickFolder);
                            }
                            if ui.button("添加压缩包…").clicked() {
                                action = Some(PanelAction::PickArchive);
                            }
                        });
                        for pinned in &self.recent_ui.pinned {
                            if let Some(clicked) = draw_location_row(ui, pinned, true) {
                                action = Some(clicked);
                            }
                        }
                        ui.add_space(8.0);
                        ui.strong("最近记录");
                        let mut any_recent = false;
                        for recent in &self.recent_ui.rows {
                            if !self
                                .recent_ui
                                .pinned
                                .iter()
                                .any(|pin| pin.path == recent.path)
                            {
                                any_recent = true;
                                if let Some(clicked) = draw_location_row(ui, recent, false) {
                                    action = Some(clicked);
                                }
                            }
                        }
                        if !any_recent && self.recent_ui.error.is_none() {
                            ui.label("没有其他最近打开的项目。");
                        }
                    });
            }
            close = ui.button("关闭").clicked();
            action
        });
        if close || modal.should_close() {
            self.recent_ui.open = false;
        }
        match modal.inner {
            Some(PanelAction::Open(recent, from_start)) => {
                self.activate_recent_row(&recent, from_start);
            }
            Some(PanelAction::Pin(path, kind)) => self.change_pin(&path, Some(kind)),
            Some(PanelAction::Unpin(path)) => self.change_pin(&path, None),
            Some(PanelAction::PickFolder) => {
                if let Some(path) = rfd::FileDialog::new()
                    .set_title("添加常用文件夹")
                    .pick_folder()
                {
                    self.change_pin(&path, Some(RecentKind::Folder));
                }
            }
            Some(PanelAction::PickArchive) => {
                if let Some(path) = rfd::FileDialog::new()
                    .set_title("添加常用压缩包")
                    .add_filter("压缩包", &["zip", "rar", "7z", "cbz", "cbr", "cb7"])
                    .pick_file()
                {
                    if super::worker::is_archive(&path) {
                        self.change_pin(&path, Some(RecentKind::Archive));
                    } else {
                        self.recent_ui.error = Some("请选择 ZIP、RAR 或 7z 压缩包。".into());
                    }
                }
            }
            None => {}
        }
    }

    fn activate_recent_row(&mut self, recent: &Recent, from_start: bool) {
        let available = match recent.kind {
            RecentKind::Folder => recent.path.is_dir(),
            RecentKind::Image | RecentKind::Archive => recent.path.is_file(),
        };
        if !available {
            self.recent_ui.error = Some(format!("找不到 {}", display_path(&recent.path)));
            return;
        }
        let path = recent.path.clone();
        self.recent_ui.open = false;
        if recent.kind == RecentKind::Folder {
            if from_start {
                self.open_recent_folder_start(path);
            } else if let Some(filename) = &recent.entry {
                let image = path.join(filename);
                if image.is_file() && crate::imaging::is_supported_image(filename) {
                    self.open_path(image);
                } else {
                    self.open_recent_folder_start(path);
                }
            } else {
                self.open_path(path);
            }
        } else {
            self.open_path_with_start(path, from_start);
        }
    }
}

fn draw_location_row(ui: &mut egui::Ui, recent: &Recent, pinned: bool) -> Option<PanelAction> {
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
    let tooltip = if recent.entry.is_some() {
        format!("续看\n{path}")
    } else {
        path.clone()
    };
    let mut action = None;
    if ui
        .add(egui::Button::new(label).truncate())
        .on_hover_text(tooltip)
        .clicked()
    {
        let from_start = pinned && recent.kind == RecentKind::Folder && recent.entry.is_none();
        action = Some(PanelAction::Open(recent.clone(), from_start));
    }
    if recent.kind != RecentKind::Image {
        ui.horizontal(|ui| {
            if ui
                .add(egui::Button::new("从头打开").small())
                .on_hover_text(format!("从第一张图片打开 {name}"))
                .clicked()
            {
                action = Some(PanelAction::Open(recent.clone(), true));
            }
            let pin_label = if pinned { "取消置顶" } else { "置顶" };
            if ui.add(egui::Button::new(pin_label).small()).clicked() {
                action = Some(if pinned {
                    PanelAction::Unpin(recent.path.clone())
                } else {
                    PanelAction::Pin(recent.path.clone(), recent.kind)
                });
            }
            if let Some(entry) = &recent.entry {
                ui.add(
                    egui::Label::new(egui::RichText::new(format!("上次看到 {entry}")).small())
                        .truncate(),
                )
                .on_hover_text(entry);
            }
        });
    }
    action
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

    #[test]
    fn recent_folder_opens_saved_image_directly_or_lists_when_starting_over() {
        use super::super::Operation;

        let folder = tempfile::tempdir().unwrap();
        let file = folder.path().join("照片.png");
        std::fs::write(&file, b"test image identity").unwrap();
        for (saved, from_start, expect_direct) in [
            ("照片.png", false, true),
            ("照片.png", true, false),
            ("已删除.png", false, false),
        ] {
            let mut viewer = Viewer::empty();
            let recent = Recent {
                path: folder.path().to_owned(),
                kind: RecentKind::Folder,
                entry: Some(saved.to_owned()),
            };
            viewer.activate_recent_row(&recent, from_start);
            let task = viewer.pending.as_ref().unwrap();
            if expect_direct {
                assert!(matches!(task.operation, Operation::Load { index: 0, .. }));
                assert_eq!(viewer.entries[0].name, saved);
            } else {
                assert!(matches!(
                    task.operation,
                    Operation::List {
                        from_start: true,
                        management: false,
                        ..
                    }
                ));
            }
        }
    }

    #[test]
    fn pinned_archives_use_the_requested_start_without_a_saved_password() {
        use super::super::{Operation, Request};

        let directory = tempfile::tempdir().unwrap();
        let database = directory.path().join("catalog.sqlite");
        for extension in ["zip", "rar", "7z"] {
            let archive = directory.path().join(format!("收藏.{extension}"));
            std::fs::write(&archive, b"test archive identity").unwrap();
            let mut catalog = Catalog::open(&database).unwrap();
            catalog.pin_source(&archive, RecentKind::Archive).unwrap();
            catalog
                .remember_archive_resume(&archive, "第二章/封面.png", SystemTime::now())
                .unwrap();

            let mut viewer = Viewer::empty();
            viewer.extra.catalog = Some(catalog);
            viewer.open_recent_ui();
            let pinned = viewer.recent_ui.pinned[0].clone();
            assert_eq!(pinned.path, archive.canonicalize().unwrap());
            assert_eq!(pinned.entry.as_deref(), Some("第二章/封面.png"));
            for from_start in [false, true] {
                viewer.activate_recent_row(&pinned, from_start);
                let task = viewer.pending.take().unwrap();
                assert!(
                    matches!(task.request, Request::List { path, password: None } if path == pinned.path)
                );
                assert!(
                    matches!(task.operation, Operation::List { from_start: start, management: false, .. } if start == from_start)
                );
            }
        }
    }

    #[test]
    fn folder_position_coalesces_confirmed_images_and_ignores_old_frames() {
        use super::super::worker::Entry;

        let folder = tempfile::tempdir().unwrap();
        let path = folder.path();
        let entries = ["first.jpg", "current.jpg", "pending.jpg"].map(|name| Entry {
            name: name.into(),
            size: 0,
            directory: false,
            archive: false,
            modified: None,
        });
        let ctx = egui::Context::default();
        let mut viewer = Viewer::empty();
        viewer.install_source(path.to_owned(), None, entries.to_vec());
        viewer.browser = false;
        viewer.current_key = Some((0, false));
        viewer.selected = Some(1);
        viewer.folder_image_drawn(0, &ctx);
        assert!(viewer.recent_folder_drained());

        viewer.selected = Some(0);
        viewer.folder_image_drawn(0, &ctx);
        viewer.selected = Some(1);
        viewer.current_key = Some((1, false));
        viewer.folder_image_drawn(1, &ctx);
        viewer.selected = Some(2);
        viewer.folder_image_drawn(2, &ctx);

        assert_eq!(viewer.recent_ui.folder_writes.len(), 1);
        let job = &viewer.recent_ui.folder_writes[path];
        assert_eq!(job.filename, "current.jpg");
        assert!(job.record_open);
    }
}

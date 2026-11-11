use super::{worker, Viewer};
use std::{
    path::{Path, PathBuf},
    time::{Duration, Instant, SystemTime},
};

const SAVE_DELAY: Duration = Duration::from_millis(350);

#[derive(Default)]
pub(super) struct State {
    archive: Option<PathBuf>,
    last: Option<Position>,
}

struct Position {
    entry: String,
    viewed_at: SystemTime,
    save_at: Option<Instant>,
    dirty: bool,
}

impl Viewer {
    pub(super) fn change_archive_source(&mut self, source: Option<&Path>) {
        self.save_archive_resume(true);
        self.archive_resume = State {
            archive: source
                .filter(|path| worker::is_archive(path) && !path.is_dir())
                .map(Path::to_owned),
            last: None,
        };
        self.archive_jump.open = false;
    }

    pub(super) fn is_archive_source(&self) -> bool {
        self.archive_resume
            .archive
            .as_ref()
            .is_some_and(|path| self.source.as_ref() == Some(path))
    }

    pub(super) fn initial_entry_selection(&self, explicit: Option<String>) -> Option<usize> {
        let name = explicit.or_else(|| {
            let archive = self.archive_resume.archive.as_ref()?;
            self.extra
                .catalog
                .as_ref()?
                .archive_resume(archive)
                .ok()
                .flatten()
        });
        name.and_then(|name| self.entries.iter().position(|entry| entry.name == name))
            .or_else(|| {
                self.entries
                    .iter()
                    .position(|entry| !entry.directory && !entry.archive)
            })
    }

    pub(super) fn archive_image_drawn(&mut self, index: usize, ctx: &egui::Context) {
        if self.browser || !self.is_archive_source() {
            return;
        }
        let Some(entry) = self.entries.get(index) else {
            return;
        };
        if self
            .archive_resume
            .last
            .as_ref()
            .is_some_and(|last| last.entry == entry.name)
        {
            return;
        }
        self.archive_resume.last = Some(Position {
            entry: entry.name.clone(),
            viewed_at: SystemTime::now(),
            save_at: Some(Instant::now() + SAVE_DELAY),
            dirty: true,
        });
        ctx.request_repaint_after(SAVE_DELAY);
    }

    pub(super) fn save_archive_resume(&mut self, force: bool) {
        let (Some(archive), Some(last)) =
            (&self.archive_resume.archive, &mut self.archive_resume.last)
        else {
            return;
        };
        if !last.dirty || (!force && !last.save_at.is_some_and(|due| due <= Instant::now())) {
            return;
        }
        last.save_at = None;
        if let Some(catalog) = self.extra.catalog.as_mut() {
            match catalog.remember_archive_resume(archive, &last.entry, last.viewed_at) {
                Ok(()) => last.dirty = false,
                Err(error) if !force && !self.closing => {
                    self.status = format!("无法保存压缩包续看位置：{error}")
                }
                Err(_) => {}
            }
        }
    }

    pub(super) fn poll_archive_resume(&mut self, ctx: &egui::Context) {
        self.save_archive_resume(false);
        if let Some(due) = self
            .archive_resume
            .last
            .as_ref()
            .and_then(|last| last.save_at)
        {
            ctx.request_repaint_after(
                due.saturating_duration_since(Instant::now())
                    .max(Duration::from_millis(10)),
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{catalog::Catalog, worker::Entry};

    #[test]
    fn archive_reopen_uses_the_last_drawn_entry_and_respects_explicit_selection() {
        let dir = tempfile::tempdir().unwrap();
        let archive = dir.path().join("图册.7z");
        std::fs::write(&archive, b"test archive identity").unwrap();
        let database = dir.path().join("catalog.sqlite");
        let entries = ["第一页.png", "甲/相同.png", "乙/相同.png"]
            .map(|name| Entry {
                name: name.into(),
                size: 1,
                directory: false,
                archive: false,
                modified: None,
            })
            .to_vec();
        {
            let mut viewer = Viewer::empty();
            viewer.extra.catalog = Some(Catalog::open(&database).unwrap());
            viewer.install_source(
                archive.clone(),
                Some("session-secret".into()),
                entries.clone(),
            );
            viewer.browser = false;
            let ctx = egui::Context::default();
            viewer.archive_image_drawn(1, &ctx);
            viewer.archive_image_drawn(2, &ctx);
            viewer.selected = Some(0); // Queued or cancelled, without a displayed frame.
            viewer.save_archive_resume(true);
        }
        let mut viewer = Viewer::empty();
        viewer.extra.catalog = Some(Catalog::open(&database).unwrap());
        viewer.install_source(archive, None, entries);
        assert_eq!(viewer.initial_entry_selection(None), Some(2));
        assert_eq!(
            viewer.initial_entry_selection(Some("甲/相同.png".into())),
            Some(1)
        );
        assert!(viewer.password.is_none());
        viewer.entries.pop();
        assert_eq!(viewer.initial_entry_selection(None), Some(0));
    }
}

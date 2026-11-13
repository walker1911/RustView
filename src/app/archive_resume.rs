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

    pub(super) fn initial_entry_selection(
        &self,
        explicit: Option<String>,
        from_start: bool,
    ) -> Option<usize> {
        let name = explicit.or_else(|| {
            if from_start {
                return None;
            }
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
        assert_eq!(viewer.initial_entry_selection(None, false), Some(2));
        assert_eq!(
            viewer.initial_entry_selection(Some("甲/相同.png".into()), false),
            Some(1)
        );
        assert!(viewer.password.is_none());
        viewer.entries.pop();
        assert_eq!(viewer.initial_entry_selection(None, false), Some(0));
    }

    #[test]
    fn opening_from_start_keeps_the_saved_position_until_the_first_image_is_drawn() {
        let directory = tempfile::tempdir().unwrap();
        for extension in ["zip", "rar", "7z"] {
            let archive = directory.path().join(format!("album.{extension}"));
            std::fs::write(&archive, b"test archive identity").unwrap();
            let mut catalog = Catalog::open(&directory.path().join("catalog.sqlite")).unwrap();
            catalog
                .remember_archive_resume(&archive, "10.png", SystemTime::now())
                .unwrap();
            let entries = ["1.png", "10.png"]
                .map(|name| Entry {
                    name: name.into(),
                    size: 1,
                    directory: false,
                    archive: false,
                    modified: None,
                })
                .to_vec();
            let mut viewer = Viewer::empty();
            viewer.extra.catalog = Some(catalog);
            viewer.install_source(archive.clone(), None, entries.clone());
            assert_eq!(viewer.initial_entry_selection(None, true), Some(0));

            // A cancelled load or a close before the first frame must leave resume alone.
            viewer.install_source(archive, None, entries);
            assert_eq!(viewer.initial_entry_selection(None, false), Some(1));
            viewer.browser = false;
            viewer.archive_image_drawn(0, &egui::Context::default());
            viewer.save_archive_resume(true);
            assert_eq!(viewer.initial_entry_selection(None, false), Some(0));
        }
    }

    #[test]
    fn opening_from_start_survives_the_password_dialog_for_an_encrypted_archive() {
        use super::super::{Event, Operation, Request, Running};
        use std::sync::{atomic::AtomicBool, mpsc, Arc};

        let directory = tempfile::tempdir().unwrap();
        let archive = directory.path().join("album.7z");
        let image = include_bytes!("../../tests/fixtures/image.png");
        for name in ["1.png", "2.png"] {
            std::fs::write(directory.path().join(name), image).unwrap();
        }
        let mut pack = std::process::Command::new(crate::archive::engine_path().unwrap());
        pack.current_dir(directory.path()).args([
            "a",
            "-mhe=on",
            "-pfixture-password",
            "album.7z",
            "1.png",
            "2.png",
        ]);
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            pack.creation_flags(0x0800_0000);
        }
        let output = pack.output().unwrap();
        assert!(output.status.success(), "{:?}", output);

        let mut catalog = Catalog::open(&directory.path().join("catalog.sqlite")).unwrap();
        catalog
            .remember_archive_resume(&archive, "2.png", SystemTime::now())
            .unwrap();
        let ctx = egui::Context::default();
        let mut viewer = Viewer::empty();
        viewer.extra.catalog = Some(catalog);
        viewer.open_path_with_start(archive.clone(), true);

        // Deliver actual archive listing replies through the viewer's normal retry flow.
        let complete_listing = |viewer: &mut Viewer| {
            let task = viewer.pending.take().unwrap();
            let Request::List { path, password } = &task.request else {
                panic!("expected archive listing")
            };
            let reply = match crate::archive::list(path, password.as_deref()) {
                Ok(entries) => worker::Reply {
                    entries: Some(
                        entries
                            .into_iter()
                            .map(|entry| Entry {
                                name: entry.name,
                                size: entry.size,
                                directory: false,
                                archive: false,
                                modified: entry.modified,
                            })
                            .collect(),
                    ),
                    ..Default::default()
                },
                Err(error) => worker::archive_error(error),
            };
            let (sender, receiver) = mpsc::sync_channel(1);
            sender
                .send(Event {
                    id: task.id,
                    request: task.request,
                    operation: task.operation,
                    result: Ok(worker::Output {
                        reply,
                        rgba: Vec::new(),
                        preview: Vec::new(),
                    }),
                })
                .ok()
                .unwrap();
            viewer.running = Some(Running {
                id: task.id,
                cancel: Arc::new(AtomicBool::new(false)),
                receiver,
                preview: None,
                preload: None,
                promoted: false,
            });
            // The unit test process cannot launch itself as an image worker.
            viewer.closing = true;
            let _ = ctx.run(egui::RawInput::default(), |ctx| viewer.poll(ctx));
            viewer.closing = false;
        };
        complete_listing(&mut viewer);
        for password in ["wrong-password", "fixture-password"] {
            assert!(viewer.password_dialog);
            assert!(matches!(
                &viewer.retry.as_ref().unwrap().1,
                Operation::List {
                    from_start: true,
                    ..
                }
            ));
            viewer.password_input = password.into();
            let input = egui::RawInput {
                events: vec![egui::Event::Key {
                    key: egui::Key::Enter,
                    physical_key: None,
                    pressed: true,
                    repeat: false,
                    modifiers: egui::Modifiers::NONE,
                }],
                ..Default::default()
            };
            let _ = ctx.run(input, |ctx| viewer.dialogs(ctx));
            complete_listing(&mut viewer);
        }
        assert!(!viewer.password_dialog);
        assert_eq!(
            viewer
                .selected
                .map(|index| viewer.entries[index].name.as_str()),
            Some("1.png")
        );
        assert_eq!(viewer.initial_entry_selection(None, false), Some(1));
    }
}

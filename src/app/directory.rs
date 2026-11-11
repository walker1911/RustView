use super::*;

#[derive(Default)]
pub(super) struct Scan {
    waiting: Option<PathBuf>,
    running: Option<ScanTask>,
}

struct ScanTask {
    cancel: Arc<AtomicBool>,
    receiver: Receiver<Result<Vec<Entry>, String>>,
}

impl Scan {
    pub fn cancel(&mut self) {
        self.waiting = None;
        if let Some(task) = &self.running {
            task.cancel.store(true, Ordering::Relaxed);
        }
    }

    pub fn is_idle(&self) -> bool {
        self.waiting.is_none() && self.running.is_none()
    }
}

impl Drop for Scan {
    fn drop(&mut self) {
        self.cancel();
    }
}

impl Viewer {
    pub(super) fn install_source(
        &mut self,
        path: PathBuf,
        password: Option<String>,
        entries: Vec<Entry>,
    ) {
        self.change_archive_source(Some(&path));
        if self.extra.history.get(self.extra.history_pos) != Some(&path) {
            self.extra.history.truncate(self.extra.history_pos + 1);
            self.extra.history.push(path.clone());
            self.extra.history_pos = self.extra.history.len() - 1;
        }
        self.source = Some(path);
        self.password = password;
        self.entries = entries;
        self.initial_entry_position = None;
        self.selected = None;
        self.nav_forward = true;
        self.extra.selection.clear();
        self.extra.thumbnails.clear();
        self.extra.edited = false;
        self.cache = ImageCache::default();
        self.current = None;
        self.current_key = None;
        self.overview = None;
        self.tiles.clear();
        self.tile_bytes = 0;
        self.rebuild_visible();
        self.extra.browser_ui.invalidate();
    }

    pub(super) fn open_local_image(&mut self, path: PathBuf) {
        let Some(name) = path.file_name() else { return };
        let directory = path
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
            .unwrap_or_else(|| Path::new("."))
            .to_owned();
        // The old source can also have a preload in slot zero.
        self.pending = None;
        if let Some(active) = &self.running {
            active.cancel.store(true, Ordering::Relaxed);
        }
        self.install_source(
            directory.clone(),
            None,
            vec![Entry {
                name: name.to_string_lossy().into_owned(),
                size: 0,
                directory: false,
                archive: false,
                modified: None,
            }],
        );
        self.load_index(0, false);
        self.directory_scan.waiting = Some(directory);
    }

    pub(super) fn directory_order(&self) -> impl Iterator<Item = usize> + '_ {
        // Keep the directly opened image's identity at slot zero. The list
        // supplies its natural position without changing any in-flight index.
        let first = self.initial_entry_position.unwrap_or(0);
        (0..self.entries.len()).map(move |position| match position.cmp(&first) {
            std::cmp::Ordering::Less => position + 1,
            std::cmp::Ordering::Equal => 0,
            std::cmp::Ordering::Greater => position,
        })
    }

    pub(super) fn directory_position(&self, index: usize) -> usize {
        let first = self.initial_entry_position.unwrap_or(0);
        match index {
            0 => first,
            index if index <= first => index - 1,
            index => index,
        }
    }

    fn merge_local_directory(&mut self, mut entries: Vec<Entry>) {
        let Some(first) = self.entries.first() else {
            return;
        };
        let mut position = entries
            .iter()
            .position(|entry| !entry.directory && !entry.archive && entry.name == first.name);
        if cfg!(windows) && position.is_none() && self.current.is_some() {
            let name = first.name.to_lowercase();
            position = entries.iter().position(|entry| {
                !entry.directory && !entry.archive && entry.name.to_lowercase() == name
            });
        }
        let first = position
            .map(|index| entries.remove(index))
            .unwrap_or_else(|| first.clone());
        entries.insert(0, first);
        self.initial_entry_position = position;
        self.entries = entries;
        self.rebuild_visible();
        self.extra.browser_ui.invalidate();
        if self.current.is_some() && self.running.is_none() {
            self.schedule_preload();
        }
    }

    pub(super) fn poll_directory_scan(&mut self, ctx: &egui::Context) {
        if let Some(task) = &self.directory_scan.running {
            let result = match task.receiver.try_recv() {
                Ok(result) => Some(result),
                Err(mpsc::TryRecvError::Disconnected) => Some(Err("目录扫描意外结束".into())),
                Err(mpsc::TryRecvError::Empty) => None,
            };
            if let Some(result) = result {
                let cancelled = task.cancel.load(Ordering::Relaxed);
                self.directory_scan.running = None;
                if !cancelled {
                    match result {
                        Ok(entries) => self.merge_local_directory(entries),
                        Err(error) => self.status = format!("无法扫描同目录：{error}"),
                    }
                }
            }
        }
        if self.closing || self.directory_scan.running.is_some() {
            return;
        }
        // This runs before polling the image result. A ready image has already
        // had a frame to draw when the directory scan is started.
        if self.current.is_none() && self.error.is_none() {
            if self.directory_scan.waiting.is_some() {
                ctx.request_repaint_after(std::time::Duration::from_millis(30));
            }
            return;
        }
        let Some(path) = self.directory_scan.waiting.take() else {
            return;
        };
        let (sender, receiver) = mpsc::sync_channel(1);
        let cancel = Arc::new(AtomicBool::new(false));
        let thread_cancel = cancel.clone();
        let context = ctx.clone();
        std::thread::spawn(move || {
            let result = worker::execute(
                Request::List {
                    path,
                    password: None,
                },
                thread_cancel,
            )
            .and_then(|output| match output.reply.error {
                Some(error) => Err(error),
                None => Ok(output.reply.entries.unwrap_or_default()),
            });
            let _ = sender.send(result);
            context.request_repaint();
        });
        self.directory_scan.running = Some(ScanTask { cancel, receiver });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(name: &str) -> Entry {
        Entry {
            name: name.into(),
            size: 123,
            directory: false,
            archive: false,
            modified: None,
        }
    }

    #[test]
    fn direct_image_queues_load_first_and_directory_merge_keeps_the_display() {
        let folder = tempfile::tempdir().unwrap();
        let path = folder.path().join("b2.jpg");
        let mut viewer = Viewer::empty();
        viewer.open_path(path.clone());
        assert!(matches!(&viewer.pending.as_ref().unwrap().request,
            Request::Load { path: selected, entry: None, .. } if *selected == path));
        assert_eq!(
            viewer.directory_scan.waiting.as_deref(),
            Some(folder.path())
        );
        let ctx = egui::Context::default();
        viewer.poll_directory_scan(&ctx);
        assert!(viewer.directory_scan.running.is_none());

        viewer.pending = None;
        let image = Arc::new(Pixels {
            width: 1,
            height: 1,
            rgba: vec![0; 4],
            preview: vec![0; 4],
            preview_width: 1,
            preview_height: 1,
            note: String::new(),
            record: None,
        });
        viewer.set_current(0, false, image.clone());
        viewer.merge_local_directory(vec![entry("a.jpg"), entry("b2.jpg"), entry("b10.jpg")]);
        assert_eq!(viewer.extra.visible, vec![1, 0, 2]);
        assert_eq!(
            (0..3)
                .map(|index| viewer.directory_position(index))
                .collect::<Vec<_>>(),
            vec![1, 0, 2]
        );
        assert_eq!(viewer.current_key, Some((0, false)));
        assert!(Arc::ptr_eq(viewer.current.as_ref().unwrap(), &image));
        assert_eq!(viewer.navigation_target(true, false), Some(2));
        assert_eq!(viewer.navigation_target(false, false), Some(1));
        assert_eq!(viewer.entries[0].size, 123);
    }

    #[test]
    fn direct_open_does_not_promote_a_pending_preload_from_an_old_source() {
        let folder = tempfile::tempdir().unwrap();
        let path = folder.path().join("new.jpg");
        let mut viewer = Viewer::empty();
        viewer.pending = Some(Task {
            id: 0,
            request: Request::Load {
                path: "old.jpg".into(),
                entry: None,
                password: None,
                full_raw: false,
            },
            operation: Operation::Load {
                index: 0,
                full_raw: false,
                preload: true,
            },
        });
        viewer.open_path(path.clone());
        assert!(matches!(&viewer.pending.as_ref().unwrap().request,
            Request::Load { path: selected, entry: None, .. } if *selected == path));
    }

    #[test]
    fn close_drains_directory_scan_and_discards_its_late_list() {
        let mut viewer = Viewer::empty();
        viewer.entries = vec![entry("current.jpg")];
        let (sender, receiver) = mpsc::sync_channel(1);
        let cancel = Arc::new(AtomicBool::new(false));
        viewer.directory_scan.running = Some(ScanTask {
            cancel: cancel.clone(),
            receiver,
        });
        let ctx = egui::Context::default();
        let first = ctx.run(egui::RawInput::default(), |ctx| {
            viewer.request_close(ctx);
            assert!(viewer.handle_close_request(ctx));
        });
        assert!(cancel.load(Ordering::Relaxed));
        assert!(!first.viewport_output[&egui::ViewportId::ROOT]
            .commands
            .iter()
            .any(|c| matches!(c, egui::ViewportCommand::Close)));

        sender.send(Ok(vec![entry("stale.jpg")])).ok().unwrap();
        let last = ctx.run(egui::RawInput::default(), |ctx| {
            assert!(viewer.handle_close_request(ctx))
        });
        assert!(viewer.directory_scan.is_idle());
        assert_eq!(viewer.entries.len(), 1);
        assert_eq!(viewer.entries[0].name, "current.jpg");
        assert!(last.viewport_output[&egui::ViewportId::ROOT]
            .commands
            .iter()
            .any(|c| matches!(c, egui::ViewportCommand::Close)));
    }
}

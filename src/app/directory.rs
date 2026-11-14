use super::*;
use super::directory_watch::{self, Changes, Watcher};
use std::{collections::HashSet, fs, io, time::Duration};

const NAVIGATION_QUIET: Duration = Duration::from_millis(150);

#[derive(Default)]
pub(super) struct Order {
    indices: Vec<usize>,
    positions: Vec<usize>,
}

impl Order {
    fn new(indices: Vec<usize>) -> Self {
        let mut positions = vec![0; indices.len()];
        for (position, &index) in indices.iter().enumerate() {
            positions[index] = position;
        }
        Self { indices, positions }
    }
}

#[derive(Default)]
pub(super) struct Scan {
    waiting: Option<PathBuf>,
    running: Option<ScanTask>,
    navigation: Option<(bool, bool)>,
    neighbors_ready: bool,
    watch_path: Option<PathBuf>,
    watcher: Option<Watcher>,
    retiring: Option<Watcher>,
    changes: Option<Changes>,
    refresh: Option<RefreshTask>,
    ready: Option<Refresh>,
    apply_after: Option<Instant>,
}

struct ScanTask {
    cancel: Arc<AtomicBool>,
    receiver: Receiver<ScanResult>,
}

enum ScanResult {
    Neighbors(Vec<Entry>),
    Full(Result<Vec<Entry>, String>),
}

struct RefreshTask {
    cancel: Arc<AtomicBool>,
    receiver: Receiver<Result<Refresh, String>>,
    full: bool,
}

enum Refresh {
    Paths { entries: HashMap<String, Option<Entry>>, renames: Vec<(String, String)> },
    All(Vec<Entry>),
}

impl Scan {
    pub fn cancel(&mut self) {
        self.cancel_scan();
        self.watch_path = None;
        if let Some(watcher) = self.watcher.take() {
            watcher.cancel();
            self.retiring = Some(watcher);
        }
    }

    pub fn cancel_scan(&mut self) {
        self.waiting = None;
        self.navigation = None;
        self.neighbors_ready = false;
        self.changes = None;
        self.ready = None;
        self.apply_after = None;
        if let Some(task) = &self.running {
            task.cancel.store(true, Ordering::Relaxed);
        }
        if let Some(task) = &self.refresh {
            task.cancel.store(true, Ordering::Relaxed);
        }
    }

    pub fn is_idle(&self) -> bool {
        self.waiting.is_none() && self.running.is_none()
    }

    pub fn drained(&self) -> bool {
        self.is_idle() && self.refresh.is_none() && self.retiring.is_none() && self.watcher.is_none()
    }

    fn arm_watch(&mut self, ctx: &egui::Context) {
        if self.watcher.is_none() && self.retiring.is_none() {
            if let Some(path) = self.watch_path.take() {
                self.watcher = Watcher::start(path, ctx.clone()).ok();
            }
        }
    }

    fn start_refresh(&mut self, path: PathBuf, changes: Changes, ctx: &egui::Context) {
        let (sender, receiver) = mpsc::sync_channel(1);
        let cancel = Arc::new(AtomicBool::new(false));
        let thread_cancel = cancel.clone();
        let ctx = ctx.clone();
        let full = changes.overflow;
        std::thread::spawn(move || {
            let result = if full {
                list_directory(path, thread_cancel).map(Refresh::All)
            } else {
                Ok(read_changed_paths(&path, changes, &thread_cancel))
            };
            let _ = sender.send(result);
            ctx.request_repaint();
        });
        self.refresh = Some(RefreshTask { cancel, receiver, full });
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
        self.directory_scan.cancel();
        // A directory itself can be named `photos.zip`.
        if !worker::is_archive(&path) || path.is_dir() {
            self.directory_scan.watch_path = Some(path.clone());
        }
        if self.extra.history.get(self.extra.history_pos) != Some(&path) {
            self.extra.history.truncate(self.extra.history_pos + 1);
            self.extra.history.push(path.clone());
            self.extra.history_pos = self.extra.history.len() - 1;
        }
        self.source = Some(path);
        self.password = password;
        self.entries = entries;
        self.directory_order = Order::default();
        self.selected = None;
        self.nav_forward = true;
        self.extra.selection.clear();
        self.extra.thumbnails.clear();
        self.extra.edited = false;
        self.cache.clear();
        self.current = None;
        self.current_key = None;
        self.pyramid = None;
        self.embedded_psd_preview = false;
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
        (0..self.entries.len()).map(|position| self.directory_order.indices.get(position).copied().unwrap_or(position))
    }

    pub(super) fn directory_position(&self, index: usize) -> usize {
        self.directory_order.positions.get(index).copied().unwrap_or(index)
    }

    fn merge_local_directory(&mut self, entries: Vec<Entry>) {
        // Decode and filmstrip jobs refer to slots. Keep all slots already
        // exposed to navigation and install the sorted order independently.
        let mut existing = self.entries.iter().enumerate()
            .map(|(index, entry)| (entry.name.clone(), index)).collect::<HashMap<_, _>>();
        let opened = self.entries.first().map(|entry| entry.name.to_lowercase());
        let mut match_opened_case = cfg!(windows) && self.entries.len() == 1
            && !self.entries.first().is_some_and(|first| entries.iter().any(|entry|
            !entry.directory && !entry.archive && entry.name == first.name));
        let mut indices = Vec::with_capacity(entries.len().max(self.entries.len()));
        self.entries.reserve(entries.len().saturating_sub(self.entries.len()));
        for entry in entries {
            let previous = existing.remove(&entry.name).or_else(|| {
                if match_opened_case && !entry.directory && !entry.archive
                    && opened.as_ref().is_some_and(|name| entry.name.to_lowercase() == *name)
                {
                    match_opened_case = false;
                    Some(0)
                } else {
                    None
                }
            });
            let index = if let Some(index) = previous {
                self.entries[index] = entry;
                index
            } else {
                let index = self.entries.len();
                self.entries.push(entry);
                index
            };
            indices.push(index);
        }
        if indices.len() != self.entries.len() {
            let present: HashSet<_> = indices.iter().copied().collect();
            indices.splice(..0, (0..self.entries.len()).filter(|index| !present.contains(index)));
        }
        self.directory_order = Order::new(indices);
        self.rebuild_visible();
        self.extra.browser_ui.invalidate();
    }

    fn partial_directory_navigation(&self) -> bool {
        !self.browser && self.extra.compare == 0 && self.extra.edit_dialog.is_none()
            && self.extra.search.is_empty() && self.extra.filter == 0
            && self.extra.group_descending.is_none()
    }

    pub(super) fn directory_listing(&self) -> bool {
        !self.directory_scan.is_idle()
    }

    pub(super) fn defer_directory_navigation(&mut self, forward: bool, end: bool) -> bool {
        self.directory_scan.navigation = None;
        if !self.directory_listing() { return false; }
        if self.partial_directory_navigation() && self.selected.is_some()
            && (end || !self.directory_scan.neighbors_ready)
        {
            self.directory_scan.navigation = Some((forward, end));
            return true;
        }
        !self.partial_directory_navigation()
    }

    pub(super) fn queue_missing_directory_neighbor(&mut self, forward: bool, end: bool) {
        if !end && self.partial_directory_navigation() && self.directory_listing() {
            self.directory_scan.navigation = Some((forward, false));
        }
    }

    pub(super) fn replay_directory_navigation(&mut self, ctx: &egui::Context) {
        let Some((forward, end)) = self.directory_scan.navigation else { return; };
        if !self.partial_directory_navigation()
            || self.password_dialog || self.properties || self.extra.dialog.is_some()
            || self.recent_ui.open || self.extra.settings_ui.open || self.extra.print_ui.open
            || self.help || self.quiet_chrome.menu_open || self.recognition.menu_open
            || ctx.memory(|memory| memory.any_popup_open())
        {
            self.directory_scan.navigation = None;
            return;
        }
        if self.directory_listing() && (end || !self.directory_scan.neighbors_ready) { return; }
        if self.navigation_target(forward, end).is_some() || !self.directory_listing() {
            self.navigate(forward, end);
        }
    }

    pub(super) fn poll_directory_scan(&mut self, ctx: &egui::Context) {
        if let Some(task) = &self.directory_scan.running {
            let result = match task.receiver.try_recv() {
                Ok(result) => Some(result),
                Err(mpsc::TryRecvError::Disconnected) => Some(ScanResult::Full(Err("目录扫描意外结束".into()))),
                Err(mpsc::TryRecvError::Empty) => None,
            };
            if let Some(result) = result {
                let cancelled = task.cancel.load(Ordering::Relaxed);
                match result {
                    ScanResult::Neighbors(entries) => {
                        if !cancelled && self.partial_directory_navigation() {
                            self.merge_local_directory(entries);
                            self.directory_scan.neighbors_ready = true;
                        }
                        // Give this frame's input a chance to use these entries
                        // before handling the complete list on the next frame.
                        ctx.request_repaint();
                    }
                    ScanResult::Full(result) => {
                        self.directory_scan.running = None;
                        if !cancelled {
                            match result {
                                Ok(entries) => {
                                    self.merge_local_directory(entries);
                                    if self.current.is_some() && self.running.is_none()
                                        && self.directory_scan.navigation.is_none() {
                                        self.schedule_preload();
                                    }
                                }
                                Err(error) => self.status = format!("无法扫描同目录：{error}"),
                            }
                        }
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
        self.directory_scan.arm_watch(ctx);
        let opened = self.entries.first().map(|entry| entry.name.clone());
        let (sender, receiver) = mpsc::sync_channel(2);
        let cancel = Arc::new(AtomicBool::new(false));
        let thread_cancel = cancel.clone();
        let context = ctx.clone();
        std::thread::spawn(move || {
            let result = if let Some(opened) = opened.filter(|_| directory_watch::supported_path(&path)) {
                super::directory_neighbors::scan(&path, &opened, &thread_cancel, |entries| {
                    let _ = sender.send(ScanResult::Neighbors(entries));
                    context.request_repaint();
                })
            } else {
                list_directory(path, thread_cancel)
            };
            let _ = sender.send(ScanResult::Full(result));
            context.request_repaint();
        });
        self.directory_scan.running = Some(ScanTask { cancel, receiver });
    }

    pub(super) fn poll_directory_refresh(&mut self, ctx: &egui::Context) {
        let scan = &mut self.directory_scan;
        if scan.retiring.as_ref().is_some_and(Watcher::is_finished) {
            scan.retiring = None;
        }
        if let Some(task) = &scan.refresh {
            let result = match task.receiver.try_recv() {
                Ok(result) => Some(result),
                Err(mpsc::TryRecvError::Disconnected) => Some(Err("目录刷新意外结束".into())),
                Err(mpsc::TryRecvError::Empty) => None,
            };
            if let Some(result) = result {
                if !task.cancel.load(Ordering::Relaxed) {
                    scan.ready = result.ok();
                }
                scan.refresh = None;
            }
        }
        if self.closing { return; }

        // Direct-open arms the watcher alongside its first directory scan. A
        // source opened as a directory can start even when it contains no images.
        if scan.waiting.is_none() && (self.current.is_some() || self.browser) {
            scan.arm_watch(ctx);
        }
        if let Some(changes) = scan.watcher.as_ref().and_then(Watcher::take_changes) {
            scan.changes.get_or_insert_with(Changes::default).merge(changes);
        }
        if scan.ready.is_none() && scan.changes.is_none() && scan.refresh.is_none() { return; }
        let held = ctx.input(|input| input.focused && [egui::Key::ArrowLeft, egui::Key::ArrowRight,
            egui::Key::ArrowUp, egui::Key::ArrowDown, egui::Key::Home, egui::Key::End,
            egui::Key::PageUp, egui::Key::PageDown].iter().any(|&key| input.key_down(key)));
        let busy = self.pending.is_some() || self.running.is_some() || self.deferred_image.is_some()
            || scan.running.is_some() || scan.waiting.is_some()
            || self.extra.writing.is_some() || self.extra.pending_write.is_some()
            || self.extra.edited || self.extra.edit_dialog.is_some() || self.extra.pending_edit_id.is_some()
            || self.extra.compare > 0
            || self.password_dialog || self.extra.dialog.is_some()
            || self.extra.animation.is_some() && self.selected != self.current_key.map(|key| key.0);
        if held || busy {
            scan.apply_after = Some(Instant::now() + NAVIGATION_QUIET);
            if let Some(task) = scan.refresh.as_ref().filter(|task| task.full) {
                if !task.cancel.swap(true, Ordering::Relaxed) {
                    scan.changes.get_or_insert_with(Changes::default).merge(Changes { overflow: true, ..Changes::default() });
                }
            }
            // Release, foreground completion, or closing a dialog wakes the view.
            return;
        }
        if let Some(remaining) = scan.apply_after.and_then(|due| due.checked_duration_since(Instant::now())) {
            ctx.request_repaint_after(remaining);
            return;
        }
        scan.apply_after = None;
        if let Some(refresh) = scan.ready.take() {
            self.apply_directory_refresh(refresh);
            ctx.request_repaint();
        } else if scan.refresh.is_none() {
            if let (Some(path), Some(changes)) = (self.source.clone(), scan.changes.take()) {
                scan.start_refresh(path, changes, ctx);
            }
        }
    }

    fn apply_directory_refresh(&mut self, refresh: Refresh) {
        let (entries, renames, touched) = match refresh {
            Refresh::Paths { mut entries, renames } => {
                let matching = self.entries.iter().enumerate().filter_map(|(index, entry)| {
                    entries.contains_key(&entry.name).then_some(index)
                }).collect::<Vec<_>>();
                let additions = entries.values().filter(|entry| entry.is_some()).count();
                if matching.len() == additions && matching.iter().all(|&index| {
                    let old = &self.entries[index];
                    entries[&old.name].as_ref().is_some_and(|entry| same_kind(entry, old))
                }) {
                    for &index in &matching {
                        self.entries[index] = entries.remove(&self.entries[index].name).unwrap().unwrap();
                    }
                    for &index in &matching {
                        self.invalidate_directory_pixels(index);
                    }
                    self.reload_changed_current(|index| matching.contains(&index));
                    if !matching.is_empty() { self.extra.browser_ui.invalidate(); }
                    return;
                }
                let touched = entries.keys().cloned().collect::<HashSet<_>>();
                let mut next = Vec::with_capacity((self.entries.len() + entries.len()).min(worker::MAX_ENTRIES));
                for old in &self.entries {
                    match entries.remove(&old.name) {
                        Some(Some(entry)) => next.push(entry),
                        Some(None) => {},
                        None => next.push(old.clone()),
                    }
                }
                next.extend(entries.into_values().flatten());
                if next.len() > worker::MAX_ENTRIES { return; }
                next.sort_by(|a, b| b.directory.cmp(&a.directory).then_with(|| natord::compare(&a.name, &b.name)));
                (next, renames, Some(touched))
            }
            Refresh::All(entries) => (entries, Vec::new(), None),
        };
        self.merge_directory_refresh(entries, renames, touched);
    }

    fn invalidate_directory_pixels(&mut self, index: usize) {
        for key in [(index, false), (index, true)] {
            if let Some(old) = self.cache.entries.remove(&key) { self.cache.bytes -= old.bytes(); }
            self.cache.order.retain(|cached| *cached != key);
        }
        self.invalidate_coverflow_preview(index);
    }

    fn reload_changed_current(&mut self, changed: impl FnOnce(usize) -> bool) {
        if let Some((index, full_raw)) = self.current_key.filter(|key| self.selected == Some(key.0)) {
            if changed(index) { self.reload_directory_image(index, full_raw); }
        }
    }

    fn merge_directory_refresh(&mut self, entries: Vec<Entry>, renames: Vec<(String, String)>, touched: Option<HashSet<String>>) {
        let by_name = entries.iter().enumerate().map(|(i, entry)| (entry.name.as_str(), i)).collect::<HashMap<_, _>>();
        let rename_paths = renames.iter().map(|(from, to)| (from, to)).collect::<HashMap<_, _>>();
        let mut occupied = HashSet::new();
        let mut indices = self.entries.iter().map(|old| {
            let new = by_name.get(old.name.as_str()).copied().filter(|&new| same_kind(old, &entries[new]));
            if let Some(new) = new { occupied.insert(new); }
            new
        }).collect::<Vec<_>>();
        let unchanged = self.entries.iter().enumerate().map(|(old_index, old)| {
            indices[old_index].filter(|&new_index| touched.as_ref().is_some_and(|names| !names.contains(&old.name))
                && entries[new_index].modified == old.modified && entries[new_index].size == old.size)
        }).collect::<Vec<_>>();
        for (old, index) in self.entries.iter().zip(&mut indices) {
            // A replacement at the same path takes precedence over rename hints.
            if index.is_some() { continue; }
            let mut name = &old.name;
            for _ in 0..renames.len() {
                let Some(&next) = rename_paths.get(name) else { break; };
                name = next;
                if let Some(&new) = by_name.get(name.as_str()).filter(|&&new| same_kind(old, &entries[new])) {
                    if occupied.insert(new) { *index = Some(new); }
                    break;
                }
            }
        }
        let remap = |index: usize| indices.get(index).copied().flatten();
        let old_selected = self.selected;
        let old_current = self.current_key;
        self.selected = old_selected.and_then(remap);
        self.current_key = old_current.and_then(|(index, raw)| Some((remap(index)?, raw)));
        self.extra.selection = self.extra.selection.iter().filter_map(|&index| remap(index)).collect();
        let replacement = old_selected.filter(|_| self.selected.is_none()).and_then(|old| {
            let visible = &self.extra.visible;
            let position = visible.iter().position(|&index| index == old).unwrap_or(0);
            visible.iter().skip(position.saturating_add(1)).chain(visible.iter().take(position).rev())
                .filter_map(|&index| remap(index))
                .find(|&index| !entries[index].directory && !entries[index].archive)
        });
        self.entries = entries;
        self.directory_order = Order::default();

        let cache = &mut self.cache;
        cache.bytes = 0;
        cache.entries = std::mem::take(&mut cache.entries).into_iter().filter_map(|((old, raw), image)| {
            let index = unchanged.get(old).copied().flatten()?;
            cache.bytes += image.bytes();
            Some(((index, raw), image))
        }).collect();
        cache.order = cache.order.iter().filter_map(|&(old, raw)| Some((unchanged.get(old).copied().flatten()?, raw))).collect();
        self.extra.thumbnails = std::mem::take(&mut self.extra.thumbnails).into_iter().filter_map(|(old, texture)| {
            Some((unchanged.get(old).copied().flatten()?, texture))
        }).collect();
        self.extra.coverflow.remap_directory(&unchanged);
        self.extra.browser_ui.remap_directory(&indices);
        self.extra.presentation.remap_directory(old_current.map(|key| key.0), self.current_key.map(|key| key.0));
        self.remap_directory_animation(&indices);
        self.rebuild_visible();
        if let Some((old, _)) = old_current {
            self.reload_changed_current(|index| unchanged[old] != Some(index));
        }

        if old_selected.is_some() && self.selected.is_none() {
            let replacement = replacement.filter(|index| self.extra.visible.contains(index))
                .or_else(|| self.extra.visible.iter().copied().find(|&index| !self.entries[index].directory && !self.entries[index].archive));
            if let Some(index) = replacement {
                let management = self.browser;
                self.load_index(index, false);
                self.browser = management;
                if management { self.extra.selection.insert(index); }
            } else {
                self.current = None;
                self.pyramid = None;
                self.overview = None;
                self.tiles.clear();
                self.tile_bytes = 0;
                self.extra.presentation.clear();
                self.recognition.reset();
                self.status = "当前图片已移走".into();
            }
        }
    }
}

fn same_kind(a: &Entry, b: &Entry) -> bool {
    a.directory == b.directory && a.archive == b.archive
}

fn list_directory(path: PathBuf, cancel: Arc<AtomicBool>) -> Result<Vec<Entry>, String> {
    worker::execute(Request::List { path, password: None }, cancel)
        .and_then(|output| match output.reply.error {
            Some(error) => Err(error),
            None => Ok(output.reply.entries.unwrap_or_default()),
        })
}

fn read_changed_paths(path: &Path, changes: Changes, cancel: &AtomicBool) -> Refresh {
    let mut entries = changes.paths.into_iter().take_while(|_| !cancel.load(Ordering::Relaxed)).filter_map(|name| {
        let mut parts = Path::new(&name).components();
        if !matches!(parts.next(), Some(std::path::Component::Normal(_))) || parts.next().is_some() {
            return None;
        }
        let metadata = fs::symlink_metadata(path.join(&name));
        let name = name.to_string_lossy().into_owned();
        match metadata {
            Ok(metadata) => Some((name.clone(), worker::local_entry(name, metadata))),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Some((name, None)),
            // An editor may briefly deny metadata access while writing. Keep
            // the known entry so a transient sharing violation isn't deletion.
            Err(_) => None,
        }
    }).collect();
    if cfg!(windows) {
        check_renamed_casing(path, &changes.renames, &mut entries, cancel);
    }
    let renames = changes.renames.into_iter().map(|(from, to)| {
        (from.to_string_lossy().into_owned(), to.to_string_lossy().into_owned())
    }).collect();
    Refresh::Paths { entries, renames }
}

fn check_renamed_casing(
    path: &Path,
    renames: &[(std::ffi::OsString, std::ffi::OsString)],
    entries: &mut HashMap<String, Option<Entry>>,
    cancel: &AtomicBool,
) {
    let mut absent = HashSet::new();
    for (old, new) in renames {
        let old_name = old.to_string_lossy();
        let new_name = new.to_string_lossy();
        if old != new
            && old_name.chars().flat_map(char::to_uppercase)
                .eq(new_name.chars().flat_map(char::to_uppercase))
            && entries.get(old_name.as_ref()).is_some_and(Option::is_some)
            && entries.get(new_name.as_ref()).is_some_and(Option::is_some)
        {
            absent.extend([old.as_os_str(), new.as_os_str()]);
        }
    }
    if absent.is_empty() { return; }

    // Metadata can resolve both spellings to one file. Check the stored names
    // so a case-sensitive directory can still contain both distinct entries.
    let Ok(children) = fs::read_dir(path) else { return; };
    for (index, child) in children.enumerate() {
        if index == worker::MAX_ENTRIES || cancel.load(Ordering::Relaxed) { return; }
        let Ok(child) = child else { return; };
        absent.remove(child.file_name().as_os_str());
        if absent.is_empty() { return; }
    }
    for name in absent {
        entries.insert(name.to_string_lossy().into_owned(), None);
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

    fn pixels() -> Arc<Pixels> {
        Arc::new(Pixels { width: 1, height: 1, rgba: vec![0; 4], preview: vec![0; 4],
            preview_width: 1, preview_height: 1, note: String::new(), record: None })
    }

    fn changes(paths: &[(&str, Option<&str>)], renames: &[(&str, &str)]) -> Refresh {
        Refresh::Paths {
            entries: paths.iter().map(|&(path, name)| (path.to_owned(), name.map(entry))).collect(),
            renames: renames.iter().map(|&(from, to)| (from.to_owned(), to.to_owned())).collect(),
        }
    }

    #[cfg(windows)]
    #[test]
    fn case_only_rename_keeps_one_entry_and_follows_selection() {
        let directory = tempfile::tempdir().unwrap();
        let original = directory.path().join("photo.jpg");
        let renamed = directory.path().join("Photo.jpg");
        fs::write(&original, b"original pixels").unwrap();
        let mut viewer = Viewer::empty();
        viewer.install_source(directory.path().into(), None, vec![entry("photo.jpg")]);
        let current = pixels();
        viewer.set_current(0, false, current.clone());

        fs::rename(original, renamed).unwrap();
        let changes = Changes {
            paths: HashSet::from(["photo.jpg".into(), "Photo.jpg".into()]),
            renames: vec![("photo.jpg".into(), "Photo.jpg".into())],
            ..Changes::default()
        };
        let refresh = read_changed_paths(directory.path(), changes, &AtomicBool::new(false));
        viewer.apply_directory_refresh(refresh);

        assert_eq!(viewer.entries.len(), 1);
        assert_eq!(viewer.entries[0].name, "Photo.jpg");
        assert_eq!(viewer.selected, Some(0));
        assert_eq!(viewer.current_key, Some((0, false)));
        assert!(Arc::ptr_eq(viewer.current.as_ref().unwrap(), &current));
        assert!(matches!(viewer.pending.as_ref().unwrap().operation, Operation::Reload { index: 0, .. }));
    }

    #[test]
    fn same_stamp_replacement_refreshes_current_and_ignores_temporary_files() {
        let directory = tempfile::tempdir().unwrap();
        let mut viewer = Viewer::empty();
        viewer.install_source(directory.path().into(), None, vec![entry("a.jpg"), entry("view.psd")]);
        let current = pixels();
        let cached = pixels();
        viewer.set_current(1, false, current.clone());
        viewer.cache.insert((0, false), cached.clone(), false);
        viewer.cache.insert((1, false), current.clone(), false);
        viewer.apply_directory_refresh(changes(&[("view.psd", Some("view.psd")), ("temp.tmp", None)],
            &[("view.psd", "temp.tmp")]));
        assert_eq!(viewer.selected, Some(1));
        assert_eq!(viewer.current_key, Some((1, false)));
        assert!(Arc::ptr_eq(viewer.current.as_ref().unwrap(), &current));
        assert!(Arc::ptr_eq(&viewer.cache.entries[&(0, false)], &cached));
        assert_eq!(viewer.cache.bytes, cached.bytes());
        assert!(!viewer.cache.entries.contains_key(&(1, false)));
        assert!(matches!(viewer.pending.as_ref().unwrap().operation, Operation::Reload { index: 1, full_raw: false }));
    }

    #[test]
    fn rename_follows_selected_identity_and_remaps_unmodified_neighbor_cache() {
        let directory = tempfile::tempdir().unwrap();
        let mut viewer = Viewer::empty();
        viewer.install_source(directory.path().into(), None, vec![entry("a.psd"), entry("b.jpg"), entry("c.jpg")]);
        let current = pixels();
        let cached = pixels();
        viewer.set_current(0, false, current.clone());
        viewer.cache.insert((1, false), cached.clone(), false);
        viewer.extra.selection.extend([0, 1]);
        viewer.apply_directory_refresh(changes(&[("a.psd", None), ("z.psd", Some("z.psd"))],
            &[("a.psd", "tmp"), ("tmp", "z.psd")]));
        assert_eq!(viewer.selected, Some(2));
        assert_eq!(viewer.current_key, Some((2, false)));
        assert_eq!(viewer.extra.selection.iter().copied().collect::<Vec<_>>(), vec![0, 2]);
        assert_eq!(viewer.entries[2].name, "z.psd");
        assert!(Arc::ptr_eq(viewer.current.as_ref().unwrap(), &current));
        assert!(Arc::ptr_eq(&viewer.cache.entries[&(0, false)], &cached));
        assert!(matches!(viewer.pending.as_ref().unwrap().operation, Operation::Reload { index: 2, .. }));
    }

    #[test]
    fn deletion_chooses_next_surviving_neighbor_and_clears_last_image() {
        let directory = tempfile::tempdir().unwrap();
        let mut viewer = Viewer::empty();
        viewer.install_source(directory.path().into(), None, vec![entry("a.jpg"), entry("b.jpg"), entry("c.jpg")]);
        viewer.browser = false;
        viewer.set_current(1, false, pixels());
        viewer.apply_directory_refresh(changes(&[("b.jpg", None)], &[]));
        assert_eq!(viewer.selected, Some(1));
        assert_eq!(viewer.entries[1].name, "c.jpg");
        assert!(!viewer.browser);
        viewer.pending = None;
        viewer.set_current(1, false, pixels());
        viewer.apply_directory_refresh(changes(&[("a.jpg", None), ("c.jpg", None)], &[]));
        assert!(viewer.selected.is_none() && viewer.current.is_none() && viewer.entries.is_empty());
    }

    #[test]
    fn completed_refresh_waits_through_held_navigation() {
        let mut viewer = Viewer::empty();
        viewer.entries = vec![entry("a.jpg")];
        viewer.directory_scan.ready = Some(changes(&[("b.jpg", Some("b.jpg"))], &[]));
        let ctx = egui::Context::default();
        let _ = ctx.run(egui::RawInput { focused: true, events: vec![egui::Event::Key {
            key: egui::Key::ArrowRight, physical_key: None, pressed: true, repeat: false,
            modifiers: egui::Modifiers::NONE,
        }], ..Default::default() }, |ctx| viewer.poll_directory_refresh(ctx));
        assert_eq!(viewer.entries.len(), 1);
        assert!(viewer.directory_scan.ready.is_some());
        viewer.directory_scan.apply_after = Some(Instant::now() - NAVIGATION_QUIET);
        let _ = ctx.run(egui::RawInput { focused: false, ..Default::default() }, |ctx| viewer.poll_directory_refresh(ctx));
        assert_eq!(viewer.entries.len(), 2);
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
        let image = pixels();
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
    fn opening_arrow_uses_neighbors_before_full_list_without_remapping_its_load() {
        let folder = tempfile::tempdir().unwrap();
        let mut viewer = Viewer::empty();
        viewer.open_local_image(folder.path().join("b10.jpg"));
        viewer.pending = None;
        let image = pixels();
        viewer.set_current(0, false, image.clone());
        viewer.directory_scan.waiting = None;
        let (sender, receiver) = mpsc::sync_channel(2);
        viewer.directory_scan.running = Some(ScanTask { cancel: Arc::new(AtomicBool::new(false)), receiver });
        let ctx = egui::Context::default();

        viewer.navigate(true, false);
        assert!(viewer.pending.is_none());
        assert!(Arc::ptr_eq(viewer.current.as_ref().unwrap(), &image));
        sender.send(ScanResult::Neighbors(vec![entry("b2.jpg"), entry("b10.jpg"), entry("b11.jpg")])).ok().unwrap();
        viewer.poll_directory_scan(&ctx);
        viewer.replay_directory_navigation(&ctx);
        assert!(viewer.directory_listing());
        assert_eq!(viewer.selected, Some(2));
        let pending = viewer.pending.as_ref().unwrap().id;

        sender.send(ScanResult::Full(Ok(vec![entry("a.jpg"), entry("b2.jpg"), entry("b10.jpg"), entry("b11.jpg"), entry("z.jpg")]))).ok().unwrap();
        viewer.poll_directory_scan(&ctx);
        assert!(!viewer.directory_listing());
        assert_eq!(viewer.selected, Some(2));
        assert_eq!(viewer.pending.as_ref().unwrap().id, pending);
        assert!(matches!(&viewer.pending.as_ref().unwrap().request, Request::Load { path, .. } if path.ends_with("b11.jpg")));
        assert_eq!(viewer.navigation_target(false, false), Some(0));
        let next = viewer.navigation_target(true, false).unwrap();
        assert_eq!(viewer.entries[next].name, "z.jpg");
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

        sender.send(ScanResult::Full(Ok(vec![entry("stale.jpg")]))).ok().unwrap();
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

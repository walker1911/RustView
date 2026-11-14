use std::{collections::HashSet, ffi::OsString, io, path::{Path, PathBuf}};

pub(super) fn supported_path(path: &Path) -> bool {
    #[cfg(windows)]
    { !windows::remote(path) }
    #[cfg(not(windows))]
    { let _ = path; false }
}

const MAX_NAMES: usize = 256;

#[derive(Default, Debug)]
pub(super) struct Changes {
    pub paths: HashSet<OsString>,
    pub renames: Vec<(OsString, OsString)>,
    pub overflow: bool,
}

impl Changes {
    fn overflowed(&mut self) {
        *self = Self {
            overflow: true,
            ..Self::default()
        };
    }

    fn is_empty(&self) -> bool {
        self.paths.is_empty() && !self.overflow
    }

    fn bound(&mut self) {
        if self.paths.len() + self.renames.len() > MAX_NAMES {
            self.overflowed();
        }
    }

    pub(super) fn merge(&mut self, incoming: Self) {
        if incoming.overflow {
            self.overflowed();
        } else if !self.overflow {
            self.paths.extend(incoming.paths);
            self.renames.extend(incoming.renames);
            self.bound();
        }
    }
}

#[cfg(windows)]
use std::{
    os::windows::io::{AsRawHandle, OwnedHandle},
    sync::{atomic::Ordering, Arc},
};
#[cfg(windows)]
use windows::{event, run, State};
#[cfg(windows)]
use windows_sys::Win32::System::Threading::SetEvent;

#[cfg(windows)]
pub(super) struct Watcher {
    stop: Arc<OwnedHandle>,
    state: Arc<State>,
}

#[cfg(windows)]
impl Watcher {
    pub(super) fn start(path: PathBuf, ctx: egui::Context) -> io::Result<Self> {
        let stop = Arc::new(event()?);
        let state = Arc::new(State::default());
        let worker_stop = stop.clone();
        let worker_state = state.clone();
        std::thread::Builder::new()
            .name("rustview-directory-watch".into())
            .spawn(move || {
                run(path, &worker_stop, &worker_state, &ctx);
                worker_state.finished.store(true, Ordering::Release);
                ctx.request_repaint();
            })?;
        Ok(Self { stop, state })
    }

    pub(super) fn take_changes(&self) -> Option<Changes> {
        self.state.changes.lock().unwrap().take()
    }

    pub(super) fn cancel(&self) {
        unsafe { SetEvent(self.stop.as_raw_handle()) };
    }

    #[cfg(test)]
    pub(super) fn is_ready(&self) -> bool {
        self.state.ready.load(Ordering::Acquire)
    }

    pub(super) fn is_finished(&self) -> bool {
        self.state.finished.load(Ordering::Acquire)
    }
}

#[cfg(windows)]
impl Drop for Watcher {
    fn drop(&mut self) {
        self.cancel();
    }
}

#[cfg(not(windows))]
pub(super) struct Watcher;

#[cfg(not(windows))]
impl Watcher {
    pub(super) fn start(_: PathBuf, _: egui::Context) -> io::Result<Self> {
        Err(io::ErrorKind::Unsupported.into())
    }

    pub(super) fn take_changes(&self) -> Option<Changes> {
        None
    }

    pub(super) fn cancel(&self) {}

    pub(super) fn is_finished(&self) -> bool {
        true
    }
}

#[cfg(windows)]
mod windows {
    use super::*;
    use std::{
        cell::{Cell, UnsafeCell},
        mem,
        os::windows::{
            ffi::OsStringExt,
            fs::OpenOptionsExt,
            io::{AsRawHandle, FromRawHandle, OwnedHandle},
        },
        path::{Component, Path, Prefix},
        ptr, slice,
        sync::{
            atomic::{AtomicBool, Ordering},
            Mutex,
        },
        time::{Duration, Instant},
    };
    use windows_sys::Win32::{
        Foundation::{
            GetLastError, ERROR_IO_INCOMPLETE, ERROR_IO_PENDING, ERROR_NOTIFY_ENUM_DIR,
            WAIT_OBJECT_0, WAIT_TIMEOUT,
        },
        Storage::FileSystem::{
            GetDriveTypeW, ReadDirectoryChangesW, FILE_ACTION_ADDED, FILE_ACTION_MODIFIED,
            FILE_ACTION_REMOVED, FILE_ACTION_RENAMED_NEW_NAME, FILE_ACTION_RENAMED_OLD_NAME,
            FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OVERLAPPED, FILE_LIST_DIRECTORY,
            FILE_NOTIFY_CHANGE_DIR_NAME, FILE_NOTIFY_CHANGE_FILE_NAME,
            FILE_NOTIFY_CHANGE_LAST_WRITE, FILE_NOTIFY_CHANGE_SIZE, FILE_SHARE_DELETE,
            FILE_SHARE_READ, FILE_SHARE_WRITE,
        },
        System::{
            Threading::{
                CreateEventW, GetCurrentThread, ResetEvent, SetThreadPriority,
                WaitForMultipleObjects, WaitForSingleObject, INFINITE,
                THREAD_PRIORITY_BELOW_NORMAL,
            },
            IO::{CancelIoEx, GetOverlappedResult, OVERLAPPED},
        },
    };

    const BUFFER_WORDS: usize = 64 * 1024 / mem::size_of::<u32>();
    const QUIET: Duration = Duration::from_millis(150);
    const MAX_DELAY: Duration = Duration::from_millis(500);

    #[derive(Default)]
    pub(super) struct State {
        pub changes: Mutex<Option<Changes>>,
        pub ready: AtomicBool,
        pub finished: AtomicBool,
    }

    pub(super) fn event() -> io::Result<OwnedHandle> {
        let handle = unsafe { CreateEventW(ptr::null(), 1, 0, ptr::null()) };
        if handle.is_null() {
            Err(io::Error::last_os_error())
        } else {
            Ok(unsafe { OwnedHandle::from_raw_handle(handle) })
        }
    }

    // The worker keeps the buffer and OVERLAPPED at stable addresses until I/O is drained.
    struct DirectoryRead {
        directory: OwnedHandle,
        complete: OwnedHandle,
        buffer: UnsafeCell<[u32; BUFFER_WORDS]>,
        overlapped: UnsafeCell<OVERLAPPED>,
        pending: Cell<bool>,
    }

    impl DirectoryRead {
        fn open(path: PathBuf) -> io::Result<Self> {
            let directory = std::fs::OpenOptions::new()
                .access_mode(FILE_LIST_DIRECTORY)
                .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE)
                .custom_flags(FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OVERLAPPED)
                .open(path)?
                .into();
            Ok(Self {
                directory,
                complete: event()?,
                buffer: UnsafeCell::new([0; BUFFER_WORDS]),
                overlapped: UnsafeCell::new(unsafe { mem::zeroed() }),
                pending: Cell::new(false),
            })
        }

        fn queue(&self) -> io::Result<()> {
            let success = unsafe {
                let complete = self.complete.as_raw_handle();
                ResetEvent(complete);
                *self.overlapped.get() = mem::zeroed();
                (*self.overlapped.get()).hEvent = complete;
                ReadDirectoryChangesW(
                    self.directory.as_raw_handle(),
                    self.buffer.get().cast(),
                    (BUFFER_WORDS * mem::size_of::<u32>()) as u32,
                    0,
                    FILE_NOTIFY_CHANGE_FILE_NAME
                        | FILE_NOTIFY_CHANGE_DIR_NAME
                        | FILE_NOTIFY_CHANGE_SIZE
                        | FILE_NOTIFY_CHANGE_LAST_WRITE,
                    ptr::null_mut(),
                    self.overlapped.get(),
                    None,
                )
            };
            if success == 0 && unsafe { GetLastError() } != ERROR_IO_PENDING {
                return Err(io::Error::last_os_error());
            }
            self.pending.set(true);
            Ok(())
        }

        fn finish(&self, wait: bool) -> io::Result<usize> {
            let mut length = 0;
            let success = unsafe {
                GetOverlappedResult(
                    self.directory.as_raw_handle(),
                    self.overlapped.get(),
                    &mut length,
                    i32::from(wait),
                )
            };
            if success == 0 {
                let error = unsafe { GetLastError() };
                if error != ERROR_IO_INCOMPLETE {
                    self.pending.set(false);
                }
                return Err(io::Error::from_raw_os_error(error as i32));
            }
            self.pending.set(false);
            let length = length as usize;
            if length > BUFFER_WORDS * mem::size_of::<u32>() {
                return Err(io::ErrorKind::InvalidData.into());
            }
            Ok(length)
        }

        fn record(&self, length: usize, batch: &mut Changes, rename_from: &mut Option<OsString>) {
            debug_assert!(!self.pending.get());
            let bytes = unsafe { slice::from_raw_parts(self.buffer.get().cast(), length) };
            record(bytes, batch, rename_from);
        }
    }

    impl Drop for DirectoryRead {
        fn drop(&mut self) {
            if self.pending.get() {
                unsafe { CancelIoEx(self.directory.as_raw_handle(), self.overlapped.get()) };
                let _ = self.finish(true);
            }
        }
    }

    pub(super) fn remote(path: &Path) -> bool {
        const DRIVE_REMOTE: u32 = 4;
        match path.components().next() {
            Some(Component::Prefix(prefix)) => match prefix.kind() {
                Prefix::UNC(..) | Prefix::VerbatimUNC(..) => true,
                Prefix::Disk(letter) | Prefix::VerbatimDisk(letter) => {
                    let root = [u16::from(letter), u16::from(b':'), u16::from(b'\\'), 0];
                    unsafe { GetDriveTypeW(root.as_ptr()) == DRIVE_REMOTE }
                }
                _ => false,
            },
            _ => unsafe { GetDriveTypeW(ptr::null()) == DRIVE_REMOTE },
        }
    }

    pub(super) fn run(path: PathBuf, stop: &OwnedHandle, state: &State, ctx: &egui::Context) {
        unsafe { SetThreadPriority(GetCurrentThread(), THREAD_PRIORITY_BELOW_NORMAL) };
        if remote(&path) {
            return;
        }
        let Ok(read) = DirectoryRead::open(path) else {
            return;
        };
        let handles = [stop.as_raw_handle(), read.complete.as_raw_handle()];
        let mut batch = Changes::default();
        let mut rename_from = None;
        let mut first = None;
        let mut deadline = None;
        while unsafe { WaitForSingleObject(handles[0], 0) } == WAIT_TIMEOUT {
            if deadline.is_some_and(|due| Instant::now() >= due) {
                publish(state, ctx, mem::take(&mut batch));
                rename_from = None;
                first = None;
                deadline = None;
            }
            let result = match read.queue() {
                Ok(()) => {
                    if !state.ready.swap(true, Ordering::Release) {
                        ctx.request_repaint();
                    }
                    loop {
                        let now = Instant::now();
                        if deadline.is_some_and(|due| now >= due) {
                            publish(state, ctx, mem::take(&mut batch));
                            rename_from = None;
                            first = None;
                            deadline = None;
                        }
                        let timeout = deadline.map_or(INFINITE, |due: Instant| {
                            due.saturating_duration_since(now).as_millis().max(1) as u32
                        });
                        match unsafe { WaitForMultipleObjects(2, handles.as_ptr(), 0, timeout) } {
                            WAIT_TIMEOUT => continue,
                            event if event == WAIT_OBJECT_0 + 1 => break read.finish(false),
                            _ => return,
                        }
                    }
                }
                Err(err) => {
                    // An overflow can also be reported while a read is being queued.
                    // Bound retries if the filesystem remains unable to queue another read.
                    if err.raw_os_error() == Some(ERROR_NOTIFY_ENUM_DIR as i32)
                        && unsafe { WaitForSingleObject(handles[0], QUIET.as_millis() as u32) }
                            != WAIT_TIMEOUT
                    {
                        return;
                    }
                    Err(err)
                }
            };
            match result {
                Ok(length) => read.record(length, &mut batch, &mut rename_from),
                Err(err) if err.raw_os_error() == Some(ERROR_NOTIFY_ENUM_DIR as i32) => {
                    batch.overflowed();
                    rename_from = None;
                }
                Err(_) => {
                    if state.ready.load(Ordering::Relaxed) {
                        batch.overflowed();
                        publish(state, ctx, batch);
                    }
                    return;
                }
            }
            if !batch.is_empty() {
                let now = Instant::now();
                let first = *first.get_or_insert(now);
                deadline = Some((now + QUIET).min(first + MAX_DELAY));
            }
        }
    }

    fn publish(state: &State, ctx: &egui::Context, batch: Changes) {
        let mut changes = state.changes.lock().unwrap();
        if let Some(pending) = changes.as_mut() {
            pending.merge(batch);
        } else {
            *changes = Some(batch);
            drop(changes);
            ctx.request_repaint();
        }
    }

    fn record(mut bytes: &[u8], batch: &mut Changes, rename_from: &mut Option<OsString>) {
        if batch.overflow {
            return;
        }
        loop {
            let Some((name, action, tail)) = entry(bytes) else {
                batch.overflowed();
                *rename_from = None;
                return;
            };
            batch.paths.insert(name.clone());
            match action {
                FILE_ACTION_RENAMED_OLD_NAME => *rename_from = Some(name),
                FILE_ACTION_RENAMED_NEW_NAME => {
                    if let Some(old) = rename_from.take() {
                        batch.renames.push((old, name));
                    }
                }
                _ => *rename_from = None,
            }
            batch.bound();
            if batch.overflow || tail.is_empty() {
                return;
            }
            bytes = tail;
        }
    }

    fn entry(bytes: &[u8]) -> Option<(OsString, u32, &[u8])> {
        let header = bytes.get(..12)?;
        let next = u32::from_le_bytes(header[..4].try_into().ok()?) as usize;
        let action = u32::from_le_bytes(header[4..8].try_into().ok()?);
        let length = u32::from_le_bytes(header[8..].try_into().ok()?) as usize;
        if !matches!(
            action,
            FILE_ACTION_ADDED
                | FILE_ACTION_REMOVED
                | FILE_ACTION_MODIFIED
                | FILE_ACTION_RENAMED_OLD_NAME
                | FILE_ACTION_RENAMED_NEW_NAME
        ) || length == 0
            || length > 510
            || length % 2 != 0
        {
            return None;
        }
        let end = 12 + length;
        let name: Vec<u16> = bytes
            .get(12..end)?
            .chunks_exact(2)
            .map(|pair| u16::from_le_bytes([pair[0], pair[1]]))
            .collect();
        let tail = if next == 0 {
            &[][..]
        } else {
            if next < end || next % 4 != 0 {
                return None;
            }
            bytes.get(next..).filter(|tail| tail.len() >= 12)?
        };
        if name.iter().any(|&c| matches!(c, 0 | 47 | 58 | 92)) || name == [46] || name == [46, 46] {
            return None;
        }
        Some((OsString::from_wide(&name), action, tail))
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        fn notification(action: u32, name: &str, more: bool) -> Vec<u8> {
            let name: Vec<_> = name.encode_utf16().flat_map(u16::to_le_bytes).collect();
            let length = (12 + name.len()).next_multiple_of(4);
            let mut bytes = Vec::new();
            bytes.extend((if more { length as u32 } else { 0 }).to_le_bytes());
            bytes.extend(action.to_le_bytes());
            bytes.extend((name.len() as u32).to_le_bytes());
            bytes.extend(name);
            bytes.resize(length, 0);
            bytes
        }

        #[test]
        fn utf16_rename_records_and_duplicates_are_coalesced() {
            let mut bytes = notification(FILE_ACTION_RENAMED_OLD_NAME, "原图📷.psd", true);
            bytes.extend(notification(
                FILE_ACTION_RENAMED_NEW_NAME,
                "修改后.psd",
                true,
            ));
            bytes.extend(notification(FILE_ACTION_MODIFIED, "修改后.psd", false));
            let mut changes = Changes::default();
            record(&bytes, &mut changes, &mut None);
            assert_eq!(changes.paths.len(), 2);
            assert_eq!(
                changes.renames,
                [("原图📷.psd".into(), "修改后.psd".into())]
            );
            assert!(!changes.overflow);
        }

        #[test]
        fn dropped_or_oversized_changes_collapse_to_rescan() {
            let mut changes = Changes::default();
            for index in 0..=MAX_NAMES {
                changes.merge(Changes {
                    paths: HashSet::from([format!("photo-{index}.jpg").into()]),
                    ..Changes::default()
                });
            }
            assert!(changes.overflow && changes.paths.is_empty());

            let mut changes = Changes::default();
            record(&[], &mut changes, &mut None);
            assert!(changes.overflow);
            assert!(entry(&notification(FILE_ACTION_ADDED, "..\\elsewhere.png", false)).is_none());
            let mut truncated = notification(FILE_ACTION_ADDED, "photo.jpg", true);
            truncated.truncate(14);
            assert!(entry(&truncated).is_none());
        }

        fn wait_for<T>(mut condition: impl FnMut() -> Option<T>) -> T {
            let until = Instant::now() + Duration::from_secs(5);
            loop {
                if let Some(result) = condition() {
                    return result;
                }
                assert!(Instant::now() < until, "directory notification timed out");
                std::thread::sleep(Duration::from_millis(10));
            }
        }

        #[test]
        fn local_file_replace_notifies_and_cancel_drains_pending_read() {
            let folder = tempfile::tempdir().unwrap();
            let target = folder.path().join("测试.psd");
            let staged = folder.path().join("staged.tmp");
            std::fs::write(&target, b"old").unwrap();
            let watcher = Watcher::start(folder.path().into(), egui::Context::default()).unwrap();
            wait_for(|| watcher.is_ready().then_some(()));
            std::fs::write(&staged, b"new").unwrap();
            std::fs::rename(&staged, &target).unwrap();
            let mut changes = Changes::default();
            wait_for(|| {
                if let Some(incoming) = watcher.take_changes() {
                    changes.merge(incoming);
                }
                changes
                    .paths
                    .contains(&OsString::from("测试.psd"))
                    .then_some(())
            });
            assert!(!changes.overflow);
            assert_eq!(std::fs::read(target).unwrap(), b"new");
            watcher.cancel();
            wait_for(|| watcher.is_finished().then_some(()));
        }
    }
}

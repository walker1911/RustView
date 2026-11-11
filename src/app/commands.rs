use super::*;
use crate::{
    catalog::{Catalog, Change, Filter, Record},
    fileops::{self, Source},
};
use std::collections::{BTreeSet, HashSet};

type WriteJob = Box<dyn FnOnce() -> Result<String, String> + Send>;
type KeyMoves = Vec<(String, String)>;
#[derive(Default)]
struct KeyMoveBatch{moves:KeyMoves,containers:Vec<crate::recipes::ContainerUpdate>}
struct KeyMoveCompletion{batch:KeyMoveBatch,errors:Vec<String>}
const CATALOG_PAGE: usize = 100;
const MAX_FILTER_ITEMS: usize = 100_000;

struct CatalogFilterRequest {
    candidates: HashMap<String, usize>,
    mode: u8,
    selection: bool,
}
struct CatalogFilterTask {
    receiver: Receiver<Result<Vec<usize>, String>>,
    cancel: Arc<AtomicBool>,
    selection: bool,
}

pub(super) struct Extras {
    pub browser_ui:super::browser_ui::State,
    pub coverflow:super::coverflow::State,
    pub presentation:super::presentation::State,
    pub workspace:super::workspace::State,
    pub metadata_ui:super::metadata_ui::State,
    pub print_ui:super::print_ui::State,
    pub settings_ui:super::settings_ui::State,
    pub catalog: Option<Catalog>,
    pub selection: BTreeSet<usize>,
    pub visible: Vec<usize>,
    pub search: String,
    pub filter: u8,
    pub descending: bool,
    pub group_descending: Option<bool>,
    pub exposure_warning: bool,
    pub exposure_texture: Option<TextureHandle>,
    pub layout: u16,
    pub toolbar: bool,
    pub statusbar: bool,
    pub organizer: bool,
    pub search_panel: bool,
    pub basket: bool,
    catalog_window: bool,
    catalog_page: usize,
    catalog_rows: Vec<(String, Record)>,
    catalog_more: bool,
    catalog_dirty: bool,
    catalog_filter: u8,
    catalog_filter_task: Option<CatalogFilterTask>,
    pending_catalog_filter: Option<CatalogFilterRequest>,
    pub navigator: bool,
    pub histogram: bool,
    pub magnifier: bool,
    pub magnification: f32,
    pub property_tab: u8,
    pub pan_lock: bool,
    pub zoom_lock: bool,
    pub color_managed: bool,
    pub header_visible: bool,
    pub header: String,
    pub footer: String,
    pub tool: u16,
    pub selection_rect: Option<Rect>,
    pub selection_origin: Option<Pos2>,
    pub viewport: Rect,
    pub history: Vec<PathBuf>,
    pub history_pos: usize,
    pub record: Record,
    pub dialog: Option<u16>,
    pub dialog_focus_seen: Option<u16>,
    pub text: String,
    pub value: f32,
    pub width: u32,
    pub height: u32,
    pub edited: bool,
    pub edits: Vec<crate::editing::Edit>,
    pub redo_edits: Vec<crate::editing::Edit>,
    pub edit_dialog: Option<u16>,
    pub pending_edit_id: Option<u16>,
    pub pending_write: Option<WriteJob>,
    pub writing: Option<Receiver<Result<String, String>>>,
    pub write_source: Option<PathBuf>,
    pub write_refresh: bool,
    key_moves: Option<Receiver<KeyMoveCompletion>>,
    pub cut_paths: Vec<PathBuf>,
    pub finish_cut: Option<(crate::clipboard::FileClipboard, Vec<PathBuf>)>,
    pub clipboard_image_dir: Option<tempfile::TempDir>,
    clipboard_ready: Option<Receiver<Vec<PathBuf>>>,
    rename_template:fileops::RenameTemplate,
    rename_sources:Vec<Source>,
    rename_preview:Vec<fileops::RenameItem>,
    rename_problem:Option<String>,
    rename_build:Option<Receiver<(u64,Result<Vec<fileops::RenameItem>,String>)>>,
    rename_generation:u64,
    pub thumbnails: HashMap<usize, TextureHandle>,
    pub compare: usize,
    pub comparison: super::compare::Comparison,
    pub page_index: u32,
    pub page_total: u32,
    pub page_view: bool,
    pub background_tile: Option<TextureHandle>,
    pub animation: Option<crate::animation_player::Session>,
    pub retiring: Option<crate::animation_player::Session>,
    pub animation_pending: Option<crate::animation_player::Packet>,
    pub animation_operation: Option<(Request, Operation)>,
    pub animation_deadline: Instant,
    pub animation_paused: bool,
    pub animation_ended: bool,
    pub animation_end_pending: bool,
    pub animated: bool,
    pub frame_duration: std::time::Duration,
    pub animation_texture: Option<TextureHandle>,
    pub max_texture: u32,
    pub hwnd: usize,
}

impl Extras {
    pub fn new() -> Self {
        let database = std::env::current_exe()
            .ok()
            .and_then(|p| p.parent().map(|p| p.join("RustView-data/catalog.sqlite")));
        Self {
            browser_ui:super::browser_ui::State::default(),
            coverflow:super::coverflow::State::default(),
            presentation:super::presentation::State::default(),
            workspace:super::workspace::State::default(),
            metadata_ui:super::metadata_ui::State::default(),
            print_ui:super::print_ui::State::default(),
            settings_ui:super::settings_ui::State::default(),
            catalog: database.and_then(|p| Catalog::open(&p).ok()),
            selection: BTreeSet::new(),
            visible: Vec::new(),
            search: String::new(),
            filter: 0,
            descending: false,
            layout: 2011,
            toolbar: true,
            statusbar: true,
            group_descending: None,
            exposure_warning: false,
            exposure_texture: None,
            organizer: false,
            search_panel: false,
            basket: false,
            navigator: false,
            histogram: false,
            magnifier: false,
            magnification: 2.5,
            property_tab: 0,
            catalog_window: false,
            catalog_page: 0,
            catalog_rows: Vec::new(),
            catalog_more: false,
            catalog_dirty: true,
            catalog_filter: 0,
            catalog_filter_task: None,
            pending_catalog_filter: None,
            pan_lock: false,
            zoom_lock: false,
            color_managed: true,
            header_visible: false,
            header: String::new(),
            footer: String::new(),
            tool: 2371,
            selection_rect: None,
            selection_origin: None,
            viewport: Rect::NOTHING,
            history: Vec::new(),
            history_pos: 0,
            record: Record::default(),
            dialog: None,
            dialog_focus_seen: None,
            text: String::new(),
            value: 100.0,
            width: 1920,
            height: 1080,
            edited: false,
            edits: Vec::new(),
            redo_edits: Vec::new(),
            edit_dialog: None,
            pending_edit_id: None,
            pending_write: None,
            writing: None,
            write_source: None,
            write_refresh: true,
            cut_paths: Vec::new(),
            finish_cut: None,
            clipboard_image_dir: None,
            clipboard_ready:None,
            rename_template:fileops::RenameTemplate::default(),rename_sources:Vec::new(),rename_preview:Vec::new(),rename_problem:None,rename_build:None,rename_generation:0,
            key_moves: None,
            thumbnails: HashMap::new(),
            compare: 0,
            comparison: super::compare::Comparison::default(),
            page_index: 0,
            page_total: 1,
            page_view: false,
            background_tile: None,
            animation: None,
            retiring: None,
            animation_pending: None,
            animation_operation: None,
            animation_deadline: Instant::now(),
            animation_paused: false,
            animation_ended: false,
            animation_end_pending: false,
            animated: false,
            frame_duration: std::time::Duration::from_millis(100),
            animation_texture: None,
            max_texture: 4096,
            hwnd: 0,
        }
    }
}

impl Viewer {
    pub(super) fn item_key(&self, index: usize) -> Option<String> {
        let source = self.source.as_ref()?;
        let entry = self.entries.get(index)?;
        let source = source.canonicalize().unwrap_or_else(|_| source.clone());
        Some(if source.is_dir() {
            serde_json::to_string(&(
                source.join(&entry.name).to_string_lossy(),
                Option::<&str>::None,
            ))
            .ok()?
        } else {
            serde_json::to_string(&(source.to_string_lossy(), Some(&entry.name))).ok()?
        })
    }
    pub(super) fn selected_indices(&self) -> Vec<usize> {
        if self.extra.compare>0{return self.extra.comparison.active_index().into_iter().collect();}
        if self.browser {
            self.extra.visible.iter().copied().filter(|i|self.extra.selection.contains(i)).collect()
        } else {
            self.selected.into_iter().collect()
        }
    }
    pub(super) fn selected_sources(&mut self) -> Vec<Source> {
        let Some(source) = self.source.clone() else {
            return Vec::new();
        };
        let indices=if self.browser&&self.extra.compare==0{self.extra.visible.iter().copied().filter(|i|self.extra.selection.contains(i)).collect()}else{self.selected_indices()};
        let bytes=indices.iter().filter_map(|i|self.entries.get(*i)).map(|entry|source.as_os_str().len().saturating_add(entry.name.len()).saturating_add(self.password.as_ref().map(String::len).unwrap_or(0))).fold(0usize,usize::saturating_add);
        if bytes>32*1024*1024{self.error=Some("所选路径总量超过32MiB，请分批处理".into());return Vec::new();}
        let local=source.is_dir();
        indices
            .into_iter()
            .filter_map(|i| self.entries.get(i))
            .map(|e| {
                if local {
                    Source::Local(source.join(&e.name))
                } else {
                    Source::Archive {
                        path: source.clone(),
                        entry: e.name.clone(),
                        password: self.password.clone(),
                    }
                }
            })
            .collect()
    }
    pub(super) fn read_record(&mut self) {
        self.extra.record = self
            .selected
            .and_then(|i| self.item_key(i))
            .and_then(|k| self.extra.catalog.as_ref()?.get(&k).ok())
            .unwrap_or_default();
    }
    pub(super) fn change_record(&mut self, change: Change) {
        let keys: Vec<_> = self
            .selected_indices()
            .into_iter()
            .filter_map(|i| self.item_key(i))
            .collect();
        let result = self
            .extra
            .catalog
            .as_mut()
            .ok_or_else(|| "无法打开 RustView 元数据库（请检查程序目录写权限）".to_string())
            .and_then(|c| c.update_many(&keys, change));
        match result {
            Ok(()) => {
                self.extra.catalog_dirty = true;
                self.extra.browser_ui.invalidate();
                self.read_record();
                self.rebuild_visible();
                self.status = format!("已更新 {} 个项目的图片信息", keys.len());
            }
            Err(e) => self.error = Some(e),
        }
    }
    pub(super) fn rebuild_visible(&mut self) {
        self.extra.coverflow.invalidate_sequence();
        if let Some(task) = &self.extra.catalog_filter_task {
            task.cancel.store(true, Ordering::Relaxed);
        }
        self.extra.pending_catalog_filter = None;
        let search = self.extra.search.to_lowercase();
        self.extra.visible = self.directory_order()
            .filter_map(|i| {
                let e = &self.entries[i];
                if !search.is_empty() && !e.name.to_lowercase().contains(&search) {
                    return None;
                }
                Some(i)
            })
            .collect();
        if self.extra.filter != 0 {
            let candidates = self.catalog_candidates(&self.extra.visible);
            self.extra.visible.clear();
            self.extra.pending_catalog_filter = Some(CatalogFilterRequest {
                candidates,
                mode: self.extra.filter,
                selection: false,
            });
        }
        if self.extra.descending {
            self.extra.visible.reverse();
        }
        self.group_visible();
    }
    fn group_visible(&mut self) {
        if let Some(descending) = self.extra.group_descending {
            sort_groups(&mut self.extra.visible, &self.entries, descending);
        }
    }
    fn catalog_candidates(&self, indices: &[usize]) -> HashMap<String, usize> {
        let Some(source) = &self.source else {
            return HashMap::new();
        };
        let source = source.canonicalize().unwrap_or_else(|_| source.clone());
        let directory = source.is_dir();
        indices
            .iter()
            .take(MAX_FILTER_ITEMS)
            .filter_map(|&index| {
                let entry = self.entries.get(index)?;
                let key = if directory {
                    serde_json::to_string(&(
                        source.join(&entry.name).to_string_lossy(),
                        Option::<&str>::None,
                    ))
                } else {
                    serde_json::to_string(&(source.to_string_lossy(), Some(&entry.name)))
                }
                .ok()?;
                Some((key, index))
            })
            .collect()
    }
    fn poll_catalog_filter(&mut self, ctx: &egui::Context) {
        let done = self.extra.catalog_filter_task.as_ref().and_then(|task| {
            match task.receiver.try_recv() {
                Ok(result) => Some(result),
                Err(mpsc::TryRecvError::Disconnected) => Some(Err("目录筛选线程意外结束".into())),
                _ => None,
            }
        });
        if let Some(result) = done {
            let task = self.extra.catalog_filter_task.take().unwrap();
            if !task.cancel.load(Ordering::Relaxed) {
                self.extra.coverflow.invalidate_sequence();
                match result {
                    Ok(mut indices) => {
                        if task.selection {
                            self.extra.selection = indices.into_iter().collect();
                        } else {
                            indices.sort_unstable_by_key(|&index| self.directory_position(index));
                            if self.extra.descending {
                                indices.reverse();
                            }
                            self.extra.visible = indices;
                            self.group_visible();
                        }
                    }
                    Err(error) => self.error = Some(error),
                }
            }
        }
        if self.extra.catalog_filter_task.is_none() {
            if let Some(request) = self.extra.pending_catalog_filter.take() {
                let Some(database) = catalog_path() else {
                    self.error = Some("无法确定目录库位置".into());
                    return;
                };
                let cancel = Arc::new(AtomicBool::new(false));
                let worker_cancel = cancel.clone();
                let selection = request.selection;
                let (tx, rx) = mpsc::sync_channel(1);
                let ctx = ctx.clone();
                std::thread::spawn(move || {
                    let result = filter_catalog(&database, request, &worker_cancel);
                    let _ = tx.send(result);
                    ctx.request_repaint();
                });
                self.extra.catalog_filter_task = Some(CatalogFilterTask {
                    receiver: rx,
                    cancel,
                    selection,
                });
            }
        }
        if self.extra.catalog_filter_task.is_some() {
            ctx.request_repaint_after(std::time::Duration::from_millis(50));
        }
    }
    fn write_job_with_moves(
        &mut self,
        make_job: impl FnOnce(mpsc::SyncSender<KeyMoveBatch>) -> WriteJob,
    ) {
        if self.extra.writing.is_some() || self.extra.pending_write.is_some() {
            self.error = Some("请等待当前文件操作完成".into());
            return;
        }
        let (file_tx,file_rx)=mpsc::sync_channel(1);
        let job=make_job(file_tx);let database=self.extra.catalog.as_ref().and_then(|c|c.path()).map(Path::to_owned);
        let (tx, rx) = mpsc::sync_channel(1);
        self.write_job(Box::new(move||{
            let result=job();
            if let Ok(batch)=file_rx.try_recv(){
                let mut errors=Vec::new();
                if !batch.moves.is_empty(){match database{Some(path)=>{if let Err(error)=Catalog::open(&path).and_then(|mut catalog|catalog.move_keys(&batch.moves)){errors.push(format!("文件已移动，目录库迁移未完成：{error}"));}},None=>errors.push("文件已移动，但目录库不可写，原图片信息仍保留在旧路径".into())}}
                if !batch.moves.is_empty()||!batch.containers.is_empty(){if let Err(error)=crate::recipes::data_root().and_then(|root|crate::recipes::relocate_keys_with_updates(&root,&batch.moves,&batch.containers)){errors.push(format!("文件已改变，编辑记录迁移未完成：{error}"));}}
                let _=tx.send(KeyMoveCompletion{batch,errors});
            }
            result
        }));
        self.extra.key_moves = Some(rx);
    }
    pub(super) fn write_job(&mut self, job: WriteJob) {
        self.write_job_options(job,true);
    }
    pub(super) fn write_job_no_refresh(&mut self,job:WriteJob){self.write_job_options(job,false);}
    fn write_job_options(&mut self, job:WriteJob,refresh:bool){
        if self.extra.writing.is_some() || self.extra.pending_write.is_some() {
            self.error = Some("请等待当前文件操作完成".into());
            return;
        }
        self.stop_animation();
        self.serial += 1;
        self.pending = None;
        if let Some(r) = &self.running {
            r.cancel.store(true, Ordering::Relaxed);
        }
        self.extra.pending_write = Some(job);
        self.extra.write_refresh=refresh;
        self.status = "正在处理文件，请稍候…".into();
        self.extra.write_source = self.source.clone();
    }
    pub(super) fn poll_writes(&mut self, ctx: &egui::Context) {
        self.poll_rename_preview(ctx);
        if let Some(result) = self.extra.writing.as_ref().and_then(poll_write_result) {
            self.extra.writing = None;
            let mut moved_any = false;
            let mut migration_error = None;
            if let Some(completion) = self
                .extra
                .key_moves
                .take()
                .and_then(|rx| rx.try_recv().ok())
            {
                let batch=completion.batch;
                let moves=batch.moves;
                moved_any = !moves.is_empty();
                let errors = completion.errors;
                self.relocate_management_focus(&moves);
                self.extra.catalog_dirty = true;
                self.extra.browser_ui.invalidate();
                if !errors.is_empty() {
                    migration_error =
                        Some(errors.into_iter().take(5).collect::<Vec<_>>().join("\n"));
                }
            }
            if result.is_ok() {
                if let Some((original, paths)) = self.extra.finish_cut.take() {
                    if let Err(e) = crate::clipboard::finish_cut(&original, &paths) {
                        self.error = Some(e);
                    }
                }
                if let Some(paths)=self.extra.clipboard_ready.as_ref().and_then(|r|r.try_recv().ok()) {
                    if let Err(e) = crate::clipboard::copy_files(&paths) {
                        self.error = Some(e);
                    }
                }
            }
            self.extra.finish_cut = None;
            self.extra.clipboard_ready=None;
            match result {
                Ok(message) => {
                    self.status = message;
                    if self.extra.write_refresh&&self.pending.is_none() && self.source == self.extra.write_source {
                        self.perform(Action::Refresh, ctx);
                    }
                }
                Err(e) => {
                    if moved_any && self.pending.is_none() && self.source == self.extra.write_source
                    {
                        self.perform(Action::Refresh, ctx);
                    }
                    self.error = Some(e)
                }
            }
            if let Some(error) = migration_error {
                self.error = Some(match self.error.take() {
                    Some(other) => format!("{other}\n{error}"),
                    None => error,
                });
            }
            self.extra.write_source = None;
        }
        if self.running.is_none() && self.extra.retiring.is_none() && self.extra.writing.is_none() && self.extra.rename_build.is_none() {
            if let Some(job) = self.extra.pending_write.take() {
                let (tx, rx) = mpsc::sync_channel(1);
                let ctx = ctx.clone();
                std::thread::spawn(move || {
                    let result = execute_write(job);
                    let _ = tx.send(result);
                    ctx.request_repaint();
                });
                self.extra.writing = Some(rx);
            }
        }
        if self.extra.writing.is_some() || self.extra.pending_write.is_some() {
            if ctx.input(|i| i.viewport().close_requested()) {
                ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
                self.status = "正在完成文件操作，完成后可关闭窗口".into();
            }
            ctx.request_repaint_after(std::time::Duration::from_millis(50));
        }
    }
    fn relocate_management_focus(&mut self,moves:&KeyMoves){
        let Some(source)=self.source.clone()else{return;};
        let local=source.is_dir();let canonical=source.canonicalize().unwrap_or_else(|_|source.clone());
        let selected_key=self.selected.and_then(|i|self.item_key(i));
        let decoded=moves.iter().filter_map(|(old,new)|Some((old,serde_json::from_str::<(String,Option<String>)>(old).ok()?,serde_json::from_str::<(String,Option<String>)>(new).ok()?))).collect::<Vec<_>>();
        // A container renamed while open changes every entry's base path.
        for(_, (old,entry),(new,_))in &decoded{if entry.is_none()&&path_identity(Path::new(old))==path_identity(&canonical){self.source=Some(PathBuf::from(new));if self.extra.write_source.as_ref()==Some(&source){self.extra.write_source=self.source.clone();}}}
        let Some(index)=self.selected else{return;};let Some(key)=selected_key else{return;};
        let Ok((selected_path,selected_entry))=serde_json::from_str::<(String,Option<String>)>(&key)else{return;};
        let Some((_,_,(new,entry)))=decoded.iter().find(|(_, (old,entry),_)|path_identity(Path::new(old))==path_identity(Path::new(&selected_path))&&entry==&selected_entry)else{return;};
        let new_source=self.source.as_ref().unwrap_or(&source);
        let same_directory=if local{Path::new(new).parent().is_some_and(|p|path_identity(p)==path_identity(new_source))}else{path_identity(Path::new(new))==path_identity(new_source)};
        if same_directory{if let Some(name)=if local{Path::new(new).file_name().and_then(|s|s.to_str())}else{entry.as_deref()}{if let Some(item)=self.entries.get_mut(index){item.name=name.to_owned();}}return;}
        let removed=decoded.iter().filter_map(|(_, (old,entry),_)|{if local{if Path::new(old).parent().is_some_and(|p|path_identity(p)==path_identity(&canonical)){return Path::new(old).file_name().and_then(|s|s.to_str()).map(str::to_owned);}}else if path_identity(Path::new(old))==path_identity(&canonical){return entry.clone();}None}).collect::<HashSet<_>>();
        let position=self.extra.visible.iter().position(|i|*i==index).unwrap_or(0);
        let next=self.extra.visible.iter().skip(position+1).chain(self.extra.visible.iter().take(position)).copied().find(|i|self.entries.get(*i).is_some_and(|e|!e.directory&&!removed.contains(&e.name)));
        self.selected=next;self.extra.selection.clear();if let Some(next)=next{self.extra.selection.insert(next);}
    }

    pub(super) fn copy_move_to(&mut self, moving: bool) {
        let sources = self.selected_sources();
        if sources.is_empty() {
            return;
        }
        if let Some(directory) = rfd::FileDialog::new()
            .set_title(if moving {
                "移动到文件夹"
            } else {
                "复制／提取到文件夹"
            })
            .pick_folder()
        {
            self.write_job_with_moves(|tx| {
                Box::new(move || {
                    let result = if moving {
                        fileops::move_into(&sources, &directory, false, worker::MAX_INPUT_BYTES)
                    } else {
                        fileops::copy_into(&sources, &directory, false, worker::MAX_INPUT_BYTES)
                    };
                    if moving {
                        let _ =
                            tx.send(KeyMoveBatch{moves:completed_key_moves(&sources,&directory,&result.completed),containers:result.containers.clone()});
                    }
                    batch_message(result)
                })
            });
        }
    }
    pub(super) fn delete_selected(&mut self, permanent: bool) {
        if !self.browser && self.current.is_none() && self.extra.presentation.has_frame() {
            self.status = "当前显示加载预览；请等图片就绪，或回管理模式删除".into();
            return;
        }
        let sources = self.selected_sources();
        if sources.is_empty() {
            return;
        }
        if !rfd::MessageDialog::new()
            .set_title("删除所选文件")
            .set_description(format!(
                "{} 个项目{}？文件夹会包含其全部内容；不跟随符号链接/重解析点。压缩包内删除保留原包备份。",
                sources.len(),
                if permanent {
                    "将永久删除"
                } else {
                    "将移到回收站（包内条目从包中删除）"
                }
            ))
            .set_buttons(rfd::MessageButtons::YesNo)
            .show()
            .eq(&rfd::MessageDialogResult::Yes)
        {
            return;
        }
        self.write_job_with_moves(|tx|Box::new(move || match sources.first() {
            Some(Source::Archive { path, password, .. }) => {
                let entries = sources
                    .iter()
                    .filter_map(|s| {
                        if let Source::Archive { entry, .. } = s {
                            Some(entry.clone())
                        } else {
                            None
                        }
                    })
                    .collect();
                let r = fileops::edit_archive(
                    path,
                    &fileops::ArchiveEdit::Delete(entries),
                    password.as_deref(),
                )?;
                let _=tx.send(KeyMoveBatch{moves:Vec::new(),containers:r.container_update.into_iter().collect()});
                Ok(format!("包内条目已删除；原包备份：{}", r.backup.display()))
            }
            _ => batch_message(fileops::delete_local(
                &sources
                    .into_iter()
                    .filter_map(|s| {
                        if let Source::Local(p) = s {
                            Some(p)
                        } else {
                            None
                        }
                    })
                    .collect::<Vec<_>>(),
                permanent,
            )),
        }));
    }
    pub(super) fn legacy_command(&mut self, id: u16, ctx: &egui::Context) {
        if self.extra.compare>0&&self.comparison_action(Action::LegacyCommand(id),ctx){return;}
        match id {
            2248..=2251 => {
                if let Some(index) = self.selected {
                    if let Some(source) = self.make_load(index, self.full_raw) {
                        let page = match id {
                            2248 => 0,
                            2249 => self.extra.page_index.saturating_add(1),
                            2250 => self.extra.page_index.saturating_sub(1),
                            _ => 0,
                        };
                        self.cache = ImageCache::default();
                        self.queue(
                            Request::Page {
                                source: Box::new(source),
                                index: page,
                                last: id == 2251,
                            },
                            Operation::Load {
                                index,
                                full_raw: self.full_raw,
                                preload: false,
                            },
                        );
                    }
                }
            }
            2310 => {
                self.extra.page_view = !self.extra.page_view;
            }
            2114..=2119 => self.change_record(Change::Rating((id - 2114) as u8)),
            22323..=22328 => self.change_record(Change::Label((id - 22323) as u8)),
            12021 | 12210 | 2489 => self.change_record(Change::ToggleMark),
            2144 | 2381 => self.change_record(Change::Basket(true)),
            2384 | 2385 => self.change_record(Change::Basket(false)),
            2008 => {
                if self.extra.catalog_window && self.extra.basket {
                    self.extra.catalog_window = false;
                } else {
                    self.extra.catalog_window = true;
                    self.extra.basket = true;
                    self.extra.catalog_filter = 2;
                    self.extra.catalog_page = 0;
                    self.extra.catalog_dirty = true;
                }
            }
            2037 | 2038 | 2039 => {
                self.extra.coverflow.invalidate_sequence();
                self.extra.selection = self
                    .extra
                    .visible
                    .iter()
                    .copied()
                    .filter(|i| {
                        id == 2037
                            || (!self.entries[*i].directory
                                && (id != 2039 || !self.entries[*i].archive))
                    })
                    .collect();
            }
            2040 => { self.extra.selection.clear(); self.extra.coverflow.invalidate_sequence(); },
            2279 => {self.extra.selection_rect=None;self.extra.selection_origin=None;},
            2494 => {
                self.extra.coverflow.invalidate_sequence();
                self.extra.selection = self
                    .extra
                    .visible
                    .iter()
                    .copied()
                    .filter(|i| !self.extra.selection.contains(i))
                    .collect()
            }
            10657 => {
                if self.extra.catalog_filter_task.is_some()
                    || self.extra.pending_catalog_filter.is_some()
                {
                    self.status = "目录筛选正在进行，请完成后再选择已标记项目".into();
                } else {
                    self.extra.pending_catalog_filter = Some(CatalogFilterRequest {
                        candidates: self.catalog_candidates(&self.extra.visible),
                        mode: 1,
                        selection: true,
                    });
                }
            }
            2035 | 2007 => {
                self.extra.search_panel = !self.extra.search_panel;
                self.browser = true;
            }
            12262 | 2437 => {
                self.extra.organizer = true;
                self.browser = true;
            }
            2002 => self.extra.organizer = !self.extra.organizer,
            1999 => self.extra.browser_ui.tasks = !self.extra.browser_ui.tasks,
            2001 => self.extra.browser_ui.folders = !self.extra.browser_ui.folders,
            2181 => self.perform(Action::ToggleBrowser,ctx),
            2006 => self.extra.browser_ui.preview = !self.extra.browser_ui.preview,
            2009..=2015 => {
                self.extra.layout = id;
                self.browser = true;
            }
            2075 => self.perform(Action::Fullscreen, ctx),
            2211 | 2241 => self.help = true,
            2256 => self.perform(Action::Close, ctx),
            2327 => ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(true)),
            2431 | 2432 => {
                self.extra.magnifier = true;
                self.extra.magnification = (self.extra.magnification
                    * if id == 2432 { 1.25 } else { 0.8 })
                .clamp(1.0, 32.0);
            }
            2533 | 2534 => {
                self.properties = true;
                self.extra.property_tab =
                    (self.extra.property_tab + if id == 2533 { 1 } else { 2 }) % 3;
            }
            2324 | 2229 | 2235 | 57681 => {} // Focus traversal is handled by the native focus bridge.
            2151 | 2247 => self.extra.statusbar = !self.extra.statusbar,
            2153 | 2154 | 2252 => self.extra.toolbar = !self.extra.toolbar,
            2003 | 2351 | 2490 => self.open_source_metadata(None),
            2022 => {
                if let Some(p) = self
                    .source
                    .as_ref()
                    .and_then(|p| p.parent())
                    .map(Path::to_path_buf)
                {
                    self.open_path(p);
                }
            }
            2021 | 2172 => {
                let next = if id == 2021 {
                    self.extra.history_pos.checked_add(1)
                } else {
                    self.extra.history_pos.checked_sub(1)
                };
                if let Some(n) = next.filter(|n| *n < self.extra.history.len()) {
                    self.extra.history_pos = n;
                    let p = self.extra.history[n].clone();
                    self.open_path(p);
                }
            }
            2024 => {
                let mut c = std::process::Command::new(std::env::current_exe().unwrap_or_default());
                if let Some(s) = &self.source {
                    c.arg(s);
                }
                if let Err(e) = c.spawn() {
                    self.error = Some(e.to_string());
                }
            }
            2033 | 2274 | 2061 => self.copy_move_to(false),
            2034 | 2275 => self.copy_move_to(true),
            2031 | 2261 => self.delete_selected(ctx.input(|i| i.modifiers.shift)),
            2032 | 2262 => {
                let sources=self.selected_sources();if sources.is_empty(){return;}
                self.stop_animation();if let Some(running)=&self.running{running.cancel.store(true,Ordering::Relaxed);}self.pending=None;
                self.extra.rename_generation=self.extra.rename_generation.wrapping_add(1);
                self.extra.rename_preview.clear();self.extra.rename_problem=None;
                self.extra.rename_template=fileops::RenameTemplate::default();
                self.extra.text = if sources.len()>1{self.extra.rename_template.pattern.clone()}else{sources
                    .first()
                    .and_then(|source| match source {
                        Source::Local(path) => path
                            .file_name()
                            .map(|name| name.to_string_lossy().into_owned()),
                        Source::Archive { entry, .. } => {
                            entry.rsplit(['/', '\\']).next().map(str::to_owned)
                        }
                    })
                    .unwrap_or_default()};
                self.extra.rename_sources=sources;
                self.extra.dialog = Some(id);
            }
            2103 | 2281 => {
                self.extra.cut_paths = self
                    .selected_sources()
                    .into_iter()
                    .filter_map(|s| {
                        if let Source::Local(p) = s {
                            Some(p)
                        } else {
                            None
                        }
                    })
                    .collect();
                if self.extra.cut_paths.is_empty() {
                    self.error = Some("包内文件请使用 Alt+M 移动到文件夹".into());
                } else if let Err(e) = crate::clipboard::cut_files(&self.extra.cut_paths) {
                    self.error = Some(e)
                } else {
                    self.status = "文件已剪切，请选择目标文件夹后粘贴".into();
                }
            }
            2105 | 2273 => self.paste_files(),
            2048 => {
                let sources = self.selected_sources();
                if let Some(path) = rfd::FileDialog::new()
                    .set_file_name("文件列表.txt")
                    .save_file()
                {
                    self.write_job(Box::new(move || {
                        fileops::write_file_list(&sources, &path)?;
                        Ok("文件列表已保存".into())
                    }));
                }
            }
            2412 | 2263 => {
                self.read_record();
                self.extra.text = self.extra.record.title.clone();
                self.extra.dialog = Some(id);
            }
            2435 => {
                self.read_record();
                self.extra.text = self.extra.record.category.clone();
                self.extra.dialog = Some(id);
            }
            2436 => self.open_source_metadata(Some(3)),
            22298 => {
                self.read_record();
                self.extra.text = self.extra.record.keywords.clone();
                self.extra.dialog = Some(id);
            }
            21853 => {
                let keys = self
                    .selected_indices()
                    .into_iter()
                    .filter_map(|i| self.item_key(i))
                    .collect::<Vec<_>>();
                match self
                    .extra
                    .catalog
                    .as_mut()
                    .ok_or_else(|| "无法打开元数据库".to_owned())
                    .and_then(|c| c.apply_default(&keys))
                {
                    Ok(()) => {
                        self.read_record();
                        self.rebuild_visible();
                        self.status = "已应用默认元数据预设".into();
                    }
                    Err(e) => {
                        self.status = e;
                        self.extra.dialog = Some(21853);
                    }
                }
            }
            2047 | 2284 => {
                self.extra.text = fileops::format_local_datetime(std::time::SystemTime::now());
                self.extra.dialog = Some(id);
            }
            2302 => {
                self.extra.value = self.scale * ctx.pixels_per_point() * 100.0;
                self.extra.dialog = Some(id);
            }
            2050 | 2230 => self.extra.settings_ui.open = true,
            2160 => self.extra.pan_lock = !self.extra.pan_lock,
            2277 => self.extra.zoom_lock = !self.extra.zoom_lock,
            2313 => self.extra.magnifier = !self.extra.magnifier,
            2314 => self.extra.navigator = !self.extra.navigator,
            2088 => self.extra.histogram = !self.extra.histogram,
            11151 | 2862 => {
                self.extra.color_managed = !self.extra.color_managed;
                self.cache = ImageCache::default();
                if let Some(i) = self.selected {
                    self.load_index(i, self.full_raw);
                }
            }
            22109 | 22067 => {
                self.extra.exposure_warning = !self.extra.exposure_warning;
                self.extra.exposure_texture = None;
            }
            2371..=2373 => self.extra.tool = id,
            2300 | 2301 => {
                if let Some(image) = &self.current {
                    let(width,height)=if self.rotation%2==0{(image.width,image.height)}else{(image.height,image.width)};
                    self.fit = false;
                    self.target_scale = if id == 2300 {
                        self.extra.viewport.width() / width as f32
                    } else {
                        self.extra.viewport.height() / height as f32
                    };
                    self.pan = Vec2::ZERO;
                }
            }
            2303 => self.extra.dialog = Some(id),
            13023|13024|13026|13029|13030|13036|13037 => self.insert_header_field(id),
            2375 => self.extra.header_visible = !self.extra.header_visible,
            2308 => {
                self.extra.background_tile = None;
            }
            2331 => {
                if let Some(r) = self.extra.selection_rect {
                    self.fit = false;
                    self.target_scale = (self.extra.viewport.width() / r.width())
                        .min(self.extra.viewport.height() / r.height())
                        * self.scale;
                    self.pan += (self.extra.viewport.center() - r.center()) * self.target_scale
                        / self.scale;
                }
            }
            2401 | 10415 => {
                self.extra.descending = false;
                self.rebuild_visible();
            }
            2402 | 10416 => {
                self.extra.descending = true;
                self.rebuild_visible();
            }
            12171 | 12172 => {
                self.extra.group_descending = Some(id == 12172);
                self.rebuild_visible();
            }
            10420 => {if matches!(self.extra.edit_dialog,Some(10579|10556)){self.save_edit_record();}else{self.save_edits(self.extra.edit_dialog.is_some_and(|id|matches!(id,2043..=2046|10576)));}},
            22294 => self.save_edits(false),
            22119 => self.commit_workspace_tool(),
            2043 | 2268 | 22490 | 2044 | 2285 | 2045 | 21852 | 2046 | 21851
            | 10576 | 10579 | 10556 | 21902 | 2356 => self.open_edit_dialog(id),
            2315..=2323 => self.color_depth(id),
            2311 => self.undo_edit(false),
            2312 => self.undo_edit(true),
            2528 => self.start_comparison(),
            2164..=2167 => self.extra.compare = (id - 2163) as usize,
            2138 | 2185 | 2186 | 2334 => self.external_open(id != 2185),
            2106 | 2259 | 2282 => self.print_image(id == 2282),
            2178 | 2179 | 2180 | 2265 | 2266 | 2267 | 21897 => self.wallpaper(id),
            2028 => self.choose_folder(),
            12238 => {
                self.extra.catalog_window = true;
                self.extra.basket = false;
                self.extra.catalog_filter = 0;
                self.extra.catalog_page = 0;
                self.extra.catalog_dirty = true;
            }
            2233 => self.external_open(false),
            2194 => {
                self.extra.text = self
                    .source
                    .as_ref()
                    .map(|path| path.to_string_lossy().into_owned())
                    .unwrap_or_default();
                self.extra.dialog = Some(id);
            }
            // Unknown resource commands stay inactive until their original meaning is evidenced.
            _ => {
                self.status = format!("此原版命令尚未完成兼容：{id}（详见快捷键验收表）");
            }
        }
    }
    pub(super) fn paste_files(&mut self) {
        if self.extra.writing.is_some() || self.extra.pending_write.is_some() {
            return;
        }
        let clipboard = match crate::clipboard::read_files() {
            Ok(Some(p)) => p,
            Ok(None) => {
                let dir = match tempfile::Builder::new().prefix("rustview-paste-").tempdir() {
                    Ok(d) => d,
                    Err(e) => {
                        self.error = Some(e.to_string());
                        return;
                    }
                };
                let path = dir.path().to_owned();
                let destination = path.join("clipboard.png");
                self.extra.clipboard_image_dir = Some(dir);
                self.queue(
                    Request::Clipboard { destination },
                    Operation::Clipboard { path },
                );
                return;
            }
            Err(e) => {
                self.error = Some(e);
                return;
            }
        };
        let paths = clipboard.paths.clone();
        let cut = clipboard.cut;
        let Some(destination) = self.source.clone() else {
            return;
        };
        if cut && !destination.is_dir() {
            self.error = Some("向压缩包粘贴暂只支持复制，请先Ctrl+C复制文件".into());
            return;
        }
        if cut {
            self.extra.finish_cut = Some((
                clipboard,
                paths
                    .iter()
                    .filter_map(|p| p.file_name().map(|n| destination.join(n)))
                    .collect(),
            ));
        }
        let password = self.password.clone();
        self.write_job_with_moves(|tx| {
            Box::new(move || {
                if destination.is_dir() {
                    let result = fileops::paste_local(&paths, &destination, cut, false);
                    if cut {
                        let sources = paths.iter().cloned().map(Source::Local).collect::<Vec<_>>();
                        let _ = tx.send(KeyMoveBatch{moves:completed_key_moves(
                            &sources,
                            &destination,
                            &result.completed,
                        ),containers:Vec::new()});
                    }
                    batch_message(result)
                } else {
                    let r = fileops::edit_archive(
                        &destination,
                        &fileops::ArchiveEdit::Add(paths),
                        password.as_deref(),
                    )?;
                    let _=tx.send(KeyMoveBatch{moves:Vec::new(),containers:r.container_update.into_iter().collect()});
                    Ok(format!("已添加到压缩包；原包备份：{}", r.backup.display()))
                }
            })
        });
        self.extra.cut_paths.clear();
    }
    pub(super) fn copy_file_selection(&mut self) {
        if self.extra.writing.is_some() || self.extra.pending_write.is_some() {
            return;
        }
        let sources = self.selected_sources();
        if sources.is_empty() {
            return;
        }
        if sources.iter().all(|s| matches!(s, Source::Local(_))) {
            let paths = sources
                .into_iter()
                .filter_map(|s| {
                    if let Source::Local(p) = s {
                        Some(p)
                    } else {
                        None
                    }
                })
                .collect::<Vec<_>>();
            match crate::clipboard::copy_files(&paths) {
                Ok(()) => self.status = format!("已复制 {} 个文件引用", paths.len()),
                Err(e) => self.error = Some(e),
            }
            return;
        }
        let Some(path)=clipboard_cache_path()else{self.error=Some("无法定位剪贴板缓存目录".into());return;};
        let pinned=match crate::clipboard::read_files(){Ok(Some(files))=>files.paths,Ok(None)=>Vec::new(),Err(e)=>{self.error=Some(e);return;}};
        let (tx,rx)=mpsc::sync_channel(1);self.extra.clipboard_ready=Some(rx);
        self.write_job_no_refresh(Box::new(move||{
            let exported=fileops::export_clipboard_cache(&sources,&path,&pinned)?;
            let count=exported.paths.len();let _=tx.send(exported.paths);
            Ok(format!("已复制{count}个包内原文件，可在关闭程序后粘贴；缓存目录：{}",exported.directory.display()))
        }));
    }
    fn draw_catalog_window(&mut self, ctx: &egui::Context) {
        if !self.extra.catalog_window {
            return;
        }
        if self.extra.catalog_dirty {
            self.extra.catalog_dirty = false;
            let result = self
                .extra
                .catalog
                .as_ref()
                .ok_or_else(|| "无法打开RustView目录库".to_owned())
                .and_then(|catalog| {
                    catalog.list(
                        catalog_filter(self.extra.catalog_filter)?,
                        CATALOG_PAGE + 1,
                        self.extra.catalog_page.saturating_mul(CATALOG_PAGE),
                    )
                });
            match result {
                Ok(mut rows) => {
                    self.extra.catalog_more = rows.len() > CATALOG_PAGE;
                    rows.truncate(CATALOG_PAGE);
                    self.extra.catalog_rows = rows;
                }
                Err(error) => {
                    self.extra.catalog_rows.clear();
                    self.extra.catalog_more = false;
                    self.error = Some(error);
                }
            }
        }
        let mut open = true;
        let mut jump = None;
        let mut remove = None;
        let mut folder = None;
        egui::Window::new("图片目录与图片篮子").id(egui::Id::new("catalog-global"))
            .open(&mut open).default_size(egui::vec2(760.0,480.0)).show(ctx,|ui|{
                ui.label("跨文件夹和压缩包的RustView图片信息；每页最多100项。不会把压缩包密码写入目录库。");
                let previous=self.extra.catalog_filter;
                ui.horizontal(|ui|{
                    egui::ComboBox::from_label("显示").selected_text(filter_name(self.extra.catalog_filter)).show_ui(ui,|ui|{
                        for filter in 0..=12{ui.selectable_value(&mut self.extra.catalog_filter,filter,filter_name(filter));}
                    });
                    if ui.button("刷新").clicked(){self.extra.catalog_dirty=true;}
                });
                if previous!=self.extra.catalog_filter{self.extra.catalog_page=0;self.extra.catalog_dirty=true;self.extra.basket=self.extra.catalog_filter==2;}
                ui.horizontal(|ui|{
                    if ui.add_enabled(self.extra.catalog_page>0,egui::Button::new("上一页")).clicked(){self.extra.catalog_page-=1;self.extra.catalog_dirty=true;}
                    ui.label(format!("第 {} 页",self.extra.catalog_page+1));
                    if ui.add_enabled(self.extra.catalog_more,egui::Button::new("下一页")).clicked(){self.extra.catalog_page+=1;self.extra.catalog_dirty=true;}
                });
                egui::ScrollArea::vertical().max_height(520.0).show(ui,|ui|{
                    if self.extra.catalog_rows.is_empty(){ui.label("本页没有符合条件的记录；可返回上一页或更改筛选。");}
                    for(key,record)in &self.extra.catalog_rows{
                        ui.horizontal(|ui|{
                            if ui.button("打开").clicked(){jump=Some(key.clone());}
                            let response=ui.label(catalog_key_label(key));
                            response.context_menu(|ui|{
                                if ui.button("打开图片").clicked(){jump=Some(key.clone());ui.close_menu();}
                                if ui.button("打开所在文件夹").clicked(){folder=Some(key.clone());ui.close_menu();}
                                if record.basket&&ui.button("移出图片篮子").clicked(){remove=Some(key.clone());ui.close_menu();}
                            });
                        });
                        ui.small(format!("评级 {}　颜色 {}　{}{}",record.rating,record.label,record.category,if record.marked{"　已标记"}else{""}));
                        if !record.title.is_empty(){ui.label(&record.title);}
                        ui.separator();
                    }
                });
            });
        self.extra.catalog_window = open;
        if let Some(key) = remove {
            match self
                .extra
                .catalog
                .as_mut()
                .ok_or_else(|| "无法打开RustView目录库".to_owned())
                .and_then(|catalog| catalog.set_basket(&key, false))
            {
                Ok(_) => {
                    self.extra.catalog_dirty = true;
                    self.read_record();
                    self.rebuild_visible();
                }
                Err(error) => self.error = Some(error),
            }
        }
        if let Some(key) = folder {
            match decode_catalog_key(&key) {
                Ok((path, _)) => {
                    if let Some(parent) = path.parent() {
                        self.open_path(parent.to_owned());
                    }
                }
                Err(error) => self.error = Some(error),
            }
        } else if let Some(key) = jump {
            match decode_catalog_key(&key) {
                Ok((path, entry)) => {
                    if !path.is_file() {
                        self.error = Some(format!("图片或压缩包已不存在：{}", path.display()));
                    } else {
                        self.open_path(path);
                        if let Some(entry) = entry {
                            if let Some(Task {
                                operation: Operation::List { select, .. },
                                ..
                            }) = &mut self.pending
                            {
                                *select = Some(entry);
                            }
                        }
                    }
                }
                Err(error) => self.error = Some(error),
            }
        }
    }
    pub(super) fn draw_extra_panels(&mut self, ctx: &egui::Context) {
        self.poll_catalog_filter(ctx);
        self.draw_catalog_window(ctx);
        if self.extra.organizer {
            egui::SidePanel::right("organizer")
                .default_width(220.0)
                .show(ctx, |ui| {
                    ui.heading("整理与筛选");
                    let old = self.extra.filter;
                    egui::ComboBox::from_label("筛选")
                        .selected_text(filter_name(self.extra.filter))
                        .show_ui(ui, |ui| {
                            for i in 0..=12 {
                                ui.selectable_value(&mut self.extra.filter, i, filter_name(i));
                            }
                        });
                    if old != self.extra.filter {
                        self.rebuild_visible();
                    }
                    ui.label(format!("已选 {} 项", self.selected_indices().len()));
                    ui.horizontal(|ui| {
                        ui.label("评级");
                        for n in 0..=5 {
                            if ui
                                .selectable_label(self.extra.record.rating == n, n.to_string())
                                .clicked()
                            {
                                self.change_record(Change::Rating(n));
                            }
                        }
                    });
                    ui.horizontal(|ui| {
                        ui.label("标签");
                        for n in 0..=5 {
                            if ui
                                .selectable_label(self.extra.record.label == n, n.to_string())
                                .clicked()
                            {
                                self.change_record(Change::Label(n));
                            }
                        }
                    });
                    if ui.checkbox(&mut self.extra.record.marked, "标记").changed() {
                        self.change_record(Change::ToggleMark);
                    }
                    if ui
                        .checkbox(&mut self.extra.record.basket, "图片篮子")
                        .changed()
                    {
                        self.change_record(Change::Basket(self.extra.record.basket));
                    }
                    ui.label("类别");
                    ui.text_edit_singleline(&mut self.extra.text);
                    if ui.button("设为所选项目的类别").clicked() {
                        self.change_record(Change::Category(self.extra.text.clone()));
                    }
                    if self.extra.catalog.is_none() {
                        ui.colored_label(Color32::LIGHT_RED, "元数据库不可写");
                    }
                });
        }
        if self.extra.search_panel {
            egui::TopBottomPanel::top("search").show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.label("在当前目录／压缩包内查找");
                    if ui.text_edit_singleline(&mut self.extra.search).changed() {
                        self.rebuild_visible();
                    }
                    if ui.button("清空").clicked() {
                        self.extra.search.clear();
                        self.rebuild_visible();
                    }
                });
            });
        }
        if self.extra.header_visible {
            let text = self.expand_fields(&self.extra.header);
            egui::TopBottomPanel::top("header").show(ctx, |ui| {
                ui.label(text);
            });
            let text = self.expand_fields(&self.extra.footer);
            egui::TopBottomPanel::bottom("footer").show(ctx, |ui| {
                ui.label(text);
            });
        }
    }
    pub(super) fn expand_fields(&self, text: &str) -> String {
        crate::printing::expand_fields(text, &self.source_fields())
    }
    pub(super) fn command_dialogs(&mut self, ctx: &egui::Context) {
        let Some(id) = self.extra.dialog else {
            return;
        };
        if matches!(id,2032|2262){self.rename_dialog(ctx);return;}
        let first = self.extra.dialog_focus_seen != Some(id);
        self.extra.dialog_focus_seen = Some(id);
        let mut close = false;
        let mut submit = false;
        egui::Modal::new(egui::Id::new("command-dialog")).show(ctx,|ui|{
            ui.heading(match id{2032|2262=>"重命名所选文件",2302=>"缩放到",2047|2284=>"调整文件时间戳",2303=>"编辑页眉／页脚",2050|2230=>"选项",12238=>"图片目录",2412|2263=>"设置图片标题",2435=>"批量设置图片信息",2436=>"设置IPTC关键词",22298=>"设置图片关键词",21853=>"默认元数据预设",2194=>"打开目录路径",_=>"图片信息"});
            match id{
                2302=>{ui.add(egui::Slider::new(&mut self.extra.value,1.0..=6400.0).logarithmic(true).suffix("%"));},
                2303=>{ui.label("页眉");if ui.text_edit_singleline(&mut self.extra.header).has_focus(){self.set_header_field_target(false);}ui.label("页脚");if ui.text_edit_singleline(&mut self.extra.footer).has_focus(){self.set_header_field_target(true);}ui.horizontal_wrapped(|ui|{for(title,id)in[("页码",13023),("总页数",13024),("文件名",13026),("宽",13029),("高",13030),("日期",13036),("时间",13037)]{if ui.small_button(title).clicked(){self.insert_header_field(id);}}});ui.small("支持 {name}、{width}、{height}、{title}、{date} 与完整元数据字段");},
                2050|2230=>{ui.checkbox(&mut self.extra.pan_lock,"翻图保留平移位置");ui.checkbox(&mut self.extra.zoom_lock,"翻图保留缩放倍率");ui.label("图像缓存上限 768 MiB；动画期间清空静态预读；GPU细节 256 MiB。");
                    if ui.button("将当前图像预览设为平铺背景").clicked(){if let Some(image)=&self.current{self.extra.background_tile=Some(ctx.load_texture("background-tile",egui::ColorImage::from_rgba_unmultiplied([image.preview_width as usize,image.preview_height as usize],&image.preview),egui::TextureOptions{wrap_mode:egui::TextureWrapMode::Repeat,..egui::TextureOptions::LINEAR}));}}
                    if ui.button("重设背景平铺 (Ctrl+Shift+G)").clicked(){self.extra.background_tile=None;}
                },
                12238=>{ui.label("目录信息存储在 RustView-data/catalog.sqlite；评级、颜色标签、类别、标记和图片篮子随重启保留。");},
                2412|2263=>{ui.label("标题（保存在RustView目录库）");primary_text(ui,&mut self.extra.text,first);},
                2435=>{ui.label("标题");ui.text_edit_singleline(&mut self.extra.record.title);ui.label("说明");ui.text_edit_multiline(&mut self.extra.record.description);ui.label("类别");primary_text(ui,&mut self.extra.text,first);ui.small("保存在RustView目录库，不修改源图元数据。");},
                2436|22298=>{ui.label(if id==2436{"IPTC关键词（RustView目录库）"}else{"ACDSee习惯的关键词字段（RustView目录库）"});primary_text(ui,&mut self.extra.text,first);ui.small("此处保存到本程序目录库，不回写源图IPTC/XMP。");},
                21853=>{ui.label(&self.status);ui.label("默认预设会应用当前图片的评级、颜色标签、类别、标题、说明和关键词；保留各图片的标记及篮子状态。");ui.label("按确定将当前图片信息保存为默认预设，下次Ctrl+M应用。");},
                _=>{primary_text(ui,&mut self.extra.text,first);if matches!(id,2047|2284){ui.small("输入本地时间 YYYY-MM-DD HH:MM:SS；只修改本地文件的最后写入时间，压缩包条目不在此操作范围内");}
                    if matches!(id,2032|2262){ui.small("只重命名所选的第一个文件；包内条目会保留所在子目录，并备份原包。");}}
            }
            ui.horizontal(|ui|{if ui.button("确定").clicked(){submit=true;}if ui.button("取消").clicked(){close=true;}});
            if ctx.input(|i|i.key_pressed(egui::Key::Escape)){close=true;}
        });
        if submit {
            match id {
                2302 => {
                    self.fit = false;
                    self.target_scale = self.extra.value / 100.0 / ctx.pixels_per_point();
                    self.zoom_anchor = None;
                }
                2412 | 2263 => self.change_record(Change::Title(self.extra.text.clone())),
                2435 => {
                    let title = self.extra.record.title.clone();
                    let description = self.extra.record.description.clone();
                    let category = self.extra.text.clone();
                    self.change_record(Change::Title(title));
                    self.change_record(Change::Description(description));
                    self.change_record(Change::Category(category));
                }
                2436 => self.change_record(Change::IptcKeywords(self.extra.text.clone())),
                22298 => self.change_record(Change::Keywords(self.extra.text.clone())),
                21853 => {
                    self.read_record();
                    if let Some(c) = &mut self.extra.catalog {
                        match c.set_default_preset(&self.extra.record) {
                            Ok(()) => self.status = "默认元数据预设已保存".into(),
                            Err(e) => self.error = Some(e),
                        }
                    }
                }
                2194 => self.open_path(PathBuf::from(self.extra.text.trim())),
                2047 | 2284 => match fileops::parse_local_datetime(&self.extra.text) {
                    Ok(t) => {
                        let paths = self
                            .selected_sources()
                            .into_iter()
                            .filter_map(|s| {
                                if let Source::Local(p) = s {
                                    Some(p)
                                } else {
                                    None
                                }
                            })
                            .collect::<Vec<_>>();
                        self.write_job(Box::new(move || {
                            batch_message(fileops::set_times(
                                &paths,
                                &fileops::Times {
                                    modified: Some(t),
                                    accessed: None,
                                    created: None,
                                },
                            ))
                        }));
                    }
                    Err(e) => self.error = Some(e),
                },
                _ => {}
            }
            close = true;
        }
        if close {
            self.extra.dialog = None;
            self.extra.dialog_focus_seen = None;
        }
    }
}

impl Viewer {
    fn poll_rename_preview(&mut self,ctx:&egui::Context){
        if let Some(result)=self.extra.rename_build.as_ref().map(|rx|rx.try_recv()){
            match result{
                Ok((generation,result))=>{
                    self.extra.rename_build=None;
                    if generation==self.extra.rename_generation&&matches!(self.extra.dialog,Some(2032|2262)){
                        match result{Ok(plan)=>{self.extra.rename_preview=plan;self.extra.rename_problem=None;},Err(e)=>self.extra.rename_problem=Some(e)}
                    }
                },
                Err(mpsc::TryRecvError::Disconnected)=>{self.extra.rename_build=None;self.extra.rename_problem=Some("重命名预览读取已结束，请重新生成".into());},
                Err(mpsc::TryRecvError::Empty)=>{}
            }
        }
        if self.extra.rename_build.is_some(){ctx.request_repaint_after(std::time::Duration::from_millis(40));}
    }

    fn start_rename_preview(&mut self,ctx:&egui::Context){
        if self.extra.rename_build.is_some()||self.running.is_some()||self.extra.retiring.is_some(){self.extra.rename_problem=Some("请等待当前读取完成后生成预览".into());return;}
        let sources=self.extra.rename_sources.clone();let mut template=self.extra.rename_template.clone();template.pattern=self.extra.text.clone();
        let generation=self.extra.rename_generation;let(tx,rx)=mpsc::sync_channel(1);self.extra.rename_build=Some(rx);self.extra.rename_problem=None;
        let context=ctx.clone();
        std::thread::spawn(move||{let result=std::panic::catch_unwind(std::panic::AssertUnwindSafe(||fileops::plan_rename(&sources,&template))).unwrap_or_else(|_|Err("重命名预览读取意外结束".into()));let _=tx.send((generation,result));context.request_repaint();});
    }

    fn rename_dialog(&mut self,ctx:&egui::Context){
        let multiple=self.extra.rename_sources.len()>1;let busy=self.extra.rename_build.is_some();
        let first=self.extra.dialog_focus_seen!=self.extra.dialog;self.extra.dialog_focus_seen=self.extra.dialog;
        let mut close=false;let mut apply=false;let mut preview=false;
        egui::Modal::new(egui::Id::new("rename-plan")).show(ctx,|ui|{
            ui.heading(if multiple{"批量重命名"}else{"重命名"});
            if multiple{
                let mut changed=false;
                ui.add_enabled_ui(!busy,|ui|{
                    ui.label("模板");let response=ui.add(egui::TextEdit::singleline(&mut self.extra.text).desired_width(460.0));if first{response.request_focus();}changed|=response.changed();
                    ui.horizontal(|ui|{ui.label("起始序号");changed|=ui.add(egui::DragValue::new(&mut self.extra.rename_template.start)).changed();ui.label("步长");changed|=ui.add(egui::DragValue::new(&mut self.extra.rename_template.step)).changed();ui.label("位数");changed|=ui.add(egui::DragValue::new(&mut self.extra.rename_template.padding).range(0..=20)).changed();});
                });
                if changed{self.extra.rename_preview.clear();self.extra.rename_problem=None;}
                ui.small("{name}原名主体；{ext}含点扩展名；{n}序号；{date}/{time}各项目的修改日期/时间；也支持*原名和###序号。");
                ui.label(format!("按当前列表顺序处理 {} 个项目，先检查全部冲突；ZIP/7z一次提交并保留原包备份。",self.extra.rename_sources.len()));
                if ui.add_enabled(!busy,egui::Button::new("生成预览")).clicked(){preview=true;}
                if busy{ui.horizontal(|ui|{ui.spinner();ui.label("正在读取重命名计划…");});}
                if !self.extra.rename_preview.is_empty(){
                    egui::ScrollArea::vertical().max_height(280.0).show_rows(ui,24.0,self.extra.rename_preview.len(),|ui,range|{
                        for i in range{let item=&self.extra.rename_preview[i];ui.horizontal(|ui|{ui.add(egui::Label::new(rename_source_label(&item.source)).truncate());ui.label("→");ui.label(&item.new_name);});}
                    });
                }
                if let Some(problem)=&self.extra.rename_problem{ui.colored_label(Color32::LIGHT_RED,problem);}
            }else{primary_text(ui,&mut self.extra.text,first);ui.small("输入完整文件名；目录/压缩包整体改名会同步其后代图片信息。");}
            ui.horizontal(|ui|{if ui.add_enabled(!busy&&(!multiple||!self.extra.rename_preview.is_empty()),egui::Button::new("确认重命名")).clicked(){apply=true;}if ui.button("取消").clicked(){close=true;}});
            if ctx.input(|i|i.key_pressed(egui::Key::Escape)){close=true;}
            if ctx.input(|i|i.key_pressed(egui::Key::Enter))&&!busy{if multiple&&self.extra.rename_preview.is_empty(){preview=true;}else{apply=true;}}
        });
        if preview{self.start_rename_preview(ctx);}
        if apply{
            if multiple{
                let plan=std::mem::take(&mut self.extra.rename_preview);
                self.write_job_with_moves(|tx|Box::new(move||{
                    let result=fileops::rename_many(&plan)?;let mut moves=Vec::with_capacity(result.moved.len());
                    for(old,new)in result.moved{moves.push((source_key(&old)?,source_key(&new)?));}
                    let count=moves.len();let _=tx.send(KeyMoveBatch{moves,containers:result.containers});
                    Ok(format!("已批量重命名 {count} 项{}",if result.backups.is_empty(){String::new()}else{format!("；原包备份：{}",result.backups.iter().map(|p|p.display().to_string()).collect::<Vec<_>>().join("；"))}))
                }));
            }else if let Some(source)=self.extra.rename_sources.first().cloned(){
                let name=self.extra.text.clone();
                if rename_source_label(&source)==name{self.status="名称未变化".into();}
                else{self.write_job_with_moves(|tx|Box::new(move||{
                    let old=source_key(&source)?;
                    let(message,new,containers)=match source{
                        Source::Local(path)=>{let renamed=fileops::rename_local(&path,&name)?;("项目已重命名".into(),source_key(&Source::Local(renamed))?,Vec::new())},
                        Source::Archive{path,entry,password}=>{let result=fileops::edit_archive(&path,&fileops::ArchiveEdit::Rename{entry,new_name:name},password.as_deref())?;let renamed=result.renamed_entry.ok_or("名称未变化")?;(format!("条目已重命名，原包备份：{}",result.backup.display()),source_key(&Source::Archive{path,entry:renamed,password:None})?,result.container_update.into_iter().collect())}
                    };let _=tx.send(KeyMoveBatch{moves:vec![(old,new)],containers});Ok(message)
                }));}
            }
            close=true;
        }
        if close{self.extra.dialog=None;self.extra.dialog_focus_seen=None;self.extra.rename_generation=self.extra.rename_generation.wrapping_add(1);self.extra.rename_sources.clear();self.extra.rename_preview.clear();self.extra.rename_problem=None;}
    }

    pub(super) fn clear_clipboard_exports(&mut self){
        let pinned=match crate::clipboard::read_files(){Ok(Some(files))=>files.paths,Ok(None)=>Vec::new(),Err(e)=>{self.error=Some(e);return;}};
        let Some(root)=clipboard_cache_path()else{return;};
        if rfd::MessageDialog::new().set_title("清理剪贴板导出缓存").set_description("清理本程序保存的包内原文件副本；当前剪贴板仍引用的副本会保留，源压缩包和原图片不会改变。").set_buttons(rfd::MessageButtons::YesNo).show()!=rfd::MessageDialogResult::Yes{return;}
        self.write_job_no_refresh(Box::new(move||{let result=fileops::clear_clipboard_cache(&root,&pinned)?;Ok(format!("清理 {} 个导出目录，释放 {:.1} MiB；仍保留 {:.1} MiB（含当前剪贴板引用）",result.removed,result.bytes as f64/1048576.0,result.retained_bytes as f64/1048576.0))}));
    }
}

fn rename_source_label(source:&Source)->String{match source{Source::Local(path)=>path.file_name().map(|n|n.to_string_lossy().into_owned()).unwrap_or_default(),Source::Archive{entry,..}=>entry.rsplit(['/', '\\']).next().unwrap_or(entry).to_owned()}}
fn clipboard_cache_path()->Option<PathBuf>{std::env::current_exe().ok().and_then(|p|p.parent().map(|p|p.join("RustView-data/clipboard-cache")))}

fn batch_message(result: fileops::BatchResult) -> Result<String, String> {
    let mut message = format!("完成 {} 项", result.completed.len());
    if !result.backups.is_empty() {
        message.push_str(&format!(
            "；原包备份：{}",
            result
                .backups
                .iter()
                .take(5)
                .map(|p| p.display().to_string())
                .collect::<Vec<_>>()
                .join("；")
        ));
    }
    if result.failures.is_empty() {
        Ok(message)
    } else {
        Err(format!(
            "{message}；失败 {} 项\n{}",
            result.failures.len(),
            result
                .failures
                .iter()
                .take(10)
                .map(|(p, e)| format!("{p}：{e}"))
                .collect::<Vec<_>>()
                .join("\n")
        ))
    }
}
fn filter_name(n: u8) -> String {
    match n {
        0 => "全部".into(),
        1 => "已标记".into(),
        2 => "图片篮子".into(),
        3..=7 => format!("{} 星", n - 2),
        8..=12 => format!("颜色标签 {}", n - 7),
        _ => "全部".into(),
    }
}

fn sort_groups(indices: &mut [usize], entries: &[Entry], descending: bool) {
    let extension = |index: &usize| {
        entries
            .get(*index)
            .and_then(|entry| Path::new(&entry.name).extension())
            .map(|extension| extension.to_string_lossy().to_lowercase())
            .unwrap_or_default()
    };
    if descending {
        indices.sort_by_cached_key(|index| std::cmp::Reverse(extension(index)));
    } else {
        indices.sort_by_cached_key(extension);
    }
}

const WRITE_INTERRUPTED: &str = "文件操作线程意外结束；请检查部分文件是否已完成操作后再重试";
fn execute_write(job: WriteJob) -> Result<String, String> {
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(job))
        .unwrap_or_else(|_| Err(WRITE_INTERRUPTED.into()))
}
fn poll_write_result(
    receiver: &Receiver<Result<String, String>>,
) -> Option<Result<String, String>> {
    match receiver.try_recv() {
        Ok(result) => Some(result),
        Err(mpsc::TryRecvError::Disconnected) => Some(Err(WRITE_INTERRUPTED.into())),
        _ => None,
    }
}

fn catalog_path() -> Option<PathBuf> {
    std::env::current_exe()
        .ok()?
        .parent()
        .map(|path| path.join("RustView-data/catalog.sqlite"))
}

fn catalog_filter(mode: u8) -> Result<Filter, String> {
    match mode {
        0 => Ok(Filter::All),
        1 => Ok(Filter::Marked),
        2 => Ok(Filter::Basket),
        3..=7 => Ok(Filter::Rating(mode - 2)),
        8..=12 => Ok(Filter::Label(mode - 7)),
        _ => Err("无效的目录筛选".into()),
    }
}

fn filter_catalog(
    database: &Path,
    request: CatalogFilterRequest,
    cancel: &AtomicBool,
) -> Result<Vec<usize>, String> {
    if request.candidates.is_empty() {
        return Ok(Vec::new());
    }
    let catalog = Catalog::open(database)?;
    let mut indices = Vec::new();
    let mut offset = 0;
    loop {
        if cancel.load(Ordering::Relaxed) {
            return Ok(Vec::new());
        }
        let rows = catalog.list(catalog_filter(request.mode)?, 500, offset)?;
        for (key, _) in &rows {
            if let Some(index) = request.candidates.get(key) {
                indices.push(*index);
            }
        }
        if rows.len() < 500 || indices.len() == request.candidates.len() {
            return Ok(indices);
        }
        offset += rows.len();
    }
}

fn decode_catalog_key(key: &str) -> Result<(PathBuf, Option<String>), String> {
    let (path, entry): (String, Option<String>) =
        serde_json::from_str(key).map_err(|_| "此目录记录的路径格式无法识别")?;
    if path.is_empty()
        || path.contains('\0')
        || entry
            .as_ref()
            .is_some_and(|entry| entry.is_empty() || entry.contains('\0'))
    {
        return Err("目录记录含无效路径".into());
    }
    Ok((PathBuf::from(path), entry))
}

fn catalog_key_label(key: &str) -> String {
    match decode_catalog_key(key) {
        Ok((path, Some(entry))) => format!("{} :: {entry}", path.display()),
        Ok((path, None)) => path.display().to_string(),
        Err(_) => "无法识别的目录记录".into(),
    }
}

fn source_key(source: &Source) -> Result<String, String> {
    let (path, entry) = match source {
        Source::Local(path) => {
            let parent = path.parent().unwrap_or(Path::new("."));
            let parent = parent.canonicalize().unwrap_or_else(|_| parent.to_owned());
            (
                parent.join(path.file_name().ok_or("文件没有有效名称")?),
                None,
            )
        }
        Source::Archive { path, entry, .. } => (
            path.canonicalize().unwrap_or_else(|_| path.clone()),
            Some(entry.as_str()),
        ),
    };
    serde_json::to_string(&(path.to_string_lossy(), entry)).map_err(|error| error.to_string())
}

fn path_identity(path:&Path)->String{crate::recipes::normal_path(path)}

fn completed_key_moves(sources: &[Source], directory: &Path, completed: &[PathBuf]) -> KeyMoves {
    let completed: HashSet<_> = completed.iter().collect();
    let mut targets = HashSet::new();
    sources
        .iter()
        .filter_map(|source| {
            let name = match source {
                Source::Local(path) => path.file_name()?.to_string_lossy(),
                Source::Archive { entry, .. } => entry.rsplit(['/', '\\']).next()?.into(),
            };
            let target = directory.join(name.as_ref());
            if !targets.insert(target.to_string_lossy().to_lowercase())
                || !completed.contains(&target)
            {
                return None;
            }
            Some((
                source_key(source).ok()?,
                source_key(&Source::Local(target)).ok()?,
            ))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rename_refresh_keeps_new_name_and_move_out_chooses_next_original_item(){
        let root=tempfile::tempdir().unwrap();let folder=root.path().join("photos");let target=root.path().join("target");std::fs::create_dir(&folder).unwrap();std::fs::create_dir(&target).unwrap();for name in["a.jpg","b.jpg"]{std::fs::write(folder.join(name),name.as_bytes()).unwrap();}
        let make=|name:&str|Entry{name:name.into(),size:5,directory:false,archive:false,modified:None};let mut app=super::super::browser_ui::tests::viewer(vec![make("a.jpg"),make("b.jpg")]);app.source=Some(folder.clone());let old=Source::Local(folder.join("a.jpg"));let new=fileops::rename_local(&folder.join("a.jpg"),"renamed.jpg").unwrap();let new=Source::Local(new);app.relocate_management_focus(&vec![(source_key(&old).unwrap(),source_key(&new).unwrap())]);assert_eq!(app.entries[0].name,"renamed.jpg");assert_eq!(app.selected,Some(0));
        let ctx=egui::Context::default();app.perform(Action::Refresh,&ctx);assert!(matches!(app.pending.as_ref().map(|t|&t.operation),Some(Operation::List{select:Some(name),..})if name=="renamed.jpg"));app.pending=None;
        let result=fileops::paste_local(&[folder.join("renamed.jpg")],&target,true,false);assert!(result.failures.is_empty());app.relocate_management_focus(&vec![(source_key(&new).unwrap(),source_key(&Source::Local(target.join("renamed.jpg"))).unwrap())]);assert_eq!(app.selected,Some(1));assert_eq!(app.extra.selection,BTreeSet::from([1]));
    }

    #[test]
    fn open_container_rename_updates_source_even_after_old_path_disappears(){
        let root=tempfile::tempdir().unwrap();let old=root.path().join("images.zip");std::fs::write(&old,b"archive-identity-fixture").unwrap();let mut app=super::super::browser_ui::tests::viewer(vec![Entry{name:"inside/a.jpg".into(),size:3,directory:false,archive:false,modified:None}]);app.source=Some(old.clone());app.extra.write_source=Some(old.clone());let old_key=source_key(&Source::Local(old.clone())).unwrap();let new=fileops::rename_local(&old,"renamed.zip").unwrap();let expected=new.canonicalize().unwrap();app.relocate_management_focus(&vec![(old_key,source_key(&Source::Local(new)).unwrap())]);assert_eq!(app.source,Some(expected.clone()));assert_eq!(app.extra.write_source,Some(expected));assert_eq!(app.entries[0].name,"inside/a.jpg");assert_eq!(app.selected,Some(0));
    }

    #[test]
    fn partial_moves_migrate_only_completed_sources_and_do_not_duplicate_targets() {
        let root = tempfile::tempdir().unwrap();
        let destination = root.path().join("target");
        std::fs::create_dir(&destination).unwrap();
        let local = root.path().join("first.png");
        std::fs::write(&local, b"image").unwrap();
        let sources = vec![
            Source::Local(local.clone()),
            Source::Archive {
                path: root.path().join("book.zip"),
                entry: "pages/second.png".into(),
                password: Some("never-save-this".into()),
            },
            Source::Archive {
                path: root.path().join("other.zip"),
                entry: "other/first.png".into(),
                password: None,
            },
        ];
        let keys = completed_key_moves(&sources, &destination, &[destination.join("first.png")]);
        assert_eq!(keys.len(), 1);
        assert_eq!(keys[0].0, source_key(&Source::Local(local)).unwrap());
        assert_eq!(
            decode_catalog_key(&keys[0].1)
                .unwrap()
                .0
                .file_name()
                .unwrap(),
            "first.png"
        );
        assert!(!keys[0].0.contains("never-save-this"));
        assert!(completed_key_moves(&sources, &destination, &[]).is_empty());
    }

    #[test]
    fn global_basket_paginates_and_archive_keys_keep_the_entry_without_password() {
        let root = tempfile::tempdir().unwrap();
        let mut catalog = Catalog::open(&root.path().join("catalog.sqlite")).unwrap();
        let keys = (0..205)
            .map(|index| {
                source_key(&Source::Local(
                    root.path().join(format!("image-{index:04}.png")),
                ))
                .unwrap()
            })
            .collect::<Vec<_>>();
        catalog.update_many(&keys, Change::Basket(true)).unwrap();
        let first = catalog.list(Filter::Basket, CATALOG_PAGE + 1, 0).unwrap();
        let second = catalog
            .list(Filter::Basket, CATALOG_PAGE + 1, CATALOG_PAGE)
            .unwrap();
        assert_eq!(first.len(), 101);
        assert_eq!(second.len(), 101);
        assert_ne!(first[0].0, second[0].0);
        let third = catalog.list(Filter::Basket, CATALOG_PAGE + 1, 200).unwrap();
        assert_eq!(third.len(), 5);
        let all = first
            .iter()
            .take(CATALOG_PAGE)
            .chain(second.iter().take(CATALOG_PAGE))
            .chain(&third)
            .map(|(key, _)| key)
            .collect::<HashSet<_>>();
        assert_eq!(all.len(), 205);
        let source = Source::Archive {
            path: root.path().join("book.zip"),
            entry: "pages/02.png".into(),
            password: Some("never-save-this".into()),
        };
        let key = source_key(&source).unwrap();
        assert!(!key.contains("never-save-this"));
        assert_eq!(
            decode_catalog_key(&key).unwrap().1.as_deref(),
            Some("pages/02.png")
        );
    }

    #[test]
    fn background_filter_finds_later_pages_and_obeys_cancellation() {
        let root = tempfile::tempdir().unwrap();
        let database = root.path().join("catalog.sqlite");
        let mut catalog = Catalog::open(&database).unwrap();
        let keys = (0..1100)
            .map(|index| format!("image-{index:04}"))
            .collect::<Vec<_>>();
        catalog.update_many(&keys, Change::Rating(4)).unwrap();
        let request = || CatalogFilterRequest {
            candidates: HashMap::from([(keys[1050].clone(), 7), ("absent".into(), 8)]),
            mode: 6,
            selection: false,
        };
        assert_eq!(
            filter_catalog(&database, request(), &AtomicBool::new(false)).unwrap(),
            vec![7]
        );
        assert!(filter_catalog(&database, request(), &AtomicBool::new(true))
            .unwrap()
            .is_empty());
    }

    #[test]
    fn writer_panic_and_disconnection_finish_as_errors_instead_of_staying_busy() {
        let (tx, rx) = mpsc::sync_channel(1);
        assert!(poll_write_result(&rx).is_none());
        drop(tx);
        assert!(poll_write_result(&rx).unwrap().is_err());
        let (tx, rx) = mpsc::sync_channel(1);
        let writer = std::thread::spawn(move || {
            let result = execute_write(Box::new(|| panic!("fixture writer panic")));
            tx.send(result).unwrap();
        });
        writer.join().unwrap();
        assert!(poll_write_result(&rx)
            .unwrap()
            .unwrap_err()
            .contains("文件操作线程意外结束"));
    }

    #[test]
    fn grouping_changes_group_direction_and_keeps_order_within_each_group() {
        let entries = ["a.PNG", "b.jpg", "c.png", "d.JPG"].map(|name| Entry {
            name: name.into(),
            size: 0,
            directory: false,
            archive: false,
            modified:None,
        });
        let original = vec![3, 0, 1, 2];
        let mut ascending = original.clone();
        sort_groups(&mut ascending, &entries, false);
        assert_eq!(ascending, vec![3, 1, 0, 2]);
        let mut descending = original;
        sort_groups(&mut descending, &entries, true);
        assert_eq!(descending, vec![0, 2, 3, 1]);
    }
}

fn primary_text(ui: &mut egui::Ui, text: &mut String, focus: bool) {
    let response = ui.add(egui::TextEdit::singleline(text).id_source("command-primary"));
    if focus {
        response.request_focus();
    }
}

use super::*;
use crate::metadata::{Destination, Metadata, Values};

#[derive(Default)]
pub(super) struct State {
    key: Option<String>,
    requested: bool,
    data: Option<Metadata>,
    error: Option<String>,
    values: Values,
    keywords: String,
    iptc_keywords: String,
    destination: Destination,
    focus: Option<u8>,
    dirty: bool,
    saved: Option<Receiver<(String, Result<(), String>)>>,
    header_target: u8,
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn changing_selection_cannot_write_old_metadata_into_the_new_picture() {
        let directory=tempfile::tempdir().unwrap();
        let mut viewer=super::super::browser_ui::tests::viewer(vec![Entry{name:"a.jpg".into(),size:1,directory:false,archive:false,modified:None},Entry{name:"b.jpg".into(),size:1,directory:false,archive:false,modified:None}]);
        viewer.source=Some(directory.path().to_owned());viewer.selected=Some(0);
        viewer.extra.metadata_ui.key=viewer.item_key(0);viewer.extra.metadata_ui.values.title="belongs to A".into();viewer.extra.metadata_ui.dirty=true;
        viewer.selected=Some(1);viewer.save_source_metadata();
        assert!(viewer.error.as_deref().is_some_and(|e|e.contains("当前图像已改变")));
        assert!(viewer.extra.pending_write.is_none());assert!(viewer.extra.metadata_ui.saved.is_none());
    }
}

impl Viewer {
    pub(super) fn open_source_metadata(&mut self, focus: Option<u8>) {
        self.properties = true;
        self.extra.property_tab = 1;
        self.extra.metadata_ui.focus = focus;
    }

    pub(super) fn accept_metadata(&mut self, key: String, metadata: Metadata) {
        let state = &mut self.extra.metadata_ui;
        if state.key.as_ref() != Some(&key) {
            return;
        }
        state.keywords = metadata.values.keywords.join("\n");
        state.iptc_keywords = metadata.values.iptc_keywords.join("\n");
        state.values = metadata.values.clone();
        state.data = Some(metadata);
        state.error = None;
        state.dirty = false;
    }

    pub(super) fn metadata_error(&mut self, key: String, error: String) {
        if self.extra.metadata_ui.key.as_ref() == Some(&key) {
            self.extra.metadata_ui.error = Some(error);
        }
    }

    pub(super) fn schedule_metadata(&mut self) {
        if let Some((key, result)) = self
            .extra
            .metadata_ui
            .saved
            .as_ref()
            .and_then(|r| r.try_recv().ok())
        {
            self.extra.metadata_ui.saved = None;
            if self.extra.metadata_ui.key.as_ref() == Some(&key) && result.is_ok() {
                self.extra.metadata_ui.requested = false;
                self.extra.metadata_ui.data = None;
                self.extra.metadata_ui.dirty = false;
            }
        }
        if !self.properties && !self.extra.header_visible {
            return;
        }
        let index = if self.extra.compare > 0 {
            self.extra.comparison.active_index()
        } else {
            self.selected
        };
        let Some(index) = index else {
            return;
        };
        let Some(key) = self.item_key(index) else {
            return;
        };
        if self.extra.metadata_ui.key.as_ref() != Some(&key) {
            let focus = self.extra.metadata_ui.focus;
            self.extra.metadata_ui = State {
                key: Some(key.clone()),
                focus,
                ..Default::default()
            };
        }
        if self.extra.metadata_ui.requested
            || self.deferred_image.is_some()
            || self.running.is_some()
            || self.pending.is_some()
            || self.extra.writing.is_some()
            || self.extra.pending_write.is_some()
        {
            return;
        }
        if let Some(source) = self.make_load(index, self.full_raw) {
            self.extra.metadata_ui.requested = true;
            self.queue(
                Request::MetadataRead {
                    source: Box::new(source),
                },
                Operation::Metadata { key },
            );
        }
    }

    pub(super) fn draw_source_metadata(&mut self, ctx: &egui::Context) {
        if !self.properties {
            return;
        }
        let mut open = true;
        let path = self.current_path().unwrap_or_default();
        let local = self.source.as_ref().filter(|s| s.is_dir()).and_then(|s| {
            self.selected
                .and_then(|i| self.entries.get(i))
                .map(|e| s.join(&e.name))
        });
        let busy = self.extra.writing.is_some()
            || self.extra.pending_write.is_some()
            || self.extra.metadata_ui.saved.is_some();
        let mut save = false;
        let mut reload = false;
        let mut copy_catalog = false;
        egui::Window::new("图像属性").id(egui::Id::new("source-metadata")).open(&mut open).default_width(630.0).show(ctx, |ui| {
            ui.horizontal(|ui| {
                for (tab, label) in [(0,"图像"),(1,"源图元数据"),(2,"全部 EXIF / IPTC / XMP")] {
                    ui.selectable_value(&mut self.extra.property_tab,tab,label);
                }
            });
            ui.label(&path);
            if let Some(entry)=self.selected.and_then(|i|self.entries.get(i)).filter(|e|e.directory||e.archive){
                ui.label(if entry.directory{"文件夹"}else{"压缩包"});
                if let Some(meta)=local.as_ref().and_then(|p|std::fs::metadata(p).ok()){
                    if meta.is_file(){ui.label(format!("{} 字节",meta.len()));}
                    if let Ok(time)=meta.modified(){ui.label(format!("修改时间：{}",crate::fileops::format_local_datetime(time)));}
                    ui.label(if meta.permissions().readonly(){"只读"}else{"可写"});
                }
                return;
            }
            if self.extra.property_tab == 0 {
                if let Some(p) = &self.current { let(w,h)=self.extra.comparison.active_dimensions().unwrap_or((p.width,p.height));ui.label(format!("{} × {} 像素；RGBA {:.1} MiB",w,h,p.rgba.len() as f64/1048576.0)); ui.label(&p.note); }
                ui.separator();
                ui.label(format!("目录库标题：{}",self.extra.record.title));
                ui.label(format!("目录库类别：{}；评级：{}",self.extra.record.category,self.extra.record.rating));
                ui.small("库内评级、标题与分类由对应快捷键单独保存。点击源图元数据页后显式写入照片。");
                return;
            }
            let state = &mut self.extra.metadata_ui;
            if let Some(error)=&state.error { ui.colored_label(Color32::LIGHT_RED,error); }
            if state.data.is_none() { ui.label(if state.error.is_some(){"未能读取元数据"}else{"等待读取源图元数据…"}); if ui.button("重新读取").clicked(){reload=true;} return; }
            if self.extra.property_tab == 2 {
                let fields=&state.data.as_ref().unwrap().fields;
                egui::ScrollArea::vertical().max_height(500.0).show_rows(ui,22.0,fields.len(), |ui,range| {
                    for(tag,value)in fields.iter().skip(range.start).take(range.len()){
                        ui.horizontal(|ui|{
                            ui.add_sized([190.0,20.0],egui::Label::new(tag).truncate());
                            let text=match value{serde_json::Value::String(v)=>std::borrow::Cow::Borrowed(v.as_str()),_=>std::borrow::Cow::Owned(value.to_string())};
                            let preview=text.chars().take(512).map(|c|if matches!(c,'\r'|'\n'){'↵'}else{c}).collect::<String>();
                            ui.add_sized([300.0,20.0],egui::Label::new(preview).truncate());
                            if ui.small_button("复制").clicked(){ctx.copy_text(text.into_owned());}
                        });
                    }
                }); return;
            }
            if let Some(sidecar)=state.data.as_ref().and_then(|d|d.sidecar.as_ref()){ui.small(format!("已合并 sidecar：{}",sidecar.display()));}
            if local.is_none(){ui.small("压缩包内元数据为只读。请先提取图片，再写入源图或 XMP。");}
            let writable=local.is_some()&&!busy;
            let text_bytes=[&state.values.title,&state.values.description,&state.values.creator,&state.values.copyright,&state.values.datetime_original,&state.keywords,&state.iptc_keywords].iter().map(|s|s.len()).sum::<usize>();
            if text_bytes>128*1024{ui.label("文字字段超过128 KiB编辑预算。全部标签页可查看和复制完整值；为避免截断数据，此文件保持只读。");return;}
            ui.add_enabled_ui(writable, |ui| {
                egui::ScrollArea::vertical().max_height(440.0).show(ui, |ui| {
                    let focus=state.focus.take();
                    let mut field=|ui:&mut egui::Ui,id:u8,label:&str,text:&mut String,multiline:bool|{
                        ui.label(label);
                        let edit=if multiline{egui::TextEdit::multiline(text).desired_rows(2)}else{egui::TextEdit::singleline(text)};
                        let response=ui.add(edit.desired_width(f32::INFINITY));
                        if focus==Some(id){response.request_focus();}
                        state.dirty|=response.changed();
                    };
                    field(ui,0,"标题 / ACDSee Caption",&mut state.values.title,false);
                    field(ui,1,"说明 / Notes",&mut state.values.description,true);
                    field(ui,2,"ACDSee 关键词（每行一个）",&mut state.keywords,true);
                    field(ui,3,"IPTC / 标准 XMP 关键词（每行一个）",&mut state.iptc_keywords,true);
                    field(ui,4,"作者 / Creator",&mut state.values.creator,false);
                    field(ui,5,"版权 / Copyright",&mut state.values.copyright,false);
                    field(ui,6,"拍摄时间（YYYY:MM:DD HH:MM:SS）",&mut state.values.datetime_original,false);
                    ui.horizontal(|ui| {
                        ui.label("评级");let mut rating=state.values.rating.map(i32::from).unwrap_or(-1);
                        if ui.add(egui::DragValue::new(&mut rating).range(-1..=5)).changed(){state.values.rating=(rating>=0).then_some(rating as u8);state.dirty=true;}
                        ui.small("-1 清空");ui.label("颜色标签");let label=state.values.label.get_or_insert_default();
                        state.dirty|=ui.text_edit_singleline(label).changed();
                    });
                });
                ui.horizontal(|ui| {
                    ui.label("写入位置");ui.selectable_value(&mut state.destination,Destination::Auto,"自动（RAW → XMP）");
                    ui.selectable_value(&mut state.destination,Destination::Sidecar,"XMP sidecar");
                });
                ui.horizontal(|ui| {
                    save=ui.add_enabled(state.dirty,egui::Button::new("保存到源图 / XMP")).clicked();
                    copy_catalog=ui.button("从当前目录库填入").clicked();
                    reload=ui.button("放弃编辑并重读").clicked();
                });
                ui.small("首次覆盖前备份到同目录 .rustview-originals；RAW 文件原始数据不改写。保存会先验证写入结果，再替换文件。");
            });
        });
        self.properties = open;
        if reload {
            self.extra.metadata_ui.requested = false;
            self.extra.metadata_ui.data = None;
            self.extra.metadata_ui.error = None;
        }
        if copy_catalog {
            let record = &self.extra.record;
            let state = &mut self.extra.metadata_ui;
            state.values.title = record.title.clone();
            state.values.description = record.description.clone();
            state.values.rating = Some(record.rating);
            state.values.label = Some(
                ["", "Red", "Yellow", "Green", "Blue", "Purple"][usize::from(record.label.min(5))]
                    .into(),
            );
            state.keywords = record.keywords.replace(';', "\n");
            state.iptc_keywords = record.iptc_keywords.replace(';', "\n");
            state.dirty = true;
        }
        if save {
            self.save_source_metadata();
        }
    }

    fn save_source_metadata(&mut self) {
        if self.extra.writing.is_some()||self.extra.pending_write.is_some(){return;}
        let current_key=self.selected.and_then(|i|self.item_key(i));
        if self.extra.metadata_ui.key!=current_key {self.error=Some("当前图像已改变，请重新读取元数据后再保存".into());return;}
        let state = &mut self.extra.metadata_ui;
        let Some(key) = state.key.clone() else {
            return;
        };
        let Ok((path,None))=serde_json::from_str::<(PathBuf,Option<String>)>(&key) else {self.error=Some("压缩包内的元数据为只读".into());return;};
        let mut values = state.values.clone();
        values.keywords = state
            .keywords
            .lines()
            .map(str::trim)
            .filter(|v| !v.is_empty())
            .map(str::to_owned)
            .collect();
        values.iptc_keywords = state
            .iptc_keywords
            .lines()
            .map(str::trim)
            .filter(|v| !v.is_empty())
            .map(str::to_owned)
            .collect();
        if values.label.as_deref() == Some("") {
            values.label = None;
        }
        let destination = state.destination;
        let (tx, rx) = mpsc::sync_channel(1);
        state.saved = Some(rx);
        self.write_job_no_refresh(Box::new(move || {
            let result = worker::execute(
                Request::MetadataWrite {
                    path,
                    values,
                    destination,
                },
                Arc::new(AtomicBool::new(false)),
            )
            .and_then(|output| {
                if let Some(error) = output.reply.error {
                    return Err(error);
                }
                let result = output
                    .reply
                    .metadata_write
                    .ok_or("工作进程未返回元数据写入结果")?;
                Ok(format!(
                    "已保存元数据：{}{}",
                    result.destination.display(),
                    result
                        .backup
                        .map(|p| format!("；原件备份：{}", p.display()))
                        .unwrap_or_default()
                ))
            });
            let _ = tx.send((key, result.as_ref().map(|_| ()).map_err(Clone::clone)));
            result
        }));
    }

    pub(super) fn source_fields(&self) -> crate::printing::Fields {
        let path = self.current_path().map(PathBuf::from).unwrap_or_default();
        let dimensions=if self.current_key.map(|k|k.0)==self.selected{self.extra.comparison.active_dimensions().or_else(||self.current.as_ref().map(|i|(i.width,i.height)))}else{None};
        let mut fields = crate::printing::Fields::from_path(
            &path,
            dimensions.map(|i| i.0).unwrap_or(0),
            dimensions.map(|i| i.1).unwrap_or(0),
            self.extra.page_index + 1,
            self.extra.page_total.max(1),
        );
        fields.name = self
            .selected
            .and_then(|i| self.entries.get(i))
            .map(|e| e.name.clone())
            .unwrap_or_default();
        if let Some(metadata) = self.extra.metadata_ui.data.as_ref().filter(|_|self.extra.metadata_ui.key==self.selected.and_then(|i|self.item_key(i))) {
            fields.apply_metadata(metadata);
        }
        fields
    }
    pub(super) fn set_header_field_target(&mut self, footer: bool) {
        self.extra.metadata_ui.header_target = u8::from(footer);
    }
    pub(super) fn insert_header_field(&mut self, id: u16) {
        let Some(token) = crate::printing::field_token(id) else {
            return;
        };
        if self.extra.print_ui.insert_field(token) {
            return;
        }
        let target = if self.extra.metadata_ui.header_target == 1 {
            &mut self.extra.footer
        } else {
            &mut self.extra.header
        };
        if target.len() + token.len() <= 16384 {
            target.push_str(token);
            self.extra.dialog = Some(2303);
        }
    }
}

use super::*;
use crate::printing::{self, Fields, ImageFit, PrintSettings, TextAlignment};

struct Item {
    request: Request,
    metadata: Request,
    path: PathBuf,
    name: String,
}
struct Preview {
    cell: u32,
    width: u32,
    height: u32,
    rgba: Vec<u8>,
    fields: PreviewText,
}
#[derive(Default)]
struct PreviewText {
    width: u32,
    height: u32,
    header: String,
    footer: String,
    caption: String,
}
impl PreviewText {
    fn from_fields(settings: &PrintSettings, fields: &Fields) -> Self {
        Self {
            width: fields.width,
            height: fields.height,
            header: printing::expand_fields(&settings.header, fields),
            footer: printing::expand_fields(&settings.footer, fields),
            caption: printing::expand_fields(&settings.caption, fields),
        }
    }
}
pub(super) struct State {
    pub(super) open: bool,
    settings: PrintSettings,
    items: Vec<Item>,
    page: u32,
    printer: Option<printing::PrintJob>,
    preview: HashMap<u32, (TextureHandle, PreviewText)>,
    receiver: Option<Receiver<Result<Preview, String>>>,
    cancel: Option<Arc<AtomicBool>>,
    loaded_page: u32,
    error: Option<String>,
    text_target: u8,
    metadata_preview: bool,
}
impl Default for State {
    fn default() -> Self {
        Self {
            open: false,
            settings: PrintSettings::default(),
            items: Vec::new(),
            page: 1,
            printer: None,
            preview: HashMap::new(),
            receiver: None,
            cancel: None,
            loaded_page: 0,
            error: None,
            text_target: 0,
            metadata_preview: false,
        }
    }
}
impl Drop for State {
    fn drop(&mut self) {
        if let Some(cancel) = &self.cancel {
            cancel.store(true, Ordering::Relaxed);
        }
    }
}
impl State {
    pub(super) fn insert_field(&mut self, token: &str) -> bool {
        if !self.open {
            return false;
        }
        let target = match self.text_target {
            1 => &mut self.settings.footer,
            2 => &mut self.settings.caption,
            _ => &mut self.settings.header,
        };
        if target.len() + token.len() <= 16384 {
            target.push_str(token);
        }
        true
    }
}

impl Viewer {
    pub(super) fn print_image(&mut self, all: bool) {
        if self.extra.writing.is_some() || self.extra.pending_write.is_some() {
            return;
        }
        let indices = if all {
            if self.extra.selection.is_empty() {
                self.extra.visible.clone()
            } else {
                self.selected_indices()
            }
        } else {
            self.selected_indices()
        };
        let mut items = Vec::new();
        let mut bytes = 0usize;
        for index in indices {
            let (Some(mut request), Some(metadata), Some(entry)) = (
                self.make_view_load(index, self.full_raw),
                self.make_load(index, self.full_raw),
                self.entries.get(index),
            ) else {
                continue;
            };
            if self.current.is_some() && self.current_key.is_some_and(|key| key.0 == index)
                && self.selected == Some(index) {
                let mut settings = self.extra.workspace.export.clone();
                settings.format = crate::precision::OutputFormat::Png;
                settings.depth = 8;
                settings.animation = crate::precision::AnimationMode::CurrentFrame;
                request = Request::Render {
                    source: Box::new(request),
                    edits: self.document_edits(),
                    settings,
                    raw: self.extra.workspace.raw.clone(),
                    save: None,
                };
            }
            let Some(source) = &self.source else {
                continue;
            };
            let path = if source.is_dir() {
                source.join(&entry.name)
            } else {
                PathBuf::from(&entry.name)
            };
            bytes = bytes.saturating_add(
                serde_json::to_vec(&request)
                    .map(|v| v.len())
                    .unwrap_or(usize::MAX),
            );
            bytes = bytes.saturating_add(
                serde_json::to_vec(&metadata)
                    .map(|v| v.len())
                    .unwrap_or(usize::MAX),
            );
            if bytes > 32 * 1024 * 1024 {
                self.error = Some("打印任务路径超过32 MiB，请分批选择".into());
                return;
            }
            items.push(Item {
                request,
                metadata,
                path,
                name: entry.name.clone(),
            });
        }
        if items.is_empty() {
            return;
        }
        let state = &mut self.extra.print_ui;
        if let Some(cancel) = state.cancel.take() {
            cancel.store(true, Ordering::Relaxed);
        }
        state.items = items;
        state.open = true;
        state.page = 1;
        state.loaded_page = 0;
        state.preview.clear();
        state.receiver = None;
        state.error = None;
        state.printer = None;
    }

    pub(super) fn draw_print_dialog(&mut self, ctx: &egui::Context) {
        if !self.extra.print_ui.open {
            return;
        }
        let state = &mut self.extra.print_ui;
        if let Some(receiver) = &state.receiver {
            while let Ok(result) = receiver.try_recv() {
                match result {
                    Ok(p) => {
                        let texture = ctx.load_texture(
                            format!("print-{}-{}", state.loaded_page, p.cell),
                            egui::ColorImage::from_rgba_unmultiplied(
                                [p.width as usize, p.height as usize],
                                &p.rgba,
                            ),
                            egui::TextureOptions::LINEAR,
                        );
                        state.preview.insert(p.cell, (texture, p.fields));
                    }
                    Err(e) => state.error = Some(e),
                }
            }
            ctx.request_repaint_after(std::time::Duration::from_millis(50));
        }
        let mut open = true;
        let mut select_printer = false;
        let mut print = false;
        let before = (
            state.settings.rows,
            state.settings.columns,
            state.settings.copies_per_image,
            state.page,
        );
        let before_text = [
            state.settings.header.clone(),
            state.settings.footer.clone(),
            state.settings.caption.clone(),
        ];
        egui::Window::new("图片打印与接触表").open(&mut open).default_size(egui::vec2(940.0,720.0)).show(ctx,|ui|{
            ui.columns(2,|columns|{
                let ui=&mut columns[0];
                ui.label(format!("{} 张图片（逐张解码，不同时加载原图）",state.items.len()));
                ui.horizontal(|ui|{ui.label("行");ui.add(egui::DragValue::new(&mut state.settings.rows).range(1..=20));ui.label("列");ui.add(egui::DragValue::new(&mut state.settings.columns).range(1..=20));ui.label("每图份数");ui.add(egui::DragValue::new(&mut state.settings.copies_per_image).range(1..=1000));});
                ui.horizontal(|ui|{ui.label("边距 mm");ui.add(egui::DragValue::new(&mut state.settings.margin_mm).range(0.0..=100.0));ui.label("间距 mm");ui.add(egui::DragValue::new(&mut state.settings.gap_mm).range(0.0..=100.0));});
                ui.horizontal(|ui|{ui.selectable_value(&mut state.settings.fit,ImageFit::Fit,"缩小适应，保留整图");ui.selectable_value(&mut state.settings.fit,ImageFit::Fill,"裁切填满");});
                ui.horizontal(|ui|{let mut fixed=state.settings.image_width_mm.is_some();if ui.checkbox(&mut fixed,"指定图宽 mm").changed(){state.settings.image_width_mm=fixed.then_some(100.0);}if let Some(width)=&mut state.settings.image_width_mm{ui.add(egui::DragValue::new(width).range(1.0..=2000.0));}});
                ui.horizontal(|ui|{let mut fixed=state.settings.image_height_mm.is_some();if ui.checkbox(&mut fixed,"指定图高 mm").changed(){state.settings.image_height_mm=fixed.then_some(100.0);}if let Some(height)=&mut state.settings.image_height_mm{ui.add(egui::DragValue::new(height).range(1.0..=2000.0));}});
                for(index,(label,text))in[("页眉",&mut state.settings.header),("页脚",&mut state.settings.footer),("图片标题",&mut state.settings.caption)].into_iter().enumerate(){ui.label(label);if ui.add(egui::TextEdit::multiline(text).desired_rows(2).desired_width(f32::INFINITY)).has_focus(){state.text_target=index as u8;}}
                ui.small("可用字段：{name} {path} {width} {height} {date} {time} {title} {description} {page} {pages}，以及 {tag:EXIF字段名}。");
                ui.horizontal(|ui|{ui.label("字号 pt");ui.add(egui::DragValue::new(&mut state.settings.font_points).range(4.0..=120.0));ui.label("图片标题行数");ui.add(egui::DragValue::new(&mut state.settings.caption_lines).range(1..=20));});
                ui.horizontal(|ui|{ui.selectable_value(&mut state.settings.alignment,TextAlignment::Left,"左对齐");ui.selectable_value(&mut state.settings.alignment,TextAlignment::Center,"居中");ui.selectable_value(&mut state.settings.alignment,TextAlignment::Right,"右对齐");});
                let pages=printing::page_count(state.items.len(),&state.settings);
                match &pages{Ok(count)=>{state.page=state.page.clamp(1,*count);ui.horizontal(|ui|{ui.label("预览页");ui.add(egui::DragValue::new(&mut state.page).range(1..=*count));ui.label(format!("/ {count}"));});},Err(e)=>{ui.colored_label(Color32::LIGHT_RED,e);}}
                select_printer=ui.button("选择打印机、纸张、方向和页码范围…").clicked();
                if let Some(printer)=&state.printer{let(w,h,dx,dy)=printer.page_metrics();let(from,to)=printer.page_range();ui.small(format!("可打印区域 {:.1} × {:.1} mm；页码 {from}–{to}",w as f32*25.4/dx,h as f32*25.4/dy));}else{ui.small("当前为 A4 比例预览；选择打印机后按实际可打印区域排版。");}
                print=ui.add_enabled(pages.is_ok(),egui::Button::new("提交打印")).clicked();
                if let Some(error)=&state.error{ui.colored_label(Color32::LIGHT_RED,error);}
                draw_preview(&mut columns[1],state);
            });
        });
        self.extra.print_ui.open = open;
        if !open {
            let state = &mut self.extra.print_ui;
            if let Some(c) = state.cancel.take() {
                c.store(true, Ordering::Relaxed);
            }
            state.receiver = None;
            state.preview.clear();
            state.printer = None;
            return;
        }
        let state = &mut self.extra.print_ui;
        let after = (
            state.settings.rows,
            state.settings.columns,
            state.settings.copies_per_image,
            state.page,
        );
        let text_changed = before_text[0] != state.settings.header
            || before_text[1] != state.settings.footer
            || before_text[2] != state.settings.caption;
        if before != after
            || text_changed
            || (needs_metadata(&state.settings) && !state.metadata_preview)
        {
            state.loaded_page = 0;
            state.preview.clear();
            state.receiver = None;
            if let Some(cancel) = state.cancel.take() {
                cancel.store(true, Ordering::Relaxed);
            }
            if (before.0, before.1, before.2) != (after.0, after.1, after.2) {
                state.printer = None;
            }
        }
        if select_printer {
            match printing::page_count(state.items.len(), &state.settings)
                .and_then(|pages| printing::choose_printer_for_pages(self.extra.hwnd as _, pages))
            {
                Ok(p) => state.printer = p,
                Err(e) => state.error = Some(e),
            }
        }
        if state.loaded_page != state.page
            && self.extra.writing.is_none()
            && self.extra.pending_write.is_none()
        {
            let job = start_preview(state);
            self.write_job_no_refresh(job);
        }
        if print {
            self.submit_print();
        }
    }

    fn submit_print(&mut self) {
        if self.extra.writing.is_some() || self.extra.pending_write.is_some() {
            self.extra.print_ui.error = Some("请等预览生成完成后提交打印".into());
            return;
        }
        let state = &mut self.extra.print_ui;
        let pages = match printing::page_count(state.items.len(), &state.settings) {
            Ok(p) => p,
            Err(e) => {
                state.error = Some(e);
                return;
            }
        };
        if state.printer.is_none() {
            match printing::choose_printer_for_pages(self.extra.hwnd as _, pages) {
                Ok(Some(p)) => state.printer = Some(p),
                Ok(None) => return,
                Err(e) => {
                    state.error = Some(e);
                    return;
                }
            }
        }
        let mut printer = state.printer.take().unwrap();
        if let Err(e) = printer.page_layout(&state.settings) {
            state.error = Some(e);
            return;
        }
        let settings = state.settings.clone();
        let items = std::mem::take(&mut state.items);
        if let Some(c) = state.cancel.take() {
            c.store(true, Ordering::Relaxed);
        }
        state.receiver = None;
        state.preview.clear();
        state.open = false;
        self.write_job_no_refresh(Box::new(move || {
            let (from, to) = printer.page_range();
            let to = to.min(pages);
            if from > to {
                return Err("选定的打印页码超出当前文档".into());
            }
            printer.start_document("RustView 图片打印")?;
            let mut count = 0;
            for page in from..=to {
                printer.start_page()?;
                let mut page_fields = Fields {
                    page,
                    pages,
                    ..Default::default()
                };
                let mut header = false;
                for cell in 0..settings.rows * settings.columns {
                    let Some(index) =
                        printing::slot_image_index(page, cell, items.len(), &settings)
                    else {
                        break;
                    };
                    let item = &items[index];
                    let output =
                        worker::execute(item.request.clone(), Arc::new(AtomicBool::new(false)))?;
                    if let Some(e) = output.reply.error {
                        return Err(e);
                    }
                    let mut fields = Fields::from_path(
                        &item.path,
                        output.reply.width,
                        output.reply.height,
                        page,
                        pages,
                    );
                    fields.name = item.name.clone();
                    if needs_metadata(&settings) {
                        let meta = worker::execute(
                            Request::MetadataRead {
                                source: Box::new(item.metadata.clone()),
                            },
                            Arc::new(AtomicBool::new(false)),
                        )?;
                        if let Some(e) = meta.reply.error {
                            return Err(e);
                        }
                        if let Some(data) = meta.reply.metadata {
                            fields.apply_metadata(&data);
                        }
                    }
                    if !header {
                        page_fields = fields.clone();
                        printer.draw_page_text(&settings, &page_fields)?;
                        header = true;
                    }
                    printer.draw_cell_rgba(
                        output.reply.width,
                        output.reply.height,
                        &output.rgba,
                        cell,
                        &settings,
                        &fields,
                    )?;
                }
                if !header {
                    printer.draw_page_text(&settings, &page_fields)?;
                }
                printer.end_page()?;
                count += 1;
            }
            printer.finish()?;
            Ok(format!("已向打印机提交 {count} 页"))
        }));
    }
}

fn needs_metadata(settings: &PrintSettings) -> bool {
    [&settings.header, &settings.footer, &settings.caption]
        .iter()
        .any(|s| {
            ["{title}", "{description}", "{date}", "{time}", "{tag:"]
                .iter()
                .any(|token| s.contains(token))
        })
}
fn start_preview(state: &mut State) -> Box<dyn FnOnce() -> Result<String, String> + Send> {
    if let Some(c) = state.cancel.take() {
        c.store(true, Ordering::Relaxed);
    }
    state.preview.clear();
    state.loaded_page = state.page;
    state.metadata_preview = needs_metadata(&state.settings);
    let (tx, rx) = mpsc::sync_channel(1);
    state.receiver = Some(rx);
    let cancel = Arc::new(AtomicBool::new(false));
    state.cancel = Some(cancel.clone());
    let settings = state.settings.clone();
    let page = state.page;
    let pages = printing::page_count(state.items.len(), &settings).unwrap_or(1);
    let cells = settings.rows * settings.columns;
    let side = ((16 * 1024 * 1024 / (cells.max(1) as usize * 4)) as f64)
        .sqrt()
        .floor()
        .clamp(32.0, 384.0) as u32;
    let items = (0..cells)
        .filter_map(|cell| {
            let i = printing::slot_image_index(page, cell, state.items.len(), &settings)?;
            let item = &state.items[i];
            Some((
                cell,
                item.request.clone(),
                item.metadata.clone(),
                item.path.clone(),
                item.name.clone(),
            ))
        })
        .collect::<Vec<_>>();
    Box::new(move || {
        for (cell, request, metadata, path, name) in items {
            if cancel.load(Ordering::Relaxed) {
                break;
            }
            let result = worker::execute(
                Request::Preview {
                    source: Box::new(request),
                    max_side: side,
                    region: None,
                },
                cancel.clone(),
            )
            .and_then(|o| {
                if let Some(e) = o.reply.error {
                    return Err(e);
                }
                let mut fields = Fields::from_path(
                    &path,
                    o.reply.source_width.max(o.reply.width),
                    o.reply.source_height.max(o.reply.height),
                    page,
                    pages,
                );
                fields.name = name;
                if needs_metadata(&settings) {
                    let meta = worker::execute(
                        Request::MetadataRead {
                            source: Box::new(metadata),
                        },
                        cancel.clone(),
                    )?;
                    if let Some(e) = meta.reply.error {
                        return Err(e);
                    }
                    if let Some(data) = meta.reply.metadata {
                        fields.apply_metadata(&data);
                    }
                }
                Ok(Preview {
                    cell,
                    width: o.reply.width,
                    height: o.reply.height,
                    rgba: o.rgba,
                    fields: PreviewText::from_fields(&settings, &fields),
                })
            });
            if tx.send(result).is_err() {
                break;
            }
        }
        Ok("打印预览已更新".into())
    })
}

fn draw_preview(ui: &mut egui::Ui, state: &State) {
    let (w, h, dx, dy) = state
        .printer
        .as_ref()
        .map(|p| p.page_metrics())
        .unwrap_or((794, 1123, 96.0, 96.0));
    let layout = match printing::layout(w, h, dx, dy, &state.settings) {
        Ok(v) => v,
        Err(e) => {
            ui.colored_label(Color32::LIGHT_RED, e);
            return;
        }
    };
    let aspect = (w as f32 / dx) / (h as f32 / dy);
    let height = ui.available_height().min(620.0);
    let width = (height * aspect).min(ui.available_width());
    let size = egui::vec2(width, width / aspect);
    let (rect, _) = ui.allocate_exact_size(size, Sense::hover());
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, 0.0, Color32::WHITE);
    let convert = |r: printing::PixelRect| {
        Rect::from_min_max(
            rect.min
                + egui::vec2(
                    r.left as f32 / w as f32 * size.x,
                    r.top as f32 / h as f32 * size.y,
                ),
            rect.min
                + egui::vec2(
                    r.right as f32 / w as f32 * size.x,
                    r.bottom as f32 / h as f32 * size.y,
                ),
        )
    };
    let fallback = PreviewText::from_fields(
        &state.settings,
        &Fields {
            page: state.page,
            pages: printing::page_count(state.items.len(), &state.settings).unwrap_or(1),
            ..Default::default()
        },
    );
    let fields = state.preview.get(&0).map(|(_, f)| f).unwrap_or(&fallback);
    let draw_text = |p: &egui::Painter, r: printing::PixelRect, text: &str| {
        let r = convert(r);
        if r.height() <= 0.0 {
            return;
        }
        let align = match state.settings.alignment {
            TextAlignment::Left => egui::Align2::LEFT_TOP,
            TextAlignment::Center => egui::Align2::CENTER_TOP,
            TextAlignment::Right => egui::Align2::RIGHT_TOP,
        };
        let x = match state.settings.alignment {
            TextAlignment::Left => r.left(),
            TextAlignment::Center => r.center().x,
            TextAlignment::Right => r.right(),
        };
        let font = (state.settings.font_points * dy / 72.0 * size.y / h as f32).max(4.0);
        p.with_clip_rect(r).text(
            Pos2::new(x, r.top()),
            align,
            text,
            egui::FontId::proportional(font),
            Color32::BLACK,
        );
    };
    draw_text(&painter, layout.header, &fields.header);
    draw_text(&painter, layout.footer, &fields.footer);
    for (cell, layout) in layout.cells.iter().enumerate() {
        let target = convert(layout.image);
        painter.rect_filled(target, 0.0, Color32::from_gray(235));
        if let Some((texture, fields)) = state.preview.get(&(cell as u32)) {
            let mut target = target;
            if state.settings.image_width_mm.is_some() || state.settings.image_height_mm.is_some() {
                let fw = state.settings.image_width_mm.unwrap_or_else(|| {
                    state.settings.image_height_mm.unwrap() * fields.width as f32
                        / fields.height.max(1) as f32
                });
                let fh = state.settings.image_height_mm.unwrap_or_else(|| {
                    state.settings.image_width_mm.unwrap() * fields.height as f32
                        / fields.width.max(1) as f32
                });
                let physical = egui::vec2(
                    fw * dx / 25.4 / w as f32 * size.x,
                    fh * dy / 25.4 / h as f32 * size.y,
                );
                if physical.x <= target.width() && physical.y <= target.height() {
                    target = Rect::from_center_size(target.center(), physical);
                }
            }
            let image = texture.size_vec2();
            let factor = match state.settings.fit {
                ImageFit::Fit => (target.width() / image.x).min(target.height() / image.y),
                ImageFit::Fill => (target.width() / image.x).max(target.height() / image.y),
            };
            let image_rect = Rect::from_center_size(target.center(), image * factor);
            painter.with_clip_rect(target).image(
                texture.id(),
                image_rect,
                Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)),
                Color32::WHITE,
            );
            draw_text(&painter, layout.caption, &fields.caption);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn four_hundred_cells_keep_only_bounded_expanded_text_not_full_metadata() {
        let large = "图".repeat(8 * 1024 * 1024 / 3);
        let fields = Fields {
            width: 6000,
            height: 4000,
            page: 1,
            pages: 2,
            extra: std::collections::BTreeMap::from([("XMP-test:Large".into(), large)]),
            ..Default::default()
        };
        let settings = PrintSettings {
            rows: 20,
            columns: 20,
            header: "{page} {tag:XMP-test:Large}".into(),
            footer: "{width} {tag:XMP-test:Large}".into(),
            caption: "{tag:XMP-test:Large}".into(),
            ..Default::default()
        };
        let cells = (0..400)
            .map(|_| PreviewText::from_fields(&settings, &fields))
            .collect::<Vec<_>>();
        drop(fields);
        let retained = cells
            .iter()
            .map(|cell| {
                assert_eq!((cell.width, cell.height), (6000, 4000));
                for text in [&cell.header, &cell.footer, &cell.caption] {
                    assert!(text.len() <= printing::TEXT_BYTES);
                    assert!(text.capacity() <= printing::TEXT_BYTES);
                    assert!(text.contains('图'));
                }
                cell.header.capacity() + cell.footer.capacity() + cell.caption.capacity()
            })
            .sum::<usize>();
        assert!(retained <= 400 * 3 * printing::TEXT_BYTES);
        assert!(retained < 400 * 64 * 1024);
    }
}

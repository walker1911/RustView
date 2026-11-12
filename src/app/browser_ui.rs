use super::*;
use crate::catalog::Catalog;
use std::{
    cmp::Ordering as CmpOrdering,
    collections::{BTreeSet, HashSet},
    hash::{Hash, Hasher},
    time::{Duration, UNIX_EPOCH},
};

const INDEX_BYTES: usize = 64 * 1024 * 1024;
const TREE_ROWS: usize = 10_000;
const TREE_BYTES: usize = 8 * 1024 * 1024;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub(super) enum SortBy {
    #[default]
    Name,
    Type,
    Size,
    Date,
    Rating,
    Label,
}
impl SortBy {
    fn title(self) -> &'static str {
        match self {
            Self::Name => "名称",
            Self::Type => "类型",
            Self::Size => "大小",
            Self::Date => "修改日期",
            Self::Rating => "评级",
            Self::Label => "颜色标签",
        }
    }
    fn needs_catalog(self) -> bool {
        matches!(self, Self::Rating | Self::Label)
    }
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub(super) enum GroupBy {
    #[default]
    None,
    Name,
    Type,
    Size,
    Date,
    Rating,
    Label,
}
impl GroupBy {
    fn title(self) -> &'static str {
        match self {
            Self::None => "不分组",
            Self::Name => "名称首字",
            Self::Type => "类型",
            Self::Size => "大小",
            Self::Date => "修改日期",
            Self::Rating => "评级",
            Self::Label => "颜色标签",
        }
    }
    fn needs_catalog(self) -> bool {
        matches!(self, Self::Rating | Self::Label)
    }
}

struct MetadataIndex {
    generation: u64,
    values: HashMap<usize, (u8, u8)>,
    order: Vec<usize>,
    groups: Vec<(usize, String)>,
}
struct MetadataTask {
    receiver: Receiver<Result<MetadataIndex, String>>,
    cancel: Arc<AtomicBool>,
}
struct FolderNode {
    path: PathBuf,
    depth: usize,
    expanded: bool,
    loaded: bool,
}
struct FolderTask {
    path: PathBuf,
    receiver: Receiver<Result<Vec<PathBuf>, String>>,
}
enum DisplayRow {
    Header(usize),
    Items(usize, usize),
}

pub(super) struct State {
    pub folders: bool,
    pub preview: bool,
    pub tasks: bool,
    pub sort: SortBy,
    pub group: GroupBy,
    source: Option<PathBuf>,
    generation: u64,
    metadata: HashMap<usize, (u8, u8)>,
    metadata_task: Option<MetadataTask>,
    metadata_ready: bool,
    order_hash: u64,
    settings_hash: u64,
    dirty: bool,
    selection_anchor: Option<usize>,
    scroll_target: Option<usize>,
    last_columns: usize,
    folders_rows: Vec<FolderNode>,
    folder_task: Option<FolderTask>,
    folder_problem: Option<String>,
    thumb_failed: HashSet<usize>,
    groups: Vec<(usize, String)>,
    rows: Vec<DisplayRow>,
    row_columns: usize,
    row_hash: u64,
    thumbnail_in_flight: Option<(usize, u64)>,
}
impl Default for State {
    fn default() -> Self {
        Self {
            folders: true,
            preview: true,
            tasks: false,
            sort: SortBy::Name,
            group: GroupBy::None,
            source: None,
            generation: 0,
            metadata: HashMap::new(),
            metadata_task: None,
            metadata_ready: false,
            order_hash: 0,
            settings_hash: 0,
            dirty: true,
            selection_anchor: None,
            scroll_target: None,
            last_columns: 1,
            folders_rows: Vec::new(),
            folder_task: None,
            folder_problem: None,
            thumb_failed: HashSet::new(),
            groups: Vec::new(),
            rows: Vec::new(),
            row_columns: 0,
            row_hash: 0,
            thumbnail_in_flight: None,
        }
    }
}
impl State {
    pub fn invalidate(&mut self) {
        self.generation = self.generation.wrapping_add(1);
        self.dirty = true;
        self.metadata_ready = false;
        self.metadata = HashMap::new();
        self.groups = Vec::new();
        self.rows = Vec::new();
        self.row_hash = 0;
        self.thumb_failed.clear();
        self.thumbnail_in_flight = None;
        if let Some(task) = &self.metadata_task {
            task.cancel.store(true, Ordering::Relaxed);
        }
    }
}
impl Drop for State {
    fn drop(&mut self) {
        if let Some(task) = &self.metadata_task {
            task.cancel.store(true, Ordering::Relaxed);
        }
    }
}

impl Viewer {
    pub(super) fn draw_browser_panels(&mut self, ctx: &egui::Context) {
        if !self.browser {
            return;
        }
        self.extra.coverflow.invalidate_sequence();
        self.poll_browser_metadata(ctx);
        self.ensure_management_order(ctx);
        self.poll_folder_tree(ctx);
        if self.extra.browser_ui.folders {
            self.draw_folder_tree(ctx);
        }
        if self.extra.browser_ui.preview {
            self.draw_management_preview(ctx);
        }
        if self.extra.browser_ui.tasks {
            self.draw_management_tasks(ctx);
        }
        if self.extra.layout == 2014 {
            self.draw_management_filmstrip(ctx);
        }
        self.management_keyboard(ctx);
    }

    pub(super) fn draw_management(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        self.management_controls(ui);
        let columns = if matches!(self.extra.layout, 2009 | 2010 | 2013) {
            (ui.available_width()
                / if self.extra.layout == 2010 {
                    104.0
                } else if self.extra.layout == 2013 {
                    280.0
                } else {
                    152.0
                })
            .floor()
            .max(1.0) as usize
        } else {
            1
        };
        self.extra.browser_ui.last_columns = columns;
        let height = match self.extra.layout {
            2009 => 160.0,
            2010 => 110.0,
            2013 => 104.0,
            2015 => 88.0,
            _ => 28.0,
        };
        let mut action = None;
        let mut thumbnail = None;
        if self.extra.layout == 2012 {
            self.management_detail_header(ui);
        }
        let mut scroll = egui::ScrollArea::vertical()
            .id_salt("management-scroll")
            .auto_shrink([false, false]);
        self.rebuild_management_rows(columns);
        if let Some(index) = self.extra.browser_ui.scroll_target.take() {
            if let Some(position) = self.extra.visible.iter().position(|i| *i == index) {
                if let Some(row)=self.extra.browser_ui.rows.iter().position(|r|matches!(r,DisplayRow::Items(start,count)if position>=*start&&position<start+count)){scroll=scroll.vertical_scroll_offset(row as f32*(height+ui.spacing().item_spacing.y));}
            }
        }
        let rows = self.extra.browser_ui.rows.len();
        let width = ui.available_width();
        scroll.show_rows(ui, height, rows, |ui, range| {
            for row in range {
                let origin = ui.cursor().min;
                let (start, count) = match self.extra.browser_ui.rows[row] {
                    DisplayRow::Header(group) => {
                        ui.painter().text(
                            origin + egui::vec2(6.0, height / 2.0),
                            egui::Align2::LEFT_CENTER,
                            &self.extra.browser_ui.groups[group].1,
                            egui::TextStyle::Body.resolve(ui.style()),
                            Color32::LIGHT_BLUE,
                        );
                        ui.allocate_space(egui::vec2(width, height));
                        continue;
                    }
                    DisplayRow::Items(start, count) => (start, count),
                };
                for column in 0..count {
                    let position = start + column;
                    if position >= self.extra.visible.len() {
                        break;
                    }
                    let index = self.extra.visible[position];
                    let rect = Rect::from_min_size(
                        origin + egui::vec2(column as f32 * width / columns as f32, 0.0),
                        egui::vec2(width / columns as f32 - 4.0, height - 3.0),
                    );
                    let response = self.paint_management_item(ui, index, rect, self.extra.layout);
                    if response.clicked() {
                        action =
                            Some((index, ctx.input(|i| i.modifiers), response.double_clicked()));
                    }
                    if thumbnail.is_none() && self.wants_management_thumbnail(index) {
                        thumbnail = Some(index);
                    }
                }
                ui.allocate_space(egui::vec2(width, height));
            }
        });
        if let Some((index, modifiers, double)) = action {
            self.activate_management(index, modifiers, double);
        }
        if let Some(index) = thumbnail {
            self.queue_management_thumbnail(index);
        }
    }

    pub(super) fn activate_management(
        &mut self,
        index: usize,
        modifiers: egui::Modifiers,
        double: bool,
    ) {
        if index >= self.entries.len() {
            return;
        }
        if modifiers.shift && self.extra.browser_ui.selection_anchor.is_none() {
            self.extra.browser_ui.selection_anchor = self.selected;
        }
        select_visible(
            &self.extra.visible,
            &mut self.extra.selection,
            &mut self.extra.browser_ui.selection_anchor,
            index,
            modifiers,
        );
        let navigate = self.entries[index].directory || self.entries[index].archive;
        let was_browser = self.browser;
        if double && !modifiers.ctrl && !modifiers.shift {
            self.load_index(index, false);
            if !navigate && self.selected == Some(index) {
                self.browser = false;
            } else if !navigate {
                self.browser = was_browser;
            }
            return;
        }
        if !navigate && !modifiers.ctrl && !modifiers.shift {
            self.load_index(index, false);
            self.browser = if self.selected == Some(index) {
                true
            } else {
                was_browser
            };
        } else {
            self.selected = Some(index);
        }
    }

    pub(super) fn open_management_selection(&mut self, _ctx: &egui::Context) {
        if let Some(index) = self
            .selected
            .or_else(|| self.extra.selection.iter().next().copied())
        {
            if index < self.entries.len() {
                self.load_index(index, false);
                if self.selected == Some(index)
                    && !self.entries[index].directory
                    && !self.entries[index].archive
                {
                    self.browser = false;
                }
            }
        }
    }

    fn poll_browser_metadata(&mut self, ctx: &egui::Context) {
        if let Some((index, serial)) = self.extra.browser_ui.thumbnail_in_flight {
            if self.serial != serial || self.running.is_none() && self.pending.is_none() {
                self.extra.browser_ui.thumbnail_in_flight = None;
                if self.serial == serial && !self.extra.thumbnails.contains_key(&index) {
                    self.extra.browser_ui.thumb_failed.insert(index);
                }
            }
        }
        let result = self.extra.browser_ui.metadata_task.as_ref().and_then(|t| {
            match t.receiver.try_recv() {
                Ok(v) => Some(v),
                Err(mpsc::TryRecvError::Disconnected) => Some(Err("目录排序任务已结束".into())),
                Err(mpsc::TryRecvError::Empty) => None,
            }
        });
        if let Some(result) = result {
            self.extra.browser_ui.metadata_task = None;
            match result {
                Ok(index) if index.generation == self.extra.browser_ui.generation => {
                    self.extra.visible = index.order;
                    self.extra.coverflow.invalidate_sequence();
                    self.extra.browser_ui.order_hash = hash_indices(&self.extra.visible);
                    self.extra.browser_ui.metadata = index.values;
                    self.extra.browser_ui.groups = index.groups;
                    self.extra.browser_ui.metadata_ready = true;
                    self.extra.browser_ui.row_hash = 0;
                }
                Ok(_) => {}
                Err(e) if e != "操作已取消" => {
                    self.status = e;
                }
                Err(_) => {}
            }
        }
        if self.extra.browser_ui.metadata_task.is_some() {
            ctx.request_repaint_after(Duration::from_millis(40));
        }
    }

    fn ensure_management_order(&mut self, ctx: &egui::Context) {
        let order_hash = hash_indices(&self.extra.visible);
        let settings_hash = (self.extra.browser_ui.sort as u64)
            | ((self.extra.browser_ui.group as u64) << 8)
            | ((self.extra.descending as u64) << 16)
            | ((self.extra.group_descending.unwrap_or(false) as u64) << 17);
        if self.extra.browser_ui.source != self.source {
            self.extra.browser_ui.source = self.source.clone();
            self.extra.browser_ui.invalidate();
            self.extra.browser_ui.selection_anchor = None;
        }
        if self.extra.browser_ui.order_hash != order_hash
            || self.extra.browser_ui.settings_hash != settings_hash
        {
            self.extra.browser_ui.invalidate();
            self.extra.browser_ui.order_hash = order_hash;
            self.extra.browser_ui.settings_hash = settings_hash;
        }
        if !self.extra.browser_ui.dirty || self.extra.browser_ui.metadata_task.is_some() {
            return;
        }
        self.extra.browser_ui.dirty = false;
        let bytes = self
            .extra
            .visible
            .iter()
            .filter_map(|i| self.entries.get(*i))
            .map(|e| std::mem::size_of::<(usize, Entry)>() + e.name.len())
            .sum::<usize>();
        if bytes > INDEX_BYTES / 3 {
            self.status = "目录排序索引达到64MiB工作预算，保留当前顺序；请缩小筛选范围".into();
            return;
        }
        let entries = self
            .extra
            .visible
            .iter()
            .filter_map(|i| self.entries.get(*i).map(|e| (*i, e.clone())))
            .collect::<Vec<_>>();
        let generation = self.extra.browser_ui.generation;
        let source = self.source.clone();
        let sort = self.extra.browser_ui.sort;
        let group = self.extra.browser_ui.group;
        let descending = self.extra.descending;
        let group_descending = self.extra.group_descending.unwrap_or(false);
        let cancel = Arc::new(AtomicBool::new(false));
        let flag = cancel.clone();
        let (tx, rx) = mpsc::sync_channel(1);
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            let result = build_index(
                entries,
                source,
                generation,
                sort,
                group,
                descending,
                group_descending,
                &flag,
            );
            let _ = tx.send(result);
            ctx.request_repaint();
        });
        self.extra.browser_ui.metadata_task = Some(MetadataTask {
            receiver: rx,
            cancel,
        });
    }

    fn rebuild_management_rows(&mut self, columns: usize) {
        let hash = hash_indices(&self.extra.visible);
        let state = &mut self.extra.browser_ui;
        if state.row_hash == hash && state.row_columns == columns {
            return;
        }
        state.rows.clear();
        state.row_columns = columns;
        state.row_hash = hash;
        if state.groups.is_empty() || !state.metadata_ready || state.group == GroupBy::None {
            for start in (0..self.extra.visible.len()).step_by(columns) {
                state.rows.push(DisplayRow::Items(
                    start,
                    columns.min(self.extra.visible.len() - start),
                ));
            }
        } else {
            for group in 0..state.groups.len() {
                let start = state.groups[group].0;
                let end = state
                    .groups
                    .get(group + 1)
                    .map(|x| x.0)
                    .unwrap_or(self.extra.visible.len());
                state.rows.push(DisplayRow::Header(group));
                for position in (start..end).step_by(columns) {
                    state
                        .rows
                        .push(DisplayRow::Items(position, columns.min(end - position)));
                }
            }
        }
    }

    fn management_controls(&mut self, ui: &mut egui::Ui) {
        ui.horizontal_wrapped(|ui| {
            egui::ComboBox::from_id_salt("management-sort")
                .selected_text(format!("排序：{}", self.extra.browser_ui.sort.title()))
                .show_ui(ui, |ui| {
                    for value in [
                        SortBy::Name,
                        SortBy::Type,
                        SortBy::Size,
                        SortBy::Date,
                        SortBy::Rating,
                        SortBy::Label,
                    ] {
                        ui.selectable_value(&mut self.extra.browser_ui.sort, value, value.title());
                    }
                });
            ui.checkbox(&mut self.extra.descending, "降序");
            egui::ComboBox::from_id_salt("management-group")
                .selected_text(self.extra.browser_ui.group.title())
                .show_ui(ui, |ui| {
                    for value in [
                        GroupBy::None,
                        GroupBy::Name,
                        GroupBy::Type,
                        GroupBy::Size,
                        GroupBy::Date,
                        GroupBy::Rating,
                        GroupBy::Label,
                    ] {
                        ui.selectable_value(&mut self.extra.browser_ui.group, value, value.title());
                    }
                });
            ui.label(format!(
                "{} 项／已选 {} 项",
                self.extra.visible.len(),
                self.extra.selection.len()
            ));
            if self.extra.browser_ui.metadata_task.is_some() {
                ui.spinner();
            }
        });
        ui.separator();
    }

    fn management_detail_header(&mut self, ui: &mut egui::Ui) {
        let width = ui.available_width();
        ui.horizontal(|ui| {
            for (value, fraction) in [
                (SortBy::Name, 0.40),
                (SortBy::Type, 0.10),
                (SortBy::Size, 0.12),
                (SortBy::Date, 0.22),
                (SortBy::Rating, 0.08),
                (SortBy::Label, 0.08),
            ] {
                if ui
                    .add_sized(
                        [width * fraction - 8.0, 22.0],
                        egui::Button::new(value.title()),
                    )
                    .clicked()
                {
                    if self.extra.browser_ui.sort == value {
                        self.extra.descending = !self.extra.descending;
                    } else {
                        self.extra.browser_ui.sort = value;
                        self.extra.descending = false;
                    }
                }
            }
        });
    }

    fn paint_management_item(
        &self,
        ui: &mut egui::Ui,
        index: usize,
        rect: Rect,
        layout: u16,
    ) -> egui::Response {
        let entry = &self.entries[index];
        let selected = self.extra.selection.contains(&index);
        let response = ui.interact(
            rect,
            egui::Id::new(("management-item", index, layout)),
            Sense::click(),
        );
        response.widget_info(|| {
            egui::WidgetInfo::selected(
                egui::WidgetType::SelectableLabel,
                ui.is_enabled(),
                selected,
                &entry.name,
            )
        });
        let painter = ui.painter_at(rect);
        if selected || response.hovered() {
            painter.rect_filled(
                rect,
                3.0,
                if selected {
                    Color32::from_rgb(47, 77, 108)
                } else {
                    Color32::from_rgb(42, 45, 49)
                },
            );
        }
        let marks = self
            .extra
            .browser_ui
            .metadata
            .get(&index)
            .copied()
            .unwrap_or_default();
        let body_font = egui::TextStyle::Body.resolve(ui.style());
        let small_font = egui::TextStyle::Small.resolve(ui.style());
        let text = Color32::from_gray(220);
        let draw_text = |area: Rect, value: &str, font: &egui::FontId, color| {
            let galley = painter.layout(value.to_owned(), font.clone(), color, area.width());
            painter.with_clip_rect(area).galley(area.min, galley, color);
        };
        if layout == 2012 {
            let cells = [
                entry.name.clone(),
                entry_type(entry),
                human_bytes(entry.size),
                format_timestamp(entry.modified),
                if marks.0 == 0 {
                    String::new()
                } else {
                    format!("{} ★", marks.0)
                },
                label_title(marks.1).to_owned(),
            ];
            let mut x = rect.left() + 4.0;
            for (column, (value, fraction)) in
                cells.iter().zip([0.40, 0.10, 0.12, 0.22, 0.08, 0.08]).enumerate()
            {
                let width = rect.width() * fraction;
                draw_text(
                    Rect::from_min_size(
                        egui::pos2(x, rect.top() + 4.0),
                        egui::vec2(width - 5.0, 22.0),
                    ),
                    value,
                    if column == 0 { &body_font } else { &small_font },
                    text,
                );
                x += width;
            }
        } else if layout == 2011 {
            let prefix = if entry.directory {
                "[文件夹] "
            } else if entry.archive {
                "[压缩包] "
            } else {
                ""
            };
            draw_text(
                rect.shrink2(egui::vec2(5.0, 4.0)),
                &format!("{prefix}{}", entry.name),
                &body_font,
                text,
            );
        } else {
            let vertical = matches!(layout, 2009 | 2010 | 2014);
            let image_rect = if vertical {
                Rect::from_min_max(
                    rect.min + egui::vec2(4.0, 4.0),
                    egui::pos2(rect.right() - 4.0, rect.bottom() - 27.0),
                )
            } else {
                Rect::from_min_size(
                    rect.min + egui::vec2(4.0, 4.0),
                    egui::vec2(rect.height() - 8.0, rect.height() - 8.0),
                )
            };
            if let Some(texture) = self.extra.thumbnails.get(&index) {
                let size = texture.size_vec2();
                let scale = (image_rect.width() / size.x).min(image_rect.height() / size.y);
                let target = Rect::from_center_size(image_rect.center(), size * scale);
                painter.image(
                    texture.id(),
                    target,
                    Rect::from_min_max(Pos2::ZERO, egui::pos2(1.0, 1.0)),
                    Color32::WHITE,
                );
            } else {
                painter.rect_filled(image_rect, 2.0, Color32::from_rgb(30, 32, 35));
                painter.text(
                    image_rect.center(),
                    egui::Align2::CENTER_CENTER,
                    if entry.directory {
                        "文件夹"
                    } else if entry.archive {
                        "压缩包"
                    } else {
                        "图片"
                    },
                    small_font.clone(),
                    Color32::GRAY,
                );
            }
            let name_rect = if vertical {
                Rect::from_min_max(
                    egui::pos2(rect.left() + 4.0, rect.bottom() - 23.0),
                    rect.max - egui::vec2(4.0, 0.0),
                )
            } else {
                Rect::from_min_max(
                    egui::pos2(image_rect.right() + 8.0, rect.top() + 5.0),
                    rect.max - egui::vec2(4.0, 0.0),
                )
            };
            draw_text(name_rect, &entry.name, &body_font, text);
            if !vertical {
                draw_text(
                    Rect::from_min_max(name_rect.min + egui::vec2(0.0, 34.0), name_rect.max),
                    &format!(
                        "{}   {}\n{}  {}",
                        entry_type(entry),
                        human_bytes(entry.size),
                        format_timestamp(entry.modified),
                        label_title(marks.1)
                    ),
                    &small_font,
                    Color32::GRAY,
                );
            }
        }
        response.on_hover_text(format!(
            "{}\n{}  {}\n{}",
            entry.name,
            entry_type(entry),
            human_bytes(entry.size),
            format_timestamp(entry.modified)
        ))
    }

    fn wants_management_thumbnail(&self, index: usize) -> bool {
        self.entries
            .get(index)
            .is_some_and(|e| !e.directory && !e.archive)
            && (!self.large_archived_photoshop(index) || self.selected == Some(index))
            && !self.extra.thumbnails.contains_key(&index)
            && !self.extra.browser_ui.thumb_failed.contains(&index)
            && self.extra.layout != 2011
            && self.extra.layout != 2012
    }
    fn queue_management_thumbnail(&mut self, index: usize) {
        if self.running.is_some()
            || self.pending.is_some()
            || self.extra.animation.is_some()
            || self.extra.retiring.is_some()
            || self.extra.compare > 0
            || self.extra.writing.is_some()
            || self.extra.pending_write.is_some()
            || self.password_dialog
            || self.extra.dialog.is_some()
        {
            return;
        }
        if let Some(source) = self.make_load(index, false) {
            self.queue(
                Request::Thumbnail {
                    source: Box::new(source),
                },
                Operation::Thumbnail { index },
            );
            self.extra.browser_ui.thumbnail_in_flight = Some((index, self.serial));
        }
    }

    fn draw_management_preview(&mut self, ctx: &egui::Context) {
        egui::SidePanel::right("management-preview")
            .default_width(270.0)
            .width_range(180.0..=500.0)
            .show(ctx, |ui| {
                ui.heading("预览");
                let name = self
                    .current_key
                    .and_then(|(i, _)| self.entries.get(i))
                    .map(|e| e.name.as_str())
                    .unwrap_or("选择图片预览");
                ui.add(egui::Label::new(name).wrap());
                if let Some(image) = self.current.clone() {
                    if self.overview.is_none() {
                        self.overview = Some(ctx.load_texture(
                            "management-preview-image",
                            egui::ColorImage::from_rgba_unmultiplied(
                                [image.preview_width as usize, image.preview_height as usize],
                                &image.preview,
                            ),
                            egui::TextureOptions::LINEAR,
                        ));
                    }
                    if let Some(texture) = &self.overview {
                        let preview = egui::Image::from_texture(texture)
                            .max_size(egui::vec2(ui.available_width(), 320.0))
                            .maintain_aspect_ratio(true);
                        let size = preview.calc_size(ui.available_size(), preview.size());
                        let (rect, response) = ui.allocate_exact_size(size, Sense::hover());
                        response.widget_info(|| egui::WidgetInfo::new(egui::WidgetType::Image));
                        if ui.is_rect_visible(rect) {
                            super::paint_transparency(ui.painter(), rect);
                            preview.paint_at(ui, rect);
                        }
                    }
                    ui.label(format!("{} × {}", image.width, image.height));
                    ui.small(&image.note);
                }
                if ui.button("进入查看 (Enter)").clicked() {
                    self.open_management_selection(ctx);
                }
            });
    }

    fn draw_management_tasks(&mut self, ctx: &egui::Context) {
        let mut command = None;
        egui::SidePanel::right("management-tasks")
            .default_width(180.0)
            .show(ctx, |ui| {
                ui.heading("文件任务");
                for (title, id) in [
                    ("复制到…", 2033),
                    ("移动到…", 2034),
                    ("重命名 F2", 2032),
                    ("删除 Delete", 2031),
                    ("属性 Alt+Enter", 2003),
                ] {
                    if ui.button(title).clicked() {
                        command = Some(id);
                    }
                }
                ui.separator();
                ui.small("文件操作先检查冲突；压缩包修改保留原包备份。");
            });
        if let Some(id) = command {
            self.legacy_command(id, ctx);
        }
    }

    fn draw_management_filmstrip(&mut self, ctx: &egui::Context) {
        let mut action = None;
        let mut thumbnail = None;
        self.extra.browser_ui.last_columns = 1;
        let target = self
            .extra
            .browser_ui
            .scroll_target
            .take()
            .and_then(|index| self.extra.visible.iter().position(|i| *i == index));
        egui::TopBottomPanel::bottom("management-filmstrip")
            .exact_height(148.0)
            .show(ctx, |ui| {
                let mut scroll =
                    egui::ScrollArea::horizontal().id_salt("management-filmstrip-scroll");
                if let Some(position) = target {
                    scroll = scroll.horizontal_scroll_offset(position as f32 * 144.0);
                }
                scroll.show_viewport(ui, |ui, view| {
                    let stride = 144.0;
                    let start = (view.min.x / stride).max(0.0) as usize;
                    let end =
                        ((view.max.x / stride).ceil() as usize + 1).min(self.extra.visible.len());
                    let origin = ui.min_rect().min;
                    ui.set_min_size(egui::vec2(stride * self.extra.visible.len() as f32, 130.0));
                    for position in start..end {
                        let index = self.extra.visible[position];
                        let rect = Rect::from_min_size(
                            origin + egui::vec2(position as f32 * stride, 0.0),
                            egui::vec2(136.0, 130.0),
                        );
                        let response = self.paint_management_item(ui, index, rect, 2014);
                        if response.clicked() {
                            action = Some((
                                index,
                                ctx.input(|i| i.modifiers),
                                response.double_clicked(),
                            ));
                        }
                        if thumbnail.is_none() && self.wants_management_thumbnail(index) {
                            thumbnail = Some(index);
                        }
                    }
                });
            });
        if let Some((index, modifiers, double)) = action {
            self.activate_management(index, modifiers, double);
        }
        if let Some(index) = thumbnail {
            self.queue_management_thumbnail(index);
        }
    }

    fn management_keyboard(&mut self, ctx: &egui::Context) {
        if text_field_focused(ctx)
            || self.password_dialog
            || self.extra.dialog.is_some()
            || self.extra.edit_dialog.is_some()
            || self.extra.visible.is_empty()
        {
            return;
        }
        let modifiers = ctx.input(|i| i.modifiers);
        if modifiers.alt || modifiers.command && !modifiers.ctrl {
            return;
        }
        let columns = self.extra.browser_ui.last_columns.max(1);
        let position = self
            .selected
            .and_then(|v| self.extra.visible.iter().position(|i| *i == v))
            .unwrap_or(0);
        let len = self.extra.visible.len();
        let mut target = None;
        ctx.input_mut(|i| {
            for (key, amount) in [
                (egui::Key::ArrowUp, -(columns as isize)),
                (egui::Key::ArrowDown, columns as isize),
                (egui::Key::ArrowLeft, -1),
                (egui::Key::ArrowRight, 1),
                (egui::Key::PageUp, -(columns as isize) * 5),
                (egui::Key::PageDown, columns as isize * 5),
            ] {
                if i.consume_key(modifiers, key) {
                    target = Some(position.saturating_add_signed(amount).min(len - 1));
                }
            }
            if i.consume_key(modifiers, egui::Key::Home) {
                target = Some(0);
            }
            if i.consume_key(modifiers, egui::Key::End) {
                target = Some(len - 1);
            }
        });
        if let Some(position) = target {
            let index = self.extra.visible[position];
            if modifiers.ctrl && !modifiers.shift {
                self.selected = Some(index);
            } else {
                self.activate_management(index, modifiers, false);
            }
            self.extra.browser_ui.scroll_target = Some(index);
        }
    }

    fn poll_folder_tree(&mut self, ctx: &egui::Context) {
        if self.extra.browser_ui.folders_rows.is_empty() {
            #[cfg(windows)]
            {
                let drives = unsafe { windows_sys::Win32::Storage::FileSystem::GetLogicalDrives() };
                for n in 0..26 {
                    if drives & (1 << n) != 0 {
                        self.extra.browser_ui.folders_rows.push(FolderNode {
                            path: PathBuf::from(format!("{}:\\", (b'A' + n) as char)),
                            depth: 0,
                            expanded: false,
                            loaded: false,
                        });
                    }
                }
            }
            #[cfg(not(windows))]
            self.extra.browser_ui.folders_rows.push(FolderNode {
                path: PathBuf::from("/"),
                depth: 0,
                expanded: false,
                loaded: false,
            });
        }
        let result =
            self.extra
                .browser_ui
                .folder_task
                .as_ref()
                .and_then(|t| match t.receiver.try_recv() {
                    Ok(v) => Some(v),
                    Err(mpsc::TryRecvError::Disconnected) => {
                        Some(Err("文件夹读取任务已结束".into()))
                    }
                    Err(mpsc::TryRecvError::Empty) => None,
                });
        if let Some(result) = result {
            let task = self.extra.browser_ui.folder_task.take().unwrap();
            match result {
                Ok(children) => {
                    if let Some(index) = self
                        .extra
                        .browser_ui
                        .folders_rows
                        .iter()
                        .position(|n| n.path == task.path && n.expanded)
                    {
                        let depth = self.extra.browser_ui.folders_rows[index].depth + 1;
                        let existing = self
                            .extra
                            .browser_ui
                            .folders_rows
                            .iter()
                            .map(|n| {
                                n.path.as_os_str().len() * 2 + std::mem::size_of::<FolderNode>()
                            })
                            .sum::<usize>();
                        let mut bytes = existing;
                        let capacity =
                            TREE_ROWS.saturating_sub(self.extra.browser_ui.folders_rows.len());
                        let nodes = children
                            .into_iter()
                            .take(capacity)
                            .take_while(|p| {
                                bytes +=
                                    p.as_os_str().len() * 2 + std::mem::size_of::<FolderNode>();
                                bytes <= TREE_BYTES
                            })
                            .map(|path| FolderNode {
                                path,
                                depth,
                                expanded: false,
                                loaded: false,
                            })
                            .collect::<Vec<_>>();
                        self.extra.browser_ui.folders_rows[index].loaded = true;
                        self.extra
                            .browser_ui
                            .folders_rows
                            .splice(index + 1..index + 1, nodes);
                    }
                }
                Err(e) => self.extra.browser_ui.folder_problem = Some(e),
            }
        }
        if self.extra.browser_ui.folder_task.is_some() {
            ctx.request_repaint_after(Duration::from_millis(60));
        }
    }

    fn draw_folder_tree(&mut self, ctx: &egui::Context) {
        let mut expand = None;
        let mut open = None;
        egui::SidePanel::left("management-folders")
            .default_width(220.0)
            .width_range(140.0..=450.0)
            .show(ctx, |ui| {
                ui.heading("文件夹");
                if let Some(source) = self.source.clone() {
                    ui.add(egui::Label::new(source.to_string_lossy()).wrap());
                }
                if let Some(problem) = &self.extra.browser_ui.folder_problem {
                    ui.colored_label(Color32::LIGHT_RED, problem);
                }
                let len = self.extra.browser_ui.folders_rows.len();
                egui::ScrollArea::vertical()
                    .id_salt("folder-tree-scroll")
                    .show_rows(ui, 24.0, len, |ui, range| {
                        for index in range {
                            let node = &self.extra.browser_ui.folders_rows[index];
                            ui.horizontal(|ui| {
                                ui.add_space((node.depth.min(32) * 14) as f32);
                                if ui
                                    .small_button(if node.expanded { "−" } else { "+" })
                                    .clicked()
                                {
                                    expand = Some(index);
                                }
                                let title = node
                                    .path
                                    .file_name()
                                    .map(|s| s.to_string_lossy().into_owned())
                                    .unwrap_or_else(|| node.path.to_string_lossy().into_owned());
                                if ui
                                    .selectable_label(
                                        self.source.as_ref() == Some(&node.path),
                                        title,
                                    )
                                    .clicked()
                                {
                                    open = Some(node.path.clone());
                                }
                            });
                        }
                    });
            });
        if let Some(index) = expand {
            if self.extra.browser_ui.folders_rows[index].expanded {
                let depth = self.extra.browser_ui.folders_rows[index].depth;
                let end = self
                    .extra
                    .browser_ui
                    .folders_rows
                    .iter()
                    .enumerate()
                    .skip(index + 1)
                    .find(|(_, n)| n.depth <= depth)
                    .map(|(i, _)| i)
                    .unwrap_or(self.extra.browser_ui.folders_rows.len());
                self.extra.browser_ui.folders_rows.drain(index + 1..end);
                self.extra.browser_ui.folders_rows[index].expanded = false;
                self.extra.browser_ui.folders_rows[index].loaded = false;
            } else if self.extra.browser_ui.folder_task.is_none() {
                let path = self.extra.browser_ui.folders_rows[index].path.clone();
                self.extra.browser_ui.folders_rows[index].expanded = true;
                let worker_path = path.clone();
                let (tx, rx) = mpsc::sync_channel(1);
                let ctx = ctx.clone();
                std::thread::spawn(move || {
                    let result = read_child_folders(&worker_path);
                    let _ = tx.send(result);
                    ctx.request_repaint();
                });
                self.extra.browser_ui.folder_task = Some(FolderTask { path, receiver: rx });
                self.extra.browser_ui.folder_problem = None;
            }
        }
        if let Some(path) = open {
            self.open_path(path);
        }
    }
}

fn hash_indices(values: &[usize]) -> u64 {
    let mut h = std::collections::hash_map::DefaultHasher::new();
    values.hash(&mut h);
    h.finish()
}
fn human_bytes(bytes: u64) -> String {
    if bytes >= 1024 * 1024 * 1024 {
        format!("{:.1} GB", bytes as f64 / (1024.0 * 1024.0 * 1024.0))
    } else if bytes >= 1024 * 1024 {
        format!("{:.1} MB", bytes as f64 / (1024.0 * 1024.0))
    } else if bytes >= 1024 {
        format!("{:.1} KB", bytes as f64 / 1024.0)
    } else {
        format!("{bytes} B")
    }
}
fn entry_type(entry: &Entry) -> String {
    if entry.directory {
        "文件夹".into()
    } else if entry.archive {
        format!("{} 压缩包", extension(entry))
    } else {
        extension(entry)
    }
}
fn extension(entry: &Entry) -> String {
    Path::new(&entry.name)
        .extension()
        .and_then(|s| s.to_str())
        .unwrap_or("图片")
        .to_ascii_uppercase()
}
fn label_title(value: u8) -> &'static str {
    match value {
        1 => "红色",
        2 => "黄色",
        3 => "绿色",
        4 => "蓝色",
        5 => "紫色",
        _ => "",
    }
}
fn format_timestamp(value: Option<i64>) -> String {
    value
        .and_then(|v| {
            if v >= 0 {
                UNIX_EPOCH.checked_add(Duration::from_secs(v as u64))
            } else {
                UNIX_EPOCH.checked_sub(Duration::from_secs(v.unsigned_abs()))
            }
        })
        .map(crate::fileops::format_local_datetime)
        .unwrap_or_else(|| "—".into())
}
fn group_key(entry: &Entry, marks: (u8, u8), group: GroupBy) -> String {
    if entry.directory {
        return "文件夹".into();
    }
    match group {
        GroupBy::None => String::new(),
        GroupBy::Name => entry
            .name
            .chars()
            .next()
            .map(|c| c.to_uppercase().collect())
            .unwrap_or_default(),
        GroupBy::Type => entry_type(entry),
        GroupBy::Size => match entry.size {
            0..=1_048_575 => "1 · 小于1 MB",
            1_048_576..=10_485_759 => "2 · 1–10 MB",
            10_485_760..=104_857_599 => "3 · 10–100 MB",
            _ => "4 · 100 MB以上",
        }
        .into(),
        GroupBy::Date => format_timestamp(entry.modified).chars().take(10).collect(),
        GroupBy::Rating => format!("{} 星", marks.0),
        GroupBy::Label => {
            if marks.1 == 0 {
                "无颜色标签".into()
            } else {
                label_title(marks.1).into()
            }
        }
    }
}

fn build_index(
    mut entries: Vec<(usize, Entry)>,
    source: Option<PathBuf>,
    generation: u64,
    sort: SortBy,
    group: GroupBy,
    descending: bool,
    group_descending: bool,
    cancel: &AtomicBool,
) -> Result<MetadataIndex, String> {
    let check = || {
        if cancel.load(Ordering::Relaxed) {
            Err("操作已取消".to_string())
        } else {
            Ok(())
        }
    };
    check()?;
    let mut values = HashMap::new();
    if let Some(source) = source {
        let source = source.canonicalize().map_err(|e| e.to_string())?;
        let local = source.is_dir();
        let database = std::env::current_exe()
            .map_err(|e| e.to_string())?
            .parent()
            .ok_or("无法定位目录库")?
            .join("RustView-data/catalog.sqlite");
        let catalog = match Catalog::open(&database) {
            Ok(catalog) => Some(catalog),
            Err(e) if sort.needs_catalog() || group.needs_catalog() => return Err(e),
            Err(_) => None,
        };
        if let Some(catalog) = catalog {
            for batch in entries.chunks(500) {
                check()?;
                let mut keys = HashMap::new();
                let mut bytes = 0usize;
                for (index, entry) in batch {
                    let key = if local {
                        serde_json::to_string(&(
                            source.join(&entry.name).to_string_lossy(),
                            Option::<&str>::None,
                        ))
                    } else {
                        serde_json::to_string(&(source.to_string_lossy(), Some(&entry.name)))
                    }
                    .map_err(|e| e.to_string())?;
                    bytes += key.len();
                    if bytes > 4 * 1024 * 1024 {
                        return Err("目录键查询批次超过4MiB，请缩小筛选范围".into());
                    }
                    keys.insert(key, *index);
                }
                let input = keys.keys().cloned().collect::<Vec<_>>();
                for (key, rating, label) in catalog.get_marks(&input)? {
                    if let Some(index) = keys.get(&key) {
                        values.insert(*index, (rating, label));
                    }
                }
            }
        }
    }
    check()?;
    let mut groups_by_index = HashMap::new();
    if group != GroupBy::None {
        let mut bytes = 0usize;
        for (index, entry) in &entries {
            check()?;
            let key = group_key(entry, values.get(index).copied().unwrap_or_default(), group);
            bytes += key.len() + 48;
            if bytes > INDEX_BYTES / 4 {
                return Err("分组索引达到内存上限，请缩小筛选范围".into());
            }
            groups_by_index.insert(*index, key);
        }
    }
    entries.sort_unstable_by(|(ia, a), (ib, b)| {
        let directory = b.directory.cmp(&a.directory);
        if directory != CmpOrdering::Equal {
            return directory;
        }
        if group != GroupBy::None {
            let order = natord::compare_ignore_case(&groups_by_index[ia], &groups_by_index[ib]);
            if order != CmpOrdering::Equal {
                return if group_descending {
                    order.reverse()
                } else {
                    order
                };
            }
        }
        let order = match sort {
            SortBy::Name => natord::compare_ignore_case(&a.name, &b.name),
            SortBy::Type => extension(a).cmp(&extension(b)),
            SortBy::Size => a.size.cmp(&b.size),
            SortBy::Date => a.modified.cmp(&b.modified),
            SortBy::Rating => values
                .get(ia)
                .copied()
                .unwrap_or_default()
                .0
                .cmp(&values.get(ib).copied().unwrap_or_default().0),
            SortBy::Label => values
                .get(ia)
                .copied()
                .unwrap_or_default()
                .1
                .cmp(&values.get(ib).copied().unwrap_or_default().1),
        }
        .then_with(|| natord::compare_ignore_case(&a.name, &b.name))
        .then_with(|| ia.cmp(ib));
        if descending {
            order.reverse()
        } else {
            order
        }
    });
    check()?;
    let mut groups = Vec::new();
    let mut previous = None;
    let mut group_bytes = 0usize;
    for (position, (index, _)) in entries.iter().enumerate() {
        if let Some(key) = groups_by_index.remove(index) {
            if previous.as_ref() != Some(&key) {
                group_bytes += key.len() + std::mem::size_of::<(usize, String)>();
                if group_bytes > INDEX_BYTES / 8 {
                    return Err("分组标题超过8MiB上限".into());
                }
                groups.push((position, key.clone()));
                previous = Some(key);
            }
        }
    }
    Ok(MetadataIndex {
        generation,
        values,
        order: entries.into_iter().map(|(index, _)| index).collect(),
        groups,
    })
}

fn read_child_folders(path: &Path) -> Result<Vec<PathBuf>, String> {
    let mut result = Vec::new();
    let mut bytes = 0usize;
    for item in std::fs::read_dir(path).map_err(|e| e.to_string())? {
        let item = item.map_err(|e| e.to_string())?;
        let kind = item.file_type().map_err(|e| e.to_string())?;
        if !kind.is_dir() || kind.is_symlink() {
            continue;
        }
        let metadata = item.metadata().map_err(|e| e.to_string())?;
        #[cfg(windows)]
        {
            use std::os::windows::fs::MetadataExt;
            if metadata.file_attributes() & 0x400 != 0 {
                continue;
            }
        }
        let path = item.path();
        bytes += path.as_os_str().len() * 2 + std::mem::size_of::<FolderNode>();
        if result.len() >= 4096 || bytes > TREE_BYTES / 2 {
            return Err("该文件夹的子目录超过4096项或4MiB上限，请从路径栏进入具体目录".into());
        }
        result.push(path);
    }
    result.sort_unstable_by(|a, b| {
        natord::compare_ignore_case(&a.to_string_lossy(), &b.to_string_lossy())
    });
    Ok(result)
}

fn select_visible(
    visible: &[usize],
    selection: &mut BTreeSet<usize>,
    anchor: &mut Option<usize>,
    index: usize,
    modifiers: egui::Modifiers,
) {
    if modifiers.shift {
        let Some(end) = visible.iter().position(|i| *i == index) else {
            return;
        };
        let start = anchor
            .and_then(|a| visible.iter().position(|i| *i == a))
            .unwrap_or(end);
        if !modifiers.ctrl {
            selection.clear();
        }
        selection.extend(visible[start.min(end)..=start.max(end)].iter().copied());
        if anchor.is_none() {
            *anchor = Some(index);
        }
    } else if modifiers.ctrl {
        if !selection.insert(index) {
            selection.remove(&index);
        }
        *anchor = Some(index);
    } else {
        selection.clear();
        selection.insert(index);
        *anchor = Some(index);
    }
}

#[cfg(test)]
pub(super) mod tests {
    use super::*;
    pub(in crate::app) fn viewer(entries: Vec<Entry>) -> Viewer {
        let mut app = Viewer::empty();
        app.extra.visible = (0..entries.len()).collect();
        app.extra.selection.insert(0);
        app.extra.browser_ui.folders = false;
        app.extra.browser_ui.preview = false;
        app.extra.browser_ui.tasks = false;
        app.entries = entries;
        app.selected = Some(0);
        app.status.clear();
        app
    }
    fn entry(name: &str, size: u64, directory: bool) -> Entry {
        Entry {
            name: name.into(),
            size,
            directory,
            archive: false,
            modified: Some(1_700_000_000),
        }
    }

    #[test]
    fn large_archived_photoshop_management_thumbnail_requires_selection() {
        let mut viewer = viewer(vec![entry("large.psd", worker::MAX_INPUT_BYTES + 1, false), entry("small.png", 1, false)]);
        viewer.extra.layout = 2010;
        viewer.source = Some("photos.zip".into());
        viewer.selected = Some(1);
        assert!(!viewer.wants_management_thumbnail(0));
        assert!(viewer.wants_management_thumbnail(1));
        viewer.selected = Some(0);
        assert!(viewer.wants_management_thumbnail(0));
    }
    #[test]
    fn shift_selection_follows_filtered_visible_order() {
        let visible = [8, 5, 2, 9];
        let mut selected = BTreeSet::new();
        let mut anchor = None;
        select_visible(
            &visible,
            &mut selected,
            &mut anchor,
            5,
            egui::Modifiers::NONE,
        );
        select_visible(
            &visible,
            &mut selected,
            &mut anchor,
            9,
            egui::Modifiers::SHIFT,
        );
        assert_eq!(selected, BTreeSet::from([5, 2, 9]));
        select_visible(
            &visible,
            &mut selected,
            &mut anchor,
            8,
            egui::Modifiers::CTRL,
        );
        select_visible(
            &visible,
            &mut selected,
            &mut anchor,
            5,
            egui::Modifiers {
                ctrl: true,
                shift: true,
                ..Default::default()
            },
        );
        assert_eq!(selected, BTreeSet::from([8, 5, 2, 9]));
        select_visible(
            &visible,
            &mut selected,
            &mut anchor,
            2,
            egui::Modifiers::CTRL,
        );
        assert!(!selected.contains(&2));
        assert!(!selected.contains(&0));
    }
    #[test]
    fn natural_sort_groups_and_cancellation_are_bounded() {
        let entries = vec![
            (8, entry("photo10.jpg", 4, false)),
            (2, entry("photo2.webp", 2, false)),
            (5, entry("folder", 0, true)),
            (7, entry("photo1.jpg", 1, false)),
        ];
        let flag = AtomicBool::new(false);
        let sorted = build_index(
            entries.clone(),
            None,
            4,
            SortBy::Name,
            GroupBy::None,
            false,
            false,
            &flag,
        )
        .unwrap();
        assert_eq!(sorted.order, [5, 7, 2, 8]);
        let descending = build_index(
            entries.clone(),
            None,
            4,
            SortBy::Name,
            GroupBy::None,
            true,
            false,
            &flag,
        )
        .unwrap();
        assert_eq!(descending.order, [5, 8, 2, 7]);
        let grouped = build_index(
            entries.clone(),
            None,
            4,
            SortBy::Name,
            GroupBy::Type,
            false,
            false,
            &flag,
        )
        .unwrap();
        assert_eq!(grouped.order, [5, 7, 8, 2]);
        assert_eq!(
            grouped.groups.iter().map(|x| x.0).collect::<Vec<_>>(),
            [0, 1, 3]
        );
        flag.store(true, Ordering::Relaxed);
        assert!(build_index(
            entries,
            None,
            4,
            SortBy::Name,
            GroupBy::None,
            false,
            false,
            &flag
        )
        .is_err());
    }
    #[test]
    fn folder_tree_listing_only_returns_direct_directories() {
        let root = tempfile::tempdir().unwrap();
        std::fs::create_dir(root.path().join("folder10")).unwrap();
        std::fs::create_dir(root.path().join("folder2")).unwrap();
        std::fs::create_dir(root.path().join("folder2/deep")).unwrap();
        std::fs::write(root.path().join("photo.jpg"), b"photo").unwrap();
        let children = read_child_folders(root.path()).unwrap();
        assert_eq!(children.len(), 2);
        assert_eq!(children[0].file_name().unwrap(), "folder2");
        assert_eq!(children[1].file_name().unwrap(), "folder10");
    }

    #[test]
    fn hundred_thousand_items_sort_and_each_layout_only_paint_visible_rows() {
        let entries = (0..100_000)
            .rev()
            .map(|n| (n, entry(&format!("photo{n}.jpg"), n as u64, false)))
            .collect::<Vec<_>>();
        let started = Instant::now();
        let index = build_index(
            entries.clone(),
            None,
            1,
            SortBy::Name,
            GroupBy::Type,
            false,
            false,
            &AtomicBool::new(false),
        )
        .unwrap();
        assert_eq!(index.order[0], 0);
        assert_eq!(index.order[99_999], 99_999);
        assert_eq!(index.groups.len(), 1);
        eprintln!("100000-item name/type index: {:?}", started.elapsed());
        let mut viewer = viewer(entries.into_iter().map(|(_, e)| e).collect());
        let context = egui::Context::default();
        for layout in [2009, 2010, 2011, 2012, 2013, 2015] {
            viewer.extra.layout = layout;
            let output = context.run(
                egui::RawInput {
                    screen_rect: Some(Rect::from_min_size(Pos2::ZERO, egui::vec2(1024.0, 768.0))),
                    ..Default::default()
                },
                |ctx| {
                    egui::CentralPanel::default().show(ctx, |ui| viewer.draw_management(ui, ctx));
                },
            );
            assert!(
                output.shapes.len() < 5000,
                "layout {layout} created {} shapes",
                output.shapes.len()
            );
            assert!(viewer.extra.thumbnails.is_empty());
        }
        viewer.extra.layout = 2014;
        let output = context.run(
            egui::RawInput {
                screen_rect: Some(Rect::from_min_size(Pos2::ZERO, egui::vec2(1024.0, 768.0))),
                ..Default::default()
            },
            |ctx| viewer.draw_management_filmstrip(ctx),
        );
        assert!(output.shapes.len() < 500);
    }

    fn key_input(key: egui::Key, modifiers: egui::Modifiers) -> egui::RawInput {
        egui::RawInput {
            modifiers,
            events: vec![egui::Event::Key {
                key,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers,
            }],
            ..Default::default()
        }
    }
    #[test]
    fn ctrl_navigation_only_moves_focus_and_initial_shift_uses_current_anchor() {
        let mut app = viewer(vec![
            entry("a.jpg", 1, false),
            entry("folder", 0, true),
            entry("b.jpg", 1, false),
        ]);
        let ctx = egui::Context::default();
        let _ = ctx.run(
            key_input(egui::Key::ArrowDown, egui::Modifiers::CTRL),
            |ctx| app.management_keyboard(ctx),
        );
        assert_eq!(app.selected, Some(1));
        assert_eq!(app.extra.selection, BTreeSet::from([0]));
        assert!(app.pending.is_none());
        app.selected = Some(0);
        let _ = ctx.run(
            key_input(egui::Key::ArrowDown, egui::Modifiers::SHIFT),
            |ctx| app.management_keyboard(ctx),
        );
        assert_eq!(app.selected, Some(1));
        assert_eq!(app.extra.selection, BTreeSet::from([0, 1]));
        assert!(app.pending.is_none());
    }

    #[test]
    fn filmstrip_enter_opens_cached_preview_and_modifier_click_stays_management() {
        let root = tempfile::tempdir().unwrap();
        std::fs::write(root.path().join("a.jpg"), b"fixture").unwrap();
        let mut app = viewer(vec![entry("a.jpg", 7, false)]);
        app.source = Some(root.path().to_owned());
        app.extra.layout = 2014;
        let pixels = Arc::new(Pixels {
            width: 1,
            height: 1,
            rgba: vec![255; 4],
            preview: vec![255; 4],
            preview_width: 1,
            preview_height: 1,
            note: String::new(),
            record: None,
        });
        app.cache.insert((0, false), pixels, false);
        let ctx = egui::Context::default();
        app.activate_management(0, egui::Modifiers::CTRL, false);
        assert!(app.browser);
        assert!(app.pending.is_none());
        app.open_management_selection(&ctx);
        assert!(!app.browser);
        assert_eq!(app.current_key, Some((0, false)));
        assert!(app.current.is_some());
        assert!(app.pending.is_none());
    }
}

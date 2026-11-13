use super::*;
use crate::{
    editing::Edit,
    precision::{AnimationMode, ExportSettings, OutputFormat, RawSettings},
    recipes::{self, EditRecord, Recipe},
};

pub(super) struct State {
    pub raw: RawSettings,
    pub export: ExportSettings,
    pub batch_indices: Vec<usize>,
    pub preview_valid: bool,
    pub save_pending: bool,
    source_key: Option<String>,
    baseline: Vec<u8>,
    name: String,
    presets: Vec<Recipe>,
    selected_preset: usize,
    tool: usize,
    p: [f32; 9],
    text: String,
    watermark: String,
    color: [u8; 4],
    crop: [u32; 4],
    keep_aspect: bool,
    pub preview_due: Option<Instant>,
    pub output_pattern: String,
    pub number: u64,
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn saved_recipe_survives_preview_replacement_and_undo_returns_to_clean_baseline() {
        let directory = tempfile::tempdir().unwrap();
        let source = directory.path().join("sample.png");
        image::RgbaImage::from_pixel(6, 4, image::Rgba([60, 80, 100, 96]))
            .save(&source)
            .unwrap();
        let mut viewer = super::super::browser_ui::tests::viewer(vec![Entry {
            name: "sample.png".into(),
            size: 1,
            directory: false,
            archive: false,
            modified: None,
        }]);
        viewer.source = Some(directory.path().to_owned());
        let mut recipe = Recipe::default();
        recipe.edits.push(Edit::Exposure { ev: 0.5 });
        let record = EditRecord::capture(&source, None, recipe.clone()).unwrap();
        viewer.set_current(
            0,
            false,
            Arc::new(Pixels {
                width: 6,
                height: 4,
                rgba: [60, 80, 100, 96].repeat(24),
                preview: vec![60, 80, 100, 96],
                preview_width: 1,
                preview_height: 1,
                note: String::new(),
                record: Some(Arc::new(record)),
            }),
        );
        assert_eq!(viewer.extra.edits, recipe.edits);
        assert!(!viewer.extra.edited);
        viewer.apply_edit(Edit::Resize {
            width: 3,
            height: 2,
        });
        assert!(viewer.extra.edited);
        let record_path =
            recipes::record_path(&recipes::data_root().unwrap(), &source, None).unwrap();
        viewer.save_edit_record();
        assert!(viewer.extra.workspace.save_pending);
        assert!(!record_path.exists());
        assert!(matches!(
            viewer.pending.as_ref().unwrap().operation,
            Operation::Rendered { index: 0 }
        ));
        viewer.accept_render_preview(
            0,
            worker::Output {
                reply: worker::Reply {
                    width: 3,
                    height: 2,
                    preview_width: 3,
                    preview_height: 2,
                    ..Default::default()
                },
                rgba: [60, 80, 100, 96].repeat(6),
                preview: [60, 80, 100, 96].repeat(6),
            },
        );
        assert_eq!(viewer.extra.edits.len(), 2);
        assert!(!viewer.extra.edited);
        assert!(record_path.exists());
        std::fs::remove_file(record_path).unwrap();
        viewer.undo_edit(false);
        assert_eq!(viewer.extra.edits, recipe.edits);
        assert!(viewer.extra.edited);
        viewer.undo_edit(true);
        assert!(!viewer.extra.edited);
        viewer.extra.workspace.raw.exposure_ev = 1.0;
        viewer.render_edits();
        assert!(viewer.extra.edited);
        viewer.entries.push(Entry {
            name: "other.png".into(),
            size: 1,
            directory: false,
            archive: false,
            modified: None,
        });
        viewer.selected = Some(1);
        viewer.sync_workspace_source(None);
        assert!(viewer.extra.edits.is_empty());
        assert_eq!(viewer.extra.workspace.raw.exposure_ev, 0.0);
    }
    #[test]
    fn processing_workspace_renders_with_raw_controls_without_a_native_window() {
        let mut viewer = super::super::browser_ui::tests::viewer(vec![Entry {
            name: "sample.NEF".into(),
            size: 1,
            directory: false,
            archive: false,
            modified: None,
        }]);
        viewer.selected = Some(0);
        viewer.extra.edit_dialog = Some(10579);
        viewer.current_key = Some((0, false));
        viewer.current = Some(Arc::new(Pixels {
            width: 6,
            height: 4,
            rgba: [0, 0, 0, 255].repeat(24),
            preview: vec![0, 0, 0, 255],
            preview_width: 1,
            preview_height: 1,
            note: String::new(),
            record: None,
        }));
        let context = egui::Context::default();
        let output = context.run(
            egui::RawInput {
                screen_rect: Some(Rect::from_min_size(Pos2::ZERO, egui::vec2(1280.0, 840.0))),
                ..Default::default()
            },
            |ctx| {
                viewer.edit_dialogs(ctx);
                egui::CentralPanel::default().show(ctx, |ui| {
                    ui.label("图像预览");
                });
            },
        );
        assert!(!output.shapes.is_empty());
        assert!(output.shapes.len() < 5000);
    }
    #[test]
    fn a_late_raw_preview_cannot_replace_navigation_or_apply_to_a_new_source() {
        let mut viewer = super::super::browser_ui::tests::viewer(vec![
            Entry {
                name: "first.NEF".into(),
                size: 1,
                directory: false,
                archive: false,
                modified: None,
            },
            Entry {
                name: "second.NEF".into(),
                size: 1,
                directory: false,
                archive: false,
                modified: None,
            },
        ]);
        viewer.source = Some(PathBuf::from("C:/example"));
        viewer.selected = Some(1);
        viewer.current_key = Some((0, false));
        viewer.extra.workspace.preview_due = Some(Instant::now());
        viewer.extra.workspace.raw.exposure_ev = 3.0;
        viewer.serial = 17;
        viewer.pending = Some(Task {
            id: 17,
            request: Request::List {
                path: PathBuf::from("C:/new"),
                password: None,
            },
            operation: Operation::List {
                path: PathBuf::from("C:/new"),
                select: None,
                password: None,
                management: true,
                from_start: false,
            },
        });
        viewer.schedule_edit_preview();
        assert!(viewer.extra.workspace.preview_due.is_none());
        assert_eq!(viewer.serial, 17);
        assert!(matches!(
            viewer.pending.as_ref().unwrap().operation,
            Operation::List { .. }
        ));
    }
    #[test]
    fn batch_workspace_keeps_visible_selection_when_switching_from_manage_to_edit() {
        let mut viewer = super::super::browser_ui::tests::viewer(vec![
            Entry {
                name: "a.png".into(),
                size: 1,
                directory: false,
                archive: false,
                modified: None,
            },
            Entry {
                name: "b.png".into(),
                size: 1,
                directory: false,
                archive: false,
                modified: None,
            },
            Entry {
                name: "hidden.png".into(),
                size: 1,
                directory: false,
                archive: false,
                modified: None,
            },
        ]);
        viewer.extra.visible = vec![1, 0];
        viewer.extra.selection = [0, 1, 2].into_iter().collect();
        viewer.selected = Some(0);
        viewer.current_key = Some((0, false));
        viewer.current = Some(Arc::new(Pixels {
            width: 1,
            height: 1,
            rgba: vec![0, 0, 0, 255],
            preview: vec![0, 0, 0, 255],
            preview_width: 1,
            preview_height: 1,
            note: String::new(),
            record: None,
        }));
        viewer.open_edit_dialog(2045);
        assert!(!viewer.browser);
        assert_eq!(viewer.extra.workspace.batch_indices, vec![1, 0]);
    }
}
impl Default for State {
    fn default() -> Self {
        Self {
            raw: RawSettings::default(),
            export: ExportSettings::default(),
            batch_indices: Vec::new(),
            preview_valid: false,
            save_pending: false,
            source_key: None,
            baseline: Vec::new(),
            name: "我的处理预设".into(),
            presets: Vec::new(),
            selected_preset: 0,
            tool: 0,
            p: [0.0; 9],
            text: String::new(),
            watermark: String::new(),
            color: [255, 255, 255, 255],
            crop: [0, 0, 1, 1],
            keep_aspect: true,
            preview_due: None,
            output_pattern: "{name}-edited-{n}{ext}".into(),
            number: 1,
        }
    }
}
const TOOLS: &[&str] = &[
    "曝光",
    "调整大小",
    "裁剪",
    "旋转／翻转",
    "白平衡",
    "亮度／对比／Gamma",
    "色阶",
    "饱和度",
    "色相",
    "通道混合",
    "自动色阶",
    "自动对比",
    "锐化",
    "模糊",
    "降噪",
    "褐色",
    "暗角",
    "反相",
    "曝光过度效果",
    "色调分离",
    "浮雕",
    "边缘",
    "油画",
    "扩散",
    "漩涡",
    "波浪",
    "镜头畸变",
    "文字",
    "图片水印",
    "灰度",
];
impl State {
    pub fn select_command(&mut self, id: u16) {
        self.tool = match id {
            2045 | 21852 => 1,
            2044 | 2285 => 3,
            _ => 0,
        };
        self.reset_parameters();
    }
    fn reset_parameters(&mut self) {
        self.p = [0.0; 9];
        match self.tool {
            4 => self.p[..3].fill(1.0),
            5 => self.p[..3].copy_from_slice(&[0.0, 0.0, 1.0]),
            6 => self.p[..3].copy_from_slice(&[0.0, 100.0, 1.0]),
            7 | 15 => self.p[0] = 100.0,
            9 => self.p = [1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0],
            12 => self.p[..3].copy_from_slice(&[1.0, 1.0, 0.0]),
            13 | 14 | 16 | 20 | 21 | 22 | 23 => self.p[0] = 1.0,
            18 => self.p[0] = 50.0,
            19 => self.p[0] = 8.0,
            24 => self.p[0] = 90.0,
            25 => self.p[..2].copy_from_slice(&[10.0, 100.0]),
            26 => self.p[3] = 1.0,
            27 => self.p[..3].copy_from_slice(&[20.0, 30.0, 24.0]),
            28 => self.p[..4].copy_from_slice(&[0.0, 0.0, 0.5, 512.0]),
            _ => {}
        }
    }
    fn dirty_bytes(edits: &[Edit], raw: &RawSettings) -> Vec<u8> {
        serde_json::to_vec(&(edits, raw)).unwrap_or_default()
    }
    pub fn mark_saved(&mut self, edits: &[Edit]) {
        self.baseline = Self::dirty_bytes(edits, &self.raw);
    }
    pub fn is_dirty(&self, edits: &[Edit]) -> bool {
        self.baseline != Self::dirty_bytes(edits, &self.raw)
    }
}

impl Viewer {
    pub(super) fn workspace_source(&self) -> Option<(PathBuf, Option<String>)> {
        fn walk(r: Request) -> Option<(PathBuf, Option<String>)> {
            match r {
                Request::Load { path, entry, .. } => Some((path, entry)),
                Request::ColorPreview { source, .. } | Request::Page { source, .. } => {
                    walk(*source)
                }
                _ => None,
            }
        }
        walk(self.make_load(self.selected?, self.full_raw)?)
    }
    fn current_recipe(&self) -> Recipe {
        Recipe {
            version: 1,
            name: self.extra.workspace.name.clone(),
            edits: self.document_edits(),
            export: self.extra.workspace.export.clone(),
            raw: self.extra.workspace.raw.clone(),
        }
    }
    pub(super) fn sync_workspace_source(&mut self, record: Option<&EditRecord>) {
        let key = self.selected.and_then(|i| self.item_key(i));
        if self.extra.workspace.source_key != key {
            self.extra.edits.clear();
            self.extra.redo_edits.clear();
            let mut state = State::default();
            state.source_key = key;
            state.presets = recipes::data_root()
                .and_then(|p| recipes::load_presets(&p.join("processing-presets.json")))
                .unwrap_or_default();
            if self
                .selected
                .and_then(|i| self.entries.get(i))
                .is_some_and(|e| {
                    matches!(
                        Path::new(&e.name)
                            .extension()
                            .and_then(|s| s.to_str())
                            .unwrap_or("")
                            .to_ascii_lowercase()
                            .as_str(),
                        "gif" | "webp"
                    )
                })
            {
                state.export.animation = AnimationMode::AllFrames;
                state.export.format = OutputFormat::WebP;
            }
            self.extra.workspace = state;
        }
        if let Some(record) = record {
            self.extra.edits = record.recipe.edits.clone();
            self.extra.workspace.raw = record.recipe.raw.clone();
            self.extra.workspace.export = record.recipe.export.clone();
            self.extra.workspace.name = record.recipe.name.clone();
        }
        self.extra.workspace.mark_saved(&self.extra.edits);
        self.extra.edited = false;
        self.extra.workspace.preview_valid = self.current.is_some();
        self.extra.workspace.save_pending = false;
        if let Some(image) = &self.current {
            self.extra.width = image.width;
            self.extra.height = image.height;
            self.extra.workspace.crop = [0, 0, image.width, image.height];
        }
    }
    pub(super) fn schedule_edit_preview(&mut self) {
        if self.extra.workspace.source_key != self.selected.and_then(|i| self.item_key(i))
            || self.current_key.map(|k| k.0) != self.selected
            || self.current.is_none()
        {
            self.extra.workspace.preview_due = None;
            return;
        }
        if self
            .extra
            .workspace
            .preview_due
            .is_some_and(|t| t <= Instant::now())
            && self.extra.writing.is_none()
            && self.extra.pending_write.is_none()
        {
            self.extra.workspace.preview_due = None;
            self.render_edits();
        }
    }
    pub(super) fn accept_render_preview(&mut self, index: usize, output: worker::Output) {
        if self.selected != Some(index) {
            return;
        }
        let image = Arc::new(Pixels {
            width: output.reply.width,
            height: output.reply.height,
            rgba: output.rgba,
            preview: output.preview,
            preview_width: output.reply.preview_width,
            preview_height: output.reply.preview_height,
            note: output.reply.note,
            record: None,
        });
        let same = self
            .current
            .as_ref()
            .is_some_and(|p| (p.width, p.height) == (image.width, image.height));
        self.current = Some(image);
        self.current_key = Some((index, self.full_raw));
        self.overview = None;
        self.tiles.clear();
        self.tile_bytes = 0;
        self.extra.exposure_texture = None;
        if !same {
            self.reset_fit();
        }
        self.extra.workspace.preview_valid = true;
        self.status = "预览已更新；导出会重新从原始高位数据处理".into();
        if std::mem::take(&mut self.extra.workspace.save_pending) {
            self.save_edit_record();
        }
    }
    pub(super) fn save_edit_record(&mut self) {
        if self.current_key.map(|k| k.0) != self.selected || self.current.is_none() {
            self.status = "请等当前图像读取完成".into();
            return;
        }
        if !self.extra.workspace.preview_valid {
            self.extra.workspace.save_pending = true;
            self.render_edits();
            self.status = "预览验证通过后将自动保存无损记录".into();
            return;
        }
        let Some((source, entry)) = self.workspace_source() else {
            return;
        };
        let recipe = self.current_recipe();
        let result = (|| -> Result<(), String> {
            let path = recipes::record_path(&recipes::data_root()?, &source, entry.as_deref())?;
            EditRecord::capture(&source, entry, recipe)?.save(&path)?;
            Ok(())
        })();
        match result {
            Ok(()) => {
                self.extra.edits = self.document_edits();
                self.rotation = 0;
                self.flip_x = false;
                self.flip_y = false;
                self.extra.workspace.mark_saved(&self.extra.edits);
                self.extra.edited = false;
                self.cache.clear();
                if let Some(index) = self.selected {
                    self.invalidate_coverflow_preview(index);
                }
                self.render_edits();
                self.status = "已保存无损编辑记录；原始像素未改写".into();
            }
            Err(e) => self.error = Some(e),
        }
    }
    fn load_edit_record(&mut self) {
        let Some(path) = rfd::FileDialog::new()
            .set_title("读取无损编辑记录")
            .add_filter("RustView编辑记录", &["json"])
            .pick_file()
        else {
            return;
        };
        let result = EditRecord::load(&path).and_then(|r| {
            let (source, entry) = self.workspace_source().ok_or("没有当前图像")?;
            if r.source != source.canonicalize().map_err(|e| e.to_string())? || r.entry != entry {
                return Err("记录属于另一张图片，未套用".into());
            }
            Ok(r)
        });
        match result {
            Ok(record) => {
                self.extra.edits = record.recipe.edits;
                self.extra.workspace.raw = record.recipe.raw;
                self.extra.workspace.export = record.recipe.export;
                self.extra.workspace.mark_saved(&self.extra.edits);
                self.render_edits();
            }
            Err(e) => self.error = Some(e),
        }
    }
    pub(super) fn commit_workspace_tool(&mut self) {
        let s = &self.extra.workspace;
        let p = s.p;
        let edit = match s.tool {
            0 => Edit::Exposure { ev: p[0] },
            1 => Edit::Resize {
                width: self.extra.width,
                height: self.extra.height,
            },
            2 => Edit::Crop {
                x: s.crop[0],
                y: s.crop[1],
                width: s.crop[2],
                height: s.crop[3],
            },
            3 => Edit::Rotate {
                degrees: p[0],
                background: s.color,
            },
            4 => Edit::WhiteBalance {
                red: p[0],
                green: p[1],
                blue: p[2],
            },
            5 => Edit::Tone {
                brightness: p[0],
                contrast: p[1],
                gamma: p[2],
            },
            6 => Edit::Levels {
                black: p[0],
                white: p[1],
                gamma: p[2],
            },
            7 => Edit::Saturation { percent: p[0] },
            8 => Edit::Hue { degrees: p[0] },
            9 => Edit::ChannelMixer { matrix: p },
            10 => Edit::AutoLevels,
            11 => Edit::AutoContrast,
            12 => Edit::Sharpen {
                sigma: p[0],
                amount: p[1],
                threshold: p[2],
            },
            13 => Edit::Blur { sigma: p[0] },
            14 => Edit::Denoise {
                radius: p[0].round() as u8,
            },
            15 => Edit::Sepia { percent: p[0] },
            16 => Edit::Vignette { sigma: p[0] },
            17 => Edit::Negate,
            18 => Edit::Solarize { threshold: p[0] },
            19 => Edit::Posterize {
                levels: p[0] as u16,
            },
            20 => Edit::Emboss { sigma: p[0] },
            21 => Edit::Edge { radius: p[0] },
            22 => Edit::OilPaint { radius: p[0] },
            23 => Edit::Spread { radius: p[0] },
            24 => Edit::Swirl { degrees: p[0] },
            25 => Edit::Wave {
                amplitude: p[0],
                wavelength: p[1],
            },
            26 => Edit::LensDistortion {
                coefficients: [p[0], p[1], p[2], p[3]],
            },
            27 => Edit::Text {
                text: s.text.clone(),
                x: p[0] as i32,
                y: p[1] as i32,
                size: p[2] as u32,
                color: s.color,
            },
            28 => Edit::Watermark {
                path: PathBuf::from(&s.watermark),
                x: p[0] as i32,
                y: p[1] as i32,
                opacity: p[2],
                width: (p[3] > 0.0).then_some(p[3] as u32),
                height: None,
            },
            _ => Edit::Grayscale,
        };
        self.apply_edit(edit);
    }
    pub(super) fn edit_dialogs(&mut self, ctx: &egui::Context) {
        let Some(id) = self.extra.edit_dialog else {
            return;
        };
        let batch = matches!(id, 2043..=2046 | 10576);
        let mut apply = false;
        let mut save = false;
        let mut save_record = false;
        let mut load_record = false;
        let mut close = false;
        let mut save_preset = false;
        let mut load_preset = false;
        let mut remove_preset = false;
        egui::SidePanel::right("processing-workspace").default_width(320.0).width_range(260.0..=500.0).show(ctx,|ui|{
            ui.horizontal(|ui|{ui.heading(if matches!(id,10579|10556){"RAW调节"}else if batch{"批量处理"}else{"图像编辑"});if ui.button("返回查看").clicked(){close=true;}});
            if self.current.is_none()||self.current_key.map(|k|k.0)!=self.selected {ui.label("等待读取当前图像…");return;}
            egui::ScrollArea::vertical().show(ui,|ui|{
                if batch{ui.label(format!("所选 {} 项；逐个处理并保存副本",self.extra.workspace.batch_indices.len()));}
                ui.collapsing("无损记录与预设",|ui|{
                    ui.text_edit_singleline(&mut self.extra.workspace.name);
                    ui.horizontal(|ui|{if ui.button("保存编辑记录").clicked(){save_record=true;}if ui.button("读取记录…").clicked(){load_record=true;}});
                    let names=self.extra.workspace.presets.iter().map(|p|p.name.clone()).collect::<Vec<_>>();
                    egui::ComboBox::from_label("处理预设").selected_text(names.get(self.extra.workspace.selected_preset).cloned().unwrap_or_else(||"未选择".into())).show_ui(ui,|ui|{for(i,name)in names.iter().enumerate(){ui.selectable_value(&mut self.extra.workspace.selected_preset,i,name);}});
                    ui.horizontal(|ui|{if ui.button("保存预设").clicked(){save_preset=true;}if ui.button("套用").clicked(){load_preset=true;}if ui.button("删除预设").clicked(){remove_preset=true;}});
                });
                let raw=self.selected.and_then(|i|self.entries.get(i)).is_some_and(|e|crate::imaging::is_raw(&e.name));
                if raw{ui.collapsing("RAW解码（原始高位）",|ui|{
                    let r=&mut self.extra.workspace.raw;let mut changed=false;
                    changed|=ui.add(egui::Slider::new(&mut r.exposure_ev,-5.0..=5.0).text("曝光EV")).changed();changed|=ui.checkbox(&mut r.auto_bright,"自动亮度").changed();
                    changed|=ui.add(egui::Slider::new(&mut r.brightness,0.1..=3.0).text("亮度")).changed();
                    let mut custom=r.white_balance.is_some();if ui.checkbox(&mut custom,"自定义白平衡乘数").changed(){r.white_balance=custom.then_some([1.0;4]);changed=true;}
                    if let Some(wb)=&mut r.white_balance{for(i,name)in["红","绿1","蓝","绿2"].iter().enumerate(){changed|=ui.add(egui::Slider::new(&mut wb[i],0.1..=8.0).text(*name)).changed();}}
                    let old=r.output_color;egui::ComboBox::from_label("工作色域").selected_text(match r.output_color{2=>"Adobe RGB",4=>"ProPhoto RGB",_=>"sRGB"}).show_ui(ui,|ui|{ui.selectable_value(&mut r.output_color,1,"sRGB");ui.selectable_value(&mut r.output_color,2,"Adobe RGB");ui.selectable_value(&mut r.output_color,4,"ProPhoto RGB");});changed|=old!=r.output_color;
                    changed|=ui.add(egui::Slider::new(&mut r.highlight,0..=9).text("高光恢复")).changed();changed|=ui.add(egui::Slider::new(&mut r.noise_reduction,0..=2).text("RAW降噪")).changed();
                    let previous=r.demosaic;
                    egui::ComboBox::from_label("去马赛克").selected_text(match r.demosaic{0=>"Bilinear",1=>"VNG",2=>"PPG",4=>"DCB",11=>"DHT",12=>"AAHD",_=>"AHD"}).show_ui(ui,|ui|{for(value,label)in[(0,"Bilinear"),(1,"VNG"),(2,"PPG"),(3,"AHD"),(4,"DCB"),(11,"DHT"),(12,"AAHD")]{ui.selectable_value(&mut r.demosaic,value,label);}});
                    changed|=previous!=r.demosaic;
                    changed|=ui.add(egui::Slider::new(&mut r.gamma[0],0.05..=1.0).text("Gamma指数")).changed();
                    changed|=ui.add(egui::Slider::new(&mut r.gamma[1],1.0..=100.0).text("Gamma线性斜率")).changed();
                    if changed{self.extra.workspace.preview_due=Some(Instant::now()+std::time::Duration::from_millis(180));self.extra.workspace.preview_valid=false;self.extra.edited=true;}
                });}
                let old=self.extra.workspace.tool;
                egui::ComboBox::from_label("工具").selected_text(TOOLS[self.extra.workspace.tool]).show_ui(ui,|ui|{for(i,name)in TOOLS.iter().enumerate(){ui.selectable_value(&mut self.extra.workspace.tool,i,*name);}});
                if old!=self.extra.workspace.tool{self.extra.workspace.reset_parameters();}
                self.draw_tool_controls(ui);
                if ui.button("应用工具 (Ctrl+A)").clicked(){apply=true;}
                ui.horizontal(|ui|{if ui.button("撤销 Ctrl+Z").clicked(){self.undo_edit(false);}if ui.button("重做 Ctrl+Y").clicked(){self.undo_edit(true);}});
                ui.label(format!("{} 个处理步骤",self.extra.edits.len()));
                ui.collapsing("导出设置",|ui|{
                    let e=&mut self.extra.workspace.export;
                    egui::ComboBox::from_label("格式").selected_text(format!("{:?}",e.format)).show_ui(ui,|ui|{for f in[OutputFormat::Png,OutputFormat::Jpeg,OutputFormat::Tiff,OutputFormat::Bmp,OutputFormat::WebP,OutputFormat::Gif,OutputFormat::Avif]{ui.selectable_value(&mut e.format,f,format!("{f:?}"));}});
                    ui.horizontal(|ui|{ui.label("位深");ui.selectable_value(&mut e.depth,8,"8位");ui.selectable_value(&mut e.depth,16,"16位");});
                    ui.add(egui::Slider::new(&mut e.quality,1..=100).text("质量"));ui.checkbox(&mut e.preserve_metadata,"保留EXIF／其它元数据");
                    ui.horizontal(|ui|{ui.selectable_value(&mut e.animation,AnimationMode::CurrentFrame,"静态首帧");ui.selectable_value(&mut e.animation,AnimationMode::AllFrames,"完整动画");});
                    if batch{ui.label("输出命名模板");ui.text_edit_singleline(&mut self.extra.workspace.output_pattern);ui.horizontal(|ui|{ui.label("起始编号");ui.add(egui::DragValue::new(&mut self.extra.workspace.number).range(0..=u64::MAX));});ui.small("{name}原名；{n}序号；{ext}含点的新扩展名");}
                });
                if ui.button(if batch{"导出所选项目…"}else{"导出副本…"}).clicked(){save=true;}
                ui.small("显示预览为RGBA8；导出重新读取原始16位数据。记录只保存参数，不改写RAW像素。");
            });
        });
        if apply {
            self.commit_workspace_tool();
        }
        if save {
            self.save_edits(batch);
        }
        if save_record {
            self.save_edit_record();
        }
        if load_record {
            self.load_edit_record();
        }
        if save_preset || remove_preset {
            let result = recipes::data_root().and_then(|root| {
                let path = root.join("processing-presets.json");
                if save_preset {
                    recipes::save_preset(&path, self.current_recipe())?;
                } else if let Some(p) = self
                    .extra
                    .workspace
                    .presets
                    .get(self.extra.workspace.selected_preset)
                {
                    recipes::remove_preset(&path, &p.name)?;
                }
                self.extra.workspace.presets = recipes::load_presets(&path)?;
                Ok(())
            });
            if let Err(e) = result {
                self.error = Some(e);
            }
        }
        if load_preset {
            if let Some(p) = self
                .extra
                .workspace
                .presets
                .get(self.extra.workspace.selected_preset)
                .cloned()
            {
                self.extra.edits = p.edits;
                self.extra.workspace.raw = p.raw;
                self.extra.workspace.export = p.export;
                self.render_edits();
            }
        }
        if close {
            self.extra.edit_dialog = None;
        }
    }
    fn draw_tool_controls(&mut self, ui: &mut egui::Ui) {
        let tool = self.extra.workspace.tool;
        if tool == 1 {
            ui.horizontal(|ui| {
                ui.label("宽");
                let w = ui
                    .add(egui::DragValue::new(&mut self.extra.width).range(1..=100000))
                    .changed();
                ui.label("高");
                let h = ui
                    .add(egui::DragValue::new(&mut self.extra.height).range(1..=100000))
                    .changed();
                if self.extra.workspace.keep_aspect {
                    if let Some(p) = &self.current {
                        if w {
                            self.extra.height = ((self.extra.width as f64 * p.height as f64
                                / p.width as f64)
                                .round() as u32)
                                .max(1);
                        } else if h {
                            self.extra.width = ((self.extra.height as f64 * p.width as f64
                                / p.height as f64)
                                .round() as u32)
                                .max(1);
                        }
                    }
                }
            });
            ui.checkbox(&mut self.extra.workspace.keep_aspect, "保持比例");
            return;
        }
        if tool == 2 {
            for (i, name) in ["左", "上", "宽", "高"].iter().enumerate() {
                ui.horizontal(|ui| {
                    ui.label(*name);
                    ui.add(
                        egui::DragValue::new(&mut self.extra.workspace.crop[i]).range(if i < 2 {
                            0..=100000
                        } else {
                            1..=100000
                        }),
                    );
                });
            }
            if ui.button("使用图像上的选区").clicked() {
                if let (Some(r), Some(image)) = (self.extra.selection_rect, &self.current) {
                    let d = if self.rotation % 2 == 0 {
                        egui::vec2(image.width as f32, image.height as f32)
                    } else {
                        egui::vec2(image.height as f32, image.width as f32)
                    };
                    let bounds = Rect::from_center_size(
                        self.extra.viewport.center() + self.pan,
                        d * self.scale,
                    );
                    let b = r.intersect(bounds);
                    if b.is_positive() {
                        self.extra.workspace.crop = [
                            ((b.min.x - bounds.min.x) / self.scale).floor().max(0.0) as u32,
                            ((b.min.y - bounds.min.y) / self.scale).floor().max(0.0) as u32,
                            (b.width() / self.scale).round().max(1.0) as u32,
                            (b.height() / self.scale).round().max(1.0) as u32,
                        ];
                    }
                }
                self.extra.tool = 2372;
            }
            return;
        }
        if tool == 3 {
            ui.horizontal(|ui| {
                for (name, e) in [
                    ("左90°", Edit::Rotate90 { turns: 3 }),
                    ("右90°", Edit::Rotate90 { turns: 1 }),
                    ("水平翻转", Edit::FlipH),
                    ("垂直翻转", Edit::FlipV),
                ] {
                    if ui.button(name).clicked() {
                        self.apply_edit(e);
                    }
                }
            });
        }
        let labels: &[(&str, f32, f32)] = match tool {
            0 => &[("EV", -5.0, 5.0)],
            3 => &[("角度", -180.0, 180.0)],
            4 => &[("红", 0.1, 8.0), ("绿", 0.1, 8.0), ("蓝", 0.1, 8.0)],
            5 => &[
                ("亮度", -100.0, 100.0),
                ("对比", -100.0, 100.0),
                ("Gamma", 0.1, 5.0),
            ],
            6 => &[
                ("黑点%", 0.0, 99.0),
                ("白点%", 1.0, 100.0),
                ("Gamma", 0.1, 5.0),
            ],
            7 => &[("百分比", 0.0, 200.0)],
            15 => &[("百分比", 0.0, 100.0)],
            8 => &[("色相角度", -180.0, 180.0)],
            9 => &[
                ("R←R", -3.0, 3.0),
                ("R←G", -3.0, 3.0),
                ("R←B", -3.0, 3.0),
                ("G←R", -3.0, 3.0),
                ("G←G", -3.0, 3.0),
                ("G←B", -3.0, 3.0),
                ("B←R", -3.0, 3.0),
                ("B←G", -3.0, 3.0),
                ("B←B", -3.0, 3.0),
            ],
            12 => &[("Sigma", 0.1, 20.0), ("强度", 0.0, 5.0), ("阈值", 0.0, 1.0)],
            13 | 20 => &[("Sigma", 0.1, 30.0)],
            16 => &[("Sigma", 1.0, 30.0)],
            14 => &[("半径", 1.0, 3.0)],
            18 => &[("阈值%", 0.0, 100.0)],
            19 => &[("层数", 2.0, 256.0)],
            21 | 22 | 23 => &[("半径", 0.1, 10.0)],
            24 => &[("角度", -360.0, 360.0)],
            25 => &[("振幅", 0.0, 100.0), ("波长", 1.0, 1000.0)],
            26 => &[
                ("k1", -1.0, 1.0),
                ("k2", -1.0, 1.0),
                ("k3", -1.0, 1.0),
                ("比例", 0.1, 2.0),
            ],
            27 => &[
                ("X", 0.0, 10000.0),
                ("Y", 0.0, 10000.0),
                ("字号", 4.0, 500.0),
            ],
            28 => &[
                ("X", 0.0, 10000.0),
                ("Y", 0.0, 10000.0),
                ("不透明度", 0.0, 1.0),
                ("宽度(0原大小)", 0.0, 10000.0),
            ],
            _ => &[],
        };
        for (i, (name, min, max)) in labels.iter().enumerate() {
            ui.add(egui::Slider::new(&mut self.extra.workspace.p[i], *min..=*max).text(*name));
        }
        if tool == 27 {
            ui.text_edit_multiline(&mut self.extra.workspace.text);
        }
        if matches!(tool, 3 | 27) {
            ui.color_edit_button_srgba_unmultiplied(&mut self.extra.workspace.color);
        }
        if tool == 28 {
            ui.text_edit_singleline(&mut self.extra.workspace.watermark);
            if ui.button("选择水印图片…").clicked() {
                if let Some(p) = rfd::FileDialog::new().pick_file() {
                    self.extra.workspace.watermark = p.to_string_lossy().into_owned();
                }
            }
        }
    }
    pub(super) fn save_edits(&mut self, batch: bool) {
        if !batch && (self.current_key.map(|k| k.0) != self.selected || self.current.is_none()) {
            self.status = "请等当前图像读取完成".into();
            return;
        }
        let recipe = self.current_recipe();
        let indices = if batch {
            self.extra.workspace.batch_indices.clone()
        } else {
            self.selected.into_iter().collect()
        };
        let requests = indices
            .into_iter()
            .filter_map(|i| {
                self.make_view_load(i, true)
                    .map(|r| (r, self.entries[i].name.clone()))
            })
            .collect::<Vec<_>>();
        if requests.is_empty() {
            return;
        }
        let mut settings = recipe.export.clone();
        if let Some(mode) = recipe.edits.iter().rev().find_map(|e| {
            if let Edit::ColorDepth { mode } = e {
                Some(*mode)
            } else {
                None
            }
        }) {
            match mode {
                crate::editing::ColorDepth::Rgb15 => settings.format = OutputFormat::Bmp,
                crate::editing::ColorDepth::BlackWhite1
                | crate::editing::ColorDepth::Gray4
                | crate::editing::ColorDepth::Gray8
                | crate::editing::ColorDepth::Indexed4
                | crate::editing::ColorDepth::Indexed8 => settings.format = OutputFormat::Png,
                _ => {}
            }
        }
        let extension = settings.format.extension();
        let folder = if batch {
            let Some(folder) = rfd::FileDialog::new()
                .set_title("批量导出到文件夹")
                .pick_folder()
            else {
                return;
            };
            Some(folder)
        } else {
            None
        };
        let single = if !batch {
            let stem = Path::new(&requests[0].1)
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("image");
            let Some(p) = rfd::FileDialog::new()
                .set_title("高位导出副本")
                .set_file_name(format!("{stem}-edited.{extension}"))
                .save_file()
            else {
                return;
            };
            settings.format = match OutputFormat::from_path(&p) {
                Ok(f) => f,
                Err(e) => {
                    self.error = Some(e);
                    return;
                }
            };
            Some(p)
        } else {
            None
        };
        let pattern = self.extra.workspace.output_pattern.clone();
        let start = self.extra.workspace.number;
        self.write_job_no_refresh(Box::new(move || {
            let mut plans = Vec::new();
            let mut destinations = std::collections::HashSet::new();
            for (i, (source, name)) in requests.into_iter().enumerate() {
                let path = if let Some(path) = &single {
                    path.clone()
                } else {
                    let stem = Path::new(&name)
                        .file_stem()
                        .and_then(|s| s.to_str())
                        .unwrap_or("image");
                    let base = pattern
                        .replace("{name}", stem)
                        .replace("{n}", &format!("{:04}", start.saturating_add(i as u64)))
                        .replace("{ext}", &format!(".{}", settings.format.extension()));
                    if base.is_empty()
                        || base.contains(['/', '\\', ':', '\0'])
                        || base.contains(['{', '}'])
                    {
                        return Err("输出模板包含未知字段或无效路径字符".into());
                    }
                    folder.as_ref().unwrap().join(base)
                };
                if !destinations.insert(path.to_string_lossy().to_lowercase()) {
                    return Err("输出模板会产生重名文件，尚未处理任何图片".into());
                }
                if batch && path.exists() {
                    return Err(format!("目标已存在，尚未处理任何图片：{}", path.display()));
                }
                plans.push((source, path));
            }
            let mut count = 0;
            for (source, path) in plans {
                let out = worker::execute(
                    Request::Render {
                        source: Box::new(source),
                        edits: recipe.edits.clone(),
                        settings: settings.clone(),
                        raw: recipe.raw.clone(),
                        save: Some(path),
                    },
                    Arc::new(AtomicBool::new(false)),
                )?;
                if let Some(e) = out.reply.error {
                    return Err(format!("已完成{count}项；{e}"));
                }
                count += 1;
            }
            Ok(format!("已从原始数据导出 {count} 项"))
        }));
    }
}

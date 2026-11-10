use super::*;
use crate::legacy_settings::{self, ImportReport, ShortcutBinding};
use crate::native_keys::NativeKeyEvent;

pub(super) struct State {
    pub open: bool,
    loaded: bool,
    active: ImportReport,
    preview: Option<ImportReport>,
    error: Option<String>,
    table: u16,
    command: u16,
    draft: ShortcutBinding,
    capture: bool,
    search: String,
    saved: bool,
}
impl Default for State {
    fn default() -> Self {
        Self {
            open: false,
            loaded: false,
            active: ImportReport::default(),
            preview: None,
            error: None,
            table: 2,
            command: 2102,
            draft: ShortcutBinding {
                table: 2,
                virtual_key: true,
                modifiers: 1,
                key: 0x4f,
                command: 2102,
            },
            capture: false,
            search: String::new(),
            saved: true,
        }
    }
}

fn config_path() -> Option<PathBuf> {
    std::env::current_exe()
        .ok()?
        .parent()
        .map(|p| p.join("RustView-data/settings.json"))
}
fn name(command: u16) -> String {
    command_names()
        .iter()
        .find(|(id, _)| *id == command)
        .map(|(_, n)| n.clone())
        .unwrap_or_else(|| format!("命令 {command}"))
}
fn command_names() -> &'static Vec<(u16, String)> {
    static NAMES: std::sync::OnceLock<Vec<(u16, String)>> = std::sync::OnceLock::new();
    NAMES.get_or_init(|| {
        include_str!("../../docs/retained-command-audit.csv")
            .lines()
            .skip(1)
            .filter_map(|line| {
                let mut parts = line.split(',');
                let id = parts.next()?.parse().ok()?;
                let title = parts.next()?.trim_matches('"').to_owned();
                shortcuts::allowed_command(id).then_some((id, title))
            })
            .collect()
    })
}
fn bits(m: egui::Modifiers) -> u8 {
    u8::from(m.ctrl)
        | (u8::from(m.shift) << 1)
        | (u8::from(m.alt) << 2)
        | (u8::from(m.mac_cmd) << 3)
}
fn label(binding: &ShortcutBinding) -> String {
    format!(
        "{}{}{}{}{:04X}",
        if binding.modifiers & 1 != 0 {
            "Ctrl+"
        } else {
            ""
        },
        if binding.modifiers & 2 != 0 {
            "Shift+"
        } else {
            ""
        },
        if binding.modifiers & 4 != 0 {
            "Alt+"
        } else {
            ""
        },
        if binding.virtual_key {
            "VK 0x"
        } else {
            "字符 U+"
        },
        binding.key
    )
}
fn key_equal(a: &ShortcutBinding, b: &ShortcutBinding) -> bool {
    a.table == b.table
        && a.virtual_key == b.virtual_key
        && a.modifiers == b.modifiers
        && a.key == b.key
}

fn action_override(
    report: &ImportReport,
    events: &[NativeKeyEvent],
    table: u16,
) -> Option<Option<Action>> {
    let complete = report.replace_tables.contains(&table);
    let mut pressed = false;
    for event in events {
        let key = match event {
            NativeKeyEvent::Key {
                virtual_key,
                pressed: true,
                modifiers,
                ..
            } => Some((true, *virtual_key, bits(*modifiers))),
            NativeKeyEvent::Char {
                utf16, modifiers, ..
            } => Some((false, *utf16, bits(*modifiers))),
            _ => None,
        };
        let Some((virtual_key, key, modifiers)) = key else {
            continue;
        };
        pressed = true;
        if let Some(binding) = report.bindings.iter().find(|b| {
            b.table == table
                && b.virtual_key == virtual_key
                && b.key == key
                && b.modifiers == modifiers
        }) {
            return Some(shortcuts::action_for_command(binding.command));
        }
    }
    if complete && pressed {
        Some(None)
    } else {
        None
    }
}

impl Viewer {
    pub(super) fn load_settings(&mut self) {
        if self.extra.settings_ui.loaded {
            return;
        }
        self.extra.settings_ui.loaded = true;
        if let Some(path) = config_path().filter(|p| p.is_file()) {
            match legacy_settings::import_file(&path) {
                Ok(report) => {
                    self.apply_visual_settings(&report);
                    self.extra.settings_ui.active = report;
                }
                Err(e) => self.extra.settings_ui.error = Some(e),
            }
        }
    }
    fn apply_visual_settings(&mut self, report: &ImportReport) {
        for (key, value) in &report.settings {
            let Some(value) = value.as_bool() else {
                continue;
            };
            match key.as_str() {
                "rustview.toolbar" => self.extra.toolbar = value,
                "rustview.statusbar" => self.extra.statusbar = value,
                "rustview.color_managed" => self.extra.color_managed = value,
                "rustview.zoom_lock" => self.extra.zoom_lock = value,
                "rustview.pan_lock" => self.extra.pan_lock = value,
                _ => {}
            }
        }
    }
    /// Some(None) consumes a key from a complete imported table even when its default was removed.
    pub(super) fn custom_native_action(
        &mut self,
        events: &[NativeKeyEvent],
        table: u16,
    ) -> Option<Option<Action>> {
        self.load_settings();
        let state = &mut self.extra.settings_ui;
        if state.capture {
            for event in events {
                if let NativeKeyEvent::Key {
                    virtual_key,
                    pressed: true,
                    modifiers,
                    ..
                } = event
                {
                    if matches!(*virtual_key,0x10..=0x12|0x5b|0x5c|0xa0..=0xa5) {
                        continue;
                    }
                    if bits(*modifiers) & 8 != 0 {
                        continue;
                    }
                    state.draft.key = *virtual_key;
                    state.draft.modifiers = bits(*modifiers);
                    state.draft.virtual_key = true;
                    state.capture = false;
                    break;
                }
            }
            return Some(None);
        }
        action_override(&state.active, events, table)
    }

    pub(super) fn draw_settings(&mut self, ctx: &egui::Context) {
        self.load_settings();
        if !self.extra.settings_ui.open {
            return;
        }
        let previous_color = self.extra.color_managed;
        let mut open = true;
        let mut apply = false;
        let mut save = false;
        let mut import = false;
        let mut inspect = false;
        let mut export = false;
        let mut clear_cache = false;
        let state = &mut self.extra.settings_ui;
        egui::Window::new("选项与快捷键").open(&mut open).default_width(780.0).show(ctx,|ui|{
            ui.horizontal(|ui|{ui.checkbox(&mut self.extra.toolbar,"工具栏");ui.checkbox(&mut self.extra.statusbar,"状态栏");ui.checkbox(&mut self.extra.color_managed,"色彩管理");ui.checkbox(&mut self.extra.zoom_lock,"缩放锁定");ui.checkbox(&mut self.extra.pan_lock,"平移锁定");});
            clear_cache=ui.button("清理剪贴板导出缓存…").clicked();
            if ui.button("将当前图像预览设为平铺背景").clicked(){if let Some(image)=&self.current{if !image.preview.is_empty(){self.extra.background_tile=Some(ctx.load_texture("background-tile",egui::ColorImage::from_rgba_unmultiplied([image.preview_width as usize,image.preview_height as usize],&image.preview),egui::TextureOptions{wrap_mode:egui::TextureWrapMode::Repeat,..egui::TextureOptions::LINEAR}));}}}
            if ui.button("重设背景平铺 (Ctrl+Shift+G)").clicked(){self.extra.background_tile=None;}
            ui.separator();
            ui.horizontal(|ui|{import=ui.button("导入 JSON / REG…").clicked();inspect=ui.button("只读检查原 ACDSee 用户配置").clicked();export=ui.button("导出当前配置…").clicked();});
            ui.small("只导入已确认的管理 / 查看 / 比较模式键位；被排除功能不会启用。未知 MFC 二进制会保留为未识别摘要，不猜测导入。");
            if let Some(report)=&state.preview {
                ui.group(|ui|{
                    let usable=report.bindings.iter().filter(|b|shortcuts::allowed_command(b.command)).count();
                    ui.label(format!("导入预览：{} 个可用键位，{} 项设置，{} 项未识别记录",usable,report.settings.len(),report.ignored.len()));
                    egui::ScrollArea::vertical().id_salt("import-preview").max_height(170.0).show(ui,|ui|{
                        for path in &report.sources{ui.small(path.display().to_string());}
                        for b in &report.bindings{ui.label(format!("表 {}　{} → {}{}",b.table,label(b),name(b.command),if shortcuts::allowed_command(b.command){""}else{"（排除，不激活）"}));}
                        for(key,value)in &report.settings{ui.small(format!("{key} = {value}"));}
                        for message in &report.ignored{ui.colored_label(Color32::YELLOW,message);}
                    });
                    apply=ui.button("应用这些已识别键位").clicked();
                });
            }
            ui.horizontal(|ui|{ui.label("模式");ui.selectable_value(&mut state.table,2,"管理");ui.selectable_value(&mut state.table,552,"查看");ui.selectable_value(&mut state.table,553,"比较");ui.label("筛选");ui.text_edit_singleline(&mut state.search);});
            let default={static DEFAULT:std::sync::OnceLock<Vec<ShortcutBinding>>=std::sync::OnceLock::new();DEFAULT.get_or_init(shortcuts::default_bindings).clone()};
            let bindings=if state.active.replace_tables.contains(&state.table){state.active.bindings.iter().filter(|b|b.table==state.table).cloned().collect::<Vec<_>>()}else{
                let mut b=default.into_iter().filter(|b|b.table==state.table).collect::<Vec<_>>();for custom in state.active.bindings.iter().filter(|b|b.table==state.table){b.retain(|b|!key_equal(b,custom));b.push(custom.clone());}b
            };
            let mut remove=None;
            egui::ScrollArea::vertical().id_salt("key-settings").max_height(210.0).show(ui,|ui|{
                for binding in &bindings {
                    if !shortcuts::allowed_command(binding.command){continue;}
                    let title=name(binding.command);if !state.search.is_empty()&&!title.to_lowercase().contains(&state.search.to_lowercase())&&!label(binding).to_lowercase().contains(&state.search.to_lowercase()){continue;}
                    ui.horizontal(|ui|{ui.monospace(label(binding));ui.label(title);if ui.small_button("修改").clicked(){state.command=binding.command;state.draft=binding.clone();state.capture=true;}if ui.small_button("删除").clicked(){remove=Some(binding.clone());}});
                }
            });
            if let Some(binding)=remove{
                if !state.active.replace_tables.contains(&state.table){state.active.bindings.retain(|b|b.table!=state.table);state.active.bindings.extend(bindings);state.active.replace_tables.push(state.table);}
                state.active.bindings.retain(|b|!key_equal(b,&binding));state.saved=false;
            }
            ui.separator();
            egui::ComboBox::from_id_salt("shortcut-command").selected_text(name(state.command)).width(480.0).show_ui(ui,|ui|{for(id,title)in command_names(){ui.selectable_value(&mut state.command,*id,format!("{id}: {title}"));}});
            ui.horizontal(|ui|{
                if ui.button(if state.capture{"请按快捷键…"}else{"录入快捷键"}).clicked(){state.capture=!state.capture;}
                ui.label(label(&state.draft));
                if ui.button("指定给此命令").clicked(){
                    state.draft.table=state.table;state.draft.command=state.command;
                    state.active.bindings.retain(|b|!key_equal(b,&state.draft));state.active.bindings.push(state.draft.clone());state.saved=false;
                }
                if ui.button("恢复本模式默认键位").clicked(){state.active.bindings.retain(|b|b.table!=state.table);state.active.replace_tables.retain(|t|*t!=state.table);state.saved=false;}
            });
            if !state.saved{ui.small("快捷键已在本次运行生效，点击保存后下次仍保留。");}
            save=ui.button("保存选项与快捷键").clicked();
            if let Some(error)=&state.error{ui.colored_label(Color32::LIGHT_RED,error);}
        });
        self.extra.settings_ui.open = open;
        if clear_cache {
            self.clear_clipboard_exports();
        }
        if apply {
            if let Some(report) = self.extra.settings_ui.preview.take() {
                self.apply_visual_settings(&report);
                self.extra.settings_ui.active = report;
                self.extra.settings_ui.saved = false;
            }
        }
        if inspect {
            match legacy_settings::inspect_current_user() {
                Ok(r) => self.extra.settings_ui.preview = Some(r),
                Err(e) => self.extra.settings_ui.error = Some(e),
            }
        }
        if import {
            if let Some(path) = rfd::FileDialog::new()
                .add_filter("设置文件", &["json", "reg"])
                .pick_file()
            {
                match legacy_settings::import_file(&path) {
                    Ok(r) => self.extra.settings_ui.preview = Some(r),
                    Err(e) => self.extra.settings_ui.error = Some(e),
                }
            }
        }
        if save || export {
            let mut report = self.extra.settings_ui.active.clone();
            for (key, value) in [
                ("rustview.toolbar", self.extra.toolbar),
                ("rustview.statusbar", self.extra.statusbar),
                ("rustview.color_managed", self.extra.color_managed),
                ("rustview.zoom_lock", self.extra.zoom_lock),
                ("rustview.pan_lock", self.extra.pan_lock),
            ] {
                report.settings.insert(key.into(), value.into());
            }
            let path = if export {
                rfd::FileDialog::new()
                    .set_file_name("RustView-settings.json")
                    .save_file()
            } else {
                config_path()
            };
            if let Some(path) = path {
                let result = path
                    .parent()
                    .map(std::fs::create_dir_all)
                    .transpose()
                    .map_err(|e| e.to_string())
                    .and_then(|_| legacy_settings::export_file(&path, &report));
                match result {
                    Ok(()) => {
                        self.extra.settings_ui.active = report;
                        self.extra.settings_ui.saved = true;
                        self.status = format!("设置已保存：{}", path.display());
                    }
                    Err(e) => self.extra.settings_ui.error = Some(e),
                }
            }
        }
        if self.extra.color_managed != previous_color {
            self.cache = ImageCache::default();
            if let Some(i) = self.selected {
                self.load_index(i, self.full_raw);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn key(virtual_key: u16) -> NativeKeyEvent {
        NativeKeyEvent::Key {
            virtual_key,
            scan_code: 0,
            extended: false,
            pressed: true,
            repeat: false,
            modifiers: egui::Modifiers::default(),
        }
    }
    #[test]
    fn removed_defaults_do_not_return_and_other_modes_still_fall_back() {
        let report = ImportReport {
            replace_tables: vec![2],
            ..Default::default()
        };
        assert_eq!(action_override(&report, &[key(0x4f)], 2), Some(None));
        assert_eq!(action_override(&report, &[key(0x4f)], 552), None);
    }
    #[test]
    fn character_binding_survives_its_unmatched_virtual_key_message() {
        let report = ImportReport {
            bindings: vec![ShortcutBinding {
                table: 552,
                virtual_key: false,
                modifiers: 0,
                key: 42,
                command: 2238,
            }],
            replace_tables: vec![552],
            ..Default::default()
        };
        let events = [
            key(0x38),
            NativeKeyEvent::Char {
                utf16: 42,
                repeat: false,
                modifiers: egui::Modifiers::default(),
            },
        ];
        assert_eq!(
            action_override(&report, &events, 552),
            Some(Some(Action::ZoomIn))
        );
    }
    #[test]
    fn excluded_commands_cannot_be_reenabled_by_an_import() {
        let report = ImportReport {
            bindings: vec![ShortcutBinding {
                table: 2,
                virtual_key: true,
                modifiers: 0,
                key: 0x53,
                command: 2042,
            }],
            ..Default::default()
        };
        assert_eq!(action_override(&report, &[key(0x53)], 2), Some(None));
    }
    #[test]
    fn modifiers_and_contexts_distinguish_bindings() {
        let b = ShortcutBinding {
            table: 2,
            virtual_key: true,
            modifiers: 1,
            key: 0x4f,
            command: 2102,
        };
        let mut c = b.clone();
        c.table = 552;
        assert!(!key_equal(&b, &c));
        c = b.clone();
        c.modifiers = 3;
        assert!(!key_equal(&b, &c));
    }
}

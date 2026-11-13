use super::*;

#[derive(Clone, Copy)]
enum Phase {
    Loading,
    PhotoDrawn,
    FontsQueued(u64),
}

pub(super) struct ImageStartup {
    serial: u64,
    phase: Phase,
}

impl ImageStartup {
    pub(super) fn new(serial: u64) -> Self {
        Self {
            serial,
            phase: Phase::Loading,
        }
    }
}

impl Viewer {
    #[cfg(windows)]
    pub(super) fn skip_open_transition(&mut self) {
        if set_window_transitions(self.extra.hwnd, true) {
            // Keep the newly created window's first show out of the DWM animation.
            // The first egui passes can finish before the compositor shows the window.
            self.restore_window_transition_at =
                Some(Instant::now() + std::time::Duration::from_millis(500));
        }
    }

    pub(super) fn advance_startup(&mut self, ctx: &egui::Context) {
        #[cfg(windows)]
        if let Some(deadline) = self.restore_window_transition_at {
            if let Some(delay) = deadline.checked_duration_since(Instant::now()) {
                ctx.request_repaint_after(delay);
            } else {
                set_window_transitions(self.extra.hwnd, false);
                self.restore_window_transition_at = None;
            }
        }
        let Some(startup) = &mut self.startup_image else {
            return;
        };
        match startup.phase {
            Phase::FontsQueued(pass) if ctx.cumulative_pass_nr() != pass => {
                self.startup_image = None;
                return;
            }
            Phase::PhotoDrawn if ctx.output(|output| output.num_completed_passes == 0) => {
                self.finish_startup_ui(ctx);
                return;
            }
            _ => {}
        }
    }

    pub(super) fn finish_startup_ui(&mut self, ctx: &egui::Context) {
        let Some(startup) = &mut self.startup_image else {
            return;
        };
        if matches!(startup.phase, Phase::FontsQueued(_)) {
            return;
        }
        // egui activates the new definitions at the beginning of the next pass.
        startup.phase = Phase::FontsQueued(ctx.cumulative_pass_nr());
        configure_fonts(ctx);
        self.initialize_catalog();
        ctx.request_repaint();
    }

    pub(super) fn draw_direct_startup(&mut self, ctx: &egui::Context) -> bool {
        let Some(startup) = &self.startup_image else {
            return false;
        };
        if startup.serial != self.serial
            || self.error.is_some()
            || self.browser
            || self.password_dialog
        {
            self.finish_startup_ui(ctx);
        }
        egui::CentralPanel::default()
            .frame(egui::Frame::NONE)
            .show(ctx, |ui| self.draw_image(ui, ctx));
        if self.pending.is_some() || self.running.is_some() || self.extra.animation.is_some() {
            egui::Area::new(egui::Id::new("direct-startup-controls"))
                .anchor(egui::Align2::RIGHT_BOTTOM, egui::vec2(-12.0, -12.0))
                .show(ctx, |ui| {
                    ui.horizontal(|ui| {
                        ui.spinner();
                        if ui.small_button("Cancel").clicked() {
                            self.cancel_current_read();
                            self.finish_startup_ui(ctx);
                        }
                    });
                });
        }
        let startup = self.startup_image.as_mut().unwrap();
        if matches!(startup.phase, Phase::Loading)
            && (self.current.is_some() || self.extra.presentation.has_frame())
        {
            startup.phase = Phase::PhotoDrawn;
            ctx.request_repaint();
        }
        true
    }
}

#[cfg(windows)]
fn set_window_transitions(hwnd: usize, disabled: bool) -> bool {
    use windows_sys::Win32::Graphics::Dwm::{
        DwmSetWindowAttribute, DWMWA_TRANSITIONS_FORCEDISABLED,
    };

    let value = i32::from(disabled);
    unsafe {
        DwmSetWindowAttribute(
            hwnd as _,
            DWMWA_TRANSITIONS_FORCEDISABLED as u32,
            std::ptr::from_ref(&value).cast(),
            std::mem::size_of_val(&value) as u32,
        ) >= 0
    }
}

pub(super) fn configure_fonts(ctx: &egui::Context) {
    for path in [
        "C:/Windows/Fonts/msyh.ttc",
        "C:/Windows/Fonts/msyh.ttf",
        "C:/Windows/Fonts/simhei.ttf",
    ] {
        let Ok(data) = std::fs::read(path) else {
            continue;
        };
        let mut fonts = egui::FontDefinitions::default();
        fonts
            .font_data
            .insert("chinese".into(), Arc::new(egui::FontData::from_owned(data)));
        for family in [egui::FontFamily::Proportional, egui::FontFamily::Monospace] {
            fonts
                .families
                .entry(family)
                .or_default()
                .push("chinese".into());
        }
        ctx.set_fonts(fonts);
        break;
    }
}

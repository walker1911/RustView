use super::*;
use crate::animation_player::{Event as AnimationEvent, Packet, Session};
use std::time::Duration;

fn toggle_pause_clock(
    paused: &mut bool,
    deadline: &mut Instant,
    remaining: &mut Duration,
    now: Instant,
) {
    if *paused {
        *deadline = now + *remaining;
        *paused = false;
    } else {
        *remaining = deadline.saturating_duration_since(now);
        *paused = true;
    }
}

impl Viewer {
    pub(super) fn stop_animation(&mut self) {
        if let Some(session) = self.extra.animation.take() {
            session.cancel();
            self.extra.retiring = Some(session);
        }
        self.extra.animation_pending = None;
        self.extra.animation_operation = None;
        self.extra.animation_texture = None;
        self.extra.animated = false;
        self.extra.animation_paused = false;
        self.extra.animation_ended = false;
        self.extra.animation_end_pending = false;
    }
    pub(super) fn is_animation_candidate(request: &Request) -> bool {
        if let Request::ColorPreview { source, .. } | Request::Unedited { source } = request {
            return Self::is_animation_candidate(source);
        }
        if let Request::Load { path, entry, .. } = request {
            let path = entry.as_deref().map(Path::new).unwrap_or(path);
            matches!(
                path.extension()
                    .and_then(|e| e.to_str())
                    .unwrap_or("")
                    .to_ascii_lowercase()
                    .as_str(),
                "gif" | "webp"
            )
        } else {
            false
        }
    }
    pub(super) fn start_animation(&mut self, task: Task, ctx: &egui::Context) {
        self.reset_comparison();
        self.extra.animation_operation = Some((task.request.clone(), task.operation));
        self.extra.animation_deadline = Instant::now();
        self.extra.animation = Some(Session::start(task.request, ctx.clone()));
    }
    pub(super) fn toggle_animation(&mut self) {
        if self.extra.animation_ended {
            if let Some((request, operation)) = self.extra.animation_operation.clone() {
                self.current_key = None;
                self.queue(request, operation);
            } else if let Some(i) = self.selected {
                self.load_index(i, self.full_raw);
            }
        } else if self.extra.animated {
            toggle_pause_clock(
                &mut self.extra.animation_paused,
                &mut self.extra.animation_deadline,
                &mut self.extra.frame_duration,
                Instant::now(),
            );
            self.status = if self.extra.animation_paused {
                "动画已暂停"
            } else {
                "动画播放中"
            }
            .into();
        }
    }
    pub(super) fn display_animation_frame(&mut self, packet: Packet, ctx: &egui::Context) {
        self.extra.exposure_texture = None;
        let Some((
            request,
            Operation::Load {
                index, full_raw, ..
            },
        )) = self.extra.animation_operation.clone()
        else {
            return;
        };
        let first = self.current_key != Some((index, full_raw));
        let output = packet.output;
        let full_texture = output.reply.width <= self.extra.max_texture
            && output.reply.height <= self.extra.max_texture;
        let mut note = output.reply.note;
        if packet.animated && !full_texture {
            note.push_str(" · 超大画布播放预览，暂停后可放大查看原始细节");
        }
        let image = Arc::new(Pixels {
            width: output.reply.width,
            height: output.reply.height,
            rgba: output.rgba,
            preview: output.preview,
            preview_width: output.reply.preview_width,
            preview_height: output.reply.preview_height,
            note,
            record: output.reply.editing_record.map(Arc::new),
        });
        if first {
            self.set_current(index, full_raw, image.clone());
        } else {
            self.current = Some(image.clone());
            self.tiles.clear();
            self.tile_bytes = 0;
        }
        self.extra.animated = packet.animated;
        if !packet.animated {
            if !matches!(request, Request::Unedited { .. }) {
                self.cache.insert((index, full_raw), image, false);
            }
            self.stop_animation();
            return;
        }
        let pixels = egui::ColorImage::from_rgba_unmultiplied(
            [image.preview_width as usize, image.preview_height as usize],
            &image.preview,
        );
        if let Some(t) = &mut self.overview {
            t.set(pixels, egui::TextureOptions::LINEAR)
        } else {
            self.overview =
                Some(ctx.load_texture("animation-overview", pixels, egui::TextureOptions::LINEAR));
        }
        if full_texture {
            let pixels = egui::ColorImage::from_rgba_unmultiplied(
                [image.width as usize, image.height as usize],
                &image.rgba,
            );
            if let Some(t) = &mut self.extra.animation_texture {
                t.set(pixels, egui::TextureOptions::LINEAR)
            } else {
                self.extra.animation_texture =
                    Some(ctx.load_texture("animation-frame", pixels, egui::TextureOptions::LINEAR));
            }
        } else {
            self.extra.animation_texture = None;
        }
        self.extra.frame_duration = Duration::from_millis(packet.duration_ms.max(10) as u64);
        self.extra.animation_deadline = Instant::now() + self.extra.frame_duration;
        self.status = image.note.clone();
        if let Some(s) = &self.extra.animation {
            s.next();
        }
    }
    pub(super) fn poll_animation(&mut self, ctx: &egui::Context) {
        if self.extra.retiring.as_ref().is_some_and(|s| s.finished()) {
            self.extra.retiring = None;
        }
        let event = self.extra.animation.as_ref().and_then(|s| s.try_recv());
        match event {
            Some(AnimationEvent::Frame(packet)) => self.extra.animation_pending = Some(packet),
            Some(AnimationEvent::End) => self.extra.animation_end_pending = true,
            Some(AnimationEvent::Error {
                message,
                password_required,
            }) => {
                let retry = self.extra.animation_operation.clone();
                self.stop_animation();
                self.status = message.clone();
                if password_required {
                    self.retry = retry;
                    self.password_dialog = true;
                    self.password_focus = true;
                    self.password_input.clear();
                } else {
                    self.error = Some(message);
                }
            }
            None => {}
        }
        let due = Instant::now() >= self.extra.animation_deadline;
        if !self.extra.animation_paused && due {
            if let Some(packet) = self.extra.animation_pending.take() {
                self.display_animation_frame(packet, ctx);
            } else if self.extra.animation_end_pending {
                self.extra.animation_end_pending = false;
                self.extra.animation_ended = true;
                self.extra.animation_paused = true;
                self.status = "动画已播放结束；可点击重播".into();
            }
        }
        if self.extra.animation.is_some()
            && !self.extra.animation_paused
            && !self.extra.animation_ended
        {
            // A late decoder wakes the UI through the channel; don't spin on an expired deadline.
            if self.extra.animation_pending.is_some() || self.extra.animation_end_pending {
                ctx.request_repaint_after(
                    self.extra
                        .animation_deadline
                        .saturating_duration_since(Instant::now()),
                );
            }
        }
        if self.extra.retiring.is_some() {
            ctx.request_repaint_after(Duration::from_millis(20));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn replay_preserves_original_view_instead_of_reapplying_a_failed_record() {
        let mut viewer = super::super::browser_ui::tests::viewer(Vec::new());
        viewer.extra.animation_ended = true;
        viewer.extra.animation_operation = Some((
            Request::Unedited {
                source: Box::new(Request::Load {
                    path: PathBuf::from("sample.gif"),
                    entry: None,
                    password: None,
                    full_raw: false,
                }),
            },
            Operation::Load {
                index: 0,
                full_raw: false,
                preload: false,
            },
        ));
        viewer.toggle_animation();
        let task = viewer.pending.as_ref().unwrap();
        assert!(matches!(task.request, Request::Unedited { .. }));
        assert!(Viewer::is_animation_candidate(&task.request));
    }

    #[test]
    fn pause_resume_preserves_remaining_frame_time() {
        let start = Instant::now();
        let mut paused = false;
        let mut deadline = start + Duration::from_millis(100);
        let mut remaining = Duration::from_millis(100);
        toggle_pause_clock(
            &mut paused,
            &mut deadline,
            &mut remaining,
            start + Duration::from_millis(30),
        );
        assert!(paused);
        assert_eq!(remaining, Duration::from_millis(70));
        toggle_pause_clock(
            &mut paused,
            &mut deadline,
            &mut remaining,
            start + Duration::from_millis(530),
        );
        assert!(!paused);
        assert_eq!(deadline, start + Duration::from_millis(600));
        toggle_pause_clock(
            &mut paused,
            &mut deadline,
            &mut remaining,
            start + Duration::from_millis(550),
        );
        assert_eq!(remaining, Duration::from_millis(50));
        toggle_pause_clock(
            &mut paused,
            &mut deadline,
            &mut remaining,
            start + Duration::from_millis(900),
        );
        assert_eq!(deadline, start + Duration::from_millis(950));
    }

    #[test]
    fn overdue_frame_does_not_gain_extra_delay_on_resume() {
        let now = Instant::now();
        let mut paused = false;
        let mut deadline = now - Duration::from_millis(1);
        let mut remaining = Duration::from_millis(100);
        toggle_pause_clock(&mut paused, &mut deadline, &mut remaining, now);
        assert_eq!(remaining, Duration::ZERO);
        toggle_pause_clock(
            &mut paused,
            &mut deadline,
            &mut remaining,
            now + Duration::from_secs(1),
        );
        assert_eq!(deadline, now + Duration::from_secs(1));
    }

    #[test]
    fn color_wrapped_animation_keeps_animated_route() {
        let request = Request::ColorPreview {
            source: Box::new(Request::Load {
                path: PathBuf::from("book.zip"),
                entry: Some("page.GIF".into()),
                password: None,
                full_raw: false,
            }),
            managed: false,
        };
        assert!(Viewer::is_animation_candidate(&request));
    }
}

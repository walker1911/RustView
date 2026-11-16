use super::{Entry, Operation, Viewer};
use std::{borrow::Cow, path::PathBuf, sync::atomic::Ordering};

const MAX_VISIBLE_MATCHES: usize = 48;

#[derive(Default)]
pub(super) struct State {
    pub(super) open: bool,
    focus: bool,
    source: Option<PathBuf>,
    query: String,
    revision: Option<u64>,
    results: Results,
    scrub: Option<usize>,
}

impl State {
    fn update_scrub(&mut self, response: &egui::Response, page: usize, order_changed: bool) -> Option<usize> {
        let primary = egui::PointerButton::Primary;
        let interrupted = order_changed || response.ctx.input(|input| {
            !input.focused || input.key_pressed(egui::Key::Escape)
                || input.events.iter().any(|event| matches!(event, egui::Event::WindowFocused(false)))
        });
        if interrupted {
            self.scrub = None;
            if response.dragged_by(primary) {
                response.ctx.stop_dragging();
            }
        } else if response.drag_stopped_by(primary) {
            return self.scrub.take().map(|_| page);
        } else if response.drag_started_by(primary) || self.scrub.is_some() && response.dragged_by(primary) {
            self.scrub = Some(page);
        } else {
            self.scrub = None;
        }
        None
    }
}

#[derive(Default)]
enum Results {
    #[default]
    Empty,
    InvalidNumber,
    Matches {
        count: usize,
        hits: Vec<Hit>,
    },
}

#[derive(Clone, Copy)]
struct Hit {
    entry: usize,
    number: usize,
    rank: MatchRank,
}

#[derive(Clone, Copy, Eq, Ord, PartialEq, PartialOrd)]
enum MatchRank {
    Contains,
    FileName,
    Path,
    ExactPath,
}

impl Results {
    fn search(query: &str, order: &[usize], entries: &[Entry]) -> Self {
        let query = query.trim();
        if query.is_empty() {
            return Self::Empty;
        }
        if query.bytes().all(|byte| byte.is_ascii_digit()) {
            let hit = query
                .parse::<usize>()
                .ok()
                .and_then(|number| number.checked_sub(1))
                .and_then(|position| order.get(position).map(|&entry| (position, entry)));
            return match hit {
                Some((position, entry)) => Self::Matches {
                    count: 1,
                    hits: vec![Hit {
                        entry,
                        number: position + 1,
                        rank: MatchRank::ExactPath,
                    }],
                },
                None => Self::InvalidNumber,
            };
        }

        let query = normalize_slashes(query);
        let folded_query = query.to_lowercase();
        let mut count = 0;
        let mut hits: Vec<Hit> = Vec::new();
        for (position, &entry) in order.iter().enumerate() {
            let name = normalize_slashes(&entries[entry].name);
            let rank = if name == query {
                MatchRank::ExactPath
            } else {
                let folded = name.to_lowercase();
                if folded == folded_query {
                    MatchRank::Path
                } else if folded.rsplit('/').next() == Some(folded_query.as_str()) {
                    MatchRank::FileName
                } else if folded.contains(&folded_query) {
                    MatchRank::Contains
                } else {
                    continue;
                }
            };
            count += 1;
            // Later exact matches can replace earlier substring results even when the list is full.
            let insert_at = hits.partition_point(|hit| hit.rank >= rank);
            if insert_at < MAX_VISIBLE_MATCHES {
                hits.insert(
                    insert_at,
                    Hit {
                        entry,
                        number: position + 1,
                        rank,
                    },
                );
                if hits.len() > MAX_VISIBLE_MATCHES {
                    hits.pop();
                }
            }
        }
        Self::Matches { count, hits }
    }

    fn primary(&self) -> Option<usize> {
        let Self::Matches { hits, .. } = self else {
            return None;
        };
        let first = hits.first()?;
        if hits.get(1).is_some_and(|second| second.rank == first.rank) {
            return None;
        }
        Some(first.entry)
    }
}

fn normalize_slashes(name: &str) -> Cow<'_, str> {
    if name.contains('\\') {
        Cow::Owned(name.replace('\\', "/"))
    } else {
        Cow::Borrowed(name)
    }
}

impl Viewer {
    pub(super) fn jump_available(&mut self) -> bool {
        self.source.is_some()
            && !self.password_dialog
            && (self.is_archive_source() || !self.directory_listing())
            && !self.browse_entries().1.is_empty()
    }

    pub(super) fn open_image_jump(&mut self) {
        if self.jump_available() {
            self.archive_jump = State {
                open: true,
                focus: true,
                source: self.source.clone(),
                ..State::default()
            };
            if self.is_archive_source() {
                self.cancel_coverflow_thumbnail();
                if self.pending.as_ref().is_some_and(|task| matches!(task.operation, Operation::Load { preload: true, .. })) {
                    self.pending = None;
                }
                if let Some(task) = self.running.as_ref().filter(|task| task.preload.is_some() && !task.promoted) {
                    task.cancel.store(true, Ordering::Relaxed);
                }
            }
        }
    }

    pub(super) fn draw_image_jump(&mut self, ctx: &egui::Context) {
        if !self.archive_jump.open {
            return;
        }
        let archive = self.is_archive_source();
        let query_id = egui::Id::new("image-jump-query");
        if self.source.is_none() || self.source != self.archive_jump.source
            || self.password_dialog || (!archive && self.directory_listing()) {
            self.archive_jump = State::default();
            ctx.memory_mut(|memory| memory.surrender_focus(query_id));
            return;
        }

        let mut state = std::mem::take(&mut self.archive_jump);
        self.ensure_browse_sequence();
        let (revision, order) = self.extra.coverflow.order();
        let entries = &self.entries;
        // A refresh can replace the match under the pointer or at the same ordinal.
        let order_changed = state.revision.is_some_and(|previous| previous != revision);
        let mut target = None;
        let modal = egui::Modal::new(egui::Id::new("image-jump")).show(ctx, |ui| {
            ui.set_width((ctx.screen_rect().width() - 48.0).clamp(180.0, 500.0));
            ui.heading(if archive { "压缩包跳页" } else { "文件夹跳图" });
            let response = ui.add(
                egui::TextEdit::singleline(&mut state.query)
                    .id(query_id)
                    .hint_text(if archive { "输入文件名、包内路径或序号" } else { "输入文件名或序号" })
                    .char_limit(4096)
                    .desired_width(f32::INFINITY),
            );
            if state.focus {
                response.request_focus();
                state.focus = false;
            }
            if response.changed() || state.revision != Some(revision) {
                state.results = Results::search(&state.query, order, entries);
                state.revision = Some(revision);
            }

            let primary = state.results.primary();
            let submit =
                response.lost_focus() && ui.input(|input| input.key_pressed(egui::Key::Enter));
            if submit {
                if let Some(entry) = primary.filter(|_| !order_changed) {
                    target = Some(entry);
                } else {
                    response.request_focus();
                }
            }
            match &state.results {
                Results::Empty => {
                    ui.small(format!("共 {} 张，序号从 1 开始。", order.len()));
                }
                Results::InvalidNumber => {
                    ui.small(if order.is_empty() {
                        "当前没有可跳转的图片。".into()
                    } else {
                        format!("请输入 1 至 {} 之间的序号。", order.len())
                    });
                }
                Results::Matches { count: 0, .. } => {
                    ui.small("没有匹配的图片。");
                }
                Results::Matches { count, hits } => {
                    if *count > hits.len() {
                        ui.small(format!("显示 {count} 项中的前 {} 项。", hits.len()));
                    } else if primary.is_none() {
                        ui.small("请选择要打开的图片。");
                    }
                    egui::ScrollArea::vertical()
                        .max_height(250.0)
                        .show(ui, |ui| {
                            for hit in hits {
                                let name = &entries[hit.entry].name;
                                let label = format!("{}  {}", hit.number, name);
                                let clicked = ui.push_id((revision, hit.entry), |ui| {
                                    ui.add(egui::Button::new(label).truncate())
                                        .on_hover_text(name)
                                        .clicked()
                                });
                                if clicked.inner && !order_changed {
                                    target = Some(hit.entry);
                                }
                            }
                        });
                }
            }
            if archive && order.len() > 1 {
                let current_page = self.extra.coverflow.position(self.selected).unwrap_or(0) + 1;
                let mut page = state.scrub.unwrap_or(current_page);
                let track = ui.scope(|ui| {
                    ui.spacing_mut().slider_width = (ui.available_width() - 88.0).max(60.0);
                    ui.add(egui::Slider::new(&mut page, 1..=order.len())
                        .show_value(false).smart_aim(false).text("拖动跳页"))
                }).inner;
                if let Some(page) = state.update_scrub(&track, page, order_changed) {
                    target = order.get(page - 1).copied();
                }

                let preview_page = state.scrub.unwrap_or(current_page);
                let index = order[preview_page - 1];
                let name = &entries[index].name;
                let texture = self.cached_coverflow_preview(index);
                ui.horizontal(|ui| {
                    let (bounds, _) = ui.allocate_exact_size(egui::vec2(88.0, 58.0), egui::Sense::hover());
                    if let Some(texture) = texture {
                        let size = texture.size_vec2();
                        let scale = (bounds.width() / size.x).min(bounds.height() / size.y);
                        let rect = egui::Rect::from_center_size(bounds.center(), size * scale);
                        let uv = egui::Rect::from_min_max(egui::Pos2::ZERO, egui::pos2(1.0, 1.0));
                        ui.painter().image(texture.id(), rect, uv, egui::Color32::WHITE);
                    }
                    ui.vertical(|ui| {
                        ui.strong(format!("{preview_page} / {}", order.len()));
                        ui.add(egui::Label::new(name).truncate()).on_hover_text(name);
                    });
                });
            } else {
                state.scrub = None;
            }
            ui.horizontal(|ui| {
                let clicked = ui.push_id(revision, |ui| {
                    ui.add_enabled(primary.is_some(), egui::Button::new("跳转"))
                        .clicked()
                });
                if clicked.inner && !order_changed {
                    target = primary;
                }
                if ui.button("关闭").clicked() {
                    state.open = false;
                }
            });
        });
        if modal.should_close() || target.is_some() {
            state.open = false;
        }
        if state.open {
            self.archive_jump = state;
        } else {
            ctx.memory_mut(|memory| memory.surrender_focus(query_id));
        }
        if let Some(entry) = target {
            if self.selected != Some(entry) || self.current_key.map(|key| key.0) != Some(entry) {
                self.load_index(entry, false);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(name: &str) -> Entry {
        Entry {
            name: name.into(),
            directory: false,
            archive: false,
            size: 0,
            modified: None,
        }
    }

    #[test]
    fn jumps_use_visible_order_and_require_full_paths_for_ambiguous_names() {
        let entries = [
            entry("甲\\封面.JPG"),
            entry("乙/封面.jpg"),
            entry("outside.jpg"),
        ];
        let order = [1, 0];
        assert_eq!(Results::search("1", &order, &entries).primary(), Some(1));
        assert_eq!(Results::search("2", &order, &entries).primary(), Some(0));
        for query in ["0", "3", "99999999999999999999999"] {
            assert!(matches!(
                Results::search(query, &order, &entries),
                Results::InvalidNumber
            ));
        }
        assert_eq!(
            Results::search("封面.jpg", &order, &entries).primary(),
            None
        );
        assert_eq!(
            Results::search("甲/封面.jpg", &order, &entries).primary(),
            Some(0)
        );
        assert_eq!(
            Results::search("outside.jpg", &order, &entries).primary(),
            None
        );
    }

    #[test]
    fn exact_paths_are_kept_even_after_the_results_fill() {
        let mut entries: Vec<_> = (0..100)
            .map(|n| entry(&format!("folder/{n}-image.jpg")))
            .collect();
        entries.push(entry("image.jpg"));
        let order: Vec<_> = (0..entries.len()).collect();
        let result = Results::search("image.jpg", &order, &entries);
        assert_eq!(result.primary(), Some(100));
        assert!(
            matches!(result, Results::Matches { count: 101, hits } if hits.len() == MAX_VISIBLE_MATCHES)
        );
    }

    #[test]
    fn local_jump_waits_for_full_order_and_uses_the_arrow_sequence() {
        let mut viewer = Viewer::empty();
        viewer.open_local_image(PathBuf::from("gallery/1.jpg"));
        assert!(!viewer.jump_available());

        viewer.install_source(
            PathBuf::from("gallery"),
            None,
            vec![entry("1.jpg"), entry("2.NEF"), entry("10.jpg")],
        );
        viewer.extra.descending = true;
        viewer.rebuild_visible();
        viewer.open_image_jump();
        assert!(viewer.archive_jump.open);
        let (_, order, entries) = viewer.browse_entries();
        let index = Results::search("2", order, entries).primary().unwrap();
        assert_eq!(Results::search("2.nef", order, entries).primary(), Some(index));
        viewer.selected = Some(index);
        assert_eq!(viewer.navigation_target(false, false), Some(2));
        assert_eq!(viewer.navigation_target(true, false), Some(0));
    }

    #[test]
    fn dragging_an_archive_opens_only_the_released_page_and_uses_its_cached_cover() {
        let mut viewer = archive_viewer();
        let ctx = egui::Context::default();
        ctx.enable_accesskit();
        let last = *viewer.browse_entries().1.last().unwrap();
        let texture = ctx.load_texture("last-archive-cover", egui::ColorImage::new([8, 4], egui::Color32::WHITE), egui::TextureOptions::LINEAR);
        viewer.extra.thumbnails.insert(last, texture.clone());
        viewer.schedule_preload();
        assert!(matches!(viewer.pending.as_ref().map(|task| &task.operation), Some(Operation::Load { preload: true, .. })));

        let output = jump_frame(&mut viewer, &ctx, true, vec![]);
        let count = widget_bounds(&output, egui::accesskit::Role::Button, "1 / 3").center();
        jump_frame(&mut viewer, &ctx, true, pointer(count, true));
        jump_frame(&mut viewer, &ctx, true, pointer(count, false));
        assert!(viewer.archive_jump.open);
        assert!(viewer.pending.is_none());
        let output = jump_frame(&mut viewer, &ctx, true, vec![]);
        let track = widget_bounds(&output, egui::accesskit::Role::Slider, "拖动跳页");
        let start = egui::pos2(track.left() + 1.0, track.center().y);
        let end = egui::pos2(track.right() - 1.0, track.center().y);
        let original = viewer.selected;
        let revision = viewer.browse_entries().0;

        jump_frame(&mut viewer, &ctx, true, pointer(start, true));
        jump_frame(&mut viewer, &ctx, true, vec![egui::Event::PointerMoved(track.center())]);
        assert_eq!(viewer.archive_jump.scrub, Some(2));
        let output = jump_frame(&mut viewer, &ctx, true, vec![egui::Event::PointerMoved(end)]);
        assert_eq!(viewer.archive_jump.scrub, Some(3));
        assert_eq!(viewer.selected, original);
        assert!(viewer.pending.is_none() && viewer.running.is_none());
        assert_eq!(viewer.browse_entries().0, revision);
        assert!(output.shapes.iter().any(|clip| matches!(&clip.shape,
            egui::Shape::Mesh(mesh) if mesh.texture_id == texture.id() && mesh.calc_bounds().top() < track.top() + 100.0)));

        jump_frame(&mut viewer, &ctx, true, pointer(end, false));
        assert!(!viewer.archive_jump.open);
        assert_eq!(viewer.selected, Some(last));
        assert!(matches!(viewer.pending.as_ref().map(|task| &task.operation),
            Some(Operation::Load { index, preload: false, .. }) if *index == last));
        assert!(viewer.navigation_target(false, false).is_some());
    }

    #[test]
    fn interrupted_archive_drag_cannot_load_on_release() {
        for interruption in ["escape", "focus", "order", "source"] {
            let mut viewer = archive_viewer();
            let ctx = egui::Context::default();
            ctx.enable_accesskit();
            viewer.open_image_jump();
            jump_frame(&mut viewer, &ctx, true, vec![]);
            let output = jump_frame(&mut viewer, &ctx, true, vec![]);
            let track = widget_bounds(&output, egui::accesskit::Role::Slider, "拖动跳页");
            let end = egui::pos2(track.right() - 1.0, track.center().y);
            jump_frame(&mut viewer, &ctx, true, pointer(end, true));
            assert_eq!(viewer.archive_jump.scrub, Some(3));
            let original = viewer.selected;

            let mut release = pointer(end, false);
            match interruption {
                "escape" => release.push(egui::Event::Key {
                    key: egui::Key::Escape, physical_key: None, pressed: true,
                    repeat: false, modifiers: egui::Modifiers::NONE,
                }),
                "focus" => release.push(egui::Event::WindowFocused(false)),
                "order" => {
                    viewer.extra.descending = true;
                    viewer.rebuild_visible();
                }
                "source" => viewer.source = Some(PathBuf::from("different.zip")),
                _ => unreachable!(),
            }
            jump_frame(&mut viewer, &ctx, interruption != "focus", release);
            assert_eq!(viewer.archive_jump.scrub, None, "{interruption}");
            assert_eq!(viewer.selected, original, "{interruption}");
            assert!(viewer.pending.is_none() && viewer.running.is_none(), "{interruption}");
        }
    }

    fn archive_viewer() -> Viewer {
        let mut viewer = Viewer::empty();
        viewer.install_source(PathBuf::from("fixture.zip"), None,
            ["a/image.jpg", "b/image.jpg", "c/image.jpg"].map(entry).to_vec());
        viewer.browser = false;
        viewer.selected = Some(0);
        viewer
    }

    fn jump_frame(viewer: &mut Viewer, ctx: &egui::Context, focused: bool, events: Vec<egui::Event>) -> egui::FullOutput {
        ctx.run(egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1200.0, 900.0))),
            focused, events, ..Default::default()
        }, |ctx| {
            viewer.draw_coverflow(ctx);
            viewer.draw_image_jump(ctx);
        })
    }

    fn widget_bounds(output: &egui::FullOutput, role: egui::accesskit::Role, label: &str) -> egui::Rect {
        let update = output.platform_output.accesskit_update.as_ref().unwrap();
        let bounds = update.nodes.iter().find_map(|(_, node)| {
            (node.role() == role && node.label() == Some(label)).then(|| node.bounds()).flatten()
        }).unwrap();
        egui::Rect::from_min_max(egui::pos2(bounds.x0 as f32, bounds.y0 as f32), egui::pos2(bounds.x1 as f32, bounds.y1 as f32))
    }

    fn pointer(pos: egui::Pos2, pressed: bool) -> Vec<egui::Event> {
        vec![egui::Event::PointerMoved(pos), egui::Event::PointerButton {
            pos, button: egui::PointerButton::Primary, pressed, modifiers: egui::Modifiers::NONE,
        }]
    }
}

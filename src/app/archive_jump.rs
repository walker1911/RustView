use super::{Entry, Viewer};
use std::borrow::Cow;

const MAX_VISIBLE_MATCHES: usize = 48;

#[derive(Default)]
pub(super) struct State {
    pub(super) open: bool,
    focus: bool,
    query: String,
    revision: Option<u64>,
    results: Results,
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
    pub(super) fn open_archive_jump(&mut self) {
        if self.is_archive_source() && !self.browse_entries().1.is_empty() {
            self.archive_jump = State {
                open: true,
                focus: true,
                ..State::default()
            };
        }
    }

    pub(super) fn draw_archive_jump(&mut self, ctx: &egui::Context) {
        if !self.archive_jump.open {
            return;
        }
        if !self.is_archive_source() || self.password_dialog {
            self.archive_jump = State::default();
            return;
        }

        let mut state = std::mem::take(&mut self.archive_jump);
        let (revision, order, entries) = self.browse_entries();
        let mut target = None;
        let modal = egui::Modal::new(egui::Id::new("archive-jump")).show(ctx, |ui| {
            ui.set_width((ctx.screen_rect().width() - 48.0).clamp(180.0, 500.0));
            ui.heading("压缩包跳页");
            let response = ui.add(
                egui::TextEdit::singleline(&mut state.query)
                    .id(egui::Id::new("archive-jump-query"))
                    .hint_text("输入文件名、包内路径或序号")
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
                if let Some(entry) = primary {
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
                                if ui
                                    .add(egui::Button::new(label).truncate())
                                    .on_hover_text(name)
                                    .clicked()
                                {
                                    target = Some(hit.entry);
                                }
                            }
                        });
                }
            }
            ui.horizontal(|ui| {
                if ui
                    .add_enabled(primary.is_some(), egui::Button::new("跳转"))
                    .clicked()
                {
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
        }
        if let Some(entry) = target {
            if self.current_key.map(|key| key.0) != Some(entry) {
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
}

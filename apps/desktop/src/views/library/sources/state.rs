use super::{SOURCE_ROW_HEIGHT, SourceEntry, SourceGroup, playlist_sources};
use gpui::{ListOffset, ListState, px};
use std::time::{Duration, Instant};

const DURATION: Duration = Duration::from_millis(180);

struct Disclosure {
    open: bool,
    from: f32,
    started: Option<Instant>,
}

impl Default for Disclosure {
    fn default() -> Self {
        Self {
            open: true,
            from: 1.,
            started: None,
        }
    }
}

impl Disclosure {
    fn progress(&self, now: Instant) -> f32 {
        let target = if self.open { 1. } else { 0. };
        let Some(started) = self.started else {
            return target;
        };
        let phase = (now.saturating_duration_since(started).as_secs_f32() / DURATION.as_secs_f32())
            .clamp(0., 1.);
        self.from + (target - self.from) * (1. - (1. - phase).powi(3))
    }
}

#[derive(Default)]
pub(crate) struct SourceTree {
    groups: [Disclosure; 2],
    rows: Vec<(SourceEntry, f32)>,
    focus: Option<[gpui::FocusHandle; 2]>,
}

impl SourceTree {
    pub(super) fn focus_handles(&mut self, cx: &mut gpui::App) -> [gpui::FocusHandle; 2] {
        self.focus
            .get_or_insert_with(|| std::array::from_fn(|_| cx.focus_handle()))
            .clone()
    }

    pub(super) fn toggle(&mut self, group: SourceGroup) {
        let now = Instant::now();
        let state = &mut self.groups[group as usize];
        state.from = state.progress(now);
        state.open = !state.open;
        state.started = Some(now);
    }

    /// Controller browsing reveals its destination immediately before scrolling.
    pub(crate) fn reveal(&mut self, folder: bool) {
        self.groups[if folder { 0 } else { 1 }] = Disclosure::default();
    }

    pub(super) fn frame(
        &mut self,
        playlists: &[mixless_library::PlaylistSummary],
        scroll: &ListState,
        reduced: bool,
    ) -> (Vec<(SourceEntry, f32)>, [f32; 2], bool) {
        let now = Instant::now();
        let progress = self.groups.each_mut().map(|state| {
            if reduced
                || state
                    .started
                    .is_some_and(|start| now.duration_since(start) >= DURATION)
            {
                state.started = None;
            }
            state.progress(now)
        });
        let mut remaining = 0.;
        let rows: Vec<_> = playlist_sources(playlists)
            .into_iter()
            .filter_map(|entry| {
                let height = match entry {
                    SourceEntry::Group(group, count) => {
                        remaining = count as f32 * SOURCE_ROW_HEIGHT * progress[group as usize];
                        SOURCE_ROW_HEIGHT
                    }
                    SourceEntry::Playlist(_) => {
                        let height = remaining.clamp(0., SOURCE_ROW_HEIGHT);
                        remaining -= SOURCE_ROW_HEIGHT;
                        height
                    }
                };
                (height > 0.).then_some((entry, height))
            })
            .collect();
        if rows != self.rows {
            // Invalidate changed measurements without jumping to the top. If
            // an anchored child disappears, retain its disclosure header.
            let offset = scroll.logical_scroll_top();
            let old = self.rows.get(offset.item_ix).map(|r| r.0);
            let anchor = old.and_then(|entry| rows.iter().position(|r| r.0 == entry));
            let fallback = old.and_then(|entry| {
                let group = match entry {
                    SourceEntry::Group(group, _) => group,
                    SourceEntry::Playlist(index) => {
                        SourceGroup::of(playlists.get(index)?.folder_path.is_some())
                    }
                };
                rows.iter()
                    .position(|r| matches!(r.0, SourceEntry::Group(g, _) if g == group))
            });
            scroll.splice_focusable(
                0..scroll.item_count(),
                rows.iter().map(|(entry, _)| match entry {
                    SourceEntry::Group(group, _) => self
                        .focus
                        .as_ref()
                        .map(|handles| handles[*group as usize].clone()),
                    SourceEntry::Playlist(_) => None,
                }),
            );
            if let Some(ix) = anchor.or(fallback) {
                scroll.scroll_to(ListOffset {
                    item_ix: ix,
                    offset_in_item: if anchor.is_some() {
                        offset.offset_in_item.min(px(rows[ix].1))
                    } else {
                        px(0.)
                    },
                });
            }
            self.rows = rows.clone();
        }
        (
            rows,
            progress,
            self.groups.iter().any(|state| state.started.is_some()),
        )
    }

    pub(crate) fn playlist_row(
        &mut self,
        playlists: &[mixless_library::PlaylistSummary],
        id: i64,
        scroll: &ListState,
    ) -> Option<usize> {
        self.frame(playlists, scroll, crate::motion::reduce_motion()).0.iter().position(|(entry, _)| {
            matches!(entry, SourceEntry::Playlist(index) if playlists[*index].id == id)
        })
    }
}

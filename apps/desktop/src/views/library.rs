//! Compact library browser with a source tree, aligned virtualized track
//! table, cached cover art, and a secondary import modal.

mod drag;
mod menu;
mod status;
pub use drag::TrackDrag;
pub use menu::TrackMenu;

use std::path::PathBuf;

use crate::state::UiState;
use crate::theme;
use gpui::prelude::*;
use gpui::{IntoElement, MouseButton, MouseDownEvent, SharedString, Styled, Window, px};
use mixless_protocol::{Track, TrackId};

fn fmt_duration(ms: u64) -> String {
    let s = ms / 1000;
    format!("{}:{:02}", s / 60, s % 60)
}

// Header and rows use this exact width set, including flex-none on every
// fixed column. This prevents content-dependent shrinking in narrow windows.
const COL_COVER: f32 = 26.;
const COL_ARTIST: f32 = 136.;
const COL_ALBUM: f32 = 120.;
const COL_BPM: f32 = 56.;
const COL_KEY: f32 = 56.;
const COL_TIME: f32 = 52.;
const ROW_PAD: f32 = 14.;
// uniform_list measures a single item, so group labels and sources must share
// the same compact height to keep scrolling and hit targets aligned.
const SOURCE_ROW_HEIGHT: f32 = 24.;
const SOURCE_ROW_PAD: f32 = 6.;

fn cover_placeholder(track: &Track) -> gpui::AnyElement {
    let color = theme::cue_color(track.id.0.unsigned_abs() as usize);
    let initial = track
        .title
        .chars()
        .next()
        .unwrap_or('•')
        .to_uppercase()
        .to_string();
    gpui::div()
        .flex()
        .flex_none()
        .items_center()
        .justify_center()
        .size(px(24.))
        .rounded(px(3.))
        .bg(theme::with_alpha(color, 0.16))
        .text_size(px(10.))
        .font_weight(gpui::FontWeight::BOLD)
        .text_color(color)
        .child(initial)
        .into_any_element()
}

fn track_cover(track: &Track) -> gpui::AnyElement {
    let tile = if let Some(path) = &track.artwork_path {
        gpui::div()
            .flex()
            .flex_none()
            .size(px(24.))
            .rounded(px(3.))
            .overflow_hidden()
            .bg(theme::PANEL_RAISED)
            .child(
                gpui::img(PathBuf::from(path))
                    .size_full()
                    .object_fit(gpui::ObjectFit::Cover),
            )
            .into_any_element()
    } else {
        cover_placeholder(track)
    };

    gpui::div()
        .flex()
        .flex_none()
        .items_center()
        .w(px(COL_COVER))
        .child(tile)
        .into_any_element()
}

#[derive(Clone, Copy)]
enum SourceIcon {
    Collection,
    Folder,
    Playlist,
}

enum SourceEntry {
    Group(&'static str, usize),
    Playlist(usize),
}

// Source groups are flat: reserve space for the icon, not an empty tree gutter.
// Both source types share the same geometry so names and counts stay aligned.
fn source_row(
    id: impl Into<gpui::ElementId>,
    name: impl Into<SharedString>,
    icon: SourceIcon,
    count: Option<u32>,
    selected: bool,
) -> gpui::Stateful<gpui::Div> {
    let color = if selected {
        theme::ACCENT
    } else {
        theme::MUTED
    };
    gpui::div()
        .id(id)
        .relative()
        .flex()
        .flex_none()
        .items_center()
        .gap(px(6.))
        .w_full()
        .h(px(SOURCE_ROW_HEIGHT))
        .px(px(SOURCE_ROW_PAD))
        .rounded(px(3.))
        .cursor_pointer()
        .text_size(px(11.))
        .line_height(px(16.))
        .text_color(theme::TEXT)
        .when(selected, |el| {
            el.bg(theme::with_alpha(theme::ACCENT, 0.10)).child(
                gpui::div()
                    .absolute()
                    .left_0()
                    .top(px((SOURCE_ROW_HEIGHT - 14.) / 2.))
                    .w(px(2.))
                    .h(px(14.))
                    .rounded(px(1.))
                    .bg(theme::ACCENT),
            )
        })
        .when(!selected, |el| el.hover(|s| s.bg(theme::PANEL_RAISED)))
        .child(
            gpui::canvas(
                |_, _, _| {},
                move |bounds, _, window, _| {
                    let x = f32::from(bounds.origin.x);
                    let y = f32::from(bounds.origin.y);
                    match icon {
                        SourceIcon::Collection => {
                            for (dx, dy) in [(1., 1.), (7., 1.), (1., 7.), (7., 7.)] {
                                crate::controls::quad_fill(window, x + dx, y + dy, 4., 4., color);
                            }
                        }
                        SourceIcon::Folder => {
                            crate::controls::quad_fill(window, x + 1., y + 2., 5., 2., color);
                            crate::controls::quad_fill(window, x + 1., y + 4., 10., 7., color);
                        }
                        SourceIcon::Playlist => {
                            for dy in [1., 5., 9.] {
                                crate::controls::quad_fill(window, x + 1., y + dy, 2., 2., color);
                                crate::controls::quad_fill(window, x + 5., y + dy, 6., 2., color);
                            }
                        }
                    }
                },
            )
            .flex_none()
            .size(px(12.)),
        )
        .child(gpui::div().flex_1().min_w_0().truncate().child(name.into()))
        .when_some(count, |el, count| {
            el.child(
                gpui::div()
                    .flex_none()
                    .min_w(px(22.))
                    .text_align(gpui::TextAlign::Right)
                    .text_size(px(10.))
                    .text_color(theme::MUTED)
                    .child(count.to_string()),
            )
        })
}

struct PathTip(String);
impl gpui::Render for PathTip {
    fn render(&mut self, _: &mut Window, _: &mut gpui::Context<Self>) -> impl IntoElement {
        gpui::div()
            .px_2()
            .py_1()
            .rounded(px(4.))
            .bg(theme::PANEL_RAISED)
            .text_size(px(11.))
            .text_color(theme::TEXT)
            .child(self.0.clone())
    }
}

// Downloader-created folder IDs are not useful playlist titles. Keep the
// filesystem identity intact and use parent names when display titles collide.
fn readable_folder_name(name: &str) -> String {
    if let Some((id, title)) = name
        .strip_prefix("playlist_")
        .and_then(|s| s.split_once('_'))
    {
        if id.len() >= 5
            && id.chars().all(|c| c.is_ascii_alphanumeric())
            && !title.trim_matches('_').is_empty()
        {
            return title.trim_matches('_').replace('_', " ");
        }
    }
    name.to_owned()
}

fn folder_name(
    playlist: &mixless_library::PlaylistSummary,
    all: &[mixless_library::PlaylistSummary],
) -> String {
    let Some(path) = &playlist.folder_path else {
        return playlist.name.clone();
    };
    let parts = |path: &str| -> Vec<String> {
        std::path::Path::new(path)
            .components()
            .filter_map(|c| match c {
                std::path::Component::Normal(name) => Some(name.to_string_lossy().into_owned()),
                _ => None,
            })
            .collect()
    };
    let display = |parts: &[String], depth: usize| -> String {
        let mut suffix = parts[parts.len().saturating_sub(depth)..].to_vec();
        if let Some(last) = suffix.last_mut() {
            *last = readable_folder_name(last);
        }
        suffix.join("/")
    };
    let own = parts(path);
    for depth in 1..=own.len() {
        let name = display(&own, depth);
        if !all.iter().any(|other| {
            other.id != playlist.id
                && other
                    .folder_path
                    .as_ref()
                    .is_some_and(|path| display(&parts(path), depth) == name)
        }) {
            return name;
        }
    }
    own.last().cloned().unwrap_or_else(|| path.clone())
}

pub type LibraryKey = (
    usize,
    usize,
    Option<i64>,
    Option<i64>,
    mixless_protocol::DeckId,
    [Option<TrackId>; 2],
    u64,
    bool,
    bool,
    SharedString,
);

pub struct LibraryView {
    pub owner: gpui::WeakEntity<UiState>,
}
impl gpui::Render for LibraryView {
    fn render(&mut self, _: &mut Window, cx: &mut gpui::Context<Self>) -> impl IntoElement {
        self.owner
            .update(cx, |state, cx| state.render_library(cx, 160.))
            .unwrap_or_else(|_| gpui::div().into_any_element())
    }
}

impl UiState {
    pub fn render_library(&self, cx: &mut gpui::Context<Self>, height: f32) -> gpui::AnyElement {
        let state = cx.entity();

        let all_selected = self.playlist_sel.is_none();
        let all_tracks_row = {
            let el = source_row(
                "tree-all",
                "All Tracks",
                SourceIcon::Collection,
                None,
                all_selected,
            );
            let st = state.clone();
            el.on_click(move |_ev, _window, cx| {
                st.update(cx, |s, cx| {
                    s.select_playlist(None);
                    cx.notify();
                });
            })
        };

        let playlists = self.playlists.clone();
        let playlist_sel = self.playlist_sel;
        let playlist_state = cx.entity();
        let mut sources = Vec::new();
        for (title, folders) in [("LOCAL FOLDERS", true), ("PLAYLISTS", false)] {
            let indices: Vec<_> = playlists
                .iter()
                .enumerate()
                .filter(|(_, playlist)| playlist.folder_path.is_some() == folders)
                .map(|(index, _)| index)
                .collect();
            if !indices.is_empty() {
                sources.push(SourceEntry::Group(title, indices.len()));
                sources.extend(indices.into_iter().map(SourceEntry::Playlist));
            }
        }
        let playlist_list = gpui::uniform_list(
            "playlist-list",
            sources.len(),
            move |range, _window, _cx| {
                let mut rows = Vec::new();
                for ix in range {
                    let index = match sources[ix] {
                        SourceEntry::Group(title, count) => {
                            rows.push(
                                gpui::div()
                                    .flex()
                                    .items_center()
                                    .justify_between()
                                    .w_full()
                                    .h(px(SOURCE_ROW_HEIGHT))
                                    .px(px(SOURCE_ROW_PAD))
                                    .text_size(px(9.))
                                    .line_height(px(14.))
                                    .font_weight(gpui::FontWeight::MEDIUM)
                                    .text_color(theme::MUTED)
                                    .child(title)
                                    .child(count.to_string())
                                    .into_any_element(),
                            );
                            continue;
                        }
                        SourceEntry::Playlist(index) => index,
                    };
                    let pl = &playlists[index];
                    let selected = playlist_sel == Some(pl.id);
                    let display_name = folder_name(pl, &playlists);
                    let row = source_row(
                        SharedString::from(format!("pl-{}", pl.id)),
                        display_name,
                        if pl.folder_path.is_some() {
                            SourceIcon::Folder
                        } else {
                            SourceIcon::Playlist
                        },
                        Some(pl.tracks),
                        selected,
                    );
                    let row = if let Some(path) = &pl.folder_path {
                        let path = path.clone();
                        row.tooltip(move |_, cx| cx.new(|_| PathTip(path.clone())).into())
                    } else {
                        row
                    };
                    let st = playlist_state.clone();
                    let id = pl.id;
                    rows.push(
                        row.on_click(move |_ev, _window, cx| {
                            st.update(cx, |s, cx| {
                                s.select_playlist(Some(id));
                                cx.notify();
                            });
                        })
                        .into_any_element(),
                    );
                }
                rows
            },
        )
        .flex_1()
        .min_h_0();

        let source_name = self
            .playlist_sel
            .and_then(|id| self.playlists.iter().find(|pl| pl.id == id))
            .map(|pl| folder_name(pl, &self.playlists))
            .unwrap_or_else(|| "All Tracks".into());
        let toolbar = gpui::div()
            .flex()
            .flex_none()
            .items_center()
            .gap_2()
            .h(px(30.))
            .px_2()
            .border_b_1()
            .border_color(theme::LINE)
            .child(
                gpui::div()
                    .flex_1()
                    .min_w_0()
                    .overflow_hidden()
                    .text_size(px(11.))
                    .text_color(theme::TEXT)
                    .child(source_name.to_owned()),
            )
            .child(
                gpui::div()
                    .flex_none()
                    .text_size(px(10.))
                    .text_color(theme::MUTED)
                    .child(format!("{} tracks", self.tracks.len())),
            )
            .child(self.local_import_button(cx, false, "+ Files"))
            .child(self.local_import_button(cx, true, "+ Folder"))
            .child(
                gpui::div()
                    .id("library-import")
                    .cursor_pointer()
                    .px_2()
                    .py_1()
                    .rounded(px(3.))
                    .text_size(px(10.))
                    .text_color(theme::MUTED)
                    .hover(|s| s.bg(theme::PANEL_RAISED).text_color(theme::TEXT))
                    .child("Spotify")
                    .on_click(cx.listener(|s, _, _, cx| {
                        s.show_import_modal = true;
                        cx.notify();
                    })),
            );
        let side = gpui::div()
            .flex()
            .flex_none()
            .w(px(166.))
            .flex_col()
            .min_h_0()
            .border_r_1()
            .border_color(theme::LINE)
            .bg(gpui::rgb(0x0e0e10))
            .overflow_hidden()
            .child(
                gpui::div()
                    .flex()
                    .flex_none()
                    .items_center()
                    .h(px(30.))
                    .px(px(4. + SOURCE_ROW_PAD))
                    .border_b_1()
                    .border_color(theme::LINE)
                    .text_size(px(10.))
                    .text_color(theme::MUTED)
                    .child("LIBRARY"),
            )
            .child(
                gpui::div()
                    .flex()
                    .flex_col()
                    .flex_1()
                    .min_h_0()
                    .px(px(4.))
                    .py(px(2.))
                    .child(all_tracks_row)
                    .when(self.playlists.is_empty(), |el| {
                        el.child(
                            gpui::div()
                                .px(px(SOURCE_ROW_PAD))
                                .py(px(4.))
                                .text_size(px(11.))
                                .text_color(theme::MUTED)
                                .child("No playlists yet"),
                        )
                    })
                    .child(playlist_list),
            );

        let shown = self.tracks.clone();
        let analysis_statuses = self.core.analysis.statuses();
        let loaded_tracks = self.snapshot.decks.each_ref().map(|d| d.track_id);
        let track_sel = self.track_sel;
        let focus_deck = self.focus;
        let menu_playlist = self.playlist_sel;
        let track_state = cx.entity();
        let previews = self.library_previews.clone();
        let preview_core = self.core.clone();
        let reorder_end_state = track_state.clone();
        let track_count = shown.len();

        let header = gpui::div()
            .flex()
            .flex_none()
            .items_center()
            .h(px(22.))
            .px(px(ROW_PAD))
            .gap_2()
            .text_size(px(9.))
            .text_color(theme::MUTED)
            .bg(gpui::rgb(0x17171a))
            .child(gpui::div().flex_none().w(px(28.)))
            .child(gpui::div().flex_none().w(px(COL_COVER)))
            .child(gpui::div().flex_1().min_w_0().child("TITLE"))
            .child(gpui::div().flex_none().w(px(220.)).child("WAVEFORM · CUES"))
            .child(gpui::div().flex_none().w(px(COL_ARTIST)).child("ARTIST"))
            .child(gpui::div().flex_none().w(px(COL_ALBUM)).child("ALBUM"))
            .child(
                gpui::div()
                    .flex_none()
                    .flex()
                    .justify_end()
                    .w(px(COL_BPM))
                    .child("BPM"),
            )
            .child(
                gpui::div()
                    .flex_none()
                    .flex()
                    .justify_center()
                    .w(px(COL_KEY))
                    .child("KEY"),
            )
            .child(
                gpui::div()
                    .flex_none()
                    .flex()
                    .justify_end()
                    .w(px(COL_TIME))
                    .child("TIME"),
            );

        let table_body = if shown.is_empty() {
            gpui::div()
                .flex()
                .flex_1()
                .items_center()
                .justify_center()
                .h(px(40.))
                .text_size(px(11.))
                .text_color(theme::MUTED)
                .child("No tracks")
                .into_any_element()
        } else {
            gpui::uniform_list("track-table", shown.len(), move |range, _window, _cx| {
                let mut rows = Vec::new();
                for ix in range {
                    let t = &shown[ix];
                    let selected = track_sel == Some(t.id.0);
                    let key = t.camelot.clone().or_else(|| t.key.clone());
                    let bpm = t
                        .bpm
                        .map(|b| format!("{b:.1}"))
                        .unwrap_or_else(|| "—".into());
                    let status_overlay = status::overlay(
                        analysis_statuses
                            .get(&t.id)
                            .unwrap_or(&crate::analysis::Status::Queued),
                    );
                    let dur = fmt_duration(t.duration_ms as u64);

                    let row = gpui::div()
                        .id(SharedString::from(format!("track-{}-{ix}", t.id.0)))
                        .relative()
                        .overflow_hidden()
                        .w_full()
                        .flex()
                        .items_center()
                        .h(px(40.))
                        .px(px(ROW_PAD))
                        .gap_2()
                        .text_size(px(12.))
                        .when(selected, |el| el.bg(theme::with_alpha(theme::ACCENT, 0.12)))
                        .when(!selected && ix % 2 == 1, |el| el.bg(gpui::rgb(0x101013)))
                        .when(!selected, |el| el.hover(|s| s.bg(gpui::rgb(0x1c1c20))))
                        .child(
                            gpui::div()
                                .flex()
                                .flex_none()
                                .w(px(28.))
                                .gap(px(2.))
                                .children(
                                    loaded_tracks
                                        .iter()
                                        .enumerate()
                                        .filter(|(_, id)| **id == Some(t.id))
                                        .map(|(i, _)| {
                                            gpui::div()
                                                .flex()
                                                .items_center()
                                                .justify_center()
                                                .w(px(13.))
                                                .h(px(17.))
                                                .rounded(px(2.))
                                                .bg(theme::PANEL_RAISED)
                                                .text_size(px(10.))
                                                .font_weight(gpui::FontWeight::BOLD)
                                                .text_color(theme::deck_color(if i == 0 {
                                                    mixless_protocol::DeckId::A
                                                } else {
                                                    mixless_protocol::DeckId::B
                                                }))
                                                .child(if i == 0 { "A" } else { "B" })
                                        }),
                                ),
                        )
                        .child(track_cover(t))
                        .child(
                            gpui::div()
                                .flex_1()
                                .min_w_0()
                                .text_color(theme::TEXT)
                                .overflow_hidden()
                                .child(t.title.clone()),
                        )
                        .child(crate::wave::preview::element(
                            previews.get(t, &preview_core),
                        ))
                        .child(
                            gpui::div()
                                .flex_none()
                                .w(px(COL_ARTIST))
                                .min_w_0()
                                .overflow_hidden()
                                .text_color(theme::TEXT)
                                .child(t.artist.clone()),
                        )
                        .child(
                            gpui::div()
                                .flex_none()
                                .w(px(COL_ALBUM))
                                .min_w_0()
                                .overflow_hidden()
                                .text_color(theme::MUTED)
                                .child(t.album.clone().unwrap_or_else(|| "—".into())),
                        )
                        .child(
                            gpui::div()
                                .flex()
                                .flex_none()
                                .justify_end()
                                .w(px(COL_BPM))
                                .text_size(px(11.))
                                .text_color(theme::MUTED)
                                .child(bpm),
                        )
                        .child(
                            gpui::div()
                                .flex()
                                .flex_none()
                                .justify_center()
                                .w(px(COL_KEY))
                                .child(
                                    gpui::div()
                                        .when(key.is_some(), |el| {
                                            el.px_2()
                                                .rounded(px(9.))
                                                .bg(theme::with_alpha(theme::ACCENT, 0.12))
                                                .border_1()
                                                .border_color(theme::with_alpha(
                                                    theme::ACCENT,
                                                    0.35,
                                                ))
                                                .text_size(px(10.))
                                                .font_weight(gpui::FontWeight::BOLD)
                                                .text_color(theme::ACCENT)
                                        })
                                        .child(key.unwrap_or_else(|| "—".into())),
                                ),
                        )
                        .child(
                            gpui::div()
                                .flex()
                                .flex_none()
                                .justify_end()
                                .w(px(COL_TIME))
                                .text_size(px(11.))
                                .text_color(theme::MUTED)
                                .child(dur),
                        )
                        .children(status_overlay)
                        .children([false, true].map(|after| {
                            drag::drop_target(
                                track_state.clone(),
                                shown.clone(),
                                menu_playlist,
                                ix,
                                after,
                            )
                        }));

                    let st = track_state.clone();
                    let menu_state = track_state.clone();
                    let track_id = t.id;
                    let menu_title = t.title.clone();
                    rows.push(
                        row.on_mouse_down(MouseButton::Right, move |ev, _, cx| {
                            cx.stop_propagation();
                            menu_state.update(cx, |s, cx| {
                                s.track_sel = Some(track_id.0);
                                s.track_menu = Some(TrackMenu {
                                    track: track_id,
                                    playlist: menu_playlist,
                                    title: menu_title.clone(),
                                    position: ev.position,
                                });
                                cx.notify();
                            });
                        })
                        .cursor_move()
                        .on_drag(
                            TrackDrag {
                                id: track_id,
                                title: t.title.clone(),
                                playlist: menu_playlist,
                                index: ix,
                                tracks: shown.clone(),
                            },
                            |drag, _, _, cx| cx.new(|_| drag.clone()),
                        )
                        .on_mouse_down(
                            MouseButton::Left,
                            move |ev: &MouseDownEvent, _window, cx| {
                                st.update(cx, |s, cx| {
                                    s.track_sel = Some(track_id.0);
                                    if ev.click_count >= 2 {
                                        s.load_deck(focus_deck, track_id);
                                    }
                                    cx.notify();
                                });
                            },
                        ),
                    );
                }
                rows
            })
            .track_scroll(self.track_scroll.clone())
            .on_drop(move |drag: &TrackDrag, _, cx| {
                // Rows consume their own drops; unused space below the rows
                // appends to the list without requiring a tiny final target.
                reorder_end_state.update(cx, |s, cx| {
                    s.reorder_track(drag, track_count);
                    cx.notify();
                });
            })
            .on_drag_move::<TrackDrag>({
                let scroll = self.track_scroll.clone();
                let active = self.track_scroll_active.clone();
                let view = self.library_view.downgrade();
                move |_, window, cx| {
                    drag::start_scroll(scroll.clone(), active.clone(), view.clone(), window, cx);
                }
            })
            .flex_1()
            .min_h_0()
            .into_any_element()
        };

        let main_col = gpui::div()
            .flex()
            .flex_1()
            .min_w_0()
            .flex_col()
            .overflow_hidden()
            .child(toolbar)
            .child(header)
            .when(!self.playlist_issues.is_empty(), |el| {
                el.child(
                    gpui::div()
                        .id("playlist-import-issues")
                        .flex_none()
                        .max_h(px(72.))
                        .overflow_y_scroll()
                        .px_2()
                        .py_1()
                        .text_size(px(10.))
                        .text_color(theme::MUTED)
                        .children(self.playlist_issues.iter().map(|item| {
                            gpui::div().child(format!(
                                "{} · {} — {} [{}] {}",
                                item.position + 1,
                                item.artist,
                                item.title,
                                item.status,
                                item.error
                                    .as_deref()
                                    .unwrap_or("Waiting for a local recording")
                            ))
                        })),
                )
            })
            .child(table_body);

        gpui::div()
            .id("library")
            .flex()
            .flex_1()
            .min_h(px(height))
            .size_full()
            .flex_col()
            .rounded(px(8.))
            .bg(theme::PANEL)
            .border_1()
            .border_color(theme::LINE)
            .overflow_hidden()
            .child(
                gpui::div()
                    .flex()
                    .flex_1()
                    .min_h_0()
                    .child(side)
                    .child(main_col),
            )
            .when(!self.acquire.is_empty(), |el| {
                el.child(
                    gpui::div()
                        .flex_none()
                        .h(px(18.))
                        .px_2()
                        .text_size(px(9.))
                        .text_color(theme::MUTED)
                        .overflow_hidden()
                        .child(self.import_status()),
                )
            })
            .into_any_element()
    }

    fn local_import_button(
        &self,
        cx: &mut gpui::Context<Self>,
        folders: bool,
        label: &'static str,
    ) -> gpui::AnyElement {
        let disabled = self.picker_open;
        gpui::div()
            .id(if folders {
                "choose-folders"
            } else {
                "choose-files"
            })
            .flex_none()
            .px_2()
            .py_1()
            .rounded(px(3.))
            .text_size(px(10.))
            .text_color(theme::TEXT)
            .bg(theme::PANEL_RAISED)
            .when(disabled, |el| el.opacity(0.4))
            .when(!disabled, |el| {
                el.cursor_pointer().hover(|s| s.bg(theme::LINE))
            })
            .child(label)
            .on_click(cx.listener(move |s, _, _, cx| {
                s.choose_local(folders, cx);
            }))
            .into_any_element()
    }

    pub fn render_import_modal(
        &self,
        window: &Window,
        cx: &mut gpui::Context<Self>,
    ) -> Option<gpui::AnyElement> {
        if !self.show_import_modal {
            return None;
        }

        let state = cx.entity();
        let dismiss_state = state.clone();
        let close_state = state.clone();
        let spotify_state = state.clone();
        let can_import_spotify = !self.url.trim().is_empty();

        let close = gpui::div()
            .id("import-modal-close")
            .flex()
            .items_center()
            .justify_center()
            .size(px(26.))
            .rounded(px(4.))
            .text_size(px(16.))
            .text_color(theme::MUTED)
            .hover(|s| s.bg(theme::PANEL_RAISED).text_color(theme::TEXT))
            .child("×")
            .on_click(move |_ev, window, cx| {
                window.blur();
                close_state.update(cx, |s, cx| {
                    s.show_import_modal = false;
                    cx.notify();
                });
            });

        let spotify_button = gpui::div()
            .id("import-spotify-playlist")
            .flex()
            .flex_none()
            .items_center()
            .justify_center()
            .h(px(26.))
            .px_3()
            .rounded(px(3.))
            .bg(theme::PANEL_RAISED)
            .border_1()
            .border_color(theme::LINE)
            .text_size(px(11.))
            .text_color(theme::TEXT)
            .when(!can_import_spotify, |el| el.opacity(0.45))
            .when(can_import_spotify, |el| {
                el.cursor_pointer().hover(|s| s.bg(theme::LINE))
            })
            .child("Import")
            .on_click(move |_ev, _window, cx| {
                if can_import_spotify {
                    spotify_state.update(cx, |s, cx| {
                        if !s.url.trim().is_empty() {
                            s.import_spotify(s.url.trim().to_string());
                            cx.notify();
                        }
                    });
                }
            });

        let panel = gpui::div()
            .id("import-modal-panel")
            .flex()
            .flex_col()
            .gap_3()
            .w(px(460.))
            .p_3()
            .rounded(px(theme::DIALOG_RADIUS))
            .bg(theme::PANEL)
            .border_1()
            .border_color(theme::LINE)
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .on_click(|_ev, _window, cx| cx.stop_propagation())
            .child(
                gpui::div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .pb_2()
                    .border_b_1()
                    .border_color(theme::LINE)
                    .child(gpui::div().text_size(px(13.)).child("Import music"))
                    .child(close),
            )
            .child(
                gpui::div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .child(gpui::div().flex_1().text_size(px(11.)).child("Local audio"))
                    .child(self.local_import_button(cx, false, "Choose files…"))
                    .child(self.local_import_button(cx, true, "Choose folder…")),
            )
            .child(
                gpui::div()
                    .text_size(px(10.))
                    .text_color(theme::MUTED)
                    .child("Includes subfolders. Existing tracks are skipped."),
            )
            .child(
                gpui::div()
                    .flex()
                    .flex_col()
                    .gap_2()
                    .pt_3()
                    .border_t_1()
                    .border_color(theme::LINE)
                    .child(gpui::div().text_size(px(11.)).child("Spotify playlist"))
                    .child(
                        gpui::div()
                            .flex()
                            .items_center()
                            .gap_2()
                            .child(self.render_url_input(window, cx))
                            .child(spotify_button),
                    )
                    .child(
                        gpui::div()
                            .text_size(px(10.))
                            .text_color(theme::MUTED)
                            .child("Public playlists only. Matches local audio first."),
                    ),
            )
            .when(!self.import_details.is_empty(), |el| {
                el.child(
                    gpui::div()
                        .id("import-details")
                        .max_h(px(120.))
                        .overflow_y_scroll()
                        .text_size(px(10.))
                        .text_color(theme::MUTED)
                        .children(
                            self.import_details
                                .iter()
                                .map(|detail| gpui::div().py_1().child(detail.clone())),
                        ),
                )
            })
            .when(!self.acquire.is_empty(), |el| {
                el.child(
                    gpui::div()
                        .pt_2()
                        .border_t_1()
                        .border_color(theme::LINE)
                        .text_size(px(10.))
                        .text_color(theme::MUTED)
                        .child(self.import_status()),
                )
            });

        Some(
            gpui::div()
                .id("import-modal-backdrop")
                .absolute()
                .top_0()
                .right_0()
                .bottom_0()
                .left_0()
                .occlude()
                .flex()
                .items_center()
                .justify_center()
                .bg(gpui::rgba(0x000000b8))
                .on_click(move |_ev, window, cx| {
                    window.blur();
                    dismiss_state.update(cx, |s, cx| {
                        s.show_import_modal = false;
                        cx.notify();
                    });
                })
                .child(panel)
                .into_any_element(),
        )
    }

    fn render_url_input(
        &self,
        window: &Window,
        cx: &mut gpui::Context<Self>,
    ) -> gpui::Stateful<gpui::Div> {
        let text = self.url.clone();
        let focus_handle = self.url_focus.clone();
        let focused = focus_handle.is_focused(window);

        let el = gpui::div()
            .id("import-url-input")
            .track_focus(&focus_handle)
            .flex()
            .flex_1()
            .min_w_0()
            .items_center()
            .gap_1()
            .h(px(26.))
            .px_2()
            .rounded(px(4.))
            .bg(gpui::rgb(0x0b0b0d))
            .border_1()
            .border_color(if focused {
                theme::with_alpha(theme::ACCENT, 0.5)
            } else {
                theme::LINE.into()
            })
            .on_mouse_down(MouseButton::Left, {
                let fh = focus_handle.clone();
                move |_ev, window, _cx| fh.focus(window)
            })
            .on_key_down(
                cx.listener(|s: &mut UiState, ev: &gpui::KeyDownEvent, window, cx| {
                    s.handle_url_input_key(&ev.keystroke, window, cx);
                    cx.stop_propagation();
                }),
            );

        let content = if text.is_empty() {
            gpui::div()
                .flex_1()
                .min_w_0()
                .text_size(px(11.))
                .text_color(gpui::rgb(0x5a5a60))
                .overflow_hidden()
                .child("https://open.spotify.com/playlist/…")
                .into_any_element()
        } else {
            gpui::div()
                .flex_1()
                .min_w_0()
                .text_size(px(11.))
                .text_color(theme::TEXT)
                .overflow_hidden()
                .child(text)
                .into_any_element()
        };

        el.child(content).child(
            gpui::div()
                .flex_none()
                .w(px(1.))
                .h(px(13.))
                .bg(theme::ACCENT)
                .when(!focused, |el| el.opacity(0.0)),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn folder_labels_use_the_shortest_unique_suffix() {
        let paths = [
            "/Music/House",
            "/Music/Studio/Set",
            "/Backup/Studio/Set",
            "/Archive/Set",
        ];
        let folders: Vec<_> = paths
            .iter()
            .enumerate()
            .map(|(i, path)| mixless_library::PlaylistSummary {
                id: i as i64,
                name: "unused".into(),
                tracks: 0,
                folder_path: Some((*path).into()),
            })
            .collect();
        let names: Vec<_> = folders
            .iter()
            .map(|folder| folder_name(folder, &folders))
            .collect();
        assert_eq!(
            names,
            [
                "House",
                "Music/Studio/Set",
                "Backup/Studio/Set",
                "Archive/Set"
            ]
        );
        assert!(names.iter().all(|name| !name.starts_with('/')));
    }
}

#[cfg(test)]
mod folder_display_tests {
    use super::*;
    #[test]
    fn friendly_folder_names_disambiguate_without_absolute_paths() {
        let item = |id, path: &str| mixless_library::PlaylistSummary {
            id,
            name: path.into(),
            tracks: 14,
            folder_path: Some(path.into()),
        };
        let a = item(1, "/music/North/playlist_G7SAZ_DnB_");
        assert_eq!(folder_name(&a, std::slice::from_ref(&a)), "DnB");
        let b = item(2, "/music/South/playlist_XYZ42_DnB_");
        let all = [a, b];
        assert_eq!(folder_name(&all[0], &all), "North/DnB");
        assert_eq!(folder_name(&all[1], &all), "South/DnB");
        assert_eq!(readable_folder_name("My_DnB"), "My_DnB");
        assert_eq!(
            readable_folder_name("playlist_live_set"),
            "playlist_live_set"
        );
    }
}

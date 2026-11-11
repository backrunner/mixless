//! Compact library browser with a source tree, aligned virtualized track
//! table, cached cover art, and a secondary import modal.

mod drag;
mod import;
pub(crate) mod import_rows;
mod menu;
mod metadata;
mod packages;
mod playlist_menu;
mod sources;
mod status;
pub use drag::TrackDrag;
pub use menu::TrackMenu;
pub use playlist_menu::PlaylistMenu;
pub(crate) use sources::{SourceEntry, SourceTree, playlist_sources};
use sources::{SourceIcon, source_count, source_icon};

use std::path::PathBuf;

use crate::state::UiState;
use crate::theme;
use crate::views::{TextTip, ellipsized_text};
mod cache;

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
const COL_TITLE: f32 = 200.;
const COL_ARTIST: f32 = 136.;
const COL_ALBUM: f32 = 120.;
const COL_BPM: f32 = 56.;
const COL_KEY: f32 = 56.;
const COL_TIME: f32 = 52.;
const ROW_PAD: f32 = 14.;
// Source headers and rows share the same compact height when fully expanded.
const SOURCE_ROW_HEIGHT: f32 = 24.;
const SOURCE_ROW_PAD: f32 = 6.;
const SOURCE_LIST_PAD: f32 = 4.;
// The import glyph and every source count share the same right edge.
const SOURCE_TRAILING_WIDTH: f32 = 22.;
const SOURCE_IMPORT_PAD: f32 = 7.;

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

pub(crate) fn folder_name(
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
    (Option<i64>, Option<crate::automix::QueueEntry>),
    mixless_protocol::DeckId,
    [Option<TrackId>; 2],
    [Option<u32>; 2],
    (u64, bool, bool, u64),
    bool,
    bool,
    SharedString,
    (u32, u32),
);

pub struct LibraryView {
    pub owner: gpui::WeakEntity<UiState>,
}
impl gpui::Render for LibraryView {
    fn render(&mut self, window: &mut Window, cx: &mut gpui::Context<Self>) -> impl IntoElement {
        let compact = f32::from(window.viewport_size().width) < 1360.;
        self.owner
            .update(cx, |state, cx| {
                state.render_library(window, cx, 160., compact)
            })
            .unwrap_or_else(|_| gpui::div().into_any_element())
    }
}

impl UiState {
    pub fn render_library(
        &mut self,
        window: &mut Window,
        cx: &mut gpui::Context<Self>,
        height: f32,
        compact: bool,
    ) -> gpui::AnyElement {
        let side = self.render_sources(window, cx);

        let shown = self.shown_tracks();
        let analysis_statuses = self.core.analysis.statuses();
        let stem_statuses = self.core.analysis.stem_statuses();
        let loaded_tracks = self.snapshot.decks.each_ref().map(|d| d.track_id);
        let active_rows = self
            .queue_active_rows(&shown)
            .zip(self.automix_queue.outgoing);
        let playheads =
            crate::wave::preview::positions(&self.snapshot.decks, self.presentation_frames);
        let track_sel = self.track_sel;
        let selected_entry = self.track_sel_entry;
        let shown_entries = self.shown_entries(&shown);
        let focus_deck = self.focus;
        let menu_playlist = self.playlist_sel;
        let track_state = cx.entity();
        let previews = self.library_previews.clone();
        let mix_overlays = self.library_mix_overlays(&shown);
        let preview_core = self.core.clone();
        let reorder_end_state = track_state.clone();
        let track_count = shown.len();
        let imports = self.playlist_imports.clone();
        let retrying: Vec<_> = imports
            .iter()
            .map(|item| self.import_retry_pending(menu_playlist, item))
            .collect();
        let display_rows = import_rows::rows(shown.len(), &imports);

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
            .child(gpui::div().flex_none().w(px(COL_TITLE)).child("TITLE"))
            .child(
                gpui::div()
                    .flex_1()
                    .min_w(px(280.))
                    .child("WAVEFORM / CUES"),
            )
            .child(gpui::div().flex_none().w(px(COL_ARTIST)).child("ARTIST"))
            .when(!compact, |el| {
                el.child(gpui::div().flex_none().w(px(COL_ALBUM)).child("ALBUM"))
            })
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

        let table_body = if display_rows.is_empty() {
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
            gpui::uniform_list(
                "track-table",
                display_rows.len(),
                move |range, _window, _cx| {
                    let mut rows = Vec::new();
                    for row_ix in range {
                        let ix = match display_rows[row_ix] {
                            import_rows::Row::Track(ix) => ix,
                            import_rows::Row::Import(ix) => {
                                rows.push(import_rows::element(
                                    &imports[ix],
                                    row_ix,
                                    compact,
                                    menu_playlist,
                                    retrying[ix],
                                    track_state.clone(),
                                ));
                                continue;
                            }
                        };
                        let t = &shown[ix];
                        let entry = shown_entries[ix];
                        let selected = track_sel == Some(t.id.0)
                            && selected_entry
                                .filter(|e| Some(e.track.0) == track_sel)
                                .is_none_or(|e| e == entry);
                        let key = t.camelot.clone().or_else(|| t.key.clone());
                        let key_color =
                            key.as_deref().map(theme::key_color).unwrap_or(theme::MUTED);
                        let bpm = t
                            .bpm
                            .map(|b| format!("{b:.1}"))
                            .unwrap_or_else(|| "—".into());
                        let status_overlay = status::overlay(
                            analysis_statuses
                                .get(&t.id)
                                .unwrap_or(&crate::analysis::Status::Queued),
                            t.id,
                            track_state.clone(),
                        );
                        let dur = fmt_duration(t.duration_ms as u64);

                        let row = gpui::div()
                            .id(SharedString::from(format!(
                                "track-{}-{}",
                                t.id.0, entry.occurrence
                            )))
                            .debug_selector({
                                let id = t.id.0;
                                move || format!("track-row-{id}")
                            })
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
                            .when(!selected && row_ix % 2 == 1, |el| {
                                el.bg(gpui::rgb(0x101013))
                            })
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
                                            .filter(|(deck, id)| {
                                                **id == Some(t.id)
                                                    && active_rows.is_none_or(|((a, b), out)| {
                                                        ix == if *deck == out.index() {
                                                            a
                                                        } else {
                                                            b
                                                        }
                                                    })
                                            })
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
                            .child(metadata::cell(
                                format!("track-title-{row_ix}"),
                                t.title.clone(),
                                COL_TITLE,
                                theme::TEXT,
                            ))
                            .child(crate::wave::preview::element(
                                &previews,
                                t,
                                &preview_core,
                                mix_overlays.get(ix).copied().unwrap_or_default(),
                                std::array::from_fn(|i| {
                                    (loaded_tracks[i] == Some(t.id)
                                        && active_rows.is_none_or(|((a, b), out)| {
                                            ix == if i == out.index() { a } else { b }
                                        }))
                                    .then_some(playheads[i])
                                    .flatten()
                                }),
                            ))
                            .child(metadata::cell(
                                format!("track-artist-{row_ix}"),
                                t.artist.clone(),
                                COL_ARTIST,
                                theme::TEXT,
                            ))
                            .when(!compact, |el| {
                                el.child(metadata::cell(
                                    format!("track-album-{row_ix}"),
                                    t.album.clone().unwrap_or_else(|| "—".into()),
                                    COL_ALBUM,
                                    theme::MUTED,
                                ))
                            })
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
                                                    .bg(theme::with_alpha(key_color, 0.12))
                                                    .border_1()
                                                    .border_color(theme::with_alpha(
                                                        key_color, 0.35,
                                                    ))
                                                    .text_size(px(10.))
                                                    .font_weight(gpui::FontWeight::BOLD)
                                                    .text_color(key_color)
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
                            .children(status::stem_badge(stem_statuses.get(&t.id)))
                            .children([false, true].map(|after| {
                                drag::drop_target(
                                    track_state.clone(),
                                    shown.clone(),
                                    menu_playlist,
                                    ix,
                                    after,
                                )
                            }))
                            .children(status_overlay);

                        let st = track_state.clone();
                        let menu_state = track_state.clone();
                        let track_id = t.id;
                        let menu_title = t.title.clone();
                        rows.push(
                            row.on_mouse_down(MouseButton::Right, move |ev, _, cx| {
                                cx.stop_propagation();
                                menu_state.update(cx, |s, cx| {
                                    s.track_sel = Some(track_id.0);
                                    s.track_sel_entry = Some(entry);
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
                                        s.track_sel_entry = Some(entry);
                                        if ev.click_count >= 2 {
                                            s.load_deck(focus_deck, track_id);
                                        }
                                        cx.notify();
                                    });
                                },
                            )
                            .into_any_element(),
                        );
                    }
                    rows
                },
            )
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
            .child(header)
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

    fn download_dir_button(&self, cx: &mut gpui::Context<Self>) -> gpui::AnyElement {
        let disabled = self.picker_open;
        gpui::div()
            .id("choose-download-dir")
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
            .child("Choose…")
            .on_click(cx.listener(move |s, _, _, cx| {
                s.choose_download_dir(cx);
            }))
            .into_any_element()
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
                failed_imports: 0,
                spotify_id: None,
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
            failed_imports: 0,
            spotify_id: None,
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

//! Compact library browser with a source tree, aligned virtualized track
//! table, cached cover art, and a secondary import modal.

use std::path::PathBuf;

use crate::state::UiState;
use crate::theme;
use gpui::prelude::*;
use gpui::{IntoElement, MouseButton, MouseDownEvent, SharedString, Styled, Window, px};
use mixless_protocol::Track;

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

impl UiState {
    pub fn render_library(&self, cx: &mut gpui::Context<Self>, height: f32) -> gpui::AnyElement {
        let state = cx.entity();

        let all_selected = self.playlist_sel.is_none();
        let all_tracks_row = {
            let el = gpui::div()
                .id("tree-all")
                .flex()
                .items_center()
                .gap_2()
                .w_full()
                .h(px(26.))
                .pl(px(26.))
                .pr_2()
                .rounded(px(3.))
                .text_size(px(11.))
                .text_color(if all_selected {
                    theme::TEXT
                } else {
                    theme::MUTED
                })
                .when(all_selected, |el| {
                    el.bg(theme::with_alpha(theme::ACCENT, 0.12))
                })
                .when(!all_selected, |el| el.hover(|s| s.bg(theme::PANEL_RAISED)))
                .child(
                    gpui::div()
                        .flex_none()
                        .text_size(px(10.))
                        .text_color(theme::MUTED)
                        .child("♪"),
                )
                .child(
                    gpui::div()
                        .flex_1()
                        .min_w_0()
                        .overflow_hidden()
                        .child("All Tracks"),
                )
                .child(
                    gpui::div()
                        .flex_none()
                        .text_size(px(10.))
                        .text_color(theme::MUTED)
                        .child(self.tracks.len().to_string()),
                );
            let st = state.clone();
            el.on_click(move |_ev, _window, cx| {
                st.update(cx, |s, cx| {
                    s.select_playlist(None);
                    cx.notify();
                });
            })
        };

        let local_root = gpui::div()
            .flex()
            .flex_none()
            .items_center()
            .gap_1()
            .w_full()
            .h(px(24.))
            .px_2()
            .text_size(px(9.))
            .font_weight(gpui::FontWeight::BOLD)
            .text_color(theme::MUTED)
            .child(gpui::div().flex_none().text_size(px(8.)).child("▾"))
            .child("LOCAL");

        let spotify_root = gpui::div()
            .flex()
            .flex_none()
            .items_center()
            .gap_1()
            .w_full()
            .h(px(22.))
            .px_2()
            .mt_1()
            .text_size(px(9.))
            .font_weight(gpui::FontWeight::BOLD)
            .text_color(theme::MUTED)
            .child(gpui::div().flex_none().text_size(px(8.)).child("▾"))
            .child("SPOTIFY");

        let playlists = self.playlists.clone();
        let playlist_sel = self.playlist_sel;
        let playlist_state = cx.entity();
        let playlist_list = gpui::uniform_list(
            "playlist-list",
            playlists.len(),
            move |range, _window, _cx| {
                let mut rows = Vec::new();
                for ix in range {
                    let pl = &playlists[ix];
                    let selected = playlist_sel == Some(pl.id);
                    let row = gpui::div()
                        .id(SharedString::from(format!("pl-{}", pl.id)))
                        .flex()
                        .items_center()
                        .gap_2()
                        .w_full()
                        .h(px(25.))
                        .pl(px(26.))
                        .pr_2()
                        .rounded(px(3.))
                        .text_size(px(11.))
                        .text_color(if selected { theme::TEXT } else { theme::MUTED })
                        .when(selected, |el| el.bg(theme::with_alpha(theme::ACCENT, 0.12)))
                        .when(!selected, |el| el.hover(|s| s.bg(theme::PANEL_RAISED)))
                        .child(
                            gpui::div()
                                .flex_none()
                                .text_size(px(9.))
                                .text_color(theme::MUTED)
                                .child("≡"),
                        )
                        .child(
                            gpui::div()
                                .flex_1()
                                .min_w_0()
                                .overflow_hidden()
                                .child(pl.name.clone()),
                        )
                        .child(
                            gpui::div()
                                .flex_none()
                                .text_size(px(10.))
                                .text_color(theme::MUTED)
                                .child(pl.tracks.to_string()),
                        );
                    let st = playlist_state.clone();
                    let id = pl.id;
                    rows.push(row.on_click(move |_ev, _window, cx| {
                        st.update(cx, |s, cx| {
                            s.select_playlist(Some(id));
                            cx.notify();
                        });
                    }));
                }
                rows
            },
        )
        .flex_1()
        .min_h_0();

        let import_button = {
            let label = if self.busy {
                "IMPORTING…"
            } else {
                "IMPORT AUDIO"
            };
            let el = gpui::div()
                .id("tree-import-button")
                .flex()
                .items_center()
                .gap_2()
                .w_full()
                .h(px(30.))
                .px_2()
                .rounded(px(4.))
                .bg(theme::ACCENT)
                .text_size(px(10.))
                .font_weight(gpui::FontWeight::BOLD)
                .text_color(theme::BG)
                .hover(|s| s.bg(theme::TEXT))
                .child(gpui::div().flex_none().text_size(px(13.)).child("＋"))
                .child(label);
            let st = state.clone();
            el.on_click(move |_ev, _window, cx| {
                st.update(cx, |s, cx| {
                    s.show_import_modal = true;
                    cx.notify();
                });
            })
        };

        let footer = gpui::div()
            .flex()
            .flex_none()
            .flex_col()
            .gap_1()
            .p_2()
            .when(self.busy && !self.acquire.is_empty(), |el| {
                el.child(
                    gpui::div()
                        .h(px(14.))
                        .text_size(px(9.))
                        .text_color(theme::MUTED)
                        .overflow_hidden()
                        .child(self.acquire.clone()),
                )
            })
            .child(import_button);

        let side = gpui::div()
            .flex()
            .flex_none()
            .w(px(192.))
            .flex_col()
            .py_2()
            .bg(gpui::rgb(0x0e0e10))
            .overflow_hidden()
            .child(local_root)
            .child(all_tracks_row)
            .child(spotify_root)
            .child(
                gpui::div()
                    .flex_none()
                    .px_2()
                    .pb_1()
                    .text_size(px(8.))
                    .text_color(theme::MUTED)
                    .child("PLAYLISTS"),
            )
            .child(playlist_list)
            .child(footer);

        let shown = self.tracks.clone();
        let track_sel = self.track_sel;
        let focus_deck = self.focus;
        let track_state = cx.entity();

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
            .child(gpui::div().flex_none().w(px(COL_COVER)))
            .child(gpui::div().flex_1().min_w_0().child("TITLE"))
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
                .child("Import audio or fetch a Spotify playlist to start.")
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
                    let dur = fmt_duration(t.duration_ms as u64);

                    let row = gpui::div()
                        .id(SharedString::from(format!("track-{}", t.id.0)))
                        .flex()
                        .items_center()
                        .h(px(30.))
                        .px(px(ROW_PAD))
                        .gap_2()
                        .text_size(px(12.))
                        .when(selected, |el| el.bg(theme::with_alpha(theme::ACCENT, 0.12)))
                        .when(!selected && ix % 2 == 1, |el| el.bg(gpui::rgb(0x101013)))
                        .when(!selected, |el| el.hover(|s| s.bg(gpui::rgb(0x1c1c20))))
                        .child(track_cover(t))
                        .child(
                            gpui::div()
                                .flex_1()
                                .min_w_0()
                                .text_color(theme::TEXT)
                                .overflow_hidden()
                                .child(t.title.clone()),
                        )
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
                        );

                    let st = track_state.clone();
                    let track_id = t.id;
                    rows.push(row.on_mouse_down(
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
                    ));
                }
                rows
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
            .flex_none()
            .h(px(height))
            .rounded(px(8.))
            .bg(theme::PANEL)
            .border_1()
            .border_color(theme::LINE)
            .overflow_hidden()
            .child(side)
            .child(main_col)
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
        let local_state = state.clone();
        let spotify_state = state.clone();
        let busy = self.busy;
        let can_import_spotify = !busy && !self.url.trim().is_empty();

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

        let local_button = gpui::div()
            .id("import-local-files")
            .flex()
            .flex_none()
            .items_center()
            .justify_center()
            .h(px(32.))
            .px_3()
            .rounded(px(4.))
            .bg(theme::ACCENT)
            .text_size(px(10.))
            .font_weight(gpui::FontWeight::BOLD)
            .text_color(theme::BG)
            .when(busy, |el| el.opacity(0.45))
            .when(!busy, |el| el.hover(|s| s.bg(theme::TEXT)))
            .child("CHOOSE FILES")
            .on_click(move |_ev, _window, cx| {
                if busy {
                    return;
                }
                let paths = rfd::FileDialog::new()
                    .add_filter(
                        "Audio",
                        &["wav", "mp3", "flac", "aiff", "aif", "ogg", "m4a", "aac"],
                    )
                    .pick_files();
                if let Some(paths) = paths.filter(|paths| !paths.is_empty()) {
                    local_state.update(cx, |s, cx| {
                        s.import_files(paths);
                        cx.notify();
                    });
                }
            });

        let spotify_button = gpui::div()
            .id("import-spotify-playlist")
            .flex()
            .flex_none()
            .items_center()
            .justify_center()
            .h(px(32.))
            .px_3()
            .rounded(px(4.))
            .bg(theme::ACCENT)
            .text_size(px(10.))
            .font_weight(gpui::FontWeight::BOLD)
            .text_color(theme::BG)
            .when(!can_import_spotify, |el| el.opacity(0.45))
            .when(can_import_spotify, |el| el.hover(|s| s.bg(theme::TEXT)))
            .child("IMPORT PLAYLIST")
            .on_click(move |_ev, _window, cx| {
                if !can_import_spotify {
                    return;
                }
                spotify_state.update(cx, |s, cx| {
                    if !s.busy && !s.url.trim().is_empty() {
                        s.import_spotify(s.url.trim().to_string());
                        cx.notify();
                    }
                });
            });

        let panel = gpui::div()
            .id("import-modal-panel")
            .flex()
            .flex_col()
            .gap_3()
            .w(px(520.))
            .p_4()
            .rounded(px(9.))
            .bg(gpui::rgb(0x151518))
            .border_1()
            .border_color(theme::with_alpha(theme::ACCENT, 0.3))
            .on_click(|_ev, _window, cx| cx.stop_propagation())
            .child(
                gpui::div()
                    .flex()
                    .items_center()
                    .child(
                        gpui::div()
                            .flex_1()
                            .flex_col()
                            .gap_1()
                            .child(
                                gpui::div()
                                    .text_size(px(16.))
                                    .font_weight(gpui::FontWeight::BOLD)
                                    .child("Import music"),
                            )
                            .child(
                                gpui::div()
                                    .text_size(px(10.))
                                    .text_color(theme::MUTED)
                                    .child("Add local audio or acquire a Spotify playlist."),
                            ),
                    )
                    .child(close),
            )
            .child(
                gpui::div()
                    .flex()
                    .items_center()
                    .gap_3()
                    .p_3()
                    .rounded(px(6.))
                    .bg(theme::PANEL_INSET)
                    .border_1()
                    .border_color(theme::LINE)
                    .child(
                        gpui::div()
                            .flex()
                            .flex_none()
                            .items_center()
                            .justify_center()
                            .size(px(38.))
                            .rounded(px(5.))
                            .bg(theme::with_alpha(theme::ACCENT, 0.14))
                            .text_size(px(18.))
                            .text_color(theme::ACCENT)
                            .child("♪"),
                    )
                    .child(
                        gpui::div()
                            .flex_1()
                            .min_w_0()
                            .flex_col()
                            .gap_1()
                            .child(
                                gpui::div()
                                    .text_size(px(12.))
                                    .font_weight(gpui::FontWeight::BOLD)
                                    .child("Local audio"),
                            )
                            .child(
                                gpui::div()
                                    .text_size(px(10.))
                                    .text_color(theme::MUTED)
                                    .child("Select one or more audio files."),
                            ),
                    )
                    .child(local_button),
            )
            .child(
                gpui::div()
                    .flex()
                    .flex_col()
                    .gap_2()
                    .p_3()
                    .rounded(px(6.))
                    .bg(theme::PANEL_INSET)
                    .border_1()
                    .border_color(theme::LINE)
                    .child(
                        gpui::div()
                            .text_size(px(12.))
                            .font_weight(gpui::FontWeight::BOLD)
                            .child("Spotify playlist"),
                    )
                    .child(
                        gpui::div()
                            .text_size(px(10.))
                            .text_color(theme::MUTED)
                            .child("Paste a public playlist URL. Requires yt-dlp and ffmpeg."),
                    )
                    .child(
                        gpui::div()
                            .flex()
                            .items_center()
                            .gap_2()
                            .child(self.render_url_input(window, cx))
                            .child(spotify_button),
                    ),
            )
            .child(gpui::div().text_size(px(10.)).text_color(theme::MUTED)
                .child("Public playlist links: match local audio first, then search YouTube Music / YouTube. Spotify previews may omit tracks; private playlists are not supported here yet."))
            .when(!self.import_details.is_empty(), |el| {
                el.child(gpui::div().id("import-details").max_h(px(120.)).overflow_y_scroll()
                    .text_size(px(10.)).text_color(theme::MUTED)
                    .children(self.import_details.iter().map(|detail|gpui::div().py_1().child(detail.clone()))))
            })
            .when(!self.acquire.is_empty(), |el| {
                el.child(
                    gpui::div()
                        .flex()
                        .items_center()
                        .gap_2()
                        .h(px(26.))
                        .px_2()
                        .rounded(px(4.))
                        .bg(theme::with_alpha(theme::ACCENT, 0.08))
                        .text_size(px(10.))
                        .text_color(theme::MUTED)
                        .child(gpui::div().flex_none().size(px(6.)).rounded_full().bg(
                            if self.busy {
                                theme::ACCENT
                            } else {
                                theme::LED_GREEN
                            },
                        ))
                        .child(
                            gpui::div()
                                .flex_1()
                                .min_w_0()
                                .overflow_hidden()
                                .child(self.acquire.clone()),
                        ),
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
            .h(px(32.))
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

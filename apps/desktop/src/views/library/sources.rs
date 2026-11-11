//! Collapsible source groups with clipped height transitions and virtual rows.
use super::*;
mod state;
pub(crate) use state::SourceTree;

#[derive(Clone, Copy, PartialEq)]
pub(crate) enum SourceGroup {
    Folders,
    Playlists,
}
impl SourceGroup {
    fn of(folder: bool) -> Self {
        if folder {
            Self::Folders
        } else {
            Self::Playlists
        }
    }
    fn label(self) -> &'static str {
        match self {
            Self::Folders => "LOCAL FOLDERS",
            Self::Playlists => "PLAYLISTS",
        }
    }
    fn id(self) -> &'static str {
        match self {
            Self::Folders => "source-folders",
            Self::Playlists => "source-playlists",
        }
    }
}

#[derive(Clone, Copy)]
pub(super) enum SourceIcon {
    Collection,
    Folder,
    Playlist,
}

#[derive(Clone, Copy, PartialEq)]
pub(crate) enum SourceEntry {
    Group(SourceGroup, usize),
    Playlist(usize),
}

pub(crate) fn playlist_sources(playlists: &[mixless_library::PlaylistSummary]) -> Vec<SourceEntry> {
    let mut sources = Vec::new();
    for (group, folders) in [
        (SourceGroup::Folders, true),
        (SourceGroup::Playlists, false),
    ] {
        let indices: Vec<_> = playlists
            .iter()
            .enumerate()
            .filter(|(_, playlist)| playlist.folder_path.is_some() == folders)
            .map(|(index, _)| index)
            .collect();
        if !indices.is_empty() {
            sources.push(SourceEntry::Group(group, indices.len()));
            sources.extend(indices.into_iter().map(SourceEntry::Playlist));
        }
    }
    sources
}

pub(super) fn source_count(count: impl ToString) -> gpui::Div {
    gpui::div()
        .flex_none()
        .w(px(SOURCE_TRAILING_WIDTH))
        .text_align(gpui::TextAlign::Right)
        .text_size(px(10.))
        .line_height(px(16.))
        .text_color(theme::MUTED)
        .child(count.to_string())
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
        .min_w_0()
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
        .child(source_icon(icon, color))
        .child(ellipsized_text(name).flex_1().w_0().min_w_0().h(px(16.)))
        .when_some(count, |el, count| el.child(source_count(count)))
}

pub(super) fn source_icon(icon: SourceIcon, color: gpui::Rgba) -> impl IntoElement {
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
    .size(px(12.))
}

impl UiState {
    fn toggle_source_group(&mut self, group: SourceGroup, cx: &mut gpui::Context<Self>) {
        self.source_tree.toggle(group);
        self.playlist_menu = None;
        self.library_key = None;
        self.library_view.update(cx, |_, cx| cx.notify());
        cx.notify();
    }

    pub(super) fn render_sources(
        &mut self,
        window: &mut Window,
        cx: &mut gpui::Context<Self>,
    ) -> gpui::AnyElement {
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

        let focus_handles = self.source_tree.focus_handles(cx);
        let (sources, progress, animating) = self.source_tree.frame(
            &self.playlists,
            &self.playlist_scroll,
            crate::motion::reduce_motion(),
        );
        if animating {
            let view = self.library_view.downgrade();
            window.on_next_frame(move |_, cx| {
                let _ = view.update(cx, |_, cx| cx.notify());
            });
        }
        let playlists = self.playlists.clone();
        let playlist_sel = self.playlist_sel;
        let playlist_state = cx.entity();
        let playlist_list = gpui::list(self.playlist_scroll.clone(), move |ix, _, _| {
            let (entry, height) = sources[ix];
            let index = match entry {
                SourceEntry::Group(group, count) => {
                    return group_header(
                        group,
                        count,
                        progress[group as usize],
                        focus_handles[group as usize].clone(),
                        playlist_state.clone(),
                    )
                    .into_any_element();
                }
                SourceEntry::Playlist(index) => index,
            };
            let pl = &playlists[index];
            let display_name = folder_name(pl, &playlists);
            let tooltip = match &pl.folder_path {
                Some(path) => format!("{display_name}\n{path}"),
                None => display_name.clone(),
            };
            let st = playlist_state.clone();
            let menu_state = playlist_state.clone();
            let id = pl.id;
            let row = source_row(
                SharedString::from(format!("pl-{id}")),
                display_name.clone(),
                if pl.folder_path.is_some() {
                    SourceIcon::Folder
                } else {
                    SourceIcon::Playlist
                },
                Some(pl.tracks),
                playlist_sel == Some(id),
            )
            .tooltip(move |_, cx| cx.new(|_| TextTip(tooltip.clone())).into())
            .on_click(move |_, _, cx| {
                st.update(cx, |s, cx| {
                    s.select_playlist(Some(id));
                    cx.notify();
                });
            })
            .on_mouse_down(MouseButton::Right, move |ev, _, cx| {
                cx.stop_propagation();
                menu_state.update(cx, |s, cx| {
                    s.track_menu = None;
                    s.playlist_menu = Some(PlaylistMenu {
                        id,
                        name: display_name.clone(),
                        position: ev.position,
                    });
                    cx.notify();
                });
            });
            gpui::div()
                .w_full()
                .h(px(height))
                .overflow_hidden()
                .child(row)
                .into_any_element()
        })
        .flex_1()
        .w_full()
        .min_w_0()
        .min_h_0();
        gpui::div()
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
                    .id("library-header")
                    .debug_selector(|| "library-header".into())
                    .on_mouse_down(
                        MouseButton::Right,
                        cx.listener(|s, ev: &MouseDownEvent, window, cx| {
                            s.track_menu = None;
                            s.playlist_menu = None;
                            s.deck_menu = None;
                            s.brand_menu.open = false;
                            s.audio.open = false;
                            s.package.menu = Some(ev.position);
                            s.keyboard_focus.focus(window);
                            cx.stop_propagation();
                            cx.notify();
                        }),
                    )
                    .flex()
                    .flex_none()
                    .items_center()
                    .justify_between()
                    .h(px(30.))
                    .px(px(SOURCE_LIST_PAD + SOURCE_ROW_PAD))
                    .border_b_1()
                    .border_color(theme::LINE)
                    .text_size(px(10.))
                    .text_color(theme::MUTED)
                    .child("LIBRARY")
                    .child(
                        gpui::div()
                            .id("library-import")
                            .flex()
                            .flex_none()
                            .items_center()
                            .justify_center()
                            .px(px(SOURCE_IMPORT_PAD))
                            // Extend the hover background equally around the aligned glyph.
                            .mr(px(-SOURCE_IMPORT_PAD))
                            .h(px(18.))
                            .rounded(px(3.))
                            .cursor_pointer()
                            .text_size(px(13.))
                            .line_height(px(18.))
                            .hover(|s| s.bg(theme::PANEL_RAISED).text_color(theme::TEXT))
                            .child("+")
                            .tooltip(|_, cx| cx.new(|_| TextTip("Import music".into())).into())
                            .on_click(cx.listener(|s, _, _, cx| {
                                s.package.export_open = false;
                                s.package.menu = None;
                                s.show_import_modal = true;
                                cx.notify();
                            })),
                    ),
            )
            .child(
                gpui::div()
                    .flex()
                    .flex_col()
                    .flex_1()
                    .w_full()
                    .min_w_0()
                    .min_h_0()
                    .px(px(SOURCE_LIST_PAD))
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
            )
            .into_any_element()
    }
}

fn group_header(
    group: SourceGroup,
    count: usize,
    progress: f32,
    focus: gpui::FocusHandle,
    state: gpui::Entity<UiState>,
) -> impl IntoElement {
    gpui::div()
        .id(group.id())
        .debug_selector(move || group.id().into())
        .flex()
        .items_center()
        .gap(px(6.))
        .w_full()
        .h(px(SOURCE_ROW_HEIGHT))
        .px(px(SOURCE_ROW_PAD))
        .rounded(px(3.))
        .cursor_pointer()
        .text_size(px(9.))
        .line_height(px(14.))
        .font_weight(gpui::FontWeight::MEDIUM)
        .text_color(theme::MUTED)
        .hover(|s| s.bg(theme::PANEL_RAISED).text_color(theme::TEXT))
        .track_focus(&focus)
        .on_click(move |_, window, cx| {
            focus.focus(window);
            cx.stop_propagation();
            state.update(cx, |s, cx| s.toggle_source_group(group, cx));
        })
        .on_key_down(move |event, _, cx| {
            // GPUI invokes on_click on key release. Only suppress the global
            // transport shortcut here; toggling on key-down would run twice.
            if !event.keystroke.modifiers.modified()
                && matches!(event.keystroke.key.as_str(), "enter" | "space")
            {
                cx.stop_propagation();
            }
        })
        .child(disclosure_arrow(progress))
        .child(gpui::div().flex_1().child(group.label()))
        .child(source_count(count))
}

fn disclosure_arrow(progress: f32) -> impl IntoElement {
    gpui::canvas(
        |_, _, _| (),
        move |bounds, _, window, _| {
            let angle = progress * std::f32::consts::FRAC_PI_2;
            let mut path = gpui::PathBuilder::stroke(px(1.2));
            for (i, (x, y)) in [(-2., -3.), (1., 0.), (-2., 3.)].into_iter().enumerate() {
                let p = bounds.center()
                    + gpui::point(
                        px(x * angle.cos() - y * angle.sin()),
                        px(x * angle.sin() + y * angle.cos()),
                    );
                if i == 0 {
                    path.move_to(p);
                } else {
                    path.line_to(p);
                }
            }
            if let Ok(path) = path.build() {
                window.paint_path(path, theme::MUTED);
            }
        },
    )
    .size(px(12.))
    .flex_none()
}

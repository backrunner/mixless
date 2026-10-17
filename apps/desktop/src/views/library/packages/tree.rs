use super::super::{SourceIcon, folder_name, source_count, source_icon};
use super::widgets::{checkbox, disclosure};
use crate::{
    state::{PackageNode, UiState},
    theme,
    views::{TextTip, ellipsized_text},
};
use gpui::{AnyElement, Context, Window, div, prelude::*, px};

impl UiState {
    fn package_tree_row(
        &self,
        node: PackageNode,
        label: String,
        count: Option<u32>,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let branch = node.branch();
        let expanded = branch.is_some_and(|i| self.package.expanded[i]);
        let disabled = self.package.busy() || self.picker_open;
        let depth = match node {
            PackageNode::Library => 0.,
            PackageNode::Playlist(_) => 2.,
            _ => 1.,
        };
        let icon = match node {
            PackageNode::Library => SourceIcon::Collection,
            PackageNode::Folders => SourceIcon::Folder,
            PackageNode::Playlists => SourceIcon::Playlist,
            PackageNode::Playlist(id) => {
                if self
                    .playlists
                    .iter()
                    .any(|p| p.id == id && p.folder_path.is_some())
                {
                    SourceIcon::Folder
                } else {
                    SourceIcon::Playlist
                }
            }
        };
        let id = match node {
            PackageNode::Library => "package-entire-library".to_string(),
            PackageNode::Folders => "package-folders".into(),
            PackageNode::Playlists => "package-playlists".into(),
            PackageNode::Playlist(id) => format!("package-playlist-{id}"),
        };
        let selector = id.clone();
        let tip = label.clone();
        div()
            .id(gpui::SharedString::from(id.clone()))
            .debug_selector(move || selector.clone())
            .flex()
            .flex_none()
            .items_center()
            .gap(px(6.))
            .w_full()
            .min_w_0()
            .h(px(30.))
            .pl(px(4. + 18. * depth))
            .pr(px(8.))
            .rounded(px(3.))
            .border_1()
            .border_color(gpui::transparent_black())
            .text_size(px(11.))
            .line_height(px(16.))
            .when(!disabled, |el| {
                el.tab_index(0)
                    .cursor_pointer()
                    .hover(|s| s.bg(theme::PANEL_RAISED))
                    .focus(|s| s.border_color(theme::ACCENT))
            })
            .when(disabled, |el| el.opacity(0.45))
            .child(
                div()
                    .id(gpui::SharedString::from(format!("{id}-disclosure")))
                    .debug_selector(move || format!("{id}-disclosure"))
                    .flex_none()
                    .size(px(18.))
                    .flex()
                    .items_center()
                    .justify_center()
                    .when_some(branch, |el, index| {
                        el.cursor_pointer()
                            .rounded(px(2.))
                            .hover(|s| s.bg(theme::LINE))
                            .child(disclosure(expanded))
                            .on_click(cx.listener(move |s, _, _, cx| {
                                s.package.expanded[index] = !s.package.expanded[index];
                                cx.stop_propagation();
                                cx.notify();
                            }))
                    }),
            )
            .child(checkbox(self.package.check_state(node, &self.playlists)))
            .child(source_icon(icon, theme::MUTED))
            .child(ellipsized_text(label).flex_1().w_0().min_w_0().h(px(16.)))
            .when_some(count, |el, count| el.child(source_count(count)))
            .tooltip(move |_, cx| cx.new(|_| TextTip(tip.clone())).into())
            .on_click(cx.listener(move |s, _, _, cx| {
                if !s.package.busy() && !s.picker_open {
                    s.package.toggle_selection(node, &s.playlists);
                    cx.notify();
                }
                cx.stop_propagation();
            }))
            .on_key_down(cx.listener(move |s, ev: &gpui::KeyDownEvent, window, cx| {
                match ev.keystroke.key.as_str() {
                    "left" | "right" if branch.is_some() => {
                        s.package.expanded[branch.unwrap()] = ev.keystroke.key == "right";
                    }
                    "up" => window.focus_prev(),
                    "down" => window.focus_next(),
                    _ => return,
                }
                cx.stop_propagation();
                cx.notify();
            }))
            .into_any_element()
    }

    pub(super) fn render_package_tree(
        &self,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let mut tree = div()
            .id("package-tree")
            .debug_selector(|| "package-tree".into())
            .flex()
            .flex_col()
            .min_h_0()
            .max_h(px(320.).min((window.viewport_size().height - px(220.)).max(px(60.))))
            .overflow_y_scroll()
            .child(self.package_tree_row(PackageNode::Library, "Entire library".into(), None, cx));
        if self.package.expanded[0] {
            for (node, label) in [
                (PackageNode::Folders, "Local folders"),
                (PackageNode::Playlists, "Playlists"),
            ] {
                let playlists: Vec<_> =
                    self.playlists.iter().filter(|p| node.includes(p)).collect();
                if playlists.is_empty() {
                    continue;
                }
                tree = tree.child(self.package_tree_row(
                    node,
                    label.into(),
                    Some(playlists.len() as u32),
                    cx,
                ));
                if self.package.expanded[node.branch().unwrap()] {
                    for playlist in playlists {
                        tree = tree.child(self.package_tree_row(
                            PackageNode::Playlist(playlist.id),
                            folder_name(playlist, &self.playlists),
                            Some(playlist.tracks),
                            cx,
                        ));
                    }
                }
            }
        }
        tree.into_any_element()
    }
}

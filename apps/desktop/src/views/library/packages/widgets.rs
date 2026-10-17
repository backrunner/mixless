use crate::{
    state::{CheckState, PackageAction, UiState},
    theme,
};
use gpui::{AnyElement, Context, Div, Stateful, div, prelude::*, px};

#[derive(Clone, Copy)]
pub(super) enum PackageButton {
    Import,
    Export,
    Append,
    Cancel,
    Close,
}

impl PackageButton {
    fn activate(self, s: &mut UiState, window: &mut gpui::Window, cx: &mut Context<UiState>) {
        match self {
            Self::Import => s.choose_package(PackageAction::Import, cx),
            Self::Export => s.choose_package(PackageAction::Export, cx),
            Self::Append => s.choose_package(PackageAction::Append, cx),
            Self::Cancel => s.cancel_package(),
            Self::Close => {
                s.package.export_open = false;
                s.keyboard_focus.focus(window);
            }
        }
        cx.notify();
    }
}

pub(super) fn button(
    id: &'static str,
    label: &'static str,
    action: PackageButton,
    disabled: bool,
    primary: bool,
    cx: &mut Context<UiState>,
) -> Stateful<Div> {
    div()
        .id(id)
        .debug_selector(move || id.into())
        .flex()
        .flex_none()
        .items_center()
        .justify_center()
        .h(px(28.))
        .px_2()
        .rounded(px(3.))
        .border_1()
        .border_color(if primary { theme::ACCENT } else { theme::LINE })
        .bg(if primary {
            theme::ACCENT
        } else {
            theme::PANEL_RAISED
        })
        .text_color(if primary { theme::BG } else { theme::TEXT })
        .text_size(px(11.))
        .when(disabled, |el| el.opacity(0.4))
        .when(!disabled, |el| {
            el.tab_index(0)
                .cursor_pointer()
                .hover(|s| {
                    s.bg(if primary {
                        theme::with_alpha(theme::ACCENT, 0.85)
                    } else {
                        theme::LINE.into()
                    })
                })
                .focus(|s| s.border_color(theme::TEXT))
        })
        .child(label)
        .on_click(cx.listener(move |s, _, window, cx| {
            cx.stop_propagation();
            if !disabled {
                action.activate(s, window, cx);
            }
        }))
}

pub(super) fn close_button(cx: &mut Context<UiState>) -> Stateful<Div> {
    button(
        "package-export-close",
        "",
        PackageButton::Close,
        false,
        false,
        cx,
    )
    .w(px(26.))
    .px_0()
    .border_color(gpui::transparent_black())
    .bg(theme::PANEL)
    .child(
        gpui::svg()
            .path("icons/close.svg")
            .size(px(16.))
            .flex_none()
            .text_color(theme::MUTED),
    )
    .tooltip(|_, cx| cx.new(|_| crate::views::TextTip("Close".into())).into())
}

pub(super) fn checkbox(state: CheckState) -> AnyElement {
    div()
        .flex_none()
        .size(px(14.))
        .rounded(px(2.))
        .border_1()
        .border_color(if state == CheckState::Off {
            theme::MUTED
        } else {
            theme::ACCENT
        })
        .bg(if state == CheckState::Off {
            theme::BG
        } else {
            theme::ACCENT
        })
        .flex()
        .items_center()
        .justify_center()
        .when(state != CheckState::Off, |el| {
            el.child(
                gpui::canvas(
                    |_, _, _| {},
                    move |bounds, _, window, _| {
                        let x = bounds.origin.x;
                        let y = bounds.origin.y;
                        let mut path = gpui::PathBuilder::stroke(px(1.5));
                        if state == CheckState::On {
                            path.move_to(gpui::point(x + px(1.), y + px(5.)));
                            path.line_to(gpui::point(x + px(4.), y + px(8.)));
                            path.line_to(gpui::point(x + px(9.), y + px(2.)));
                        } else {
                            path.move_to(gpui::point(x + px(2.), y + px(5.)));
                            path.line_to(gpui::point(x + px(8.), y + px(5.)));
                        }
                        if let Ok(path) = path.build() {
                            window.paint_path(path, theme::BG);
                        }
                    },
                )
                .size(px(10.)),
            )
        })
        .into_any_element()
}

pub(super) fn disclosure(expanded: bool) -> impl IntoElement {
    gpui::canvas(
        |_, _, _| {},
        move |bounds, _, window, _| {
            let x = bounds.origin.x;
            let y = bounds.origin.y;
            let mut path = gpui::PathBuilder::stroke(px(1.25));
            let points = if expanded {
                [(2., 4.), (6., 8.), (10., 4.)]
            } else {
                [(4., 2.), (8., 6.), (4., 10.)]
            };
            path.move_to(gpui::point(x + px(points[0].0), y + px(points[0].1)));
            for (dx, dy) in &points[1..] {
                path.line_to(gpui::point(x + px(*dx), y + px(*dy)));
            }
            if let Ok(path) = path.build() {
                window.paint_path(path, theme::MUTED);
            }
        },
    )
    .size(px(12.))
}

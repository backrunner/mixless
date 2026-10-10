//! Shared anchored selects. Opening is click-driven; pointer movement never dismisses a menu.
use super::*;
use std::{cell::RefCell, collections::BTreeMap, rc::Rc};

#[derive(Clone, Copy)]
pub(super) struct SelectAnchor {
    bounds: Bounds<gpui::Pixels>,
    visible: Bounds<gpui::Pixels>,
}

#[derive(Default)]
pub(super) struct SelectState {
    pub anchors: Rc<RefCell<BTreeMap<Selector, SelectAnchor>>>,
    anchor: Bounds<gpui::Pixels>,
    highlighted: usize,
    scroll: gpui::ScrollHandle,
}

fn row_height(_: Selector) -> f32 {
    30.
}

/// Keep the menu attached to the control, flip above when needed, and constrain
/// both axes to the client area. Long lists scroll instead of covering the trigger.
fn menu_bounds(
    anchor: Bounds<gpui::Pixels>,
    viewport: gpui::Size<gpui::Pixels>,
    preferred_width: f32,
    desired_height: f32,
) -> Bounds<gpui::Pixels> {
    let edge = 12.;
    let gap = 5.;
    let top_limit = TITLEBAR_HEIGHT + 8.;
    let below = (f32::from(viewport.height - anchor.bottom()) - edge - gap).max(0.);
    let above = (f32::from(anchor.top()) - top_limit - gap).max(0.);
    let down = below >= desired_height || below >= above;
    let height = desired_height.min(if down { below } else { above });
    let width = preferred_width.min(f32::from(viewport.width) - 2. * edge);
    let left = f32::from(anchor.left()).clamp(edge, f32::from(viewport.width) - edge - width);
    let top = if down {
        f32::from(anchor.bottom()) + gap
    } else {
        f32::from(anchor.top()) - gap - height
    };
    Bounds::new(gpui::point(px(left), px(top)), size(px(width), px(height)))
}

impl Preferences {
    pub(super) fn select_control(
        &self,
        selector: Selector,
        value: String,
        enabled: bool,
        cx: &Context<Self>,
    ) -> gpui::Stateful<gpui::Div> {
        let anchors = self.select_ui.anchors.clone();
        let open = self.selector == Some(selector);
        let tip = value.clone();
        div()
            .id(SharedString::from(format!("select-{}", selector as u8)))
            .debug_selector(move || format!("select-{}", selector as u8))
            .relative()
            .flex()
            .items_center()
            .flex_none()
            .gap_2()
            .w(px(280.))
            .h(px(widgets::CONTROL_HEIGHT))
            .pl_3()
            .pr_2()
            .rounded(px(widgets::CONTROL_RADIUS))
            .border_1()
            .border_color(theme::with_alpha(
                theme::TEXT,
                if open {
                    0.32
                } else if enabled {
                    0.14
                } else {
                    0.06
                },
            ))
            .bg(if selector == Selector::PresetActions {
                theme::PANEL_RAISED
            } else {
                theme::PANEL
            })
            .text_size(px(12.))
            .text_color(if enabled { theme::TEXT } else { theme::MUTED })
            .when(enabled, |el| {
                el.cursor_pointer().hover(|s| {
                    s.bg(theme::PANEL_RAISED)
                        .border_color(theme::with_alpha(theme::TEXT, 0.24))
                })
            })
            .child(
                crate::views::ellipsized_text(value)
                    .flex_1()
                    .min_w_0()
                    .w_0()
                    .h(px(18.)),
            )
            .child(widgets::select_indicator(
                selector != Selector::PresetActions,
                enabled,
            ))
            .child(
                gpui::canvas(
                    move |bounds, window, _| {
                        if enabled {
                            let bounds = Bounds::new(
                                bounds.origin - gpui::point(px(1.), px(1.)),
                                bounds.size + size(px(2.), px(2.)),
                            );
                            anchors.borrow_mut().insert(
                                selector,
                                SelectAnchor {
                                    bounds,
                                    visible: bounds.intersect(&window.content_mask().bounds),
                                },
                            );
                        }
                    },
                    |_, _, _, _| {},
                )
                .absolute()
                .inset_0(),
            )
            .when(!open, |el| {
                el.tooltip(move |_, cx| cx.new(|_| crate::views::TextTip(tip.clone())).into())
            })
            .on_click(cx.listener(move |s, _, window, cx| {
                if enabled {
                    s.toggle_selector(selector, window);
                    cx.notify();
                }
            }))
    }

    pub(super) fn select(
        &self,
        selector: Selector,
        value: String,
        cx: &Context<Self>,
    ) -> gpui::AnyElement {
        let enabled = self.job.is_none()
            && (selector != Selector::CueChannels || self.audio.cue_device.is_some());
        self.select_control(selector, value, enabled, cx)
            .into_any_element()
    }

    fn selected_value(&self, selector: Selector) -> String {
        match selector {
            Selector::Master => self.audio.master_device.clone().unwrap_or_default(),
            Selector::Cue => self.audio.cue_device.clone().unwrap_or_default(),
            Selector::MasterChannels => self.audio.master_channels[0].to_string(),
            Selector::CueChannels => self.audio.cue_channels[0].to_string(),
            Selector::Rate => self
                .audio
                .sample_rate
                .map(|n| n.to_string())
                .unwrap_or_default(),
            Selector::Depth => self
                .audio
                .bit_depth
                .map(|n| n.to_string())
                .unwrap_or_default(),
            Selector::Buffer => self
                .audio
                .buffer_frames
                .map(|n| n.to_string())
                .unwrap_or_default(),
            Selector::Source => self.binding.device_id.clone().unwrap_or_default(),
            Selector::MidiChannel => (self.binding.channel + 1).to_string(),
            Selector::MidiNumber => self.binding.number.to_string(),
            Selector::MidiMode => MidiControlMode::ALL
                .iter()
                .position(|m| *m == self.binding.mode)
                .unwrap_or(0)
                .to_string(),
            Selector::MidiPreset => self.selected_preset.clone().unwrap_or_default(),
            Selector::ControllerDevice => self.controller_device.clone().unwrap_or_default(),
            Selector::PresetActions => String::new(),
        }
    }

    pub(super) fn toggle_selector(&mut self, selector: Selector, window: &mut Window) {
        if self.selector == Some(selector) {
            self.selector = None;
            return;
        }
        let Some(anchor) = self.select_ui.anchors.borrow().get(&selector).copied() else {
            return;
        };
        self.selector = Some(selector);
        self.select_ui.anchor = anchor.bounds;
        self.select_ui.highlighted = self
            .selector_options(selector)
            .iter()
            .position(|(value, _)| *value == self.selected_value(selector))
            .unwrap_or(0);
        // A fresh handle prevents a previous device's list offset leaking into this one.
        self.select_ui.scroll = gpui::ScrollHandle::new();
        self.select_ui.scroll.set_offset(gpui::point(
            px(0.),
            px(-(self.select_ui.highlighted as f32) * row_height(selector)),
        ));
        self.select_ui
            .scroll
            .scroll_to_item(self.select_ui.highlighted);
        self.focus.focus(window);
    }

    pub(super) fn select_key(&mut self, key: &gpui::Keystroke, cx: &mut Context<Self>) -> bool {
        let Some(selector) = self.selector else {
            return false;
        };
        let options = self.selector_options(selector);
        let count = options.len();
        match key.key.as_str() {
            "escape" | "tab" => self.selector = None,
            "enter" | "space" => {
                if let Some((value, _)) = options.get(self.select_ui.highlighted) {
                    self.choose(selector, value, cx);
                }
            }
            "down" => {
                self.select_ui.highlighted =
                    (self.select_ui.highlighted + 1).min(count.saturating_sub(1))
            }
            "up" => self.select_ui.highlighted = self.select_ui.highlighted.saturating_sub(1),
            "home" => self.select_ui.highlighted = 0,
            "end" => self.select_ui.highlighted = count.saturating_sub(1),
            _ => return false,
        }
        self.select_ui
            .scroll
            .scroll_to_item(self.select_ui.highlighted);
        true
    }

    fn option_detail(&self, selector: Selector, value: &str) -> Option<String> {
        match selector {
            Selector::MidiPreset => {
                let hub = self.hub().ok()?;
                let preset = hub.preset(value)?;
                (hub.presets()
                    .iter()
                    .filter(|p| p.name == preset.name)
                    .count()
                    > 1)
                .then(|| {
                    if preset.is_builtin() {
                        "Built-in"
                    } else {
                        "User"
                    }
                    .into()
                })
            }
            Selector::ControllerDevice => self
                .controller_devices
                .iter()
                .find(|d| d.id == value)
                .map(|d| if d.hid { "HID" } else { "MIDI" }.into()),
            _ => None,
        }
    }

    fn option_tooltip(&self, selector: Selector, value: &str, label: &str) -> String {
        if selector == Selector::MidiPreset {
            if let Some(p) = self.hub().ok().and_then(|h| h.preset(value)) {
                return format!(
                    "{}\n{}\n{}",
                    p.name,
                    if p.is_builtin() {
                        "Built-in"
                    } else {
                        "User preset"
                    },
                    match p.verification {
                        mixless_controllers::Verification::Protocol => "Not hardware-tested",
                        mixless_controllers::Verification::Simulation =>
                            "Simulation tested; not hardware-tested",
                        mixless_controllers::Verification::Hardware => "Hardware tested",
                    }
                );
            }
        }
        if selector == Selector::ControllerDevice {
            if let Some(d) = self.controller_devices.iter().find(|d| d.id == value) {
                return format!(
                    "{}\n{}\n{}",
                    d.name,
                    controllers::device_status(&d.state),
                    d.id
                );
            }
        }
        label.into()
    }

    pub(super) fn select_menu(
        &self,
        selector: Selector,
        window: &Window,
        cx: &Context<Self>,
    ) -> gpui::AnyElement {
        let options = self.selector_options(selector);
        let selected = self.selected_value(selector);
        let row_height = row_height(selector);
        let width = f32::from(self.select_ui.anchor.size.width).max(match selector {
            Selector::ControllerDevice => 320.,
            Selector::MidiPreset => 280.,
            Selector::PresetActions => 188.,
            Selector::MidiNumber | Selector::MidiChannel => 96.,
            _ => 180.,
        });
        let bounds = menu_bounds(
            self.select_ui.anchor,
            window.viewport_size(),
            width,
            (options.len().max(1) as f32 * row_height + 10.).min(330.),
        );
        let menu =
            div()
                .id("preference-options")
                .debug_selector(|| "preference-options".into())
                .absolute()
                .left(bounds.left())
                .top(bounds.top())
                .w(bounds.size.width)
                .h(bounds.size.height)
                .occlude()
                .p(px(4.))
                .rounded(px(6.))
                .bg(theme::PANEL_RAISED)
                .border_1()
                .border_color(theme::with_alpha(theme::TEXT, 0.16))
                .shadow_md()
                .on_mouse_down(gpui::MouseButton::Left, |_, _, cx| cx.stop_propagation())
                .on_mouse_down(gpui::MouseButton::Right, |_, _, cx| cx.stop_propagation())
                .on_scroll_wheel(|_, _, cx| cx.stop_propagation())
                .child(
                    div()
                        .id("preference-option-list")
                        .size_full()
                        .overflow_y_scroll()
                        .track_scroll(&self.select_ui.scroll)
                        .when(options.is_empty(), |el| {
                            el.child(div().p_2().text_color(theme::MUTED).child("No options"))
                        })
                        .children(options.into_iter().enumerate().map(
                            |(index, (value, label))| {
                                let checked =
                                    selector != Selector::PresetActions && value == selected;
                                let tip = self.option_tooltip(selector, &value, &label);
                                let detail = self.option_detail(selector, &value);
                                let title = label;
                                div()
                                    .id(SharedString::from(format!("option-{index}")))
                                    .debug_selector(move || format!("option-{index}"))
                                    .flex()
                                    .items_center()
                                    .gap_2()
                                    .w_full()
                                    .h(px(row_height))
                                    .px_2()
                                    .rounded(px(3.))
                                    .text_size(px(12.))
                                    .cursor_pointer()
                                    .bg(if index == self.select_ui.highlighted {
                                        theme::LINE
                                    } else {
                                        gpui::transparent_black().into()
                                    })
                                    .when(selector != Selector::PresetActions, |el| {
                                        el.child(
                                            div()
                                                .w(px(14.))
                                                .flex_none()
                                                .text_color(theme::TEXT)
                                                .child(if checked { "✓" } else { "" }),
                                        )
                                    })
                                    .child(
                                        crate::views::ellipsized_text(title)
                                            .flex_1()
                                            .min_w_0()
                                            .w_0()
                                            .h(px(18.)),
                                    )
                                    .when_some(detail, |el, detail| {
                                        el.child(
                                            div()
                                                .flex_none()
                                                .text_size(px(10.))
                                                .text_color(theme::MUTED)
                                                .child(detail),
                                        )
                                    })
                                    .tooltip(move |_, cx| {
                                        cx.new(|_| crate::views::TextTip(tip.clone())).into()
                                    })
                                    .on_mouse_move(cx.listener(move |s, _, _, cx| {
                                        if s.select_ui.highlighted != index {
                                            s.select_ui.highlighted = index;
                                            cx.notify();
                                        }
                                    }))
                                    .on_click(cx.listener(move |s, _, _, cx| {
                                        s.choose(selector, &value, cx);
                                        cx.stop_propagation();
                                        cx.notify();
                                    }))
                            },
                        )),
                );
        div()
            .id("select-backdrop")
            .absolute()
            .inset_0()
            .occlude()
            .on_mouse_down(
                gpui::MouseButton::Left,
                cx.listener(|s, ev: &gpui::MouseDownEvent, window, cx| {
                    let hit = s
                        .select_ui
                        .anchors
                        .borrow()
                        .iter()
                        .find(|(_, anchor)| anchor.visible.contains(&ev.position))
                        .map(|(k, _)| *k);
                    if let Some(next) = hit {
                        s.toggle_selector(next, window);
                    } else {
                        s.selector = None;
                    }
                    cx.stop_propagation();
                    cx.notify();
                }),
            )
            .on_mouse_down(
                gpui::MouseButton::Right,
                cx.listener(|s, _, _, cx| {
                    s.selector = None;
                    cx.stop_propagation();
                    cx.notify();
                }),
            )
            .on_scroll_wheel(cx.listener(|s, _, _, cx| {
                s.selector = None;
                cx.stop_propagation();
                cx.notify();
            }))
            .child(menu)
            .into_any_element()
    }
}

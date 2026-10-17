use super::*;

#[test]
fn enter_and_exit_retain_content_and_stop_requesting_frames_at_rest() {
    let now = Instant::now();
    let mut presence = Presence::default();
    assert!(presence.sample(None, now, false).is_none());
    let first = presence.sample(Some("mixer"), now, false).unwrap();
    assert_eq!(first.opacity, 0.);
    assert!(first.animating && !first.closing);
    let middle = presence
        .sample(Some("mixer"), now + ENTER / 2, false)
        .unwrap();
    assert!(middle.opacity > 0. && middle.opacity < 1.);
    let settled = presence.sample(Some("mixer"), now + ENTER, false).unwrap();
    assert_eq!(settled.opacity, 1.);
    assert!(!settled.animating);
    let closing = presence.sample(None, now + ENTER, false).unwrap();
    assert_eq!(closing.value, "mixer");
    assert_eq!(closing.opacity, 1.);
    assert!(closing.animating && closing.closing);
    let middle = presence
        .sample(None, now + ENTER + EXIT / 2, false)
        .unwrap();
    assert!(middle.opacity > 0. && middle.opacity < 1.);
    assert!(presence.sample(None, now + ENTER + EXIT, false).is_none());
    assert!(presence.value.is_none() && presence.started.is_none());
}

#[test]
fn reversing_a_fade_preserves_opacity_and_refreshes_content() {
    let now = Instant::now();
    let mut presence = Presence::default();
    presence.sample(Some("old name"), now, false);
    let time = now + ENTER / 2;
    let before = presence.sample(Some("edited name"), time, false).unwrap();
    let after = presence.sample(None, time, false).unwrap();
    assert_eq!(before.opacity, after.opacity);
    assert_eq!(after.value, "edited name");
    let time = time + EXIT / 2;
    let before = presence.sample(None, time, false).unwrap();
    let after = presence.sample(Some("reopened"), time, false).unwrap();
    assert_eq!(before.opacity, after.opacity);
    assert_eq!(after.value, "reopened");
    assert!(!after.closing);
    let settled = presence
        .sample(Some("reopened"), time + ENTER, false)
        .unwrap();
    assert_eq!(settled.opacity, 1.);
    assert!(!settled.animating);
}

#[test]
fn reduced_motion_settles_both_directions_including_an_active_transition() {
    let now = Instant::now();
    let mut presence = Presence::default();
    let first = presence.sample(Some(()), now, true).unwrap();
    assert_eq!(first.opacity, 1.);
    assert!(!first.animating);
    assert!(presence.sample(None, now, true).is_none());
    presence.sample(Some(()), now, false);
    let frame = presence.sample(Some(()), now + ENTER / 2, true).unwrap();
    assert_eq!(frame.opacity, 1.);
    assert!(!frame.animating);
    presence.sample(None, now + ENTER, false);
    assert!(
        presence
            .sample(None, now + ENTER + EXIT / 2, true)
            .is_none()
    );
}

#[test]
fn dismissing_before_first_visible_frame_leaves_no_invisible_blocker() {
    let now = Instant::now();
    let mut presence = Presence::default();
    presence.sample(Some(()), now, false);
    assert!(presence.sample(None, now, false).is_none());
}

#[test]
fn outgoing_popups_always_stack_below_live_popups() {
    let popup = |closing| Popup {
        element: div().into_any_element(),
        closing,
    };
    let (_, blocked) = stack(vec![popup(true), popup(true)]);
    assert!(blocked);
    assert!(!stack(vec![popup(true), popup(false)]).1);
    assert!(!stack(Vec::new()).1);
}

#[gpui::test]
fn closing_popup_blocks_clicks_until_unmounted(cx: &mut gpui::TestAppContext) {
    use gpui::{Context, Render, px, size};
    struct Demo {
        open: bool,
        presence: Presence<()>,
        inside: usize,
        underneath: usize,
    }
    impl Render for Demo {
        fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
            let popup = self
                .presence
                .sample(
                    self.open.then_some(()),
                    cx.background_executor().now(),
                    false,
                )
                .and_then(|frame| {
                    frame.render("test-popup", window, |(), _| {
                        Some(
                            div()
                                .id("popup-action")
                                .debug_selector(|| "popup-action".into())
                                .absolute()
                                .top_0()
                                .left_0()
                                .size(px(80.))
                                .occlude()
                                .on_click(cx.listener(|s, _, _, _| s.inside += 1))
                                .into_any_element(),
                        )
                    })
                });
            div()
                .relative()
                .size_full()
                .child(
                    div()
                        .id("underneath")
                        .debug_selector(|| "underneath".into())
                        .size_full()
                        .on_click(cx.listener(|s, _, _, _| s.underneath += 1)),
                )
                .children(popup.map(|p| p.element))
        }
    }
    let (view, cx) = cx.add_window_view(|_, _| Demo {
        open: true,
        presence: Presence::default(),
        inside: 0,
        underneath: 0,
    });
    cx.simulate_resize(size(px(300.), px(200.)));
    cx.run_until_parked();
    cx.executor().advance_clock(ENTER);
    view.update(cx, |_, cx| cx.notify());
    cx.run_until_parked();
    let action = cx.debug_bounds("popup-action").unwrap().center();
    cx.simulate_click(action, Default::default());
    view.read_with(cx, |s, _| assert_eq!(s.inside, 1));
    view.update(cx, |s, cx| {
        s.open = false;
        cx.notify();
    });
    cx.run_until_parked();
    cx.simulate_click(action, Default::default());
    cx.simulate_click(gpui::point(px(250.), px(150.)), Default::default());
    view.read_with(cx, |s, _| {
        assert_eq!(s.inside, 1);
        assert_eq!(s.underneath, 0);
    });
    cx.executor().advance_clock(EXIT);
    view.update(cx, |_, cx| cx.notify());
    cx.run_until_parked();
    cx.simulate_click(action, Default::default());
    view.read_with(cx, |s, _| assert_eq!(s.underneath, 1));
}

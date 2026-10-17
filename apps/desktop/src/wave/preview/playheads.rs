//! Live deck markers do not depend on the asynchronous thumbnail cache.
use super::*;

pub(super) fn element(positions: [Option<f32>; 2]) -> impl IntoElement {
    div()
        .absolute()
        .inset_0()
        .mx(px(2.))
        .children(
            positions
                .into_iter()
                .enumerate()
                .filter_map(|(i, position)| {
                    let position = position.filter(|p| p.is_finite())?.clamp(0., 1.);
                    let color = [theme::DECK_A, theme::DECK_B][i];
                    Some(
                        div()
                            .id(if i == 0 {
                                "library-playhead-a"
                            } else {
                                "library-playhead-b"
                            })
                            .debug_selector(move || {
                                if i == 0 {
                                    "library-playhead-a"
                                } else {
                                    "library-playhead-b"
                                }
                                .into()
                            })
                            .absolute()
                            .left(gpui::relative(position))
                            .top_0()
                            .h_full()
                            .w(px(2.))
                            .ml(px(-1.))
                            .bg(color)
                            .border_color(gpui::rgb(0x08090c))
                            .child(
                                div()
                                    .absolute()
                                    .top(px(i as f32 * 11.))
                                    .when(position <= 0.95, |el| el.left(px(2.)))
                                    .when(position > 0.95, |el| el.right_0())
                                    .w(px(11.))
                                    .h(px(10.))
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .bg(color)
                                    .text_color(gpui::rgb(0x08090c))
                                    .text_size(px(8.))
                                    .font_weight(gpui::FontWeight::BOLD)
                                    .child(if i == 0 { "A" } else { "B" }),
                            ),
                    )
                }),
        )
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Row([Option<f32>; 2]);
    impl gpui::Render for Row {
        fn render(&mut self, _: &mut Window, _: &mut gpui::Context<Self>) -> impl IntoElement {
            div()
                .w(px(400.))
                .h(px(34.))
                .flex()
                .child(super::super::render(
                    |_, _, _| None,
                    Default::default(),
                    self.0,
                ))
        }
    }

    #[gpui::test]
    fn missing_thumbnail_keeps_both_live_markers_and_updates_seeks(cx: &mut gpui::TestAppContext) {
        let (row, cx) = cx.add_window_view(|_, _| Row([Some(0.25), Some(0.75)]));
        cx.run_until_parked();
        let a = cx
            .debug_bounds("library-playhead-a")
            .expect("A with no thumbnail");
        let b = cx
            .debug_bounds("library-playhead-b")
            .expect("B with no thumbnail");
        assert!((f32::from(a.origin.x) - 99.5).abs() < 1.);
        assert!((f32::from(b.origin.x) - 298.5).abs() < 1.);
        row.update(cx, |row, cx| {
            row.0 = [Some(1.), Some(0.)];
            cx.notify();
        });
        cx.run_until_parked();
        assert!(f32::from(cx.debug_bounds("library-playhead-a").unwrap().right()) <= 400.);
        assert!(f32::from(cx.debug_bounds("library-playhead-b").unwrap().left()) >= 0.);
    }
}

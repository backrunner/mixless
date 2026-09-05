//! Mixless — GPUI desktop app. Pure Rust UI, no web stack.

mod automix;
mod controls;
mod root;
mod state;
mod theme;
mod views;
mod wave;

use std::time::Duration;

use gpui::prelude::*;
use gpui::{
    App, Application, Bounds, TitlebarOptions, WindowBackgroundAppearance, WindowBounds,
    WindowOptions, point, px, size,
};

/// 60 Hz UI cadence: one update per 16.67 ms frame budget. GPUI's native
/// animation clock follows 120 Hz ProMotion displays, which doubles the work
/// without improving the 60 fps contract this UI targets.
const UI_FRAME_INTERVAL: Duration = Duration::from_micros(16_667);

fn main() {
    tracing_subscriber::fmt()
        .with_env_filter("mixless=info,mixless_engine=info")
        .init();

    Application::new().run(|cx: &mut App| {
        let core = state::app_core();

        let bounds = Bounds::centered(None, size(px(1440.), px(900.)), cx);
        let options = WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(bounds)),
            titlebar: Some(TitlebarOptions {
                title: Some("Mixless".into()),
                appears_transparent: true,
                traffic_light_position: Some(point(px(16.), px(24.))),
            }),
            window_min_size: Some(size(px(1180.), px(760.))),
            window_background: WindowBackgroundAppearance::Opaque,
            ..Default::default()
        };

        let window = cx
            .open_window(options, |_window, cx| {
                cx.new(|cx| state::UiState::new(core, cx))
            })
            .expect("open window");

        window
            .update(cx, |state, window, cx| {
                cx.observe_window_activation(window, |state, window, cx| {
                    if !window.is_window_active() {
                        state.end_drag();
                        state.end_momentary_fx();
                        cx.notify();
                    }
                })
                .detach();
                state.refresh_tracks();
                state.refresh_playlists();
                state.poll();
                cx.notify();
                cx.spawn(async move |this, cx| {
                    loop {
                        // Poll engine/MIDI state at the target cadence, but
                        // repaint idle windows only when visible state changed.
                        cx.background_executor().timer(UI_FRAME_INTERVAL).await;
                        if this
                            .update(cx, |state, cx| {
                                let changed = state.poll();
                                if changed || state.needs_continuous_repaint() {
                                    cx.notify();
                                }
                            })
                            .is_err()
                        {
                            break;
                        }
                    }
                })
                .detach();
            })
            .expect("init state");

        cx.activate(true);
    });
}

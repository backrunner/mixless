//! Mixless — GPUI desktop app. Pure Rust UI, no web stack.

mod analysis;
mod automix;
mod beat_sync;
mod branding;
mod controls;
mod fader;
mod fx;
mod menus;
mod performance;
mod preferences;
mod root;
#[cfg(target_os = "macos")]
mod self_install;
mod settings;
mod shortcuts;
mod state;
mod storage;
mod theme;
mod views;
mod wave;

use std::time::Duration;

use gpui::prelude::*;
use gpui::{
    App, Application, Bounds, TitlebarOptions, WindowBackgroundAppearance, WindowBounds,
    WindowOptions, point, px, size,
};

/// Idle worker/MIDI wakeup; active rendering follows the display clock.
const UI_FRAME_INTERVAL: Duration = Duration::from_millis(50);

fn main() {
    tracing_subscriber::fmt()
        .with_env_filter("mixless=info,mixless_engine=info")
        .init();
    let build_id = format!(
        "{} · {} · {}",
        env!("CARGO_PKG_VERSION"),
        env!("MIXLESS_REVISION"),
        env!("MIXLESS_BUILD_TIME")
    );
    tracing::info!(build = %build_id, profile = env!("MIXLESS_BUILD_PROFILE"), "starting Mixless");

    #[cfg(target_os = "macos")]
    self_install::relocate_from_disk_image();

    Application::new().run(|cx: &mut App| {
        branding::set_dock_icon();
        let core = state::app_core();

        let bounds = Bounds::centered(None, size(px(1440.), px(900.)), cx);
        let options = WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(bounds)),
            titlebar: Some(TitlebarOptions {
                title: Some("Mixless".into()),
                appears_transparent: true,
                traffic_light_position: Some(point(px(16.), px(24.))),
            }),
            // Disable AppKit's implicit titlebar mover at construction time.
            // The explicitly empty header area still calls performWindowDrag.
            is_movable: false,
            window_min_size: Some(size(px(1180.), px(760.))),
            window_background: WindowBackgroundAppearance::Opaque,
            ..Default::default()
        };

        let window = cx
            .open_window(options, |_window, cx| {
                cx.new(|cx| state::UiState::new(core.clone(), cx))
            })
            .expect("open window");

        window
            .update(cx, |state, window, cx| {
                cx.observe_window_activation(window, |state, window, cx| {
                    if !window.is_window_active() {
                        state.end_drag();
                        state.end_momentary_fx();
                        state.cancel_transport_press();
                        cx.notify();
                    }
                })
                .detach();
                branding::configure_main_window();
                state.refresh_playlists();
                state.poll();
                cx.notify();
                cx.spawn(async move |this, cx| {
                    loop {
                        // Active windows poll immediately before painting.
                        cx.background_executor().timer(UI_FRAME_INTERVAL).await;
                        if this
                            .update(cx, |state, cx| {
                                let animating = state.needs_continuous_repaint();
                                let changed = !animating && state.poll();
                                if !animating && (changed || state.needs_continuous_repaint()) {
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

        menus::init(core, window, cx);
        cx.activate(true);
    });
}

//! Native macOS application and help menus.

use gpui::{App, KeyBinding, Menu, MenuItem, SystemMenuType, WindowHandle, actions};

use crate::state::UiState;

actions!(
    mixless,
    [
        About,
        Preferences,
        KeyboardShortcuts,
        Hide,
        HideOthers,
        ShowAll,
        Quit
    ]
);

pub fn init(main_window: WindowHandle<UiState>, cx: &mut App) {
    cx.on_action(move |_: &Preferences, cx| {
        let core = main_window.update(cx, |state, _, _| {
            state.end_drag();
            state.end_momentary_fx();
            state.core.clone()
        });
        if let Ok(core) = core {
            crate::preferences::open(core, cx);
        }
    });
    cx.on_action(|_: &About, cx| crate::views::about::open(cx));
    cx.on_action(move |_: &KeyboardShortcuts, cx| {
        let _ = main_window.update(cx, |state, window, cx| {
            if state.picker_open {
                return;
            }
            state.end_drag();
            state.end_momentary_fx();
            state.show_import_modal = false;
            state.show_shortcuts = true;
            state.keyboard_focus.focus(window);
            window.activate_window();
            cx.notify();
        });
    });
    cx.on_action(|_: &Hide, cx| cx.hide());
    cx.on_action(|_: &HideOthers, cx| cx.hide_other_apps());
    cx.on_action(|_: &ShowAll, cx| cx.unhide_other_apps());
    cx.on_action(|_: &Quit, cx| cx.quit());
    cx.bind_keys([
        KeyBinding::new("cmd-,", Preferences, None),
        KeyBinding::new("cmd-h", Hide, None),
        KeyBinding::new("cmd-alt-h", HideOthers, None),
        KeyBinding::new("cmd-q", Quit, None),
    ]);
    cx.set_menus(vec![
        Menu {
            name: "Mixless".into(),
            items: vec![
                MenuItem::action("About Mixless", About),
                MenuItem::separator(),
                MenuItem::action("Preferences...", Preferences),
                MenuItem::separator(),
                MenuItem::os_submenu("Services", SystemMenuType::Services),
                MenuItem::separator(),
                MenuItem::action("Hide Mixless", Hide),
                MenuItem::action("Hide Others", HideOthers),
                MenuItem::action("Show All", ShowAll),
                MenuItem::separator(),
                MenuItem::action("Quit Mixless", Quit),
            ],
        },
        Menu {
            name: "Help".into(),
            items: vec![MenuItem::action("Keyboard Shortcuts", KeyboardShortcuts)],
        },
    ]);
}

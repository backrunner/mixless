use crate::state::{CheckState, PackageNode, UiState};
use gpui::{MouseButton, TestAppContext, VisualTestContext, px, size};
use mixless_library::PlaylistSummary;
use std::sync::Arc;

fn playlists() -> Arc<Vec<PlaylistSummary>> {
    Arc::new(vec![
        PlaylistSummary {
            id: 10,
            name: "Local collection".into(),
            tracks: 4,
            folder_path: Some("/Music/Local collection".into()),
            failed_imports: 0,
            spotify_id: None,
        },
        PlaylistSummary {
            id: 20,
            name: "Evening set".into(),
            tracks: 8,
            folder_path: None,
            failed_imports: 0,
            spotify_id: None,
        },
        PlaylistSummary {
            id: 30,
            name: "Night set".into(),
            tracks: 6,
            folder_path: None,
            failed_imports: 0,
            spotify_id: None,
        },
    ])
}

fn click(cx: &mut VisualTestContext, selector: &'static str) {
    let bounds = cx
        .debug_bounds(selector)
        .unwrap_or_else(|| panic!("missing {selector}"));
    cx.simulate_click(bounds.center(), Default::default());
    cx.run_until_parked();
}

fn press(cx: &mut VisualTestContext, keys: &str) {
    // GPUI's focused controls activate on key-up. Include both native events
    // to catch accidental double toggles from custom key-down handlers.
    for key in keys.split(' ') {
        let keystroke = gpui::Keystroke::parse(key).unwrap();
        cx.simulate_event(gpui::KeyDownEvent {
            keystroke: keystroke.clone(),
            is_held: false,
        });
        cx.run_until_parked();
        cx.simulate_event(gpui::KeyUpEvent { keystroke });
        cx.run_until_parked();
    }
}

#[gpui::test]
fn header_opens_separate_export_and_escape_dismisses_it(cx: &mut TestAppContext) {
    let (_temp, core, _) = crate::automix::tests::fixture();
    let (view, cx) = cx.add_window_view(|_, cx| UiState::new(core, cx));
    cx.simulate_resize(size(px(1180.), px(760.)));
    cx.run_until_parked();
    let header = cx.debug_bounds("library-header").unwrap();
    let point = header.center();
    cx.simulate_mouse_down(point, MouseButton::Right, Default::default());
    cx.simulate_mouse_up(point, MouseButton::Right, Default::default());
    cx.run_until_parked();
    assert!(view.read_with(cx, |s, _| s.package.menu.is_some()));
    let menu = cx.debug_bounds("library-menu-panel").unwrap();
    assert!(menu.right() <= px(1172.) && menu.bottom() <= px(752.));
    click(cx, "library-menu-export");
    view.read_with(cx, |s, _| {
        assert!(s.package.export_open && s.package.all);
        assert!(!s.show_import_modal && s.package.menu.is_none());
    });
    assert!(cx.debug_bounds("package-export-panel").is_some());
    press(cx, "tab tab space");
    cx.run_until_parked();
    view.read_with(cx, |s, _| {
        assert!(s.package.export_open, "Tab must move past the close button");
        assert!(!s.package.all, "Space toggles the focused library checkbox");
    });
    cx.simulate_keystrokes("escape");
    cx.run_until_parked();
    assert!(!view.read_with(cx, |s, _| s.package.export_open));
}

#[gpui::test]
fn selection_tree_preserves_siblings_and_supports_group_and_keyboard_controls(
    cx: &mut TestAppContext,
) {
    let (_temp, core, _) = crate::automix::tests::fixture();
    let (view, cx) = cx.add_window_view(|_, cx| UiState::new(core, cx));
    view.update(cx, |s, cx| {
        s.playlists = playlists();
        s.open_package_export(None);
        cx.notify();
    });
    cx.simulate_resize(size(px(1180.), px(760.)));
    cx.run_until_parked();
    click(cx, "package-playlist-20");
    view.read_with(cx, |s, _| {
        assert!(!s.package.all);
        assert_eq!(
            s.package.selected.iter().copied().collect::<Vec<_>>(),
            [10, 30]
        );
        assert_eq!(
            s.package.check_state(PackageNode::Playlists, &s.playlists),
            CheckState::Mixed
        );
        assert_eq!(
            s.package.check_state(PackageNode::Library, &s.playlists),
            CheckState::Mixed
        );
    });
    click(cx, "package-playlists");
    view.read_with(cx, |s, _| {
        assert_eq!(s.package.selected.len(), 3);
        assert_eq!(
            s.package.check_state(PackageNode::Library, &s.playlists),
            CheckState::Mixed
        );
    });
    // Disclosure never changes selection.
    click(cx, "package-playlists-disclosure");
    view.read_with(cx, |s, _| {
        assert!(!s.package.expanded[2]);
        assert_eq!(s.package.selected.len(), 3);
    });
    click(cx, "package-entire-library");
    assert!(view.read_with(cx, |s, _| s.package.all));
    cx.simulate_keystrokes("left");
    cx.run_until_parked();
    assert!(!view.read_with(cx, |s, _| s.package.expanded[0]));
    press(cx, "space");
    cx.run_until_parked();
    view.read_with(cx, |s, _| {
        assert!(!s.package.can_export());
        assert!(!s.snapshot.decks[0].playing && !s.snapshot.decks[1].playing);
    });
    // Empty selection must not open a Save dialog.
    click(cx, "package-export");
    assert!(!view.read_with(cx, |s, _| s.picker_open));
}

#[gpui::test]
fn export_dialog_handles_empty_library_many_rows_errors_and_small_windows(cx: &mut TestAppContext) {
    let (_temp, core, _) = crate::automix::tests::fixture();
    let (view, cx) = cx.add_window_view(|_, cx| UiState::new(core, cx));
    view.update(cx, |s, cx| {
        s.playlists = Arc::new(Vec::new());
        s.open_package_export(None);
        cx.notify();
    });
    cx.simulate_resize(size(px(1180.), px(760.)));
    cx.run_until_parked();
    assert!(view.read_with(cx, |s, _| s.package.can_export()));
    view.update(cx, |s, cx| {
        s.playlists = Arc::new(
            (0..100)
                .map(|id| PlaylistSummary {
                    id,
                    name: format!("Playlist {id}"),
                    tracks: 4,
                    folder_path: None,
                    failed_imports: 0,
                    spotify_id: None,
                })
                .collect(),
        );
        s.package.status = "Cannot write package: destination is read-only".into();
        cx.notify();
    });
    for (width, height) in [(1180., 760.), (1440., 900.), (600., 420.)] {
        cx.simulate_resize(size(px(width), px(height)));
        cx.run_until_parked();
        let panel = cx.debug_bounds("package-export-panel").unwrap();
        let footer = cx.debug_bounds("package-export-footer").unwrap();
        let tree = cx.debug_bounds("package-tree").unwrap();
        assert!(panel.left() >= px(16.) && panel.right() <= px(width - 16.));
        assert!(panel.top() >= px(16.) && panel.bottom() <= px(height - 16.));
        assert!(footer.bottom() < panel.bottom() && footer.top() >= tree.bottom());
    }
}

#[gpui::test]
fn import_contains_only_import_actions(cx: &mut TestAppContext) {
    let (_temp, core, _) = crate::automix::tests::fixture();
    let (view, cx) = cx.add_window_view(|_, cx| UiState::new(core, cx));
    view.update(cx, |s, cx| {
        s.show_import_modal = true;
        cx.notify();
    });
    cx.simulate_resize(size(px(1180.), px(760.)));
    cx.run_until_parked();
    assert!(cx.debug_bounds("package-import").is_some());
    assert!(cx.debug_bounds("package-export").is_none());
    assert!(cx.debug_bounds("package-append").is_none());
    assert!(cx.debug_bounds("package-tree").is_none());
}

#[gpui::test]
fn playlist_export_targets_clicked_playlist(cx: &mut TestAppContext) {
    let (_temp, core, _) = crate::automix::tests::fixture();
    let (view, cx) = cx.add_window_view(|_, cx| UiState::new(core, cx));
    view.update(cx, |s, cx| {
        s.playlists = playlists();
        s.playlist_sel = Some(10);
        s.playlist_menu = Some(crate::views::library::PlaylistMenu {
            id: 30,
            name: "Night set".into(),
            position: gpui::point(px(200.), px(200.)),
        });
        cx.notify();
    });
    cx.simulate_resize(size(px(1180.), px(760.)));
    cx.run_until_parked();
    click(cx, "playlist-menu-export");
    view.read_with(cx, |s, _| {
        assert!(s.package.export_open && !s.show_import_modal && !s.package.all);
        assert_eq!(s.package.selected.iter().copied().collect::<Vec<_>>(), [30]);
    });
}

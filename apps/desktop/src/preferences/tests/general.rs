use super::*;

#[gpui::test]
fn automix_playlist_choice_fits_and_persists(cx: &mut TestAppContext) {
    let (_temp, view, cx) = fixture(cx);
    view.update(cx, |s, cx| {
        s.tab = Tab::General;
        assert!(s.core.settings.get().automix_follow_playlist);
        cx.notify();
    });
    for (width, height) in [(940., 820.), (720., 540.)] {
        cx.simulate_resize(size(px(width), px(height)));
        cx.run_until_parked();
        let content = cx.debug_bounds("settings-content").unwrap();
        let choice = cx.debug_bounds("automix-playlist-switcher").unwrap();
        assert!(choice.left() >= content.left() && choice.right() <= content.right());
        if choice.bottom() > content.bottom() {
            scroll(
                cx,
                content.center(),
                f32::from(content.bottom() - choice.bottom()) - 20.,
            );
        }
        click(cx, "automix-playlist-1");
        view.read_with(cx, |s, _| {
            assert!(!s.core.settings.get().automix_follow_playlist);
            assert!(
                !crate::settings::SettingsStore::load(s.core.settings.path.clone())
                    .get()
                    .automix_follow_playlist
            );
        });
        click(cx, "automix-playlist-0");
        view.read_with(cx, |s, _| {
            assert!(s.core.settings.get().automix_follow_playlist);
            assert!(
                crate::settings::SettingsStore::load(s.core.settings.path.clone())
                    .get()
                    .automix_follow_playlist
            );
        });
    }
}

#[gpui::test]
fn stem_storage_choices_fit_apply_and_persist(cx: &mut TestAppContext) {
    use mixless_stems::StorageFormat;
    let (_temp, view, cx) = fixture(cx);
    view.update(cx, |s, cx| {
        s.tab = Tab::General;
        assert_eq!(s.core.settings.get().stem_storage, StorageFormat::Wav);
        cx.notify();
    });
    for (width, height) in [(940., 820.), (720., 540.)] {
        cx.simulate_resize(size(px(width), px(height)));
        cx.run_until_parked();
        let content = cx.debug_bounds("settings-content").unwrap();
        let choice = cx.debug_bounds("stem-storage-switcher").unwrap();
        assert!(choice.left() >= content.left() && choice.right() <= content.right());
        if choice.bottom() > content.bottom() {
            scroll(
                cx,
                content.center(),
                f32::from(content.bottom() - choice.bottom()) - 80.,
            );
        }
        for (id, format) in [
            ("stem-storage-0", StorageFormat::Wav),
            ("stem-storage-1", StorageFormat::Flac),
            ("stem-storage-2", StorageFormat::Mp3),
        ] {
            click(cx, id);
            view.read_with(cx, |s, _| {
                assert_eq!(s.core.settings.get().stem_storage, format);
                assert_eq!(s.core.stems.as_ref().unwrap().storage_format(), format);
                assert_eq!(
                    crate::settings::SettingsStore::load(s.core.settings.path.clone())
                        .get()
                        .stem_storage,
                    format
                );
            });
        }
    }
    view.update(cx, |s, _| {
        let blocked = s.core.settings.path.with_extension("json.tmp");
        std::fs::create_dir(&blocked).unwrap();
        s.save_general(|p| p.stem_storage = StorageFormat::Wav);
        assert_eq!(
            s.core.stems.as_ref().unwrap().storage_format(),
            StorageFormat::Mp3
        );
        assert_eq!(s.core.settings.get().stem_storage, StorageFormat::Mp3);
        std::fs::remove_dir(blocked).unwrap();
        s.reset_defaults();
        assert_eq!(
            s.core.stems.as_ref().unwrap().storage_format(),
            StorageFormat::Wav
        );
        assert_eq!(
            crate::settings::SettingsStore::load(s.core.settings.path.clone())
                .get()
                .stem_storage,
            StorageFormat::Wav
        );
    });
}

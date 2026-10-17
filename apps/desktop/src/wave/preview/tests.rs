use super::*;

fn wait_for(
    previews: &PreviewStore,
    track: &Track,
    core: &Arc<crate::state::AppCore>,
    columns: usize,
) -> Arc<Preview> {
    let deadline = Instant::now() + Duration::from_secs(2);
    loop {
        previews.poll();
        if let Some(p) = previews.get(track, core, columns) {
            if p.wave.len() == columns {
                return p;
            }
        }
        assert!(Instant::now() < deadline, "preview did not finish");
        std::thread::sleep(Duration::from_millis(5));
    }
}

#[test]
fn enlargement_recovers_detail_and_shrinking_reuses_the_larger_preview() {
    let (_dir, core, tracks) = crate::automix::tests::fixture();
    let track = core.library.get_track(tracks[0]).unwrap();
    let wave = mixless_protocol::Waveform {
        columns: 4096,
        duration_sec: 8.,
        peak: (0..4096)
            .map(|i| if i % 8 == 0 { 255 } else { 0 })
            .collect(),
        peak_pos: (0..4096)
            .map(|i| if i % 8 == 0 { 255 } else { 0 })
            .collect(),
        peak_neg: (0..4096)
            .map(|i| if i % 8 == 0 { 255 } else { 0 })
            .collect(),
        detail_pos: (0..4096)
            .map(|i| if i % 8 == 0 { 65535 } else { 0 })
            .collect(),
        detail_neg: (0..4096)
            .map(|i| if i % 8 == 0 { 65535 } else { 0 })
            .collect(),
        detail_rms: vec![16384; 4096],
        rms: vec![64; 4096],
        low: vec![255; 4096],
        low_mid: vec![0; 4096],
        mid: vec![0; 4096],
        high: vec![0; 4096],
    };
    core.library
        .save_waveform(track.id, &track.content_hash, &wave)
        .unwrap();
    let previews = PreviewStore::default();
    let before = wait_for(&previews, &track, &core, 512);
    assert!(before.wave.iter().all(|c| c.pos == 1.));

    let columns = sampling::resolution(960., 2.);
    let stale = previews.get(&track, &core, columns).unwrap();
    assert!(Arc::ptr_eq(&before, &stale));
    // Rapidly changing widths must not build a queue of obsolete decodes.
    previews.get(&track, &core, sampling::resolution(1800., 2.));
    assert_eq!(previews.0.lock().unwrap().pending.len(), 1);

    let enlarged = wait_for(&previews, &track, &core, columns);
    assert_eq!(enlarged.wave.len(), 2048);
    assert!(enlarged.wave.iter().any(|c| c.pos == 0.));
    assert_eq!(enlarged.wave.iter().filter(|c| c.pos == 1.).count(), 512);
    assert_eq!(enlarged.duration, before.duration);
    assert_eq!(enlarged.sample_rate, before.sample_rate);
    assert_eq!(enlarged.cues.len(), before.cues.len());

    let shrunk = previews.get(&track, &core, 512).unwrap();
    assert!(Arc::ptr_eq(&enlarged, &shrunk));
    assert!(previews.0.lock().unwrap().pending.is_empty());
}

#[test]
fn thumbnail_density_tracks_retina_pixels_and_stays_bounded() {
    assert_eq!(sampling::resolution(500., 1.), 512);
    assert_eq!(sampling::resolution(500., 2.), 1024);
    assert_eq!(sampling::resolution(960., 2.), 2048);
    assert_eq!(sampling::resolution(1800., 2.), 3840);
    assert_eq!(sampling::resolution(1801., 2.), 3840);
    assert_eq!(sampling::resolution(10000., 2.), 8192);
}

#[test]
fn shrinking_does_not_skip_transients_between_pixel_centers() {
    let quiet = cache::Column {
        peak: 0.,
        pos: 0.,
        neg: 0.,
        rms: 0.,
        color: gpui::rgb(0xff0000).into(),
    };
    let mut wave = vec![quiet; 8];
    wave[1].pos = 1.;
    wave[1].neg = 0.75;
    wave[1].peak = 1.;
    assert_eq!(sampling::sample(&wave, 4., 1.).pos, 0.);
    let reduced = sampling::sample(&wave, 4., 8.);
    assert_eq!(reduced.pos, 1.);
    assert_eq!(reduced.neg, 0.75);
}

#[gpui::test]
fn layout_requests_new_density_after_window_resize(cx: &mut gpui::TestAppContext) {
    use gpui::Render;
    struct Row {
        previews: PreviewStore,
        track: Track,
        core: Arc<crate::state::AppCore>,
        visible: bool,
    }
    impl Render for Row {
        fn render(&mut self, _: &mut Window, _: &mut gpui::Context<Self>) -> impl IntoElement {
            div().size_full().flex().when(self.visible, |el| {
                el.child(element(
                    &self.previews,
                    &self.track,
                    &self.core,
                    Default::default(),
                    [Some(0.5), None],
                ))
            })
        }
    }
    let (_dir, core, tracks) = crate::automix::tests::fixture();
    let track = core.library.get_track(tracks[0]).unwrap();
    let previews = PreviewStore::default();
    let (row, cx) = cx.add_window_view(|_, _| Row {
        previews: previews.clone(),
        track: track.clone(),
        core: core.clone(),
        visible: false,
    });
    cx.simulate_resize(gpui::size(px(500.), px(40.)));
    row.update(cx, |row, cx| {
        row.visible = true;
        cx.notify();
    });
    cx.run_until_parked();
    let deadline = Instant::now() + Duration::from_secs(2);
    while !previews.poll() {
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(5));
    }
    let small = previews.0.lock().unwrap().entries[&track.id].0.columns;
    assert!(small >= 500);
    cx.simulate_resize(gpui::size(px(1600.), px(40.)));
    row.update(cx, |_, cx| cx.notify());
    cx.run_until_parked();
    let deadline = Instant::now() + Duration::from_secs(2);
    while !previews.poll() {
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(5));
    }
    let large = previews.0.lock().unwrap().entries[&track.id].0.columns;
    assert!(large >= 1600 && large > small);
    let marker = cx.debug_bounds("library-playhead-a").unwrap();
    assert!((f32::from(marker.center().x) - 800.).abs() < 2.);
}
#[test]
fn overview_positions_follow_both_decks_and_clamp_seeks() {
    let mut decks: [mixless_protocol::DeckSnapshot; 2] = Default::default();
    decks[0].track_id = Some(TrackId(7));
    decks[1].track_id = Some(TrackId(7));
    decks[0].frames = 480000;
    decks[1].frames = 960000;
    assert_eq!(
        positions(&decks, [120000., 720000.]),
        [Some(0.25), Some(0.75)]
    );
    assert_eq!(positions(&decks, [-4., 1920000.]), [Some(0.), Some(1.)]);
    decks[0].track_id = None;
    decks[1].frames = 0;
    assert_eq!(positions(&decks, [120000., 720000.]), [None, None]);
}
#[test]
fn cached_preview_refreshes_manual_cues_without_retaining_full_waveforms() {
    let (_dir, core, tracks) = crate::automix::tests::fixture();
    let id = tracks[0];
    let track = core.library.get_track(id).unwrap();
    let previews = PreviewStore::default();
    let wait = |previous: Option<&Arc<Preview>>| {
        let deadline = Instant::now() + Duration::from_secs(2);
        loop {
            previews.poll();
            if let Some(p) = previews.get(&track, &core, 512) {
                if previous.is_none_or(|old| !Arc::ptr_eq(old, &p)) {
                    return p;
                }
            }
            assert!(Instant::now() < deadline, "preview did not finish");
            std::thread::sleep(Duration::from_millis(5));
        }
    };
    let before = wait(None);
    assert!(before.wave.len() <= 512);
    assert_eq!(before.sample_rate, 48000);
    assert!((before.duration - 8.).abs() < 0.01);
    core.library
        .set_cue(id, 7, 240000, mixless_protocol::CueKind::Out, true)
        .unwrap();
    previews.invalidate(id);
    let after = wait(Some(&before));
    assert!(!Arc::ptr_eq(&before, &after));
    let cue = after.cues.iter().find(|c| c.index == 7).unwrap();
    assert!(cue.user_set);
    assert_eq!(cue.frame, 240000);
    assert!((cue.frame as f32 / after.sample_rate as f32 / after.duration - 0.625).abs() < 0.0001);
}

#[test]
fn invalidated_preview_keeps_the_stale_render_until_refresh_lands() {
    let (_dir, core, tracks) = crate::automix::tests::fixture();
    let id = tracks[0];
    let track = core.library.get_track(id).unwrap();
    let previews = PreviewStore::default();
    let deadline = Instant::now() + Duration::from_secs(2);
    let before = loop {
        previews.poll();
        if let Some(p) = previews.get(&track, &core, 512) {
            break p;
        }
        assert!(Instant::now() < deadline, "preview did not finish");
        std::thread::sleep(Duration::from_millis(5));
    };
    previews.invalidate(id);
    let stale = previews.get(&track, &core, 512).expect("stale preview");
    assert!(Arc::ptr_eq(&before, &stale));
    // A second edit while the first refresh is queued must wake a paused
    // table when that obsolete reply is discarded, so it can request again.
    previews.invalidate(id);
    let deadline = Instant::now() + Duration::from_secs(2);
    while !previews.poll() {
        assert!(
            Instant::now() < deadline,
            "obsolete request did not wake the table"
        );
        std::thread::sleep(Duration::from_millis(5));
    }
    let deadline = Instant::now() + Duration::from_secs(2);
    loop {
        previews.poll();
        let p = previews.get(&track, &core, 512).expect("stale preview");
        if !Arc::ptr_eq(&before, &p) {
            break;
        }
        assert!(Instant::now() < deadline, "refresh did not finish");
        std::thread::sleep(Duration::from_millis(5));
    }
}

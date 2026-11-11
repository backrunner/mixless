use super::*;
use gpui::{AppContext, IntoElement, Modifiers, Render, TestAppContext};
use std::sync::Arc;

fn load(
    core: &Arc<crate::state::AppCore>,
    deck: DeckId,
    id: mixless_protocol::TrackId,
    bass: Option<bool>,
) {
    let source = Arc::new(mixless_engine::AudioBuffer {
        samples: vec![0.1; 512],
        frames: 256,
        sample_rate: 48000,
        loudness: Default::default(),
    });
    let wave = Arc::new(mixless_engine::compute_preview_waveform(&source, 16));
    core.engine
        .load_buffer_if(
            deck,
            id,
            source.clone(),
            wave,
            "Stem control fixture".into(),
            "".into(),
            || true,
        )
        .unwrap();
    if let Some(bass) = bass {
        let stems = if bass {
            mixless_engine::StemBuffer::with_bass(
                48000,
                vec![0.; 512],
                vec![0.; 512],
                vec![0.; 512],
            )
        } else {
            mixless_engine::StemBuffer::new(48000, vec![0.; 512], vec![0.; 512])
        }
        .unwrap();
        assert!(
            core.engine
                .attach_stems(deck, id, &source, Arc::new(stems))
                .unwrap()
        );
    }
}

struct Grid(gpui::Entity<UiState>);
const CONTROLS: [(DeckId, StemKind, &str, &str); 8] = [
    (
        DeckId::A,
        StemKind::Vocals,
        "stem-control-A-Vocals",
        "stem-toggle-A-Vocals",
    ),
    (
        DeckId::A,
        StemKind::Drums,
        "stem-control-A-Drums",
        "stem-toggle-A-Drums",
    ),
    (
        DeckId::A,
        StemKind::Bass,
        "stem-control-A-Bass",
        "stem-toggle-A-Bass",
    ),
    (
        DeckId::A,
        StemKind::Instruments,
        "stem-control-A-Instruments",
        "stem-toggle-A-Instruments",
    ),
    (
        DeckId::B,
        StemKind::Vocals,
        "stem-control-B-Vocals",
        "stem-toggle-B-Vocals",
    ),
    (
        DeckId::B,
        StemKind::Drums,
        "stem-control-B-Drums",
        "stem-toggle-B-Drums",
    ),
    (
        DeckId::B,
        StemKind::Bass,
        "stem-control-B-Bass",
        "stem-toggle-B-Bass",
    ),
    (
        DeckId::B,
        StemKind::Instruments,
        "stem-control-B-Instruments",
        "stem-toggle-B-Instruments",
    ),
];
impl Render for Grid {
    fn render(&mut self, _: &mut gpui::Window, cx: &mut gpui::Context<Self>) -> impl IntoElement {
        self.0.update(cx, |s, cx| {
            gpui::div().flex().gap(px(8.)).children(
                [DeckId::A, DeckId::B]
                    .into_iter()
                    .map(|deck| s.render_stems(cx, deck, s.snapshot.deck(deck))),
            )
        })
    }
}

#[gpui::test]
fn stem_grid_stays_visible_and_blocks_mouse_controls_until_audio_is_ready(cx: &mut TestAppContext) {
    let (_dir, core, ids) = crate::automix::tests::fixture();
    load(&core, DeckId::A, ids[0], None);
    // A retained value must not be reset or muted while its audio is unavailable.
    for deck in [DeckId::A, DeckId::B] {
        for stem in [
            StemKind::Vocals,
            StemKind::Drums,
            StemKind::Bass,
            StemKind::Instruments,
        ] {
            core.engine
                .dispatch(Command::SetStemGain {
                    deck,
                    stem,
                    value: 0.25,
                })
                .unwrap();
        }
    }
    let state = cx.new(|cx| {
        let mut s = UiState::new(core.clone(), cx);
        s.snapshot = core.engine.snapshot();
        s
    });
    let (grid, cx) = cx.add_window_view(|_, _| Grid(state.clone()));
    cx.simulate_resize(gpui::size(px(200.), px(100.)));
    cx.run_until_parked();
    for (deck, stem, control, toggle) in CONTROLS {
        assert!(cx.debug_bounds(control).is_some());
        let label = cx.debug_bounds(toggle).unwrap();
        cx.simulate_click(label.center(), Modifiers::default());
        state.update(cx, |s, _| {
            let ctl = KnobCtl::Stem(deck, stem);
            s.begin_knob(ctl, 100.);
            s.drag_move(0., 0.);
            s.reset_knob(ctl);
            s.end_drag();
        });
        assert_eq!(
            core.engine.snapshot().deck(deck).stem_gain[stem.index()],
            0.25
        );
    }
    // Legacy caches enable the existing three lanes while Bass remains disabled.
    load(&core, DeckId::A, ids[0], Some(false));
    core.engine
        .dispatch(Command::SetStemGain {
            deck: DeckId::A,
            stem: StemKind::Bass,
            value: 0.25,
        })
        .unwrap();
    state.update(cx, |s, _| s.snapshot = core.engine.snapshot());
    grid.update(cx, |_, cx| cx.notify());
    cx.run_until_parked();
    for (stem, selector, expected) in [
        (StemKind::Vocals, "stem-toggle-A-Vocals", 0.),
        (StemKind::Bass, "stem-toggle-A-Bass", 0.25),
    ] {
        let label = cx.debug_bounds(selector).unwrap();
        cx.simulate_click(label.center(), Modifiers::default());
        assert_eq!(
            core.engine.snapshot().decks[0].stem_gain[stem.index()],
            expected
        );
    }
    load(&core, DeckId::A, ids[0], Some(true));
    state.update(cx, |s, _| s.snapshot = core.engine.snapshot());
    grid.update(cx, |_, cx| cx.notify());
    cx.run_until_parked();
    let label = cx.debug_bounds("stem-toggle-A-Bass").unwrap();
    cx.simulate_click(label.center(), Modifiers::default());
    assert_eq!(
        core.engine.snapshot().decks[0].stem_gain[StemKind::Bass.index()],
        0.
    );
}

#[gpui::test]
fn stale_stem_input_and_an_existing_drag_cannot_change_a_replacement_track(
    cx: &mut TestAppContext,
) {
    let (_dir, core, ids) = crate::automix::tests::fixture();
    load(&core, DeckId::A, ids[0], Some(true));
    let state = cx.new(|cx| {
        let mut s = UiState::new(core.clone(), cx);
        s.snapshot = core.engine.snapshot();
        s
    });
    state.update(cx, |s, _| {
        s.begin_knob(KnobCtl::Stem(DeckId::A, StemKind::Vocals), 100.)
    });
    load(&core, DeckId::A, ids[1], None);
    // The old rendered UI still says ready; dispatch must check the engine.
    state.update(cx, |s, _| {
        s.dispatch(Command::SetStemGain {
            deck: DeckId::A,
            stem: StemKind::Vocals,
            value: 0.,
        })
    });
    assert_eq!(core.engine.snapshot().decks[0].stem_gain[0], 1.);
    // A replacement can finish before the next UI poll; track identity still
    // has to cancel the old gesture rather than resuming it on the new source.
    load(&core, DeckId::A, ids[1], Some(true));
    state.update(cx, |s, _| {
        s.dispatch(Command::SetStemGain {
            deck: DeckId::A,
            stem: StemKind::Vocals,
            value: 0.,
        })
    });
    assert_eq!(core.engine.snapshot().decks[0].stem_gain[0], 1.);
    state.update(cx, |s, _| {
        s.poll();
        s.drag_move(0., 200.);
    });
    assert_eq!(core.engine.snapshot().decks[0].stem_gain[0], 1.);
}

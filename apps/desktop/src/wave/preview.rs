//! Bounded asynchronous library thumbnails. No SQL, decoding or full-size
//! waveform allocation in the UI render loop.
use super::*;
use mixless_protocol::{Cue, Track, TrackId};
use std::{
    collections::{HashMap, HashSet, VecDeque},
    sync::mpsc::{self, Receiver, Sender},
    time::{Duration, Instant},
};

pub struct Preview {
    wave: Vec<cache::Column>,
    cues: Vec<Cue>,
    duration: f32,
    sample_rate: u32,
}
type Key = (TrackId, String, bool, u64);
type Reply = (Key, Option<Arc<Preview>>);
#[derive(Default)]
struct Inner {
    entries: HashMap<TrackId, (Key, Arc<Preview>)>,
    order: VecDeque<TrackId>,
    pending: HashSet<Key>,
    retry: HashMap<TrackId, Instant>,
    revisions: HashMap<TrackId, u64>,
    tx: Option<Sender<Key>>,
    rx: Option<Receiver<Reply>>,
}
#[derive(Clone, Default)]
pub struct PreviewStore(Arc<Mutex<Inner>>);
impl PreviewStore {
    pub fn get(&self, track: &Track, core: &Arc<crate::state::AppCore>) -> Option<Arc<Preview>> {
        let mut state = self.0.lock().unwrap();
        let key = (
            track.id,
            track.content_hash.clone(),
            track.analyzed,
            *state.revisions.get(&track.id).unwrap_or(&0),
        );
        // Stale-while-revalidate: a key change queues a refresh but keeps the
        // last preview on screen instead of blanking the row.
        let stale = match state.entries.get(&track.id) {
            Some((old, p)) if *old == key => return Some(p.clone()),
            Some((_, p)) => Some(p.clone()),
            None => None,
        };
        if state
            .retry
            .get(&track.id)
            .is_some_and(|t| t.elapsed() < Duration::from_millis(750))
            || !state.pending.insert(key.clone())
        {
            return stale;
        }
        if state.tx.is_none() {
            let (tx, requests) = mpsc::channel::<Key>();
            let (replies, rx) = mpsc::channel();
            let core = Arc::downgrade(core);
            std::thread::spawn(move || {
                while let Ok(key) = requests.recv() {
                    let Some(core) = core.upgrade() else {
                        break;
                    };
                    let result = (|| {
                        let track = core.library.get_track(key.0).ok()?;
                        if track.content_hash != key.1 {
                            return None;
                        }
                        let wave = core.library.load_waveform(key.0).ok()??;
                        let sample_rate = core.library.cue_sample_rate(key.0).ok()?.unwrap_or(0);
                        let cues = if sample_rate > 0 {
                            core.library.cues(key.0).ok()?
                        } else {
                            vec![]
                        };
                        Some(Arc::new(Preview {
                            wave: cache::overview(&wave, 512),
                            duration: wave.duration_sec,
                            sample_rate,
                            cues,
                        }))
                    })();
                    if replies.send((key, result)).is_err() {
                        break;
                    }
                }
            });
            state.tx = Some(tx);
            state.rx = Some(rx);
        }
        let _ = state.tx.as_ref().unwrap().send(key);
        stale
    }
    pub fn poll(&self) -> bool {
        let mut state = self.0.lock().unwrap();
        let mut changed = false;
        while let Some(Ok((key, value))) = state.rx.as_ref().map(|rx| rx.try_recv()) {
            state.pending.remove(&key);
            if state.revisions.get(&key.0).copied().unwrap_or(0) != key.3 {
                continue;
            }
            if let Some(value) = value {
                state.order.retain(|id| *id != key.0);
                state.order.push_back(key.0);
                state.entries.insert(key.0, (key, value));
                changed = true;
                while state.order.len() > 256 {
                    let id = state.order.pop_front().unwrap();
                    state.entries.remove(&id);
                }
            } else {
                state.retry.insert(key.0, Instant::now());
            }
        }
        changed
    }
    /// The next `get` revalidates, but the current preview keeps rendering
    /// until the worker replaces it; only LRU eviction drops an entry.
    pub fn invalidate(&self, id: TrackId) {
        let mut s = self.0.lock().unwrap();
        *s.revisions.entry(id).or_default() += 1;
        s.retry.remove(&id);
    }
}

pub fn positions(
    decks: &[mixless_protocol::DeckSnapshot; 2],
    frames: [f64; 2],
) -> [Option<f32>; 2] {
    std::array::from_fn(|i| {
        let d = &decks[i];
        (d.track_id.is_some() && d.frames > 0 && frames[i].is_finite())
            .then(|| (frames[i] / d.frames as f64).clamp(0., 1.) as f32)
    })
}

pub fn element(
    preview: Option<Arc<Preview>>,
    overlay: super::mix_overlay::Overlay,
    positions: [Option<f32>; 2],
) -> impl IntoElement {
    div()
        .flex_1()
        .min_w(px(280.))
        .h(px(34.))
        .bg(theme::PANEL_INSET)
        .overflow_hidden()
        .child(
            canvas(
                |_, _, _| {},
                move |bounds, _, window, _| {
                    let Some(p) = &preview else {
                        return;
                    };
                    if p.wave.is_empty() {
                        return;
                    }
                    let width = pf(bounds.size.width);
                    let frames = p.wave.len() as f64;
                    raster::paint_sampled(
                        window,
                        bounds,
                        false,
                        frames,
                        0.,
                        frames,
                        frames / width as f64,
                        0.,
                        |x, _| {
                            let x = (x - 0.5).clamp(0., frames - 1.);
                            let i = x.floor() as usize;
                            p.wave[i]
                                .blend(p.wave[(i + 1).min(p.wave.len() - 1)], (x - i as f64) as f32)
                        },
                    );
                    markers::flags(
                        window,
                        bounds,
                        false,
                        p.cues.iter().map(|c| {
                            (
                                c.frame as f32
                                    / p.sample_rate.max(1) as f32
                                    / p.duration.max(0.001)
                                    * width,
                                c.index as usize,
                            )
                        }),
                    );
                    super::mix_overlay::paint(overlay, p.duration, bounds, window);
                    // A and B use the deck colors and separate flag tiers, so
                    // loading the same track twice preserves both positions.
                    let letters = [[14u8, 17, 17, 31, 17, 17, 17], [30, 17, 17, 30, 17, 17, 30]];
                    for (i, position) in positions.iter().enumerate() {
                        let Some(position) = position else { continue };
                        let x = pf(bounds.origin.x)
                            + (position * width).clamp(1., (width - 2.).max(1.));
                        let y = pf(bounds.origin.y);
                        let color = [theme::DECK_A, theme::DECK_B][i];
                        quad_fill(
                            window,
                            x - 1.,
                            y,
                            4.,
                            pf(bounds.size.height),
                            gpui::rgb(0x08090c),
                        );
                        quad_fill(window, x, y, 2., pf(bounds.size.height), color);
                        let bx = (x + 2.).min(pf(bounds.origin.x) + width - 12.);
                        let by = y + i as f32 * 11.;
                        quad_fill(window, bx, by, 11., 10., color);
                        for (row, bits) in letters[i].iter().enumerate() {
                            for col in 0..5 {
                                if bits & (1 << (4 - col)) != 0 {
                                    quad_fill(
                                        window,
                                        bx + 3. + col as f32,
                                        by + 1. + row as f32,
                                        1.,
                                        1.,
                                        gpui::rgb(0x08090c),
                                    );
                                }
                            }
                        }
                    }
                },
            )
            .size_full(),
        )
}

#[cfg(test)]
mod tests {
    use super::*;
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
                if let Some(p) = previews.get(&track, &core) {
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
        assert!(
            (cue.frame as f32 / after.sample_rate as f32 / after.duration - 0.625).abs() < 0.0001
        );
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
            if let Some(p) = previews.get(&track, &core) {
                break p;
            }
            assert!(Instant::now() < deadline, "preview did not finish");
            std::thread::sleep(Duration::from_millis(5));
        };
        previews.invalidate(id);
        let stale = previews.get(&track, &core).expect("stale preview");
        assert!(Arc::ptr_eq(&before, &stale));
        let deadline = Instant::now() + Duration::from_secs(2);
        loop {
            previews.poll();
            let p = previews.get(&track, &core).expect("stale preview");
            if !Arc::ptr_eq(&before, &p) {
                break;
            }
            assert!(Instant::now() < deadline, "refresh did not finish");
            std::thread::sleep(Duration::from_millis(5));
        }
    }
}

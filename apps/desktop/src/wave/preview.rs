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
        if let Some((old, p)) = state.entries.get(&track.id) {
            if *old == key {
                return Some(p.clone());
            }
        }
        if state
            .retry
            .get(&track.id)
            .is_some_and(|t| t.elapsed() < Duration::from_millis(750))
            || !state.pending.insert(key.clone())
        {
            return None;
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
        None
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
    pub fn invalidate(&self, id: TrackId) {
        let mut s = self.0.lock().unwrap();
        *s.revisions.entry(id).or_default() += 1;
        s.entries.remove(&id);
        s.retry.remove(&id);
    }
}

pub fn element(preview: Option<Arc<Preview>>) -> impl IntoElement {
    div()
        .w(px(220.))
        .h(px(34.))
        .flex_none()
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
                },
            )
            .size_full(),
        )
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn cached_preview_refreshes_manual_cues_without_retaining_full_waveforms() {
        let (_dir, core, tracks) = crate::automix::tests::fixture();
        let id = tracks[0];
        let track = core.library.get_track(id).unwrap();
        let previews = PreviewStore::default();
        let wait = || {
            let deadline = Instant::now() + Duration::from_secs(2);
            loop {
                previews.poll();
                if let Some(p) = previews.get(&track, &core) {
                    return p;
                }
                assert!(Instant::now() < deadline, "preview did not finish");
                std::thread::sleep(Duration::from_millis(5));
            }
        };
        let before = wait();
        assert!(before.wave.len() <= 512);
        assert_eq!(before.sample_rate, 48000);
        assert!((before.duration - 8.).abs() < 0.01);
        core.library
            .set_cue(id, 7, 240000, mixless_protocol::CueKind::Out, true)
            .unwrap();
        previews.invalidate(id);
        let after = wait();
        assert!(!Arc::ptr_eq(&before, &after));
        let cue = after.cues.iter().find(|c| c.index == 7).unwrap();
        assert!(cue.user_set);
        assert_eq!(cue.frame, 240000);
        assert!(
            (cue.frame as f32 / after.sample_rate as f32 / after.duration - 0.625).abs() < 0.0001
        );
    }
}

//! Bounded asynchronous library thumbnails. No SQL, decoding or full-size
//! waveform allocation in the UI render loop.
use super::*;
mod playheads;
mod sampling;
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
#[derive(Clone)]
struct Key {
    track: TrackId,
    hash: String,
    analyzed: bool,
    revision: u64,
    columns: usize,
}
impl Key {
    fn covers(&self, other: &Self) -> bool {
        self.track == other.track
            && self.hash == other.hash
            && self.analyzed == other.analyzed
            && self.revision == other.revision
            && self.columns >= other.columns
    }
}
type Reply = (Key, Option<Arc<Preview>>);
#[derive(Default)]
struct Inner {
    entries: HashMap<TrackId, (Key, Arc<Preview>)>,
    order: VecDeque<TrackId>,
    pending: HashSet<TrackId>,
    retry: HashMap<TrackId, Instant>,
    revisions: HashMap<TrackId, u64>,
    tx: Option<Sender<Key>>,
    rx: Option<Receiver<Reply>>,
}
#[derive(Clone, Default)]
pub struct PreviewStore(Arc<Mutex<Inner>>);
impl PreviewStore {
    fn get(
        &self,
        track: &Track,
        core: &Arc<crate::state::AppCore>,
        columns: usize,
    ) -> Option<Arc<Preview>> {
        let mut state = self.0.lock().unwrap();
        let key = Key {
            track: track.id,
            hash: track.content_hash.clone(),
            analyzed: track.analyzed,
            revision: *state.revisions.get(&track.id).unwrap_or(&0),
            columns,
        };
        // Stale-while-revalidate: a key change queues a refresh but keeps the
        // last preview on screen instead of blanking the row.
        let stale = match state.entries.get(&track.id) {
            Some((old, p)) if old.covers(&key) => return Some(p.clone()),
            Some((_, p)) => Some(p.clone()),
            None => None,
        };
        if state
            .retry
            .get(&track.id)
            .is_some_and(|t| t.elapsed() < Duration::from_millis(750))
            // At most one queued request per track. A further resize is
            // picked up by the next paint after poll, without a decode backlog.
            || !state.pending.insert(track.id)
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
                        let track = core.library.get_track(key.track).ok()?;
                        if track.content_hash != key.hash {
                            return None;
                        }
                        let wave = core.library.load_waveform(key.track).ok()??;
                        let sample_rate =
                            core.library.cue_sample_rate(key.track).ok()?.unwrap_or(0);
                        let cues = if sample_rate > 0 {
                            core.library.cues(key.track).ok()?
                        } else {
                            vec![]
                        };
                        Some(Arc::new(Preview {
                            wave: cache::overview(&wave, key.columns),
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
            state.pending.remove(&key.track);
            if state.revisions.get(&key.track).copied().unwrap_or(0) != key.revision {
                // Clearing the old request must wake a cached, paused table
                // so it can enqueue the current revision.
                changed = true;
                continue;
            }
            if let Some(value) = value {
                state.order.retain(|id| *id != key.track);
                state.order.push_back(key.track);
                state.entries.insert(key.track, (key, value));
                changed = true;
                while state.order.len() > 256 {
                    let id = state.order.pop_front().unwrap();
                    state.entries.remove(&id);
                }
            } else {
                state.retry.insert(key.track, Instant::now());
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
    previews: &PreviewStore,
    track: &Track,
    core: &Arc<crate::state::AppCore>,
    overlay: super::mix_overlay::Overlay,
    positions: [Option<f32>; 2],
) -> impl IntoElement {
    let previews = previews.clone();
    let track = track.clone();
    let core = core.clone();
    render(
        move |bounds, window, _| {
            // Layout is resolved here, so flex columns and Retina scale
            // contribute their actual physical width, even after resize.
            let columns = sampling::resolution(pf(bounds.size.width), window.scale_factor());
            previews.get(&track, &core, columns)
        },
        overlay,
        positions,
    )
}

fn render(
    resolve: impl FnOnce(Bounds<Pixels>, &mut Window, &mut gpui::App) -> Option<Arc<Preview>> + 'static,
    overlay: super::mix_overlay::Overlay,
    positions: [Option<f32>; 2],
) -> impl IntoElement {
    div()
        .relative()
        .flex_1()
        .min_w(px(280.))
        .h(px(34.))
        .bg(theme::PANEL_INSET)
        .overflow_hidden()
        .child(
            canvas(resolve, move |bounds, preview, window, _| {
                let Some(p) = preview else {
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
                    |x, cpp| sampling::sample(&p.wave, x, cpp),
                );
                markers::flags(
                    window,
                    bounds,
                    false,
                    p.cues.iter().map(|c| {
                        (
                            c.frame as f32 / p.sample_rate.max(1) as f32 / p.duration.max(0.001)
                                * width,
                            c.index as usize,
                        )
                    }),
                );
                super::mix_overlay::paint(overlay, p.duration, bounds, window);
            })
            .size_full(),
        )
        // Playback position is available even while the thumbnail is loading
        // or absent. Keep it above waveform, cue and transition layers.
        .child(playheads::element(positions))
}

#[cfg(test)]
mod tests;

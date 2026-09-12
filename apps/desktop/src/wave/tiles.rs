//! Source-anchored waveform textures. Playback moves cached pixels instead of
//! resampling the peak envelope every display frame (which makes it breathe).
use super::*;
use gpui::{ContentMask, RenderImage, point, size};
use std::{
    collections::{HashMap, HashSet, VecDeque},
    sync::mpsc::{Receiver, Sender, channel},
};

const WIDTH: i64 = 512;
const MAX_TILES: usize = 24;

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
struct Key {
    index: i64,
    step: u64,
    frames: u64,
    height: u16,
    vertical: bool,
}
type Reply = (Key, Arc<RenderImage>);
#[derive(Default)]
struct Store {
    images: HashMap<Key, Arc<RenderImage>>,
    order: VecDeque<Key>,
    pending: HashSet<Key>,
    tx: Option<Sender<Key>>,
    rx: Option<Receiver<Reply>>,
}
#[derive(Default)]
pub(super) struct TileCache(Mutex<Store>);

impl TileCache {
    pub(super) fn poll(&self) -> bool {
        let mut cache = self.0.lock().unwrap();
        let mut changed = false;
        while let Some(Ok((key, image))) = cache.rx.as_ref().map(|rx| rx.try_recv()) {
            cache.pending.remove(&key);
            cache.order.retain(|k| *k != key);
            cache.order.push_back(key);
            cache.images.insert(key, image);
            while cache.order.len() > MAX_TILES {
                if let Some(old) = cache.order.pop_front() {
                    cache.images.remove(&old);
                }
            }
            changed = true;
        }
        changed
    }
    fn get(&self, data: &Arc<WaveCache>, key: Key) -> Option<Arc<RenderImage>> {
        let mut cache = self.0.lock().unwrap();
        if let Some(image) = cache.images.get(&key).cloned() {
            cache.order.retain(|k| *k != key);
            cache.order.push_back(key);
            return Some(image);
        }
        // Bound stale requests when a user resizes or scrubs rapidly.
        if cache.pending.len() >= 12 || !cache.pending.insert(key) {
            return None;
        }
        if cache.tx.is_none() {
            let (tx, requests) = channel::<Key>();
            let (replies, rx) = channel();
            let data = Arc::downgrade(data);
            std::thread::spawn(move || {
                while let Ok(key) = requests.recv() {
                    let Some(data) = data.upgrade() else { break };
                    let image = render(&data, key);
                    if replies.send((key, Arc::new(image))).is_err() {
                        break;
                    }
                }
            });
            cache.tx = Some(tx);
            cache.rx = Some(rx);
        }
        let _ = cache.tx.as_ref().unwrap().send(key);
        None
    }
}

fn render(data: &WaveCache, key: Key) -> RenderImage {
    let step = f64::from_bits(key.step);
    let frames = f64::from_bits(key.frames);
    let columns_per_pixel = (step * data.columns as f64 / frames) as f32;
    let height = key.height as u32;
    let bg = theme::PANEL_INSET;
    let background = [
        (bg.b * 255.) as u8,
        (bg.g * 255.) as u8,
        (bg.r * 255.) as u8,
        255,
    ];
    let (w, h) = if key.vertical {
        (height, (WIDTH + 2) as u32)
    } else {
        ((WIDTH + 2) as u32, height)
    };
    let mut pixels = image::RgbaImage::from_pixel(w, h, image::Rgba(background));
    let center = height as f32 * 0.5;
    let half = center * 0.93;
    for x in 0..WIDTH + 2 {
        // One-pixel gutters agree exactly with the neighbouring tile. Their
        // opaque background prevents alpha buildup at fractional translations.
        let source = (key.index * WIDTH + x - 1) as f64 + 0.5;
        let frame = source * step;
        if frame < 0. || frame >= frames {
            continue;
        }
        let column = data.sample(frame / frames * data.columns as f64, columns_per_pixel);
        let top = center - column.pos * half;
        let bottom = center + column.neg * half;
        for y in top.floor().max(0.) as u32..bottom.ceil().min(height as f32) as u32 {
            let coverage = ((y as f32 + 1.).min(bottom) - (y as f32).max(top)).clamp(0., 1.);
            let color = [column.color.b, column.color.g, column.color.r];
            let mut p = background;
            for c in 0..3 {
                p[c] = (background[c] as f32 * (1. - coverage) + color[c] * 255. * coverage).round()
                    as u8;
            }
            if key.vertical {
                pixels.put_pixel(y, x as u32, image::Rgba(p));
            } else {
                pixels.put_pixel(x as u32, y, image::Rgba(p));
            }
        }
    }
    RenderImage::new(vec![image::Frame::new(pixels)])
}

pub(super) fn paint(
    window: &mut Window,
    bounds: Bounds<Pixels>,
    vertical: bool,
    data: &Arc<WaveCache>,
    frame: f64,
    frames: f64,
    fpp: f64,
    anchor: f64,
) {
    let scale = window.scale_factor() as f64;
    let (origin, span, thickness) = if vertical {
        (
            pf(bounds.origin.y),
            pf(bounds.size.height),
            pf(bounds.size.width),
        )
    } else {
        (
            pf(bounds.origin.x),
            pf(bounds.size.width),
            pf(bounds.size.height),
        )
    };
    let span = span as f64;
    let step = fpp / scale;
    if !step.is_finite() || step <= 0. || frames <= 0. {
        return;
    }
    let first = (((frame - anchor * fpp) / step / WIDTH as f64).floor() as i64).max(0);
    let last = (((frame + (span - anchor) * fpp) / step / WIDTH as f64).floor() as i64)
        .min((frames / step / WIDTH as f64).floor() as i64);
    let height = (thickness as f64 * scale).ceil().clamp(8., 384.) as u16;
    let mut images = Vec::new();
    let mut missing = false;
    for index in first..=last + 1 {
        let key = Key {
            index,
            step: step.to_bits(),
            frames: frames.to_bits(),
            height,
            vertical,
        };
        if let Some(image) = data.tiles.get(data, key) {
            if index <= last {
                images.push((index, image));
            }
        } else if index <= last {
            missing = true;
        }
    }
    window.with_content_mask(Some(ContentMask { bounds }), |window| {
        if missing {
            raster::paint(window, bounds, vertical, data, frame, frames, fpp, anchor);
        }
        for (index, image) in images {
            let x = origin as f64 + anchor + (((index * WIDTH - 1) as f64 * step) - frame) / fpp;
            let length = px(((WIDTH + 2) as f64 / scale) as f32);
            let image_bounds = if vertical {
                Bounds {
                    origin: point(bounds.origin.x, px(x as f32)),
                    size: size(bounds.size.width, length),
                }
            } else {
                Bounds {
                    origin: point(px(x as f32), bounds.origin.y),
                    size: size(length, bounds.size.height),
                }
            };
            let _ = window.paint_image(image_bounds, px(0.).into(), image, 0, false);
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn texture_gutters_are_identical_and_source_pixels_do_not_depend_on_playhead() {
        let data = WaveCache::new(Arc::new(Waveform {
            columns: 3,
            duration_sec: 1.,
            peak: vec![10, 255, 120],
            peak_pos: vec![],
            peak_neg: vec![],
            rms: vec![4, 80, 40],
            low: vec![255, 0, 0],
            low_mid: vec![0, 0, 0],
            mid: vec![0, 255, 0],
            high: vec![0, 0, 255],
            detail_pos: vec![],
            detail_neg: vec![],
            detail_rms: vec![],
        }));
        let key = Key {
            index: 0,
            step: 40f64.to_bits(),
            frames: 48000f64.to_bits(),
            height: 96,
            vertical: false,
        };
        let a = render(&data, key);
        let b = render(&data, Key { index: 1, ..key });
        let again = render(&data, key);
        assert_eq!(a.as_bytes(0), again.as_bytes(0));
        let stride = (WIDTH + 2) as usize * 4;
        for y in 0..96 {
            assert_eq!(
                &a.as_bytes(0).unwrap()
                    [y * stride + WIDTH as usize * 4..y * stride + (WIDTH as usize + 2) * 4],
                &b.as_bytes(0).unwrap()[y * stride..y * stride + 8]
            );
        }
    }
}

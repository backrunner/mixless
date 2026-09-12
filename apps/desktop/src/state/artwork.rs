//! Deck covers follow the loaded track, independently of the visible playlist.
//! SQLite, image decoding and resizing are confined to background workers.
use std::{
    collections::VecDeque,
    path::Path,
    sync::{
        Arc, Mutex,
        mpsc::{Receiver, channel},
    },
};

use gpui::RenderImage;
use image::{DynamicImage, ImageDecoder, ImageReader, imageops::FilterType};
use mixless_protocol::TrackId;

use super::AppCore;

const COVER_SIZE: u32 = 512;
const CACHED_COVERS: usize = 8;

#[derive(Default)]
pub struct DeckArtwork {
    track: Option<TrackId>,
    pub image: Option<Arc<RenderImage>>,
    rx: Option<Receiver<Option<Arc<RenderImage>>>>,
    refresh: bool,
}

impl DeckArtwork {
    pub(super) fn invalidate(&mut self) {
        self.refresh = true;
    }

    pub(super) fn poll(
        &mut self,
        core: &Arc<AppCore>,
        cache: &Arc<ArtworkCache>,
        track: Option<TrackId>,
        reloaded: bool,
    ) -> bool {
        let changed = self.track != track;
        if changed || reloaded || std::mem::take(&mut self.refresh) {
            self.refresh = false;
            self.track = track;
            // Dropping the old receiver makes a late response unable to replace
            // the new track's artwork. Same-track refreshes keep the old cover.
            self.rx = None;
            if changed || track.is_none() {
                self.image = None;
            }
            if let Some(id) = track {
                let (tx, rx) = channel();
                self.rx = Some(rx);
                let core = core.clone();
                let cache = cache.clone();
                std::thread::spawn(move || {
                    let image = core
                        .library
                        .get_track(id)
                        .ok()
                        .and_then(|track| track.artwork_path)
                        .and_then(|path| cache.load(&path));
                    let _ = tx.send(image);
                });
            }
        }
        if let Some(rx) = self.rx.take() {
            match rx.try_recv() {
                Ok(image) => {
                    self.image = image;
                    return true;
                }
                Err(std::sync::mpsc::TryRecvError::Empty) => self.rx = Some(rx),
                Err(_) => {}
            }
        }
        changed
    }
}

type Cover = (String, Option<Arc<RenderImage>>);

/// Covers are keyed by the library's content-hashed path. Both decks and
/// repeated playlist passes share one texture; failed decodes are cached too.
#[derive(Default)]
pub(super) struct ArtworkCache(Mutex<VecDeque<Cover>>);

impl ArtworkCache {
    fn load(&self, path: &str) -> Option<Arc<RenderImage>> {
        // Only artwork workers take this lock, never the UI or audio callback.
        let mut cache = self.0.lock().ok()?;
        let entry = if let Some(index) = cache.iter().position(|(key, _)| key == path) {
            cache.remove(index)?
        } else {
            (path.to_owned(), decode(Path::new(path)).ok().map(Arc::new))
        };
        let image = entry.1.clone();
        cache.push_back(entry);
        while cache.len() > CACHED_COVERS {
            cache.pop_front();
        }
        image
    }
}

fn decode(path: &Path) -> Result<RenderImage, image::ImageError> {
    let mut reader = ImageReader::open(path)?.with_guessed_format()?;
    let mut limits = image::Limits::default();
    limits.max_alloc = Some(128 * 1024 * 1024);
    reader.limits(limits);
    let mut decoder = reader.into_decoder()?;
    let orientation = decoder.orientation()?;
    let mut decoded = DynamicImage::from_decoder(decoder)?;
    decoded.apply_orientation(orientation);
    let size = decoded.width().min(decoded.height()).min(COVER_SIZE);
    // Crop before downsampling: panoramic artwork must not produce an enormous
    // intermediate image. Do not upscale tiny embedded thumbnails.
    let side = decoded.width().min(decoded.height());
    let mut pixels = decoded
        .crop_imm(
            (decoded.width() - side) / 2,
            (decoded.height() - side) / 2,
            side,
            side,
        )
        .resize_exact(size, size, FilterType::Lanczos3)
        .into_rgba8();
    // GPUI's GPU image format is BGRA, with straight alpha.
    for pixel in pixels.chunks_exact_mut(4) {
        pixel.swap(0, 2);
    }
    Ok(RenderImage::new(vec![image::Frame::new(pixels)]))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn embedded_cover_is_cached_and_survives_reopen() {
        use lofty::{
            config::WriteOptions,
            picture::{MimeType, Picture, PictureType},
            prelude::{Accessor, TagExt},
            tag::{Tag, TagType},
        };
        let (dir, core, tracks) = crate::automix::tests::fixture();
        let path = core.library.get_track(tracks[0]).unwrap().path;
        let mut info = Tag::new(TagType::RiffInfo);
        info.set_title("Cover test".into());
        info.save_to_path(&path, WriteOptions::default()).unwrap();
        let mut bytes = std::io::Cursor::new(Vec::new());
        DynamicImage::ImageRgba8(image::RgbaImage::from_pixel(
            48,
            32,
            image::Rgba([240, 80, 10, 255]),
        ))
        .write_to(&mut bytes, image::ImageFormat::Png)
        .unwrap();
        let mut id3 = Tag::new(TagType::Id3v2);
        id3.push_picture(Picture::new_unchecked(
            PictureType::CoverFront,
            Some(MimeType::Png),
            None,
            bytes.into_inner(),
        ));
        id3.save_to_path(&path, WriteOptions::default()).unwrap();
        let id = core.library.import_file(Path::new(&path)).unwrap();
        let cover = core
            .library
            .get_track(id)
            .unwrap()
            .artwork_path
            .expect("embedded cover in ID3 alongside RIFF metadata");
        let reopened = mixless_library::Library::open(&dir.path().join("library.db")).unwrap();
        assert_eq!(
            reopened.get_track(id).unwrap().artwork_path.as_deref(),
            Some(cover.as_str())
        );
        assert_eq!(decode(Path::new(&cover)).unwrap().size(0).width.0, 32);
    }

    #[test]
    fn covers_are_center_cropped_bounded_and_keep_correct_channels() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("封面.png");
        let pixels = image::RgbaImage::from_fn(1600, 800, |x, _| {
            if (400..1200).contains(&x) {
                image::Rgba([240, 80, 10, 255])
            } else {
                image::Rgba([0, 0, 255, 255])
            }
        });
        pixels.save(&path).unwrap();
        let cover = decode(&path).unwrap();
        assert_eq!(cover.size(0).width.0, COVER_SIZE as i32);
        assert_eq!(cover.size(0).height.0, COVER_SIZE as i32);
        assert!(
            cover
                .as_bytes(0)
                .unwrap()
                .chunks_exact(4)
                .all(|pixel| pixel == [10, 80, 240, 255])
        );
    }

    #[test]
    fn repeat_loads_share_decoded_cover_and_missing_or_bad_images_fall_back() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("small.png");
        image::RgbaImage::from_pixel(24, 36, image::Rgba([1, 2, 3, 255]))
            .save(&path)
            .unwrap();
        let cache = ArtworkCache::default();
        let a = cache.load(path.to_str().unwrap()).unwrap();
        let b = cache.load(path.to_str().unwrap()).unwrap();
        assert!(Arc::ptr_eq(&a, &b));
        assert_eq!(a.size(0).width.0, 24);
        assert!(
            cache
                .load(dir.path().join("missing.jpg").to_str().unwrap())
                .is_none()
        );
        let bad = dir.path().join("broken.jpg");
        std::fs::write(&bad, b"not an image").unwrap();
        assert!(cache.load(bad.to_str().unwrap()).is_none());
    }

    #[test]
    fn changing_track_discards_a_late_artwork_result() {
        let (_dir, core, tracks) = crate::automix::tests::fixture();
        let cache = Arc::new(ArtworkCache::default());
        let (tx, rx) = channel();
        let mut slot = DeckArtwork {
            track: Some(tracks[0]),
            rx: Some(rx),
            ..Default::default()
        };
        slot.poll(&core, &cache, Some(tracks[1]), false);
        assert!(tx.send(None).is_err());
        assert_eq!(slot.track, Some(tracks[1]));
        assert!(slot.image.is_none());
    }
}

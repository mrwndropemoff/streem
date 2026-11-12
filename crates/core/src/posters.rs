//! Poster images: downloaded once, kept on disk as the original bytes, and held in memory as
//! small decoded RGBA pictures.
//!
//! The UI asks through [`crate::Core::poster`], which never waits. The first ask for a URL starts
//! the work in the background; the answer is found in the [`PosterCache`] once it is ready.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use image::imageops::FilterType;
use sha2::{Digest, Sha256};

/// Posters are drawn small, so wider pictures are scaled down to this many pixels.
const MAX_WIDTH: u32 = 300;

/// A decoded poster: `rgba` holds `width * height * 4` bytes, not premultiplied.
#[derive(Debug, PartialEq, Eq)]
pub struct Poster {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

enum Slot {
    Loading,
    Ready(Arc<Poster>),
    /// Not retried during this run, so a broken URL is not requested again every frame.
    Failed,
}

/// The decoded posters of this run, shared between the UI thread and the background task.
// shortcut: in-memory posters are never evicted, add an LRU cap when memory use matters
#[derive(Default)]
pub(crate) struct PosterCache {
    slots: Mutex<HashMap<String, Slot>>,
}

impl PosterCache {
    /// The poster if it is ready. The second value is true when the caller must now start
    /// loading it: it is true for the first ask about a URL only.
    pub(crate) fn lookup(&self, url: &str) -> (Option<Arc<Poster>>, bool) {
        let mut slots = self
            .slots
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        match slots.get(url) {
            Some(Slot::Ready(poster)) => (Some(poster.clone()), false),
            Some(Slot::Loading | Slot::Failed) => (None, false),
            None => {
                slots.insert(url.to_string(), Slot::Loading);
                (None, true)
            }
        }
    }

    pub(crate) fn finish(&self, url: String, result: Result<Poster, String>) {
        let slot = match result {
            Ok(poster) => Slot::Ready(Arc::new(poster)),
            Err(_) => Slot::Failed,
        };
        self.slots
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .insert(url, slot);
    }
}

/// Gets a poster from the disk copy if there is one, otherwise from the network (saving the
/// original bytes first). The error is text for a log line; the UI just keeps its placeholder.
pub(crate) async fn load(
    client: &reqwest::Client,
    data_dir: &Path,
    url: &str,
) -> Result<Poster, String> {
    let file = cache_file(data_dir, url);

    let cached = {
        let file = file.clone();
        tokio::task::spawn_blocking(move || std::fs::read(file).ok())
            .await
            .map_err(|error| error.to_string())?
    };
    // A saved copy that cannot be decoded is ignored and downloaded again.
    if let Some(bytes) = cached
        && let Ok(poster) = decode(bytes).await
    {
        return Ok(poster);
    }

    let response = client
        .get(url)
        .send()
        .await
        .map_err(|error| error.to_string())?;
    let status = response.status();
    if !status.is_success() {
        return Err(format!("HTTP status {}", status.as_u16()));
    }
    let bytes = response
        .bytes()
        .await
        .map_err(|error| error.to_string())?
        .to_vec();

    // Decode first, so a response that is not a picture is never saved.
    let poster = decode(bytes.clone()).await?;
    // A failed save only means the next run downloads it again.
    let _ = tokio::task::spawn_blocking(move || save(&file, &bytes)).await;
    Ok(poster)
}

fn cache_file(data_dir: &Path, url: &str) -> PathBuf {
    let hash = Sha256::digest(url.as_bytes());
    let name: String = hash.iter().map(|byte| format!("{byte:02x}")).collect();
    data_dir.join("posters").join(name)
}

/// Writes through a temporary file so a crash never leaves half a picture under the final name.
fn save(file: &Path, bytes: &[u8]) -> std::io::Result<()> {
    if let Some(folder) = file.parent() {
        std::fs::create_dir_all(folder)?;
    }
    let temporary = file.with_extension("part");
    std::fs::write(&temporary, bytes)?;
    std::fs::rename(&temporary, file)
}

async fn decode(bytes: Vec<u8>) -> Result<Poster, String> {
    tokio::task::spawn_blocking(move || decode_blocking(&bytes))
        .await
        .map_err(|error| error.to_string())?
}

fn decode_blocking(bytes: &[u8]) -> Result<Poster, String> {
    let picture = image::load_from_memory(bytes).map_err(|error| error.to_string())?;
    let picture = if picture.width() > MAX_WIDTH {
        let height = (u64::from(picture.height()) * u64::from(MAX_WIDTH)
            / u64::from(picture.width()))
        .max(1) as u32;
        picture.resize_exact(MAX_WIDTH, height, FilterType::Triangle)
    } else {
        picture
    };
    let rgba = picture.to_rgba8();
    Ok(Poster {
        width: rgba.width(),
        height: rgba.height(),
        rgba: rgba.into_raw(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn png(width: u32, height: u32) -> Vec<u8> {
        let mut bytes = Vec::new();
        image::RgbaImage::from_pixel(width, height, image::Rgba([200, 10, 10, 255]))
            .write_to(
                &mut std::io::Cursor::new(&mut bytes),
                image::ImageFormat::Png,
            )
            .unwrap();
        bytes
    }

    #[test]
    fn wide_pictures_are_scaled_down_to_300_pixels() {
        let poster = decode_blocking(&png(600, 900)).unwrap();
        assert_eq!((poster.width, poster.height), (300, 450));
        assert_eq!(poster.rgba.len(), 300 * 450 * 4);
    }

    #[test]
    fn small_pictures_are_kept_as_they_are() {
        let poster = decode_blocking(&png(100, 150)).unwrap();
        assert_eq!((poster.width, poster.height), (100, 150));
    }

    #[test]
    fn something_that_is_not_a_picture_is_an_error() {
        assert!(decode_blocking(b"<html>not found</html>").is_err());
    }

    #[test]
    fn only_the_first_ask_for_a_url_starts_loading() {
        let cache = PosterCache::default();
        assert_eq!(cache.lookup("http://x/a.png"), (None, true));
        assert_eq!(cache.lookup("http://x/a.png"), (None, false));
        cache.finish("http://x/a.png".into(), Err("broken".into()));
        assert_eq!(cache.lookup("http://x/a.png"), (None, false));
        cache.finish(
            "http://x/b.png".into(),
            Ok(decode_blocking(&png(10, 10)).unwrap()),
        );
        assert!(cache.lookup("http://x/b.png").0.is_some());
    }
}

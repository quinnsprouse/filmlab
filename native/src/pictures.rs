use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::Arc;

use gpui_kit::{App, RenderImage};

use crate::cache;

/// How many bytes of decoded thumbnails stay in memory (and as many again in the GPU's image atlas) while off screen.
const KEPT_BYTES: usize = 64 << 20;

/// A decoded thumbnail: the film render of one look, or the camera's embedded preview until that render is ready.
#[derive(Clone)]
pub struct Picture {
    pub image: Arc<RenderImage>,
    pub film: Option<u64>,
}

struct Kept {
    picture: Picture,
    seen: u64,
}

/// The decoded thumbnails of the photos on screen and of those seen most recently; everything else is read back from
/// the disk cache when it is wanted again.
#[derive(Default)]
pub struct Pictures {
    kept: HashMap<String, Kept>,
    frame: u64,
    wanted: Vec<String>,
    wanted_now: HashSet<String>,
}

/// Where a thumbnail that isn't in memory comes from.
pub enum Source {
    Film { path: PathBuf, look: u64 },
    Preview { path: PathBuf },
}

pub fn image_of(width: u32, height: u32, mut rgba: Vec<u8>) -> Option<Arc<RenderImage>> {
    for px in rgba.as_chunks_mut::<4>().0 {
        px.swap(0, 2);
    }
    let buffer = image::RgbaImage::from_raw(width, height, rgba)?;
    Some(Arc::new(RenderImage::new(vec![image::Frame::new(buffer)])))
}

impl Source {
    pub fn load(self, edge: usize) -> Option<Picture> {
        match self {
            Source::Film { path, look } => {
                let cache::Pixels { width, height, rgba } = cache::thumbnail(&cache::key(&path)?, look)?;
                Some(Picture { image: image_of(width, height, rgba)?, film: Some(look) })
            }
            Source::Preview { path } => {
                let preview = filmlab_engine::preview::embedded_preview(&path, edge)?;
                Some(Picture { image: image_of(preview.width, preview.height, preview.rgba)?, film: None })
            }
        }
    }
}

impl Picture {
    fn bytes(&self) -> usize {
        self.image.as_bytes(0).map_or(0, <[u8]>::len)
    }
}

impl Pictures {
    pub fn get(&self, id: &str) -> Option<&Picture> {
        self.kept.get(id).map(|k| &k.picture)
    }

    /// Forgets what the last frame wanted; the views drawing this frame want their photos again.
    pub fn begin_frame(&mut self) {
        self.frame += 1;
        self.wanted.clear();
        self.wanted_now.clear();
    }

    pub fn want(&mut self, id: &str) {
        if self.wanted_now.insert(id.to_string()) {
            self.wanted.push(id.to_string());
        }
        if let Some(kept) = self.kept.get_mut(id) {
            kept.seen = self.frame;
        }
    }

    /// The photos the views on screen want, most important first.
    pub fn wanted(&self) -> &[String] {
        &self.wanted
    }

    pub fn is_wanted(&self, id: &str) -> bool {
        self.wanted_now.contains(id)
    }

    pub fn insert(&mut self, id: &str, picture: Picture, cx: &mut App) {
        let seen = if self.is_wanted(id) { self.frame } else { self.kept.get(id).map_or(0, |k| k.seen) };
        if let Some(old) = self.kept.insert(id.to_string(), Kept { picture, seen }) {
            cx.drop_image(old.picture.image, None);
        }
        self.evict(cx);
    }

    fn evict(&mut self, cx: &mut App) {
        let mut held: usize = self.kept.values().map(|k| k.picture.bytes()).sum();
        while held > KEPT_BYTES {
            let Some(oldest) = self.kept.iter().filter(|(_, k)| k.seen < self.frame).min_by_key(|(_, k)| k.seen).map(|(id, _)| id.clone()) else { break };
            if let Some(gone) = self.kept.remove(&oldest) {
                held -= gone.picture.bytes();
                cx.drop_image(gone.picture.image, None);
            }
        }
    }

    pub fn clear(&mut self, cx: &mut App) {
        for (_, gone) in self.kept.drain() {
            cx.drop_image(gone.picture.image, None);
        }
        self.wanted.clear();
        self.wanted_now.clear();
    }
}

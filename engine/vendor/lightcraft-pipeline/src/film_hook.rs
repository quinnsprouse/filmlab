//! Filmlab: replaces the global tone map with a film negative and print when a hook is set.
//! With no hook the render takes exactly LightCraft's code path.

use std::cell::RefCell;
use std::sync::Arc;

/// Where a pixel is: `index` in the rendered image, `x`, `y` in the whole output frame, which is
/// `frame` wide and high (a zoomed window is part of it).
#[derive(Clone, Copy, Debug)]
pub struct Pixel {
    pub index: usize,
    pub x: f32,
    pub y: f32,
    pub frame: [f32; 2],
}

pub trait ToneHook: Send + Sync {
    /// Scene-linear Rec.2020 in (after calibration), display-linear Rec.2020 out.
    fn tone(&self, scene: [f32; 3], at: Pixel) -> [f32; 3];

    /// Only `tone` matters (the pass records the scene): the rest of the pixel's processing is skipped.
    fn capture_only(&self) -> bool {
        false
    }

    /// `tone` already holds every pixel's toned colour, so the scene-referred work before it is
    /// skipped and `scene` is zero.
    fn precomputed(&self) -> bool {
        false
    }
}

// Filmlab: the hook belongs to the thread that renders, so engines rendering at the same time on other threads
// (the native app's loupe, thumbnails and exports) never see each other's film; the stage that reads it runs on
// the rendering thread before it fans out.
thread_local! {
    static HOOK: RefCell<Option<Arc<dyn ToneHook>>> = const { RefCell::new(None) };
}

pub fn with_tone_hook<R>(hook: Arc<dyn ToneHook>, run: impl FnOnce() -> R) -> R {
    let previous = HOOK.with(|h| h.replace(Some(hook)));
    let out = run();
    HOOK.with(|h| *h.borrow_mut() = previous);
    out
}

pub(crate) fn current() -> Option<Arc<dyn ToneHook>> {
    HOOK.with(|h| h.borrow().clone())
}

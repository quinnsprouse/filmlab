//! Filmlab: replaces the global tone map with a film negative and print when a hook is set.
//! With no hook the render takes exactly LightCraft's code path.

use std::cell::{Cell, RefCell};
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

    /// Only the scene matters (the pass records it): the rest of the pixel's processing is skipped, and the pass
    /// hands the scene to [`ToneHook::capture`] instead of calling `tone`.
    fn capture_only(&self) -> bool {
        false
    }

    /// A capture-only pass's scene at once: every pixel of the rendered window, row by row, `width` wide, its
    /// top-left at `origin` in the output frame (which is `frame` wide and high). By default each pixel goes to
    /// `tone` in turn.
    fn capture(&self, scene: Vec<[f32; 3]>, width: usize, origin: [f32; 2], frame: [f32; 2]) {
        for (index, c) in scene.into_iter().enumerate() {
            let at = Pixel { index, x: origin[0] + (index % width) as f32, y: origin[1] + (index / width) as f32, frame };
            self.tone(c, at);
        }
    }

    /// `tone` already holds every pixel's toned colour, so the scene-referred work before it is
    /// skipped and `scene` is zero.
    fn precomputed(&self) -> bool {
        false
    }

    /// A capture-only pass's evaluated masks, after [`ToneHook::capture`], for the precomputed hook made from the
    /// capture to hand back ([`ToneHook::masks`]).
    fn captured_masks(&self, _masks: Arc<Vec<crate::masks::Evaluated>>) {}

    /// A precomputed hook's masks, as its capture's pass evaluated them: a pass of the same render takes them
    /// instead of evaluating them again (and with them skips the scene-referred stages).
    fn masks(&self) -> Option<Arc<Vec<crate::masks::Evaluated>>> {
        None
    }

    /// The output rows a precomputed hook holds, when only some: the pass makes just those (its image is that many
    /// rows high, every pixel as the whole pass makes it), e.g. one band of an export at a time.
    fn rows(&self) -> Option<std::ops::Range<usize>> {
        None
    }

    /// A precomputed hook's colours for `len` pixels of a row, `at` the first of them, appended to `out` as `tone`
    /// gives them one by one (a hook can hand over the row at once).
    fn tone_row(&self, at: Pixel, len: usize, out: &mut Vec<[f32; 3]>) {
        out.extend((0..len).map(|k| self.tone([0.0; 3], Pixel { index: at.index + k, x: at.x + k as f32, ..at })));
    }
}

// Filmlab: the hook belongs to the thread that renders, so engines rendering at the same time on other threads
// (the native app's loupe, thumbnails and exports) never see each other's film; the stage that reads it runs on
// the rendering thread before it fans out.
thread_local! {
    static HOOK: RefCell<Option<Arc<dyn ToneHook>>> = const { RefCell::new(None) };
    static CURVE_INPUT: Cell<bool> = const { Cell::new(false) };
}

/// Puts the thread's previous hook back when its scope ends, by return or by unwinding.
struct Restore(Option<Arc<dyn ToneHook>>);

impl Drop for Restore {
    fn drop(&mut self) {
        let previous = self.0.take();
        HOOK.with(|h| *h.borrow_mut() = previous);
    }
}

fn installed<R>(hook: Option<Arc<dyn ToneHook>>, run: impl FnOnce() -> R) -> R {
    let _restore = Restore(HOOK.with(|h| h.replace(hook)));
    run()
}

pub fn with_tone_hook<R>(hook: Arc<dyn ToneHook>, run: impl FnOnce() -> R) -> R {
    installed(Some(hook), run)
}

// Filmlab: a pool thread waiting inside one render can pick up another render whole (rayon helping), so a render
// starts by setting aside any hook its thread already holds; otherwise a render without film takes the other's
// capture and comes back without pixels, and its scene lands in the other's capture
pub fn without_tone_hook<R>(run: impl FnOnce() -> R) -> R {
    curve_input_set(false, || installed(None, run))
}

pub(crate) fn current() -> Option<Arc<dyn ToneHook>> {
    HOOK.with(|h| h.borrow().clone())
}

struct RestoreCurveInput(bool);

impl Drop for RestoreCurveInput {
    fn drop(&mut self) {
        CURVE_INPUT.set(self.0);
    }
}

fn curve_input_set<R>(on: bool, run: impl FnOnce() -> R) -> R {
    let _restore = RestoreCurveInput(CURVE_INPUT.replace(on));
    run()
}

pub fn with_curve_input<R>(run: impl FnOnce() -> R) -> R {
    curve_input_set(true, run)
}

pub(crate) fn curve_input() -> bool {
    CURVE_INPUT.get()
}

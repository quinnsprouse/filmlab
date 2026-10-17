//! Filmlab: replaces the global tone map with a film negative and print when a hook is set.
//! With no hook the render takes exactly LightCraft's code path.

use std::sync::{Arc, RwLock};

pub trait ToneHook: Send + Sync {
    /// Scene-linear Rec.2020 in (after calibration), display-linear Rec.2020 out; `x`, `y` are the
    /// pixel's position in the whole output frame.
    fn tone(&self, scene: [f32; 3], x: f32, y: f32) -> [f32; 3];
}

static HOOK: RwLock<Option<Arc<dyn ToneHook>>> = RwLock::new(None);

pub fn with_tone_hook<R>(hook: Arc<dyn ToneHook>, run: impl FnOnce() -> R) -> R {
    *HOOK.write().unwrap_or_else(|e| e.into_inner()) = Some(hook);
    let out = run();
    *HOOK.write().unwrap_or_else(|e| e.into_inner()) = None;
    out
}

pub(crate) fn current() -> Option<Arc<dyn ToneHook>> {
    HOOK.read().unwrap_or_else(|e| e.into_inner()).clone()
}

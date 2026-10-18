use gpui_kit::{Bounds, Pixels, Window};
use objc2::MainThreadMarker;
use objc2::rc::Retained;
use objc2_app_kit::{NSGlassEffectView, NSGlassEffectViewStyle, NSView, NSWindowOrderingMode};
use objc2_foundation::{NSPoint, NSRect, NSSize};
use raw_window_handle::{HasWindowHandle, RawWindowHandle};

pub fn renderer_view(window: &Window) -> Option<Retained<NSView>> {
    let handle = HasWindowHandle::window_handle(window).ok()?;
    let RawWindowHandle::AppKit(handle) = handle.as_raw() else { return None };
    unsafe { Retained::retain(handle.ns_view.as_ptr().cast::<NSView>()) }
}

fn frame_in(parent: &NSView, view: &NSView, b: Bounds<Pixels>) -> NSRect {
    let (x, top) = (f64::from(f32::from(b.origin.x)), f64::from(f32::from(b.origin.y)));
    let (w, h) = (f64::from(f32::from(b.size.width)), f64::from(f32::from(b.size.height)));
    let y = if view.isFlipped() { top } else { view.bounds().size.height - top - h };
    parent.convertRect_fromView(NSRect::new(NSPoint::new(x, y), NSSize::new(w, h)), Some(view))
}

// why: AppKit draws Liquid Glass only in its own NSGlassEffectView, so each glass surface is a native view
// placed under GPUI's transparent layer, above the photo, and moved to the GPUI element's bounds each frame
pub struct Glass {
    view: Retained<NSGlassEffectView>,
    placed: Option<(NSRect, f64)>,
}

impl Glass {
    pub fn new(window: &Window, clear: bool) -> Option<Glass> {
        let mtm = MainThreadMarker::new()?;
        let gpui = renderer_view(window)?;
        let parent = unsafe { gpui.superview() }?;
        let view = NSGlassEffectView::new(mtm);
        view.setContentView(Some(&NSView::new(mtm)));
        view.setStyle(if clear { NSGlassEffectViewStyle::Clear } else { NSGlassEffectViewStyle::Regular });
        view.setHidden(true);
        parent.addSubview_positioned_relativeTo(&view, NSWindowOrderingMode::Below, Some(&gpui));
        Some(Glass { view, placed: None })
    }

    pub fn place(&mut self, window: &Window, bounds: Bounds<Pixels>, radius: f64) {
        let Some(gpui) = renderer_view(window) else { return };
        let Some(parent) = (unsafe { gpui.superview() }) else { return };
        let frame = frame_in(&parent, &gpui, bounds);
        if self.placed != Some((frame, radius)) {
            self.view.setFrame(frame);
            self.view.setCornerRadius(radius);
            self.view.setHidden(false);
            self.placed = Some((frame, radius));
        }
    }

    pub fn hide(&mut self) {
        if self.placed.take().is_some() {
            self.view.setHidden(true);
        }
    }
}

impl Drop for Glass {
    fn drop(&mut self) {
        self.view.removeFromSuperview();
    }
}

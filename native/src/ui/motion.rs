use objc2_app_kit::NSWorkspace;

/// A CSS-style cubic Bézier easing, x solved by bisection (monotonic for x1, x2 in 0..=1).
pub fn bezier(x1: f32, y1: f32, x2: f32, y2: f32) -> impl Fn(f32) -> f32 {
    move |t| {
        let curve = |a: f32, b: f32, s: f32| {
            let u = 1.0 - s;
            3.0 * u * u * s * a + 3.0 * u * s * s * b + s * s * s
        };
        let (mut lo, mut hi) = (0.0f32, 1.0f32);
        for _ in 0..24 {
            let mid = (lo + hi) / 2.0;
            if curve(x1, x2, mid) < t { lo = mid } else { hi = mid }
        }
        curve(y1, y2, (lo + hi) / 2.0)
    }
}

/// The interface's ease-out for entering and changing state, cubic-bezier(0.2, 0, 0, 1).
pub fn emphasized() -> impl Fn(f32) -> f32 {
    bezier(0.2, 0.0, 0.0, 1.0)
}

/// macOS's Reduce Motion accessibility setting.
pub fn reduced() -> bool {
    NSWorkspace::sharedWorkspace().accessibilityDisplayShouldReduceMotion()
}

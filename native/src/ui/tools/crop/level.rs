use std::time::{Duration, Instant};

use crate::ui::motion;

const HOLD: Duration = Duration::from_millis(700);
const FADE: Duration = Duration::from_millis(250);

#[derive(Default)]
pub struct Leveling {
    seen: Option<(Option<String>, f64)>,
    changed: Option<Instant>,
}

impl Leveling {
    pub fn strength(&mut self, photo: Option<String>, angle: f64) -> f32 {
        if self.seen.as_ref().is_some_and(|(p, a)| *p == photo && *a != angle) {
            self.changed = Some(Instant::now());
        }
        self.seen = Some((photo, angle));
        let Some(changed) = self.changed else { return 0.0 };
        let fading = changed.elapsed().saturating_sub(HOLD);
        if fading.is_zero() {
            return 1.0;
        }
        if motion::reduced() || fading >= FADE {
            self.changed = None;
            return 0.0;
        }
        1.0 - motion::emphasized()(fading.as_secs_f32() / FADE.as_secs_f32())
    }
}

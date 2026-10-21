use std::sync::Arc;
use std::time::Instant;

use gpui_kit::{AppContext, Context};
use rayon::prelude::*;
use serde_json::json;

use super::plans::{Layer, Plan, plans, same_look, visible_region};
use super::{Develop, DisplayFrame, Running, Wish};
use crate::camera::{Region, camera_of};

/// How far past the visible edge the detail frame reaches before a strip is rendered there.
const LEAD: usize = 256;
/// How wide a strip is: narrow enough to render between a few frames of a pan.
const STRIP: usize = 192;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
struct Rect {
    x: usize,
    y: usize,
    w: usize,
    h: usize,
}

impl Rect {
    fn of(plan: &Plan) -> Option<Rect> {
        let w = plan.view["window"].as_array()?;
        let at = |k: usize| w.get(k).and_then(serde_json::Value::as_u64).map(|v| v as usize);
        Some(Rect { x: at(0)?, y: at(1)?, w: at(2)?, h: at(3)? })
    }

    fn right(self) -> usize {
        self.x + self.w
    }

    fn bottom(self) -> usize {
        self.y + self.h
    }

    fn union(self, other: Rect) -> Rect {
        let (x, y) = (self.x.min(other.x), self.y.min(other.y));
        Rect { x, y, w: self.right().max(other.right()) - x, h: self.bottom().max(other.bottom()) - y }
    }

    fn region(self, (fw, fh): (usize, usize)) -> Region {
        let (fw, fh) = (fw as f64, fh as f64);
        Region { u0: self.x as f64 / fw, v0: self.y as f64 / fh, u1: self.right() as f64 / fw, v1: self.bottom() as f64 / fh }
    }
}

fn full_of(plan: &Plan) -> Option<(usize, usize)> {
    Some((plan.view["maxW"].as_u64()? as usize, plan.view["maxH"].as_u64()? as usize))
}

/// Trims `span` (on one axis) to at most `most` long, keeping `keep` inside it and cutting from the side `keep` is
/// farther from.
fn trim(start: usize, len: usize, keep: (usize, usize), most: usize) -> (usize, usize) {
    if len <= most {
        return (start, len);
    }
    let cut = len - most;
    let before = keep.0.saturating_sub(start);
    let after = (start + len).saturating_sub(keep.1);
    if before >= after { (start + cut.min(before), most) } else { (start, most) }
}

enum Ahead {
    Covered,
    Strip(Rect, Rect),
    Lost,
}

/// The next strip beyond the shown window toward where the view is heading, and the window the shown one becomes
/// with it joined on (no wider than the view plus a strip and its lead on each side).
fn ahead(shown: Rect, full: (usize, usize), visible: Region) -> Ahead {
    let (fw, fh) = full;
    let px = |u: f64, n: usize| (u * n as f64).round() as usize;
    let (vx0, vx1, vy0, vy1) = (px(visible.u0, fw), px(visible.u1, fw), px(visible.v0, fh), px(visible.v1, fh));
    let short = |want: usize, have: usize| want.saturating_sub(have);
    let needs = [
        (short((vx1 + LEAD).min(fw), shown.right()), Rect { x: shown.right(), y: shown.y, w: STRIP.min(fw - shown.right()), h: shown.h }),
        (short(shown.x, vx0.saturating_sub(LEAD)), Rect { x: shown.x.saturating_sub(STRIP), y: shown.y, w: shown.x.min(STRIP), h: shown.h }),
        (short((vy1 + LEAD).min(fh), shown.bottom()), Rect { x: shown.x, y: shown.bottom(), w: shown.w, h: STRIP.min(fh - shown.bottom()) }),
        (short(shown.y, vy0.saturating_sub(LEAD)), Rect { x: shown.x, y: shown.y.saturating_sub(STRIP), w: shown.w, h: shown.y.min(STRIP) }),
    ];
    let Some((need, strip)) = needs.into_iter().filter(|(need, s)| *need > 0 && s.w > 0 && s.h > 0).max_by_key(|(need, _)| *need) else { return Ahead::Covered };
    if need > LEAD + STRIP {
        return Ahead::Lost;
    }
    let joined = shown.union(strip);
    let reach = LEAD + STRIP;
    let (x, w) = trim(joined.x, joined.w, (vx0, vx1), vx1 - vx0 + 2 * reach);
    let (y, h) = trim(joined.y, joined.h, (vy0, vy1), vy1 - vy0 + 2 * reach);
    Ahead::Strip(strip, Rect { x, y, w, h })
}

fn blit(into: &mut [u8], to: Rect, from: &[u8], at: Rect) {
    let x0 = to.x.max(at.x);
    let x1 = to.right().min(at.right());
    if x1 <= x0 {
        return;
    }
    let bytes = (x1 - x0) * 4;
    into.par_chunks_mut(to.w * 4).enumerate().for_each(|(j, row)| {
        let y = to.y + j;
        if y < at.y || y >= at.bottom() {
            return;
        }
        let src = ((y - at.y) * at.w + (x0 - at.x)) * 4;
        let dst = (x0 - to.x) * 4;
        row[dst..dst + bytes].copy_from_slice(&from[src..src + bytes]);
    });
}

fn joined(old: &DisplayFrame, was: Rect, strip: &[u8], at: Rect, to: Rect, full: (usize, usize)) -> DisplayFrame {
    let mut rgba = vec![0u8; to.w * to.h * 4];
    blit(&mut rgba, to, &old.rgba, was);
    blit(&mut rgba, to, strip, at);
    DisplayFrame { width: to.w as u32, height: to.h as u32, rgba, region: to.region(full), histogram: old.histogram.clone() }
}

/// A shown detail frame made of strips joined on while panning: it looks right but isn't one render of its window,
/// so once the pan settles that window is rendered whole.
pub fn pieced(plan: &Plan) -> bool {
    plan.view.get("pieced").is_some()
}

/// A strip to render and what it joins: the detail plan and frame on screen (`was` in the photo's pixels at the
/// zoom) and the window they become (`to`).
struct Strip {
    plan: Plan,
    base: Plan,
    frame: Arc<DisplayFrame>,
    was: Rect,
    at: Rect,
    to: Rect,
    full: (usize, usize),
}

impl Develop {
    // why: a pan moves the detail frame already on screen; only the edge it uncovers is new, so while the pan goes on a
    // strip is rendered there and joined on, instead of the whole window once the hand stops. A pan that leaves the
    // shown window behind altogether is false: its window is rendered whole at once
    pub(super) fn pan_ahead(&mut self, cx: &mut Context<Develop>) -> bool {
        let Some(Strip { plan, base, frame, was, at: strip, to, full }) = (match self.strip() {
            Some(Ok(strip)) => Some(strip),
            Some(Err(Ahead::Lost)) => return false,
            _ => None,
        }) else {
            return true;
        };
        let started = Instant::now();
        let wish = Wish::new();
        let rendering = self.rendering(&plan, &wish, cx);
        self.running = Some(Running { plan, curve_input: false, wish });
        cx.spawn(async move |this, cx| {
            let rendered = rendering.await;
            let made = match rendered {
                Ok(Some(mut f)) if (f.width as usize, f.height as usize) == (strip.w, strip.h) => {
                    let pixels = f.take_rgba();
                    Some(cx.background_spawn(async move { joined(&frame, was, &pixels, strip, to, full) }).await)
                }
                _ => None,
            };
            if std::env::var_os("FILMLAB_TRACE").is_some() {
                eprintln!("pan strip {strip:?} into {to:?} {:.1} ms joined {}", started.elapsed().as_secs_f64() * 1000.0, made.is_some());
            }
            let _ = this.update(cx, |develop, cx| {
                develop.running = None;
                if let Some(made) = made
                    && develop.shown.get(&true) == Some(&base)
                {
                    let mut view = base.view.clone();
                    view["window"] = json!([to.x, to.y, to.w, to.h]);
                    view["pieced"] = json!(true);
                    develop.detail = Some(Arc::new(made));
                    develop.shown.insert(true, Plan { view, region: to.region(full), ..base });
                    cx.notify();
                }
                develop.pump(cx);
            });
        })
        .detach();
        true
    }

    fn strip(&self) -> Option<Result<Strip, Ahead>> {
        let shown = self.shown.get(&true)?;
        let frame = self.detail.clone()?;
        let inputs = self.inputs()?;
        let wanted = plans(&inputs, self.target_zoom(), self.viewport, None).into_iter().find(|p| p.layer == Layer::Detail)?;
        if !same_look(shown, &wanted) || shown.level != wanted.level || shown.view["maxW"] != wanted.view["maxW"] {
            return None;
        }
        let was = Rect::of(shown)?;
        let full = full_of(shown)?;
        if (frame.width as usize, frame.height as usize) != (was.w, was.h) {
            return None;
        }
        let camera = camera_of(self.target_zoom(), self.native, self.viewport);
        let (at, to) = match ahead(was, full, visible_region(camera, self.native, self.viewport)) {
            Ahead::Strip(at, to) => (at, to),
            other => return Some(Err(other)),
        };
        let mut view = wanted.view.clone();
        view["window"] = json!([at.x, at.y, at.w, at.h]);
        let plan = Plan { view, region: at.region(full), ..wanted };
        Some(Ok(Strip { plan, base: shown.clone(), frame, was, at, to, full }))
    }
}

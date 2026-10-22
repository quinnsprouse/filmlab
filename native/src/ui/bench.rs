use std::time::{Duration, Instant};

use gpui_kit::*;

use super::app::{FilmlabApp, Mode};
use crate::develop::{Develop, Tool};
use super::driver::Driver;
use crate::camera::{Camera, Zoom};
use crate::library::PhotoRecord;

// why: FILMLAB_BENCH=1 times what a user waits for (window, library, opening and switching photos, edits, zoom) on
// the folder last opened, so speed work can be measured before and after; results go to stderr as "bench" lines
static STARTED: std::sync::OnceLock<Instant> = std::sync::OnceLock::new();

pub fn mark_launch() {
    let _ = STARTED.set(Instant::now());
}

/// A frame the viewer presented: when, through which camera, and what it showed.
#[derive(Clone, Copy, Debug)]
pub struct Shot {
    pub at: Instant,
    pub camera: Camera,
    /// The fit frame's revision, and whether it was a drag's draft.
    pub fit: Option<(u64, bool)>,
    /// The zoomed frame's revision, and whether it was a drag's draft.
    pub detail: Option<(u64, bool)>,
    /// The zoomed frame covers everything the camera shows, so no part of the view is the blurred fit frame.
    pub sharp: bool,
}

static RECORDING: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
static SHOTS: std::sync::Mutex<Vec<Shot>> = std::sync::Mutex::new(Vec::new());

/// What the viewer is about to show of `d`, while a bench records presented frames.
pub fn shot_of(d: &Develop) -> Option<Shot> {
    if !RECORDING.load(std::sync::atomic::Ordering::Relaxed) {
        return None;
    }
    let seen = crate::develop::plans::visible_region(d.camera, d.native, d.viewport);
    let sharp = d.detail.as_ref().is_some_and(|f| {
        let r = f.region;
        r.u0 <= seen.u0 + 1e-6 && r.v0 <= seen.v0 + 1e-6 && r.u1 >= seen.u1 - 1e-6 && r.v1 >= seen.v1 - 1e-6
    });
    let shown = |detail: bool| d.shown_plan(detail).map(|p| (p.revision, crate::develop::plans::is_draft(p)));
    let fit = shown(false).filter(|_| d.fit.is_some());
    let detail = shown(true).filter(|_| d.detail.is_some());
    Some(Shot { at: Instant::now(), camera: d.camera, fit, detail, sharp })
}

/// Logs a frame the viewer presented.
pub fn presented(mut shot: Shot) {
    shot.at = Instant::now();
    if let Ok(mut shots) = SHOTS.lock() {
        shots.push(shot);
    }
}

fn record() {
    if let Ok(mut shots) = SHOTS.lock() {
        shots.clear();
    }
    RECORDING.store(true, std::sync::atomic::Ordering::Relaxed);
}

fn recorded() -> Vec<Shot> {
    RECORDING.store(false, std::sync::atomic::Ordering::Relaxed);
    SHOTS.lock().map(|mut s| std::mem::take(&mut *s)).unwrap_or_default()
}

/// Time from `at` to the first frame presented after it that `shows`.
fn wait_for(shots: &[Shot], at: Instant, shows: impl Fn(&Shot) -> bool) -> Option<f64> {
    shots.iter().find(|s| s.at >= at && shows(s)).map(|s| ms(s.at - at))
}

fn gaps(shots: &[Shot], from: Instant, to: Instant) -> (usize, f64) {
    let times: Vec<Instant> = std::iter::once(from).chain(shots.iter().map(|s| s.at).filter(|t| *t >= from && *t <= to)).chain(std::iter::once(to)).collect();
    let worst = times.windows(2).map(|w| ms(w[1] - w[0])).fold(0.0, f64::max);
    (times.len() - 2, worst)
}

fn summary(samples: &mut [f64]) -> String {
    if samples.is_empty() {
        return "no samples".into();
    }
    samples.sort_by(f64::total_cmp);
    let p = |q: f64| samples[((samples.len() - 1) as f64 * q).round() as usize];
    format!("p50 {:.1} p90 {:.1} max {:.1} ms", p(0.5), p(0.9), p(1.0))
}

fn layer(s: &Shot, detail: bool) -> Option<(u64, bool)> {
    if detail { s.detail } else { s.fit }
}

// why: macOS stops drawing a window that something else covers, and a frame never drawn can't be timed
fn to_front(driver: &Driver, cx: &mut AsyncApp) {
    driver.on_window(cx, |window, cx| {
        cx.activate(true);
        window.activate_window();
    });
}

/// Edits sent `gap` apart, `count` of them, and when each was sent with the revision it made.
async fn send_edits(driver: &Driver, cx: &mut AsyncApp, count: u32, gap: Duration, edit: impl Fn(u32, &mut Develop, &mut Context<Develop>)) -> Vec<(Instant, u64)> {
    let mut sent = Vec::new();
    for i in 0..count {
        let at = Instant::now();
        let edit = &edit;
        if let Some(revision) = driver.develop(cx, move |d, cx| {
            edit(i, d, cx);
            d.planned_revision()
        }) {
            sent.push((at, revision));
        }
        cx.background_executor().timer(gap.saturating_sub(at.elapsed())).await;
    }
    sent
}

/// Edit to present for edits on `detail` (the zoomed frame) or the fit frame: per edit, how long until a frame that
/// shows it (or a later edit) was presented, drafts included, and how long after the last edit the finished frame was.
async fn edit_latency(driver: &Driver, cx: &mut AsyncApp, name: &str, detail: bool, count: u32, gap: Duration) {
    to_front(driver, cx);
    cx.background_executor().timer(Duration::from_millis(400)).await;
    record();
    let started = Instant::now();
    let sent = send_edits(driver, cx, count, gap, |i, d, cx| {
        let v = -0.5 + f64::from(i) / f64::from(count);
        d.edit_controls(&[("light.exposure", v)], "Exposure", cx);
    })
    .await;
    let ended = Instant::now();
    let last_sent = sent.last().map_or(ended, |s| s.0);
    driver.develop(cx, |d, cx| d.release(cx));
    until(driver, cx, 30, move |a, cx| {
        let d = a.develop.read(cx);
        !d.drafting() && d.idle_for(Duration::ZERO) && d.shown_plan(detail).is_some_and(|p| p.revision == d.planned_revision())
    })
    .await;
    cx.background_executor().timer(Duration::from_millis(50)).await;
    let shots = recorded();
    let mut seen: Vec<f64> = sent.iter().filter_map(|&(at, rev)| wait_for(&shots, at, |s| layer(s, detail).is_some_and(|(r, _)| r >= rev))).collect();
    let last = sent.last().map_or(0, |s| s.1);
    let full = wait_for(&shots, last_sent, |s| layer(s, detail).is_some_and(|(r, draft)| r >= last && !draft));
    let updates = {
        let mut revs: Vec<u64> = shots.iter().filter(|s| s.at <= ended).filter_map(|s| layer(s, detail).map(|l| l.0)).filter(|r| *r >= sent.first().map_or(0, |s| s.1)).collect();
        revs.dedup();
        revs.len()
    };
    let missed = sent.len() - seen.len();
    eprintln!(
        "bench {name:<28} edit to present {}, {:.1} updates/s, finished {:.1} ms after the last edit{}",
        summary(&mut seen),
        updates as f64 / (ended - started).as_secs_f64(),
        full.unwrap_or(f64::NAN),
        if missed > 0 { format!(", {missed} never shown in {} frames presented", shots.len()) } else { String::new() }
    );
}

/// A pan at 1:1 by `step` pixels every frame: frames presented, input to present (a frame through the camera of
/// that move or a later one), and input to sharp (such a frame with no part of the view drawn from the fit frame).
async fn pan(driver: &Driver, cx: &mut AsyncApp, name: &str, count: u32, step: f64) {
    let native = driver.read(cx, |a, cx| a.develop.read(cx).native).unwrap_or((1.0, 1.0));
    let start = 0.3;
    driver.develop(cx, move |d, cx| d.set_zoom(Zoom::At { scale: 1.0, cx: start, cy: 0.5 }, false, false, cx));
    driver.settled(cx, true).await;
    to_front(driver, cx);
    cx.background_executor().timer(Duration::from_millis(100)).await;
    record();
    let started = Instant::now();
    let mut moves: Vec<(Instant, Camera)> = Vec::new();
    for i in 0..count {
        let at = Instant::now();
        let c = start + step * f64::from(i + 1) / native.0;
        if let Some(camera) = driver.develop(cx, move |d, cx| {
            d.set_zoom(Zoom::At { scale: 1.0, cx: c, cy: 0.5 }, false, true, cx);
            d.camera
        }) {
            moves.push((at, camera));
        }
        cx.background_executor().timer(Duration::from_millis(16).saturating_sub(at.elapsed())).await;
    }
    let ended = Instant::now();
    let settled = until(driver, cx, 60, |a, cx| {
        let d = a.develop.read(cx);
        let seen = crate::develop::plans::visible_region(d.camera, d.native, d.viewport);
        let covers = |r: &crate::camera::Region| r.u0 <= seen.u0 + 1e-6 && r.v0 <= seen.v0 + 1e-6 && r.u1 >= seen.u1 - 1e-6 && r.v1 >= seen.v1 - 1e-6;
        d.detail.is_some() && d.idle_for(Duration::ZERO) && d.shown_plan(true).is_some_and(|p| p.revision == d.planned_revision() && covers(&p.region) && !crate::develop::pan::pieced(p))
    })
    .await;
    cx.background_executor().timer(Duration::from_millis(50)).await;
    let shots = recorded();
    let move_of = |s: &Shot| moves.iter().position(|(_, c)| *c == s.camera);
    let mut present = Vec::new();
    let mut sharp = Vec::new();
    for (i, (at, _)) in moves.iter().enumerate() {
        present.extend(wait_for(&shots, *at, |s| move_of(s).is_some_and(|k| k >= i)));
        sharp.extend(wait_for(&shots, *at, |s| s.sharp && move_of(s).is_some_and(|k| k >= i)));
    }
    let (frames, worst) = gaps(&shots, started, ended);
    let during: Vec<&Shot> = shots.iter().filter(|s| s.at >= started && s.at <= ended).collect();
    let blurred = during.iter().filter(|s| !s.sharp).count();
    eprintln!(
        "bench {name:<28} {:.1} fps presented, worst gap {worst:.1} ms, {blurred} of {frames} frames partly blurred, settled {:.1} ms after the last move",
        frames as f64 / (ended - started).as_secs_f64(),
        settled.map_or(f64::NAN, ms)
    );
    eprintln!("bench {:<28} input to present {}", "", summary(&mut present));
    eprintln!("bench {:<28} input to sharp {}", "", summary(&mut sharp));
}

/// Zooming to 1:1 at a few places: time to the first presented frame sharp everywhere at the current edit.
async fn zoom_in(driver: &Driver, cx: &mut AsyncApp) {
    let mut zoom = Vec::new();
    for i in 0..4 {
        let c = 0.3 + 0.12 * f64::from(i);
        to_front(driver, cx);
        record();
        let t = Instant::now();
        let Some(revision) = driver.develop(cx, move |d, cx| {
            d.set_zoom(Zoom::At { scale: 1.0, cx: c, cy: c }, false, false, cx);
            d.planned_revision()
        }) else {
            continue;
        };
        until(driver, cx, 60, |a, cx| {
            let d = a.develop.read(cx);
            d.detail.is_some() && d.shown_plan(true).is_some_and(|p| p.revision == d.planned_revision()) && d.idle_for(Duration::ZERO)
        })
        .await;
        cx.background_executor().timer(Duration::from_millis(50)).await;
        zoom.extend(wait_for(&recorded(), t, |s| s.sharp && s.detail.is_some_and(|(r, _)| r >= revision)));
        cx.background_executor().timer(Duration::from_millis(250)).await;
    }
    report("zoom to 1:1 (presented)", &mut zoom);
}

/// Exports the other photos again and again, as a batch export would, until `stop` is set.
fn export_loop(driver: &Driver, cx: &mut AsyncApp, others: Vec<String>, stop: std::sync::Arc<std::sync::atomic::AtomicBool>) -> Option<Task<usize>> {
    let choices = super::export::run::Choices::default();
    let jobs = driver.read(cx, move |a, cx| super::export::run::jobs_for(a.library.read(cx), a.develop.read(cx), &others, &choices))?;
    let folder = std::env::var_os("HOME").map(|h| std::path::PathBuf::from(h).join("bench-exports"));
    Some(cx.background_spawn(async move {
        let mut engine = filmlab_engine::core::Engine::new();
        let mut done = 0;
        while !stop.load(std::sync::atomic::Ordering::Relaxed) {
            for (_, job) in &jobs {
                let Ok(job) = job else { continue };
                if crate::perf::initiated(|| super::export::run::export_one(&mut engine, job, &choices, folder.as_deref(), None)).is_ok() {
                    done += 1;
                }
                if stop.load(std::sync::atomic::Ordering::Relaxed) {
                    break;
                }
            }
        }
        done
    }))
}

fn ms(d: Duration) -> f64 {
    (d.as_secs_f64() * 10_000.0).round() / 10.0
}

fn report(name: &str, samples: &mut [f64]) {
    if samples.is_empty() {
        return eprintln!("bench {name:<28} no samples");
    }
    samples.sort_by(f64::total_cmp);
    let p = |q: f64| samples[((samples.len() - 1) as f64 * q).round() as usize];
    eprintln!("bench {name:<28} n {:>2}  p50 {:>7.1} ms  p90 {:>7.1} ms  max {:>7.1} ms", samples.len(), p(0.5), p(0.9), p(1.0));
}

async fn until(driver: &Driver, cx: &mut AsyncApp, seconds: u64, done: impl Fn(&FilmlabApp, &App) -> bool) -> Option<Duration> {
    let started = Instant::now();
    while started.elapsed() < Duration::from_secs(seconds) {
        if driver.read(cx, &done).unwrap_or(false) {
            return Some(started.elapsed());
        }
        cx.background_executor().timer(Duration::from_millis(2)).await;
    }
    None
}

/// The fit frame on screen shows the latest edit of `photo`.
fn shows(a: &FilmlabApp, cx: &App, photo: &str) -> bool {
    let d = a.develop.read(cx);
    d.photo_id.as_deref() == Some(photo) && d.fit.is_some() && d.shown_plan(false).is_some_and(|p| p.revision == d.planned_revision() && p.photo_id == photo)
}

fn current(a: &FilmlabApp, cx: &App) -> Option<String> {
    a.develop.read(cx).photo_id.clone()
}

fn megabytes() -> f64 {
    super::soak::footprint() as f64 / 1_048_576.0
}

/// Times between the frames drawn while `step` runs before each of them; the app re-renders every frame.
async fn frames(driver: &Driver, cx: &mut AsyncApp, mut step: impl FnMut(&mut FilmlabApp, &mut Context<FilmlabApp>) -> bool) -> Vec<f64> {
    let mut intervals = Vec::new();
    let mut last: Option<Instant> = None;
    loop {
        let (drawn, next) = futures::channel::oneshot::channel();
        driver.on_window(cx, move |window, _| {
            window.on_next_frame(move |_, _| {
                let _ = drawn.send(Instant::now());
            })
        });
        let Ok(at) = next.await else { break };
        if let Some(previous) = last {
            intervals.push(ms(at - previous));
        }
        last = Some(at);
        let more = driver.act(cx, |a, cx| {
            let more = step(a, cx);
            cx.notify();
            more
        });
        if more != Some(true) {
            break;
        }
    }
    intervals
}

fn report_frames(name: &str, intervals: &mut [f64]) {
    let slow = intervals.iter().filter(|t| **t > 17.5).count();
    let fps = intervals.len() as f64 * 1000.0 / intervals.iter().sum::<f64>().max(1.0);
    report(name, intervals);
    eprintln!("bench {:<28} {fps:>7.1} fps, {slow} of {} frames over 17.5 ms", "", intervals.len());
}

async fn library(driver: &Driver, cx: &mut AsyncApp, launch: f64) {
    const LONG: u64 = 3600;
    let started = Instant::now();
    let since_launch = move || launch + ms(started.elapsed());
    let first = until(driver, cx, LONG, |a, cx| a.viewport_pictures(cx).0 > 0).await;
    eprintln!("bench {:<28} {:>7.1} ms from launch", "grid first picture", first.map_or(f64::NAN, |_| since_launch()));
    let filled = until(driver, cx, LONG, |a, cx| {
        let (shown, tiles) = a.viewport_pictures(cx);
        tiles > 0 && shown == tiles
    })
    .await;
    eprintln!("bench {:<28} {:>7.1} ms from launch", "grid viewport filled", filled.map_or(f64::NAN, |_| since_launch()));
    let sampling = Instant::now();
    let mut indexing = frames(driver, cx, move |_, _| sampling.elapsed() < Duration::from_secs(3)).await;
    report_frames("frame while indexing", &mut indexing);
    let indexed = until(driver, cx, LONG, |a, cx| {
        let l = a.library.read(cx);
        !l.photos.is_empty() && l.photos.iter().all(PhotoRecord::indexed)
    })
    .await;
    let count = driver.read(cx, |a, cx| a.library.read(cx).photos.len()).unwrap_or(0);
    eprintln!("bench {:<28} {:>7.1} ms from launch for {count} photos", "library indexed", indexed.map_or(f64::NAN, |_| since_launch()));
    cx.background_executor().timer(Duration::from_secs(3)).await;
    eprintln!("bench {:<28} {:>7.1} MB", "memory, indexed and idle", megabytes());
    driver.act(cx, |a, _| a.browse.scroll_to(0.0));
    let mut scrolling = frames(driver, cx, |a, _| {
        let top = a.browse.scrolled() + 400.0;
        a.browse.scroll_to(top);
        top < a.browse.scroll_range()
    })
    .await;
    let range = driver.read(cx, |a, _| a.browse.scroll_range()).unwrap_or(0.0);
    report_frames("frame scrolling the grid", &mut scrolling);
    eprintln!("bench {:<28} {range:>7.0} pt", "grid height scrolled");
    cx.background_executor().timer(Duration::from_secs(3)).await;
    eprintln!("bench {:<28} {:>7.1} MB", "memory, after scrolling", megabytes());
    driver.act(cx, |a, cx| {
        a.browse.scroll_to(0.0);
        cx.notify();
    });
}

fn frame_of(a: &FilmlabApp, cx: &App, detail: bool) -> Option<usize> {
    let d = a.develop.read(cx);
    if detail { d.detail.as_ref() } else { d.fit.as_ref() }.map(|f| std::sync::Arc::as_ptr(f) as usize)
}

struct Moves {
    frames: usize,
    worst: Duration,
    took: Duration,
}

async fn moves(driver: &Driver, cx: &mut AsyncApp, detail: bool, count: u32, step: impl Fn(u32, &mut AsyncApp)) -> Moves {
    let mut last = driver.read(cx, |a, cx| frame_of(a, cx, detail)).flatten();
    let (mut frames, mut worst, started) = (0usize, Duration::ZERO, Instant::now());
    let mut since = started;
    for i in 0..count {
        step(i, cx);
        let tick = Instant::now();
        while tick.elapsed() < Duration::from_millis(16) {
            let now = driver.read(cx, |a, cx| frame_of(a, cx, detail)).flatten();
            if now != last {
                frames += 1;
                last = now;
                worst = worst.max(since.elapsed());
                since = Instant::now();
            }
            cx.background_executor().timer(Duration::from_millis(1)).await;
        }
    }
    Moves { frames, worst: worst.max(since.elapsed()), took: started.elapsed() }
}

async fn drag_exposure(driver: &Driver, cx: &mut AsyncApp, photo: &str, name: &str) {
    cx.background_executor().timer(Duration::from_millis(400)).await;
    let d = driver.clone();
    let dragged = moves(driver, cx, false, 60, move |i, cx| {
        let v = -0.5 + f64::from(i) / 60.0;
        d.develop(cx, move |d, cx| d.edit_controls(&[("light.exposure", v)], "Exposure", cx));
    })
    .await;
    driver.develop(cx, |d, cx| d.release(cx));
    let p = photo.to_string();
    let full = until(driver, cx, 30, move |a, cx| !a.develop.read(cx).drafting() && shows(a, cx, &p)).await;
    eprintln!(
        "bench {name:<28} {:>7.1} fps over {:.0} ms, worst gap {:.1} ms, full frame {:.1} ms after release",
        dragged.frames as f64 / dragged.took.as_secs_f64(),
        ms(dragged.took),
        ms(dragged.worst),
        full.map_or(f64::NAN, ms)
    );
}

/// UI-thread time for one event: `step`, then drawing the window when it asked to be drawn again.
fn drawn(driver: &Driver, cx: &mut AsyncApp, step: impl FnOnce(&Entity<FilmlabApp>, &mut Window, &mut App)) -> Option<f64> {
    let app = driver.app.upgrade()?;
    let notified = std::rc::Rc::new(std::cell::Cell::new(false));
    let seen = notified.clone();
    let watch = cx.update(|cx| cx.observe(&app, move |_, _| seen.set(true)));
    let t = Instant::now();
    driver.on_window(cx, move |window, cx| step(&app, window, cx))?;
    driver.on_window(cx, |window, cx| {
        if notified.get() {
            window.draw(cx).clear(cx);
        }
    })?;
    drop(watch);
    Some(ms(t.elapsed()))
}

async fn slider_steps(driver: &Driver, cx: &mut AsyncApp, name: &str) {
    let mut steps = Vec::new();
    for i in 0..40 {
        let v = -0.4 + f64::from(i) / 50.0;
        steps.extend(drawn(driver, cx, move |app, _, cx| app.update(cx, |a, cx| a.develop.update(cx, |d, cx| d.edit_controls(&[("light.exposure", v)], "Exposure", cx)))));
        cx.background_executor().timer(Duration::from_millis(16)).await;
    }
    driver.develop(cx, |d, cx| d.release(cx));
    report(name, &mut steps);
    driver.settled(cx, false).await;
}

fn mouse_move(window: &mut Window, cx: &mut App, at: Point<Pixels>) {
    window.dispatch_event(PlatformInput::MouseMove(MouseMoveEvent { position: at, pressed_button: None, modifiers: Modifiers::default() }), cx);
}

/// Moves the pointer over the fit frame along `path` (frame fractions, from 0 to 1 over the moves).
async fn pointer_moves(driver: &Driver, cx: &mut AsyncApp, name: &str, path: impl Fn(f32) -> (f32, f32)) {
    let Some(frame) = driver.on_window(cx, |window, cx| driver.app.upgrade().and_then(|a| a.read(cx).fit_frame(window, cx))).flatten() else {
        return eprintln!("bench {name:<28} no frame");
    };
    let (origin, size) = (frame.0.origin, frame.0.size);
    driver.on_window(cx, |window, cx| window.draw(cx).clear(cx));
    let mut moves = Vec::new();
    for i in 0..80 {
        let (u, v) = path(i as f32 / 80.0);
        let at = point(origin.x + size.width * u, origin.y + size.height * v);
        moves.extend(drawn(driver, cx, move |_, window, cx| mouse_move(window, cx, at)));
        cx.background_executor().timer(Duration::from_millis(8)).await;
    }
    report(name, &mut moves);
}

fn across(t: f32) -> (f32, f32) {
    (0.2 + 0.6 * t, 0.3 + 0.4 * t)
}

async fn heal(driver: &Driver, cx: &mut AsyncApp) {
    driver.act(cx, |a, cx| a.set_tool(Tool::Heal, cx));
    driver.settled(cx, false).await;
    pointer_moves(driver, cx, "UI per heal move, no spots", across).await;
    for i in 0..24 {
        let (x, y) = (0.1 + 0.035 * f64::from(i), 0.2 + 0.025 * f64::from(i % 8));
        driver.develop(cx, move |d, cx| d.command("spot.add", serde_json::json!({"mode": "heal", "points": [[x, y], [x + 0.01, y + 0.01]], "size": 0.01, "feather": 50.0, "opacity": 100.0}), "Heal spot", &format!("bench.spot.{i}"), cx));
    }
    driver.settled(cx, false).await;
    pointer_moves(driver, cx, "UI per heal move, 24 spots", across).await;
    let grip = driver.read(cx, |a, cx| a.develop.read(cx).fit.as_ref().map(|f| (f.width as f32, f.height as f32))).flatten().unwrap_or((1.0, 1.0));
    let wobble = 1.5 / grip.0.max(grip.1);
    pointer_moves(driver, cx, "UI per heal move, on a spot", move |t| (0.1 + wobble * (t * 40.0).sin(), 0.2 + wobble * (t * 40.0).cos())).await;
    driver.act(cx, |a, cx| a.set_tool(Tool::Edit, cx));
    driver.settled(cx, false).await;
}

async fn long_history(driver: &Driver, cx: &mut AsyncApp) {
    for i in 0..200 {
        let v = f64::from(i % 20) / 100.0;
        let edit: (&str, f64, &str) = if i % 2 == 0 { ("light.exposure", v, "Exposure") } else { ("color.vibrance", v * 100.0, "Vibrance") };
        driver.develop(cx, move |d, cx| d.edit_controls(&[(edit.0, edit.1)], edit.2, cx));
    }
    let steps = driver.read(cx, |a, cx| a.develop.read(cx).history.len()).unwrap_or(0);
    driver.settled(cx, false).await;
    let mut undo = Vec::new();
    let mut redo = Vec::new();
    for _ in 0..20 {
        undo.extend(drawn(driver, cx, |app, _, cx| app.update(cx, |a, cx| a.develop.update(cx, |d, cx| d.undo(cx)))));
        cx.background_executor().timer(Duration::from_millis(30)).await;
    }
    for _ in 0..20 {
        redo.extend(drawn(driver, cx, |app, _, cx| app.update(cx, |a, cx| a.develop.update(cx, |d, cx| d.redo(cx)))));
        cx.background_executor().timer(Duration::from_millis(30)).await;
    }
    report(&format!("UI per undo, {steps} steps"), &mut undo);
    report(&format!("UI per redo, {steps} steps"), &mut redo);
    driver.settled(cx, false).await;
}

/// Brushes a long stroke into a mask, so the edit carries what a retouched photo does.
async fn brush(driver: &Driver, cx: &mut AsyncApp) {
    let points: Vec<[f64; 2]> = (0..600).map(|i| f64::from(i) / 600.0).map(|t| [0.1 + 0.8 * t, 0.5 + 0.3 * (t * 25.0).sin()]).collect();
    let params = serde_json::json!({"points": points, "size": 0.02, "feather": 50.0, "flow": 100.0, "erase": false, "autoMask": false, "connect": false});
    driver.develop(cx, move |d, cx| d.command("mask.brushStroke", params, "Brush stroke", "bench.brush", cx));
    let size = driver.read(cx, |a, cx| a.develop.read(cx).settings.as_ref().map(|s| serde_json::to_string(s).map_or(0, |j| j.len()))).flatten().unwrap_or(0);
    eprintln!("bench {:<28} {:>7.1} KB", "settings with a brushed mask", size as f64 / 1024.0);
    driver.settled(cx, false).await;
}

async fn ui_thread(driver: &Driver, cx: &mut AsyncApp) {
    slider_steps(driver, cx, "UI per slider step").await;
    heal(driver, cx).await;
    long_history(driver, cx).await;
    brush(driver, cx).await;
    slider_steps(driver, cx, "UI per slider step, brushed").await;
    driver.act(cx, |a, cx| a.set_tool(Tool::Masks, cx));
    driver.settled(cx, false).await;
    pointer_moves(driver, cx, "UI per masks move, brushed", across).await;
    driver.act(cx, |a, cx| a.set_tool(Tool::Edit, cx));
}

pub fn start(app: &Entity<FilmlabApp>, window: AnyWindowHandle, cx: &mut App) {
    if std::env::var_os("FILMLAB_BENCH").is_none() {
        return;
    }
    let driver = Driver { app: app.downgrade(), window };
    let sized = std::env::var("FILMLAB_BENCH_WINDOW").ok().and_then(|v| v.split_once('x').and_then(|(w, h)| Some((w.parse::<f32>().ok()?, h.parse::<f32>().ok()?))));
    cx.spawn(async move |cx| {
        if let Some((w, h)) = sized {
            driver.on_window(cx, |window, _| window.resize(size(px(w), px(h))));
        }
        let launch = STARTED.get().map_or(0.0, |t| ms(t.elapsed()));
        eprintln!("bench {:<28} {launch:>7.1} ms", "launch to first task");
        let only_ui = std::env::var("FILMLAB_BENCH").is_ok_and(|v| v == "ui");
        let develop_only = std::env::var("FILMLAB_BENCH").is_ok_and(|v| v == "develop");
        if develop_only {
            until(&driver, cx, 3600, |a, cx| {
                let l = a.library.read(cx);
                !l.photos.is_empty() && l.photos.iter().all(PhotoRecord::indexed)
            })
            .await;
        } else if !only_ui {
            library(&driver, cx, launch).await;
        }
        if std::env::var("FILMLAB_BENCH").is_ok_and(|v| v == "library") {
            cx.update(|cx| cx.quit());
            return;
        }
        let folder = driver.read(cx, |a, cx| a.library.read(cx).folder.clone()).flatten();
        if let Some(folder) = folder {
            let started = Instant::now();
            driver.act(cx, move |a, cx| a.open_folder(folder, cx));
            cx.background_executor().timer(Duration::from_millis(20)).await;
            let again = until(&driver, cx, 300, |a, cx| a.library.read(cx).photos.iter().all(PhotoRecord::indexed)).await;
            eprintln!("bench {:<28} {:>7.1} ms", "library reopened", again.map_or(f64::NAN, |_| ms(started.elapsed())));
        }
        let ids: Vec<String> = driver.read(cx, |a, cx| a.library.read(cx).photos.iter().filter(|r| r.state.is_some()).map(|r| r.file.id.clone()).collect()).unwrap_or_default();
        // why: real photos carry a film look; one without would time a cheaper pipeline than people use
        driver.act(cx, |a, cx| {
            let film = crate::film::FilmSettings {
                negative: Some(crate::stocks::DEFAULT_NEGATIVE.into()),
                print: Some(crate::stocks::DEFAULT_PRINT.into()),
                ..crate::film::FilmSettings::default()
            };
            a.library.update(cx, |l, cx| {
                let bare: Vec<(String, crate::library::PhotoState)> = l.photos.iter().filter_map(|r| r.state.clone().filter(|s| s.film.is_none()).map(|s| (r.file.id.clone(), s))).collect();
                for (id, mut state) in bare {
                    state.film = Some(film.to_value());
                    l.persist(&id, state, cx);
                }
            });
        });
        cx.background_executor().timer(Duration::from_millis(2500)).await;
        let Some(first) = ids.first().cloned() else { return };
        if only_ui {
            let id = first.clone();
            driver.act(cx, move |a, cx| a.open_photo(&id, cx));
            driver.settled(cx, false).await;
            ui_thread(&driver, cx).await;
            cx.update(|cx| cx.quit());
            return;
        }

        let mut open = Vec::new();
        for id in ids.iter().step_by(3).take(3) {
            let target = id.clone();
            let t = Instant::now();
            driver.act(cx, move |a, cx| a.open_photo(&target, cx));
            let id = id.clone();
            if until(&driver, cx, 60, move |a, cx| shows(a, cx, &id)).await.is_some() {
                open.push(ms(t.elapsed()));
            }
            cx.background_executor().timer(Duration::from_millis(400)).await;
        }
        report("open photo (cold)", &mut open);

        let start = first.clone();
        driver.act(cx, move |a, cx| a.open_photo(&start, cx));
        let first_shown = first.clone();
        until(&driver, cx, 60, move |a, cx| shows(a, cx, &first_shown)).await;
        cx.background_executor().timer(Duration::from_millis(800)).await;
        let mut switch = Vec::new();
        let mut switch_shown = Vec::new();
        for i in 0..8 {
            let key = if i < 4 { "right" } else { "left" };
            let before = driver.read(cx, current).flatten();
            to_front(&driver, cx);
            record();
            let t = Instant::now();
            driver.key(cx, key);
            let changed = until(&driver, cx, 5, move |a, cx| current(a, cx) != before).await;
            let revision = driver.read(cx, |a, cx| a.develop.read(cx).planned_revision()).unwrap_or(0);
            let Some(now) = driver.read(cx, current).flatten() else { break };
            if changed.is_some() && until(&driver, cx, 60, move |a, cx| shows(a, cx, &now)).await.is_some() {
                switch.push(ms(t.elapsed()));
            }
            cx.background_executor().timer(Duration::from_millis(50)).await;
            switch_shown.extend(wait_for(&recorded(), t, |s| s.fit.is_some_and(|(r, _)| r >= revision)));
            cx.background_executor().timer(Duration::from_millis(300)).await;
        }
        report("switch photo (arrow, warm)", &mut switch);
        report("switch photo (presented)", &mut switch_shown);

        let Some(photo) = driver.read(cx, current).flatten() else { return };
        let mut edits = Vec::new();
        for i in 0..12 {
            let v = 0.1 * f64::from(i % 6) - 0.2;
            let t = Instant::now();
            driver.develop(cx, |d, cx| d.edit_controls(&[("light.exposure", v)], "Exposure", cx));
            let p = photo.clone();
            if until(&driver, cx, 30, move |a, cx| shows(a, cx, &p)).await.is_some() {
                edits.push(ms(t.elapsed()));
            }
            cx.background_executor().timer(Duration::from_millis(120)).await;
        }
        report("edit exposure (fit)", &mut edits);

        let mut color = Vec::new();
        for i in 0..8 {
            let t = Instant::now();
            driver.develop(cx, |d, cx| d.edit_controls(&[("color.vibrance", f64::from(i) * 5.0)], "Vibrance", cx));
            let p = photo.clone();
            if until(&driver, cx, 30, move |a, cx| shows(a, cx, &p)).await.is_some() {
                color.push(ms(t.elapsed()));
            }
            cx.background_executor().timer(Duration::from_millis(120)).await;
        }
        report("edit vibrance (fit)", &mut color);

        let mut film = Vec::new();
        if let Some(base) = driver.film(cx) {
            for i in 0..6 {
                let mut f = base.clone();
                f.halation.amount = 0.3 + 0.1 * f64::from(i);
                let t = Instant::now();
                driver.develop(cx, move |d, cx| d.edit_film(f, "Halation", "halation.amount", cx));
                let p = photo.clone();
                if until(&driver, cx, 30, move |a, cx| shows(a, cx, &p)).await.is_some() {
                    film.push(ms(t.elapsed()));
                }
                cx.background_executor().timer(Duration::from_millis(120)).await;
            }
        }
        report("edit halation (fit)", &mut film);

        drag_exposure(&driver, cx, &photo, "drag exposure (fit, film)").await;
        edit_latency(&driver, cx, "drag exposure (fit, shown)", false, 60, Duration::from_millis(16)).await;
        let film = driver.film(cx);
        driver.develop(cx, |d, cx| {
            let settings = d.settings.clone()?;
            d.restore(settings, None, "No film", "bench.film", cx).ok()
        });
        drag_exposure(&driver, cx, &photo, "drag exposure (fit, no film)").await;
        driver.develop(cx, move |d, cx| {
            let settings = d.settings.clone()?;
            d.restore(settings, film, "Film", "bench.film", cx).ok()
        });
        let p = photo.clone();
        until(&driver, cx, 30, move |a, cx| shows(a, cx, &p)).await;

        zoom_in(&driver, cx).await;
        pan(&driver, cx, "pan 1:1 (6 px a frame)", 90, 6.0).await;
        pan(&driver, cx, "pan 1:1 (24 px a frame)", 45, 24.0).await;
        pan(&driver, cx, "pan 1:1 (60 px a frame)", 30, 60.0).await;
        edit_latency(&driver, cx, "edit exposure (1:1)", true, 12, Duration::from_millis(250)).await;
        edit_latency(&driver, cx, "drag exposure (1:1)", true, 60, Duration::from_millis(16)).await;

        let others: Vec<String> = ids.iter().filter(|id| **id != photo).take(2).cloned().collect();
        let stop = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let exporting = export_loop(&driver, cx, others, stop.clone());
        cx.background_executor().timer(Duration::from_millis(500)).await;
        edit_latency(&driver, cx, "edit at 1:1 while exporting", true, 12, Duration::from_millis(250)).await;
        pan(&driver, cx, "pan 1:1 while exporting", 45, 12.0).await;
        driver.develop(cx, |d, cx| d.set_zoom(Zoom::Fit, false, false, cx));
        driver.settled(cx, false).await;
        edit_latency(&driver, cx, "edit at fit while exporting", false, 12, Duration::from_millis(250)).await;
        stop.store(true, std::sync::atomic::Ordering::Relaxed);
        if let Some(exporting) = exporting {
            eprintln!("bench {:<28} {:>7} photos exported meanwhile", "exports", exporting.await);
        }

        let mut leave = Vec::new();
        for i in 0..4 {
            let c = 0.35 + 0.1 * f64::from(i);
            driver.develop(cx, move |d, cx| d.set_zoom(Zoom::At { scale: 1.0, cx: c, cy: c }, false, false, cx));
            cx.background_executor().timer(Duration::from_millis(40)).await;
            let t = Instant::now();
            driver.develop(cx, |d, cx| d.set_zoom(Zoom::Fit, false, false, cx));
            leave.push(ms(t.elapsed()));
            until(&driver, cx, 60, |a, cx| a.develop.read(cx).idle_for(Duration::ZERO)).await;
        }
        report("leave 1:1 mid-render (UI)", &mut leave);
        driver.develop(cx, |d, cx| d.set_zoom(Zoom::Fit, false, false, cx));
        driver.settled(cx, false).await;
        if !develop_only {
            ui_thread(&driver, cx).await;
        }
        driver.act(cx, |a, cx| a.set_mode(Mode::Browse, cx));
        cx.background_executor().timer(Duration::from_millis(300)).await;
        cx.update(|cx| cx.quit());
    })
    .detach();
}

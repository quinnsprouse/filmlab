use std::time::{Duration, Instant};

use gpui_kit::*;

use super::app::{FilmlabApp, Mode};
use super::driver::Driver;
use crate::camera::Zoom;
use crate::library::PhotoRecord;

// why: FILMLAB_BENCH=1 times what a user waits for (window, library, opening and switching photos, edits, zoom) on
// the folder last opened, so speed work can be measured before and after; results go to stderr as "bench" lines
static STARTED: std::sync::OnceLock<Instant> = std::sync::OnceLock::new();

pub fn mark_launch() {
    let _ = STARTED.set(Instant::now());
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
        library(&driver, cx, launch).await;
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
        for i in 0..8 {
            let key = if i < 4 { "right" } else { "left" };
            let before = driver.read(cx, current).flatten();
            let t = Instant::now();
            driver.key(cx, key);
            let changed = until(&driver, cx, 5, move |a, cx| current(a, cx) != before).await;
            let Some(now) = driver.read(cx, current).flatten() else { break };
            if changed.is_some() && until(&driver, cx, 60, move |a, cx| shows(a, cx, &now)).await.is_some() {
                switch.push(ms(t.elapsed()));
            }
            cx.background_executor().timer(Duration::from_millis(350)).await;
        }
        report("switch photo (arrow, warm)", &mut switch);

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

        let mut zoom = Vec::new();
        for i in 0..4 {
            let c = 0.3 + 0.12 * f64::from(i);
            let t = Instant::now();
            driver.develop(cx, move |d, cx| d.set_zoom(Zoom::At { scale: 1.0, cx: c, cy: c }, false, false, cx));
            let detail = until(&driver, cx, 60, |a, cx| {
                let d = a.develop.read(cx);
                d.detail.is_some() && d.shown_plan(true).is_some_and(|p| p.revision == d.planned_revision()) && d.idle_for(Duration::from_millis(0))
            })
            .await;
            if detail.is_some() {
                zoom.push(ms(t.elapsed()));
            }
            cx.background_executor().timer(Duration::from_millis(300)).await;
        }
        report("zoom to 1:1 / pan", &mut zoom);

        let native = driver.read(cx, |a, cx| a.develop.read(cx).native).unwrap_or((1.0, 1.0));
        let d = driver.clone();
        let panned = moves(&driver, cx, true, 40, move |i, cx| {
            let c = 0.4 + 6.0 * f64::from(i) / native.0;
            d.develop(cx, move |d, cx| d.set_zoom(Zoom::At { scale: 1.0, cx: c, cy: 0.5 }, false, true, cx));
        })
        .await;
        let settled = until(&driver, cx, 60, |a, cx| {
            let d = a.develop.read(cx);
            let seen = crate::develop::plans::visible_region(d.camera, d.native, d.viewport);
            let covers = |r: &crate::camera::Region| r.u0 <= seen.u0 + 1e-6 && r.v0 <= seen.v0 + 1e-6 && r.u1 >= seen.u1 - 1e-6 && r.v1 >= seen.v1 - 1e-6;
            d.detail.is_some() && d.idle_for(Duration::ZERO) && d.shown_plan(true).is_some_and(|p| p.revision == d.planned_revision() && covers(&p.region))
        })
        .await;
        eprintln!(
            "bench {:<28} {:>7.1} fps over {:.0} ms, worst gap {:.1} ms, settled {:.1} ms after the last move",
            "pan 1:1 (6 px a frame)",
            panned.frames as f64 / panned.took.as_secs_f64(),
            ms(panned.took),
            ms(panned.worst),
            settled.map_or(f64::NAN, ms)
        );

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
        driver.act(cx, |a, cx| a.set_mode(Mode::Browse, cx));
        cx.background_executor().timer(Duration::from_millis(300)).await;
        cx.update(|cx| cx.quit());
    })
    .detach();
}

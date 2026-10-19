use std::time::{Duration, Instant};

use gpui_kit::*;

use super::app::{FilmlabApp, Mode};
use super::driver::Driver;
use crate::camera::Zoom;

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

pub fn start(app: &Entity<FilmlabApp>, window: AnyWindowHandle, cx: &mut App) {
    if std::env::var_os("FILMLAB_BENCH").is_none() {
        return;
    }
    let driver = Driver { app: app.downgrade(), window };
    cx.spawn(async move |cx| {
        let launch = STARTED.get().map_or(0.0, |t| ms(t.elapsed()));
        eprintln!("bench {:<28} {launch:>7.1} ms", "launch to first task");
        let indexed = until(&driver, cx, 300, |a, cx| {
            let l = a.library.read(cx);
            !l.photos.is_empty() && l.photos.iter().all(|r| r.thumbnail.is_some() || r.error.is_some())
        })
        .await;
        let count = driver.read(cx, |a, cx| a.library.read(cx).photos.len()).unwrap_or(0);
        eprintln!("bench {:<28} {:>7.1} ms for {count} photos (from launch {:.1} ms)", "library indexed", indexed.map_or(f64::NAN, ms), launch + indexed.map_or(f64::NAN, ms));
        let folder = driver.read(cx, |a, cx| a.library.read(cx).folder.clone()).flatten();
        if let Some(folder) = folder {
            let started = Instant::now();
            driver.act(cx, move |a, cx| a.open_folder(folder, cx));
            cx.background_executor().timer(Duration::from_millis(20)).await;
            let again = until(&driver, cx, 300, |a, cx| a.library.read(cx).photos.iter().all(|r| r.thumbnail.is_some() || r.error.is_some())).await;
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

        // why: a real slider drag sends an edit every frame; what the hand feels is how many frames come back while
        // it moves, and how soon the full frame lands once it stops
        cx.background_executor().timer(Duration::from_millis(400)).await;
        let frame_of = |a: &FilmlabApp, cx: &App| a.develop.read(cx).fit.as_ref().map(|f| std::sync::Arc::as_ptr(f) as usize);
        let mut last = driver.read(cx, frame_of).flatten();
        let (mut frames, started) = (0usize, Instant::now());
        for i in 0..60 {
            let v = -0.5 + f64::from(i) / 60.0;
            driver.develop(cx, move |d, cx| d.edit_controls(&[("light.exposure", v)], "Exposure", cx));
            let tick = Instant::now();
            while tick.elapsed() < Duration::from_millis(16) {
                let now = driver.read(cx, frame_of).flatten();
                if now != last {
                    frames += 1;
                    last = now;
                }
                cx.background_executor().timer(Duration::from_millis(1)).await;
            }
        }
        let dragged = started.elapsed();
        driver.develop(cx, |d, cx| d.release(cx));
        let p = photo.clone();
        let full = until(&driver, cx, 30, move |a, cx| !a.develop.read(cx).drafting() && shows(a, cx, &p)).await;
        eprintln!("bench {:<28} {:>7.1} fps over {:.0} ms, full frame {:.1} ms after release", "drag exposure (fit)", frames as f64 / dragged.as_secs_f64(), ms(dragged), full.map_or(f64::NAN, ms));

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
        driver.develop(cx, |d, cx| d.set_zoom(Zoom::Fit, false, false, cx));
        driver.act(cx, |a, cx| a.set_mode(Mode::Browse, cx));
        cx.background_executor().timer(Duration::from_millis(300)).await;
        cx.update(|cx| cx.quit());
    })
    .detach();
}

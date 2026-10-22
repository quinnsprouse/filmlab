use std::time::{Duration, Instant};

use gpui_kit::*;

use super::app::{FilmlabApp, Mode};
use super::export::run::{self, Choices};
use crate::camera::Zoom;

// why: a scripted session (FILMLAB_SOAK=1) that walks Browse, Develop edits, zoom, an export and every photo of the
// folder twice over, so memory can be measured from outside without anyone driving the window; FILMLAB_SOAK_PHOTO picks
// the photo it edits, zooms and exports
fn usage() -> Option<libc::rusage_info_v4> {
    let mut info: libc::rusage_info_v4 = unsafe { std::mem::zeroed() };
    let ok = unsafe { libc::proc_pid_rusage(libc::getpid(), libc::RUSAGE_INFO_V4, (&raw mut info).cast()) } == 0;
    ok.then_some(info)
}

pub(crate) fn footprint() -> u64 {
    usage().map_or(0, |u| u.ri_phys_footprint)
}

fn peak() -> u64 {
    usage().map_or(0, |u| u.ri_lifetime_max_phys_footprint)
}

fn mb(bytes: f64) -> f64 {
    bytes / 1_048_576.0
}

/// The footprint now, in MB, after logging it with Metal's share; the session then holds still a moment, so a reading
/// taken from outside (`footprint`, vmmap) sees the same state.
async fn log(cx: &mut AsyncApp, step: &str, started: Instant) -> f64 {
    let total = mb(footprint() as f64);
    eprintln!("soak {:>7.1}s {total:>8.1} MB (metal {:>6.1} MB)  {step}", started.elapsed().as_secs_f64(), mb(crate::gpu::allocated() as f64));
    cx.background_executor().timer(Duration::from_secs(2)).await;
    total
}

async fn wait_until(cx: &mut AsyncApp, app: &WeakEntity<FilmlabApp>, done: impl Fn(&FilmlabApp, &App) -> bool) {
    for _ in 0..600 {
        cx.background_executor().timer(Duration::from_millis(100)).await;
        if app.read_with(cx, |a, cx| done(a, cx)).unwrap_or(true) {
            return;
        }
    }
}

fn settled(a: &FilmlabApp, cx: &App) -> bool {
    let d = a.develop.read(cx);
    d.fit.is_some() && d.idle_for(Duration::from_millis(400))
}

/// Waits for the open photo to settle, then for its work ahead (the neighbours, the full-size decode, thumbnails) to end
/// and the quiet after it that hands scratch memory back.
async fn idle(cx: &mut AsyncApp, app: &WeakEntity<FilmlabApp>) {
    wait_until(cx, app, settled).await;
    quiet(cx).await;
}

async fn quiet(cx: &mut AsyncApp) {
    cx.background_executor().timer(Duration::from_secs(2)).await;
    for _ in 0..600 {
        if crate::perf::quiet_for(Duration::from_secs(3)) {
            break;
        }
        cx.background_executor().timer(Duration::from_millis(100)).await;
    }
    cx.background_executor().timer(Duration::from_secs(1)).await;
}

async fn open(cx: &mut AsyncApp, app: &WeakEntity<FilmlabApp>, id: &str) {
    let _ = app.update(cx, |a, cx| a.open_photo(id, cx));
    wait_until(cx, app, settled).await;
}

async fn export(cx: &mut AsyncApp, app: &WeakEntity<FilmlabApp>, id: &str) {
    let ids = vec![id.to_string()];
    let jobs = app.update(cx, |a, cx| run::jobs_for(a.library.read(cx), a.develop.read(cx), &ids, &Choices::default())).unwrap_or_default();
    let folder = std::env::temp_dir().join("filmlab-soak");
    let exported = cx
        .background_spawn(async move {
            let mut engine = filmlab_engine::core::Engine::new();
            jobs.into_iter()
                .map(|(_, job)| {
                    let job = job?;
                    let decoded = job.loupe().and_then(run::Loupe::decoded);
                    crate::perf::initiated(|| run::export_one(&mut engine, &job, &Choices::default(), Some(&folder), decoded))
                })
                .collect::<Result<Vec<_>, String>>()
        })
        .await;
    if let Err(e) = exported {
        eprintln!("soak export failed: {e}");
    }
}

pub fn start(app: &Entity<FilmlabApp>, cx: &mut App) {
    if std::env::var_os("FILMLAB_SOAK").is_none() {
        return;
    }
    let weak = app.downgrade();
    cx.spawn(async move |cx| {
        let started = Instant::now();
        wait_until(cx, &weak, |a, cx| a.library.read(cx).photos.iter().all(crate::library::PhotoRecord::indexed)).await;
        quiet(cx).await;
        log(cx, "browse, thumbnails ready", started).await;
        let ids: Vec<String> = weak.read_with(cx, |a, cx| a.library.read(cx).photos.iter().map(|r| r.file.id.clone()).collect()).unwrap_or_default();
        let Some(first) = std::env::var("FILMLAB_SOAK_PHOTO").ok().filter(|p| ids.contains(p)).or_else(|| ids.first().cloned()) else { return };
        open(cx, &weak, &first).await;
        idle(cx, &weak).await;
        log(cx, &format!("develop, {first} at fit"), started).await;
        for i in 0..30 {
            let _ = weak.update(cx, |a, cx| a.develop.update(cx, |d, cx| d.edit_controls(&[("light.exposure", 0.02 * f64::from(i % 10))], "Exposure", cx)));
            cx.background_executor().timer(Duration::from_millis(60)).await;
        }
        idle(cx, &weak).await;
        log(cx, "after 30 exposure edits", started).await;
        let _ = weak.update(cx, |a, cx| a.develop.update(cx, |d, cx| d.set_zoom(Zoom::At { scale: 1.0, cx: 0.5, cy: 0.5 }, false, false, cx)));
        wait_until(cx, &weak, |a, cx| settled(a, cx) && a.develop.read(cx).detail.is_some()).await;
        quiet(cx).await;
        log(cx, "zoomed to 1:1", started).await;
        for i in 0..10 {
            let c = 0.3 + 0.04 * f64::from(i);
            let _ = weak.update(cx, |a, cx| a.develop.update(cx, |d, cx| d.set_zoom(Zoom::At { scale: 1.0, cx: c, cy: c }, false, false, cx)));
            wait_until(cx, &weak, settled).await;
        }
        quiet(cx).await;
        log(cx, "after 10 pans", started).await;
        let _ = weak.update(cx, |a, cx| a.develop.update(cx, |d, cx| d.set_zoom(Zoom::Fit, false, false, cx)));
        idle(cx, &weak).await;
        log(cx, "back to fit", started).await;
        export(cx, &weak, &first).await;
        idle(cx, &weak).await;
        log(cx, "after a full-size export", started).await;
        for id in ids.iter().take(20) {
            open(cx, &weak, id).await;
        }
        idle(cx, &weak).await;
        log(cx, "after opening 20 photos", started).await;
        let mut passes = Vec::new();
        for pass in 1..=2 {
            for id in &ids {
                open(cx, &weak, id).await;
            }
            idle(cx, &weak).await;
            passes.push(log(cx, &format!("after opening all {} photos, pass {pass}", ids.len()), started).await);
        }
        if let [one, two] = passes[..] {
            eprintln!("soak growth from pass 1 to pass 2: {:+.1} MB", two - one);
        }
        let _ = weak.update(cx, |a, cx| a.set_mode(Mode::Browse, cx));
        quiet(cx).await;
        log(cx, "back in browse", started).await;
        cx.background_executor().timer(Duration::from_secs(6)).await;
        quiet(cx).await;
        log(cx, "Develop unloaded", started).await;
        eprintln!("soak peak footprint {:.1} MB", mb(peak() as f64));
        if std::env::var_os("FILMLAB_SOAK_STATS").is_some() {
            unsafe { libmimalloc_sys::mi_stats_print(std::ptr::null_mut()) };
        }
        if std::env::var_os("FILMLAB_SOAK_HOLD").is_some() {
            cx.background_executor().timer(Duration::from_secs(30)).await;
        }
        cx.update(|cx| cx.quit());
    })
    .detach();
}

use std::time::{Duration, Instant};

use gpui_kit::*;

use super::app::{FilmlabApp, Mode};
use crate::camera::Zoom;

// why: a scripted session (FILMLAB_SOAK=1) that walks Browse, Develop edits, zoom and photo changes, so memory can be
// measured from outside without anyone driving the window
fn footprint() -> u64 {
    let mut info: libc::rusage_info_v4 = unsafe { std::mem::zeroed() };
    let ok = unsafe { libc::proc_pid_rusage(libc::getpid(), libc::RUSAGE_INFO_V4, (&raw mut info).cast()) } == 0;
    if ok { info.ri_phys_footprint } else { 0 }
}

fn heap() -> usize {
    let (mut a, mut b, mut c, mut rss, mut peak, mut commit, mut d, mut e) = (0, 0, 0, 0, 0, 0, 0, 0);
    unsafe { libmimalloc_sys::mi_process_info(&mut a, &mut b, &mut c, &mut rss, &mut peak, &mut commit, &mut d, &mut e) };
    commit
}

fn log(step: &str, started: Instant) {
    eprintln!("soak {:>7.1}s {:>8.1} MB (heap {:>7.1} MB)  {step}", started.elapsed().as_secs_f64(), footprint() as f64 / 1_048_576.0, heap() as f64 / 1_048_576.0);
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

pub fn start(app: &Entity<FilmlabApp>, cx: &mut App) {
    if std::env::var_os("FILMLAB_SOAK").is_none() {
        return;
    }
    let weak = app.downgrade();
    cx.spawn(async move |cx| {
        let started = Instant::now();
        wait_until(cx, &weak, |a, cx| a.library.read(cx).photos.iter().all(|r| r.thumbnail.is_some() || r.error.is_some())).await;
        log("browse, thumbnails ready", started);
        let ids: Vec<String> = weak.read_with(cx, |a, cx| a.library.read(cx).photos.iter().map(|r| r.file.id.clone()).collect()).unwrap_or_default();
        let Some(first) = ids.first().cloned() else { return };
        let _ = weak.update(cx, |a, cx| a.open_photo(&first, cx));
        wait_until(cx, &weak, settled).await;
        log("develop, first photo", started);
        for i in 0..30 {
            let _ = weak.update(cx, |a, cx| a.develop.update(cx, |d, cx| d.edit_controls(&[("light.exposure", 0.02 * f64::from(i % 10))], "Exposure", cx)));
            cx.background_executor().timer(Duration::from_millis(60)).await;
        }
        wait_until(cx, &weak, settled).await;
        log("after 30 exposure edits", started);
        let _ = weak.update(cx, |a, cx| a.develop.update(cx, |d, cx| d.set_zoom(Zoom::At { scale: 1.0, cx: 0.5, cy: 0.5 }, false, false, cx)));
        wait_until(cx, &weak, |a, cx| settled(a, cx) && a.develop.read(cx).detail.is_some()).await;
        log("zoomed to 1:1", started);
        for i in 0..10 {
            let c = 0.3 + 0.04 * f64::from(i);
            let _ = weak.update(cx, |a, cx| a.develop.update(cx, |d, cx| d.set_zoom(Zoom::At { scale: 1.0, cx: c, cy: c }, false, false, cx)));
            wait_until(cx, &weak, settled).await;
        }
        log("after 10 pans", started);
        let _ = weak.update(cx, |a, cx| a.develop.update(cx, |d, cx| d.set_zoom(Zoom::Fit, false, false, cx)));
        wait_until(cx, &weak, settled).await;
        for id in ids.iter().skip(1).take(8) {
            let _ = weak.update(cx, |a, cx| a.open_photo(id, cx));
            wait_until(cx, &weak, settled).await;
            log(&format!("opened {id}"), started);
        }
        let _ = weak.update(cx, |a, cx| a.set_mode(Mode::Browse, cx));
        cx.background_executor().timer(Duration::from_secs(3)).await;
        log("back in browse, idle 3 s", started);
        cx.background_executor().timer(Duration::from_secs(5)).await;
        log("idle 8 s (Develop unloaded)", started);
        if std::env::var_os("FILMLAB_SOAK_STATS").is_some() {
            unsafe { libmimalloc_sys::mi_stats_print(std::ptr::null_mut()) };
        }
        if std::env::var_os("FILMLAB_SOAK_HOLD").is_some() {
            cx.background_executor().timer(Duration::from_secs(30)).await;
        }
        let _ = cx.update(|cx| cx.quit());
    })
    .detach();
}

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use filmlab_engine::core::{self, Engine};
use gpui_kit::*;
use serde_json::{Value, json};

use super::app::{FilmlabApp, INSPECTOR, MARGIN, Mode, TOP};
use super::curve::{self, Tab};
use super::driver::Driver;
use super::inspector::Ui;
use super::tools::crop;
use super::wheels::GradingWheels;
use crate::camera::Zoom;
use crate::develop::plans::Plan;
use crate::develop::{Compare, DisplayFrame, Tool};
use crate::film::{FilmSettings, Gauge};
use crate::library::{Saving, ensure_luts};
use crate::settings::Settings;
use crate::stocks::{DEFAULT_NEGATIVE, DEFAULT_PRINT, StockKind};

// why: a scripted session (FILMLAB_E2E=1) that drives the real window through its key bindings, injected mouse
// events and the app's own entry points, checks every frame against an independent engine render, and writes window
// screenshots to FILMLAB_E2E_SHOTS; injected events go to this window only, never to the system's cursor or keyboard
struct Run {
    driver: Driver,
    shots: Option<PathBuf>,
    number: Option<isize>,
    started: Instant,
    passed: usize,
    failed: Vec<String>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct Look {
    hash: u64,
    mean: f64,
    spread: f64,
    flat_rows: f64,
    size: (u32, u32),
}

fn look(frame: &DisplayFrame) -> Look {
    look_of(frame.width, frame.height, &frame.rgba)
}

fn look_of(width: u32, height: u32, rgba: &[u8]) -> Look {
    let hash = rgba.iter().fold(0xcbf2_9ce4_8422_2325u64, |h, b| (h ^ u64::from(*b)).wrapping_mul(0x100_0000_01b3));
    let luma: Vec<f64> = rgba.as_chunks::<4>().0.iter().map(|p| (0.2126 * f64::from(p[0]) + 0.7152 * f64::from(p[1]) + 0.0722 * f64::from(p[2])) / 255.0).collect();
    let n = luma.len().max(1) as f64;
    let mean = luma.iter().sum::<f64>() / n;
    let spread = (luma.iter().map(|v| (v - mean).powi(2)).sum::<f64>() / n).sqrt();
    let rows = rgba.chunks_exact(width as usize * 4);
    let total = rows.len().max(1) as f64;
    let flat = rgba.chunks_exact(width as usize * 4).filter(|row| row.as_chunks::<4>().0.iter().all(|p| p == &row[..4])).count() as f64;
    Look { hash, mean, spread, flat_rows: flat / total, size: (width, height) }
}

fn healthy(l: &Look) -> bool {
    (0.02..0.98).contains(&l.mean) && l.spread > 0.02 && l.flat_rows < 0.01
}

// why: the same render the loupe asked for, on a fresh engine with its own decode, so a cache, a stale plan or a
// display bug shows up as a byte difference
fn reference(path: &Path, facts: &Value, plan: &Plan) -> Result<Vec<u8>, String> {
    let bytes = std::fs::read(path).map_err(|e| e.to_string())?;
    let mut engine = Engine::new();
    engine.insert("reference", core::decode(&facts.to_string(), &bytes, plan.level.max_edge())?);
    ensure_luts(&mut engine, plan.film.as_ref())?;
    let mut view = plan.view.clone();
    if let Some(o) = &plan.overlay {
        view["overlay"] = o.clone();
    }
    if let Some(f) = &plan.film {
        view["film"] = f.to_value();
    }
    let mut frame = engine.render("reference", &plan.settings.json(), &view.to_string())?;
    Ok(frame.take_rgba())
}

fn sidecar(folder: &Path, stem: &str, facts: &Value) -> Option<Value> {
    let xmp = std::fs::read_to_string(folder.join(format!("{stem}.xmp"))).ok()?;
    serde_json::from_str(&core::read_sidecar(&facts.to_string(), &xmp).ok()?).ok()
}

fn exposure_of(settings: &Value) -> f64 {
    core::control_values(&settings.to_string()).ok().and_then(|j| serde_json::from_str::<Value>(&j).ok()).and_then(|v| v["light.exposure"].as_f64()).unwrap_or(f64::NAN)
}

impl std::ops::Deref for Run {
    type Target = Driver;

    fn deref(&self) -> &Driver {
        &self.driver
    }
}

impl Run {
    fn check(&mut self, name: &str, ok: bool, detail: impl std::fmt::Display) {
        let at = self.started.elapsed().as_secs_f64();
        if ok {
            self.passed += 1;
            eprintln!("e2e {at:6.1}s PASS {name}: {detail}");
        } else {
            self.failed.push(name.to_string());
            eprintln!("e2e {at:6.1}s FAIL {name}: {detail}");
        }
    }

    fn shown(&self, cx: &mut AsyncApp, detail: bool) -> Option<(Look, Plan, PathBuf, Value)> {
        self.read(cx, |a, cx| {
            let d = a.develop.read(cx);
            let frame = if detail { d.detail.clone()? } else { d.fit.clone()? };
            let plan = d.shown_plan(detail)?.clone();
            let record = a.library.read(cx).record(&plan.photo_id)?;
            Some((look(&frame), plan, record.file.path.clone(), record.probed.as_ref()?.facts.clone()))
        })
        .flatten()
    }

    /// Checks the frame on screen: sane pixels and byte-identical to an independent render of its plan.
    async fn verify(&mut self, cx: &mut AsyncApp, name: &str, detail: bool) -> Option<Look> {
        if !self.settled(cx, detail).await {
            self.check(name, false, "no settled frame within 60 s");
            return None;
        }
        let Some((shown, plan, path, facts)) = self.shown(cx, detail) else {
            self.check(name, false, "no frame or plan on screen");
            return None;
        };
        let rgba = self.read(cx, |a, cx| {
            let d = a.develop.read(cx);
            if detail { d.detail.clone() } else { d.fit.clone() }
        });
        let expected = cx.background_spawn(async move { reference(&path, &facts, &plan) }).await;
        let same = match (&expected, rgba.flatten()) {
            (Ok(bytes), Some(frame)) => *bytes == frame.rgba,
            _ => false,
        };
        let detail_text = format!("{}x{} mean {:.3} spread {:.3} flat rows {:.1}% reference {}", shown.size.0, shown.size.1, shown.mean, shown.spread, shown.flat_rows * 100.0, match &expected {
            Ok(_) if same => "identical".to_string(),
            Ok(_) => "DIFFERS".to_string(),
            Err(e) => format!("failed: {e}"),
        });
        self.check(name, healthy(&shown) && same, detail_text);
        Some(shown)
    }

    fn shot(&self, name: &str) {
        let (Some(dir), Some(number)) = (&self.shots, self.number) else { return };
        let path = dir.join(format!("{name}.png"));
        let _ = std::process::Command::new("screencapture").args(["-x", "-o", &format!("-l{number}")]).arg(&path).status();
    }

    async fn shoot(&self, cx: &mut AsyncApp, name: &str) {
        self.pause(cx, 400).await;
        self.shot(name);
    }

    async fn shoot_inspector(&self, cx: &mut AsyncApp, name: &str) {
        self.shoot(cx, name).await;
        let Some(shots) = &self.shots else { return };
        let Some((width, height)) = self.on_window(cx, |window, _| {
            let (size, scale) = (window.viewport_size(), window.scale_factor());
            ((f32::from(size.width) * scale).round() as u32, (f32::from(size.height) * scale).round() as u32)
        }) else {
            return;
        };
        let crops = shots.with_file_name("crops");
        let _ = std::fs::create_dir_all(&crops);
        let wide = 640.min(width);
        let _ = std::process::Command::new("sips")
            .args(["-c", &height.to_string(), &wide.to_string(), "--cropOffset", "0", &(width - wide).to_string()])
            .arg(shots.join(format!("{name}.png")))
            .arg("--out")
            .arg(crops.join(format!("{name}.png")))
            .stdout(std::process::Stdio::null())
            .status();
    }

}

pub fn start(app: &Entity<FilmlabApp>, window: AnyWindowHandle, cx: &mut App) {
    if std::env::var_os("FILMLAB_E2E").is_none() {
        return;
    }
    let shots = std::env::var_os("FILMLAB_E2E_SHOTS").map(PathBuf::from);
    if let Some(dir) = &shots {
        let _ = std::fs::create_dir_all(dir);
    }
    let number = window
        .update(cx, |_, window, _| crate::glass::renderer_view(window).and_then(|v| v.window()).map(|w| w.windowNumber()))
        .ok()
        .flatten();
    let run = Run { driver: Driver { app: app.downgrade(), window }, shots, number, started: Instant::now(), passed: 0, failed: Vec::new() };
    let parity = std::env::var("FILMLAB_E2E").is_ok_and(|v| v == "parity");
    cx.spawn(async move |cx| {
        let mut run = run;
        let mut quit_by_key = false;
        if parity {
            reopened(&mut run, cx).await;
            for (name, ok, detail) in cx.background_spawn(async { super::parity::compare() }).await {
                run.check(&name, ok, detail);
            }
        } else {
            quit_by_key = script(&mut run, cx).await;
        }
        eprintln!("e2e done: {} passed, {} failed {:?}", run.passed, run.failed.len(), run.failed);
        if quit_by_key {
            run.key(cx, "cmd-q");
            run.pause(cx, 5000).await;
            run.check("quit: Cmd-Q quits the app", false, "still running 5 s after Cmd-Q");
        }
        cx.update(|cx| cx.quit());
    })
    .detach();
}

/// Runs every check; true when the run should end the way Cmd-Q does, leaving an edit for the relaunch to find.
async fn script(run: &mut Run, cx: &mut AsyncApp) -> bool {
    library(run, cx).await;
    let Some((id, folder, stem, facts)) = open_first(run, cx).await else { return false };
    let photo = run.read(cx, |a, cx| a.library.read(cx).record(&id).map(|r| r.file.path.clone())).flatten().unwrap_or_default();
    for (name, ok, detail) in cx.background_spawn(async move { super::parity::record(&photo) }).await {
        run.check(&name, ok, detail);
    }
    edits(run, cx, &folder, &stem, &facts).await;
    film(run, cx).await;
    color_controls(run, cx).await;
    zoom(run, cx).await;
    tools(run, cx).await;
    compare_and_profiles(run, cx).await;
    navigation(run, cx, &id).await;
    export(run, cx).await;
    leave(run, cx, &folder, &stem, &facts).await;
    torn_writes(run, cx, &folder, &stem, &facts).await;
    edit_before_quit(run, cx, &id, &stem).await
}

const QUIT_EXPOSURE: f64 = 0.42;

fn quit_note() -> Option<PathBuf> {
    std::env::var_os("FILMLAB_E2E_QUIT").map(PathBuf::from)
}

/// Opens the photo and edits it, so the edit is still waiting on its save delay when the run quits.
async fn edit_before_quit(run: &mut Run, cx: &mut AsyncApp, id: &str, stem: &str) -> bool {
    let Some(note) = quit_note() else { return false };
    let open = id.to_string();
    run.act(cx, move |a, cx| a.open_photo(&open, cx));
    run.wait(cx, 30, |a, cx| a.develop.read(cx).status == crate::develop::Status::Ready).await;
    run.develop(cx, |d, cx| d.edit_controls(&[("light.exposure", QUIT_EXPOSURE)], "Exposure", cx));
    let waiting = run.read(cx, |a, cx| a.develop.read(cx).unsaved()).unwrap_or(false);
    run.check("quit: the edit is still waiting to be saved when Cmd-Q comes", waiting, "");
    let _ = std::fs::write(note, json!({"stem": stem}).to_string());
    waiting
}

/// After a relaunch, the edit the last run made just before Cmd-Q is in the photo's sidecar and in the library.
async fn reopened(run: &mut Run, cx: &mut AsyncApp) {
    let Some(stem) = quit_note().and_then(|n| std::fs::read_to_string(n).ok()).and_then(|t| serde_json::from_str::<Value>(&t).ok()).and_then(|v| v["stem"].as_str().map(str::to_string)) else {
        return run.check("quit: the run before left an edit to find", false, "no note from the run before");
    };
    let found = run.wait(cx, 60, {
        let stem = stem.clone();
        move |a, cx| a.library.read(cx).photos.iter().find(|r| r.file.stem == stem).is_some_and(|r| r.probed.is_some() && r.state.is_some())
    })
    .await;
    let (state, on_disk) = run
        .read(cx, |a, cx| {
            let l = a.library.read(cx);
            let r = l.photos.iter().find(|r| r.file.stem == stem)?;
            let facts = r.probed.as_ref()?.facts.clone();
            let xmp = std::fs::read_to_string(r.file.sidecar.as_ref()?).ok()?;
            let on_disk: Value = serde_json::from_str(&core::read_sidecar(&facts.to_string(), &xmp).ok()?).ok()?;
            Some((r.state.as_ref()?.settings.to_value(), on_disk["settings"].clone()))
        })
        .flatten()
        .unwrap_or_default();
    let (loaded, saved) = (exposure_of(&state), exposure_of(&on_disk));
    let kept = found && (loaded - QUIT_EXPOSURE).abs() < 1e-6 && (saved - QUIT_EXPOSURE).abs() < 1e-6;
    run.check("quit: an edit made just before Cmd-Q is in the sidecar after relaunch", kept, format!("library {loaded}, sidecar {saved}"));
}

/// Reads `path` as a sidecar until `stop`: how many reads, and how many found anything but a whole sidecar.
fn read_while(path: PathBuf, facts: String, stop: Arc<AtomicBool>) -> (usize, usize) {
    let (mut reads, mut torn) = (0, 0);
    while !stop.load(Ordering::Relaxed) {
        let text = std::fs::read_to_string(&path).unwrap_or_default();
        reads += 1;
        if !text.trim_end().ends_with("<?xpacket end=\"w\"?>") || core::read_sidecar(&facts, &text).is_err() {
            torn += 1;
        }
    }
    (reads, torn)
}

async fn torn_writes(run: &mut Run, cx: &mut AsyncApp, folder: &Path, stem: &str, facts: &Value) {
    let found = run.read(cx, |a, cx| a.library.read(cx).photos.iter().find(|r| r.file.stem == stem).and_then(|r| Some((r.file.id.clone(), r.file.sidecar.clone()?, r.state.as_ref()?.rating)))).flatten();
    let Some((id, path, rating)) = found else { return run.check("sidecars: the edited photo has a sidecar to rewrite", false, "") };
    let stop = Arc::new(AtomicBool::new(false));
    let reader = std::thread::spawn({
        let (stop, facts) = (stop.clone(), facts.to_string());
        move || read_while(path, facts, stop)
    });
    for i in 0..40u8 {
        let ids = vec![id.clone()];
        run.act(cx, move |a, cx| a.library.update(cx, |l, cx| l.set_rating(&ids, i % 6, cx)));
        run.pause(cx, 15).await;
    }
    let ids = vec![id.clone()];
    run.act(cx, move |a, cx| a.library.update(cx, |l, cx| l.set_rating(&ids, rating, cx)));
    let saved = run.wait(cx, 10, |a, cx| a.library.read(cx).saving != Saving::Saving).await;
    stop.store(true, Ordering::Relaxed);
    let (reads, torn) = reader.join().unwrap_or((0, usize::MAX));
    run.check("sidecars: a sidecar read while it is rewritten is always whole", saved && reads > 0 && torn == 0, format!("{torn} of {reads} reads torn"));
    let left: Vec<String> = std::fs::read_dir(folder).map(|d| d.flatten().map(|e| e.file_name().to_string_lossy().to_string()).filter(|n| crate::files::is_temporary(n)).collect()).unwrap_or_default();
    run.check("sidecars: rewriting leaves no temporary file behind", left.is_empty(), format!("{left:?}"));
}

fn rgba_of(image: &RenderImage) -> Option<Vec<u8>> {
    Some(image.as_bytes(0)?.as_chunks::<4>().0.iter().flat_map(|p| [p[2], p[1], p[0], p[3]]).collect())
}

async fn thumbnails(run: &mut Run, cx: &mut AsyncApp) {
    let previews = run.read(cx, |a, cx| a.library.read(cx).previews_shown()).unwrap_or(0);
    run.check("library: camera previews stand in while film thumbnails render", previews > 0, format!("{previews} previews shown"));
    let filmed = run
        .wait(cx, 10, |a, cx| {
            let l = a.library.read(cx);
            a.browse.order().iter().all(|id| l.record(id).is_some_and(|r| r.film.is_some() && l.picture(id).is_some_and(|p| p.film == r.film)))
        })
        .await;
    run.check("library: every tile's preview is replaced by its film thumbnail", filmed, "");
    let sample = run
        .read(cx, |a, cx| {
            let l = a.library.read(cx);
            let id = a.browse.order().first()?;
            let r = l.record(id)?;
            Some((r.file.path.clone(), r.probed.clone()?, r.state.clone()?, l.picture(id)?.image.clone()))
        })
        .flatten();
    let Some((path, probed, state, image)) = sample else {
        run.check("library: the film thumbnail is identical to a fresh render", false, "no thumbnail on the first tile");
        return;
    };
    let expected = cx.background_spawn(async move { crate::library::fresh_thumbnail(&path, &probed, &state) }).await;
    let same = matches!((&expected, rgba_of(&image)), (Ok(e), Some(s)) if *e == s);
    run.check("library: the film thumbnail is identical to a fresh render", same, match &expected {
        Ok(_) if same => "identical".to_string(),
        Ok(_) => "DIFFERS".to_string(),
        Err(e) => format!("failed: {e}"),
    });
}

async fn library(run: &mut Run, cx: &mut AsyncApp) {
    let loaded = run.wait(cx, 120, |a, cx| {
        let l = a.library.read(cx);
        !l.photos.is_empty() && l.photos.iter().all(crate::library::PhotoRecord::indexed)
    });
    let ready = loaded.await;
    let (count, errors) = run.read(cx, |a, cx| {
        let l = a.library.read(cx);
        (l.photos.len(), l.photos.iter().filter_map(|r| r.error.clone().map(|e| format!("{}: {e}", r.file.name))).collect::<Vec<_>>())
    }).unwrap_or_default();
    run.check("library: every photo indexed with a thumbnail", ready && errors.is_empty() && count > 1, format!("{count} photos, errors {errors:?}"));
    thumbnails(run, cx).await;
    run.shoot(cx, "01-library").await;

    let order = run.read(cx, |a, _| a.browse.order().to_vec()).unwrap_or_default();
    let (Some(first), Some(second)) = (order.first().cloned(), order.get(1).cloned()) else { return };
    let tile = run.read(cx, |a, _| a.browse.tile(&second)).flatten();
    if let Some(b) = tile {
        run.click(cx, b.center(), 1);
    }
    run.pause(cx, 200).await;
    let selected = run.read(cx, |a, _| a.browse.selection.clone()).unwrap_or_default();
    run.check("library: clicking a tile selects it", selected == vec![second.clone()], format!("selection {selected:?}"));

    run.key(cx, "p");
    let picked = run.wait(cx, 5, |a, cx| a.library.read(cx).record(a.browse.selection.first().map_or("", |s| s.as_str())).and_then(|r| r.state.as_ref()).is_some_and(|s| s.flag == "pick")).await;
    run.check("library: P flags the selection as a pick", picked, "flag pick");
    run.act(cx, |a, cx| {
        a.browse.filter = super::browse::Filter::Picks;
        cx.notify();
    });
    run.pause(cx, 300).await;
    let shown = run.read(cx, |a, _| a.browse.order().to_vec()).unwrap_or_default();
    run.check("library: the Picks filter shows only the pick", shown == vec![second.clone()], format!("{shown:?}"));
    run.shoot(cx, "02-library-picks").await;
    run.act(cx, |a, cx| {
        a.browse.filter = super::browse::Filter::All;
        cx.notify();
    });
    run.pause(cx, 300).await;
    run.key(cx, "x");
    let rejected = run.wait(cx, 5, move |a, cx| a.library.read(cx).record(&second).and_then(|r| r.state.as_ref()).is_some_and(|s| s.flag == "reject")).await;
    run.check("library: X rejects it", rejected, "flag reject");
    run.key(cx, "u");
    run.pause(cx, 300).await;

    run.key(cx, "cmd-a");
    run.pause(cx, 200).await;
    let all = run.read(cx, |a, _| a.browse.selection.len()).unwrap_or(0);
    run.check("library: Cmd-A selects every photo", all == order.len(), format!("{all} of {}", order.len()));
    run.key(cx, "escape");
    run.pause(cx, 200).await;
    let cleared = run.read(cx, |a, _| a.browse.selection.is_empty()).unwrap_or(false);
    run.check("library: Escape clears the selection", cleared, "");

    if let Some(b) = run.read(cx, |a, _| a.browse.tile(&first)).flatten() {
        run.click(cx, b.center(), 2);
    }
}

async fn open_first(run: &mut Run, cx: &mut AsyncApp) -> Option<(String, PathBuf, String, Value)> {
    let opened = run.wait(cx, 10, |a, _| a.mode == Mode::Develop).await;
    run.check("library: double-clicking a tile opens it in Develop", opened, "");
    if !opened {
        let first = run.read(cx, |a, _| a.browse.order().first().cloned()).flatten()?;
        run.act(cx, |a, cx| a.open_photo(&first, cx));
    }
    run.verify(cx, "develop: the fit frame matches an independent render", false).await?;
    run.shoot(cx, "03-develop").await;
    run.read(cx, |a, cx| {
        let d = a.develop.read(cx);
        let id = d.photo_id.clone()?;
        let record = a.library.read(cx).record(&id)?;
        Some((id, record.file.path.parent()?.to_path_buf(), record.file.stem.clone(), record.probed.as_ref()?.facts.clone()))
    })
    .flatten()
}

async fn edits(run: &mut Run, cx: &mut AsyncApp, folder: &Path, stem: &str, facts: &Value) {
    let Some((before, ..)) = run.shown(cx, false) else { return };
    run.develop(cx, |d, cx| d.edit_controls(&[("light.exposure", 0.7)], "Exposure", cx));
    let Some(brighter) = run.verify(cx, "edit: exposure +0.7 renders and matches", false).await else { return };
    run.check("edit: exposure brightens the frame", brighter.mean > before.mean + 0.02, format!("mean {:.3} → {:.3}", before.mean, brighter.mean));

    let saved = run.wait(cx, 10, {
        let (folder, stem, facts) = (folder.to_path_buf(), stem.to_string(), facts.clone());
        move |_, _| sidecar(&folder, &stem, &facts).is_some_and(|s| (exposure_of(&s["settings"]) - 0.7).abs() < 1e-6)
    })
    .await;
    run.check("edit: the XMP sidecar records the exposure", saved, format!("{}/{stem}.xmp", folder.display()));

    run.key(cx, "cmd-z");
    let undone = run.verify(cx, "edit: Cmd-Z renders and matches", false).await;
    run.check("edit: undo restores the exact frame", undone.is_some_and(|l| l.hash == before.hash), "same bytes as before the edit");
    run.key(cx, "cmd-shift-z");
    let redone = run.verify(cx, "edit: Cmd-Shift-Z renders and matches", false).await;
    run.check("edit: redo restores the edited frame", redone.is_some_and(|l| l.hash == brighter.hash), "same bytes as the edit");

    let history = run.read(cx, |a, cx| a.develop.read(cx).history.len()).unwrap_or(0);
    run.develop(cx, |d, cx| d.go_to(0, cx));
    let first = run.verify(cx, "edit: jumping to the first history step matches", false).await;
    run.check("edit: history step 0 is the opened photo", first.is_some_and(|l| l.hash == before.hash), format!("{history} steps"));
    run.develop(cx, |d, cx| d.go_to(history.saturating_sub(1), cx));
    run.verify(cx, "edit: back to the latest history step matches", false).await;

    // why: an agent's change stays marked in the inspector until you change that control yourself
    run.develop(cx, |d, cx| d.command("develop.set", json!({"values": {"light.shadows": 30}}), "Open shadows", "agent.e2e.0", cx));
    let marked = run.read(cx, |a, cx| a.develop.read(cx).agent_marks.contains_key("light.shadows")).unwrap_or(false);
    run.develop(cx, |d, cx| d.edit_controls(&[("light.shadows", 35.0)], "Shadows", cx));
    let cleared = run.read(cx, |a, cx| !a.develop.read(cx).agent_marks.contains_key("light.shadows")).unwrap_or(false);
    run.develop(cx, |d, cx| d.undo(cx));
    let back = run.read(cx, |a, cx| a.develop.read(cx).agent_marks.contains_key("light.shadows")).unwrap_or(false);
    run.check("edit: an agent's change is marked until you take it over", marked && cleared && back, format!("marked {marked}, cleared {cleared}, marked again after undo {back}"));
    run.develop(cx, |d, cx| d.undo(cx));
    run.verify(cx, "edit: undoing the agent's change matches", false).await;

    let step = run.read(cx, |a, cx| a.develop.read(cx).cursor).unwrap_or(0);
    let mut drafted = false;
    for i in 0..30 {
        let v = 0.4 + f64::from(i) / 100.0;
        run.develop(cx, move |d, cx| d.edit_controls(&[("light.exposure", v)], "Exposure", cx));
        run.pause(cx, 16).await;
        drafted |= run.read(cx, |a, cx| a.develop.read(cx).shown_plan(false).is_some_and(crate::develop::plans::is_draft)).unwrap_or(false);
    }
    run.develop(cx, |d, cx| d.release(cx));
    run.verify(cx, "edit: a slider drag settles to a frame that matches an independent render", false).await;
    let added = run.read(cx, |a, cx| a.develop.read(cx).cursor).unwrap_or(0).saturating_sub(step);
    run.check("edit: a slider drag draws drafts while it moves and is one undo step", drafted && added == 1, format!("drafts shown {drafted}, {added} steps"));
    run.develop(cx, |d, cx| d.undo(cx));
    run.verify(cx, "edit: undoing the drag matches", false).await;
}

async fn film(run: &mut Run, cx: &mut AsyncApp) {
    let film = match run.film(cx).filter(FilmSettings::is_on) {
        Some(film) => film,
        None => {
            let Some((plain, ..)) = run.shown(cx, false) else { return };
            let film = FilmSettings { negative: Some(DEFAULT_NEGATIVE.into()), print: Some(DEFAULT_PRINT.into()), ..FilmSettings::default() };
            run.develop(cx, |d, cx| d.edit_film(film.clone(), "Film", "film.stock", cx));
            let filmed = run.verify(cx, "film: choosing FilmVision stocks renders and matches", false).await;
            run.check("film: the stocks change the frame", filmed.is_some_and(|l| l.hash != plain.hash), format!("{DEFAULT_NEGATIVE} / {DEFAULT_PRINT}"));
            film
        }
    };
    let Some((base, ..)) = run.shown(cx, false) else { return };

    let mut off = film.clone();
    off.halation.on = Some(false);
    run.develop(cx, |d, cx| d.edit_film(off, "Halation off", "halation.on", cx));
    let changed = run.verify(cx, "film: halation off renders and matches", false).await;
    run.check("film: halation changes the frame", changed.is_some_and(|l| l.hash != base.hash), "");
    run.key(cx, "cmd-z");
    let back = run.verify(cx, "film: undoing halation off matches", false).await;
    run.check("film: undo brings halation back exactly", back.is_some_and(|l| l.hash == base.hash), "");

    let stocks = run.read(cx, |a, _| a.stocks.iter().filter(|s| s.kind == StockKind::Negative).map(|s| s.id.clone()).collect::<Vec<_>>()).unwrap_or_default();
    if let Some(other) = stocks.iter().find(|s| Some(*s) != film.negative.as_ref()) {
        let mut next = film.clone();
        next.negative = Some(other.clone());
        run.develop(cx, |d, cx| d.edit_film(next, "Negative", "film.negative", cx));
        let swapped = run.verify(cx, "film: another negative renders and matches", false).await;
        run.check("film: the negative changes the frame", swapped.is_some_and(|l| l.hash != base.hash), other.clone());
    }

    let mut gauge = run.film(cx).unwrap_or(film.clone());
    gauge.grain = Gauge::G16.grain();
    gauge.blur = Gauge::G16.blur();
    gauge.halation.radius = Gauge::G16.halation_radius();
    run.develop(cx, |d, cx| d.edit_film(gauge.clone(), "Gauge 16", "film.gauge", cx));
    run.verify(cx, "film: 16 mm gauge renders and matches", false).await;
    let mut dusty = gauge;
    if let Some(d) = dusty.dust.as_mut() {
        d.on = false;
    }
    run.develop(cx, |d, cx| d.edit_film(dusty, "Dust", "film.dust", cx));
    run.verify(cx, "film: dust setting renders and matches", false).await;
    run.shoot(cx, "04-film").await;
    run.develop(cx, |d, cx| d.edit_film(film, "Film", "film.reset", cx));
    let restored = run.verify(cx, "film: the original look renders and matches", false).await;
    run.check("film: setting the look back restores the exact frame", restored.is_some_and(|l| l.hash == base.hash), "");
}

async fn show_section(run: &Run, cx: &mut AsyncApp, section: &'static str) {
    run.act(cx, |a, cx| {
        a.ui.open.iter_mut().for_each(|(id, open)| *open = *id == section);
        cx.notify();
    });
    run.pause(cx, 200).await;
    run.act(cx, |a, cx| {
        a.ui.reveal(section);
        cx.notify();
    });
    run.pause(cx, 300).await;
}

fn values_of(ids: [&'static str; 2]) -> impl Fn(&FilmlabApp, &App) -> [f64; 2] {
    move |a, cx| {
        let values = a.develop.read(cx).control_values();
        ids.map(|id| values.get(id).copied().unwrap_or(f64::NAN))
    }
}

fn history_step(a: &FilmlabApp, cx: &App) -> usize {
    a.develop.read(cx).cursor
}

async fn color_controls(run: &mut Run, cx: &mut AsyncApp) {
    for (section, shot) in [("color", "04a-color"), ("halation", "04b-halation"), ("calibration", "04c-calibration")] {
        show_section(run, cx, section).await;
        run.shoot(cx, shot).await;
    }
    for (tab, shot) in ["04d-mixer-hue", "04e-mixer-saturation", "04f-mixer-luminance"].into_iter().enumerate() {
        run.act(cx, |a, cx| {
            a.ui.mixer = tab;
            cx.notify();
        });
        show_section(run, cx, "mixer").await;
        run.shoot(cx, shot).await;
    }
    run.develop(cx, |d, cx| d.edit_settings(json!({"treatment": "bw"}), "Black & white", "treatment", cx));
    show_section(run, cx, "mixer").await;
    run.shoot(cx, "04g-mixer-bw").await;
    run.develop(cx, |d, cx| d.undo(cx));
    grading_wheel(run, cx).await;
    defringe_range(run, cx).await;
    tone_curve(run, cx).await;
    run.act(cx, |a, cx| {
        a.ui = Ui::default();
        cx.notify();
    });
    run.pause(cx, 300).await;
}

async fn grading_wheel(run: &mut Run, cx: &mut AsyncApp) {
    show_section(run, cx, "grading").await;
    run.shoot(cx, "04h-grading").await;
    let Some(disc) = run.read(cx, |a, _| a.ui.placed.get(&GradingWheels::disc_id("shadows"))).flatten() else {
        run.check("grading: the Shadows wheel is laid out", false, "");
        return;
    };
    let tint = values_of(["grading.shadows.hue", "grading.shadows.sat"]);
    let before = run.read(cx, &tint).unwrap_or_default();
    let steps = run.read(cx, history_step).unwrap_or(0);
    let (hue, reach) = (135f32.to_radians(), disc.size.width * 0.3);
    let centre = disc.center();
    run.drag(cx, centre, point(centre.x + reach * hue.cos(), centre.y - reach * hue.sin()));
    run.pause(cx, 300).await;
    let [h, s] = run.read(cx, &tint).unwrap_or_default();
    let selected = run.read(cx, |a, _| a.ui.grading).unwrap_or(usize::MAX);
    run.check("grading: dragging inside the Shadows wheel selects it and sets its hue and saturation", selected == 0 && (h - 135.0).abs() <= 2.0 && (s - 60.0).abs() <= 2.0, format!("selected {selected}, hue {h}, sat {s} (want 135, 60)"));
    let added = run.read(cx, history_step).unwrap_or(0) - steps;
    run.check("grading: the wheel drag is one undo step", added == 1, format!("{added} steps added"));
    run.verify(cx, "grading: the tinted photo matches", false).await;
    run.shoot(cx, "04i-grading-tinted").await;
    run.key(cx, "cmd-z");
    let undone = run.read(cx, &tint).unwrap_or_default();
    run.check("grading: one undo takes the whole drag back", undone == before, format!("{undone:?}, before {before:?}"));
    run.key(cx, "cmd-shift-z");
    run.pause(cx, 300).await;
    run.click(cx, centre, 2);
    run.pause(cx, 300).await;
    let reset = run.read(cx, &tint).unwrap_or_default();
    run.check("grading: double-clicking a wheel resets its hue and saturation", reset == [0.0, 0.0], format!("{reset:?}"));
}

async fn defringe_range(run: &mut Run, cx: &mut AsyncApp) {
    const LOW: &str = "optics.defringePurpleHueLo";
    const HIGH: &str = "optics.defringePurpleHueHi";
    show_section(run, cx, "optics").await;
    let Some(row) = run.read(cx, |a, _| a.ui.placed.get(LOW)).flatten() else {
        run.check("defringe: the Purple hue range is laid out", false, "");
        return;
    };
    let ends = values_of([LOW, HIGH]);
    let x_at = |value: f64| row.left() + row.size.width * (value / 100.0) as f32;
    let at = |value: f64| point(x_at(value), row.center().y);
    run.click(cx, at(50.0), 2);
    run.pause(cx, 300).await;
    let reset = run.read(cx, &ends).unwrap_or_default();
    run.check("defringe: double-clicking the range resets both ends", reset == [30.0, 70.0], format!("{reset:?}"));
    run.shoot(cx, "04j-optics").await;

    run.drag(cx, at(30.0), at(50.0));
    run.pause(cx, 300).await;
    let low = run.read(cx, &ends).unwrap_or_default();
    run.check("defringe: dragging the low handle moves only the low end", low == [50.0, 70.0], format!("{low:?}"));
    run.key(cx, "cmd-z");
    let undone = run.read(cx, &ends).unwrap_or_default();
    run.check("defringe: one undo takes the whole drag back", undone == reset, format!("{undone:?}"));
    run.key(cx, "cmd-shift-z");
    run.pause(cx, 300).await;

    run.drag(cx, at(70.0), at(25.0));
    run.pause(cx, 300).await;
    let high = run.read(cx, &ends).unwrap_or_default();
    run.check("defringe: the high handle stops at the low one instead of crossing it", high == [50.0, 50.0], format!("{high:?}"));
    run.verify(cx, "defringe: the edited photo matches", false).await;
    run.shoot(cx, "04k-optics-dragged").await;
    run.click(cx, at(50.0), 2);
    run.pause(cx, 300).await;
}

fn master_points(a: &FilmlabApp, cx: &App) -> Vec<[f64; 2]> {
    let settings = a.develop.read(cx).settings.as_ref().map(Settings::to_value).unwrap_or_default();
    settings["curve"]["master"].as_array().map(|ps| ps.iter().filter_map(|p| Some([p["x"].as_f64()?, p["y"].as_f64()?])).collect()).unwrap_or_default()
}

fn step_label(a: &FilmlabApp, cx: &App) -> String {
    let d = a.develop.read(cx);
    d.history.get(d.cursor).map(|s| s.label.clone()).unwrap_or_default()
}

fn on_diagonal(points: &[[f64; 2]]) -> bool {
    points.iter().all(|[x, y]| (x - y).abs() < 1e-9)
}

fn level(value: f64) -> i64 {
    (value * 255.0).round() as i64
}

fn on_graph(run: &Run, cx: &mut AsyncApp, u: f64, v: f64) -> Point<Pixels> {
    let graph = run.read(cx, |a, _| a.ui.placed.get(curve::GRAPH)).flatten().unwrap_or_default();
    let side = f32::from(graph.size.width) - 20.0;
    point(graph.left() + px(10.0 + u as f32 * side), graph.top() + px(10.0 + (1.0 - v) as f32 * side))
}

fn on_rail(run: &Run, cx: &mut AsyncApp, u: f64) -> Point<Pixels> {
    let floor = on_graph(run, cx, u, 0.0);
    point(floor.x, floor.y + px(11.5))
}

async fn pick_curve_tab(run: &mut Run, cx: &mut AsyncApp, tab: Tab, clicks: usize) -> bool {
    let Some(at) = run.read(cx, move |a, _| a.ui.placed.get(&tab.id())).flatten() else { return false };
    run.click(cx, at.center(), clicks);
    run.pause(cx, 300).await;
    run.read(cx, |a, _| a.ui.curve.tab).is_some_and(|t| t == tab)
}

async fn tone_curve(run: &mut Run, cx: &mut AsyncApp) {
    let start = run.read(cx, history_step).unwrap_or(0);
    show_section(run, cx, "curve").await;
    let graph = run.read(cx, |a, _| a.ui.placed.get(curve::GRAPH)).flatten();
    run.check("tone curve: the section opens with its graph", graph.is_some_and(|g| (f32::from(g.size.height) - 292.0).abs() < 0.5), format!("{graph:?}"));
    if graph.is_none() {
        return;
    }
    let measured = run.wait(cx, 10, |a, cx| a.develop.read(cx).curve_input.as_ref().is_some_and(|i| i.width > 0)).await;
    run.check("tone curve: the tones entering the curve are measured for its histogram", measured, "");

    let regions = pick_curve_tab(run, cx, Tab::Regions, 1).await;
    run.check("tone curve: clicking the Regions tab selects it", regions, "");
    let regional = values_of(["curve.darks", "curve.splitShadows"]);
    let steps = run.read(cx, history_step).unwrap_or(0);
    let (from, to) = (on_graph(run, cx, 0.375, 0.5), on_graph(run, cx, 0.375, 0.6));
    run.drag(cx, from, to);
    run.pause(cx, 300).await;
    let [darks, _] = run.read(cx, &regional).unwrap_or_default();
    let added = run.read(cx, history_step).unwrap_or(0) - steps;
    let label = run.read(cx, step_label).unwrap_or_default();
    run.check("tone curve: dragging up in the Darks band raises Darks as one step", darks == 20.0 && added == 1 && label == "Darks +20", format!("darks {darks}, {added} steps, \"{label}\""));
    run.verify(cx, "tone curve: the Darks edit renders and matches", false).await;
    let over = on_graph(run, cx, 0.375, 0.55);
    run.hover(cx, over);
    let lit = run.read(cx, |a, _| a.ui.curve.tab == Tab::Regions).unwrap_or(false);
    run.shoot_inspector(cx, "04l-curve-regions").await;
    let (from, to) = (on_rail(run, cx, 0.25), on_rail(run, cx, 0.30));
    run.drag(cx, from, to);
    run.pause(cx, 300).await;
    let [_, split] = run.read(cx, &regional).unwrap_or_default();
    run.check("tone curve: dragging the shadows split handle moves the split", lit && split == 30.0, format!("shadows split {split}"));

    let rgb = pick_curve_tab(run, cx, Tab::Rgb, 1).await;
    run.check("tone curve: clicking the RGB tab selects it", rgb, "");
    let Some(plain) = run.verify(cx, "tone curve: the photo before adding a point matches", false).await else { return };
    let steps = run.read(cx, history_step).unwrap_or(0);
    let on_line = on_graph(run, cx, 0.6, 0.6);
    run.click(cx, on_line, 1);
    let added = run.verify(cx, "tone curve: the photo with an added point matches", false).await;
    let master = run.read(cx, master_points).unwrap_or_default();
    let label = run.read(cx, step_label).unwrap_or_default();
    let same = added.is_some_and(|l| l.hash == plain.hash || (l.mean - plain.mean).abs() < 0.001);
    run.check(
        "tone curve: clicking the line adds a point without changing the photo",
        master.len() == 3 && on_diagonal(&master) && same && label == format!("RGB curve point at {}", level(master.get(1).map_or(0.0, |p| p[0]))) && run.read(cx, history_step).unwrap_or(0) == steps + 1,
        format!("{master:?}, \"{label}\", unchanged {same}"),
    );

    let steps = run.read(cx, history_step).unwrap_or(0);
    let before = master.clone();
    let (from, to) = (on_graph(run, cx, 0.6, 0.6), on_graph(run, cx, 0.6, 0.72));
    run.hold(cx, from, to);
    run.shoot_inspector(cx, "04m-curve-rgb-drag").await;
    run.release(cx, to);
    let lifted = run.verify(cx, "tone curve: the lifted curve renders and matches", false).await;
    let master = run.read(cx, master_points).unwrap_or_default();
    let label = run.read(cx, step_label).unwrap_or_default();
    let added = run.read(cx, history_step).unwrap_or(0) - steps;
    let lifted_point = master.get(1).copied().unwrap_or_default();
    run.check(
        "tone curve: dragging the point bends the curve and brightens the photo as one step",
        (lifted_point[1] - 0.72).abs() < 0.01 && lifted.is_some_and(|l| l.mean > plain.mean + 0.005) && added == 1 && label == format!("RGB curve {} → {}", level(lifted_point[0]), level(lifted_point[1])),
        format!("{master:?}, {added} steps, \"{label}\""),
    );
    let over = on_graph(run, cx, 0.6, 0.72);
    run.hover(cx, over);
    run.shoot_inspector(cx, "04n-curve-rgb-hover").await;
    run.key(cx, "cmd-z");
    let undone = run.read(cx, master_points).unwrap_or_default();
    run.check("tone curve: one undo takes the whole point drag back", undone == before, format!("{undone:?}"));
    run.key(cx, "cmd-shift-z");
    run.pause(cx, 300).await;

    let photo = run.on_window(cx, |window, cx| run_fit_center(&run.app, window, cx)).flatten();
    if let Some(at) = photo {
        run.hover(cx, at);
        run.pause(cx, 200).await;
        let pointing = run.read(cx, |a, _| a.ui.curve.pointed().is_some()).unwrap_or(false);
        run.check("tone curve: the pointer over the photo rings its tone on the curve", pointing, "");
        run.shoot_inspector(cx, "04o-curve-rgb-pointer").await;
        let (shown, raw) = pointer_sweep(run, cx).await;
        run.check(
            "tone curve: sliding over a textured patch moves the ring at most half as much as the raw pixels",
            raw > 0.0 && shown <= raw / 2.0,
            format!("ring travelled {shown:.0} levels, the pixels under it {raw:.0}"),
        );
        let away = on_graph(run, cx, 0.3, 0.95);
        run.hover(cx, away);
    }

    let (from, below) = (on_graph(run, cx, 0.6, 0.72), on_graph(run, cx, 0.6, 0.0));
    let off = point(below.x, below.y + px(62.0));
    run.hold(cx, from, off);
    let warned = run.read(cx, |a, _| a.ui.curve.removing()).unwrap_or(false);
    run.shoot_inspector(cx, "04p-curve-rgb-remove").await;
    run.release(cx, off);
    run.pause(cx, 300).await;
    let master = run.read(cx, master_points).unwrap_or_default();
    let label = run.read(cx, step_label).unwrap_or_default();
    run.check("tone curve: dragging a point off the graph removes it", warned && master.len() <= 2 && on_diagonal(&master) && label == "RGB curve point removed", format!("release to remove shown {warned}, {master:?}, \"{label}\""));

    let names = run.read(cx, |a, _| curve::preset_names(a)).unwrap_or_default();
    let medium = names.iter().position(|n| n == "Medium Contrast");
    if let Some(i) = medium {
        run.act(cx, |a, cx| curve::pick_preset(a, i, cx));
    }
    run.verify(cx, "tone curve: the Medium Contrast preset renders and matches", false).await;
    let master = run.read(cx, master_points).unwrap_or_default();
    let label = run.read(cx, step_label).unwrap_or_default();
    run.check(
        "tone curve: the point curve menu applies Medium Contrast",
        master == [[0.0, 0.0], [0.25, 0.2], [0.5, 0.5], [0.75, 0.8], [1.0, 1.0]] && label == "Point curve Medium Contrast",
        format!("menu {names:?}, {master:?}, \"{label}\""),
    );
    run.shoot_inspector(cx, "04q-curve-rgb-preset").await;

    let app = run.app.clone();
    run.on_window(cx, |window, cx| app.update(cx, |a, cx| curve::choose(a, curve::Preset::Save, window, cx)));
    run.pause(cx, 300).await;
    for c in "medium print".chars() {
        run.key(cx, &if c == ' ' { "space-> ".to_string() } else { format!("{c}->{c}") });
    }
    run.key(cx, "enter");
    run.pause(cx, 300).await;
    let kept = run.read(cx, |a, _| a.rail.curves().find(|p| p.name == "medium print").map(|p| p.id.clone())).flatten();
    run.check("tone curve: Save curve keeps the tone curve alone as a profile", kept.is_some(), format!("curves {:?}", run.read(cx, |a, _| a.rail.curves().map(|p| p.name.clone()).collect::<Vec<_>>())));
    if let Some(linear) = names.iter().position(|n| n == "Linear") {
        run.act(cx, |a, cx| curve::pick_preset(a, linear, cx));
    }
    run.pause(cx, 300).await;
    if let Some(id) = kept.clone() {
        run.on_window(cx, |window, cx| app.update(cx, |a, cx| curve::choose(a, curve::Preset::Saved(id), window, cx)));
    }
    run.verify(cx, "tone curve: a saved curve renders and matches", false).await;
    let master = run.read(cx, master_points).unwrap_or_default();
    let label = run.read(cx, step_label).unwrap_or_default();
    let named = run.read(cx, |a, cx| a.develop.read(cx).settings.clone().is_some_and(|s| a.rail.curves().any(|p| p.name == "medium print" && p.partial["curve"] == s.to_value()["curve"]))).unwrap_or(false);
    run.check(
        "tone curve: the curve menu applies a saved curve as one step and names it",
        master == [[0.0, 0.0], [0.25, 0.2], [0.5, 0.5], [0.75, 0.8], [1.0, 1.0]] && label == "Tone curve medium print" && named,
        format!("{master:?}, \"{label}\", named {named}"),
    );

    let (photo, steps) = run.read(cx, |a, cx| (a.develop.read(cx).photo_id.clone(), history_step(a, cx))).unwrap_or_default();
    let middle = on_graph(run, cx, 0.5, 0.5);
    run.click(cx, middle, 1);
    run.pause(cx, 200).await;
    for key in ["up", "shift-up", "left"] {
        run.key(cx, key);
    }
    run.pause(cx, 300).await;
    let nudged = run.read(cx, master_points).unwrap_or_default().get(2).copied().unwrap_or_default();
    let (still, added) = run.read(cx, |a, cx| (a.develop.read(cx).photo_id.clone(), history_step(a, cx) - steps)).unwrap_or_default();
    run.check(
        "tone curve: arrow keys move the focused point a level at a time, ten with shift, as one step",
        (nudged[1] - (0.5 + 11.0 / 255.0)).abs() < 1e-9 && (nudged[0] - (0.5 - 1.0 / 255.0)).abs() < 1e-9 && still == photo && added == 1,
        format!("{nudged:?}, {added} steps, same photo {}", still == photo),
    );
    run.key(cx, "tab");
    run.shoot_inspector(cx, "04r-curve-rgb-keyboard").await;
    run.key(cx, "backspace");
    run.pause(cx, 300).await;
    let master = run.read(cx, master_points).unwrap_or_default();
    run.check("tone curve: tab moves to the next point and delete removes it", master.len() == 4 && !master.iter().any(|p| p[0] == 0.75), format!("{master:?}"));
    run.key(cx, "escape");
    run.verify(cx, "tone curve: the keyboard edit renders and matches", false).await;

    pick_curve_tab(run, cx, Tab::Red, 1).await;
    let (from, to) = (on_graph(run, cx, 0.62, 0.62), on_graph(run, cx, 0.62, 0.66));
    run.drag(cx, from, to);
    run.pause(cx, 300).await;
    let label = run.read(cx, step_label).unwrap_or_default();
    run.check("tone curve: a press and drag on the Red line adds and drags a point in one step", label.starts_with("Red curve 158 → "), format!("\"{label}\""));
    run.shoot_inspector(cx, "04s-curve-red").await;
    pick_curve_tab(run, cx, Tab::Green, 1).await;
    run.shoot_inspector(cx, "04t-curve-green").await;
    pick_curve_tab(run, cx, Tab::Blue, 1).await;
    let (from, to) = (on_graph(run, cx, 0.0, 0.0), on_graph(run, cx, 0.0, 0.07));
    run.drag(cx, from, to);
    run.pause(cx, 300).await;
    let label = run.read(cx, step_label).unwrap_or_default();
    run.check("tone curve: dragging the Blue black point lifts it", label == "Blue curve 0 → 18", format!("\"{label}\""));
    run.shoot_inspector(cx, "04u-curve-blue").await;

    run.develop(cx, |d, cx| d.command("develop.curve", json!({"channel": "red", "points": [[0.0, 0.0], [0.4, 0.46], [1.0, 1.0]]}), "Warm the shadows", "agent.e2e.curve", cx));
    let marked = run.read(cx, |a, cx| a.develop.read(cx).agent_marks.contains_key("curve.red")).unwrap_or(false);
    pick_curve_tab(run, cx, Tab::Red, 1).await;
    run.check("tone curve: an agent's point curve is marked", marked, "");
    run.shoot_inspector(cx, "04v-curve-red-agent").await;
    run.develop(cx, |d, cx| d.undo(cx));

    pick_curve_tab(run, cx, Tab::Rgb, 1).await;
    let shown = |a: &FilmlabApp, cx: &App| a.develop.read(cx).fit.as_ref().map(|f| std::sync::Arc::as_ptr(f) as usize);
    let press = on_graph(run, cx, 0.35, 0.35);
    run.hold(cx, press, press);
    let (mut last, mut updates, mut gap, mut since) = (run.read(cx, shown).flatten(), 0, Duration::ZERO, Instant::now());
    for i in 0..60 {
        let wave = (i as f64 / 60.0 * std::f64::consts::TAU).sin() * 0.12;
        let to = on_graph(run, cx, 0.35, 0.35 + wave);
        run.move_held(cx, to);
        run.pause(cx, 16).await;
        let now = run.read(cx, shown).flatten();
        if now != last {
            (last, updates) = (now, updates + 1);
            gap = gap.max(since.elapsed());
            since = Instant::now();
        }
    }
    gap = gap.max(since.elapsed());
    run.release(cx, press);
    run.pause(cx, 300).await;
    run.check("tone curve: the photo keeps up with a one-second point drag", updates >= 20 && gap < Duration::from_millis(250), format!("{updates} photo frames for 60 moves, longest wait {} ms", gap.as_millis()));
    if let Some(rgb) = run.read(cx, |a, _| a.ui.placed.get(&Tab::Rgb.id())).flatten() {
        run.click(cx, rgb.center(), 2);
    }
    let began = Instant::now();
    let mut eased = Vec::new();
    while began.elapsed() < Duration::from_millis(400) {
        eased.push((began.elapsed().as_millis(), run.read(cx, |a, _| a.ui.curve.resetting()).flatten()));
        run.pause(cx, 10).await;
    }
    let moving: Vec<_> = eased.iter().filter_map(|(ms, t)| t.map(|t| (*ms, t))).collect();
    let done = eased.iter().find(|(_, t)| t.is_none() && !moving.is_empty()).map(|(ms, _)| *ms);
    let ordered = moving.windows(2).all(|w| w[1].1 >= w[0].1);
    run.check(
        "tone curve: double-clicking the RGB tab eases the line back to straight over about 200 ms",
        moving.len() >= 5 && ordered && done.is_some_and(|ms| ms <= 320),
        format!("{} samples while easing, {:?}, straight by {done:?} ms", moving.len(), moving.iter().map(|(ms, t)| format!("{ms}ms:{t:.2}")).collect::<Vec<_>>()),
    );

    let reset = pick_curve_tab(run, cx, Tab::Regions, 2).await;
    run.pause(cx, 300).await;
    let after = run.read(cx, &regional).unwrap_or_default();
    run.check("tone curve: double-clicking the Regions tab resets it", reset && after == [0.0, 25.0], format!("{after:?}"));
    run.verify(cx, "tone curve: the reset curve renders and matches", false).await;
    run.develop(cx, |d, cx| d.go_to(start, cx));
    run.settled(cx, false).await;
}

async fn zoom(run: &mut Run, cx: &mut AsyncApp) {
    let center = run.on_window(cx, |window, cx| run_fit_center(&run.app, window, cx)).flatten();
    let Some(center) = center else {
        run.check("zoom: the fit frame is on screen", false, "");
        return;
    };
    let scale = |a: &FilmlabApp, cx: &App| a.develop.read(cx).camera.scale;
    let fit = run.read(cx, scale).unwrap_or(0.0);

    let inspector = run.on_window(cx, |window, _| {
        let w = window.viewport_size().width;
        point(w - px(MARGIN + INSPECTOR / 2.0), px(TOP + MARGIN + 150.0))
    });
    if let Some(at) = inspector {
        run.click(cx, at, 1);
        run.pause(cx, 600).await;
        let after = run.read(cx, scale).unwrap_or(0.0);
        run.check("zoom: a click in the inspector doesn't reach the photo", (after - fit).abs() < 1e-9, format!("scale {fit:.4} → {after:.4}"));
    }

    run.click(cx, center, 1);
    let zoomed = run.wait(cx, 5, move |a, cx| a.develop.read(cx).camera.scale > fit * 1.5).await;
    run.check("zoom: clicking the photo zooms to 1:1", zoomed, "");
    run.verify(cx, "zoom: the 1:1 window matches an independent render", true).await;
    run.shoot(cx, "05-zoom").await;

    let camera = run.read(cx, |a, cx| a.develop.read(cx).camera);
    run.drag(cx, center, point(center.x - px(160.0), center.y - px(90.0)));
    run.pause(cx, 300).await;
    let panned = run.read(cx, |a, cx| a.develop.read(cx).camera);
    run.check("zoom: dragging pans the 1:1 view", camera.zip(panned).is_some_and(|(a, b)| (a.cx - b.cx).abs() > 1e-4 || (a.cy - b.cy).abs() > 1e-4), "");
    run.verify(cx, "zoom: the panned window matches an independent render", true).await;
    slow_pan(run, cx).await;

    run.key(cx, "z");
    let back = run.wait(cx, 5, move |a, cx| (a.develop.read(cx).camera.scale - fit).abs() < 1e-6).await;
    run.check("zoom: Z returns to fit", back, "");
    run.key(cx, "space");
    let again = run.wait(cx, 5, move |a, cx| a.develop.read(cx).camera.scale > fit * 1.5).await;
    run.check("zoom: Space zooms to 1:1", again, "");
    run.key(cx, "space");
    run.wait(cx, 5, move |a, cx| (a.develop.read(cx).camera.scale - fit).abs() < 1e-6).await;
    run.verify(cx, "zoom: fit again matches", false).await;
}

async fn slow_pan(run: &mut Run, cx: &mut AsyncApp) {
    let Some((camera, native)) = run.read(cx, |a, cx| (a.develop.read(cx).camera, a.develop.read(cx).native)) else { return };
    let (mut pieced, mut landed) = (false, 0);
    let mut last = run.read(cx, |a, cx| a.develop.read(cx).detail.as_ref().map(|f| Arc::as_ptr(f) as usize)).flatten();
    for i in 0..90 {
        let at = camera.cx + 6.0 * f64::from(i) / native.0;
        run.develop(cx, move |d, cx| d.set_zoom(Zoom::At { scale: camera.scale, cx: at, cy: camera.cy }, false, true, cx));
        run.pause(cx, 16).await;
        let (now, strip) = run.read(cx, |a, cx| {
            let d = a.develop.read(cx);
            (d.detail.as_ref().map(|f| Arc::as_ptr(f) as usize), d.shown_plan(true).is_some_and(crate::develop::pan::pieced))
        }).unwrap_or((None, false));
        pieced |= strip;
        landed += usize::from(now != last);
        last = now;
    }
    run.check("zoom: a slow pan joins newly rendered strips onto the 1:1 view while it moves", pieced, format!("{landed} detail frames while moving"));
    run.verify(cx, "zoom: where a slow pan stops matches an independent render", true).await;
}

async fn pointer_sweep(run: &mut Run, cx: &mut AsyncApp) -> (f64, f64) {
    let Some(frame) = run.on_window(cx, |window, cx| run.app.upgrade().and_then(|a| a.read(cx).fit_frame(window, cx))).flatten().map(|(b, _)| b) else { return (0.0, 0.0) };
    let luma = |rgb: [u8; 3]| rgb.iter().map(|c| f64::from(*c)).sum::<f64>() / 3.0;
    let (mut shown, mut raw) = (Vec::new(), Vec::new());
    for step in 0..16 {
        let u = 0.4 + 0.004 * f64::from(step);
        let at = point(frame.left() + frame.size.width * u as f32, frame.top() + frame.size.height * 0.62);
        run.hover(cx, at);
        run.pause(cx, 30).await;
        let read = run.read(cx, move |a, cx| Some((a.ui.curve.pointed()?, a.develop.read(cx).curve_input.as_ref()?.mean_around(u, 0.62, 0.0)?))).flatten();
        if let Some((s, r)) = read {
            shown.push(luma(s));
            raw.push(luma(r));
        }
    }
    let travel = |v: &[f64]| v.windows(2).map(|w| (w[1] - w[0]).abs()).sum::<f64>();
    (travel(&shown), travel(&raw))
}

fn run_fit_center(app: &WeakEntity<FilmlabApp>, window: &mut Window, cx: &mut App) -> Option<Point<Pixels>> {
    app.upgrade().and_then(|a| a.read(cx).fit_frame(window, cx)).map(|(b, _)| b.center())
}

async fn tools(run: &mut Run, cx: &mut AsyncApp) {
    let tool = |a: &FilmlabApp, cx: &App| a.develop.read(cx).tool;
    let Some((base, ..)) = run.shown(cx, false) else { return };

    let step = run.read(cx, |a, cx| a.develop.read(cx).cursor).unwrap_or(0);
    run.key(cx, "r");
    let crop = run.wait(cx, 5, move |a, cx| tool(a, cx) == Tool::Crop).await;
    run.check("crop: R opens the crop tool", crop, "");
    let square = run.develop(cx, |d, cx| d.command("crop.aspect", json!({"aspect": "1x1"}), "Crop 1 : 1", "crop.aspect", cx)).flatten();
    run.settled(cx, false).await;
    let angle = run.develop(cx, |d, cx| d.command("develop.set", json!({"values": {"crop.angle": 4.0}}), "Straighten", "crop.angle", cx)).flatten();
    run.check("crop: the 1:1 aspect and a 4° straighten apply", square.is_some() && angle.is_some(), "");
    let level = run.wait(cx, 2, |a, _| crop::shows_level_grid(a)).await;
    run.check("crop: straightening shows the level grid", level, "");
    run.shoot(cx, "06a-crop-level").await;
    let cleared = run.wait(cx, 3, |a, _| !crop::shows_level_grid(a)).await;
    run.check("crop: the level grid clears once the angle stops changing", cleared, "");
    run.verify(cx, "crop: the crop tool's frame matches", false).await;
    run.shoot(cx, "06-crop").await;
    run.key(cx, "e");
    run.wait(cx, 5, move |a, cx| tool(a, cx) == Tool::Edit).await;
    let cropped = run.verify(cx, "crop: the cropped photo matches", false).await;
    run.check("crop: 1:1 makes a square frame", cropped.is_some_and(|l| (i64::from(l.size.0) - i64::from(l.size.1)).abs() <= 2), cropped.map_or(String::new(), |l| format!("{}x{}", l.size.0, l.size.1)));
    run.develop(cx, |d, cx| d.go_to(step, cx));
    let uncropped = run.verify(cx, "crop: going back before the crop matches", false).await;
    run.check("crop: going back restores the uncropped frame", uncropped.is_some_and(|l| l.hash == base.hash), "");

    run.key(cx, "q");
    let heal = run.wait(cx, 5, move |a, cx| tool(a, cx) == Tool::Heal).await;
    run.check("heal: Q opens the heal tool", heal, "");
    let spot = run.develop(cx, |d, cx| d.command("spot.add", json!({"mode": "heal", "points": [[0.5, 0.5]], "size": 0.05, "feather": 0.5, "opacity": 1.0}), "Heal spot", "spot.add.e2e", cx));
    run.check("heal: adding a spot succeeds", spot.flatten().is_some(), "");
    let healed = run.verify(cx, "heal: the healed photo matches", false).await;
    run.check("heal: the spot changes the frame", healed.is_some_and(|l| l.hash != base.hash), "");
    run.shoot(cx, "07-heal").await;
    run.key(cx, "cmd-z");
    run.verify(cx, "heal: undoing the spot matches", false).await;

    run.key(cx, "m");
    let masks = run.wait(cx, 5, move |a, cx| tool(a, cx) == Tool::Masks).await;
    run.check("masks: M opens the masks tool", masks, "");
    let added = run.develop(cx, |d, cx| d.command("mask.add", json!({"kind": "radial"}), "Add radial gradient", "mask.add", cx)).flatten();
    let id = added.as_ref().and_then(|r| r["activeMask"].as_u64());
    let count = run.read(cx, |a, cx| a.develop.read(cx).settings.as_ref().and_then(|s| s.to_value()["masks"].as_array().map(Vec::len)).unwrap_or(0)).unwrap_or(0);
    run.check("masks: adding a radial mask succeeds", added.is_some() && count == 1, format!("{count} masks, result {}", added.as_ref().map_or(String::new(), Value::to_string)));
    if let Some(id) = id {
        run.develop(cx, |d, cx| d.command("mask.adjust", json!({"id": id, "values": {"exposure": 1.5}}), "Mask exposure", "mask.adjust.e2e", cx));
    }
    let masked = run.verify(cx, "masks: the masked photo matches", false).await;
    run.check("masks: the mask's exposure changes the frame", masked.is_some_and(|l| l.hash != base.hash), format!("mask id {id:?}"));
    run.shoot(cx, "08-masks").await;
    run.key(cx, "escape");
    let edit = run.wait(cx, 5, move |a, cx| tool(a, cx) == Tool::Edit).await;
    run.check("masks: Escape returns to Edit", edit, "");
    run.verify(cx, "masks: the edit view with a mask matches", false).await;
}

async fn compare_and_profiles(run: &mut Run, cx: &mut AsyncApp) {
    run.key(cx, "\\");
    let before = run.wait(cx, 5, |a, cx| a.develop.read(cx).compare == Compare::Before).await;
    run.check("compare: backslash shows the original", before, "");
    run.verify(cx, "compare: the before view matches", false).await;
    run.shoot(cx, "09-before").await;
    run.key(cx, "\\");
    run.wait(cx, 5, |a, cx| a.develop.read(cx).compare == Compare::After).await;
    run.verify(cx, "compare: the after view matches", false).await;

    let settings = run.read(cx, |a, cx| a.develop.read(cx).settings.clone()).flatten();
    let saved = settings.as_ref().map_or(f64::NAN, |s| exposure_of(&s.to_value()));
    let film = run.film(cx).map(|f| f.to_value());
    let profile = settings.as_ref().and_then(|s| super::rail::store::build("E2E look", s, vec!["light".into(), "film".into()], film));
    run.check("profiles: a profile builds from the current settings", profile.is_some(), "");
    let Some(profile) = profile else { return };
    let mut all = super::rail::store::load();
    all.push(profile.clone());
    super::rail::store::write(&all);
    let stored = super::rail::store::load().iter().any(|p| p.id == profile.id);
    run.check("profiles: the profile is saved and reloads", stored, "");
    run.develop(cx, |d, cx| d.edit_controls(&[("light.exposure", -1.0)], "Exposure", cx));
    run.settled(cx, false).await;
    let film = profile.film_look();
    run.develop(cx, |d, cx| d.apply_profile(&profile.id, &profile.partial, 1.0, "Profile E2E look", film, cx));
    run.verify(cx, "profiles: applying the profile renders and matches", false).await;
    let exposure = run.read(cx, |a, cx| a.develop.read(cx).control_values().get("light.exposure").copied()).flatten().unwrap_or(f64::NAN);
    run.check("profiles: applying it at 100% restores its exposure", (exposure - saved).abs() < 1e-6, format!("exposure {exposure}, profile {saved}"));
}

async fn navigation(run: &mut Run, cx: &mut AsyncApp, first: &str) {
    let (index, count) = run.read(cx, |a, cx| {
        let l = a.library.read(cx);
        (a.develop.read(cx).photo_id.as_deref().and_then(|id| l.index_of(id)).unwrap_or(0), l.photos.len())
    }).unwrap_or((0, 0));
    let (there, back) = if index + 1 < count { ("right", "left") } else { ("left", "right") };
    run.key(cx, there);
    let moved = run.wait(cx, 10, {
        let first = first.to_string();
        move |a, cx| a.develop.read(cx).photo_id.as_deref().is_some_and(|id| id != first)
    })
    .await;
    run.check("navigation: an arrow key opens the neighbouring photo", moved, format!("{there} from {} of {count}", index + 1));
    run.verify(cx, "navigation: the next photo matches", false).await;
    run.key(cx, back);
    let back = run.wait(cx, 10, {
        let first = first.to_string();
        move |a, cx| a.develop.read(cx).photo_id.as_deref() == Some(first.as_str())
    })
    .await;
    run.check("navigation: the opposite arrow comes back", back, "");
    run.verify(cx, "navigation: the first photo again matches", false).await;
}

async fn export(run: &mut Run, cx: &mut AsyncApp) {
    run.key(cx, "cmd-shift-e");
    let open = run.wait(cx, 5, |a, _| a.export.is_open()).await;
    run.check("export: Cmd-Shift-E opens the export dialog", open, "");
    run.shoot(cx, "10-export").await;
    run.key(cx, "enter");
    let finished = run.wait(cx, 120, |a, _| a.export.outcome().is_some()).await;
    let (files, errors) = run.read(cx, |a, _| a.export.outcome().map(|(f, e)| (f.to_vec(), e.to_vec()))).flatten().unwrap_or_default();
    run.check("export: the export finishes without errors", finished && errors.is_empty() && files.len() == 1, format!("{files:?} {errors:?}"));
    if let Some(file) = files.first() {
        let decoded = image::open(file).map(|i| (i.width(), i.height(), i.to_rgba8()));
        let ok = decoded.as_ref().is_ok_and(|(w, h, rgba)| healthy(&look_of(*w, *h, rgba.as_raw())) && *w.max(h) >= 2000);
        run.check("export: the file decodes to a full-size, healthy image", ok, decoded.map_or_else(|e| e.to_string(), |(w, h, _)| format!("{} {w}x{h}", file.display())));
    }
    run.key(cx, "enter");
    let closed = run.wait(cx, 5, |a, _| !a.export.is_open()).await;
    run.check("export: Enter closes the finished dialog", closed, "");
}

async fn leave(run: &mut Run, cx: &mut AsyncApp, folder: &Path, stem: &str, facts: &Value) {
    run.key(cx, "g");
    let browse = run.wait(cx, 5, |a, _| a.mode == Mode::Browse).await;
    run.check("library: G returns to the library", browse, "");
    let edited = edited_thumbnail(run, cx, stem).await;
    run.check("library: the edited photo's thumbnail is identical to a fresh render of its edit", edited, "");
    run.shoot(cx, "11-library-after").await;
    let unloaded = run.wait(cx, 15, |a, cx| a.develop.read(cx).photo_id.is_none()).await;
    run.check("memory: Develop unloads after a while in the library", unloaded, "");
    run.wait(cx, 30, |_, _| crate::perf::quiet_for(std::time::Duration::from_secs(3))).await;
    run.pause(cx, 1000).await;
    let footprint = super::soak::footprint() as f64 / 1_048_576.0;
    run.check("memory: the idle footprint stays small", footprint < 600.0, format!("{footprint:.0} MB"));

    let state = sidecar(folder, stem, facts);
    let exposure = state.as_ref().map_or(f64::NAN, |s| exposure_of(&s["settings"]));
    let masks = state.as_ref().and_then(|s| s["settings"]["masks"].as_array().map(Vec::len)).unwrap_or(0);
    run.check("persistence: the sidecar keeps the final edit and the mask", (exposure - 0.7).abs() < 1e-6 && masks == 1, format!("exposure {exposure}, {masks} masks"));
    let folder = folder.to_path_buf();
    run.act(cx, |a, cx| a.library.update(cx, |l, cx| l.open(folder, cx)));
    let reread = run.wait(cx, 60, {
        let stem = stem.to_string();
        move |a, cx| a.library.read(cx).photos.iter().find(|r| r.file.stem == stem).and_then(|r| r.state.as_ref()).is_some_and(|s| (exposure_of(&s.settings.to_value()) - 0.7).abs() < 1e-6)
    })
    .await;
    run.check("persistence: reopening the folder reads the edit back", reread, "");
    let cached = run
        .wait(cx, 20, |a, cx| {
            let l = a.library.read(cx);
            l.photos.iter().all(crate::library::PhotoRecord::indexed) && a.browse.order().iter().all(|id| l.record(id).is_some_and(|r| r.film.is_some() && l.picture(id).is_some_and(|p| p.film == r.film)))
        })
        .await;
    run.check("library: reopening shows the cached film thumbnails", cached, "");
}

async fn edited_thumbnail(run: &mut Run, cx: &mut AsyncApp, stem: &str) -> bool {
    let started = Instant::now();
    while started.elapsed() < Duration::from_secs(30) {
        let sample = run
            .read(cx, |a, cx| {
                let l = a.library.read(cx);
                let r = l.photos.iter().find(|r| r.file.stem == stem)?;
                let picture = l.picture(&r.file.id).filter(|p| p.film.is_some() && p.film == r.film)?;
                Some((r.file.path.clone(), r.probed.clone()?, r.state.clone()?, picture.image.clone()))
            })
            .flatten();
        if let Some((path, probed, state, image)) = sample {
            let expected = cx.background_spawn(async move { crate::library::fresh_thumbnail(&path, &probed, &state) }).await;
            if expected.is_ok_and(|e| rgba_of(&image).is_some_and(|s| s == e)) {
                return true;
            }
        }
        run.pause(cx, 500).await;
    }
    false
}

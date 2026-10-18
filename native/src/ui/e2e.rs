use std::path::{Path, PathBuf};
use std::time::Instant;

use filmlab_engine::core::{self, Engine};
use gpui_kit::*;
use serde_json::{Value, json};

use super::app::{FilmlabApp, INSPECTOR, MARGIN, Mode, TOP};
use super::driver::Driver;
use crate::develop::plans::Plan;
use crate::develop::{Compare, DisplayFrame, Tool};
use crate::film::{FilmSettings, Gauge};
use crate::library::ensure_luts;
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
    let luma: Vec<f64> = rgba.chunks_exact(4).map(|p| (0.2126 * f64::from(p[0]) + 0.7152 * f64::from(p[1]) + 0.0722 * f64::from(p[2])) / 255.0).collect();
    let n = luma.len().max(1) as f64;
    let mean = luma.iter().sum::<f64>() / n;
    let spread = (luma.iter().map(|v| (v - mean).powi(2)).sum::<f64>() / n).sqrt();
    let rows = rgba.chunks_exact(width as usize * 4);
    let total = rows.len().max(1) as f64;
    let flat = rgba.chunks_exact(width as usize * 4).filter(|row| row.chunks_exact(4).all(|p| p == &row[..4])).count() as f64;
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
    let mut frame = engine.render("reference", &plan.settings.to_string(), &view.to_string())?;
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
    cx.spawn(async move |cx| {
        let mut run = run;
        script(&mut run, cx).await;
        eprintln!("e2e done: {} passed, {} failed {:?}", run.passed, run.failed.len(), run.failed);
        cx.update(|cx| cx.quit());
    })
    .detach();
}

async fn script(run: &mut Run, cx: &mut AsyncApp) {
    library(run, cx).await;
    let Some((id, folder, stem, facts)) = open_first(run, cx).await else { return };
    edits(run, cx, &folder, &stem, &facts).await;
    film(run, cx).await;
    zoom(run, cx).await;
    tools(run, cx).await;
    compare_and_profiles(run, cx).await;
    navigation(run, cx, &id).await;
    export(run, cx).await;
    leave(run, cx, &folder, &stem, &facts).await;
}

async fn library(run: &mut Run, cx: &mut AsyncApp) {
    let loaded = run.wait(cx, 120, |a, cx| {
        let l = a.library.read(cx);
        !l.photos.is_empty() && l.photos.iter().all(|r| r.thumbnail.is_some() || r.error.is_some())
    });
    let ready = loaded.await;
    let (count, errors) = run.read(cx, |a, cx| {
        let l = a.library.read(cx);
        (l.photos.len(), l.photos.iter().filter_map(|r| r.error.clone().map(|e| format!("{}: {e}", r.file.name))).collect::<Vec<_>>())
    }).unwrap_or_default();
    run.check("library: every photo indexed with a thumbnail", ready && errors.is_empty() && count > 1, format!("{count} photos, errors {errors:?}"));
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
    let angle = run.develop(cx, |d, cx| d.command("develop.set", json!({"values": {"crop.angle": 4.0}}), "Straighten", "crop.angle", cx)).flatten();
    run.check("crop: the 1:1 aspect and a 4° straighten apply", square.is_some() && angle.is_some(), "");
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
    let count = run.read(cx, |a, cx| a.develop.read(cx).settings.as_ref().and_then(|s| s["masks"].as_array().map(Vec::len)).unwrap_or(0)).unwrap_or(0);
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

    let settings = run.read(cx, |a, cx| a.develop.read(cx).settings.clone()).flatten().unwrap_or(Value::Null);
    let saved = exposure_of(&settings);
    let film = run.film(cx).map(|f| f.to_value());
    let profile = super::rail::store::build("E2E look", &settings, vec!["light".into(), "film".into()], film);
    run.check("profiles: a profile builds from the current settings", profile.is_some(), "");
    let Some(profile) = profile else { return };
    let mut all = super::rail::store::load();
    all.push(profile.clone());
    super::rail::store::write(&all);
    let stored = super::rail::store::load().iter().any(|p| p.id == profile.id);
    run.check("profiles: the profile is saved and reloads", stored, "");
    run.develop(cx, |d, cx| d.edit_controls(&[("light.exposure", -1.0)], "Exposure", cx));
    run.settled(cx, false).await;
    let film = profile.film.as_ref().and_then(FilmSettings::of);
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
    run.shoot(cx, "11-library-after").await;
    let unloaded = run.wait(cx, 15, |a, cx| a.develop.read(cx).photo_id.is_none()).await;
    run.check("memory: Develop unloads after a while in the library", unloaded, "");
    run.pause(cx, 3000).await;
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
        move |a, cx| a.library.read(cx).photos.iter().find(|r| r.file.stem == stem).and_then(|r| r.state.as_ref()).is_some_and(|s| (exposure_of(&s.settings) - 0.7).abs() < 1e-6)
    })
    .await;
    run.check("persistence: reopening the folder reads the edit back", reread, "");
}

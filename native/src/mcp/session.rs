use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use filmlab_engine::core::{self, Commands, Engine};
use gpui_kit::{AppContext, AsyncApp};
use serde_json::Value;
use tokio::sync::{Mutex as Queue, OwnedMutexGuard};

use crate::develop::plans::{Level, PREVIEW_EDGE};
use crate::develop::{Applied, RenderJob, Step};
use crate::film::{FilmSettings, render_film};
use crate::library::{PhotoRecord, PhotoState};
use crate::ui::driver::Driver;

/// How many photos agents may hold open off screen at once, and how long an idle one (or its decodes) lives.
const LIVE: usize = 4;
const IDLE: Duration = Duration::from_secs(60);
const FULL_IDLE: Duration = Duration::from_secs(10);
const PREVIEW_IDLE: Duration = Duration::from_secs(10);
const SWEEP: Duration = Duration::from_secs(5);
const COALESCE: Duration = Duration::from_millis(900);
const GONE: &str = "Filmlab's window is gone";
pub const NO_PHOTO: &str = "no photo is open; open_photo first, or pass photo";
const NO_PIXELS: &str = "pixels aren't available";

fn source_key(id: &str, level: Level) -> String {
    format!("{id}#{}", level.tag())
}

fn long_edge(facts: &Value) -> f64 {
    facts["width"].as_f64().unwrap_or(0.0).max(facts["height"].as_f64().unwrap_or(0.0))
}

/// A photo an agent edits without opening it in the window: its own engine and LightCraft session, its own history,
/// and every change saved through the library like Develop's.
pub struct Session {
    pub id: String,
    folder: PathBuf,
    path: PathBuf,
    pub facts: Value,
    pub before: Value,
    pub settings: Value,
    pub film: Option<FilmSettings>,
    pub history: Vec<Step>,
    pub cursor: usize,
    pub applied: Option<Applied>,
    commands: Commands,
    engine: Arc<Mutex<Engine>>,
    preview: bool,
    full: bool,
    used: Instant,
    full_used: Instant,
}

impl Session {
    fn open(record: &PhotoRecord, folder: PathBuf) -> Result<Session, String> {
        let (Some(probed), Some(state)) = (&record.probed, &record.state) else {
            return Err(format!("{} is still being indexed; try again in a moment", record.file.id));
        };
        let mut commands = Commands::new();
        commands.open(&probed.facts.to_string(), &state.settings.to_string())?;
        let film = state.film();
        let now = Instant::now();
        Ok(Session {
            id: record.file.id.clone(),
            folder,
            path: record.file.path.clone(),
            facts: probed.facts.clone(),
            before: probed.defaults.clone(),
            settings: state.settings.clone(),
            film: film.clone(),
            history: vec![Step { label: "Opened".into(), key: "open".into(), settings: state.settings.clone(), film, at: now }],
            cursor: 0,
            applied: None,
            commands,
            engine: Arc::new(Mutex::new(Engine::new())),
            preview: false,
            full: false,
            used: now,
            full_used: now,
        })
    }

    fn reopen(&mut self) -> Result<(), String> {
        self.commands.open(&self.facts.to_string(), &self.settings.to_string())
    }

    // why: the same history rules as Develop's commit; agent keys are unique, so only a profile re-applied within
    // the coalescing window replaces its own step
    fn commit(&mut self, settings: Value, label: &str, key: &str, film: Option<FilmSettings>) {
        if key.starts_with("profile") {
            let _ = self.commands.open(&self.facts.to_string(), &settings.to_string());
        }
        self.history.truncate(self.cursor + 1);
        let now = Instant::now();
        if self.history.last().is_some_and(|last| last.key == key && now.duration_since(last.at) < COALESCE) {
            self.history.pop();
        }
        self.history.push(Step { label: label.to_string(), key: key.to_string(), settings: settings.clone(), film: film.clone(), at: now });
        self.cursor = self.history.len() - 1;
        self.settings = settings;
        self.film = film;
    }

    fn try_command(&mut self, command: &str, params: &Value, label: &str, key: &str) -> Result<(Value, bool), String> {
        let out = self.commands.run(command, &params.to_string())?;
        let mut parsed: Value = serde_json::from_str(&out).map_err(|e| e.to_string())?;
        let changed = parsed["settings"] != self.settings;
        if changed {
            let film = self.film.clone();
            self.commit(parsed["settings"].take(), label, key, film);
        }
        Ok((parsed["result"].take(), changed))
    }

    fn go_to(&mut self, index: usize) -> Result<(), String> {
        let step = self.history.get(index).cloned().ok_or("no such step")?;
        self.settings = step.settings;
        self.film = step.film;
        self.cursor = index;
        self.reopen()
    }

    // why: port of Develop::apply_profile; re-applying the same profile at the same step starts from the settings it
    // first saw, so the amount replaces the earlier application
    fn apply_profile(&mut self, profile_id: &str, partial: &Value, amount: f64, label: &str, film: Option<FilmSettings>) -> Result<(), String> {
        let before = match &self.applied {
            Some(a) if a.profile_id == profile_id && a.step == self.cursor => a.before.clone(),
            _ => self.settings.clone(),
        };
        let next: Value = serde_json::from_str(&core::apply_profile(&before.to_string(), &partial.to_string(), amount)?).map_err(|e| e.to_string())?;
        let film = film.or_else(|| self.film.clone());
        self.commit(next, label, &format!("profile:{profile_id}"), film);
        self.applied = Some(Applied { profile_id: profile_id.to_string(), before, step: self.cursor, photo_id: Some(self.id.clone()) });
        Ok(())
    }

    fn small(&self) -> bool {
        long_edge(&self.facts) <= PREVIEW_EDGE
    }
}

/// A read-only view of one photo's edit, from Develop, an agent session or the library record.
pub struct Edit<'a> {
    pub id: &'a str,
    pub settings: &'a Value,
    pub before: Option<&'a Value>,
    pub facts: Option<&'a Value>,
    pub film: Option<&'a FilmSettings>,
    pub history: &'a [Step],
    pub cursor: usize,
}

impl Edit<'_> {
    pub fn values(&self) -> HashMap<String, f64> {
        core::control_values(&self.settings.to_string()).ok().and_then(|j| serde_json::from_str(&j).ok()).unwrap_or_default()
    }
}

/// What a call works on: the photo open in the window (through Develop), or an agent's own session of another photo.
pub enum On {
    Ui,
    Agent(Rc<RefCell<Session>>),
}

/// A call's hold on its photo: the window's lock when it acts on what the user sees, the photo's lock when it names
/// one. Calls on one photo run in order; calls on different photos run side by side.
pub struct Held {
    pub on: On,
    _photo: Option<OwnedMutexGuard<()>>,
    _ui: Option<Viewing>,
}

/// The window's lock, held by calls that drive what the user sees.
pub struct Viewing {
    _guard: OwnedMutexGuard<()>,
}

impl Viewing {
    fn new(guard: OwnedMutexGuard<()>) -> Viewing {
        Viewing { _guard: guard }
    }
}

fn ui_photo(driver: &Driver, cx: &mut AsyncApp) -> Option<String> {
    driver.read(cx, |a, cx| a.develop.read(cx).photo_id.clone()).flatten()
}

fn develop_edit<R>(driver: &Driver, cx: &mut AsyncApp, f: impl FnOnce(Edit<'_>) -> R) -> Option<R> {
    driver
        .read(cx, |a, cx| {
            let d = a.develop.read(cx);
            Some(f(Edit { id: d.photo_id.as_deref()?, settings: d.settings.as_ref()?, before: d.before.as_ref(), facts: d.facts.as_ref(), film: d.film.as_ref(), history: &d.history, cursor: d.cursor }))
        })
        .flatten()
}

fn session_edit<R>(s: &Session, f: impl FnOnce(Edit<'_>) -> R) -> R {
    f(Edit { id: &s.id, settings: &s.settings, before: Some(&s.before), facts: Some(&s.facts), film: s.film.as_ref(), history: &s.history, cursor: s.cursor })
}

/// Saves a session's edit to the photo's record and sidecar (which also redraws its thumbnail), keeping the flags,
/// rating and label the library has now.
fn persist(driver: &Driver, cx: &mut AsyncApp, s: &Session) -> Result<(), String> {
    let (id, folder, settings) = (s.id.clone(), s.folder.clone(), s.settings.clone());
    let film = s.film.as_ref().map(|f| f.normalized().to_value());
    driver
        .act(cx, move |a, cx| {
            a.library.update(cx, |l, cx| {
                if l.folder.as_ref() != Some(&folder) {
                    return Err(format!("the library moved to another folder, so {id} wasn't saved; open_folder {} again", folder.display()));
                }
                let state = l.record(&id).and_then(|r| r.state.clone()).ok_or_else(|| format!("{id} left the library"))?;
                l.persist(&id, PhotoState { settings, film, ..state }, cx);
                Ok(())
            })
        })
        .ok_or(GONE)?
}

impl Held {
    pub fn is_ui(&self) -> bool {
        matches!(self.on, On::Ui)
    }

    /// The id of the photo this call acts on.
    pub fn id(&self, driver: &Driver, cx: &mut AsyncApp) -> Option<String> {
        match &self.on {
            On::Ui => ui_photo(driver, cx),
            On::Agent(s) => Some(s.borrow().id.clone()),
        }
    }

    pub fn read<R>(&self, driver: &Driver, cx: &mut AsyncApp, f: impl FnOnce(Edit<'_>) -> R) -> Option<R> {
        match &self.on {
            On::Ui => develop_edit(driver, cx, f),
            On::Agent(s) => Some(session_edit(&s.borrow(), f)),
        }
    }

    async fn changed(&self, driver: &Driver, cx: &mut AsyncApp) -> Result<(), String> {
        let On::Agent(s) = &self.on else { return Ok(()) };
        persist(driver, cx, &s.borrow())
    }

    /// Runs a LightCraft command as one history step when it changes the photo; returns its result and whether it did.
    pub async fn command(&self, driver: &Driver, cx: &mut AsyncApp, command: &str, params: Value, label: &str, key: &str) -> Result<(Value, bool), String> {
        let s = match &self.on {
            On::Ui => {
                let (command, label, key) = (command.to_string(), label.to_string(), key.to_string());
                return driver.develop(cx, move |d, cx| d.try_command(&command, params, &label, &key, cx)).ok_or(GONE)?;
            }
            On::Agent(s) => s.clone(),
        };
        let first = s.borrow_mut().try_command(command, &params, label, key);
        // why: the session reads the raw file only for the commands that need its pixels (auto tone, subject masks),
        // and lets go of it after
        let out = match first {
            Err(e) if e.contains(NO_PIXELS) => {
                let path = s.borrow().path.clone();
                let bytes = cx.background_spawn(async move { std::fs::read(&path).map_err(|e| e.to_string()) }).await?;
                let mut session = s.borrow_mut();
                session.reopen()?;
                session.commands.attach(bytes);
                let out = session.try_command(command, &params, label, key);
                session.commands.detach();
                out
            }
            other => other,
        }?;
        if out.1 {
            self.changed(driver, cx).await?;
        }
        Ok(out)
    }

    pub async fn set_film(&self, driver: &Driver, cx: &mut AsyncApp, film: FilmSettings, label: &str, key: &str) -> Result<(), String> {
        match &self.on {
            On::Ui => {
                let (label, key) = (label.to_string(), key.to_string());
                driver.develop(cx, move |d, cx| d.edit_film(film, &label, &key, cx)).ok_or(GONE)?;
            }
            On::Agent(s) => {
                let settings = s.borrow().settings.clone();
                s.borrow_mut().commit(settings, label, key, Some(film));
                self.changed(driver, cx).await?;
            }
        }
        Ok(())
    }

    pub async fn go_to(&self, driver: &Driver, cx: &mut AsyncApp, index: usize) -> Result<(), String> {
        match &self.on {
            On::Ui => driver.develop(cx, |d, cx| d.go_to(index, cx)).ok_or(GONE)?,
            On::Agent(s) => {
                s.borrow_mut().go_to(index)?;
                self.changed(driver, cx).await?;
            }
        }
        Ok(())
    }

    pub async fn restore(&self, driver: &Driver, cx: &mut AsyncApp, settings: Value, film: Option<FilmSettings>, label: &str, key: &str) -> Result<(), String> {
        match &self.on {
            On::Ui => {
                let (label, key) = (label.to_string(), key.to_string());
                driver.develop(cx, move |d, cx| d.restore(settings, film, &label, &key, cx)).ok_or(GONE)??;
            }
            On::Agent(s) => {
                {
                    let mut session = s.borrow_mut();
                    let facts = session.facts.to_string();
                    session.commands.open(&facts, &settings.to_string())?;
                    session.commit(settings, label, key, film);
                }
                self.changed(driver, cx).await?;
            }
        }
        Ok(())
    }

    pub async fn apply_profile(&self, driver: &Driver, cx: &mut AsyncApp, profile: &crate::ui::rail::store::Profile, amount: f64) -> Result<(), String> {
        let film = profile.film.as_ref().and_then(FilmSettings::of);
        let label = format!("Profile {}", profile.name);
        match &self.on {
            On::Ui => {
                let (id, partial) = (profile.id.clone(), profile.partial.clone());
                driver
                    .act(cx, move |a, cx| {
                        crate::ui::rail::store::set_amount(&mut a.rail, amount * 100.0);
                        a.develop.update(cx, |d, cx| d.apply_profile(&id, &partial, amount, &label, film, cx));
                        cx.notify();
                    })
                    .ok_or(GONE)?;
            }
            On::Agent(s) => {
                s.borrow_mut().apply_profile(&profile.id, &profile.partial, amount, &label, film)?;
                self.changed(driver, cx).await?;
            }
        }
        Ok(())
    }

    /// What an off-screen render of the photo needs, decoding it first if it isn't yet (`full`: at full size).
    pub async fn render_job(&self, driver: &Driver, cx: &mut AsyncApp, before: bool, full: bool) -> Result<RenderJob, String> {
        let s = match &self.on {
            On::Ui => return develop_job(driver, cx, before, full).await,
            On::Agent(s) => s.clone(),
        };
        let level = if full && !s.borrow().small() { Level::Full } else { Level::Preview };
        let ready = |s: &Session| if level == Level::Full { s.full } else { s.preview || s.full };
        if !ready(&s.borrow()) {
            let (engine, path, facts, id) = {
                let s = s.borrow();
                (s.engine.clone(), s.path.clone(), s.facts.to_string(), s.id.clone())
            };
            let key = source_key(&id, level);
            let started = Instant::now();
            // why: the decode runs in the pool without the engine's lock, which is taken after, on this thread; an
            // agent waits on it like the loupe does, so it runs at the loupe's priority
            cx.background_spawn(async move {
                let bytes = std::fs::read(&path).map_err(|e| e.to_string())?;
                let decoded = crate::perf::interactive(|| core::decode(&facts, &bytes, level.max_edge()))?;
                engine.lock().map_err(|e| e.to_string())?.insert(&key, decoded);
                Ok::<(), String>(())
            })
            .await?;
            eprintln!("mcp: agent session {id} decoded {} in {} ms", level.tag(), started.elapsed().as_millis());
            let mut session = s.borrow_mut();
            match level {
                Level::Full => session.full = true,
                Level::Preview => session.preview = true,
            }
        }
        let mut session = s.borrow_mut();
        let now = Instant::now();
        session.used = now;
        let level = if session.full { Level::Full } else { Level::Preview };
        if level == Level::Full {
            session.full_used = now;
        }
        let job = RenderJob {
            engine: session.engine.clone(),
            key: source_key(&session.id, level),
            full: level == Level::Full,
            settings: if before { session.before.clone() } else { session.settings.clone() },
            film: if before { None } else { render_film(session.film.as_ref()) },
            facts: session.facts.clone(),
        };
        Ok(job)
    }
}

async fn develop_job(driver: &Driver, cx: &mut AsyncApp, before: bool, full: bool) -> Result<RenderJob, String> {
    if ui_photo(driver, cx).is_none() {
        return Err(NO_PHOTO.into());
    }
    if full {
        driver.develop(cx, |d, cx| d.ensure_full(cx));
        if !driver.wait(cx, 120, |a, cx| a.develop.read(cx).full_ready()).await {
            return Err("the full-resolution decode didn't finish within 120 s".into());
        }
    }
    let wanted = move |a: &crate::ui::app::FilmlabApp, cx: &gpui_kit::App| a.develop.read(cx).render_job(before).is_some_and(|j| j.full || !full);
    if !driver.wait(cx, 60, wanted).await {
        return Err("the photo isn't decoded yet".into());
    }
    driver.read(cx, |a, cx| a.develop.read(cx).render_job(before)).flatten().ok_or_else(|| "the photo closed".into())
}

/// Every agent call's way to its photo: the window's lock, a lock per photo, and the live off-screen sessions.
#[derive(Default)]
pub struct Hub {
    ui: Arc<Queue<()>>,
    locks: RefCell<HashMap<String, Arc<Queue<()>>>>,
    sessions: RefCell<Vec<Rc<RefCell<Session>>>>,
    sweeping: Cell<bool>,
}

impl Hub {
    fn lock_of(&self, id: &str) -> Arc<Queue<()>> {
        self.locks.borrow_mut().entry(id.to_string()).or_default().clone()
    }

    /// Waits for the window: for calls that drive what the user sees.
    pub async fn window(&self) -> Viewing {
        Viewing::new(self.ui.clone().lock_owned().await)
    }

    /// Waits for the photo's earlier calls to finish.
    pub async fn photo(&self, id: &str) -> OwnedMutexGuard<()> {
        self.lock_of(id).lock_owned().await
    }

    /// The photos agents have open off screen, most recently used last.
    pub fn live(&self) -> Vec<String> {
        self.sessions.borrow().iter().map(|s| s.borrow().id.clone()).collect()
    }

    fn find(&self, id: &str) -> Option<Rc<RefCell<Session>>> {
        self.sessions.borrow().iter().find(|s| s.borrow().id == id).cloned()
    }

    /// Takes the call's photo: the open one (`photo` omitted or naming it) through Develop, any other through its
    /// own session. Locks go window first, then photo, so two calls never wait on each other; a call on the open
    /// photo also waits for a session call on it that was running when the user opened it, and takes over its history.
    pub async fn hold(self: &Rc<Self>, driver: &Driver, cx: &mut AsyncApp, photo: Option<&str>) -> Result<Held, String> {
        if let Some(id) = photo {
            let known = driver.read(cx, |a, cx| a.library.read(cx).record(id).is_some()).ok_or(GONE)?;
            if !known {
                return Err(format!("no photo {id} in the open folder; list_photos gives the ids"));
            }
        }
        loop {
            let shown = ui_photo(driver, cx);
            let target = photo.map(str::to_string).or_else(|| shown.clone());
            let ui = if photo.is_none() || shown == target { Some(self.window().await) } else { None };
            let lock = match &target {
                Some(id) => Some(self.photo(id).await),
                None => None,
            };
            let now = ui_photo(driver, cx);
            if now == target && ui.is_some() {
                if let Some(id) = &now {
                    self.hand_over(driver, cx, id);
                }
                return Ok(Held { on: On::Ui, _photo: lock, _ui: ui });
            }
            if let (Some(id), false) = (photo, now == target) {
                let session = self.session(driver, cx, id)?;
                return Ok(Held { on: On::Agent(session), _photo: lock, _ui: None });
            }
        }
    }

    fn session(self: &Rc<Self>, driver: &Driver, cx: &mut AsyncApp, id: &str) -> Result<Rc<RefCell<Session>>, String> {
        if let Some(s) = self.find(id) {
            s.borrow_mut().used = Instant::now();
            let mut sessions = self.sessions.borrow_mut();
            sessions.retain(|x| !Rc::ptr_eq(x, &s));
            sessions.push(s.clone());
            return Ok(s);
        }
        let opened = driver
            .read(cx, |a, cx| {
                let l = a.library.read(cx);
                let folder = l.folder.clone().ok_or("no folder is open; open_folder first")?;
                Session::open(l.record(id).ok_or_else(|| format!("no photo {id} in the open folder"))?, folder)
            })
            .ok_or(GONE)??;
        let session = Rc::new(RefCell::new(opened));
        self.sessions.borrow_mut().push(session.clone());
        self.evict(cx);
        eprintln!("mcp: agent session {id} opened ({} live)", self.sessions.borrow().len());
        self.sweep_soon(driver, cx);
        Ok(session)
    }

    // why: the least recently used idle sessions go first; a busy one stays until its call ends and the sweep sees it
    fn evict(&self, cx: &mut AsyncApp) {
        let over = self.sessions.borrow().len().saturating_sub(LIVE);
        let idle: Vec<String> = self.sessions.borrow().iter().map(|s| s.borrow().id.clone()).filter(|id| self.lock_of(id).try_lock().is_ok()).take(over).collect();
        for id in idle {
            self.close(cx, &id, "over the limit");
        }
    }

    fn close(&self, cx: &mut AsyncApp, id: &str, why: &str) {
        let Some(s) = self.find(id) else { return };
        self.sessions.borrow_mut().retain(|x| !Rc::ptr_eq(x, &s));
        let engine = s.borrow().engine.clone();
        drop(s);
        eprintln!("mcp: agent session {id} closed, {why} ({} live)", self.sessions.borrow().len());
        // why: the decodes are tens to hundreds of MB; they are freed off the main thread and handed back to macOS
        cx.background_spawn(async move {
            drop(engine);
            crate::perf::give_back();
        })
        .detach();
    }

    /// Ends the photo's session once the window shows it, giving Develop the session's history so the user can undo
    /// the agent's steps, and its latest state if a change landed after Develop read the library. The caller holds
    /// the photo's lock (or knows no call is running on it).
    pub fn hand_over(&self, driver: &Driver, cx: &mut AsyncApp, id: &str) {
        let Some(s) = self.find(id) else { return };
        let (history, cursor, settings, film) = {
            let s = s.borrow();
            (s.history.clone(), s.cursor, s.settings.clone(), s.film.as_ref().map(FilmSettings::normalized))
        };
        let photo = id.to_string();
        driver.develop(cx, move |d, cx| {
            if d.photo_id.as_deref() != Some(photo.as_str()) || d.history.len() != 1 {
                return;
            }
            let same = d.settings.as_ref() == Some(&settings) && d.film.as_ref().map(FilmSettings::normalized) == film;
            d.history = history;
            if same {
                d.cursor = cursor;
                d.remark();
                cx.notify();
            } else {
                d.go_to(cursor, cx);
            }
        });
        self.close(cx, id, "now open in the window");
    }

    /// Ends every session (their edits are already saved), waiting for each one's running call; for a folder change.
    pub async fn close_all(&self, cx: &mut AsyncApp) {
        for id in self.live() {
            let _lock = self.photo(&id).await;
            self.close(cx, &id, "the folder changed");
        }
    }

    fn sweep_soon(self: &Rc<Self>, driver: &Driver, cx: &mut AsyncApp) {
        if self.sweeping.replace(true) {
            return;
        }
        let (hub, driver) = (self.clone(), driver.clone());
        cx.spawn(async move |cx| {
            while !hub.sessions.borrow().is_empty() {
                cx.background_executor().timer(SWEEP).await;
                hub.sweep(&driver, cx);
            }
            hub.sweeping.set(false);
        })
        .detach();
    }

    fn sweep(&self, driver: &Driver, cx: &mut AsyncApp) {
        let shown = ui_photo(driver, cx);
        let folder = driver.read(cx, |a, cx| a.library.read(cx).folder.clone()).flatten();
        for id in self.live() {
            let lock = self.lock_of(&id);
            let Ok(_held) = lock.try_lock() else { continue };
            let Some(s) = self.find(&id) else { continue };
            let (idle, stale, full_idle, preview_idle) = {
                let s = s.borrow();
                (s.used.elapsed() > IDLE, folder.as_ref() != Some(&s.folder), s.full && s.full_used.elapsed() > FULL_IDLE, s.preview && s.used.elapsed() > PREVIEW_IDLE)
            };
            if shown.as_deref() == Some(id.as_str()) {
                self.hand_over(driver, cx, &id);
            } else if stale {
                self.close(cx, &id, "the folder changed");
            } else if idle {
                self.close(cx, &id, "idle");
            } else if full_idle || preview_idle {
                // why: an agent's session keeps its history until it closes, but its decodes (tens of MB, hundreds
                // at full size) only while it's being worked on; the next call decodes again
                let mut s = s.borrow_mut();
                let gone: &[Level] = if preview_idle { &[Level::Preview, Level::Full] } else { &[Level::Full] };
                s.full = false;
                s.preview &= !preview_idle;
                if let Ok(mut engine) = s.engine.lock() {
                    for level in gone {
                        engine.release(&source_key(&id, *level));
                    }
                }
                crate::perf::give_back();
            }
        }
        self.locks.borrow_mut().retain(|_, lock| Arc::strong_count(lock) > 1 || lock.try_lock().is_err());
        // why: a decode or render leaves its scratch memory with the pool threads, as Develop's do, until handed back
        if self.sessions.borrow().iter().any(|s| s.borrow().used.elapsed() < SWEEP * 2) {
            crate::perf::give_back();
        }
    }

    /// Hands a photo over once the user opens it in the window, after any running call on it ends.
    pub fn watch(self: &Rc<Self>, driver: &Driver, cx: &mut AsyncApp, id: String) {
        if self.find(&id).is_none() {
            return;
        }
        let (hub, driver) = (self.clone(), driver.clone());
        cx.spawn(async move |cx| {
            let _lock = hub.photo(&id).await;
            if ui_photo(&driver, cx).as_deref() == Some(id.as_str()) {
                hub.hand_over(&driver, cx, &id);
            }
        })
        .detach();
    }

    /// Reads a photo's edit without waiting on any call: Develop's for the open photo, a session's, or the library's.
    pub fn peek<R>(&self, driver: &Driver, cx: &mut AsyncApp, photo: Option<&str>, f: impl FnOnce(Edit<'_>) -> R) -> Result<R, String> {
        let Some(id) = photo.filter(|id| ui_photo(driver, cx).as_deref() != Some(*id)) else {
            return develop_edit(driver, cx, f).ok_or_else(|| NO_PHOTO.into());
        };
        if let Some(s) = self.find(id) {
            return Ok(session_edit(&s.borrow(), f));
        }
        driver
            .read(cx, |a, cx| {
                let record = a.library.read(cx).record(id).ok_or_else(|| format!("no photo {id} in the open folder; list_photos gives the ids"))?;
                let (Some(probed), Some(state)) = (&record.probed, &record.state) else { return Err(format!("{id} is still being indexed; try again in a moment")) };
                let film = state.film();
                Ok(f(Edit { id, settings: &state.settings, before: Some(&probed.defaults), facts: Some(&probed.facts), film: film.as_ref(), history: &[], cursor: 0 }))
            })
            .ok_or(GONE)?
    }
}

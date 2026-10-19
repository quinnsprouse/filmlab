pub mod run;
mod view;

use std::collections::VecDeque;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use filmlab_engine::core::Engine;
use gpui_kit::{AppContext, Context, FocusHandle, Window};

use crate::ui::app::{FilmlabApp, Mode};
use run::{Choices, Job};

const WORKERS: usize = 2;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Phase {
    Idle,
    Running,
    Done,
}

pub struct State {
    open: bool,
    phase: Phase,
    choices: Choices,
    focus: Option<FocusHandle>,
    previous: Option<FocusHandle>,
    token: u64,
    cancel: Arc<AtomicBool>,
    workers: usize,
    total: usize,
    finished: usize,
    exported: Vec<PathBuf>,
    errors: Vec<String>,
}

impl Default for State {
    fn default() -> Self {
        State {
            open: false,
            phase: Phase::Idle,
            choices: Choices::default(),
            focus: None,
            previous: None,
            token: 0,
            cancel: Arc::default(),
            workers: 0,
            total: 0,
            finished: 0,
            exported: Vec::new(),
            errors: Vec::new(),
        }
    }
}

impl State {
    pub fn is_open(&self) -> bool {
        self.open
    }

    /// The exported files and the errors, once a run has finished.
    pub fn outcome(&self) -> Option<(&[PathBuf], &[String])> {
        (self.phase == Phase::Done).then_some((self.exported.as_slice(), self.errors.as_slice()))
    }

    fn stopped(&self) -> bool {
        self.cancel.load(Ordering::Relaxed)
    }

    fn record(&mut self, name: &str, result: Result<PathBuf, String>) {
        self.finished += 1;
        match result {
            Ok(path) => self.exported.push(path),
            Err(e) => self.errors.push(format!("{name}: {e}")),
        }
    }
}

pub fn dialog(app: &mut FilmlabApp, window: &mut Window, cx: &mut Context<FilmlabApp>) -> Option<gpui_kit::AnyElement> {
    view::dialog(app, window, cx)
}

pub fn open(app: &mut FilmlabApp, window: &mut Window, cx: &mut Context<FilmlabApp>) {
    if app.export.open {
        return;
    }
    let focus = cx.focus_handle();
    let state = &mut app.export;
    state.previous = window.focused(cx);
    state.open = true;
    state.phase = Phase::Idle;
    state.exported.clear();
    state.errors.clear();
    window.focus(&focus, cx);
    state.focus = Some(focus);
    cx.notify();
}

// why: the web app exports the open photo in Develop and the selection in Browse, or every photo when none is
// selected
fn ids(app: &FilmlabApp, cx: &gpui_kit::App) -> Vec<String> {
    if app.mode == Mode::Develop {
        let open = app.develop.read(cx).photo_id.clone();
        return open.or_else(|| app.selected.clone()).into_iter().collect();
    }
    match app.targets() {
        targets if targets.is_empty() => app.library.read(cx).photos.iter().map(|r| r.file.id.clone()).collect(),
        targets => targets,
    }
}

fn close(app: &mut FilmlabApp, window: &mut Window, cx: &mut Context<FilmlabApp>) {
    let state = &mut app.export;
    state.cancel.store(true, Ordering::Relaxed);
    state.token += 1;
    state.open = false;
    state.phase = Phase::Idle;
    state.focus = None;
    if let Some(previous) = state.previous.take() {
        window.focus(&previous, cx);
    }
    cx.notify();
}

fn back(app: &mut FilmlabApp, cx: &mut Context<FilmlabApp>) {
    app.export.phase = Phase::Idle;
    app.export.errors.clear();
    app.export.exported.clear();
    cx.notify();
}

fn start(app: &mut FilmlabApp, cx: &mut Context<FilmlabApp>) {
    if app.export.phase != Phase::Idle {
        return;
    }
    let ids = ids(app, cx);
    if ids.is_empty() {
        return;
    }
    app.develop.update(cx, |d, cx| d.flush(cx));
    let planned = run::jobs_for(app.library.read(cx), app.develop.read(cx), &ids, &app.export.choices);
    let state = &mut app.export;
    state.token += 1;
    state.cancel = Arc::default();
    state.phase = Phase::Running;
    state.total = ids.len();
    state.finished = 0;
    state.exported.clear();
    state.errors.clear();
    let mut queue = VecDeque::new();
    for (id, job) in planned {
        match job {
            Ok(job) => queue.push_back(job),
            Err(e) => state.record(&id, Err(e)),
        }
    }
    state.workers = WORKERS.min(queue.len());
    if state.workers == 0 {
        state.phase = Phase::Done;
    }
    let queue = Arc::new(Mutex::new(queue));
    let run = Worker { token: app.export.token, cancel: app.export.cancel.clone(), choices: app.export.choices };
    for _ in 0..app.export.workers {
        spawn_worker(queue.clone(), run.clone(), cx);
    }
    cx.notify();
}

#[derive(Clone)]
struct Worker {
    token: u64,
    cancel: Arc<AtomicBool>,
    choices: Choices,
}

// why: `start` runs inside an update of the app, so a worker takes the run's token, cancel flag and choices from it
// rather than reading the app entity again (GPUI panics on reading an entity mid-update)
fn spawn_worker(queue: Arc<Mutex<VecDeque<Job>>>, run: Worker, cx: &mut Context<FilmlabApp>) {
    let Worker { token, cancel, choices } = run;
    cx.spawn(async move |this, cx| {
        let engine = Arc::new(Mutex::new(Engine::new()));
        while !cancel.load(Ordering::Relaxed) {
            let Some(job) = queue.lock().ok().and_then(|mut q| q.pop_front()) else { break };
            let (engine, task) = (engine.clone(), job.clone());
            let result = cx
                .background_spawn(async move {
                    let decoded = task.loupe().and_then(run::Loupe::decoded);
                    let mut engine = engine.lock().map_err(|e| e.to_string())?;
                    let engine = &mut *engine;
                    crate::perf::initiated(|| run::export_one(engine, &task, &choices, None, decoded))
                })
                .await;
            let alive = this.update(cx, |app, cx| {
                if app.export.token != token {
                    return false;
                }
                app.export.record(&job.name, result);
                cx.notify();
                true
            });
            if !alive.unwrap_or(false) {
                return;
            }
        }
        let _ = this.update(cx, |app, cx| {
            if app.export.token == token {
                app.export.workers -= 1;
                if app.export.workers == 0 {
                    app.export.phase = Phase::Done;
                }
                cx.notify();
            }
        });
    })
    .detach();
}

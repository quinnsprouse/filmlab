mod prompts;
mod server;
pub mod session;
pub mod tools;

use std::rc::Rc;
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};

use futures::StreamExt;
use futures::channel::{mpsc, oneshot};
use gpui_kit::*;
use serde_json::Value;

use crate::ui::app::FilmlabApp;
use crate::ui::driver::Driver;

/// How long the top bar keeps saying an agent is developing after its last call, once it has edited: long enough to
/// cover the looking and thinking between edits, so a grading session reads as one stretch of work rather than
/// blinking with every change. Then how long it takes to fade.
pub const LINGER: Duration = Duration::from_secs(12);
pub const FADE: Duration = Duration::from_millis(200);

static EDITING: AtomicUsize = AtomicUsize::new(0);
static LAST: Mutex<Option<Instant>> = Mutex::new(None);
static AGENT: Mutex<String> = Mutex::new(String::new());
static VIEWERS: AtomicUsize = AtomicUsize::new(0);
static LAST_VIEW: Mutex<Option<Instant>> = Mutex::new(None);

/// Whether an agent is changing a photo: from its first edit, while its calls keep coming, until `LINGER` after the
/// last, then fading out. Looking, reading and listing never start it; it's on or off, with no progress.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Presence {
    Developing,
    Leaving,
}

fn after(last: &Mutex<Option<Instant>>) -> Option<Presence> {
    let since = last.lock().ok().and_then(|last| last.map(|t| t.elapsed()))?;
    if since < LINGER {
        Some(Presence::Developing)
    } else if since < LINGER + FADE {
        Some(Presence::Leaving)
    } else {
        None
    }
}

pub fn presence() -> Option<Presence> {
    if EDITING.load(Ordering::Relaxed) > 0 {
        return Some(Presence::Developing);
    }
    after(&LAST)
}

/// The agent's presence on the photo in the window only: edits to it, not to other photos.
pub fn on_view() -> Option<Presence> {
    if VIEWERS.load(Ordering::Relaxed) > 0 {
        return Some(Presence::Developing);
    }
    after(&LAST_VIEW)
}

/// The name the connected agent gives itself ("Claude", "Codex"), or "Agent".
pub fn agent() -> String {
    AGENT.lock().ok().map(|a| a.clone()).filter(|a| !a.is_empty()).unwrap_or_else(|| "Agent".into())
}

// why: a client names itself "claude-code" or "Codex CLI"; the header wants the product, capitalised
fn agent_name(client: &str) -> String {
    let lower = client.to_lowercase();
    for (needle, name) in [("claude", "Claude"), ("codex", "Codex"), ("cursor", "Cursor"), ("gemini", "Gemini"), ("copilot", "Copilot")] {
        if lower.contains(needle) {
            return name.into();
        }
    }
    let first = client.split(|c: char| !c.is_alphanumeric()).find(|w| !w.is_empty()).unwrap_or("Agent");
    let mut chars = first.chars();
    chars.next().map(|c| c.to_uppercase().chain(chars).collect()).unwrap_or_else(|| "Agent".into())
}

/// Calls that change a photo; everything else (previews, analysis, listing, opening) leaves the indicator off.
fn edits(name: &str, args: &Value) -> bool {
    let action = args["action"].as_str().unwrap_or_default();
    match name {
        "set_controls" | "set_film" | "command" => true,
        "history" => matches!(action, "undo" | "redo" | "go_to"),
        "snapshots" => action == "restore",
        "profiles" => action == "apply",
        _ => false,
    }
}

fn touch(driver: &Driver, cx: &mut AsyncApp) {
    driver.act(cx, |_, cx| cx.notify());
}

pub const DEFAULT_PORT: u16 = 47_428;

/// One tool call, handed from the HTTP server's thread to the app's main thread.
pub struct Call {
    pub name: String,
    pub args: Value,
    /// The client's own name for itself, when it gives one.
    pub client: Option<String>,
    pub reply: oneshot::Sender<tools::Output>,
}

pub fn port() -> u16 {
    std::env::var("FILMLAB_MCP_PORT").ok().and_then(|p| p.parse().ok()).unwrap_or(DEFAULT_PORT)
}

pub fn url(port: u16) -> String {
    format!("http://127.0.0.1:{port}/mcp")
}

/// Keeps a stretch of work going: an agent previewing or measuring between edits is still developing.
fn extend(photo: Option<&str>, driver: &Driver, cx: &mut AsyncApp) {
    if presence() != Some(Presence::Developing) {
        return;
    }
    let now = Some(Instant::now());
    if let Ok(mut last) = LAST.lock() {
        *last = now;
    }
    let shown = driver.read(cx, |a, cx| a.develop.read(cx).photo_id.clone()).flatten();
    if on_view() == Some(Presence::Developing)
        && photo.is_none_or(|p| Some(p) == shown.as_deref())
        && let Ok(mut last) = LAST_VIEW.lock()
    {
        *last = now;
    }
    settle(driver, cx);
}

/// Repaints when the stretch ends and when its fade finishes, so the words leave without waiting for another frame.
fn settle(driver: &Driver, cx: &mut AsyncApp) {
    let driver = driver.clone();
    cx.spawn(async move |cx| {
        cx.background_executor().timer(LINGER).await;
        touch(&driver, cx);
        cx.background_executor().timer(FADE + Duration::from_millis(40)).await;
        touch(&driver, cx);
    })
    .detach();
}

async fn answer(hub: &Rc<session::Hub>, driver: &Driver, cx: &mut AsyncApp, call: Call) {
    if !edits(&call.name, &call.args) {
        let photo = call.args["photo"].as_str().map(str::to_string);
        let output = tools::run(hub, driver, cx, &call.name, call.args).await;
        extend(photo.as_deref(), driver, cx);
        let _ = call.reply.send(output);
        return;
    }
    if let (Some(client), Ok(mut agent)) = (call.client.as_deref(), AGENT.lock()) {
        *agent = agent_name(client);
    }
    let shown = driver.read(cx, |a, cx| a.develop.read(cx).photo_id.clone()).flatten();
    let on_view = call.args["photo"].as_str().is_none_or(|p| Some(p) == shown.as_deref());
    EDITING.fetch_add(1, Ordering::Relaxed);
    if on_view {
        VIEWERS.fetch_add(1, Ordering::Relaxed);
    }
    touch(driver, cx);
    let output = tools::run(hub, driver, cx, &call.name, call.args).await;
    let now = Some(Instant::now());
    if let Ok(mut last) = LAST.lock() {
        *last = now;
    }
    EDITING.fetch_sub(1, Ordering::Relaxed);
    if on_view {
        if let Ok(mut last) = LAST_VIEW.lock() {
            *last = now;
        }
        VIEWERS.fetch_sub(1, Ordering::Relaxed);
    }
    touch(driver, cx);
    settle(driver, cx);
    let _ = call.reply.send(output);
}

// why: the editor's state lives in GPUI entities on the main thread, so the server only forwards calls; each runs as
// its own task there, and the hub's locks keep calls on one photo (or on the window) in order while decodes and
// renders of different photos run side by side on background threads
pub fn start(app: &Entity<FilmlabApp>, window: AnyWindowHandle, cx: &mut App) {
    if std::env::var("FILMLAB_MCP").is_ok_and(|v| v == "0") {
        return;
    }
    let (calls, mut queue) = mpsc::unbounded::<Call>();
    let driver = Driver { app: app.downgrade(), window };
    let hub = Rc::new(session::Hub::default());
    let develop = app.read(cx).develop.clone();
    let mut shown: Option<String> = None;
    let (watcher, watching) = (hub.clone(), driver.clone());
    cx.observe(&develop, move |develop, cx| {
        let id = develop.read(cx).photo_id.clone();
        if id == shown {
            return;
        }
        shown.clone_from(&id);
        if let Some(id) = id {
            let (hub, driver) = (watcher.clone(), watching.clone());
            cx.spawn(async move |cx| hub.watch(&driver, cx, id)).detach();
        }
    })
    .detach();
    cx.spawn(async move |cx| {
        while let Some(call) = queue.next().await {
            let (hub, driver) = (hub.clone(), driver.clone());
            cx.spawn(async move |cx| answer(&hub, &driver, cx, call).await).detach();
        }
    })
    .detach();
    let port = port();
    let spawned = std::thread::Builder::new().name("filmlab-mcp".into()).spawn(move || server::serve(port, calls));
    if let Err(e) = spawned {
        eprintln!("mcp: unable to start the server thread: {e}");
    }
}

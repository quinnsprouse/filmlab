mod prompts;
mod server;
pub mod tools;

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use futures::StreamExt;
use futures::channel::{mpsc, oneshot};
use gpui_kit::*;
use serde_json::Value;

use crate::ui::app::FilmlabApp;
use crate::ui::driver::Driver;

/// How long the top bar keeps showing the agent after its last call, and how long it then takes to fade.
pub const LINGER: Duration = Duration::from_secs(2);
pub const FADE: Duration = Duration::from_millis(180);

static BUSY: AtomicBool = AtomicBool::new(false);
static LAST: Mutex<Option<Instant>> = Mutex::new(None);
static TOOL: Mutex<String> = Mutex::new(String::new());

/// What the window shows of an agent: at work on a tool, resting after its last call, or fading out.
#[derive(Clone, PartialEq, Eq)]
pub enum Presence {
    Working(String),
    Resting,
    Leaving,
}

pub fn presence() -> Option<Presence> {
    if BUSY.load(Ordering::Relaxed) {
        return Some(Presence::Working(TOOL.lock().map(|t| t.clone()).unwrap_or_default()));
    }
    let since = LAST.lock().ok().and_then(|last| last.map(|t| t.elapsed()))?;
    if since < LINGER {
        Some(Presence::Resting)
    } else if since < LINGER + FADE {
        Some(Presence::Leaving)
    } else {
        None
    }
}

/// Whether an agent call is running or finished within `LINGER`.
pub fn active() -> bool {
    matches!(presence(), Some(Presence::Working(_) | Presence::Resting))
}

fn touch(driver: &Driver, cx: &mut AsyncApp) {
    driver.act(cx, |_, cx| cx.notify());
}

pub const DEFAULT_PORT: u16 = 47_428;

/// One tool call, handed from the HTTP server's thread to the app's main thread.
pub struct Call {
    pub name: String,
    pub args: Value,
    pub reply: oneshot::Sender<tools::Output>,
}

pub fn port() -> u16 {
    std::env::var("FILMLAB_MCP_PORT").ok().and_then(|p| p.parse().ok()).unwrap_or(DEFAULT_PORT)
}

pub fn url(port: u16) -> String {
    format!("http://127.0.0.1:{port}/mcp")
}

// why: the editor's state lives in GPUI entities on the main thread, so the server only forwards calls; they run one
// at a time, in order, against the same app the user sees
pub fn start(app: &Entity<FilmlabApp>, window: AnyWindowHandle, cx: &mut App) {
    if std::env::var("FILMLAB_MCP").is_ok_and(|v| v == "0") {
        return;
    }
    let (calls, mut queue) = mpsc::unbounded::<Call>();
    let driver = Driver { app: app.downgrade(), window };
    cx.spawn(async move |cx| {
        while let Some(call) = queue.next().await {
            if let Ok(mut tool) = TOOL.lock() {
                tool.clone_from(&call.name);
            }
            BUSY.store(true, Ordering::Relaxed);
            touch(&driver, cx);
            let output = tools::run(&driver, cx, &call.name, call.args).await;
            if let Ok(mut last) = LAST.lock() {
                *last = Some(Instant::now());
            }
            BUSY.store(false, Ordering::Relaxed);
            touch(&driver, cx);
            let driver = driver.clone();
            cx.spawn(async move |cx| {
                cx.background_executor().timer(LINGER).await;
                touch(&driver, cx);
                cx.background_executor().timer(FADE + Duration::from_millis(40)).await;
                touch(&driver, cx);
            })
            .detach();
            let _ = call.reply.send(output);
        }
    })
    .detach();
    let port = port();
    let spawned = std::thread::Builder::new().name("filmlab-mcp".into()).spawn(move || server::serve(port, calls));
    if let Err(e) = spawned {
        eprintln!("mcp: unable to start the server thread: {e}");
    }
}

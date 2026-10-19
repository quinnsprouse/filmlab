use std::time::{Duration, Instant};

use gpui_kit::*;

use super::app::FilmlabApp;
use crate::film::FilmSettings;

/// Drives the app from an async task: reads and updates its state, and sends key and mouse events to its window
/// only (never to the system's cursor or keyboard). The end-to-end run and the MCP server both use it.
#[derive(Clone)]
pub struct Driver {
    pub app: WeakEntity<FilmlabApp>,
    pub window: AnyWindowHandle,
}

fn mouse(window: &mut Window, cx: &mut App, at: Point<Pixels>, down: bool, clicks: usize) {
    let event = if down {
        PlatformInput::MouseDown(MouseDownEvent { button: MouseButton::Left, position: at, modifiers: Modifiers::default(), click_count: clicks, first_mouse: false })
    } else {
        PlatformInput::MouseUp(MouseUpEvent { button: MouseButton::Left, position: at, modifiers: Modifiers::default(), click_count: clicks })
    };
    window.dispatch_event(event, cx);
}

impl Driver {
    pub fn read<R>(&self, cx: &mut AsyncApp, f: impl FnOnce(&FilmlabApp, &App) -> R) -> Option<R> {
        self.app.read_with(cx, f).ok()
    }

    pub fn act<R>(&self, cx: &mut AsyncApp, f: impl FnOnce(&mut FilmlabApp, &mut Context<FilmlabApp>) -> R) -> Option<R> {
        self.app.update(cx, f).ok()
    }

    pub fn on_window<R>(&self, cx: &mut AsyncApp, f: impl FnOnce(&mut Window, &mut App) -> R) -> Option<R> {
        cx.update_window(self.window, |_, window, cx| f(window, cx)).ok()
    }

    pub fn key(&self, cx: &mut AsyncApp, keys: &str) -> bool {
        let Ok(stroke) = Keystroke::parse(keys) else { return false };
        self.on_window(cx, |window, cx| window.dispatch_keystroke(stroke, cx)).unwrap_or(false)
    }

    pub fn click(&self, cx: &mut AsyncApp, at: Point<Pixels>, clicks: usize) {
        self.on_window(cx, |window, cx| {
            for n in 1..=clicks {
                mouse(window, cx, at, true, n);
                mouse(window, cx, at, false, n);
            }
        });
    }

    pub fn drag(&self, cx: &mut AsyncApp, from: Point<Pixels>, to: Point<Pixels>) {
        self.on_window(cx, |window, cx| {
            mouse(window, cx, from, true, 1);
            for step in 1..=8 {
                let t = step as f32 / 8.0;
                let position = point(from.x + (to.x - from.x) * t, from.y + (to.y - from.y) * t);
                window.dispatch_event(PlatformInput::MouseMove(MouseMoveEvent { position, pressed_button: Some(MouseButton::Left), modifiers: Modifiers::default() }), cx);
            }
            mouse(window, cx, to, false, 1);
        });
    }

    pub async fn pause(&self, cx: &mut AsyncApp, ms: u64) {
        cx.background_executor().timer(Duration::from_millis(ms)).await;
    }

    pub async fn wait(&self, cx: &mut AsyncApp, seconds: u64, done: impl Fn(&FilmlabApp, &App) -> bool) -> bool {
        let until = Instant::now() + Duration::from_secs(seconds);
        while Instant::now() < until {
            if self.read(cx, &done).unwrap_or(false) {
                return true;
            }
            self.pause(cx, 50).await;
        }
        false
    }

    // why: a frame counts as settled once the loupe has rendered the latest revision and gone quiet
    pub async fn settled(&self, cx: &mut AsyncApp, detail: bool) -> bool {
        self.wait(cx, 60, move |a, cx| {
            let d = a.develop.read(cx);
            let frame = if detail { d.detail.is_some() } else { d.fit.is_some() };
            frame && !d.drafting() && d.idle_for(Duration::from_millis(500)) && d.shown_plan(detail).is_some_and(|p| p.revision == d.planned_revision())
        })
        .await
    }

    pub fn develop<R>(&self, cx: &mut AsyncApp, f: impl FnOnce(&mut crate::develop::Develop, &mut Context<crate::develop::Develop>) -> R) -> Option<R> {
        self.act(cx, |a, cx| a.develop.update(cx, f))
    }

    pub fn film(&self, cx: &mut AsyncApp) -> Option<FilmSettings> {
        self.read(cx, |a, cx| a.develop.read(cx).film.clone()).flatten()
    }
}

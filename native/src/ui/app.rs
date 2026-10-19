use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::Arc;

use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use super::glass_layer::GlassLayer;
use super::tools::{self, Tools};
use super::{browse, export, rail};
use super::inspector::{Ui, inspector};
use super::{icon, theme};
use crate::camera::{Rect, Size, Zoom, fit_scale, pan_by, zoom_around, zoom_of};
use crate::develop::{Compare, Develop, DisplayFrame, Status, Tool};
use crate::gpu::Gpu;
use crate::library::Library;
use crate::stocks::{self, Stock};
use crate::viewer::{Frame, Layer, Scene, Viewer};

actions!(
    filmlab,
    [
        OpenFolder, Undo, Redo, ShowBrowse, ShowDevelop, NextPhoto, PreviousPhoto, ToggleZoom, ToggleBefore, Quit, EditTool, CropTool, HealTool, MasksTool, Export, ToggleRail,
        FlagPick, FlagReject, Unflag, SelectAll, ClearSelection, OpenFocused, MoveUp, MoveDown, ExtendLeft, ExtendRight, ExtendUp, ExtendDown
    ]
);

pub(crate) const TOP: f32 = 56.0;
pub(crate) const MARGIN: f32 = 10.0;
pub(crate) const INSPECTOR: f32 = 304.0;
const RAIL: f32 = 236.0;
const STRIP: f32 = 88.0;
const PINCH_SPEED: f64 = 1.0;
const WHEEL_SPEED: f64 = 0.01;
const CLICK_SLOP: f32 = 4.0;
const UNLOAD_AFTER: std::time::Duration = std::time::Duration::from_secs(5);
const REST_AFTER: std::time::Duration = std::time::Duration::from_secs(120);

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Browse,
    Develop,
}

type Action = Box<dyn Fn(&mut FilmlabApp, &mut Window, &mut Context<FilmlabApp>)>;

#[derive(Default)]
struct Pushed {
    fit: Option<usize>,
    detail: Option<usize>,
}

struct Drag {
    start: Point<Pixels>,
    camera: crate::camera::Camera,
    moved: bool,
}

pub struct FilmlabApp {
    pub library: Entity<Library>,
    pub develop: Entity<Develop>,
    pub mode: Mode,
    pub stocks: Vec<Stock>,
    pub ui: Ui,
    pub selected: Option<String>,
    pub tools: Tools,
    pub rail: rail::State,
    pub rail_open: bool,
    pub export: export::State,
    pub browse: browse::State,
    viewer: Rc<RefCell<Option<Viewer>>>,
    pushed: Rc<RefCell<Pushed>>,
    glass: GlassLayer,
    focus: FocusHandle,
    drag: Option<Drag>,
    reopen: Option<String>,
    unload: Option<Task<()>>,
    rest: Option<Task<()>>,
}

fn state_file() -> Option<PathBuf> {
    let home = std::env::var_os("HOME")?;
    Some(PathBuf::from(home).join("Library/Application Support/Filmlab/state.json"))
}

fn remember(folder: &std::path::Path, photo: Option<&str>) {
    // why: a scripted soak run must not replace the folder and photo the user last had open
    if std::env::var_os("FILMLAB_SOAK").is_some() {
        return;
    }
    let Some(file) = state_file() else { return };
    if let Some(dir) = file.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let _ = std::fs::write(file, serde_json::json!({"folder": folder, "photo": photo}).to_string());
}

fn last_session() -> Option<(PathBuf, Option<String>)> {
    let text = std::fs::read_to_string(state_file()?).ok()?;
    let value: serde_json::Value = serde_json::from_str(&text).ok()?;
    let folder = PathBuf::from(value["folder"].as_str()?);
    folder.is_dir().then(|| (folder, value["photo"].as_str().map(String::from)))
}

impl FilmlabApp {
    pub fn new(gpu: Option<Arc<Gpu>>, window: &mut Window, cx: &mut Context<Self>) -> FilmlabApp {
        let library = cx.new(|_| Library::new());
        let develop = cx.new(|_| Develop::new(library.clone()));
        cx.observe(&library, |app: &mut FilmlabApp, library, cx| {
            if let Some(id) = app.reopen.clone()
                && library.read(cx).record(&id).is_some_and(|r| r.probed.is_some() || r.error.is_some())
            {
                app.reopen = None;
                if library.read(cx).record(&id).is_some_and(|r| r.probed.is_some()) {
                    app.open_photo(&id, cx);
                }
            }
            cx.notify();
        })
        .detach();
        cx.observe(&develop, |_, _, cx| cx.notify()).detach();
        let viewer = Rc::new(RefCell::new(gpu.clone().and_then(|g| Viewer::new(window, g))));
        let focus = cx.focus_handle();
        window.focus(&focus, cx);
        let mut app = FilmlabApp {
            library,
            develop,
            mode: Mode::Browse,
            stocks: stocks::scan(),
            ui: Ui::default(),
            selected: None,
            tools: Tools::default(),
            rail: rail::State::default(),
            rail_open: true,
            export: export::State::default(),
            browse: browse::State::default(),
            viewer,
            pushed: Rc::default(),
            glass: GlassLayer::default(),
            focus,
            drag: None,
            reopen: None,
            unload: None,
            rest: None,
        };
        cx.observe_window_activation(window, |app, window, cx| app.set_active(window.is_window_active(), cx)).detach();
        if !window.is_window_active() {
            app.set_active(false, cx);
        }
        // why: the stage size otherwise arrives with the first paint, and a window that isn't drawing (screen locked,
        // hidden, minimised) never paints, so an agent's open_photo would wait for a frame that never comes
        let (stage, _) = app.device_stage(window);
        app.develop.update(cx, |d, _| d.viewport = Size { width: stage.width, height: stage.height });
        let develop = app.develop.downgrade();
        cx.set_global(super::slider::OnRelease(Rc::new(move |cx| {
            let _ = develop.update(cx, |d, cx| d.release(cx));
        })));
        if let Some((folder, photo)) = last_session() {
            app.open_folder(folder, cx);
            app.reopen = photo;
        }
        app
    }

    // why: Develop's decodes and caches (hundreds of MB) are released after a while in Library; reopening a photo
    // decodes its preview again in about a quarter of a second
    pub fn set_mode(&mut self, mode: Mode, cx: &mut Context<Self>) {
        self.mode = mode;
        self.unload = None;
        if mode == Mode::Browse {
            self.unload = Some(cx.spawn(async move |this, cx| {
                cx.background_executor().timer(UNLOAD_AFTER).await;
                let _ = this.update(cx, |app, cx| {
                    if app.mode == Mode::Browse {
                        app.develop.update(cx, |d, cx| d.unload(cx));
                    }
                });
            }));
        }
        cx.notify();
    }

    // why: the full-size source that makes zoom instant (about 300 MB for a 24 MP raw) earns its memory while someone
    // is at the window; while it's in the background it goes after two quiet minutes, and coming back to the window
    // decodes it again
    fn set_active(&mut self, active: bool, cx: &mut Context<Self>) {
        if active {
            self.rest = None;
            self.develop.update(cx, |d, cx| d.wake(cx));
            return;
        }
        self.rest = Some(cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor().timer(REST_AFTER / 4).await;
                let alive = this.update(cx, |app, cx| {
                    if app.mode == Mode::Develop {
                        app.develop.update(cx, |d, cx| d.rest(REST_AFTER, cx));
                    }
                });
                if alive.is_err() {
                    return;
                }
            }
        }));
    }

    pub fn open_folder(&mut self, folder: PathBuf, cx: &mut Context<Self>) {
        remember(&folder, None);
        self.set_mode(Mode::Browse, cx);
        self.selected = None;
        self.library.update(cx, |library, cx| library.open(folder, cx));
    }

    fn prompt_folder(&mut self, _: &OpenFolder, _: &mut Window, cx: &mut Context<Self>) {
        let rx = cx.prompt_for_paths(PathPromptOptions { files: false, directories: true, multiple: false, prompt: Some("Open".into()) });
        cx.spawn(async move |this, cx| {
            if let Ok(Ok(Some(paths))) = rx.await
                && let Some(folder) = paths.into_iter().next()
            {
                let _ = this.update(cx, |app, cx| app.open_folder(folder, cx));
            }
        })
        .detach();
    }

    pub fn open_photo(&mut self, id: &str, cx: &mut Context<Self>) {
        if let Some(folder) = self.library.read(cx).folder.clone() {
            remember(&folder, Some(id));
        }
        self.selected = Some(id.to_string());
        self.set_mode(Mode::Develop, cx);
        let id = id.to_string();
        self.develop.update(cx, |d, cx| d.open_photo(&id, cx));
        cx.notify();
    }

    fn step_photo(&mut self, by: isize, cx: &mut Context<Self>) {
        if self.mode == Mode::Browse {
            self.step_focus(by as i32, 0, false, cx);
            return;
        }
        let library = self.library.read(cx);
        let Some(current) = self.selected.as_ref().and_then(|id| library.index_of(id)) else { return };
        let next = current as isize + by;
        let Some(record) = (next >= 0).then(|| library.photos.get(next as usize)).flatten() else { return };
        let id = record.file.id.clone();
        if self.mode == Mode::Develop {
            self.open_photo(&id, cx);
        } else {
            self.selected = Some(id);
            cx.notify();
        }
    }

    fn left(&self) -> f32 {
        if self.rail_open { RAIL + 2.0 * MARGIN } else { MARGIN }
    }

    fn stage(&self, window: &Window) -> Rect {
        let v = window.viewport_size();
        let (w, h) = (f32::from(v.width), f32::from(v.height));
        let x = self.left();
        let y = TOP + MARGIN;
        Rect { x: f64::from(x), y: f64::from(y), width: f64::from((w - x - INSPECTOR - 2.0 * MARGIN).max(1.0)), height: f64::from((h - y - STRIP - 2.0 * MARGIN).max(1.0)) }
    }

    /// The fit frame as shown, in window points, with its size in frame pixels. Tool overlays lay out in
    /// this box: its pixels are the frame's pixels divided by the scale factor.
    pub fn fit_frame(&self, window: &Window, cx: &App) -> Option<(Bounds<Pixels>, (u32, u32))> {
        let d = self.develop.read(cx);
        let frame = d.fit.as_ref()?;
        let (stage, s) = self.device_stage(window);
        let r = crate::camera::rect_of(d.camera, frame.region, d.native, Size { width: stage.width, height: stage.height });
        let origin = point(px(((stage.x + r.x) / s) as f32), px(((stage.y + r.y) / s) as f32));
        // why: a draft drawn mid-drag has half the pixels but fills the same box
        let (w, h) = if d.drafting() { (r.width, r.height) } else { (f64::from(frame.width), f64::from(frame.height)) };
        let size = size(px((w / s) as f32), px((h / s) as f32));
        Some((Bounds::new(origin, size), (frame.width, frame.height)))
    }

    fn device_stage(&self, window: &Window) -> (Rect, f64) {
        let s = f64::from(window.scale_factor());
        let r = self.stage(window);
        (Rect { x: r.x * s, y: r.y * s, width: (r.width * s).round(), height: (r.height * s).round() }, s)
    }

    fn device_point(&self, p: Point<Pixels>, window: &Window) -> (f64, f64) {
        let (stage, s) = self.device_stage(window);
        (f64::from(f32::from(p.x)) * s - stage.x, f64::from(f32::from(p.y)) * s - stage.y)
    }

    fn show(&mut self, camera: crate::camera::Camera, animate: bool, gesture: bool, cx: &mut Context<Self>) {
        self.develop.update(cx, |d, cx| {
            let zoom = zoom_of(camera, d.native, d.viewport);
            d.set_zoom(zoom, animate, gesture, cx);
        });
    }

    pub fn is_zoomed(&self, cx: &App) -> bool {
        let d = self.develop.read(cx);
        d.camera.scale > fit_scale(d.native, d.viewport) * 1.001
    }

    fn on_pinch(&mut self, event: &PinchEvent, window: &mut Window, cx: &mut Context<Self>) {
        let (x, y) = self.device_point(event.position, window);
        let d = self.develop.read(cx);
        let scale = d.camera.scale * (1.0 + f64::from(event.delta) * PINCH_SPEED);
        let camera = zoom_around(d.camera, scale, x, y, d.native, d.viewport);
        self.show(camera, false, true, cx);
    }

    fn on_scroll(&mut self, event: &ScrollWheelEvent, window: &mut Window, cx: &mut Context<Self>) {
        let s = f64::from(window.scale_factor());
        let delta = event.delta.pixel_delta(px(16.));
        let (dx, dy) = (f64::from(f32::from(delta.x)), f64::from(f32::from(delta.y)));
        let d = self.develop.read(cx);
        if event.modifiers.platform || event.modifiers.control {
            let (x, y) = self.device_point(event.position, window);
            let step = (-dy).clamp(-60.0, 60.0);
            let camera = zoom_around(d.camera, d.camera.scale * (-step * WHEEL_SPEED).exp(), x, y, d.native, d.viewport);
            self.show(camera, false, true, cx);
        } else if self.is_zoomed(cx) {
            let camera = pan_by(d.camera, dx * s, dy * s, d.native, d.viewport);
            self.show(camera, false, true, cx);
        }
    }

    fn on_down(&mut self, event: &MouseDownEvent, _: &mut Window, cx: &mut Context<Self>) {
        if self.develop.read(cx).tool != crate::develop::Tool::Edit {
            return;
        }
        self.drag = Some(Drag { start: event.position, camera: self.develop.read(cx).camera, moved: false });
    }

    fn on_move(&mut self, event: &MouseMoveEvent, window: &mut Window, cx: &mut Context<Self>) {
        let zoomed = self.is_zoomed(cx);
        let Some(drag) = self.drag.as_mut() else { return };
        if event.pressed_button != Some(MouseButton::Left) {
            self.drag = None;
            return;
        }
        let delta = event.position - drag.start;
        if !drag.moved && f32::from(delta.x).hypot(f32::from(delta.y)) < CLICK_SLOP {
            return;
        }
        drag.moved = true;
        if !zoomed {
            return;
        }
        let s = f64::from(window.scale_factor());
        let start = drag.camera;
        let d = self.develop.read(cx);
        let camera = pan_by(start, f64::from(f32::from(delta.x)) * s, f64::from(f32::from(delta.y)) * s, d.native, d.viewport);
        self.show(camera, false, true, cx);
    }

    fn on_up(&mut self, event: &MouseUpEvent, window: &mut Window, cx: &mut Context<Self>) {
        let Some(drag) = self.drag.take() else { return };
        if drag.moved {
            return;
        }
        if self.is_zoomed(cx) {
            self.develop.update(cx, |d, cx| d.set_zoom(Zoom::Fit, true, false, cx));
            return;
        }
        let (x, y) = self.device_point(event.position, window);
        let d = self.develop.read(cx);
        let camera = zoom_around(d.camera, 1.0, x, y, d.native, d.viewport);
        self.show(camera, true, false, cx);
    }

    fn toggle_zoom(&mut self, _: &ToggleZoom, _: &mut Window, cx: &mut Context<Self>) {
        if self.is_zoomed(cx) {
            self.develop.update(cx, |d, cx| d.set_zoom(Zoom::Fit, true, false, cx));
        } else {
            let d = self.develop.read(cx);
            let camera = zoom_around(d.camera, 1.0, d.viewport.width / 2.0, d.viewport.height / 2.0, d.native, d.viewport);
            self.show(camera, true, false, cx);
        }
    }

    fn push_frames(&self, window: &Window, cx: &App) -> Option<Scene> {
        let d = self.develop.read(cx);
        let mut viewer = self.viewer.borrow_mut();
        let viewer = viewer.as_mut()?;
        let mut pushed = self.pushed.borrow_mut();
        let pushed = &mut *pushed;
        let develop_mode = self.mode == Mode::Develop;
        if develop_mode {
            viewer.unpark();
        } else {
            viewer.park(theme::BACKGROUND);
            pushed.fit = None;
            pushed.detail = None;
        }
        for (layer, frame) in [(Layer::Fit, &d.fit), (Layer::Detail, &d.detail)] {
            let frame = frame.as_ref().filter(|_| develop_mode);
            let id = frame.map(|f| Arc::as_ptr(f) as usize);
            let slot = if layer == Layer::Fit { &mut pushed.fit } else { &mut pushed.detail };
            if *slot != id {
                *slot = id;
                viewer.show(layer, frame.map(|f: &Arc<DisplayFrame>| Frame { width: f.width, height: f.height, rgba: f.rgba.clone(), region: f.region }));
            }
        }
        let (stage, s) = self.device_stage(window);
        let v = window.viewport_size();
        Some(Scene {
            camera: d.camera,
            native: d.native,
            stage,
            drawable: ((f64::from(f32::from(v.width)) * s).round() as u32, (f64::from(f32::from(v.height)) * s).round() as u32),
            background: theme::BACKGROUND,
        })
    }

    fn top_bar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let me = cx.entity();
        let library = self.library.read(cx);
        let develop = self.develop.read(cx);
        let title: SharedString = match self.mode {
            Mode::Browse => library.folder.as_ref().and_then(|f| f.file_name()).map(|n| n.to_string_lossy().to_string()).unwrap_or_else(|| "Filmlab".into()).into(),
            Mode::Develop => develop.photo_id.clone().unwrap_or_default().into(),
        };
        let mode = self.mode;
        let tab = |label: &'static str, which: Mode, me: Entity<FilmlabApp>| {
            div()
                .id(label)
                .px(px(12.))
                .py(px(4.))
                .rounded_full()
                .text_size(px(12.))
                .font_weight(FontWeight::MEDIUM)
                .cursor_pointer()
                .text_color(if mode == which { theme::primary() } else { theme::secondary() })
                .when(mode == which, |d| d.bg(theme::selected()))
                .on_click(move |_, _, cx| {
                    me.update(cx, |app, cx| {
                        app.set_mode(which, cx);
                        if which == Mode::Develop
                            && app.develop.read(cx).photo_id.is_none()
                            && let Some(id) = app.selected.clone()
                        {
                            app.open_photo(&id, cx);
                        }
                        cx.notify();
                    })
                })
                .child(label)
        };
        let button = |id: &'static str, glyph: &'static str, enabled: bool, action: Action, me: Entity<FilmlabApp>| {
            div()
                .id(id)
                .size(px(28.))
                .flex()
                .items_center()
                .justify_center()
                .rounded_full()
                .when(enabled, |d| d.cursor_pointer().hover(|s| s.bg(theme::hover())))
                .when(!enabled, |d| d.opacity(0.35))
                .when(enabled, |d| d.on_click(move |_, window, cx| me.update(cx, |app, cx| action(app, window, cx))))
                .child(icon::small(glyph))
        };
        let can_undo = develop.cursor > 0;
        let can_redo = develop.cursor + 1 < develop.history.len();
        let before = develop.compare == Compare::Before;
        let failed = library.saving == crate::library::Saving::Failed;
        self.glass
            .pane("top", 20.0)
            .occlude()
            .absolute()
            .top(px(MARGIN))
            .left(px(84.))
            .right(px(MARGIN))
            .h(px(TOP - MARGIN))
            .px(px(10.))
            .flex()
            .items_center()
            .gap(px(10.))
            .child(div().flex().p(px(2.)).rounded_full().bg(theme::track()).child(tab("Library", Mode::Browse, me.clone())).child(tab("Develop", Mode::Develop, me.clone())))
            // why: the agent and a failed save share the title's stretch, taking room from the title alone, so the
            // toolbar and buttons never move when they come and go; saving itself is shown in History
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .overflow_hidden()
                    .flex()
                    .items_center()
                    .gap(px(10.))
                    .child(div().flex_1().min_w_0().text_size(px(13.)).font_weight(FontWeight::SEMIBOLD).text_color(theme::primary()).truncate().child(title))
                    .when(failed, |d| d.child(div().flex_none().text_size(px(12.)).text_color(theme::danger()).child("Couldn't save")))
                    .when_some(crate::mcp::presence(), |d, presence| d.child(super::agent::pill(presence))),
            )
            .when(mode == Mode::Browse && library.folder.is_some(), |d| d.child(div().flex_none().child(self.browse_toolbar(cx))))
            .child(button("open", "folder-open", true, Box::new(|app, window, cx| app.prompt_folder(&OpenFolder, window, cx)), me.clone()))
            .child(button("export", "download04", true, Box::new(export::open), me.clone()))
            .when(mode == Mode::Develop, |d| {
                d.child(button("undo", "undo02", can_undo, Box::new(|app, _, cx| app.develop.update(cx, |d, cx| d.undo(cx))), me.clone()))
                    .child(button("redo", "redo02", can_redo, Box::new(|app, _, cx| app.develop.update(cx, |d, cx| d.redo(cx))), me.clone()))
                    .child(
                        div()
                            .id("before")
                            .px(px(10.))
                            .py(px(4.))
                            .rounded_full()
                            .text_size(px(12.))
                            .font_weight(FontWeight::MEDIUM)
                            .cursor_pointer()
                            .text_color(if before { theme::accent() } else { theme::secondary() })
                            .hover(|s| s.bg(theme::hover()))
                            .on_click({
                                let me = me.clone();
                                move |_, _, cx| me.update(cx, |app, cx| app.develop.update(cx, |d, cx| d.set_compare(if before { Compare::After } else { Compare::Before }, cx)))
                            })
                            .child(if before { "Before" } else { "After" }),
                    )
            })
    }
}

impl FilmlabApp {
    fn thumb(&self, id: &str, image: Option<Arc<RenderImage>>, selected: bool, edge: f32, cx: &mut Context<Self>) -> Stateful<Div> {
        let me = cx.entity();
        let open = id.to_string();
        div()
            .id(SharedString::from(format!("thumb-{id}")))
            .size(px(edge))
            .flex()
            .items_center()
            .justify_center()
            .rounded(px(8.))
            .p(px(4.))
            .cursor_pointer()
            .when(selected, |d| d.bg(theme::selected()))
            .when(!selected, |d| d.hover(|s| s.bg(theme::hover())))
            .on_click(move |event, _, cx| {
                let open = open.clone();
                me.update(cx, |app, cx| {
                    if event.click_count() >= 2 || app.mode == Mode::Develop {
                        app.open_photo(&open, cx);
                    } else {
                        app.selected = Some(open);
                        cx.notify();
                    }
                })
            })
            .child(match image {
                Some(image) => img(image).max_w_full().max_h_full().rounded(px(3.)).into_any_element(),
                None => div().size(px(edge * 0.5)).rounded(px(4.)).bg(theme::track()).into_any_element(),
            })
    }

    fn filmstrip(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let photos: Vec<(String, Option<Arc<RenderImage>>)> = self.library.read(cx).photos.iter().map(|r| (r.file.id.clone(), r.thumbnail.clone())).collect();
        let current = self.develop.read(cx).photo_id.clone();
        let thumbs: Vec<_> = photos.into_iter().map(|(id, image)| self.thumb(&id, image, current.as_deref() == Some(id.as_str()), STRIP - 16.0, cx)).collect();
        self.glass
            .pane("strip", 20.0)
            .occlude()
            .absolute()
            .left(px(self.left()))
            .right(px(INSPECTOR + 2.0 * MARGIN))
            .bottom(px(MARGIN))
            .h(px(STRIP))
            .child(div().id("strip-scroll").size_full().overflow_x_scroll().flex().items_center().gap(px(2.)).px(px(8.)).children(thumbs))
    }

    fn browse(&mut self, cx: &mut Context<Self>) -> AnyElement {
        if self.library.read(cx).folder.is_some() {
            return self.browse_grid(RAIL + 2.0 * MARGIN, INSPECTOR + 2.0 * MARGIN, TOP + MARGIN, cx);
        }
        let me = cx.entity();
        div()
            .absolute()
            .inset_0()
            .flex()
            .flex_col()
            .items_center()
            .justify_center()
            .gap(px(12.))
            .child(div().text_size(px(20.)).font_weight(FontWeight::SEMIBOLD).text_color(theme::primary()).child("Open a folder of photos"))
            .child(div().text_size(px(13.)).text_color(theme::secondary()).child("Filmlab edits the photos in place and saves XMP sidecars next to them."))
            .child(
                div()
                    .id("open-first")
                    .mt(px(8.))
                    .px(px(16.))
                    .py(px(8.))
                    .rounded_full()
                    .bg(theme::accent())
                    .text_color(theme::backdrop())
                    .text_size(px(13.))
                    .font_weight(FontWeight::SEMIBOLD)
                    .cursor_pointer()
                    .on_click(move |_, window, cx| me.update(cx, |app, cx| app.prompt_folder(&OpenFolder, window, cx)))
                    .child("Choose folder…"),
            )
            .into_any_element()
    }

    pub fn set_tool(&mut self, tool: Tool, cx: &mut Context<Self>) {
        if self.mode != Mode::Develop {
            return;
        }
        let current = self.develop.read(cx).tool;
        let next = if current == tool && tool != Tool::Edit { Tool::Edit } else { tool };
        self.develop.update(cx, |d, cx| d.set_tool(next, cx));
        cx.notify();
    }

    fn tool_row(&self, active: Tool, cx: &mut Context<Self>) -> impl IntoElement {
        let me = cx.entity();
        let tools = [(Tool::Edit, "Edit", "sliders-horizontal"), (Tool::Crop, "Crop", "crop"), (Tool::Heal, "Heal", "bandage"), (Tool::Masks, "Masks", "paint-brush02")];
        div().p(px(10.)).pb(px(2.)).child(div().flex().p(px(2.)).gap(px(2.)).rounded(px(10.)).bg(theme::surface()).children(tools.into_iter().map(move |(tool, label, glyph)| {
            let me = me.clone();
            let on = tool == active;
            div()
                .id(label)
                .flex_1()
                .h(px(30.))
                .flex()
                .items_center()
                .justify_center()
                .gap(px(5.))
                .rounded(px(8.))
                .text_size(px(12.))
                .font_weight(FontWeight::MEDIUM)
                .text_color(if on { theme::primary() } else { theme::label() })
                .when(on, |d| d.bg(theme::fill()))
                .when(!on, |d| d.cursor_pointer().hover(|s| s.bg(theme::hover())))
                .on_click(move |_, _, cx| me.update(cx, |app, cx| app.set_tool(tool, cx)))
                .child(icon::icon(glyph, px(14.)))
                .child(label)
        })))
    }

    fn stage_layer(&self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let d = self.develop.read(cx);
        let status = match d.status {
            Status::Loading if d.fit.is_none() => Some("Loading…".to_string()),
            Status::Failed => Some(d.error.clone().unwrap_or_else(|| "This photo couldn't be opened.".into())),
            _ => None,
        };
        let stage = self.stage(window);
        let seen = Bounds::new(point(px(stage.x as f32), px(stage.y as f32)), size(px(stage.width as f32), px(stage.height as f32)));
        let outline = self.fit_frame(window, cx).map(|(frame, _)| super::agent::outline(frame.intersect(&seen)));
        div()
            .id("stage")
            .absolute()
            .inset_0()
            .on_mouse_down(MouseButton::Left, cx.listener(Self::on_down))
            .on_mouse_move(cx.listener(Self::on_move))
            .on_mouse_up(MouseButton::Left, cx.listener(Self::on_up))
            .on_scroll_wheel(cx.listener(Self::on_scroll))
            .on_pinch(cx.listener(Self::on_pinch))
            .children(outline)
            .when_some(status, |d, text| {
                d.child(
                    div()
                        .absolute()
                        .left(px(stage.x as f32))
                        .top(px(stage.y as f32))
                        .w(px(stage.width as f32))
                        .h(px(stage.height as f32))
                        .flex()
                        .items_center()
                        .justify_center()
                        .text_size(px(13.))
                        .text_color(theme::secondary())
                        .child(text),
                )
            })
    }

    fn painter(&self, window: &Window, cx: &App) -> impl IntoElement {
        let scene = self.push_frames(window, cx);
        let viewer = self.viewer.clone();
        canvas(
            |_, _, _| {},
            move |_, _, _, _| {
                if let (Some(scene), Some(viewer)) = (scene, viewer.borrow_mut().as_mut()) {
                    viewer.draw(scene);
                }
            },
        )
        .absolute()
        .size_0()
    }
}

impl Render for FilmlabApp {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.glass.begin();
        if self.mode == Mode::Develop {
            let (stage, _) = self.device_stage(window);
            let viewport = Size { width: stage.width, height: stage.height };
            let animating = self.develop.update(cx, |d, cx| {
                d.set_viewport(viewport, cx);
                d.tick()
            });
            if animating {
                window.request_animation_frame();
            }
        }
        let develop_mode = self.mode == Mode::Develop;
        let tool = self.develop.read(cx).tool;
        let inspector_body = develop_mode.then(|| {
            let body = match tool {
                Tool::Edit => inspector(self, cx),
                Tool::Crop => tools::crop::panel(self, window, cx),
                Tool::Heal => tools::heal::panel(self, window, cx),
                Tool::Masks => tools::masks::panel(self, window, cx),
            };
            div().size_full().flex().flex_col().child(self.tool_row(tool, cx)).child(div().flex_1().min_h_0().child(body)).into_any_element()
        });
        let overlay = if develop_mode && tool != Tool::Edit {
            self.fit_frame(window, cx).and_then(|(frame, _)| match tool {
                Tool::Crop => tools::crop::overlay(self, frame, window, cx),
                Tool::Heal => tools::heal::overlay(self, frame, window, cx),
                Tool::Masks => tools::masks::overlay(self, frame, window, cx),
                Tool::Edit => None,
            })
        } else {
            None
        };
        let rail_body = (develop_mode && self.rail_open).then(|| rail::rail(self, window, cx));
        let browse_panes = (!develop_mode && self.library.read(cx).folder.is_some()).then(|| (self.browse_rail(cx), self.browse_panel(cx)));
        let dialog = export::dialog(self, window, cx);
        div()
            .size_full()
            .relative()
            .font_family(theme::FONT)
            .text_color(theme::primary())
            .track_focus(&self.focus)
            .key_context("Filmlab")
            .on_action(cx.listener(Self::prompt_folder))
            .on_action(cx.listener(|app, _: &Undo, _, cx| app.develop.update(cx, |d, cx| d.undo(cx))))
            .on_action(cx.listener(|app, _: &Redo, _, cx| app.develop.update(cx, |d, cx| d.redo(cx))))
            .on_action(cx.listener(|app, _: &ShowBrowse, _, cx| app.set_mode(Mode::Browse, cx)))
            .on_action(cx.listener(|app, _: &ShowDevelop, _, cx| {
                if let Some(id) = app.selected.clone() {
                    app.open_photo(&id, cx);
                }
            }))
            .on_action(cx.listener(|app, _: &NextPhoto, _, cx| app.step_photo(1, cx)))
            .on_action(cx.listener(|app, _: &PreviousPhoto, _, cx| app.step_photo(-1, cx)))
            .on_action(cx.listener(Self::toggle_zoom))
            .on_action(cx.listener(|app, _: &ToggleBefore, _, cx| {
                app.develop.update(cx, |d, cx| {
                    let next = if d.compare == Compare::Before { Compare::After } else { Compare::Before };
                    d.set_compare(next, cx)
                })
            }))
            .on_key_down(cx.listener(|app, event: &KeyDownEvent, _, cx| {
                if app.develop.read(cx).tool == Tool::Masks && tools::masks::on_key(app, event, cx) {
                    cx.stop_propagation();
                }
            }))
            .on_action(cx.listener(|app, _: &Export, window, cx| export::open(app, window, cx)))
            .on_action(cx.listener(|app, _: &EditTool, _, cx| app.set_tool(Tool::Edit, cx)))
            .on_action(cx.listener(|app, _: &CropTool, _, cx| app.set_tool(Tool::Crop, cx)))
            .on_action(cx.listener(|app, _: &HealTool, _, cx| app.set_tool(Tool::Heal, cx)))
            .on_action(cx.listener(|app, _: &MasksTool, _, cx| app.set_tool(Tool::Masks, cx)))
            .on_action(cx.listener(|app, _: &ToggleRail, _, cx| {
                app.rail_open = !app.rail_open;
                cx.notify();
            }))
            .when(develop_mode, |d| d.child(self.stage_layer(window, cx)))
            .children(overlay)
            .when(!develop_mode, |d| d.child(self.browse(cx)))
            .when_some(browse_panes, |d, (rail_body, panel_body)| {
                d.child(self.glass.pane("browse-rail", 22.0).occlude().absolute().left(px(MARGIN)).top(px(TOP + MARGIN)).bottom(px(MARGIN)).w(px(RAIL)).child(rail_body))
                    .child(self.glass.pane("browse-panel", 22.0).occlude().absolute().right(px(MARGIN)).top(px(TOP + MARGIN)).bottom(px(MARGIN)).w(px(INSPECTOR)).child(panel_body))
            })
            .on_action(cx.listener(|app, _: &FlagPick, _, cx| app.flag("pick", cx)))
            .on_action(cx.listener(|app, _: &FlagReject, _, cx| app.flag("reject", cx)))
            .on_action(cx.listener(|app, _: &Unflag, _, cx| app.flag("none", cx)))
            .on_action(cx.listener(|app, _: &SelectAll, _, cx| app.select_all(cx)))
            .on_action(cx.listener(|app, _: &ClearSelection, _, cx| {
                if app.mode == Mode::Browse {
                    app.clear_selection(cx);
                } else if app.develop.read(cx).tool != Tool::Edit {
                    app.set_tool(Tool::Edit, cx);
                }
            }))
            .on_action(cx.listener(|app, _: &OpenFocused, _, cx| {
                if app.mode == Mode::Browse
                    && let Some(id) = app.selected.clone()
                {
                    app.open_photo(&id, cx);
                }
            }))
            .on_action(cx.listener(|app, _: &MoveUp, _, cx| app.step_focus(0, -1, false, cx)))
            .on_action(cx.listener(|app, _: &MoveDown, _, cx| app.step_focus(0, 1, false, cx)))
            .on_action(cx.listener(|app, _: &ExtendLeft, _, cx| app.step_focus(-1, 0, true, cx)))
            .on_action(cx.listener(|app, _: &ExtendRight, _, cx| app.step_focus(1, 0, true, cx)))
            .on_action(cx.listener(|app, _: &ExtendUp, _, cx| app.step_focus(0, -1, true, cx)))
            .on_action(cx.listener(|app, _: &ExtendDown, _, cx| app.step_focus(0, 1, true, cx)))
            .child(self.top_bar(cx))
            .when_some(inspector_body, |d, body| {
                d.child(self.glass.pane("inspector", 22.0).occlude().absolute().right(px(MARGIN)).top(px(TOP + MARGIN)).bottom(px(MARGIN)).w(px(INSPECTOR)).child(body))
            })
            .when_some(rail_body, |d, body| {
                d.child(self.glass.pane("rail", 22.0).occlude().absolute().left(px(MARGIN)).top(px(TOP + MARGIN)).bottom(px(MARGIN)).w(px(RAIL)).child(body))
            })
            .when(develop_mode, |d| d.child(self.filmstrip(cx)))
            .children(dialog)
            .child(self.painter(window, cx))
            .child(self.glass.finish())
    }
}

pub fn bind(cx: &mut App) {
    // why: single keys stay out of text fields, which handle their own typing
    let keys = Some("Filmlab && !Input && !Dialog");
    cx.bind_keys([
        KeyBinding::new("cmd-o", OpenFolder, None),
        KeyBinding::new("cmd-z", Undo, keys),
        KeyBinding::new("cmd-shift-z", Redo, keys),
        KeyBinding::new("g", ShowBrowse, keys),
        KeyBinding::new("d", ShowDevelop, keys),
        KeyBinding::new("right", NextPhoto, keys),
        KeyBinding::new("left", PreviousPhoto, keys),
        KeyBinding::new("up", MoveUp, keys),
        KeyBinding::new("down", MoveDown, keys),
        KeyBinding::new("shift-left", ExtendLeft, keys),
        KeyBinding::new("shift-right", ExtendRight, keys),
        KeyBinding::new("shift-up", ExtendUp, keys),
        KeyBinding::new("shift-down", ExtendDown, keys),
        KeyBinding::new("z", ToggleZoom, keys),
        KeyBinding::new("space", ToggleZoom, keys),
        KeyBinding::new("\\", ToggleBefore, keys),
        KeyBinding::new("cmd-q", Quit, None),
        KeyBinding::new("e", EditTool, keys),
        KeyBinding::new("r", CropTool, keys),
        KeyBinding::new("q", HealTool, keys),
        KeyBinding::new("m", MasksTool, keys),
        KeyBinding::new("p", FlagPick, keys),
        KeyBinding::new("x", FlagReject, keys),
        KeyBinding::new("u", Unflag, keys),
        KeyBinding::new("cmd-a", SelectAll, keys),
        KeyBinding::new("escape", ClearSelection, keys),
        KeyBinding::new("enter", OpenFocused, keys),
        KeyBinding::new("cmd-shift-e", Export, None),
        KeyBinding::new("cmd-\\", ToggleRail, None),
    ]);
    cx.on_action(|_: &Quit, cx| cx.quit());
    cx.set_menus([
        Menu { name: "Filmlab".into(), items: vec![MenuItem::action("Quit Filmlab", Quit)], disabled: false },
        Menu { name: "File".into(), items: vec![MenuItem::action("Open Folder…", OpenFolder), MenuItem::action("Export…", Export)], disabled: false },
        Menu {
            name: "Tools".into(),
            items: vec![MenuItem::action("Edit", EditTool), MenuItem::action("Crop", CropTool), MenuItem::action("Heal", HealTool), MenuItem::action("Masks", MasksTool)],
            disabled: false,
        },
        Menu { name: "Edit".into(), items: vec![MenuItem::action("Undo", Undo), MenuItem::action("Redo", Redo)], disabled: false },
        Menu {
            name: "View".into(),
            items: vec![
                MenuItem::action("Library", ShowBrowse),
                MenuItem::action("Develop", ShowDevelop),
                MenuItem::action("Show Rail", ToggleRail),
                MenuItem::separator(),
                MenuItem::action("Zoom", ToggleZoom),
                MenuItem::action("Before / After", ToggleBefore),
                MenuItem::separator(),
                MenuItem::action("Next Photo", NextPhoto),
                MenuItem::action("Previous Photo", PreviousPhoto),
            ],
            disabled: false,
        },
    ]);
}

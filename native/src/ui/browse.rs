use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::rc::Rc;
use std::sync::Arc;

use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use super::app::{FilmlabApp, Mode};
use super::controls::Segmented;
use super::grid::{self, CAPTION, CAPTION_GAP, GAP_X, GAP_Y, Grid, HEADER_GAP, PAD_TOP, PAD_X, ROW_HEIGHT, SECTION_END, Section, TILE_HEIGHT, day_label, passes};
use super::{icon, theme};
use crate::library::PhotoRecord;

const PREVIEW_COUNT: usize = 3;

#[derive(Clone, Copy, PartialEq, Eq, Default)]
pub enum Filter {
    #[default]
    All,
    Picks,
    Rejected,
}

#[derive(Clone, Copy, PartialEq, Eq, Default)]
pub enum Sort {
    #[default]
    Newest,
    Oldest,
    Name,
}

#[derive(Default)]
pub struct State {
    pub filter: Filter,
    pub sort: Sort,
    pub selection: Vec<String>,
    anchor: Option<String>,
    scroll: ScrollHandle,
    tiles: Rc<RefCell<HashMap<String, Bounds<Pixels>>>>,
    order: Vec<String>,
    grid: Grid,
    /// How many rows the first day had when its whole section was last drawn, to learn the header's height from.
    first_drawn: Option<usize>,
}

impl State {
    /// Where a photo's tile was last laid out, in window points, while it is on screen.
    pub fn tile(&self, id: &str) -> Option<Bounds<Pixels>> {
        self.tiles.borrow().get(id).copied()
    }

    /// The photos as the grid shows them, after the filter and the sort.
    pub fn order(&self) -> &[String] {
        &self.order
    }

    /// How far the grid is scrolled down, in points.
    pub fn scrolled(&self) -> f32 {
        -f32::from(self.scroll.offset().y)
    }

    /// How far the grid can scroll down, in points.
    pub fn scroll_range(&self) -> f32 {
        f32::from(self.scroll.max_offset().y)
    }

    pub fn scroll_to(&self, top: f32) {
        self.scroll.set_offset(point(px(0.), px(-top.clamp(0.0, self.scroll_range()))));
    }

    fn regroup(&mut self, library: &crate::library::Library) {
        if self.grid.regroup(library, self.filter, self.sort) {
            self.order = self.grid.order();
        }
    }

    fn measure_header(&mut self) -> bool {
        let Some(rows) = self.first_drawn.take() else { return false };
        let Some(first) = self.scroll.bounds_for_item(0) else { return false };
        self.grid.measured(f32::from(first.size.height), rows)
    }
}

/// What one tile on screen shows.
struct Face {
    id: String,
    index: usize,
    width: f32,
    flag: String,
    failed: bool,
    image: Option<Arc<RenderImage>>,
}

fn face_of(tile: &grid::Tile, record: &PhotoRecord, image: Option<Arc<RenderImage>>) -> Face {
    Face {
        id: tile.id.clone(),
        index: tile.index,
        width: tile.width,
        flag: record.state.as_ref().map(|s| s.flag.clone()).unwrap_or_default(),
        failed: record.error.is_some(),
        image,
    }
}

fn count_label(n: usize) -> String {
    format!("{n} {}", if n == 1 { "photo" } else { "photos" })
}

/// The rows of one day the grid draws, with what each of their tiles shows.
struct Shown {
    rows: std::ops::Range<usize>,
    faces: Vec<Vec<Face>>,
}

struct DayRow {
    key: String,
    label: String,
    count: usize,
}

/// One day of the grid this frame: a spacer as tall as its section, or its header and the rows near the viewport.
struct Drawn {
    at: usize,
    rows: usize,
    height: f32,
    rows_height: f32,
    shown: Option<(usize, Shown)>,
}

impl FilmlabApp {
    pub fn click_tile(&mut self, id: &str, range: bool, toggle: bool, cx: &mut Context<Self>) {
        let b = &mut self.browse;
        if range {
            let anchor = b.anchor.clone().or_else(|| self.selected.clone());
            let ids = &b.order;
            let (from, to) = (anchor.and_then(|a| ids.iter().position(|x| *x == a)), ids.iter().position(|x| x == id));
            b.selection = match (from, to) {
                (Some(f), Some(t)) => ids[f.min(t)..=f.max(t)].to_vec(),
                _ => vec![id.to_string()],
            };
        } else if toggle {
            b.anchor = Some(id.to_string());
            if let Some(at) = b.selection.iter().position(|x| x == id) {
                b.selection.remove(at);
            } else {
                b.selection.push(id.to_string());
            }
        } else {
            b.anchor = Some(id.to_string());
            b.selection = vec![id.to_string()];
        }
        self.selected = Some(id.to_string());
        cx.notify();
    }

    /// The tiles in the grid's viewport, and how many of them show a picture.
    pub fn viewport_pictures(&self, cx: &App) -> (usize, usize) {
        let viewport = self.browse.scroll.bounds();
        let library = self.library.read(cx);
        let tiles = self.browse.tiles.borrow();
        let visible: Vec<&String> = tiles.iter().filter(|(_, b)| b.intersects(&viewport)).map(|(id, _)| id).collect();
        let shown = visible.iter().filter(|id| library.picture(id).is_some()).count();
        (shown, visible.len())
    }

    pub fn targets(&self) -> Vec<String> {
        if self.mode == Mode::Browse && !self.browse.selection.is_empty() {
            return self.browse.selection.clone();
        }
        self.selected.iter().cloned().collect()
    }

    pub fn flag(&mut self, flag: &str, cx: &mut Context<Self>) {
        let ids = self.targets();
        self.library.update(cx, |l, cx| l.set_flag(&ids, flag, cx));
    }

    pub fn select_all(&mut self, cx: &mut Context<Self>) {
        self.browse.selection = self.browse.order.clone();
        if self.selected.is_none() {
            self.selected = self.browse.order.first().cloned();
        }
        cx.notify();
    }

    pub fn clear_selection(&mut self, cx: &mut Context<Self>) {
        self.browse.anchor = None;
        self.browse.selection.clear();
        cx.notify();
    }

    // why: arrow keys follow the laid-out grid, so up and down pick the nearest tile in the next row
    pub fn step_focus(&mut self, dx: i32, dy: i32, extend: bool, cx: &mut Context<Self>) {
        let ids = self.browse.order.clone();
        let Some(at) = self.selected.as_ref().and_then(|s| ids.iter().position(|x| x == s)) else {
            if let Some(first) = ids.first() {
                self.click_tile(&first.clone(), false, false, cx);
            }
            return;
        };
        let next = if dy == 0 {
            ids.get((at as i64 + i64::from(dx)).clamp(0, ids.len() as i64 - 1) as usize).cloned()
        } else {
            let places = self.browse.grid.places();
            places.get(ids[at].as_str()).copied().and_then(|(top, centre)| {
                ids.iter()
                    .filter_map(|id| places.get(id.as_str()).map(|p| (id, *p)))
                    .filter(|(_, (t, _))| (t - top) * dy as f32 > 1.0)
                    .min_by(|(_, a), (_, b)| {
                        let row = |(t, c): &(f32, f32)| ((t - top) * dy as f32, (c - centre).abs());
                        row(a).partial_cmp(&row(b)).unwrap_or(std::cmp::Ordering::Equal)
                    })
                    .map(|(id, _)| id.clone())
            })
        };
        if let Some(next) = next {
            if extend {
                self.click_tile(&next, true, false, cx);
            } else {
                self.click_tile(&next, false, false, cx);
            }
        }
    }

    fn tile(&self, face: Face, focused: bool, selected: bool, cx: &mut Context<Self>) -> AnyElement {
        let me = cx.entity();
        let id = face.id.clone();
        let reject = face.flag == "reject";
        let tiles = self.browse.tiles.clone();
        let record_id = id.clone();
        let picture = match (face.image, face.failed) {
            (_, true) => div()
                .size_full()
                .flex()
                .flex_col()
                .items_center()
                .justify_center()
                .gap(px(4.))
                .bg(theme::surface())
                .text_size(px(11.))
                .text_color(theme::tertiary())
                .child(icon::icon("alert02", px(16.)))
                .child("Couldn't read this file")
                .into_any_element(),
            (Some(image), false) => img(image).size_full().object_fit(ObjectFit::Cover).when(reject, |i| i.opacity(0.35)).into_any_element(),
            (None, false) => div().size_full().bg(theme::surface()).into_any_element(),
        };
        div()
            .id(SharedString::from(format!("tile-{id}")))
            .w(px(face.width))
            .flex()
            .flex_col()
            .gap(px(CAPTION_GAP))
            .on_click(move |event, _, cx| {
                let id = id.clone();
                let modifiers = event.modifiers();
                me.update(cx, |app, cx| {
                    if event.click_count() >= 2 {
                        app.open_photo(&id, cx);
                    } else {
                        app.click_tile(&id, modifiers.shift, modifiers.platform || modifiers.control, cx);
                    }
                })
            })
            .child(
                div()
                    .relative()
                    .h(px(ROW_HEIGHT))
                    .w_full()
                    .rounded(px(4.))
                    .overflow_hidden()
                    .child(canvas(move |b, _, _| {
                        tiles.borrow_mut().insert(record_id.clone(), b);
                    }, |_, _, _, _| {}).absolute().size_full())
                    .child(picture)
                    .child(div().absolute().inset_0().rounded(px(4.)).border_1().border_color(if focused {
                        theme::accent()
                    } else if selected {
                        theme::primary()
                    } else {
                        hsla(0.0, 0.0, 1.0, 0.1)
                    }).when(focused || selected, |d| d.border_2()))
                    .when(selected, |d| {
                        d.child(div().absolute().top(px(6.)).left(px(6.)).size(px(18.)).rounded_full().bg(theme::primary()).flex().items_center().justify_center().child(svg().path("icons/tick02.svg").size(px(12.)).text_color(theme::backdrop())))
                    }),
            )
            .child(
                div()
                    .h(px(CAPTION))
                    .flex()
                    .items_center()
                    .gap(px(6.))
                    .text_size(px(11.))
                    .font_features(theme::tabular())
                    .text_color(if focused { theme::accent() } else { theme::tertiary() })
                    .when(focused, |d| d.font_weight(FontWeight::BOLD))
                    .child(format!("▸{}", face.index))
                    .when(face.flag == "pick", |d| d.child(icon::icon("flag02", px(12.))))
                    .when(face.flag == "reject", |d| d.child(icon::icon("cancel01", px(12.)))),
            )
            .into_any_element()
    }

    fn day_header(day: &grid::Day, chosen: usize) -> Div {
        div()
            .flex()
            .items_baseline()
            .gap(px(10.))
            .child(div().text_size(px(15.)).font_weight(FontWeight::BOLD).text_color(theme::primary()).child(day.label.clone()))
            .child(div().text_size(px(12.)).font_features(theme::tabular()).text_color(theme::tertiary()).child(format!(
                "{}{}",
                count_label(day.tiles.len()),
                if chosen > 0 { format!(" · {chosen} selected") } else { String::new() }
            )))
    }

    /// The rows of `section` that overlap `from..to`, with what each of their tiles shows.
    fn shown(&self, section: &Section, header: f32, from: f32, to: f32, cx: &App) -> Shown {
        let library = self.library.read(cx);
        let rows = section.rows_within(header, from, to);
        let faces = section.rows[rows.clone()]
            .iter()
            .map(|range| {
                section.day.tiles[range.clone()]
                    .iter()
                    .filter_map(|tile| Some(face_of(tile, library.record(&tile.id)?, library.picture(&tile.id).map(|p| p.image.clone()))))
                    .collect()
            })
            .collect();
        Shown { rows, faces }
    }

    /// The photos of `section` in rows overlapping `from..to`, top to bottom.
    fn photos_within<'a>(section: &'a Section, header: f32, from: f32, to: f32) -> impl Iterator<Item = &'a String> {
        let rows = section.rows_within(header, from, to);
        section.rows[rows].iter().flat_map(|range| section.day.tiles[range.clone()].iter().map(|t| &t.id))
    }

    pub fn browse_grid(&mut self, left: f32, right: f32, top: f32, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        self.browse.regroup(self.library.read(cx));
        let viewport = window.viewport_size();
        self.browse.grid.reflow(f32::from(viewport.width) - left - right - 2.0 * PAD_X);
        if self.browse.measure_header() {
            window.request_animation_frame();
        }
        self.browse.tiles.borrow_mut().clear();
        let header = self.browse.grid.header();
        let whole = !self.browse.grid.header_known();
        let height = match f32::from(self.browse.scroll.bounds().size.height) {
            h if h > 0.0 => h,
            _ => f32::from(viewport.height) - top,
        };
        let (from, to) = (self.browse.scrolled(), self.browse.scrolled() + height);
        let selection: HashSet<&str> = self.browse.selection.iter().map(String::as_str).collect();
        let focus = self.selected.clone();
        let (mut visible, mut ahead, mut behind) = (Vec::new(), Vec::new(), Vec::new());
        let mut days: Vec<Drawn> = Vec::new();
        for (at, section) in self.browse.grid.sections().iter().enumerate() {
            visible.extend(Self::photos_within(section, header, from, to).cloned());
            ahead.extend(Self::photos_within(section, header, to, to + height).cloned());
            behind.extend(Self::photos_within(section, header, from - height, from).cloned());
            let near = section.top < to + GAP_Y && section.top + section.height > from - GAP_Y;
            let shown = (whole || near).then(|| {
                let chosen = section.day.tiles.iter().filter(|t| selection.contains(t.id.as_str())).count();
                (chosen, self.shown(section, header, from - TILE_HEIGHT, to + TILE_HEIGHT, cx))
            });
            days.push(Drawn { at, rows: section.rows.len(), height: section.height, rows_height: section.rows_height(), shown });
        }
        behind.reverse();
        let mut seen: HashSet<String> = HashSet::new();
        let wanted: Vec<String> = visible.into_iter().chain(ahead).chain(behind).filter(|id| seen.insert(id.clone())).collect();
        self.library.update(cx, |l, cx| l.want(&wanted, cx));
        if whole && !days.is_empty() {
            window.request_animation_frame();
        }
        let selection: HashSet<String> = self.browse.selection.iter().cloned().collect();
        let mut sections: Vec<AnyElement> = Vec::new();
        for drawn in days {
            let Some((chosen, shown)) = drawn.shown else {
                sections.push(div().flex_none().h(px(drawn.height)).into_any_element());
                continue;
            };
            if drawn.at == 0 {
                self.browse.first_drawn = Some(drawn.rows);
            }
            let head = Self::day_header(&self.browse.grid.days()[drawn.at], chosen);
            let mut lines: Vec<AnyElement> = Vec::new();
            for (row, faces) in shown.rows.clone().zip(shown.faces) {
                let tiles: Vec<AnyElement> = faces
                    .into_iter()
                    .map(|face| {
                        let focused = focus.as_deref() == Some(face.id.as_str());
                        let selected = selection.contains(&face.id);
                        self.tile(face, focused, selected, cx)
                    })
                    .collect();
                lines.push(div().absolute().left_0().right_0().top(px(grid::row_top(row))).flex().gap_x(px(GAP_X)).children(tiles).into_any_element());
            }
            sections.push(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(HEADER_GAP))
                    .pb(px(SECTION_END))
                    .child(head)
                    .child(div().relative().flex_none().w_full().h(px(drawn.rows_height)).children(lines))
                    .into_any_element(),
            );
        }
        let photos = self.library.read(cx).photos.len();
        let empty = match (photos == 0, sections.is_empty(), self.browse.filter) {
            (false, true, Filter::Picks) => Some("No picks yet. Press P to pick a photo."),
            (false, true, Filter::Rejected) => Some("No rejected photos."),
            _ => None,
        };
        div()
            .id("grid")
            .absolute()
            .left(px(left))
            .right(px(right))
            .top(px(top))
            .bottom_0()
            .track_scroll(&self.browse.scroll)
            .overflow_y_scroll()
            .px(px(PAD_X))
            .pt(px(PAD_TOP))
            .children(sections)
            .when_some(empty, |d, text| d.child(div().pt(px(80.)).flex().justify_center().text_size(px(13.)).text_color(theme::tertiary()).child(text)))
            .into_any_element()
    }

    pub fn browse_toolbar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let photos = &self.library.read(cx).photos;
        let picks = photos.iter().filter(|r| passes(Filter::Picks, r)).count();
        let rejected = photos.iter().filter(|r| passes(Filter::Rejected, r)).count();
        let filter = self.browse.filter;
        let me = cx.entity();
        let filters = [(Filter::All, "All", photos.len()), (Filter::Picks, "Picks", picks), (Filter::Rejected, "Rejected", rejected)];
        let sort = self.browse.sort;
        let sorts = [(Sort::Newest, "Newest first"), (Sort::Oldest, "Oldest first"), (Sort::Name, "File name")];
        let sort_label = sorts.iter().find(|(s, _)| *s == sort).map(|(_, l)| *l).unwrap_or("");
        let pick_me = me.clone();
        div()
            .flex()
            .items_center()
            .gap(px(8.))
            .child(div().flex().p(px(2.)).gap(px(2.)).rounded_full().bg(theme::surface()).children(filters.into_iter().map(move |(value, label, count)| {
                let me = me.clone();
                let on = value == filter;
                div()
                    .id(label)
                    .flex()
                    .items_center()
                    .gap(px(6.))
                    .px(px(10.))
                    .py(px(4.))
                    .rounded_full()
                    .text_size(px(12.))
                    .font_weight(FontWeight::MEDIUM)
                    .text_color(if on { theme::primary() } else { theme::label() })
                    .when(on, |d| d.bg(theme::fill()))
                    .when(!on, |d| d.cursor_pointer().hover(|s| s.bg(theme::hover())))
                    .on_click(move |_, _, cx| {
                        me.update(cx, |app, cx| {
                            app.browse.filter = value;
                            cx.notify();
                        })
                    })
                    .child(label)
                    .child(div().text_size(px(11.)).font_features(theme::tabular()).text_color(theme::tertiary()).child(count.to_string()))
            })))
            .child(super::controls::Picker::new(
                "sort",
                "Sort",
                sort_label,
                move || sorts.iter().map(|(s, l)| crate::menu::Entry::Item { label: l.to_string(), checked: *s == sort }).collect(),
                move |i, _, cx| {
                    pick_me.update(cx, |app, cx| {
                        app.browse.sort = sorts[i].0;
                        cx.notify();
                    })
                },
            ))
    }

    pub fn browse_rail(&mut self, cx: &mut Context<Self>) -> AnyElement {
        self.browse.regroup(self.library.read(cx));
        let cover = self.library.read(cx).photos.iter().find(|r| r.error.is_none()).map(|r| r.file.id.clone());
        if let Some(id) = &cover {
            self.library.update(cx, |l, cx| l.want([id], cx));
        }
        let library = self.library.read(cx);
        let name = library.folder.as_ref().and_then(|f| f.file_name()).map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
        let cover = cover.and_then(|id| library.picture(&id).map(|p| p.image.clone()));
        let total = library.photos.len();
        let days: Vec<DayRow> = self.browse.grid.days().iter().map(|d| DayRow { key: d.key.clone(), label: d.label.clone(), count: d.tiles.len() }).collect();
        let top = self.browse.scroll.top_item();
        let me = cx.entity();
        div()
            .size_full()
            .flex()
            .flex_col()
            .gap(px(6.))
            .p(px(12.))
            .child(div().h(px(28.)).flex().items_center().text_size(px(15.)).font_weight(FontWeight::SEMIBOLD).text_color(theme::focus()).child("Folder"))
            .child(
                div()
                    .h(px(52.))
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .px(px(8.))
                    .rounded(px(theme::RADIUS))
                    .bg(theme::fill())
                    .child(match cover {
                        Some(image) => img(image).w(px(60.)).h(px(40.)).rounded(px(3.)).object_fit(ObjectFit::Cover).into_any_element(),
                        None => div().w(px(60.)).h(px(40.)).rounded(px(3.)).bg(theme::surface()).into_any_element(),
                    })
                    .child(
                        div()
                            .min_w_0()
                            .flex()
                            .flex_col()
                            .gap(px(2.))
                            .child(div().truncate().text_size(px(13.)).font_weight(FontWeight::MEDIUM).text_color(theme::primary()).child(name))
                            .child(div().text_size(px(12.)).font_features(theme::tabular()).text_color(theme::label()).child(count_label(total))),
                    ),
            )
            .child(div().id("days").flex_1().min_h_0().overflow_y_scroll().pt(px(6.)).flex().flex_col().gap(px(2.)).children(days.iter().enumerate().map(|(i, day)| {
                let me = me.clone();
                let current = i == top;
                div()
                    .id(SharedString::from(format!("day-{}", day.key)))
                    .h(px(32.))
                    .flex()
                    .items_center()
                    .justify_between()
                    .px(px(10.))
                    .rounded(px(6.))
                    .text_size(px(13.))
                    .font_weight(FontWeight::MEDIUM)
                    .text_color(if current { theme::primary() } else { theme::label() })
                    .when(current, |d| d.bg(theme::surface()))
                    .cursor_pointer()
                    .hover(|s| s.bg(theme::hover()))
                    .on_click(move |_, _, cx| {
                        me.update(cx, |app, cx| {
                            app.browse.scroll.scroll_to_item(i);
                            cx.notify();
                        })
                    })
                    .child(day.label.clone())
                    .child(div().text_size(px(12.)).font_features(theme::tabular()).text_color(theme::tertiary()).child(day.count.to_string()))
            })))
            .into_any_element()
    }

    fn info_rows(record: &PhotoRecord) -> Vec<(&'static str, String)> {
        let facts = record.probed.as_ref().map(|p| p.facts.clone()).unwrap_or_default();
        let meta = &facts["meta"];
        let join = |parts: Vec<Option<String>>| parts.into_iter().flatten().filter(|p| !p.is_empty()).collect::<Vec<_>>().join(" · ");
        let captured = facts["captured"].as_str().unwrap_or("");
        let taken = (captured.len() >= 10).then(|| join(vec![Some(day_label(&captured[..10])), captured.get(11..16).map(String::from)]));
        let camera = meta["camera"].as_str().unwrap_or("").to_string();
        let shutter = meta["shutter"].as_str().unwrap_or("").to_string();
        vec![
            ("File", record.file.name.clone()),
            ("Taken", taken.unwrap_or_default()),
            ("Camera", join(vec![Some(camera), meta["focal_mm"].as_f64().map(|f| format!("{} mm", (f * 10.0).round() / 10.0))])),
            ("Lens", meta["lens"].as_str().unwrap_or("").to_string()),
            (
                "Exposure",
                join(vec![
                    meta["aperture"].as_f64().map(|a| format!("ƒ/{a}")),
                    (!shutter.is_empty()).then(|| if shutter.ends_with('s') { shutter.clone() } else { format!("{shutter} s") }),
                    meta["iso"].as_f64().map(|i| format!("ISO {i}")),
                ]),
            ),
            ("Size", if facts["width"].is_number() { format!("{} × {} px", facts["width"], facts["height"]) } else { String::new() }),
        ]
    }

    pub fn browse_panel(&self, cx: &mut Context<Self>) -> AnyElement {
        let previewed: Vec<String> = self.browse.selection.iter().take(PREVIEW_COUNT).cloned().collect();
        self.library.update(cx, |l, cx| l.want(&previewed, cx));
        let library = self.library.read(cx);
        let selection = &self.browse.selection;
        let total = library.photos.len();
        let count = selection.len();
        let me = cx.entity();
        let flags: HashSet<String> = selection.iter().filter_map(|id| library.record(id)).map(|r| r.state.as_ref().map(|s| s.flag.clone()).unwrap_or_default()).collect();
        let current = (flags.len() == 1).then(|| flags.into_iter().next().unwrap_or_default());
        let thumbs: Vec<AnyElement> = previewed
            .iter()
            .map(|id| match library.picture(id).map(|p| p.image.clone()) {
                Some(image) => img(image).w(px(60.)).h(px(40.)).rounded(px(3.)).object_fit(ObjectFit::Cover).into_any_element(),
                None => div().w(px(60.)).h(px(40.)).rounded(px(3.)).bg(theme::surface()).into_any_element(),
            })
            .collect();
        let focused = self.selected.as_ref().and_then(|id| library.index_of(id).map(|i| (i, library.photos[i].clone())));
        let flag_options = [("pick", "Pick"), ("none", "None"), ("reject", "Reject")];
        let clear = me.clone();
        div()
            .id("browse-panel")
            .size_full()
            .overflow_y_scroll()
            .p(px(12.))
            .flex()
            .flex_col()
            .gap(px(theme::GAP))
            .child(
                div()
                    .h(px(28.))
                    .flex()
                    .items_center()
                    .justify_between()
                    .child(
                        div()
                            .flex()
                            .items_baseline()
                            .gap(px(6.))
                            .child(div().text_size(px(15.)).font_weight(FontWeight::SEMIBOLD).text_color(theme::focus()).child(if count == 0 { "No selection".to_string() } else { format!("{count} selected") }))
                            .child(div().text_size(px(12.)).font_features(theme::tabular()).text_color(theme::tertiary()).child(format!("of {total}"))),
                    )
                    .when(count > 0, |d| {
                        d.child(div().id("clear").px(px(8.)).py(px(3.)).rounded(px(6.)).text_size(px(12.)).text_color(theme::label()).cursor_pointer().hover(|s| s.bg(theme::hover())).on_click(move |_, _, cx| clear.update(cx, |app, cx| app.clear_selection(cx))).child("Clear"))
                    }),
            )
            .when(count == 0, |d| d.child(div().py(px(6.)).text_size(px(12.)).text_color(theme::tertiary()).child("Click a photo to select it, or press ⌘A to select all.")))
            .when(count > 0, |d| {
                let flag_me = me.clone();
                d.child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(6.))
                        .py(px(4.))
                        .children(thumbs)
                        .when(count > PREVIEW_COUNT, |d| d.child(div().flex_1().h(px(40.)).rounded(px(3.)).bg(theme::surface()).flex().items_center().justify_center().text_size(px(12.)).text_color(theme::label()).child(format!("+{}", count - PREVIEW_COUNT)))),
                )
                .child(
                    Segmented::new("flag", flag_options.iter().map(|(v, l)| (SharedString::from(*l), current.as_deref() == Some(*v))).collect(), move |i, _, cx| {
                        let flag = flag_options[i].0;
                        flag_me.update(cx, |app, cx| app.flag(flag, cx))
                    })
                    .label("Flag"),
                )
            })
            .when_some(focused, |d, (index, record)| {
                let rows = Self::info_rows(&record);
                d.child(div().h(px(1.)).my(px(6.)).bg(theme::divider())).child(
                    div()
                        .flex()
                        .flex_col()
                        .child(
                            div()
                                .h(px(28.))
                                .flex()
                                .items_center()
                                .justify_between()
                                .child(div().text_size(px(13.)).font_weight(FontWeight::SEMIBOLD).text_color(theme::label()).child("Info"))
                                .child(div().text_size(px(12.)).font_weight(FontWeight::BOLD).font_features(theme::tabular()).text_color(theme::accent()).child(format!("▸{}", index + 1))),
                        )
                        .children(rows.into_iter().filter(|(_, v)| !v.is_empty()).map(|(label, value)| {
                            div()
                                .h(px(28.))
                                .flex()
                                .items_center()
                                .justify_between()
                                .gap(px(12.))
                                .child(div().flex_none().text_size(px(13.)).font_weight(FontWeight::MEDIUM).text_color(theme::tertiary()).child(label))
                                .child(div().min_w_0().truncate().text_size(px(12.)).font_features(theme::tabular()).text_color(theme::label()).child(value))
                        })),
                )
            })
            .into_any_element()
    }
}

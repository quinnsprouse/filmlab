use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use super::app::{FilmlabApp, Mode};
use super::controls::Segmented;
use super::{icon, theme};
use crate::library::PhotoRecord;

const ROW_HEIGHT: f32 = 136.0;
const DEFAULT_ASPECT: f32 = 1.5;
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
}

struct Tile {
    id: String,
    index: usize,
    width: f32,
    time: String,
}

struct Day {
    key: String,
    label: String,
    tiles: Vec<Tile>,
}

const INDEXING: &str = "indexing";
const UNDATED: &str = "undated";

fn day_key(record: &PhotoRecord) -> String {
    let Some(probed) = &record.probed else { return INDEXING.into() };
    probed.facts["captured"].as_str().and_then(|c| c.get(0..10)).map(String::from).unwrap_or_else(|| UNDATED.into())
}

fn weekday(y: i64, m: i64, d: i64) -> usize {
    let (y, m) = if m < 3 { (y - 1, m + 12) } else { (y, m) };
    ((d + (13 * (m + 1)) / 5 + y + y / 4 - y / 100 + y / 400 + 6) % 7) as usize
}

pub fn day_label(key: &str) -> String {
    match key {
        INDEXING => return "Indexing".into(),
        UNDATED => return "Undated".into(),
        _ => {}
    }
    let parts: Vec<i64> = key.split('-').filter_map(|p| p.parse().ok()).collect();
    let [y, m, d] = parts[..] else { return key.into() };
    const DAYS: [&str; 7] = ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"];
    const MONTHS: [&str; 12] = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];
    format!("{}, {} {d}", DAYS[weekday(y, m, d)], MONTHS[(m.clamp(1, 12) - 1) as usize])
}

fn wanted(filter: Filter, record: &PhotoRecord) -> bool {
    let flag = record.state.as_ref().map(|s| s.flag.as_str()).unwrap_or("none");
    match filter {
        Filter::All => true,
        Filter::Picks => flag == "pick",
        Filter::Rejected => flag == "reject",
    }
}

fn group(photos: &[PhotoRecord], filter: Filter, sort: Sort) -> Vec<Day> {
    let mut days: Vec<Day> = Vec::new();
    for (index, record) in photos.iter().enumerate() {
        if !wanted(filter, record) {
            continue;
        }
        let facts = record.probed.as_ref().map(|p| &p.facts);
        let aspect = facts.and_then(|f| Some(f["width"].as_f64()? as f32 / f["height"].as_f64().filter(|h| *h > 0.0)? as f32)).unwrap_or(DEFAULT_ASPECT);
        let time = facts.and_then(|f| f["captured"].as_str()).and_then(|c| c.get(11..)).unwrap_or_default().to_string();
        let tile = Tile { id: record.file.id.clone(), index: index + 1, width: (ROW_HEIGHT * aspect).round(), time };
        let key = day_key(record);
        match days.iter_mut().find(|d| d.key == key) {
            Some(day) => day.tiles.push(tile),
            None => days.push(Day { label: day_label(&key), key, tiles: vec![tile] }),
        }
    }
    let rank = |k: &str| u8::from(k == INDEXING || k == UNDATED);
    days.sort_by(|a, b| rank(&a.key).cmp(&rank(&b.key)).then_with(|| if sort == Sort::Oldest { a.key.cmp(&b.key) } else { b.key.cmp(&a.key) }));
    for day in &mut days {
        day.tiles.sort_by(|a, b| match sort {
            Sort::Name => photos[a.index - 1].file.name.cmp(&photos[b.index - 1].file.name),
            _ => a.time.cmp(&b.time).then(a.index.cmp(&b.index)),
        });
    }
    days
}

fn count_label(n: usize) -> String {
    format!("{n} {}", if n == 1 { "photo" } else { "photos" })
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
            let tiles = self.browse.tiles.borrow();
            let current = tiles.get(&ids[at]).copied();
            current.and_then(|c| {
                let centre = c.center().x;
                ids.iter()
                    .filter_map(|id| tiles.get(id).map(|b| (id, b)))
                    .filter(|(_, b)| f32::from(b.top() - c.top()) * dy as f32 > 1.0)
                    .min_by(|(_, a), (_, b)| {
                        let row = |r: &&Bounds<Pixels>| (f32::from(r.top() - c.top()) * dy as f32, f32::from(r.center().x - centre).abs());
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

    fn tile(&self, tile: &Tile, record: &PhotoRecord, cx: &mut Context<Self>) -> AnyElement {
        let me = cx.entity();
        let id = tile.id.clone();
        let focused = self.selected.as_deref() == Some(id.as_str());
        let selected = self.browse.selection.contains(&id);
        let reject = record.state.as_ref().is_some_and(|s| s.flag == "reject");
        let flag = record.state.as_ref().map(|s| s.flag.clone()).unwrap_or_default();
        let tiles = self.browse.tiles.clone();
        let record_id = id.clone();
        let face = match (&record.thumbnail, &record.error) {
            (_, Some(_)) => div()
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
            (Some(image), None) => img(image.clone()).size_full().object_fit(ObjectFit::Cover).when(reject, |i| i.opacity(0.35)).into_any_element(),
            (None, None) => div().size_full().bg(theme::surface()).into_any_element(),
        };
        div()
            .id(SharedString::from(format!("tile-{id}")))
            .w(px(tile.width))
            .flex()
            .flex_col()
            .gap(px(6.))
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
                    .child(face)
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
                    .h(px(14.))
                    .flex()
                    .items_center()
                    .gap(px(6.))
                    .text_size(px(11.))
                    .font_features(theme::tabular())
                    .text_color(if focused { theme::accent() } else { theme::tertiary() })
                    .when(focused, |d| d.font_weight(FontWeight::BOLD))
                    .child(format!("▸{}", tile.index))
                    .when(flag == "pick", |d| d.child(icon::icon("flag02", px(12.))))
                    .when(flag == "reject", |d| d.child(icon::icon("cancel01", px(12.)))),
            )
            .into_any_element()
    }

    pub fn browse_grid(&mut self, left: f32, right: f32, top: f32, cx: &mut Context<Self>) -> AnyElement {
        let photos = self.library.read(cx).photos.clone();
        let days = group(&photos, self.browse.filter, self.browse.sort);
        self.browse.order = days.iter().flat_map(|d| d.tiles.iter().map(|t| t.id.clone())).collect();
        let selection = self.browse.selection.clone();
        let sections: Vec<AnyElement> = days
            .iter()
            .map(|day| {
                let chosen = day.tiles.iter().filter(|t| selection.contains(&t.id)).count();
                let tiles: Vec<AnyElement> = day.tiles.iter().map(|t| self.tile(t, &photos[t.index - 1], cx)).collect();
                div()
                    .flex()
                    .flex_col()
                    .gap(px(14.))
                    .pb(px(28.))
                    .child(
                        div()
                            .flex()
                            .items_baseline()
                            .gap(px(10.))
                            .child(div().text_size(px(15.)).font_weight(FontWeight::BOLD).text_color(theme::primary()).child(day.label.clone()))
                            .child(div().text_size(px(12.)).font_features(theme::tabular()).text_color(theme::tertiary()).child(format!(
                                "{}{}",
                                count_label(day.tiles.len()),
                                if chosen > 0 { format!(" · {chosen} selected") } else { String::new() }
                            ))),
                    )
                    .child(div().flex().flex_wrap().gap_x(px(6.)).gap_y(px(14.)).children(tiles))
                    .into_any_element()
            })
            .collect();
        let empty = match (photos.is_empty(), sections.is_empty(), self.browse.filter) {
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
            .px(px(16.))
            .pt(px(8.))
            .children(sections)
            .when_some(empty, |d, text| d.child(div().pt(px(80.)).flex().justify_center().text_size(px(13.)).text_color(theme::tertiary()).child(text)))
            .into_any_element()
    }

    pub fn browse_toolbar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let photos = &self.library.read(cx).photos;
        let picks = photos.iter().filter(|r| wanted(Filter::Picks, r)).count();
        let rejected = photos.iter().filter(|r| wanted(Filter::Rejected, r)).count();
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
                move || sorts.iter().map(|(s, l)| super::controls::PickerItem { label: l.to_string(), checked: *s == sort, header: false }).collect(),
                move |i, _, cx| {
                    pick_me.update(cx, |app, cx| {
                        app.browse.sort = sorts[i].0;
                        cx.notify();
                    })
                },
            ))
    }

    pub fn browse_rail(&self, cx: &mut Context<Self>) -> AnyElement {
        let library = self.library.read(cx);
        let name = library.folder.as_ref().and_then(|f| f.file_name()).map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
        let cover = library.photos.iter().find_map(|r| r.thumbnail.clone());
        let total = library.photos.len();
        let days = group(&library.photos, self.browse.filter, self.browse.sort);
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
                    .child(div().text_size(px(12.)).font_features(theme::tabular()).text_color(theme::tertiary()).child(day.tiles.len().to_string()))
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
        let library = self.library.read(cx);
        let selection = self.browse.selection.clone();
        let total = library.photos.len();
        let count = selection.len();
        let me = cx.entity();
        let flags: std::collections::HashSet<String> = library.photos.iter().filter(|r| selection.contains(&r.file.id)).map(|r| r.state.as_ref().map(|s| s.flag.clone()).unwrap_or_default()).collect();
        let current = (flags.len() == 1).then(|| flags.into_iter().next().unwrap_or_default());
        let thumbs: Vec<AnyElement> = selection
            .iter()
            .take(PREVIEW_COUNT)
            .map(|id| match library.record(id).and_then(|r| r.thumbnail.clone()) {
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

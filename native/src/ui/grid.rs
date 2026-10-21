use std::collections::HashMap;
use std::ops::Range;

use super::browse::{Filter, Sort};
use crate::library::{Library, PhotoRecord};

pub const ROW_HEIGHT: f32 = 136.0;
pub const CAPTION_GAP: f32 = 6.0;
pub const CAPTION: f32 = 14.0;
pub const GAP_X: f32 = 6.0;
pub const GAP_Y: f32 = 14.0;
pub const HEADER_GAP: f32 = 14.0;
pub const SECTION_END: f32 = 28.0;
pub const PAD_X: f32 = 16.0;
pub const PAD_TOP: f32 = 8.0;
pub const TILE_HEIGHT: f32 = ROW_HEIGHT + CAPTION_GAP + CAPTION;
const ROW_PITCH: f32 = TILE_HEIGHT + GAP_Y;
const DEFAULT_ASPECT: f32 = 1.5;
const HEADER_GUESS: f32 = 24.0;
const INDEXING: &str = "indexing";
const UNDATED: &str = "undated";

pub struct Tile {
    pub id: String,
    pub index: usize,
    pub width: f32,
    time: String,
}

pub struct Day {
    pub key: String,
    pub label: String,
    pub tiles: Vec<Tile>,
}

/// One day of the grid as laid out: where it starts, how tall it is, and where its tiles break into rows.
pub struct Section<'a> {
    pub day: &'a Day,
    pub top: f32,
    pub height: f32,
    pub rows: &'a [Range<usize>],
}

struct Grouping {
    revision: u64,
    filter: Filter,
    sort: Sort,
    days: Vec<Day>,
}

struct Flow {
    width: f32,
    rows: Vec<Vec<Range<usize>>>,
}

/// The browse grid's layout, kept until the photos, the filter, the sort or the width change, so a frame only builds
/// the tiles it shows.
#[derive(Default)]
pub struct Grid {
    grouping: Option<Grouping>,
    flow: Option<Flow>,
    header: Option<f32>,
}

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

pub fn passes(filter: Filter, record: &PhotoRecord) -> bool {
    let flag = record.state.as_ref().map(|s| s.flag.as_str()).unwrap_or("none");
    match filter {
        Filter::All => true,
        Filter::Picks => flag == "pick",
        Filter::Rejected => flag == "reject",
    }
}

fn group(photos: &[PhotoRecord], filter: Filter, sort: Sort) -> Vec<Day> {
    let mut days: Vec<Day> = Vec::new();
    let mut by_key: HashMap<String, usize> = HashMap::new();
    for (index, record) in photos.iter().enumerate() {
        if !passes(filter, record) {
            continue;
        }
        let facts = record.probed.as_ref().map(|p| &p.facts);
        let aspect = facts.and_then(|f| Some(f["width"].as_f64()? as f32 / f["height"].as_f64().filter(|h| *h > 0.0)? as f32)).unwrap_or(DEFAULT_ASPECT);
        let time = facts.and_then(|f| f["captured"].as_str()).and_then(|c| c.get(11..)).unwrap_or_default().to_string();
        let tile = Tile { id: record.file.id.clone(), index: index + 1, width: (ROW_HEIGHT * aspect).round(), time };
        let key = day_key(record);
        match by_key.get(&key) {
            Some(at) => days[*at].tiles.push(tile),
            None => {
                by_key.insert(key.clone(), days.len());
                days.push(Day { label: day_label(&key), key, tiles: vec![tile] });
            }
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

/// Breaks tiles into rows the way a wrapping flex row of `width` does: a tile starts a new row when it would overflow.
fn rows_of(tiles: &[Tile], width: f32) -> Vec<Range<usize>> {
    let mut rows = Vec::new();
    let (mut start, mut line) = (0, 0.0);
    for (at, tile) in tiles.iter().enumerate() {
        let next = if at == start { tile.width } else { line + GAP_X + tile.width };
        if at > start && next > width {
            rows.push(start..at);
            (start, line) = (at, tile.width);
        } else {
            line = next;
        }
    }
    if start < tiles.len() {
        rows.push(start..tiles.len());
    }
    rows
}

fn rows_height(rows: usize) -> f32 {
    rows as f32 * ROW_PITCH - GAP_Y
}

/// Where row `row` of a day's tiles starts, below that day's header.
pub fn row_top(row: usize) -> f32 {
    row as f32 * ROW_PITCH
}

impl Grid {
    /// Groups the photos again if the library, the filter or the sort changed since; true when it did.
    pub fn regroup(&mut self, library: &Library, filter: Filter, sort: Sort) -> bool {
        let revision = library.revision();
        if self.grouping.as_ref().is_some_and(|g| g.revision == revision && g.filter == filter && g.sort == sort) {
            return false;
        }
        self.grouping = Some(Grouping { revision, filter, sort, days: group(&library.photos, filter, sort) });
        self.flow = None;
        true
    }

    pub fn days(&self) -> &[Day] {
        self.grouping.as_ref().map_or(&[], |g| &g.days)
    }

    /// The photos as the grid shows them, after the filter and the sort.
    pub fn order(&self) -> Vec<String> {
        self.days().iter().flat_map(|d| d.tiles.iter().map(|t| t.id.clone())).collect()
    }

    pub fn reflow(&mut self, width: f32) {
        if self.flow.as_ref().is_some_and(|f| f.width == width) {
            return;
        }
        let rows = self.days().iter().map(|d| rows_of(&d.tiles, width)).collect();
        self.flow = Some(Flow { width, rows });
    }

    /// The height of a day's header, once one has been laid out; a guess before.
    pub fn header(&self) -> f32 {
        self.header.unwrap_or(HEADER_GUESS)
    }

    pub fn header_known(&self) -> bool {
        self.header.is_some()
    }

    /// Learns the header's height from how tall the first day's section was laid out with `rows` rows.
    pub fn measured(&mut self, section_height: f32, rows: usize) -> bool {
        let header = section_height - HEADER_GAP - rows_height(rows) - SECTION_END;
        let changed = header > 0.0 && self.header.is_none_or(|known| (known - header).abs() > 0.25);
        if changed {
            self.header = Some(header);
        }
        changed
    }

    pub fn sections(&self) -> Vec<Section<'_>> {
        let Some(flow) = &self.flow else { return Vec::new() };
        let header = self.header();
        let mut top = PAD_TOP;
        self.days()
            .iter()
            .zip(&flow.rows)
            .map(|(day, rows)| {
                let height = header + HEADER_GAP + rows_height(rows.len()) + SECTION_END;
                let section = Section { day, top, height, rows };
                top += height;
                section
            })
            .collect()
    }

    /// Each shown photo's tile as (top, horizontal centre) in the grid's content, for moving between rows.
    pub fn places(&self) -> HashMap<&str, (f32, f32)> {
        let mut places = HashMap::new();
        for section in self.sections() {
            let rows_top = section.top + self.header() + HEADER_GAP;
            for (row, range) in section.rows.iter().enumerate() {
                let mut x = 0.0;
                for tile in &section.day.tiles[range.clone()] {
                    places.insert(tile.id.as_str(), (rows_top + row_top(row), x + tile.width / 2.0));
                    x += tile.width + GAP_X;
                }
            }
        }
        places
    }
}

impl Section<'_> {
    pub fn rows_top(&self, header: f32) -> f32 {
        self.top + header + HEADER_GAP
    }

    pub fn rows_height(&self) -> f32 {
        rows_height(self.rows.len())
    }

    /// The rows of this day that overlap `from..to` of the grid's content.
    pub fn rows_within(&self, header: f32, from: f32, to: f32) -> Range<usize> {
        let top = self.rows_top(header);
        let first = ((from - top) / ROW_PITCH).floor().max(0.0) as usize;
        let last = (((to - top) / ROW_PITCH).ceil().max(0.0) as usize).min(self.rows.len());
        first.min(last)..last
    }
}

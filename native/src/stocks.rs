use std::path::{Path, PathBuf};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum StockKind {
    Negative,
    Print,
    Utility,
}

#[derive(Clone, Debug)]
pub struct Stock {
    pub id: String,
    pub kind: StockKind,
    pub name: String,
    pub group: String,
}

impl Stock {
    pub fn label(&self) -> String {
        if self.group.is_empty() || self.group == "Standard" { self.name.clone() } else { format!("{} · {}", self.name, self.group) }
    }
}

pub fn folder() -> PathBuf {
    std::env::var_os("FILMVISION_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/Library/Application Support/Blackmagic Design/DaVinci Resolve/LUT/FilmVision Pro"))
}

fn cubes(dir: &Path, path: &mut Vec<String>, out: &mut Vec<Vec<String>>) {
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    let mut entries: Vec<_> = entries.flatten().collect();
    entries.sort_by_key(|e| e.file_name());
    for entry in entries {
        let name = entry.file_name().to_string_lossy().to_string();
        if name.starts_with('.') {
            continue;
        }
        path.push(name.clone());
        if entry.file_type().is_ok_and(|t| t.is_dir()) {
            cubes(&entry.path(), path, out);
        } else if name.to_lowercase().ends_with(".cube") {
            out.push(path.clone());
        }
        path.pop();
    }
}

fn strip_prefix(text: &str, digits: bool) -> &str {
    let Some((head, rest)) = text.split_once("- ") else { return text };
    let ok = if digits { head.chars().all(|c| c.is_ascii_digit()) } else { head.chars().all(|c| c == 'I' || c == 'V') };
    if ok && !head.is_empty() { rest } else { text }
}

fn stock_of(path: &[String]) -> Option<Stock> {
    let (kind, rest) = path.split_first()?;
    let kind = match kind.as_str() {
        "Negative" => StockKind::Negative,
        "Print" => StockKind::Print,
        "Utility" => StockKind::Utility,
        _ => return None,
    };
    let file = rest.last()?;
    let stem = &file[..file.len() - ".cube".len()];
    let name = strip_prefix(stem.strip_prefix("Print ").unwrap_or(stem), true).to_string();
    let group = if rest.len() > 1 { strip_prefix(&rest[0], false).to_string() } else { String::new() };
    let joined = path.join("/");
    Some(Stock { id: joined[..joined.len() - ".cube".len()].to_string(), kind, name, group })
}

pub fn scan() -> Vec<Stock> {
    let mut found = Vec::new();
    cubes(&folder(), &mut Vec::new(), &mut found);
    found.iter().filter_map(|p| stock_of(p)).collect()
}

pub fn lut_text(id: &str) -> std::io::Result<String> {
    std::fs::read_to_string(folder().join(format!("{id}.cube")))
}

pub const DEFAULT_NEGATIVE: &str = "Negative/Vision3 250D";
pub const DEFAULT_PRINT: &str = "Print/I- Standard/Print SA1 Kodak 2393";

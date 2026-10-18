use std::borrow::Cow;

use gpui_kit::{AssetSource, Result, SharedString};
use rust_embed::RustEmbed;

#[derive(RustEmbed)]
#[folder = "assets"]
#[include = "icons/*.svg"]
#[include = "fonts/*.otf"]
struct Bundled;

pub struct Assets;

impl AssetSource for Assets {
    fn load(&self, path: &str) -> Result<Option<Cow<'static, [u8]>>> {
        match Bundled::get(path) {
            Some(file) => Ok(Some(file.data)),
            None => gpui_kit::assets::Assets.load(path),
        }
    }

    fn list(&self, path: &str) -> Result<Vec<SharedString>> {
        let mut found: Vec<SharedString> = Bundled::iter().filter(|p| p.starts_with(path)).map(|p| p.to_string().into()).collect();
        found.extend(gpui_kit::assets::Assets.list(path)?);
        Ok(found)
    }
}

pub fn fonts() -> Vec<Cow<'static, [u8]>> {
    Bundled::iter().filter(|p| p.starts_with("fonts/")).filter_map(|p| Bundled::get(&p)).map(|f| f.data).collect()
}

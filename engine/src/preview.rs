use std::path::Path;

/// A camera's own JPEG preview of a photo, in sRGB, upright, as RGBA.
pub struct Preview {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

/// The preview a raw file carries, no larger than `max_edge`, read from the file's header and the preview itself
/// rather than the whole file. `None` for files without one, such as JPEGs and PNGs.
pub fn embedded_preview(path: &Path, max_edge: usize) -> Option<Preview> {
    let image = (lightcraft_engine::files::fs_preview_loader())(path.to_str()?, max_edge)?;
    Some(Preview { width: u32::try_from(image.width).ok()?, height: u32::try_from(image.height).ok()?, rgba: image.data.into_flattened() })
}

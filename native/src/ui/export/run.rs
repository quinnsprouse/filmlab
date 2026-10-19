use std::fs::OpenOptions;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use filmlab_engine::core::{self, Decoded, Engine};
use serde_json::{Value, json};

use crate::develop::Develop;
use crate::film::render_film;
use crate::library::{self, Library, PhotoRecord};

const KEY: &str = "export";

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Format {
    Jpeg,
    Png,
    Tiff,
}

pub const FORMATS: [Format; 3] = [Format::Jpeg, Format::Png, Format::Tiff];

impl Format {
    pub fn label(self) -> &'static str {
        match self {
            Format::Jpeg => "JPEG",
            Format::Png => "PNG",
            Format::Tiff => "TIFF",
        }
    }

    fn key(self) -> &'static str {
        match self {
            Format::Jpeg => "jpeg",
            Format::Png => "png",
            Format::Tiff => "tiff",
        }
    }

    fn extension(self) -> &'static str {
        match self {
            Format::Jpeg => "jpg",
            Format::Png => "png",
            Format::Tiff => "tif",
        }
    }
}

#[derive(Clone, Copy)]
pub struct Choices {
    pub format: Format,
    pub long_edge: Option<u32>,
    pub quality: u32,
}

impl Default for Choices {
    fn default() -> Self {
        Choices { format: Format::Jpeg, long_edge: None, quality: 90 }
    }
}

impl Choices {
    fn options(&self) -> String {
        let mut options = json!({"format": self.format.key(), "quality": self.quality});
        if let Some(edge) = self.long_edge {
            options["resize"] = json!({"mode": "longEdge", "value": edge});
        }
        options.to_string()
    }
}

#[derive(Clone)]
pub struct Job {
    pub name: String,
    pub stem: String,
    path: PathBuf,
    facts: String,
    state: String,
    film: String,
    loupe: Option<Loupe>,
}

/// The loupe's decode of the photo at the size its export decodes to.
#[derive(Clone)]
pub struct Loupe {
    engine: Arc<Mutex<Engine>>,
    key: String,
}

impl Loupe {
    /// The decode, shared rather than copied, if the loupe still holds it. Takes the loupe's engine lock, so it is
    /// called outside the render pools.
    pub fn decoded(&self) -> Option<Decoded> {
        self.engine.lock().ok()?.decoded(&self.key)
    }
}

impl Job {
    pub fn loupe(&self) -> Option<&Loupe> {
        self.loupe.as_ref()
    }
}

pub fn job_of(record: &PhotoRecord) -> Result<Job, String> {
    let (Some(probed), Some(state)) = (&record.probed, &record.state) else {
        return Err("The photo hasn't finished loading. Wait a moment and try again.".into());
    };
    let film = render_film(state.film().as_ref()).map_or(Value::Null, |f| f.to_value());
    Ok(Job {
        name: record.file.name.clone(),
        stem: record.file.stem.clone(),
        path: record.file.path.clone(),
        facts: probed.facts.to_string(),
        state: serde_json::to_string(state).map_err(|e| e.to_string())?,
        film: film.to_string(),
        loupe: None,
    })
}

fn max_edge(job: &Job, choices: &Choices) -> Result<usize, String> {
    let plan: Value = serde_json::from_str(&core::export_plan(&job.facts, &job.state, &choices.options())?).map_err(|e| e.to_string())?;
    Ok(plan["maxEdge"].as_u64().unwrap_or(0) as usize)
}

// why: the photo open in Develop is usually already decoded at full size (about 300 MB for 24 MP), so its export
// shares that decode instead of making a second one
pub fn jobs_for(library: &Library, develop: &Develop, ids: &[String], choices: &Choices) -> Vec<(String, Result<Job, String>)> {
    let job = |id: &String| -> Result<Job, String> {
        let mut job = job_of(library.record(id).ok_or("The photo is no longer in the folder.")?)?;
        let loupe = max_edge(&job, choices).ok().and_then(|edge| develop.decode_at(id, edge));
        job.loupe = loupe.map(|(engine, key)| Loupe { engine, key });
        Ok(job)
    };
    ids.iter().map(|id| (id.clone(), job(id))).collect()
}

fn create_unique(folder: &Path, stem: &str, extension: &str) -> Result<(PathBuf, std::fs::File), String> {
    std::fs::create_dir_all(folder).map_err(|e| format!("Couldn't create {}: {e}", folder.display()))?;
    for n in 1..1000 {
        let name = if n == 1 { format!("{stem}.{extension}") } else { format!("{stem} {n}.{extension}") };
        let path = folder.join(name);
        match OpenOptions::new().write(true).create_new(true).open(&path) {
            Ok(file) => return Ok((path, file)),
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(format!("Couldn't write {}: {e}", path.display())),
        }
    }
    Err("Too many exports of this photo already exist.".into())
}

/// Exports `job`, from `decoded` (the loupe's decode at the export's size) when given.
pub fn export_one(engine: &mut Engine, job: &Job, choices: &Choices, folder: Option<&Path>, decoded: Option<Decoded>) -> Result<PathBuf, String> {
    let options = choices.options();
    let bytes = std::fs::read(&job.path).map_err(|e| format!("Couldn't read the photo: {e}"))?;
    let film = serde_json::from_str::<Value>(&job.film).ok().and_then(|v| crate::film::FilmSettings::of(&v));
    library::ensure_luts(engine, film.as_ref())?;
    match decoded {
        Some(decoded) => {
            engine.insert(KEY, decoded);
            engine.read_original(KEY, &bytes);
        }
        None => engine.load(KEY, &job.facts, &bytes, max_edge(job, choices)?)?,
    }
    let encoded = engine.export(KEY, &job.facts, &job.state, &options, &job.film);
    engine.release(KEY);
    let encoded = encoded?;
    let folder = folder.map_or_else(|| job.path.parent().unwrap_or(Path::new(".")).join("Exports"), Path::to_path_buf);
    let (path, mut file) = create_unique(&folder, &job.stem, choices.format.extension())?;
    if let Err(e) = file.write_all(&encoded) {
        let _ = std::fs::remove_file(&path);
        return Err(format!("Couldn't write {}: {e}", path.display()));
    }
    Ok(path)
}

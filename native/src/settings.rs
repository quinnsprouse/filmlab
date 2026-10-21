use std::ops::Deref;
use std::sync::Arc;

use filmlab_engine::core::DevelopSettings;
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// A photo's develop settings, shared rather than copied between the edit, its history, its renders and its saves.
/// They are written out as LightCraft's JSON only where they leave the app: sidecars, MCP and the clipboard.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Settings(Arc<DevelopSettings>);

impl Deref for Settings {
    type Target = DevelopSettings;

    fn deref(&self) -> &DevelopSettings {
        &self.0
    }
}

impl From<DevelopSettings> for Settings {
    fn from(settings: DevelopSettings) -> Settings {
        Settings(Arc::new(settings))
    }
}

impl From<Arc<DevelopSettings>> for Settings {
    fn from(settings: Arc<DevelopSettings>) -> Settings {
        Settings(settings)
    }
}

impl Settings {
    /// Settings given as JSON, failing with the engine's own words for settings it can't read.
    pub fn parse(value: &Value) -> Result<Settings, String> {
        serde_json::from_str(&value.to_string()).map_err(|e| e.to_string())
    }

    pub fn to_value(&self) -> Value {
        serde_json::to_value(&self.0).unwrap_or_default()
    }

    pub fn json(&self) -> String {
        serde_json::to_string(&self.0).unwrap_or_default()
    }

    /// The settings to change, copied first when anything else shares them.
    pub fn edit(&mut self) -> &mut DevelopSettings {
        Arc::make_mut(&mut self.0)
    }
}

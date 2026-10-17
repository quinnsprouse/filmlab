#![forbid(unsafe_code)]

pub mod core;
pub mod film;

#[cfg(feature = "threads")]
pub use wasm_bindgen_rayon::init_thread_pool;

use wasm_bindgen::prelude::*;

fn js(e: String) -> JsError {
    JsError::new(&e)
}

#[wasm_bindgen(start)]
pub fn start() {
    console_error_panic_hook::set_once();
}

#[wasm_bindgen]
pub fn controls() -> Result<String, JsError> {
    core::controls().map_err(js)
}

#[wasm_bindgen]
pub fn probe(name: &str, bytes: &[u8]) -> Result<String, JsError> {
    core::probe(name, bytes).map_err(js)
}

#[wasm_bindgen(js_name = readSidecar)]
pub fn read_sidecar(facts: &str, xmp: &str) -> Result<String, JsError> {
    core::read_sidecar(facts, xmp).map_err(js)
}

#[wasm_bindgen(js_name = writeSidecar)]
pub fn write_sidecar(facts: &str, state: &str, existing: Option<String>) -> Result<String, JsError> {
    core::write_sidecar(facts, state, existing).map_err(js)
}

#[wasm_bindgen(js_name = extractProfile)]
pub fn extract_profile(settings: &str, groups: &str) -> Result<String, JsError> {
    core::extract_profile(settings, groups).map_err(js)
}

#[wasm_bindgen(js_name = applyProfile)]
pub fn apply_profile(settings: &str, partial: &str, amount: f64) -> Result<String, JsError> {
    core::apply_profile(settings, partial, amount).map_err(js)
}

#[wasm_bindgen(js_name = controlValues)]
pub fn control_values(settings: &str) -> Result<String, JsError> {
    core::control_values(settings).map_err(js)
}

#[wasm_bindgen(js_name = setControls)]
pub fn set_controls(facts: &str, settings: &str, values: &str) -> Result<String, JsError> {
    core::set_controls(facts, settings, values).map_err(js)
}

#[wasm_bindgen(js_name = whiteBalance)]
pub fn white_balance(facts: &str, settings: &str) -> Result<String, JsError> {
    core::white_balance(facts, settings).map_err(js)
}

#[wasm_bindgen(js_name = setWhiteBalanceMode)]
pub fn set_white_balance_mode(facts: &str, settings: &str, mode: &str) -> Result<String, JsError> {
    core::set_white_balance_mode(facts, settings, mode).map_err(js)
}

#[wasm_bindgen(js_name = exportPlan)]
pub fn export_plan(facts: &str, state: &str, options: &str) -> Result<String, JsError> {
    core::export_plan(facts, state, options).map_err(js)
}

#[wasm_bindgen(js_name = mapPoints)]
pub fn map_points(facts: &str, settings: &str, request: &str) -> Result<String, JsError> {
    core::map_points(facts, settings, request).map_err(js)
}

#[wasm_bindgen(js_name = filmOf)]
pub fn film_of(xmp: &str) -> Option<String> {
    core::film_of(xmp).map(|v| v.to_string())
}

#[wasm_bindgen(js_name = outputSize)]
pub fn output_size(facts: &str, settings: &str) -> Result<String, JsError> {
    core::native_size(facts, settings).map_err(js)
}

#[wasm_bindgen(js_name = normalizeSettings)]
pub fn normalize_settings(settings: &str) -> Result<String, JsError> {
    core::normalize_settings(settings).map_err(js)
}

#[wasm_bindgen]
pub struct Frame(core::Frame);

#[wasm_bindgen]
impl Frame {
    #[wasm_bindgen(getter)]
    pub fn width(&self) -> u32 {
        self.0.width
    }

    #[wasm_bindgen(getter)]
    pub fn height(&self) -> u32 {
        self.0.height
    }

    #[wasm_bindgen(js_name = takeRgba)]
    pub fn take_rgba(&mut self) -> Vec<u8> {
        self.0.take_rgba()
    }

    #[wasm_bindgen(js_name = takeHistogram)]
    pub fn take_histogram(&mut self) -> Vec<u32> {
        self.0.take_histogram()
    }
}

#[wasm_bindgen]
#[derive(Default)]
pub struct Engine(core::Engine);

#[wasm_bindgen]
impl Engine {
    #[wasm_bindgen(constructor)]
    pub fn new() -> Engine {
        Engine::default()
    }

    pub fn load(&mut self, key: &str, facts: &str, bytes: &[u8], max_edge: usize) -> Result<(), JsError> {
        self.0.load(key, facts, bytes, max_edge).map_err(js)
    }

    pub fn has(&self, key: &str) -> bool {
        self.0.has(key)
    }

    pub fn release(&mut self, key: &str) {
        self.0.release(key);
    }

    pub fn render(&mut self, key: &str, settings: &str, view: &str) -> Result<Frame, JsError> {
        self.0.render(key, settings, view).map(Frame).map_err(js)
    }

    pub fn export(&mut self, key: &str, facts: &str, state: &str, options: &str, film: &str) -> Result<Vec<u8>, JsError> {
        self.0.export(key, facts, state, options, film).map_err(js)
    }

    #[wasm_bindgen(js_name = loadLut)]
    pub fn load_lut(&mut self, id: &str, text: &str) -> Result<(), JsError> {
        self.0.load_lut(id, text).map_err(js)
    }

    #[wasm_bindgen(js_name = hasLut)]
    pub fn has_lut(&self, id: &str) -> bool {
        self.0.has_lut(id)
    }
}

#[wasm_bindgen]
#[derive(Default)]
pub struct Commands(core::Commands);

#[wasm_bindgen]
impl Commands {
    #[wasm_bindgen(constructor)]
    pub fn new() -> Commands {
        Commands::default()
    }

    pub fn open(&mut self, facts: &str, settings: &str) -> Result<(), JsError> {
        self.0.open(facts, settings).map_err(js)
    }

    pub fn run(&mut self, command: &str, params: &str) -> Result<String, JsError> {
        self.0.run(command, params).map_err(js)
    }

    pub fn attach(&mut self, bytes: Vec<u8>) {
        self.0.attach(bytes);
    }

    pub fn detach(&mut self) {
        self.0.detach();
    }
}


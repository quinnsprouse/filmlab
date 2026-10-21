use std::sync::{Arc, Mutex};

use filmlab_engine::core::Engine;

type Chore = Box<dyn FnOnce(&mut Engine) + Send>;

#[derive(Clone, Default)]
pub struct Shared {
    engine: Arc<Mutex<Engine>>,
    chores: Arc<Mutex<Vec<Chore>>>,
}

impl Shared {
    pub fn engine(&self) -> Arc<Mutex<Engine>> {
        self.engine.clone()
    }

    pub fn with<R>(&self, work: impl FnOnce(&mut Engine) -> R) -> Result<R, String> {
        let mut engine = self.engine.lock().map_err(|e| e.to_string())?;
        self.catch_up(&mut engine);
        Ok(work(&mut engine))
    }

    fn catch_up(&self, engine: &mut Engine) {
        let chores = self.chores.lock().map(|mut c| std::mem::take(&mut *c)).unwrap_or_default();
        for chore in chores {
            chore(engine);
        }
    }

    pub fn later(&self, chore: impl FnOnce(&mut Engine) + Send + 'static) -> bool {
        if let Ok(mut chores) = self.chores.lock() {
            chores.push(Box::new(chore));
        }
        let Ok(mut engine) = self.engine.try_lock() else { return false };
        self.catch_up(&mut engine);
        true
    }
}

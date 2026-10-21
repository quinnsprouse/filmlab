use std::cell::{Cell, RefCell};
use std::sync::Arc;

pub type Wanted = Arc<dyn Fn() -> bool + Send + Sync>;

// why: Filmlab: a pool thread waiting inside one render can pick up another render whole (rayon helping), so the
// check belongs to the rendering thread, as the film hook does
thread_local! {
    static WANTED: RefCell<Option<Wanted>> = const { RefCell::new(None) };
    static STOPPED: Cell<bool> = const { Cell::new(false) };
}

struct Restore(Option<Wanted>, bool);

impl Drop for Restore {
    fn drop(&mut self) {
        let previous = self.0.take();
        WANTED.with(|w| *w.borrow_mut() = previous);
        STOPPED.set(self.1);
    }
}

pub fn while_wanted<R>(wanted: Option<Wanted>, run: impl FnOnce() -> R) -> R {
    let _restore = Restore(WANTED.with(|w| w.replace(wanted)), STOPPED.replace(false));
    run()
}

pub fn stopped() -> bool {
    if !STOPPED.get() && WANTED.with(|w| w.borrow().as_ref().is_some_and(|wanted| !wanted())) {
        STOPPED.set(true);
    }
    STOPPED.get()
}

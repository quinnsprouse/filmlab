use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use gpui_kit::*;

use crate::glass::Glass;

#[derive(Default)]
struct Panes {
    glass: HashMap<SharedString, Glass>,
    seen: Vec<SharedString>,
}

#[derive(Clone, Default)]
pub struct GlassLayer(Rc<RefCell<Panes>>);

impl GlassLayer {
    pub fn begin(&self) {
        self.0.borrow_mut().seen.clear();
    }

    fn place(&self, id: &SharedString, bounds: Bounds<Pixels>, radius: f64, window: &Window) {
        let mut panes = self.0.borrow_mut();
        if !panes.glass.contains_key(id)
            && let Some(glass) = Glass::new(window, false)
        {
            panes.glass.insert(id.clone(), glass);
        }
        if let Some(glass) = panes.glass.get_mut(id) {
            glass.place(window, bounds, radius);
        }
        panes.seen.push(id.clone());
    }

    fn end(&self) {
        let mut panes = self.0.borrow_mut();
        let seen = std::mem::take(&mut panes.seen);
        for (id, glass) in panes.glass.iter_mut() {
            if !seen.contains(id) {
                glass.hide();
            }
        }
        panes.seen = seen;
    }

    // why: a pane is drawn by AppKit under GPUI's layer, so it follows the element's laid-out bounds
    pub fn pane(&self, id: impl Into<SharedString>, radius: f32) -> Div {
        let id = id.into();
        let layer = self.clone();
        div().relative().child(
            canvas(move |bounds, window, _| layer.place(&id, bounds, f64::from(radius), window), |_, _, _, _| {}).absolute().top_0().left_0().size_full(),
        )
    }

    pub fn finish(&self) -> impl IntoElement {
        let layer = self.clone();
        canvas(move |_, _, _| layer.end(), |_, _, _, _| {}).absolute().size_0()
    }
}

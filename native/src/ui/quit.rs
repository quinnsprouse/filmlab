use gpui_kit::*;

use crate::develop::Develop;
use crate::library::Library;

/// Where the edits not yet on disk wait: the open photo's debounced save, and the library's sidecar writes.
struct Unsaved {
    develop: WeakEntity<Develop>,
    library: WeakEntity<Library>,
}

impl Global for Unsaved {}

/// Saves before the app or its window goes: on Quit, when the window closes, and when macOS quits the app itself.
pub fn install(develop: &Entity<Develop>, library: &Entity<Library>, window: &Window, cx: &mut App) {
    cx.set_global(Unsaved { develop: develop.downgrade(), library: library.downgrade() });
    window.on_window_should_close(cx, |_, cx| {
        save(cx);
        true
    });
    cx.on_app_quit(|cx| {
        save(cx);
        async {}
    })
    .detach();
}

/// Writes the open photo's pending edit and every sidecar not on disk yet, before returning.
pub fn save(cx: &mut App) {
    let Some(unsaved) = cx.try_global::<Unsaved>() else { return };
    let (develop, library) = (unsaved.develop.upgrade(), unsaved.library.upgrade());
    if let Some(develop) = develop {
        develop.update(cx, |d, cx| d.flush(cx));
    }
    if let Some(library) = library {
        library.update(cx, |l, _| l.save_now());
    }
}

pub fn quit(cx: &mut App) {
    save(cx);
    cx.quit();
}

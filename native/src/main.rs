use filmlab::assets::{Assets, fonts};
use filmlab::gpu::{Gpu, Runner};
use filmlab::ui::app::{FilmlabApp, bind};
use filmlab::ui::theme;
use gpui_kit::component::{Theme, ThemeMode};
use gpui_kit::*;

// why: the system allocator keeps freed multi-megabyte blocks (every film frame's buffers) cached for good, so the
// app's footprint only ever grew; mimalloc returns them to the system
#[cfg(not(any(feature = "profile", feature = "dhat")))]
#[global_allocator]
static ALLOCATOR: mimalloc::MiMalloc = mimalloc::MiMalloc;

#[cfg(all(feature = "profile", not(feature = "dhat")))]
#[global_allocator]
static ALLOCATOR: tracy_client::ProfiledAllocator<mimalloc::MiMalloc> = tracy_client::ProfiledAllocator::new(mimalloc::MiMalloc, 0);

#[cfg(feature = "dhat")]
#[global_allocator]
static ALLOCATOR: dhat::Alloc = dhat::Alloc;

// why: mimalloc tags its pages 100 by default, the tag `footprint`, vmmap and Instruments name "IOAccelerator", so the
// heap read as GPU memory; tag 240 (VM_MEMORY_APPLICATION_SPECIFIC_1) shows it as the app's own. A page keeps the tag
// it was mapped with, and Rust's runtime allocates before main, so the tag is set by an initializer that runs first
const HEAP_TAG: std::ffi::c_long = 240;

#[used]
#[unsafe(link_section = "__DATA,__mod_init_func")]
static TAG_HEAP: extern "C" fn() = tag_heap;

extern "C" fn tag_heap() {
    unsafe { libmimalloc_sys::mi_option_set(libmimalloc_sys::mi_option_os_tag, HEAP_TAG) };
}

fn main() {
    filmlab::profile::start();
    let gpu = Gpu::open();
    if let Some(gpu) = &gpu {
        filmlab::gpu::share(gpu);
        // why: FILMLAB_CPU=1 keeps the film stage on the CPU, to compare against the GPU path
        if std::env::var_os("FILMLAB_CPU").is_none() {
            filmlab_engine::install_gpu(Box::new(Runner(gpu.clone())));
        }
    }
    // why: scripted runs (the end-to-end test, MCP test launches with FILMLAB_BACKGROUND=1) leave the user's focus alone
    let e2e = ["FILMLAB_E2E", "FILMLAB_BACKGROUND"].iter().any(|k| std::env::var_os(k).is_some());
    gpui_kit::application().with_assets(Assets).run(move |cx| {
        gpui_kit::init(cx);
        let _ = cx.text_system().add_fonts(fonts());
        Theme::change(ThemeMode::Dark, None, cx);
        let theme_now = Theme::global_mut(cx);
        theme_now.tokens.background = transparent_black().into();
        theme_now.font_family = theme::FONT.into();
        bind(cx);
        let options = WindowOptions {
            // why: macOS stops presenting a window that's fully covered, and a scripted run opens behind the user's own
            // (centred) Filmlab window, so it sits at the top left where it's never wholly hidden
            window_bounds: Some(WindowBounds::Windowed(if e2e { Bounds::new(point(px(0.), px(0.)), size(px(1480.), px(940.))) } else { Bounds::centered(None, size(px(1480.), px(940.)), cx) })),
            titlebar: Some(TitlebarOptions { title: Some("Filmlab".into()), appears_transparent: true, traffic_light_position: Some(point(px(18.), px(26.))) }),
            window_background: WindowBackgroundAppearance::Transparent,
            window_min_size: Some(size(px(900.), px(600.))),
            focus: !e2e,
            ..Default::default()
        };
        let gpu = gpu.clone();
        let (window, app) = gpui_kit::open_window(options, cx, move |window, cx| cx.new(|cx| FilmlabApp::new(gpu, window, cx))).expect("window");
        filmlab::profile::on_quit(cx);
        filmlab::ui::e2e::start(&app, window, cx);
        filmlab::mcp::start(&app, window, cx);
        if !e2e {
            cx.activate(true);
        }
    });
}

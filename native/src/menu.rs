use std::cell::Cell;

use block2::RcBlock;
use futures::channel::oneshot;
use gpui_kit::{Bounds, Pixels, Window};
use objc2::rc::Retained;
use objc2::runtime::{AnyObject, NSObject, NSObjectProtocol};
use objc2::{DefinedClass, MainThreadMarker, MainThreadOnly, define_class, msg_send, sel};
use objc2_app_kit::{NSControlStateValueOff, NSControlStateValueOn, NSMenu, NSMenuItem, NSView};
use objc2_foundation::{NSOperationQueue, NSPoint, NSString};

use crate::glass::renderer_view;

pub struct Ivars {
    picked: Cell<isize>,
}

define_class!(
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[ivars = Ivars]
    struct Target;

    unsafe impl NSObjectProtocol for Target {}

    impl Target {
        #[unsafe(method(pick:))]
        fn pick(&self, sender: &NSMenuItem) {
            self.ivars().picked.set(sender.tag());
        }
    }
);

impl Target {
    fn new(mtm: MainThreadMarker) -> Retained<Target> {
        let this = Target::alloc(mtm).set_ivars(Ivars { picked: Cell::new(-1) });
        unsafe { msg_send![super(this), init] }
    }
}

pub enum Entry {
    Item { label: String, checked: bool },
    Header(String),
    Separator,
}

// why: AppKit's own menus look and behave natively (Liquid Glass on macOS 26), but tracking runs a nested
// run loop, so the menu opens from the main queue after GPUI's event handling has returned
pub fn popup(window: &Window, anchor: Bounds<Pixels>, entries: Vec<Entry>) -> oneshot::Receiver<Option<usize>> {
    let (tx, rx) = oneshot::channel();
    let Some(view) = renderer_view(window) else {
        let _ = tx.send(None);
        return rx;
    };
    let x = f64::from(f32::from(anchor.left()));
    let bottom = f64::from(f32::from(anchor.bottom()));
    let tx = Cell::new(Some(tx));
    let entries = Cell::new(Some(entries));
    let block = RcBlock::new(move || {
        let Some(mtm) = MainThreadMarker::new() else { return };
        let (Some(tx), Some(entries)) = (tx.take(), entries.take()) else { return };
        let _ = tx.send(show(mtm, &view, x, bottom, entries));
    });
    unsafe { NSOperationQueue::mainQueue().addOperationWithBlock(&block) };
    rx
}

fn show(mtm: MainThreadMarker, view: &NSView, x: f64, bottom: f64, entries: Vec<Entry>) -> Option<usize> {
    let menu = NSMenu::initWithTitle(NSMenu::alloc(mtm), &NSString::from_str(""));
    menu.setAutoenablesItems(false);
    let target = Target::new(mtm);
    let mut index = 0isize;
    let mut checked = None;
    for entry in &entries {
        match entry {
            Entry::Separator => menu.addItem(&NSMenuItem::separatorItem(mtm)),
            Entry::Header(title) => {
                let item = NSMenuItem::sectionHeaderWithTitle(&NSString::from_str(title), mtm);
                menu.addItem(&item);
            }
            Entry::Item { label, checked: on } => {
                let item = unsafe { NSMenuItem::initWithTitle_action_keyEquivalent(NSMenuItem::alloc(mtm), &NSString::from_str(label), Some(sel!(pick:)), &NSString::from_str("")) };
                unsafe { item.setTarget(Some(as_object(&target))) };
                item.setTag(index);
                item.setState(if *on { NSControlStateValueOn } else { NSControlStateValueOff });
                if *on {
                    checked = Some(item.clone());
                }
                menu.addItem(&item);
                index += 1;
            }
        }
    }
    let y = if view.isFlipped() { bottom } else { view.bounds().size.height - bottom };
    menu.popUpMenuPositioningItem_atLocation_inView(checked.as_deref(), NSPoint::new(x, y), Some(view));
    let picked = target.ivars().picked.get();
    (picked >= 0).then_some(picked as usize)
}

fn as_object(target: &Target) -> &AnyObject {
    let object: &NSObject = target;
    object
}

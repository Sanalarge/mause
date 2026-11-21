use dispatch2::DispatchQueue;
use objc2::rc::Retained;
use objc2::{MainThreadMarker, MainThreadOnly};
use objc2_app_kit::{
    NSBackingStoreType, NSColor, NSScreen, NSWindow, NSWindowCollectionBehavior, NSWindowStyleMask,
};
use objc2_foundation::{NSPoint, NSRect, NSSize};
use std::cell::RefCell;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

thread_local! {
    static WINDOW: RefCell<Option<(u64, Retained<NSWindow>)>> = const { RefCell::new(None) };
}
static NEXT: AtomicU64 = AtomicU64::new(1);

pub(super) fn show(x: i32, y: i32, w: i32, h: i32) {
    if w <= 0 || h <= 0 {
        return;
    }
    let id = NEXT.fetch_add(1, Ordering::Relaxed);
    DispatchQueue::main().exec_async(move || {
        let Some(mtm) = MainThreadMarker::new() else {
            return;
        };
        let screens = NSScreen::screens(mtm);
        let Some(primary) = screens.firstObject() else {
            return;
        };
        let rect = NSRect::new(
            NSPoint::new(x as f64, primary.frame().size.height - (y + h) as f64),
            NSSize::new(w as f64, h as f64),
        );
        let window = unsafe {
            let window = NSWindow::initWithContentRect_styleMask_backing_defer(
                NSWindow::alloc(mtm),
                rect,
                NSWindowStyleMask::Borderless,
                NSBackingStoreType::Buffered,
                false,
            );
            window.setReleasedWhenClosed(false);
            window
        };
        window.setIgnoresMouseEvents(true);
        window.setHidesOnDeactivate(false);
        window.setHasShadow(false);
        window.setOpaque(false);
        window.setLevel(102);
        window.setCollectionBehavior(
            NSWindowCollectionBehavior::CanJoinAllSpaces
                | NSWindowCollectionBehavior::Stationary
                | NSWindowCollectionBehavior::IgnoresCycle
                | NSWindowCollectionBehavior::FullScreenAuxiliary,
        );
        set_color(&window, super::FLASH_COLORS[0]);
        window.orderFrontRegardless();
        WINDOW.with(|slot| {
            if let Some((_, previous)) = slot.borrow_mut().replace((id, window)) {
                previous.close();
            }
        });
    });
    for step in 1..=super::FLASH_STEPS {
        std::thread::sleep(Duration::from_millis(super::FLASH_STEP_MS));
        DispatchQueue::main().exec_async(move || {
            WINDOW.with(|slot| {
                let mut slot = slot.borrow_mut();
                if slot.as_ref().is_none_or(|(current, _)| *current != id) {
                    return;
                }
                if step == super::FLASH_STEPS {
                    if let Some((_, window)) = slot.take() {
                        window.close();
                    }
                } else if let Some((_, window)) = slot.as_ref() {
                    set_color(window, super::FLASH_COLORS[step as usize % 2]);
                }
            });
        });
    }
}

fn set_color(window: &NSWindow, color: u32) {
    let color = NSColor::colorWithSRGBRed_green_blue_alpha(
        ((color >> 16) & 255) as f64 / 255.0,
        ((color >> 8) & 255) as f64 / 255.0,
        (color & 255) as f64 / 255.0,
        0.90,
    );
    window.setBackgroundColor(Some(&color));
}

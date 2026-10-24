//! macOS: the screen's work area, for placing windows (Arrange All, View Side by Side, #322).
//!
//! egui reports a monitor's size but not where it is or what the menu bar and the Dock take, so
//! AppKit is asked: the visible frame of the screen the key window is on (else the main screen),
//! in points with the origin at the top left of the main screen, as egui places windows. These
//! calls are safe in the `objc2-app-kit` bindings (the workspace forbids unsafe).

use objc2::MainThreadMarker;
use objc2_app_kit::{NSApplication, NSScreen};

/// The work area of the key window's screen, or the main screen's; `None` off the main thread
/// or with no screen.
pub fn work_area(_window: Option<egui::Rect>) -> Option<egui::Rect> {
    let mtm = MainThreadMarker::new()?;
    let app = NSApplication::sharedApplication(mtm);
    let screen = app.keyWindow().and_then(|w| w.screen()).or_else(|| NSScreen::mainScreen(mtm))?;
    // AppKit's origin is the bottom left of the main screen, y upwards.
    let main_h = NSScreen::screens(mtm).firstObject().map(|s| s.frame().size.height).unwrap_or(screen.frame().size.height);
    let f = screen.visibleFrame();
    let (x, w, h) = (f.origin.x as f32, f.size.width as f32, f.size.height as f32);
    let top = (main_h - (f.origin.y + f.size.height)) as f32;
    if !(x.is_finite() && top.is_finite() && w > 0.0 && h > 0.0) {
        return None;
    }
    Some(egui::Rect::from_min_size(egui::pos2(x, top), egui::vec2(w, h)))
}

//! View › Window › Split: two panes on the document in one window, as Word splits it (#322): a
//! bar across the window, each pane with its own caret, selection, scroll and zoom, the document
//! one and the same. The active pane (the one with the caret) is the app's own view and canvas;
//! the other pane's are kept here and swapped in to draw it (`Session::swap_view`). A press in the
//! other pane makes it the active one. Remove Split keeps the active pane.

use egui::{CursorIcon, Rect, Sense, Ui, UiBuilder, pos2};
use wordcraft_engine::ViewSlot;

use crate::canvas::{self, CanvasState};
use crate::theme::Tokens;
use crate::{WordApp, wid};

/// The bar between the panes, in points.
const BAR: f32 = 6.0;
/// A pane never shrinks below this.
const MIN_PANE: f32 = 60.0;

pub struct Split {
    /// The inactive pane's view and canvas (the active pane's are the app's own).
    slot: ViewSlot,
    canvas: Box<CanvasState>,
    /// Where the bar is, as a fraction of the panes' height.
    pub fraction: f32,
    /// The active pane is the lower one.
    pub lower_active: bool,
}

/// View › Split: split the window at the middle, the new pane showing the same place; split
/// again (Remove Split) to go back to one pane, the active one.
pub fn toggle(app: &mut WordApp) {
    if app.split.take().is_some() {
        app.canvas.want_focus = true;
        return;
    }
    let slot = app.session.clone_view();
    let mut canvas = Box::new(CanvasState::default());
    canvas.scroll_to_caret = true;
    app.split = Some(Split { slot, canvas, fraction: 0.5, lower_active: false });
}

/// Draw the document: one canvas, or the two panes and the bar between them.
pub fn show(app: &mut WordApp, ui: &mut Ui) {
    if app.split.is_none() {
        canvas::show(app, ui);
        return;
    }
    let full = ui.available_rect_before_wrap();
    let room = (full.height() - BAR).max(0.0);
    let fraction = app.split.as_ref().map_or(0.5, |s| s.fraction);
    let y = full.top() + (room * fraction).clamp(MIN_PANE.min(room / 2.0), (room - MIN_PANE).max(room / 2.0));
    let upper = Rect::from_min_max(full.min, pos2(full.right(), y));
    let bar = Rect::from_min_max(pos2(full.left(), y), pos2(full.right(), y + BAR));
    let lower = Rect::from_min_max(pos2(full.left(), y + BAR), full.max);
    let pressed_at = ui.input(|i| if i.pointer.any_pressed() { i.pointer.interact_pos() } else { None });
    let lower_active = app.split.as_ref().is_some_and(|s| s.lower_active);
    let mut flip = false;
    for (pane, rect) in [(0u8, upper), (1u8, lower)] {
        let active = (pane == 1) == lower_active;
        if !active {
            swap(app);
        }
        // The pane's fixed widget ids (the page, its scroll area) are its own.
        crate::set_pane_salt(pane);
        let mut child = ui.new_child(UiBuilder::new().max_rect(rect).id_salt(("split pane", pane)));
        canvas::show(app, &mut child);
        crate::set_pane_salt(0);
        if !active {
            // A press in the other pane makes it the active one: the caret is there now.
            flip = pressed_at.is_some_and(|p| rect.contains(p));
            swap(app);
        }
    }
    if flip {
        swap(app);
        if let Some(s) = app.split.as_mut() {
            s.lower_active = !s.lower_active;
        }
        app.canvas.want_focus = true;
        ui.ctx().request_repaint();
    }
    // The bar: dragged, it moves the split.
    let t = Tokens::get(ui.ctx());
    let resp = ui.interact(bar, wid("split bar"), Sense::drag()).on_hover_cursor(CursorIcon::ResizeVertical);
    ui.painter().rect_filled(bar, 0.0, t.border_strong);
    if resp.dragged()
        && let Some(p) = ui.input(|i| i.pointer.interact_pos())
        && room > 0.0
        && let Some(s) = app.split.as_mut()
    {
        s.fraction = ((p.y - full.top()) / room).clamp(0.05, 0.95);
    }
}

/// Exchange the active pane's view and canvas with the other pane's.
fn swap(app: &mut WordApp) {
    let Some(s) = app.split.as_mut() else { return };
    app.session.swap_view(&mut s.slot);
    std::mem::swap(&mut app.canvas, &mut *s.canvas);
}

#[cfg(test)]
mod tests {
    use crate::{Services, WordApp};
    use serde_json::json;
    use wordcraft_engine::Session;

    fn harness(app: WordApp) -> egui_kittest::Harness<'static, WordApp> {
        egui_kittest::Harness::builder().with_size(egui::vec2(1440.0, 900.0)).build_ui_state(
            |ui, app: &mut WordApp| {
                let ctx = ui.ctx().clone();
                app.logic(&ctx);
                app.ui(ui);
            },
            app,
        )
    }

    fn body(app: &WordApp) -> String {
        app.session.doc.plain_text(wordcraft_doc::StoryRef::Body)
    }

    /// View › Split draws the document in two panes with a bar between them; the upper pane keeps
    /// the caret, a press in the lower pane moves the caret there (typing goes to that pane's
    /// caret), and Remove Split keeps the active pane. The ribbon button reads Remove Split while
    /// the window is split.
    #[test]
    fn split_draws_two_panes_and_the_pressed_pane_takes_the_caret() {
        let mut app = WordApp::new(Session::new(wordcraft_doc::Document::new()), Services::default());
        app.run("text.insert", json!({"text": "First paragraph"})).unwrap();
        app.run("text.newParagraph", json!({})).unwrap();
        app.run("text.insert", json!({"text": "Second paragraph"})).unwrap();
        let mut h = harness(app);
        for _ in 0..4 {
            h.step();
        }
        let one_pane = h.state().canvas.canvas_rect.unwrap();
        assert!(h.state().split.is_none());
        h.state_mut().run("view.split", json!({})).unwrap();
        for _ in 0..4 {
            h.step();
        }
        let app = h.state();
        let s = app.split.as_ref().expect("split");
        assert!(!s.lower_active, "the upper pane keeps the caret");
        let upper = app.canvas.canvas_rect.unwrap();
        let lower = s.canvas.canvas_rect.unwrap();
        assert!(
            upper.bottom() < lower.top() && upper.top() >= one_pane.top() && lower.bottom() <= one_pane.bottom() + 0.5,
            "two panes: {upper:?} {lower:?}"
        );
        assert!(!app.canvas.page_rects.is_empty() && !s.canvas.page_rects.is_empty(), "both panes draw the page");
        assert_eq!(app.session.sel.focus.off, "Second paragraph".len(), "the caret is where it was");
        assert_eq!(s.slot.sel.focus, app.session.sel.focus, "the new pane opens at the same place");

        // A press in the lower pane: that pane is active and the caret is there.
        let at = lower.center();
        let button =
            |pressed| egui::Event::PointerButton { pos: at, button: egui::PointerButton::Primary, pressed, modifiers: egui::Modifiers::NONE };
        h.event(egui::Event::PointerMoved(at));
        h.step();
        h.event(button(true));
        h.step();
        h.event(button(false));
        for _ in 0..3 {
            h.step();
        }
        assert!(h.state().split.as_ref().is_some_and(|s| s.lower_active), "the lower pane took the press");
        assert_eq!(h.state().canvas.canvas_rect.unwrap(), lower, "the active canvas is the lower pane's");
        // Typing goes to the active pane's caret (wherever the press put it), and the page shows
        // it in both panes.
        let before = body(h.state());
        h.state_mut().run("text.insert", json!({"text": "X"})).unwrap();
        for _ in 0..2 {
            h.step();
        }
        assert_ne!(body(h.state()), before);
        assert!(h.state().split.as_ref().unwrap().canvas.page_rects.len() == h.state().canvas.page_rects.len());

        // Remove Split keeps the active (lower) pane's caret and canvas.
        let caret = h.state().session.sel.focus.clone();
        h.state_mut().run("view.split", json!({})).unwrap();
        for _ in 0..3 {
            h.step();
        }
        assert!(h.state().split.is_none());
        assert_eq!(h.state().session.sel.focus, caret);
        assert_eq!(h.state().canvas.canvas_rect.unwrap(), one_pane, "one pane again");
    }
}

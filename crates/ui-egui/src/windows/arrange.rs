//! View › Window's Arrange All, View Side by Side, Synchronous Scrolling and Reset Window Position
//! (#322), on the native windows ([`super`]), as Word has them: Arrange All stacks the windows top
//! to bottom across the screen's work area; View Side by Side puts the asking window on the left
//! half and its partner (the one other window, or the one picked in the Compare Side by Side
//! dialog) on the right with Synchronous Scrolling on, so a scroll in one moves the other by as
//! much, once; Reset Window Position halves them again; off again puts both back.
//!
//! The engine owns no window: the commands check their parameters against what the app reports in
//! [`wordcraft_engine::Windows`] ([`WordApp::publish_windows`], whenever a window is entered) and
//! queue a request ([`WordApp::window_request`]) carried out here.

use std::collections::HashMap;

use egui::{Pos2, Rect, Vec2, ViewportCommand, pos2, vec2};
use serde_json::Value;
use wordcraft_engine::WindowEntry;

use crate::window_geometry::MIN_SIZE;
use crate::{WordApp, dialogs, documents};

/// The screen's work area in points (the desktop less the menu bar, Dock or taskbar) for the
/// window at that frame rect, where the platform tells (macOS: NSScreen's visible frame).
pub type WorkArea = Box<dyn Fn(Option<Rect>) -> Option<Rect>>;

/// Room left for a system bar on a monitor whose work area isn't known (a taskbar, a panel).
const SYSTEM_BAR: f32 = 48.0;

/// View Side by Side: the two windows, where to put them back, and whether they scroll together.
pub(crate) struct Pair {
    left: u64,
    right: u64,
    restore: Vec<(u64, Rect, Vec2)>,
    sync: bool,
}

/// What the window arrangements keep.
#[derive(Default)]
pub(crate) struct Arrange {
    pub(super) work_area: Option<WorkArea>,
    pub(super) pair: Option<Pair>,
    /// Synchronous Scrolling: each window's page offset at the end of its last drawing, and
    /// whether its next change is the partner's scroll arriving (not to be passed back).
    scroll_seen: HashMap<u64, (Vec2, bool)>,
}

impl WordApp {
    /// The platform's work area for a window's screen (macOS: NSScreen's visible frame), for
    /// Arrange All and View Side by Side. Without it a window's monitor less a system bar is used.
    pub fn set_work_area(&mut self, work_area: WorkArea) {
        self.windows.arrange.work_area = Some(work_area);
    }

    /// Tell the session about the open windows ([`wordcraft_engine::Windows`]), as the drawn
    /// window sees them: the View › Window commands check their parameters against it.
    pub(crate) fn publish_windows(&mut self) {
        let open: Vec<u64> = self.windows.open.iter().map(|w| w.key).collect();
        if let Some(p) = &self.windows.arrange.pair
            && !(open.contains(&p.left) && open.contains(&p.right))
        {
            self.windows.arrange.pair = None;
        }
        self.windows.arrange.scroll_seen.retain(|k, _| open.contains(k));
        let docs = self.session.documents();
        let this = self.windows.shown;
        // The drawn window's document is the one its state shows (its entry is brought up to
        // date when another window is drawn).
        let shown_document = self.shown_document;
        let doc_of = |w: &super::Window| if w.key == this { shown_document } else { w.doc };
        let list = self
            .windows
            .open
            .iter()
            .map(|w| {
                let id = doc_of(w);
                let doc = docs.iter().find(|d| d.id == id);
                let title = self.windows.menu_title(w.key).map(str::to_string).or_else(|| doc.map(documents::display_title)).unwrap_or_default();
                let shared = self.windows.open.iter().filter(|o| doc_of(o) == id).count() > 1;
                WindowEntry {
                    id: w.key,
                    title,
                    path: doc.and_then(|d| d.path.as_ref()).map(|p| p.to_string_lossy().into_owned()),
                    document: if shared { id } else { 0 },
                }
            })
            .collect();
        // While the windows draw, the viewport being drawn is the window's (the driver has the
        // viewports then); between frames the drawn window is the active one.
        let root = match (&self.ctx, self.windows.showing) {
            (Some(ctx), true) => ctx.viewport_id() == egui::ViewportId::ROOT,
            _ => !self.windows.driven || self.windows.viewport(this) == egui::ViewportId::ROOT,
        };
        crate::set_window_is_root(root);
        let pair = self.windows.arrange.pair.as_ref();
        let side_by_side = pair.and_then(|p| match this {
            t if t == p.left => Some(p.right),
            t if t == p.right => Some(p.left),
            _ => None,
        });
        let w = &mut self.session.windows;
        w.host = self.windows.native;
        w.this = this;
        w.list = list;
        w.side_by_side = side_by_side;
        w.sync_scroll = side_by_side.is_some() && pair.is_some_and(|p| p.sync);
    }

    /// A View › Window request from the engine (`{"window": what}`), for the drawn window.
    pub(crate) fn window_request(&mut self, what: &str, req: &Value) {
        let on = req.get("value").and_then(Value::as_bool);
        match what {
            "arrangeAll" => self.arrange_all(),
            "sideBySide" => match (on.unwrap_or(true), req.get("with").and_then(Value::as_u64)) {
                (true, Some(with)) if with != self.windows.shown && self.windows.get(with).is_some() => {
                    self.begin_side_by_side(self.windows.shown, with)
                }
                (true, _) => {}
                (false, _) => self.end_side_by_side(),
            },
            "syncScroll" => {
                let shown = self.windows.shown;
                let mut seen = Vec::new();
                if let Some(p) = self.windows.arrange.pair.as_mut() {
                    p.sync = on.unwrap_or(!p.sync);
                    seen = vec![p.left, p.right];
                }
                // Scrolling from here on is what moves the partner.
                for key in seen {
                    let offset = if key == shown {
                        Some(self.canvas.scroll_offset)
                    } else {
                        self.windows.get(key).and_then(|w| w.ui.as_ref()).map(|u| u.canvas.scroll_offset)
                    };
                    if let Some(offset) = offset {
                        self.windows.arrange.scroll_seen.insert(key, (offset, false));
                    }
                }
            }
            "resetPosition" => {
                if let Some((left, right)) = self.windows.arrange.pair.as_ref().map(|p| (p.left, p.right)) {
                    self.place_halves(left, right);
                }
            }
            "switch" => {
                if let Some(to) = req.get("to").and_then(Value::as_u64) {
                    self.show_window(to);
                }
            }
            // Word's Compare Side by Side dialog: which of the other windows.
            "chooseSideBySide" => self.dialog = dialogs::Dialog::open("compareSideBySide", self),
            _ => {}
        }
        self.publish_windows();
    }

    /// The window's frame and content rects and its monitor's size, as its last frame saw them.
    fn window_rects(&self, key: u64) -> (Option<Rect>, Option<Rect>, Option<Vec2>) {
        let Some(ctx) = &self.ctx else { return (None, None, None) };
        let viewport = self.windows.viewport(key);
        ctx.input_for(viewport, |i| {
            let v = i.viewport();
            (v.outer_rect, v.inner_rect, v.monitor_size)
        })
    }

    /// The work area the window is on: the platform's answer, else its monitor less a system bar.
    fn work_area_of(&self, key: u64) -> Option<Rect> {
        let (outer, _, monitor) = self.window_rects(key);
        if let Some(f) = &self.windows.arrange.work_area
            && let Some(r) = f(outer)
        {
            return Some(r);
        }
        let m = monitor.filter(|m| m.is_finite() && m.x > 0.0 && m.y > 0.0)?;
        Some(Rect::from_min_max(Pos2::ZERO, pos2(m.x, m.y - SYSTEM_BAR)))
    }

    /// Put a window's frame at `outer` (its content size follows from the frame's).
    fn place(&self, key: u64, outer: Rect) {
        let Some(ctx) = &self.ctx else { return };
        let (o, i, _) = self.window_rects(key);
        let insets = match (o, i) {
            (Some(o), Some(i)) => (o.size() - i.size()).max(Vec2::ZERO),
            _ => Vec2::ZERO,
        };
        let size = (outer.size() - insets).max(MIN_SIZE);
        let viewport = self.windows.viewport(key);
        ctx.send_viewport_cmd_to(viewport, ViewportCommand::Maximized(false));
        ctx.send_viewport_cmd_to(viewport, ViewportCommand::Minimized(false));
        ctx.send_viewport_cmd_to(viewport, ViewportCommand::OuterPosition(outer.min));
        ctx.send_viewport_cmd_to(viewport, ViewportCommand::InnerSize(size));
        ctx.request_repaint_of(viewport);
    }

    /// View › Window › Arrange All: every window stacked top to bottom across the work area of
    /// the drawn window's screen, in the order they were opened, as Word tiles them.
    fn arrange_all(&mut self) {
        let Some(work) = self.work_area_of(self.windows.shown) else { return };
        let keys: Vec<u64> = self.windows.open.iter().map(|w| w.key).collect();
        for (key, rect) in keys.iter().zip(tile_rows(work, keys.len())) {
            self.place(*key, rect);
        }
    }

    /// View Side by Side on: `left` (the window asking) on the left half, `right` on the right,
    /// with Synchronous Scrolling on; both windows' places are kept to restore.
    fn begin_side_by_side(&mut self, left: u64, right: u64) {
        let restore = match self.windows.arrange.pair.take() {
            Some(p) => p.restore,
            None => [left, right]
                .iter()
                .filter_map(|&key| {
                    let (outer, inner, _) = self.window_rects(key);
                    Some((key, outer?, inner?.size()))
                })
                .collect(),
        };
        self.windows.arrange.pair = Some(Pair { left, right, restore, sync: true });
        self.windows.arrange.scroll_seen.clear();
        self.place_halves(left, right);
    }

    fn place_halves(&self, left: u64, right: u64) {
        let Some(work) = self.work_area_of(left) else { return };
        let [l, r] = halves(work);
        self.place(left, l);
        self.place(right, r);
    }

    /// View Side by Side off: both windows go back where they were.
    fn end_side_by_side(&mut self) {
        let Some(p) = self.windows.arrange.pair.take() else { return };
        let Some(ctx) = self.ctx.clone() else { return };
        for (key, outer, inner) in p.restore {
            if self.windows.get(key).is_none() {
                continue;
            }
            let viewport = self.windows.viewport(key);
            ctx.send_viewport_cmd_to(viewport, ViewportCommand::OuterPosition(outer.min));
            ctx.send_viewport_cmd_to(viewport, ViewportCommand::InnerSize(inner.max(MIN_SIZE)));
            ctx.request_repaint_of(viewport);
        }
    }

    /// After window `key` drew: with Synchronous Scrolling, a scroll in it moves its partner by
    /// the same amount, once (the partner's own change from applying it is not passed back).
    pub(crate) fn sync_scroll_after_draw(&mut self, key: u64) {
        let partner = match &self.windows.arrange.pair {
            Some(p) if p.sync && p.left == key => p.right,
            Some(p) if p.sync && p.right == key => p.left,
            _ => return,
        };
        let now = self.canvas.scroll_offset;
        let (delta, applied) = match self.windows.arrange.scroll_seen.insert(key, (now, false)) {
            Some((seen, applied)) => (now - seen, applied),
            None => (Vec2::ZERO, false),
        };
        if delta == Vec2::ZERO || applied {
            return;
        }
        let viewport = self.windows.viewport(partner);
        let moved = self.windows.get_mut(partner).and_then(|w| w.ui.as_mut()).is_some_and(|ui| ui.canvas.scroll_by(delta));
        if moved {
            let seen = self.windows.arrange.scroll_seen.get(&partner).map_or(Vec2::ZERO, |s| s.0);
            self.windows.arrange.scroll_seen.insert(partner, (seen, true));
            if let Some(ctx) = &self.ctx {
                ctx.request_repaint_of(viewport);
            }
        }
    }

    /// Whether View Side by Side pairs two windows ([`Pair`]).
    #[cfg(test)]
    pub(crate) fn side_by_side(&self) -> Option<(u64, u64)> {
        self.windows.arrange.pair.as_ref().map(|p| (p.left, p.right))
    }
}

/// Arrange All's tiles: `n` rows across the work area, top to bottom, each at least [`MIN_SIZE`]
/// tall (more windows than fit overlap at the bottom).
pub fn tile_rows(work: Rect, n: usize) -> Vec<Rect> {
    let n = n.max(1);
    let h = (work.height() / n as f32).max(MIN_SIZE.y);
    (0..n).map(|i| Rect::from_min_size(pos2(work.left(), work.top() + i as f32 * h), vec2(work.width(), h))).collect()
}

/// View Side by Side's halves of the work area: left and right.
pub fn halves(work: Rect) -> [Rect; 2] {
    let w = (work.width() / 2.0).max(MIN_SIZE.x);
    [Rect::from_min_size(work.min, vec2(w, work.height())), Rect::from_min_size(pos2(work.left() + w, work.top()), vec2(w, work.height()))]
}

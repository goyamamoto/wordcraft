//! Document windows (#322): one window per open document, as Word has them.
//!
//! The desktop app installs a [`Host`] that owns every window's [`WordApp`] (its session, canvas
//! and interface state) and the window it lives in: the first document window is eframe's root
//! viewport, every other one a deferred child viewport the root declares each pass
//! (`egui::Context::show_viewport_deferred`). Each viewport's frame runs its own `WordApp` through
//! [`Host::logic`] and [`Host::ui`]; the host lives in a thread-local because a child viewport's
//! callback can borrow nothing of the eframe app.
//!
//! What follows Word (observed in Word for Mac): File › New and File › Open put the document in a
//! new window and leave the current one alone, except that a window showing no document, or a new
//! blank document nobody has typed in, takes the document itself; a file already open in another
//! window brings that window forward instead of opening twice; File › Close closes the document's
//! window when another is open and otherwise leaves the empty window (#1712); the window's close
//! button asks about unsaved changes and closes only that window; quitting asks about each dirty
//! document in turn, and Cancel in any prompt keeps the app open. View › Window: Switch Windows
//! lists the windows in the order they were opened, Arrange All stacks every window top to
//! bottom across the screen's work area, View Side by Side puts this window on the left half and
//! its partner on the right and turns Synchronous Scrolling on, Reset Window Position puts the
//! pair back to the halves, and turning Side by Side off gives both windows their earlier places
//! back. New Window (a second window on one document) and Split need two views of one document,
//! which a session doesn't have yet: their commands are disabled, with the reason.
//!
//! Things every window shares: egui's one `Memory` (so fixed widget ids go through
//! [`crate::wid`]), the interface fonts (each window's wanted faces are pooled before its frame),
//! the interface theme, size, language and the other settings, which the window the user last
//! changed them in hands to the rest ([`WordApp::adopt_settings`]).
//!
//! Only the desktop app has a host: the web app, `ui_shot` and the tests drive one `WordApp`, and
//! `Session::windows` tells the engine so (`host: false`) so the window commands are disabled.

use std::cell::RefCell;
use std::sync::mpsc::Receiver;

use egui::{Rect, Vec2, ViewportBuilder, ViewportCommand, ViewportId, pos2, vec2};
use serde_json::{Value, json};
use wordcraft_engine::{WindowEntry, Windows};

use crate::native_menu::{self, NativeMenu};
use crate::window_geometry::{MIN_SIZE, WindowGeometry};
use crate::{ControlRequest, WordApp};

/// A window's id: stable while it is open, unique within the run, never 0.
pub type WindowId = u64;

/// How far down and right a window opened from another one appears, as macOS cascades windows.
const CASCADE: Vec2 = vec2(22.0, 22.0);
/// Room left on each side of a monitor whose work area isn't known (a taskbar, a panel).
const SYSTEM_BAR: f32 = 48.0;

/// What a window asks the host for, queued by its `WordApp` and carried out by [`Host`].
#[derive(Clone, Debug, PartialEq)]
pub enum WindowRequest {
    /// Open the document `file.open`'s params name in a new window.
    Open(Value),
    /// A new document (`file.new`, `file.newFromTemplate`) in a new window.
    New {
        command: String,
        params: Value,
    },
    /// Bring that window to the front (Switch Windows).
    Focus(WindowId),
    ArrangeAll,
    /// View Side by Side on (with the partner, or the one other window) or off.
    SideBySide {
        on: bool,
        with: Option<WindowId>,
    },
    SyncScroll(bool),
    ResetPosition,
    /// Close this window: File › Close with other windows open.
    Close,
    /// Quit the app: every window closes, asking about unsaved changes (`app.quit`).
    Quit,
}

/// A window and the `WordApp` it shows.
pub struct Window {
    pub id: WindowId,
    pub viewport: ViewportId,
    pub app: WordApp,
    /// The child viewport's builder, fixed once it is created (egui only patches changes, so the
    /// same one is declared every pass; the title travels as a command).
    builder: ViewportBuilder,
    /// The window's frame and content rects and monitor as its last frame saw them.
    outer: Option<Rect>,
    inner: Option<Rect>,
    monitor: Option<Vec2>,
    /// Whether the window was on screen at its last frame (not minimized, not covered): eframe
    /// runs no frame of a child viewport that isn't, so such a window answers nothing until it
    /// shows again.
    visible: Option<bool>,
    /// The host frame count when the window last had the focus.
    focused_at: u64,
    /// Being closed: dropped once the root pass stops declaring it.
    closing: bool,
    /// Synchronous Scrolling: the canvas offset seen at the end of the last frame, and whether
    /// the next change is the partner's scroll arriving (not to be passed back).
    scroll_seen: Option<Vec2>,
    scroll_applied: bool,
}

/// View Side by Side: the two windows, and where to put them back.
struct Pair {
    left: WindowId,
    right: WindowId,
    restore: Vec<(WindowId, Rect, Vec2)>,
    sync: bool,
}

/// Quitting: the window whose Save Changes prompt the quit waits on.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Quit {
    No,
    Asking(WindowId),
}

/// Makes the `WordApp` for a new window: a blank session with the desktop services.
pub type NewApp = Box<dyn FnMut() -> WordApp>;

/// The screen's work area in points (the desktop less the menu bar, Dock or taskbar) for the
/// window at that frame rect, where the platform tells.
pub type WorkArea = Box<dyn Fn(Option<Rect>) -> Option<Rect>>;

/// Runs after a window's frame with whether the window has the focus (the macOS input method's
/// view of the focused document).
pub type AfterUi = Box<dyn FnMut(&WordApp, bool)>;

/// The open windows and what they share.
pub struct Host {
    windows: Vec<Window>,
    next_id: WindowId,
    frames: u64,
    active: WindowId,
    pair: Option<Pair>,
    quit: Quit,
    new_app: NewApp,
    template: ViewportBuilder,
    pub work_area: Option<WorkArea>,
    pub after_ui: Option<AfterUi>,
    /// The native menu bar (macOS): one for the process, showing the window in use and running
    /// its items there.
    menu: Option<NativeMenu>,
    control_rx: Option<Receiver<ControlRequest>>,
    /// Every window has closed (or the last one may): the eframe app ends the process.
    exit: bool,
    /// The shared settings as last handed round, to notice a change.
    shared_settings: Option<String>,
    /// Frames left in which the settings are compared, after a frame with input.
    share_pending: u8,
    ctx: Option<egui::Context>,
}

thread_local! {
    static HOST: RefCell<Option<Host>> = const { RefCell::new(None) };
}

impl Host {
    /// A host with its first window: `first` in the root viewport, `template` the viewport
    /// builder new windows start from (title, chrome, icon), `new_app` making their apps.
    pub fn new(first: WordApp, mut template: ViewportBuilder, new_app: NewApp) -> Self {
        // Where the first window was restored to is its own; new windows cascade from the window
        // they come from, or the system places them.
        template.position = None;
        template.maximized = None;
        let window = Window::new(1, ViewportId::ROOT, first, ViewportBuilder::default());
        Host {
            windows: vec![window],
            next_id: 2,
            frames: 0,
            active: 1,
            pair: None,
            quit: Quit::No,
            new_app,
            template,
            work_area: None,
            after_ui: None,
            menu: None,
            control_rx: None,
            exit: false,
            shared_settings: None,
            share_pending: 0,
            ctx: None,
        }
    }

    /// The control channel's requests (`--control`): routed to the window each one names.
    pub fn with_control(mut self, rx: Receiver<ControlRequest>) -> Self {
        self.control_rx = Some(rx);
        self
    }

    /// The native menu bar the host installed ([`native_menu`]).
    pub fn with_menu(mut self, menu: NativeMenu) -> Self {
        self.menu = Some(menu);
        self
    }

    /// Whether the window `vp` shows is the one in use: the one the menu bar shows and acts on.
    fn is_active_viewport(&self, vp: ViewportId) -> bool {
        self.active().is_some_and(|w| w.viewport == vp)
    }

    /// Install the host for this thread (the main thread), replacing any earlier one.
    pub fn install(host: Host) {
        HOST.with(|h| *h.borrow_mut() = Some(host));
    }

    /// Run `f` on the installed host. `None` without one, or while it is already in use (a
    /// nested call), which never happens on eframe's thread.
    pub fn with<R>(f: impl FnOnce(&mut Host) -> R) -> Option<R> {
        HOST.with(|h| match h.try_borrow_mut() {
            Ok(mut host) => host.as_mut().map(f),
            Err(_) => {
                log::error!("window host borrowed twice");
                None
            }
        })
    }

    /// Take the host out again (the end of the run, or a test).
    pub fn take() -> Option<Host> {
        HOST.with(|h| h.borrow_mut().take())
    }

    /// Whether every window has closed, or the last one was allowed to: the process ends.
    pub fn exit_requested(&self) -> bool {
        self.exit
    }

    /// The windows, in the order they were opened (Switch Windows' order).
    pub fn windows(&self) -> impl Iterator<Item = &Window> {
        self.windows.iter()
    }

    /// The window that last had the focus (the first, before any did); `None` once every window
    /// has closed.
    pub fn active(&self) -> Option<&Window> {
        self.windows.iter().find(|w| w.id == self.active && !w.closing).or_else(|| self.windows.iter().find(|w| !w.closing))
    }

    /// The window that last had the focus, to change.
    pub fn active_mut(&mut self) -> Option<&mut Window> {
        let id = self.active()?.id;
        self.get_mut(id)
    }

    pub fn get(&self, id: WindowId) -> Option<&Window> {
        self.windows.iter().find(|w| w.id == id)
    }

    pub fn get_mut(&mut self, id: WindowId) -> Option<&mut Window> {
        self.windows.iter_mut().find(|w| w.id == id)
    }

    /// The window shown in that viewport.
    pub fn window_of_viewport_mut(&mut self, vp: ViewportId) -> Option<&mut Window> {
        self.windows.iter_mut().find(|w| w.viewport == vp && !w.closing)
    }

    fn index(&self, id: WindowId) -> Option<usize> {
        self.windows.iter().position(|w| w.id == id)
    }

    fn index_of_viewport(&self, vp: ViewportId) -> Option<usize> {
        self.windows.iter().position(|w| w.viewport == vp && !w.closing)
    }

    /// The window whose id, or 1-based number in the Switch Windows list, is `n`.
    pub fn resolve(&self, n: u64) -> Option<WindowId> {
        self.snapshot(0).resolve(n)
    }

    /// What the engine sees of the windows from window `this` ([`wordcraft_engine::Windows`]).
    fn snapshot(&self, this: WindowId) -> Windows {
        let list = self
            .windows
            .iter()
            .filter(|w| !w.closing)
            .map(|w| WindowEntry { id: w.id, title: w.app.title_stem(), path: w.app.session.path.as_ref().map(|p| p.to_string_lossy().into_owned()) })
            .collect();
        let pair = self.pair.as_ref();
        let partner = pair.and_then(|p| {
            if p.left == this {
                Some(p.right)
            } else if p.right == this {
                Some(p.left)
            } else {
                None
            }
        });
        Windows { host: true, this, list, side_by_side: partner, sync_scroll: partner.is_some() && pair.is_some_and(|p| p.sync) }
    }

    /// The window's frame, before it is drawn: the engine's view of the windows, the pooled
    /// fonts, the app's own logic, then the window's geometry, focus, close button and requests.
    pub fn logic(&mut self, vp: ViewportId, ctx: &egui::Context) {
        if self.ctx.is_none() {
            self.ctx = Some(ctx.clone());
        }
        let Some(i) = self.index_of_viewport(vp) else { return };
        self.frames += 1;
        let id = self.windows[i].id;
        crate::set_window_salt(id);
        crate::set_window_is_root(vp == ViewportId::ROOT);
        if vp == ViewportId::ROOT {
            self.drain_control(ctx);
            self.quit_tick(ctx);
            self.pass_wake_ups_on(ctx);
            // A menu event wakes the root viewport; the window in use takes it in its frame.
            if let Some(menu) = &mut self.menu
                && menu.pull()
                && let Some(w) = self.active()
                && w.viewport != ViewportId::ROOT
            {
                ctx.request_repaint_of(w.viewport);
            }
        }
        self.pass_screenshots_on(vp, ctx);
        self.pool_fonts();
        let snapshot = self.snapshot(id);
        let single = self.windows.len() == 1;
        let active = self.is_active_viewport(vp);
        let w = &mut self.windows[i];
        w.app.session.windows = snapshot;
        w.app.applies_zoom = single || self.active == id;
        w.app.menu_bar_installed = self.menu.is_some();
        if active && let Some(menu) = &mut self.menu {
            native_menu::run(menu, &mut w.app, ctx);
        }
        w.app.logic(ctx);
        // Where the window is, for cascading, tiling and the saved geometry.
        let info = ctx.input(|i| i.viewport().clone());
        w.outer = info.outer_rect;
        w.inner = info.inner_rect;
        w.monitor = info.monitor_size;
        w.visible = info.visible();
        let content = ctx.input(|i| i.viewport_rect().size());
        w.app.ui.window = WindowGeometry::track(w.app.ui.window, &info, content);
        if info.focused == Some(true) {
            w.focused_at = self.frames;
            self.active = id;
        }
        // The window's close button, or the document's own request to close (File › Close with
        // unsaved changes answered Don't Save, `file.close {"quit": true}`): with unsaved changes
        // the app asks first and the close waits (`WordApp::close_requested`).
        let closing = ctx.input(|i| i.viewport().close_requested());
        let allowed = w.app.quit_requested || (closing && w.app.close_requested());
        if closing && !allowed {
            ctx.send_viewport_cmd(ViewportCommand::CancelClose);
        }
        if allowed {
            // The window is gone (or the root now shows another document): its frame is over.
            self.close_window(id, ctx);
            return;
        }
        let requests = std::mem::take(&mut self.windows[i].app.window_requests);
        for req in requests {
            self.handle(id, req, ctx);
        }
        // A change made by a click shows in the settings on the next frame, which the click's
        // repaint brings without input of its own: look for one frame longer than the input.
        if ctx.input(|i| !i.events.is_empty()) {
            self.share_pending = 2;
        }
        if self.share_pending > 0 {
            self.share_pending -= 1;
            self.share_settings(id, ctx);
        }
    }

    /// The window's frame: draw its app, pass a scroll on to its Side by Side partner, tell the
    /// host's hook, and (the root) declare the other windows so they stay open.
    pub fn ui(&mut self, vp: ViewportId, ui: &mut egui::Ui) {
        let ctx = ui.ctx().clone();
        if let Some(i) = self.index_of_viewport(vp) {
            let id = self.windows[i].id;
            crate::set_window_salt(id);
            crate::set_window_is_root(vp == ViewportId::ROOT);
            self.windows[i].app.ui(ui);
            self.sync_scroll_tick(i, &ctx);
            if self.is_active_viewport(vp)
                && let Some(menu) = &mut self.menu
            {
                native_menu::sync(menu, &self.windows[i].app, &ctx);
            }
            let focused = ctx.input(|i| i.viewport().focused == Some(true));
            if let Some(after) = &mut self.after_ui {
                after(&self.windows[i].app, focused);
            }
        }
        if vp == ViewportId::ROOT {
            self.declare_children(&ctx);
        }
        crate::set_window_salt(0);
        crate::set_window_is_root(true);
    }

    /// Work another thread finishes (a file dialog's answer, a print job, a Zotero call) wakes
    /// egui's root viewport, whichever window asked: a root frame that no input caused passes the
    /// wake-up on to the windows waiting on such work, so their frames pick the answer up.
    fn pass_wake_ups_on(&self, ctx: &egui::Context) {
        if self.windows.len() < 2 || ctx.input(|i| !i.events.is_empty()) {
            return;
        }
        for w in self.windows.iter().filter(|w| w.viewport != ViewportId::ROOT && !w.closing && w.app.waits_on_background()) {
            ctx.request_repaint_of(w.viewport);
        }
    }

    /// A screenshot (`ui.screenshot`) comes back as an event in whichever viewport's frame runs
    /// next, not necessarily the window that asked: one taken of another window goes to that
    /// window's app (this window's own frame handles its own).
    fn pass_screenshots_on(&mut self, vp: ViewportId, ctx: &egui::Context) {
        let foreign: Vec<(ViewportId, u64, std::sync::Arc<egui::ColorImage>)> = ctx.input(|i| {
            i.raw
                .events
                .iter()
                .filter_map(|e| match e {
                    egui::Event::Screenshot { viewport_id, .. } if *viewport_id != vp => {
                        crate::screenshot_event(e).map(|(token, image)| (*viewport_id, token, image))
                    }
                    _ => None,
                })
                .collect()
        });
        for (owner, token, image) in foreign {
            if let Some(w) = self.window_of_viewport_mut(owner) {
                w.app.screenshot_arrived(token, &image);
            }
        }
    }

    /// eframe's raw input hook, for the viewport the input is for: the menu bar's events go to
    /// the window in use (the one the menu shows), then the window's own hook runs.
    pub fn raw_input_hook(&mut self, raw: &mut egui::RawInput) {
        let Some(i) = self.index_of_viewport(raw.viewport_id) else { return };
        if self.is_active_viewport(raw.viewport_id)
            && let Some(menu) = &mut self.menu
        {
            native_menu::raw_input(menu, &mut self.windows[i].app, raw);
        }
        self.windows[i].app.raw_input_hook(raw);
    }

    /// The root pass: every other window is declared, so egui keeps it; a window being closed is
    /// left out, which closes it, and dropped.
    fn declare_children(&mut self, ctx: &egui::Context) {
        for w in &self.windows {
            if w.viewport == ViewportId::ROOT || w.closing {
                continue;
            }
            let vp = w.viewport;
            ctx.show_viewport_deferred(vp, w.builder.clone(), move |ui, _class| {
                Host::with(|h| {
                    h.logic(vp, ui.ctx());
                    h.ui(vp, ui);
                });
            });
        }
        for w in self.windows.iter_mut().filter(|w| w.closing) {
            w.app.session.finish_recovery();
        }
        self.windows.retain(|w| !w.closing);
    }

    /// Carry out what a window asked for.
    fn handle(&mut self, from: WindowId, req: WindowRequest, ctx: &egui::Context) {
        match req {
            WindowRequest::Open(params) => {
                if let Err(e) = self.spawn(from, Some(("file.open", params.clone())), ctx) {
                    let path = params.get("path").and_then(Value::as_str).unwrap_or("");
                    log::warn!("{path}: {e}");
                    let msg = crate::i18n::fmt(tl!("Couldn't open {path}: {error}"), &[("path", path), ("error", &e)]);
                    if let Some(w) = self.get_mut(from) {
                        w.app.status(msg);
                    }
                }
            }
            WindowRequest::New { command, params } => {
                if let Err(e) = self.spawn(from, Some((&command, params)), ctx)
                    && let Some(w) = self.get_mut(from)
                {
                    w.app.status(e);
                }
            }
            WindowRequest::Focus(id) => self.focus(id, ctx),
            WindowRequest::ArrangeAll => self.arrange_all(ctx),
            WindowRequest::SideBySide { on: false, .. } => self.end_side_by_side(ctx),
            WindowRequest::SideBySide { on: true, with } => {
                let partner = with.or_else(|| {
                    let others: Vec<WindowId> = self.windows.iter().filter(|w| !w.closing && w.id != from).map(|w| w.id).collect();
                    match others.as_slice() {
                        [one] => Some(*one),
                        _ => None,
                    }
                });
                if let Some(partner) = partner.filter(|p| *p != from && self.get(*p).is_some()) {
                    self.begin_side_by_side(from, partner, ctx);
                }
            }
            WindowRequest::SyncScroll(on) => {
                if let Some(p) = &mut self.pair {
                    p.sync = on;
                    for w in &mut self.windows {
                        w.scroll_seen = Some(w.app.canvas.scroll_offset);
                        w.scroll_applied = false;
                    }
                }
            }
            WindowRequest::ResetPosition => {
                if let Some(p) = &self.pair {
                    let (l, r) = (p.left, p.right);
                    self.place_halves(l, r, ctx);
                }
            }
            WindowRequest::Close => {
                if self.windows.iter().filter(|w| !w.closing).count() > 1 {
                    self.close_window(from, ctx);
                }
            }
            WindowRequest::Quit => self.request_quit(ctx),
        }
    }

    /// A new window from `origin`, running `command` (New or Open) in it first: a command that
    /// fails opens no window and is the error. The window takes the settings of the window it
    /// came from and appears a step down and right of it.
    fn spawn(&mut self, origin: WindowId, command: Option<(&str, Value)>, ctx: &egui::Context) -> Result<WindowId, String> {
        let mut app = (self.new_app)();
        let (geometry, work) = match self.get(origin) {
            Some(o) => {
                app.adopt_settings(&o.app.prefs());
                app.integrated_titlebar = o.app.integrated_titlebar;
                app.mac_titlebar = o.app.mac_titlebar;
                app.autosave = o.app.autosave;
                (o.app.ui.window, self.work_area_of(o))
            }
            None => (None, None),
        };
        // The command runs in the new window as in a single one (no host: it opens here, not in
        // yet another window); the host's view of the windows arrives before its first frame.
        if let Some((cmd, params)) = command {
            app.run(cmd, params)?;
        }
        app.session.windows.host = true;
        let id = self.next_id;
        self.next_id = self.next_id.wrapping_add(1).max(2);
        let viewport = ViewportId::from_hash_of(("wordcraft-window", id));
        let mut builder = self.template.clone().with_title(format!("{} - WordCraft", app.title_stem()));
        if let Some(g) = geometry.map(|g| cascade(g, work)) {
            builder = g.apply(builder);
        }
        let mut window = Window::new(id, viewport, app, builder);
        window.focused_at = self.frames;
        self.windows.push(window);
        self.active = id;
        ctx.request_repaint_of(ViewportId::ROOT);
        Ok(id)
    }

    /// Open documents handed to the app (the command line, Finder): each in a window of its own,
    /// the first one in the active window when it shows nothing yet.
    pub fn open_paths(&mut self, paths: impl IntoIterator<Item = String>, ctx: &egui::Context) {
        for p in paths {
            let Some(w) = self.active_mut() else { return };
            let id = w.id;
            match w.app.run("file.open", json!({"path": p})) {
                // Leave Backstage, as opening from the Open dialog does.
                Ok(_) => w.app.ui.backstage = false,
                Err(e) => {
                    log::warn!("{p}: {e}");
                    let msg = crate::i18n::fmt(tl!("Couldn't open {path}: {error}"), &[("path", &p), ("error", &e)]);
                    w.app.status(msg);
                }
            }
            let requests = std::mem::take(&mut w.app.window_requests);
            for req in requests {
                self.handle(id, req, ctx);
            }
        }
    }

    /// Bring a window to the front (and out of the Dock or taskbar).
    fn focus(&mut self, id: WindowId, ctx: &egui::Context) {
        let Some(w) = self.get_mut(id) else { return };
        let viewport = w.viewport;
        w.app.canvas.want_focus = true;
        ctx.send_viewport_cmd_to(viewport, ViewportCommand::Minimized(false));
        ctx.send_viewport_cmd_to(viewport, ViewportCommand::Focus);
        ctx.request_repaint_of(viewport);
        self.active = id;
    }

    /// Close a window. The root viewport is the app's window: with others open, the window that
    /// last had the focus moves into it (the root takes that window's place on screen and the
    /// other viewport goes); the last window closing ends the app.
    fn close_window(&mut self, id: WindowId, ctx: &egui::Context) {
        let Some(i) = self.index(id) else { return };
        if self.pair.as_ref().is_some_and(|p| p.left == id || p.right == id) {
            self.pair = None;
        }
        if self.windows[i].viewport == ViewportId::ROOT {
            let successor = self.windows.iter().enumerate().filter(|(j, w)| *j != i && !w.closing).max_by_key(|(_, w)| w.focused_at).map(|(j, _)| j);
            let Some(j) = successor else {
                self.exit = true;
                return;
            };
            let moved = self.windows.remove(j);
            let Some(root) = self.windows.iter_mut().find(|w| w.id == id) else { return };
            let mut old = std::mem::replace(&mut root.app, moved.app);
            old.session.finish_recovery();
            drop(old);
            root.id = moved.id;
            root.focused_at = self.frames;
            root.scroll_seen = None;
            root.scroll_applied = false;
            root.app.rehomed();
            // The root takes the closed window's place, so what the user sees is that window
            // closing and the other staying where it was.
            if let Some(outer) = moved.outer {
                ctx.send_viewport_cmd_to(ViewportId::ROOT, ViewportCommand::OuterPosition(outer.min));
            }
            if let Some(inner) = moved.inner {
                ctx.send_viewport_cmd_to(ViewportId::ROOT, ViewportCommand::InnerSize(inner.size()));
            }
            ctx.send_viewport_cmd_to(ViewportId::ROOT, ViewportCommand::Focus);
            self.active = moved.id;
        } else {
            self.windows[i].closing = true;
            if self.active == id {
                self.active = self.windows.iter().filter(|w| !w.closing).max_by_key(|w| w.focused_at).map_or(1, |w| w.id);
            }
        }
        ctx.request_repaint_of(ViewportId::ROOT);
    }

    /// Quit: close every window, asking about unsaved changes one window at a time (the prompt
    /// shows in the window it is about); Cancel in a prompt keeps the app open.
    pub fn request_quit(&mut self, ctx: &egui::Context) {
        if self.quit != Quit::No {
            return;
        }
        self.quit_next(ctx);
    }

    fn quit_next(&mut self, ctx: &egui::Context) {
        loop {
            let Some(w) = self.windows.iter_mut().find(|w| !w.closing) else {
                self.exit = true;
                self.quit = Quit::No;
                return;
            };
            let id = w.id;
            if w.app.printing.busy() {
                // `close_requested` said so on the status line; the quit is off.
                let _ = w.app.close_requested();
                self.quit = Quit::No;
                return;
            }
            if w.app.close_requested() {
                self.close_window(id, ctx);
                if self.exit {
                    self.quit = Quit::No;
                    return;
                }
                continue;
            }
            self.quit = Quit::Asking(id);
            self.focus(id, ctx);
            return;
        }
    }

    /// Each root frame while quitting: the prompt was answered (the window closes and the next
    /// one is asked) or cancelled (the quit is off).
    fn quit_tick(&mut self, ctx: &egui::Context) {
        let Quit::Asking(id) = self.quit else { return };
        let Some(w) = self.get_mut(id) else {
            self.quit = Quit::No;
            self.quit_next(ctx);
            return;
        };
        if w.app.quit_requested {
            self.close_window(id, ctx);
            self.quit = Quit::No;
            if !self.exit {
                self.quit_next(ctx);
            }
        } else if w.app.dialog.is_none() && !w.app.file_dialog_open() {
            self.quit = Quit::No;
        }
    }

    /// The control channel (`--control`): `ui.windows` lists the windows, `ui.focus` brings one
    /// forward and `app.quit` closes them all; every other request goes to the window its
    /// `window` parameter names (an id or a Switch Windows number), else to the window in use,
    /// and is answered from that window's own frame. A window that is minimized or covered gets
    /// no frame until it shows again (eframe), so a request that named no window goes to the
    /// first window instead while the one in use is hidden: the first window's logic always runs.
    fn drain_control(&mut self, ctx: &egui::Context) {
        let Some(rx) = self.control_rx.take() else { return };
        while let Ok(req) = rx.try_recv() {
            if req.method == "ui.windows" {
                let _ = req.reply.send(json!({"ok": true, "result": self.windows_json()}));
                continue;
            }
            let active = self.active().map(|w| w.id);
            let (named, target) = match req.params.get("window") {
                None | Some(Value::Null) => (false, active),
                Some(Value::String(s)) if s == "active" => (false, active),
                Some(v) => (true, v.as_u64().and_then(|n| self.resolve(n))),
            };
            let Some(mut target) = target else {
                let _ = req.reply.send(json!({"ok": false, "error": "no such window; `window` is an id or a number from ui.windows"}));
                continue;
            };
            match req.method.as_str() {
                "ui.focus" => {
                    self.focus(target, ctx);
                    let _ = req.reply.send(json!({"ok": true, "result": {"window": target}}));
                    continue;
                }
                "app.quit" => {
                    self.request_quit(ctx);
                    let _ = req.reply.send(json!({"ok": true, "result": {}}));
                    continue;
                }
                _ => {}
            }
            if !named
                && self.get(target).is_some_and(|w| w.viewport != ViewportId::ROOT && w.visible == Some(false))
                && let Some(root) = self.windows.iter().find(|w| w.viewport == ViewportId::ROOT && !w.closing)
            {
                target = root.id;
            }
            if let Some(w) = self.get_mut(target) {
                w.app.control_inbox.push(req);
                ctx.request_repaint_of(w.viewport);
            }
        }
        self.control_rx = Some(rx);
    }

    /// The windows as `ui.windows` reports them.
    pub fn windows_json(&self) -> Value {
        let active = self.active().map(|w| w.id);
        json!({
            "active": active,
            "windows": self.windows.iter().filter(|w| !w.closing).map(|w| json!({
                "id": w.id,
                "title": w.app.title_stem(),
                "path": w.app.session.path.as_ref().map(|p| p.to_string_lossy()),
                "dirty": w.app.session.dirty,
                "active": Some(w.id) == active,
                "root": w.viewport == ViewportId::ROOT,
                "sideBySide": self.pair.as_ref().is_some_and(|p| p.left == w.id || p.right == w.id),
            })).collect::<Vec<_>>(),
            "syncScroll": self.pair.as_ref().is_some_and(|p| p.sync),
        })
    }

    /// The work area the window is on: the platform's answer, else its monitor less a system bar.
    fn work_area_of(&self, w: &Window) -> Option<Rect> {
        if let Some(f) = &self.work_area
            && let Some(r) = f(w.outer)
        {
            return Some(r);
        }
        let m = w.monitor.filter(|m| m.is_finite() && m.x > 0.0 && m.y > 0.0)?;
        Some(Rect::from_min_max(pos2(0.0, 0.0), pos2(m.x, m.y - SYSTEM_BAR)))
    }

    /// Put a window's frame at `outer` (its content size follows from the frame's).
    fn place(&self, id: WindowId, outer: Rect, ctx: &egui::Context) {
        let Some(w) = self.get(id) else { return };
        let insets = match (w.outer, w.inner) {
            (Some(o), Some(i)) => (o.size() - i.size()).max(Vec2::ZERO),
            _ => Vec2::ZERO,
        };
        let size = (outer.size() - insets).max(MIN_SIZE);
        ctx.send_viewport_cmd_to(w.viewport, ViewportCommand::Maximized(false));
        ctx.send_viewport_cmd_to(w.viewport, ViewportCommand::Minimized(false));
        ctx.send_viewport_cmd_to(w.viewport, ViewportCommand::OuterPosition(outer.min));
        ctx.send_viewport_cmd_to(w.viewport, ViewportCommand::InnerSize(size));
        ctx.request_repaint_of(w.viewport);
    }

    /// View › Window › Arrange All: every window stacked top to bottom across the work area of
    /// the active window's screen, in the Switch Windows order, as Word tiles them.
    fn arrange_all(&mut self, ctx: &egui::Context) {
        let Some(work) = self.active().and_then(|w| self.work_area_of(w)) else { return };
        let ids: Vec<WindowId> = self.windows.iter().filter(|w| !w.closing).map(|w| w.id).collect();
        for (id, rect) in ids.iter().zip(tile_rows(work, ids.len())) {
            self.place(*id, rect, ctx);
        }
    }

    /// View Side by Side on: `left` (the window asking) on the left half, `right` on the right,
    /// with Synchronous Scrolling on; both windows' places are kept to restore.
    fn begin_side_by_side(&mut self, left: WindowId, right: WindowId, ctx: &egui::Context) {
        let restore = if let Some(p) = self.pair.take() {
            p.restore
        } else {
            [left, right]
                .iter()
                .filter_map(|id| {
                    let w = self.get(*id)?;
                    Some((*id, w.outer?, w.inner?.size()))
                })
                .collect()
        };
        self.pair = Some(Pair { left, right, restore, sync: true });
        for w in &mut self.windows {
            w.scroll_seen = Some(w.app.canvas.scroll_offset);
            w.scroll_applied = false;
        }
        self.place_halves(left, right, ctx);
    }

    fn place_halves(&self, left: WindowId, right: WindowId, ctx: &egui::Context) {
        let Some(work) = self.get(left).and_then(|w| self.work_area_of(w)) else { return };
        let [l, r] = halves(work);
        self.place(left, l, ctx);
        self.place(right, r, ctx);
    }

    /// View Side by Side off: both windows go back where they were.
    fn end_side_by_side(&mut self, ctx: &egui::Context) {
        let Some(p) = self.pair.take() else { return };
        for (id, outer, inner) in p.restore {
            let Some(w) = self.get(id) else { continue };
            ctx.send_viewport_cmd_to(w.viewport, ViewportCommand::OuterPosition(outer.min));
            ctx.send_viewport_cmd_to(w.viewport, ViewportCommand::InnerSize(inner.max(MIN_SIZE)));
            ctx.request_repaint_of(w.viewport);
        }
    }

    /// Synchronous Scrolling: a scroll in one window of the pair moves the other by the same
    /// amount, once (the partner's own change from applying it is not passed back).
    fn sync_scroll_tick(&mut self, i: usize, ctx: &egui::Context) {
        let now = self.windows[i].app.canvas.scroll_offset;
        let id = self.windows[i].id;
        let partner = match &self.pair {
            Some(p) if p.sync && p.left == id => Some(p.right),
            Some(p) if p.sync && p.right == id => Some(p.left),
            _ => None,
        };
        let w = &mut self.windows[i];
        let delta = match w.scroll_seen {
            Some(seen) => now - seen,
            None => Vec2::ZERO,
        };
        w.scroll_seen = Some(now);
        if delta == Vec2::ZERO || partner.is_none() {
            return;
        }
        if w.scroll_applied {
            w.scroll_applied = false;
            return;
        }
        if let Some(p) = partner.and_then(|p| self.get_mut(p))
            && p.app.canvas.scroll_by(delta)
        {
            p.scroll_applied = true;
            ctx.request_repaint_of(p.viewport);
        }
    }

    /// The interface fonts are one set for every window: a face one window needs (a caption in
    /// Hebrew, a composition in a document font) joins every window's list, so whichever window
    /// installs the fonts next installs it too, and the lists never fight.
    fn pool_fonts(&mut self) {
        if self.windows.len() < 2 {
            return;
        }
        let mut faces: Vec<std::sync::Arc<wordcraft_fonts::FontFace>> = Vec::new();
        let mut cjk = false;
        for w in &self.windows {
            cjk |= w.app.want_system_cjk;
            for f in &w.app.canvas.ime_faces {
                if !faces.iter().any(|g| std::sync::Arc::ptr_eq(g, f)) {
                    faces.push(f.clone());
                }
            }
        }
        for w in &mut self.windows {
            w.app.adopt_faces(&faces, cjk);
        }
    }

    /// The settings the user changed in the window they are using go to the other windows.
    fn share_settings(&mut self, id: WindowId, ctx: &egui::Context) {
        if self.windows.len() < 2 || self.active != id {
            return;
        }
        let Some(w) = self.get(id) else { return };
        let prefs = w.app.prefs();
        let Ok(now) = serde_json::to_string(&prefs) else { return };
        if self.shared_settings.as_deref() == Some(now.as_str()) {
            return;
        }
        self.shared_settings = Some(now);
        for w in self.windows.iter_mut().filter(|w| w.id != id) {
            w.app.adopt_settings(&prefs);
            ctx.request_repaint_of(w.viewport);
        }
    }

    /// The end of the run: every document's recovery copy is settled.
    pub fn on_exit(&mut self) {
        for w in &mut self.windows {
            w.app.session.finish_recovery();
        }
    }
}

impl Window {
    fn new(id: WindowId, viewport: ViewportId, app: WordApp, builder: ViewportBuilder) -> Self {
        Window {
            id,
            viewport,
            app,
            builder,
            outer: None,
            inner: None,
            monitor: None,
            visible: None,
            focused_at: 0,
            closing: false,
            scroll_seen: None,
            scroll_applied: false,
        }
    }
}

/// Where a window opened from one at `from` goes: a step down and right, back to the work area's
/// corner when that would run off it. Never maximized: the new window is a window of its own.
pub fn cascade(from: WindowGeometry, work: Option<Rect>) -> WindowGeometry {
    let size = vec2(from.width, from.height);
    let position = from.position.map(|[x, y]| {
        let mut p = pos2(x, y) + CASCADE;
        if let Some(work) = work
            && (p.x + size.x > work.right() || p.y + size.y > work.bottom())
        {
            p = work.min + CASCADE;
        }
        [p.x, p.y]
    });
    WindowGeometry { position, maximized: false, ..from }
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

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::*;
    use crate::Services;
    use egui::{Pos2, Rect, ViewportEvent, ViewportInfo, pos2, vec2};
    use wordcraft_engine::Session;

    const UNSAVED: &str = "Unsaved words";

    fn app() -> WordApp {
        let mut a = WordApp::new(Session::new(wordcraft_doc::Document::new()), Services::default());
        a.ui.animations = crate::theme::Animations::Off;
        a
    }

    fn host() -> (Host, egui::Context) {
        let ctx = egui::Context::default();
        // Real child viewports, as eframe has them, not windows embedded in the root.
        ctx.set_embed_viewports(false);
        (Host::new(app(), ViewportBuilder::default(), Box::new(app)), ctx)
    }

    fn body(w: &Window) -> String {
        w.app.session.doc.plain_text(wordcraft_doc::StoryRef::Body)
    }

    fn vp(h: &Host, id: WindowId) -> ViewportId {
        h.get(id).map(|w| w.viewport).unwrap_or_else(|| panic!("window {id}"))
    }

    fn ids(h: &Host) -> Vec<WindowId> {
        h.windows().map(|w| w.id).collect()
    }

    /// One pass of the window in viewport `vp`, as eframe runs it: with `rect` as its place on
    /// a 1920 × 1080 monitor, `focused`, `events` (a close request, say) and input `input`.
    /// Returns the viewport commands the pass sent, by the viewport they went to.
    fn pass(
        h: &mut Host,
        ctx: &egui::Context,
        vp: ViewportId,
        rect: Rect,
        focused: bool,
        events: Vec<ViewportEvent>,
        input: Vec<egui::Event>,
    ) -> BTreeMap<ViewportId, Vec<ViewportCommand>> {
        let info = ViewportInfo {
            parent: (vp != ViewportId::ROOT).then_some(ViewportId::ROOT),
            events,
            outer_rect: Some(rect),
            inner_rect: Some(rect),
            monitor_size: Some(vec2(1920.0, 1080.0)),
            focused: Some(focused),
            ..Default::default()
        };
        let mut raw = egui::RawInput {
            viewport_id: vp,
            focused,
            time: Some(ctx.input(|i| i.time) + 1.0 / 60.0),
            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, rect.size())),
            events: input,
            ..Default::default()
        };
        raw.viewports.insert(vp, info);
        h.raw_input_hook(&mut raw);
        let out = ctx.run_ui(raw, |ui| {
            h.logic(vp, ui.ctx());
            h.ui(vp, ui);
        });
        let sent = out.viewport_output.iter().map(|(id, v)| (*id, v.commands.clone())).collect();
        out.drop_without_applying_deltas();
        sent
    }

    /// [`pass`] for the window `id`.
    fn pass_id(
        h: &mut Host,
        ctx: &egui::Context,
        id: WindowId,
        rect: Rect,
        focused: bool,
        events: Vec<ViewportEvent>,
        input: Vec<egui::Event>,
    ) -> BTreeMap<ViewportId, Vec<ViewportCommand>> {
        let v = vp(h, id);
        pass(h, ctx, v, rect, focused, events, input)
    }

    /// Settle: a few passes of every window, none focused, the root last (it drops closed windows).
    fn settle(h: &mut Host, ctx: &egui::Context) {
        for _ in 0..3 {
            let others: Vec<ViewportId> = h.windows().map(|w| w.viewport).filter(|v| *v != ViewportId::ROOT).collect();
            for (i, v) in others.iter().enumerate() {
                pass(h, ctx, *v, Rect::from_min_size(pos2(100.0 * (i + 1) as f32, 80.0), vec2(1280.0, 800.0)), false, vec![], vec![]);
            }
            pass(h, ctx, ViewportId::ROOT, Rect::from_min_size(pos2(40.0, 40.0), vec2(1280.0, 800.0)), false, vec![], vec![]);
        }
    }

    fn temp_file(name: &str, text: &str) -> String {
        let dir = std::env::temp_dir().join(format!("wordcraft-windows-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join(name);
        std::fs::write(&p, text).unwrap();
        p.to_string_lossy().into_owned()
    }

    /// Word opens each document in a window of its own and leaves the current one alone, so
    /// nothing is asked; a new blank window nobody typed in takes the document itself (Word
    /// replaces its untouched Document1 too); a file already open comes to the front instead of
    /// opening twice.
    #[test]
    fn new_and_open_go_to_their_own_windows_and_a_blank_window_takes_the_document() {
        let (mut h, ctx) = host();
        settle(&mut h, &ctx);
        let first = h.active().unwrap().id;
        assert!(h.active().unwrap().app.session.windows.host, "the engine knows the host");
        assert!(h.active().unwrap().app.window_reusable(), "a blank window nobody typed in");

        let alpha = temp_file("alpha.txt", "Alpha");
        h.active_mut().unwrap().app.run("file.open", json!({"path": alpha})).unwrap();
        settle(&mut h, &ctx);
        assert_eq!(ids(&h), vec![first], "the blank window took the file");
        assert!(body(h.get(first).unwrap()).contains("Alpha"));
        assert!(!h.get(first).unwrap().app.window_reusable());

        // Typing in it, then New: a new window, and no Save Changes prompt here.
        h.get_mut(first).unwrap().app.run("text.insert", json!({"text": UNSAVED})).unwrap();
        let r = h.get_mut(first).unwrap().app.run("file.new", json!({})).unwrap();
        assert_eq!(r["pending"], "window");
        settle(&mut h, &ctx);
        assert_eq!(ids(&h).len(), 2);
        let second = ids(&h)[1];
        assert!(h.get(first).unwrap().app.dialog.is_none(), "nothing asked");
        assert!(body(h.get(first).unwrap()).contains(UNSAVED), "the first document stays");
        assert!(body(h.get(second).unwrap()).trim().is_empty(), "the new one is blank");
        assert_ne!(vp(&h, second), ViewportId::ROOT, "in a window of its own");
        assert_eq!(h.active().unwrap().id, second, "the new window is the one in use");
        assert!(h.get(second).unwrap().app.session.windows.host, "and knows the host from the start");

        // Open from the first window: a third window; the same file again brings it forward.
        let beta = temp_file("beta.txt", "Beta");
        h.get_mut(first).unwrap().app.run("file.open", json!({"path": beta})).unwrap();
        settle(&mut h, &ctx);
        assert_eq!(ids(&h).len(), 3);
        let third = ids(&h)[2];
        assert!(body(h.get(third).unwrap()).contains("Beta"));
        let r = h.get_mut(first).unwrap().app.run("file.open", json!({"path": beta})).unwrap();
        assert_eq!(r["alreadyOpen"], true);
        let sent = pass(&mut h, &ctx, ViewportId::ROOT, Rect::from_min_size(pos2(40.0, 40.0), vec2(1280.0, 800.0)), true, vec![], vec![]);
        assert!(sent.get(&vp(&h, third)).is_some_and(|c| c.contains(&ViewportCommand::Focus)), "the open window comes forward: {sent:?}");
        settle(&mut h, &ctx);
        assert_eq!(ids(&h).len(), 3, "not opened twice");
        assert_eq!(h.active().unwrap().id, third);

        // A file that can't be opened: no window, the error on the status line of the asker.
        h.get_mut(first).unwrap().app.run("file.open", json!({"path": "/nonexistent/none.docx"})).unwrap();
        settle(&mut h, &ctx);
        assert_eq!(ids(&h).len(), 3);
        assert!(h.get(first).unwrap().app.status_msg.as_ref().is_some_and(|(m, _)| m.contains("none.docx")), "the asker hears why");

        // The engine's view from one window: every window, in order, this one marked.
        let w = &h.get(second).unwrap().app.session.windows;
        assert_eq!((w.this, w.list.len(), w.list[0].id), (second, 3, first));
        assert_eq!(w.list[2].path.as_deref(), Some(beta.as_str()));
    }

    /// File › Close closes the document's window when another is open, as Word does; the last
    /// window stays, empty (#1712). The window's close button closes only its window; the root
    /// window (the app's) takes over the document last used when it is the one closing, and
    /// closing the last window ends the app.
    #[test]
    fn closing_takes_one_window_and_the_root_takes_over_the_last_used_document() {
        let (mut h, ctx) = host();
        settle(&mut h, &ctx);
        let first = h.active().unwrap().id;
        h.get_mut(first).unwrap().app.run("text.insert", json!({"text": "Root text"})).unwrap();
        h.get_mut(first).unwrap().app.session.dirty = false;
        h.get_mut(first).unwrap().app.run("file.new", json!({})).unwrap();
        settle(&mut h, &ctx);
        let second = ids(&h)[1];
        h.get_mut(second).unwrap().app.run("text.insert", json!({"text": "Second text"})).unwrap();
        h.get_mut(second).unwrap().app.session.dirty = false;
        h.get_mut(second).unwrap().app.run("file.new", json!({})).unwrap();
        settle(&mut h, &ctx);
        let third = ids(&h)[2];
        assert_eq!(ids(&h).len(), 3);

        // File › Close in the third: its window goes; the others keep their documents.
        let r = h.get_mut(third).unwrap().app.run("file.close", json!({})).unwrap();
        assert_eq!(r["closed"], true);
        settle(&mut h, &ctx);
        assert_eq!(ids(&h), vec![first, second]);
        assert!(h.get(first).unwrap().app.has_document() && h.get(second).unwrap().app.has_document());

        // The close button on the root window while the second was last in use: the second's
        // document moves into the root window, which takes the second window's place.
        let second_rect = Rect::from_min_size(pos2(300.0, 200.0), vec2(1000.0, 700.0));
        pass_id(&mut h, &ctx, second, second_rect, true, vec![], vec![]);
        let root_rect = Rect::from_min_size(pos2(40.0, 40.0), vec2(1280.0, 800.0));
        let sent = pass(&mut h, &ctx, ViewportId::ROOT, root_rect, true, vec![ViewportEvent::Close], vec![]);
        assert!(!h.exit_requested(), "another window is open");
        assert_eq!(ids(&h), vec![second], "the root window now holds the second document");
        assert_eq!(vp(&h, second), ViewportId::ROOT);
        assert!(body(h.get(second).unwrap()).contains("Second text"));
        let root = sent.get(&ViewportId::ROOT).cloned().unwrap_or_default();
        assert!(root.contains(&ViewportCommand::OuterPosition(second_rect.min)), "takes the closed window's place: {root:?}");
        assert!(root.contains(&ViewportCommand::InnerSize(second_rect.size())));
        settle(&mut h, &ctx);
        assert_eq!(ids(&h).len(), 1);

        // File › Close in the last window keeps the window, empty, as before.
        h.get_mut(second).unwrap().app.run("file.close", json!({})).unwrap();
        settle(&mut h, &ctx);
        assert_eq!(ids(&h).len(), 1);
        assert!(!h.get(second).unwrap().app.has_document());
        // The close button on the last window ends the app.
        pass(&mut h, &ctx, ViewportId::ROOT, root_rect, true, vec![ViewportEvent::Close], vec![]);
        assert!(h.exit_requested());
    }

    /// The close button on a window with unsaved changes asks first, in that window; Cancel keeps
    /// it, Don't Save closes just that window.
    #[test]
    fn a_child_window_with_unsaved_changes_asks_before_it_closes() {
        let (mut h, ctx) = host();
        settle(&mut h, &ctx);
        let first = h.active().unwrap().id;
        h.get_mut(first).unwrap().app.run("file.new", json!({})).unwrap();
        settle(&mut h, &ctx);
        let second = ids(&h)[1];
        h.get_mut(second).unwrap().app.run("text.insert", json!({"text": UNSAVED})).unwrap();
        let rect = Rect::from_min_size(pos2(300.0, 200.0), vec2(1000.0, 700.0));
        let sent = pass_id(&mut h, &ctx, second, rect, true, vec![ViewportEvent::Close], vec![]);
        assert!(sent.get(&vp(&h, second)).is_some_and(|c| c.contains(&ViewportCommand::CancelClose)), "{sent:?}");
        assert_eq!(h.get(second).unwrap().app.dialog.as_ref().map(|d| d.name()), Some("saveChanges"));
        h.get_mut(second).unwrap().app.run("ui.saveChanges", json!({"answer": "cancel"})).unwrap();
        settle(&mut h, &ctx);
        assert_eq!(ids(&h), vec![first, second], "Cancel keeps the window");

        pass_id(&mut h, &ctx, second, rect, true, vec![ViewportEvent::Close], vec![]);
        h.get_mut(second).unwrap().app.run("ui.saveChanges", json!({"answer": "dontSave"})).unwrap();
        assert!(h.get(second).unwrap().app.quit_requested, "the window may close");
        settle(&mut h, &ctx);
        assert_eq!(ids(&h), vec![first], "only that window closed");
        assert!(!h.exit_requested());
    }

    /// Quitting asks about each window with unsaved changes in turn, in that window, and Cancel
    /// in any prompt keeps the app open with every window; once each is answered the app ends.
    #[test]
    fn quitting_asks_each_dirty_window_in_turn_and_cancel_keeps_the_app() {
        let (mut h, ctx) = host();
        settle(&mut h, &ctx);
        let first = h.active().unwrap().id;
        h.get_mut(first).unwrap().app.run("file.new", json!({})).unwrap();
        settle(&mut h, &ctx);
        let second = ids(&h)[1];
        for id in [first, second] {
            h.get_mut(id).unwrap().app.run("text.insert", json!({"text": UNSAVED})).unwrap();
        }
        h.request_quit(&ctx);
        assert_eq!(h.get(first).unwrap().app.dialog.as_ref().map(|d| d.name()), Some("saveChanges"), "the first window asks");
        assert!(h.get(second).unwrap().app.dialog.is_none(), "one at a time");
        h.get_mut(first).unwrap().app.run("ui.saveChanges", json!({"answer": "cancel"})).unwrap();
        settle(&mut h, &ctx);
        assert_eq!(ids(&h).len(), 2, "Cancel keeps everything");
        assert!(!h.exit_requested());
        assert_eq!(h.quit, Quit::No);

        h.request_quit(&ctx);
        h.get_mut(first).unwrap().app.run("ui.saveChanges", json!({"answer": "dontSave"})).unwrap();
        settle(&mut h, &ctx);
        assert_eq!(ids(&h), vec![second], "the first closed; the second is asked");
        assert_eq!(h.get(second).unwrap().app.dialog.as_ref().map(|d| d.name()), Some("saveChanges"));
        h.get_mut(second).unwrap().app.run("ui.saveChanges", json!({"answer": "dontSave"})).unwrap();
        settle(&mut h, &ctx);
        assert!(h.exit_requested(), "every window answered: the app ends");
    }

    /// Arrange All stacks the windows top to bottom across the work area; View Side by Side puts
    /// the asking window on the left half and its partner on the right, with Synchronous Scrolling
    /// on; off again puts both back where they were. Reset Window Position re-halves them.
    #[test]
    fn arrange_all_and_side_by_side_place_the_windows() {
        let (mut h, ctx) = host();
        h.work_area = Some(Box::new(|_| Some(Rect::from_min_size(pos2(0.0, 25.0), vec2(1600.0, 975.0)))));
        settle(&mut h, &ctx);
        let first = h.active().unwrap().id;
        h.get_mut(first).unwrap().app.run("file.new", json!({})).unwrap();
        settle(&mut h, &ctx);
        let second = ids(&h)[1];
        let (vp1, vp2) = (vp(&h, first), vp(&h, second));
        let r1 = Rect::from_min_size(pos2(40.0, 40.0), vec2(1280.0, 800.0));
        let r2 = Rect::from_min_size(pos2(300.0, 200.0), vec2(1000.0, 700.0));
        pass(&mut h, &ctx, vp2, r2, false, vec![], vec![]);
        pass(&mut h, &ctx, vp1, r1, true, vec![], vec![]);

        h.get_mut(first).unwrap().app.run("view.arrangeAll", json!({})).unwrap();
        let sent = pass(&mut h, &ctx, vp1, r1, true, vec![], vec![]);
        let rows = tile_rows(Rect::from_min_size(pos2(0.0, 25.0), vec2(1600.0, 975.0)), 2);
        for (v, row) in [(vp1, rows[0]), (vp2, rows[1])] {
            let c = sent.get(&v).cloned().unwrap_or_default();
            assert!(c.contains(&ViewportCommand::OuterPosition(row.min)) && c.contains(&ViewportCommand::InnerSize(row.size())), "{v:?}: {c:?}");
            assert!(c.contains(&ViewportCommand::Maximized(false)));
        }

        // Side by Side from the first window: left half, the second on the right half.
        let r = h.get_mut(first).unwrap().app.run("view.sideBySide", json!({})).unwrap();
        assert_eq!(r["with"], second);
        let sent = pass(&mut h, &ctx, vp1, r1, true, vec![], vec![]);
        let [left, right] = halves(Rect::from_min_size(pos2(0.0, 25.0), vec2(1600.0, 975.0)));
        assert!(sent.get(&vp1).is_some_and(|c| c.contains(&ViewportCommand::OuterPosition(left.min))), "{sent:?}");
        assert!(sent.get(&vp2).is_some_and(|c| c.contains(&ViewportCommand::OuterPosition(right.min))), "{sent:?}");
        settle(&mut h, &ctx);
        let w = &h.get(second).unwrap().app.session.windows;
        assert_eq!((w.side_by_side, w.sync_scroll), (Some(first), true), "the partner sees it too");
        assert!(h.get(second).unwrap().app.session.why_disabled("view.syncScroll").is_none());

        // Synchronous Scrolling off and on again; Reset Window Position halves again.
        assert_eq!(h.get_mut(first).unwrap().app.run("view.syncScroll", json!({})).unwrap()["value"], false);
        settle(&mut h, &ctx);
        assert!(!h.get(first).unwrap().app.session.windows.sync_scroll);
        h.get_mut(first).unwrap().app.run("view.syncScroll", json!({"value": true})).unwrap();
        h.get_mut(first).unwrap().app.run("view.resetWindowPosition", json!({})).unwrap();
        let sent = pass(&mut h, &ctx, vp1, left, true, vec![], vec![]);
        assert!(sent.get(&vp2).is_some_and(|c| c.contains(&ViewportCommand::OuterPosition(right.min))), "{sent:?}");

        // Off: both windows go back where they were.
        assert_eq!(h.get_mut(first).unwrap().app.run("view.sideBySide", json!({})).unwrap()["value"], false);
        let sent = pass(&mut h, &ctx, vp1, left, true, vec![], vec![]);
        assert!(
            sent.get(&vp1).is_some_and(|c| c.contains(&ViewportCommand::OuterPosition(r1.min)) && c.contains(&ViewportCommand::InnerSize(r1.size()))),
            "{sent:?}"
        );
        assert!(
            sent.get(&vp2).is_some_and(|c| c.contains(&ViewportCommand::OuterPosition(r2.min)) && c.contains(&ViewportCommand::InnerSize(r2.size()))),
            "{sent:?}"
        );
        settle(&mut h, &ctx);
        assert!(h.get(first).unwrap().app.session.windows.side_by_side.is_none());
        assert!(h.get(first).unwrap().app.session.why_disabled("view.syncScroll").is_some(), "only while side by side");

        // Closing a window of the pair ends Side by Side.
        h.get_mut(first).unwrap().app.run("view.sideBySide", json!({})).unwrap();
        settle(&mut h, &ctx);
        assert!(h.pair.is_some());
        h.get_mut(second).unwrap().app.run("file.close", json!({})).unwrap();
        settle(&mut h, &ctx);
        assert!(h.pair.is_none() && ids(&h) == vec![first]);
    }

    /// Switch Windows brings the named window forward and makes it the one in use.
    #[test]
    fn switch_windows_brings_the_named_window_forward() {
        let (mut h, ctx) = host();
        settle(&mut h, &ctx);
        let first = h.active().unwrap().id;
        h.get_mut(first).unwrap().app.run("file.new", json!({})).unwrap();
        settle(&mut h, &ctx);
        let second = ids(&h)[1];
        let r1 = Rect::from_min_size(pos2(40.0, 40.0), vec2(1280.0, 800.0));
        pass(&mut h, &ctx, ViewportId::ROOT, r1, true, vec![], vec![]);
        assert_eq!(h.active().unwrap().id, first);
        h.get_mut(first).unwrap().app.run("view.switchWindows", json!({"window": 2})).unwrap();
        let sent = pass(&mut h, &ctx, ViewportId::ROOT, r1, true, vec![], vec![]);
        let c = sent.get(&vp(&h, second)).cloned().unwrap_or_default();
        assert!(c.contains(&ViewportCommand::Focus) && c.contains(&ViewportCommand::Minimized(false)), "{c:?}");
        assert_eq!(h.active().unwrap().id, second);
    }

    /// Synchronous Scrolling: a wheel in one window of the pair scrolls the other by as much,
    /// once; the partner's own change from applying it is not sent back.
    #[test]
    fn synchronous_scrolling_moves_the_partner_by_as_much_once() {
        let ctx = egui::Context::default();
        ctx.set_embed_viewports(false);
        let long = || {
            let mut a = app();
            for _ in 0..3 {
                a.run("insert.pageBreak", json!({})).unwrap();
            }
            a.run("caret.docStart", json!({})).unwrap();
            a.session.dirty = false;
            a
        };
        let mut h = Host::new(long(), ViewportBuilder::default(), Box::new(app));
        h.work_area = Some(Box::new(|_| Some(Rect::from_min_size(Pos2::ZERO, vec2(1600.0, 1000.0)))));
        settle(&mut h, &ctx);
        let first = h.active().unwrap().id;
        h.get_mut(first).unwrap().app.run("file.new", json!({})).unwrap();
        settle(&mut h, &ctx);
        let second = ids(&h)[1];
        // The second document is long too, so it has room to scroll.
        for _ in 0..3 {
            h.get_mut(second).unwrap().app.run("insert.pageBreak", json!({})).unwrap();
        }
        h.get_mut(second).unwrap().app.run("caret.docStart", json!({})).unwrap();
        h.get_mut(second).unwrap().app.session.dirty = false;
        let (vp1, vp2) = (vp(&h, first), vp(&h, second));
        let r1 = Rect::from_min_size(Pos2::ZERO, vec2(800.0, 1000.0));
        let r2 = Rect::from_min_size(pos2(800.0, 0.0), vec2(800.0, 1000.0));
        h.get_mut(first).unwrap().app.run("view.sideBySide", json!({})).unwrap();
        for _ in 0..3 {
            pass(&mut h, &ctx, vp2, r2, false, vec![], vec![]);
            pass(&mut h, &ctx, vp1, r1, true, vec![], vec![]);
        }
        let offset = |h: &Host, id| h.get(id).unwrap().app.canvas.scroll_offset.y;
        assert_eq!((offset(&h, first), offset(&h, second)), (0.0, 0.0));
        let at = h.get(first).unwrap().app.canvas.canvas_rect.unwrap().center();
        let wheel = vec![
            egui::Event::PointerMoved(at),
            egui::Event::MouseWheel {
                unit: egui::MouseWheelUnit::Point,
                delta: vec2(0.0, -300.0),
                phase: egui::TouchPhase::Move,
                modifiers: egui::Modifiers::NONE,
            },
        ];
        pass(&mut h, &ctx, vp1, r1, true, vec![], wheel);
        for _ in 0..3 {
            pass(&mut h, &ctx, vp1, r1, true, vec![], vec![]);
            pass(&mut h, &ctx, vp2, r2, false, vec![], vec![]);
        }
        let (a, b) = (offset(&h, first), offset(&h, second));
        assert!(a > 100.0, "the first window scrolled by {a}");
        assert!((a - b).abs() < 1.0, "the partner followed: {a} vs {b}");
        for _ in 0..4 {
            pass(&mut h, &ctx, vp1, r1, true, vec![], vec![]);
            pass(&mut h, &ctx, vp2, r2, false, vec![], vec![]);
        }
        assert_eq!((offset(&h, first), offset(&h, second)), (a, b), "nothing echoes back and forth");
    }

    /// The control channel: `ui.windows` lists the windows, and a request goes to the window its
    /// `window` names (an id, or a number in that list), else to the window in use; a window that
    /// isn't open is an error.
    #[test]
    fn control_requests_reach_the_named_window() {
        let (mut h, ctx) = host();
        let (tx, rx) = std::sync::mpsc::channel();
        h = h.with_control(rx);
        settle(&mut h, &ctx);
        let first = h.active().unwrap().id;
        h.get_mut(first).unwrap().app.run("file.new", json!({})).unwrap();
        settle(&mut h, &ctx);
        let second = ids(&h)[1];

        let (req, reply) = ControlRequest::new("ui.windows", json!({}));
        tx.send(req).unwrap();
        settle(&mut h, &ctx);
        let v = reply.try_recv().unwrap();
        assert_eq!(v["ok"], true);
        assert_eq!(v["result"]["windows"].as_array().map(Vec::len), Some(2));
        assert_eq!(v["result"]["active"], second);

        // New and Open from the channel open windows too, as from the ribbon: a window of their own.
        let (req, reply) = ControlRequest::new("engine.execute", json!({"command": "file.new"}));
        tx.send(req).unwrap();
        settle(&mut h, &ctx);
        assert_eq!(reply.try_recv().unwrap()["result"]["pending"], "window");
        assert_eq!(ids(&h).len(), 3);
        let third = ids(&h)[2];
        h.get_mut(third).unwrap().app.run("file.close", json!({})).unwrap();
        settle(&mut h, &ctx);
        assert_eq!(ids(&h), vec![first, second]);

        let (req, reply) =
            ControlRequest::new("engine.execute", json!({"window": first, "command": "text.insert", "params": {"text": "To the first"}}));
        tx.send(req).unwrap();
        let (req2, reply2) = ControlRequest::new("engine.execute", json!({"command": "text.insert", "params": {"text": "To the active"}}));
        tx.send(req2).unwrap();
        let (req3, reply3) = ControlRequest::new("engine.execute", json!({"window": 99, "command": "text.insert", "params": {"text": "Nowhere"}}));
        tx.send(req3).unwrap();
        settle(&mut h, &ctx);
        assert_eq!(reply.try_recv().unwrap()["ok"], true);
        assert_eq!(reply2.try_recv().unwrap()["ok"], true);
        assert_eq!(reply3.try_recv().unwrap()["ok"], false);
        assert!(body(h.get(first).unwrap()).contains("To the first"));
        assert!(body(h.get(second).unwrap()).contains("To the active"));
        assert!(!body(h.get(second).unwrap()).contains("To the first"));
    }

    /// A setting changed in the window in use (the theme, the language, the recent list…) reaches
    /// the other windows; what is a window's own (its ribbon tab) stays.
    #[test]
    fn settings_changed_in_one_window_reach_the_others() {
        let (mut h, ctx) = host();
        settle(&mut h, &ctx);
        let first = h.active().unwrap().id;
        h.get_mut(first).unwrap().app.run("file.new", json!({})).unwrap();
        settle(&mut h, &ctx);
        let second = ids(&h)[1];
        let r2 = Rect::from_min_size(pos2(300.0, 200.0), vec2(1000.0, 700.0));
        pass_id(&mut h, &ctx, second, r2, true, vec![], vec![]);
        h.get_mut(second).unwrap().app.run("ui.theme", json!({"value": "dark"})).unwrap();
        h.get_mut(second).unwrap().app.run("ui.tab", json!({"tab": "View"})).unwrap();
        // A frame with input in the window in use hands the settings on.
        pass_id(&mut h, &ctx, second, r2, true, vec![], vec![egui::Event::PointerMoved(pos2(10.0, 10.0))]);
        assert_eq!(h.get(first).unwrap().app.ui.theme, crate::theme::Appearance::Dark);
        assert_eq!(h.get(first).unwrap().app.ui.tab, crate::DEFAULT_TAB, "the tab is the window's own");
    }

    /// Arrange All's rows, Side by Side's halves, and where a window opened from another goes.
    #[test]
    fn placement_geometry() {
        let work = Rect::from_min_size(pos2(0.0, 25.0), vec2(1600.0, 975.0));
        let rows = tile_rows(work, 3);
        assert_eq!(rows.len(), 3);
        assert_eq!(rows[0], Rect::from_min_size(pos2(0.0, 25.0), vec2(1600.0, 480.0)), "at least the minimum height");
        assert_eq!(rows[1].min, pos2(0.0, 505.0));
        let rows = tile_rows(work, 2);
        assert_eq!(rows[1], Rect::from_min_size(pos2(0.0, 512.5), vec2(1600.0, 487.5)));
        let [l, r] = halves(work);
        assert_eq!(l, Rect::from_min_size(pos2(0.0, 25.0), vec2(800.0, 975.0)));
        assert_eq!(r, Rect::from_min_size(pos2(800.0, 25.0), vec2(800.0, 975.0)));
        assert_eq!(halves(Rect::from_min_size(Pos2::ZERO, vec2(1000.0, 600.0)))[1].min.x, MIN_SIZE.x, "never narrower than the minimum");

        let from = WindowGeometry { position: Some([100.0, 100.0]), width: 1280.0, height: 800.0, maximized: true, monitor: Some([1920.0, 1080.0]) };
        let next = cascade(from, Some(Rect::from_min_size(Pos2::ZERO, vec2(1920.0, 1080.0))));
        assert_eq!((next.position, next.maximized), (Some([122.0, 122.0]), false));
        let far = cascade(WindowGeometry { position: Some([700.0, 300.0]), ..from }, Some(Rect::from_min_size(Pos2::ZERO, vec2(1920.0, 1080.0))));
        assert_eq!(far.position, Some([22.0, 22.0]), "back to the corner when it would run off the screen");
        assert_eq!(cascade(WindowGeometry { position: None, ..from }, None).position, None, "no position: the system places it");
    }

    /// Fixed widget ids are kept apart per window, and are the plain ids outside a window host.
    #[test]
    fn fixed_widget_ids_are_per_window() {
        crate::set_window_salt(0);
        assert_eq!(crate::wid("canvas"), egui::Id::new("canvas"));
        crate::set_window_salt(1);
        let one = crate::wid("canvas");
        crate::set_window_salt(2);
        let two = crate::wid("canvas");
        crate::set_window_salt(0);
        assert!(one != two && one != egui::Id::new("canvas"));
    }
}

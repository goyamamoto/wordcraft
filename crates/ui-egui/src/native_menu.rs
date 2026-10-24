//! The native menu bar, alongside (never replacing) the ribbon: on macOS the WordCraft, File,
//! Edit, View, Insert, Format, Tools, Table, Window and Help menus Word for Mac has at the top of
//! the screen, each item running the same command the ribbon does.
//!
//! The model and routing here are platform-free; the desktop app supplies the AppKit backend
//! (`apps/wordcraft/src/mac_menu.rs`, through `muda`). Adapted from storytold/wordcraft#985 by
//! YTwsy, itself based on PhotoCraft's native-menu design at
//! 67e4b8a499b55ef728b1da700005382c45e4677d; its key-equivalent ordering and modal/clipboard
//! fixes are kept here. See NOTICE.
//!
//! The menu is one for the process while the app has a window per document (`windows::Host`):
//! it shows the state of the window in use, its items run in that window, and the Window menu's
//! list of documents is AppKit's own (the host registers the menu as the windows menu).

use std::hash::{DefaultHasher, Hash, Hasher};

use egui::{Context, Key, Modifiers};
use serde_json::{Value, json};
use wordcraft_layout::ViewMode;

use crate::{WordApp, i18n, widgets, windows::WindowRequest};

/// Row ids of the app menu's own entries (not commands of the registry).
pub const QUIT: &str = "app.quit";
pub const HIDE: &str = "app.hide";
pub const HIDE_OTHERS: &str = "app.hideOthers";
pub const SHOW_ALL: &str = "app.showAll";
pub const MINIMIZE: &str = "window.minimize";

#[derive(Clone, Debug, PartialEq)]
pub struct MenuBar {
    pub menus: Vec<Menu>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Menu {
    pub title: String,
    pub role: Role,
    pub children: Vec<Node>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Role {
    App,
    Normal,
    Window,
    Help,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Node {
    Item(Item),
    Separator,
    Submenu { label: String, children: Vec<Node> },
    Standard(Standard),
}

/// Items AppKit provides itself.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Standard {
    Services,
    Zoom,
    BringAllToFront,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Item {
    /// Stable row id, distinct from the command (Quit and Close both use file.close).
    pub id: String,
    pub command: String,
    pub params: Value,
    pub label: String,
    pub shortcut: Option<Chord>,
    pub enabled: bool,
    pub checked: Option<bool>,
    /// An asynchronous system file dialog owns editing; use the AppKit responder chain.
    pub native_text: bool,
}

impl MenuBar {
    pub fn items(&self) -> Vec<&Item> {
        fn walk<'a>(nodes: &'a [Node], out: &mut Vec<&'a Item>) {
            for node in nodes {
                match node {
                    Node::Item(it) => out.push(it),
                    Node::Submenu { children, .. } => walk(children, out),
                    _ => {}
                }
            }
        }
        let mut out = Vec::new();
        for menu in &self.menus {
            walk(&menu.children, &mut out);
        }
        out
    }

    pub fn find(&self, id: &str) -> Option<&Item> {
        self.items().into_iter().find(|it| it.id == id)
    }
}

/// The native menu may take command/control shortcuts and function keys, never ordinary
/// typing (including Option characters). `Mod` is Command here, on every test platform.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Chord {
    pub cmd: bool,
    pub ctrl: bool,
    pub alt: bool,
    pub shift: bool,
    pub key: String,
}

impl Chord {
    pub fn parse(s: &str) -> Option<Self> {
        let s = wordcraft_engine::keymap::normalize(s)?;
        let (mods, key) = if let Some(mods) = s.strip_suffix("++") { (mods, "+") } else { s.rsplit_once('+').unwrap_or(("", &s)) };
        let parts: Vec<_> = mods.split('+').collect();
        let chord = Self {
            cmd: parts.contains(&"Mod"),
            ctrl: parts.contains(&"Ctrl"),
            alt: parts.contains(&"Alt"),
            shift: parts.contains(&"Shift"),
            key: key.into(),
        };
        chord.key_event()?;
        Some(chord)
    }

    /// Only keys the shared keyboard dispatcher can route are advertised as key equivalents: a
    /// literal `+` is normalized there to `=`, say, so it must not acquire a misleading native
    /// equivalent. The original keymap remains untouched.
    pub fn native_ok(&self) -> bool {
        let Some(egui::Event::Key { key, .. }) = self.key_event() else { return false };
        crate::keys::key_name(key) == Some(self.key.as_str())
            && (self.cmd || self.ctrl || self.key.strip_prefix('F').and_then(|s| s.parse::<u8>().ok()).is_some_and(|n| (1..=24).contains(&n)))
    }

    pub fn modifiers(&self) -> Modifiers {
        Modifiers { command: self.cmd, mac_cmd: self.cmd, ctrl: self.ctrl, alt: self.alt, shift: self.shift }
    }

    pub fn key_event(&self) -> Option<egui::Event> {
        let key = match self.key.as_str() {
            "+" => Key::Plus,
            "=" => Key::Equals,
            "-" => Key::Minus,
            "[" => Key::OpenBracket,
            "]" => Key::CloseBracket,
            "\\" => Key::Backslash,
            "/" => Key::Slash,
            "'" => Key::Quote,
            ";" => Key::Semicolon,
            "," => Key::Comma,
            "." => Key::Period,
            "Left" => Key::ArrowLeft,
            "Right" => Key::ArrowRight,
            "Up" => Key::ArrowUp,
            "Down" => Key::ArrowDown,
            k => Key::from_name(k)?,
        };
        Some(egui::Event::Key { key, physical_key: Some(key), pressed: true, repeat: false, modifiers: self.modifiers() })
    }
}

fn text_edit_focused(ctx: &Context) -> bool {
    ctx.text_edit_focused()
}

fn editing(id: &str) -> bool {
    matches!(id, "edit.undo" | "edit.redo" | "edit.cut" | "edit.copy" | "edit.paste" | "select.all")
}

fn shows_document(id: &str) -> bool {
    matches!(id, "file.new" | "file.open" | "select.all" | "review.newComment")
        || ["edit.", "insert.", "format.", "para.", "view.", "table."].iter().any(|prefix| id.starts_with(prefix))
}

fn modal(app: &WordApp) -> bool {
    app.dialog.is_some() || app.colors.is_some() || app.file_dialog_open()
}

/// A native key-down can share a frame with the command that opened a dialog: with the menu bar
/// installed, the keyboard dispatcher checks again during dispatch, after the earlier command has
/// actually run.
pub(crate) fn blocks_document(app: &WordApp) -> bool {
    app.menu_bar_installed && modal(app)
}

fn allowed(app: &WordApp, ctx: &Context, id: &str) -> bool {
    if matches!(id, HIDE | HIDE_OTHERS | SHOW_ALL | MINIMIZE | QUIT) {
        return !modal(app) || id != QUIT;
    }
    if app.file_dialog.is_some() {
        return false;
    }
    if editing(id) && text_edit_focused(ctx) {
        return true;
    }
    if modal(app) {
        return false;
    }
    if app.session.view.read_mode
        && (matches!(id, "edit.undo" | "edit.redo" | "edit.cut" | "edit.paste" | "edit.pasteSpecial" | "review.newComment")
            || id.starts_with("insert.")
            || id.starts_with("format.")
            || id.starts_with("para.")
            || id.starts_with("table."))
    {
        return false;
    }
    match id {
        "edit.undo" => app.session.can_undo(),
        "edit.redo" => app.session.can_redo(),
        _ => widgets::enabled(app, id),
    }
}

/// A system shortcut is used only if the user's effective keymap leaves it free. WordCraft's
/// existing Command-H (Replace) and Command-M (Indent) keep working; Hide/Minimize try Control
/// as well. Custom assignments are checked across the whole registry, not just visible rows.
fn free_shortcut(app: &WordApp, candidates: &[&str]) -> Option<Chord> {
    candidates.iter().find(|key| app.session.keymap.resolve(&app.session.registry, key).is_none()).and_then(|s| Chord::parse(s))
}

/// The menu bar as Word for Mac lays it out: WordCraft, File, Edit, View, Insert, Format, Tools,
/// Table, Window, Help. Labels are the commands' own, translated; shortcuts the effective keymap's;
/// checkmarks read the state the ribbon reads.
pub fn menu_bar(app: &WordApp, ctx: &Context) -> MenuBar {
    let lang = i18n::Lang::from_pref(&app.ui.language);
    let tr = |s: &str| i18n::tr(lang, s).to_string();
    let named = |s: &str| i18n::fmt(i18n::tr(lang, s), &[("app", "WordCraft")]);
    let item = |id: &str| {
        let label = app.session.registry.get(id).map(|s| s.label).unwrap_or(id);
        let shortcut = app
            .session
            .keymap
            .keys_for(&app.session.registry, id)
            .iter()
            .filter(|key| app.session.keymap.resolve(&app.session.registry, key).is_some_and(|s| s.id == id))
            .filter_map(|key| Chord::parse(key))
            .find(Chord::native_ok);
        let native_text = app.file_dialog.is_some() && editing(id);
        Item {
            id: id.into(),
            command: id.into(),
            params: json!({}),
            label: tr(label),
            shortcut,
            enabled: native_text || allowed(app, ctx, id),
            checked: None,
            native_text,
        }
    };
    let command = |id: &str| Node::Item(item(id));
    let prompt = |id: &str| {
        let mut it = item(id);
        it.label.push('…');
        Node::Item(it)
    };
    let labeled = |id: &str, label: &str| {
        let mut it = item(id);
        it.label = tr(label);
        Node::Item(it)
    };
    let dialog = |id: &str, label: &str, name: &str| {
        let mut it = item("ui.dialog");
        it.id = id.into();
        it.label = tr(label);
        it.params = json!({"name": name});
        Node::Item(it)
    };
    let checked = |id: &str, on: bool| {
        let mut it = item(id);
        it.checked = Some(on);
        Node::Item(it)
    };
    let system = |id: &str, label: String, candidates: &[&str]| {
        let mut it = item(id);
        it.label = label;
        it.shortcut = free_shortcut(app, candidates);
        Node::Item(it)
    };
    let submenu = |label: &str, children: Vec<Node>| Node::Submenu { label: tr(label), children };
    // Quit asks about unsaved changes as the window's close button does (every window, with a
    // window host) and never terminates through AppKit's own Quit, which would ask nothing.
    let mut quit = item("file.close");
    quit.id = QUIT.into();
    quit.label = named("Quit {app}");
    quit.params = json!({"quit": true});
    quit.shortcut = free_shortcut(app, &["Mod+Q"]);
    quit.enabled = !modal(app);
    let mut settings = item("file.options");
    settings.label = tr("Settings…");
    settings.shortcut = settings.shortcut.or_else(|| free_shortcut(app, &["Mod+,"]));
    let mut about = dialog("app.about", "About", "about");
    if let Node::Item(it) = &mut about {
        it.label = named("About {app}");
    }

    let mut files = vec![command("file.new"), prompt("file.newFromTemplate"), prompt("file.open")];
    let recent = app
        .ui
        .recent
        .iter()
        .take(crate::MAX_RECENT)
        .map(|path| {
            let mut it = item("file.open");
            it.id = format!("recent:{path}");
            it.label = path.clone();
            it.params = json!({"path": path});
            it.shortcut = None;
            Node::Item(it)
        })
        .collect();
    files.push(Node::Submenu { label: tr("Open Recent"), children: recent });
    files.extend([
        Node::Separator,
        command("file.close"),
        command("file.save"),
        prompt("file.saveAs"),
        prompt("file.saveTemplate"),
        Node::Separator,
        prompt("file.exportPdf"),
        prompt("file.exportPng"),
        command("file.share"),
        Node::Separator,
        command("file.properties"),
        prompt("file.print"),
    ]);

    // Read the same selection state as the ribbon (including mixed formatting and RTL
    // paragraph alignment), so a menu checkmark never becomes a second source of truth.
    let formatting = wordcraft_engine::cmd::format::state(&app.session);
    let flag = |key: &str| formatting.get(key).and_then(Value::as_bool).unwrap_or(false);
    let paragraph = app.session.doc.para_at(&app.session.sel.focus).map(|p| app.session.doc.styles.resolve_para(&p.props));
    let align = paragraph.as_ref().map(|p| p.align.visual(p.bidi)).unwrap_or_default();
    let lists = wordcraft_engine::cmd::para::list_buttons(&app.session.doc, paragraph.as_ref().and_then(|p| p.numbering));
    let mut table = dialog("insert.tableDialog", "Insert Table…", "insertTable");
    if let Node::Item(it) = &mut table {
        it.enabled = allowed(app, ctx, "insert.table");
    }
    let v = &app.session.view;
    let menus = vec![
        Menu {
            title: "WordCraft".into(),
            role: Role::App,
            children: vec![
                about,
                Node::Separator,
                Node::Item(settings),
                Node::Separator,
                Node::Standard(Standard::Services),
                Node::Separator,
                system(HIDE, named("Hide {app}"), &["Mod+H", "Mod+Ctrl+H"]),
                system(HIDE_OTHERS, tr("Hide Others"), &["Mod+Alt+H"]),
                system(SHOW_ALL, tr("Show All"), &[]),
                Node::Separator,
                Node::Item(quit),
            ],
        },
        Menu { title: tr("File"), role: Role::Normal, children: files },
        Menu {
            title: tr("Edit"),
            role: Role::Normal,
            children: vec![
                command("edit.undo"),
                command("edit.redo"),
                Node::Separator,
                command("edit.cut"),
                command("edit.copy"),
                command("edit.paste"),
                prompt("edit.pasteSpecial"),
                command("select.all"),
                Node::Separator,
                command("edit.find"),
                command("edit.findNext"),
                command("edit.findPrevious"),
                prompt("edit.findReplace"),
                prompt("edit.goto"),
            ],
        },
        Menu {
            title: tr("View"),
            role: Role::Normal,
            children: vec![
                checked("view.printLayout", v.mode == ViewMode::Print && !v.read_mode),
                checked("view.webLayout", v.mode == ViewMode::Web),
                checked("view.draft", v.mode == ViewMode::Draft),
                checked("view.outline", v.mode == ViewMode::Outline),
                checked("view.readMode", v.read_mode),
                checked("view.focus", v.focus_mode),
                Node::Separator,
                checked("view.ruler", v.ruler),
                checked("view.gridlines", v.gridlines),
                checked("view.marks", v.marks),
                checked("view.navigationPane", v.nav_pane),
                checked("view.stylesPane", v.styles_pane),
                checked("view.commentsPane", v.comments_pane),
                Node::Separator,
                dialog("view.zoomDialog", "Zoom…", "zoom"),
                command("view.zoomIn"),
                command("view.zoomOut"),
                command("view.zoom100"),
                command("view.pageWidth"),
            ],
        },
        Menu {
            title: tr("Insert"),
            role: Role::Normal,
            children: vec![
                command("insert.pageBreak"),
                command("insert.blankPage"),
                command("insert.coverPage"),
                Node::Separator,
                table,
                prompt("insert.picture"),
                prompt("insert.chart"),
                prompt("insert.icon"),
                command("insert.textBox"),
                Node::Separator,
                command("insert.editHeader"),
                command("insert.editFooter"),
                command("insert.pageNumber"),
                Node::Separator,
                command("review.newComment"),
                command("insert.equation"),
                prompt("insert.symbol"),
                prompt("insert.dateTime"),
                prompt("insert.crossReference"),
                prompt("insert.link"),
                prompt("insert.bookmark"),
            ],
        },
        Menu {
            title: tr("Format"),
            role: Role::Normal,
            children: vec![
                labeled("format.fontDialog", "Font…"),
                labeled("para.dialog", "Paragraph…"),
                Node::Separator,
                checked("format.bold", flag("bold")),
                checked("format.italic", flag("italic")),
                checked("format.underline", flag("underline")),
                checked("format.strikethrough", flag("strike")),
                checked("format.subscript", flag("subscript")),
                checked("format.superscript", flag("superscript")),
                Node::Separator,
                command("format.growFont"),
                command("format.shrinkFont"),
                command("format.clear"),
                Node::Separator,
                submenu(
                    "Alignment",
                    vec![
                        checked("para.alignLeft", align == wordcraft_doc::Align::Left),
                        checked("para.alignCenter", align == wordcraft_doc::Align::Center),
                        checked("para.alignRight", align == wordcraft_doc::Align::Right),
                        checked("para.justify", align == wordcraft_doc::Align::Justify),
                    ],
                ),
                checked("para.bullets", lists.bullets),
                checked("para.numbering", lists.numbering),
                command("para.indent"),
                command("para.outdent"),
            ],
        },
        Menu {
            title: tr("Tools"),
            role: Role::Normal,
            children: vec![
                prompt("review.spelling"),
                prompt("review.thesaurus"),
                prompt("layout.hyphenation"),
                prompt("review.language"),
                prompt("review.wordCount"),
                dialog("tools.autoCorrectDialog", "AutoCorrect Options…", "autoCorrect"),
                Node::Separator,
                checked("review.trackChanges", app.session.doc.settings.track_changes),
                dialog("tools.compareDialog", "Compare Documents…", "compare"),
                dialog("tools.combineDialog", "Combine Documents…", "combine"),
                command("review.restrict"),
                Node::Separator,
                command("tools.macros"),
                prompt("tools.templates"),
                prompt("tools.customizeKeyboard"),
                prompt("tools.customizeRibbon"),
            ],
        },
        Menu {
            title: tr("Table"),
            role: Role::Normal,
            children: vec![
                submenu(
                    "Insert",
                    vec![
                        dialog("table.insertDialog", "Table…", "insertTable"),
                        command("table.insertRowAbove"),
                        command("table.insertRowBelow"),
                        command("table.insertColumnLeft"),
                        command("table.insertColumnRight"),
                    ],
                ),
                submenu(
                    "Delete",
                    vec![command("table.deleteTable"), command("table.deleteRow"), command("table.deleteColumn"), command("table.deleteCells")],
                ),
                submenu(
                    "Select",
                    vec![command("table.selectTable"), command("table.selectRow"), command("table.selectColumn"), command("table.selectCell")],
                ),
                Node::Separator,
                command("table.merge"),
                prompt("table.split"),
                command("table.splitTable"),
                Node::Separator,
                command("table.autofit"),
                submenu("Convert", vec![prompt("table.fromText"), prompt("table.toText")]),
                prompt("table.sort"),
                prompt("table.formula"),
                Node::Separator,
                prompt("table.properties"),
            ],
        },
        // The Window menu as Word for Mac has it (observed 2026-10-24): Minimize, Zoom; New
        // Window, Arrange All, Split; Bring All to Front. View Side by Side, Synchronous Scrolling
        // and Reset Window Position stay on the ribbon alone. The rest is AppKit's: the host
        // registers this menu as the windows menu, and macOS adds Minimize All, Zoom All, Fill,
        // Center, Move & Resize, Full Screen Tile and the list of open documents, the one in
        // front checked.
        Menu {
            title: tr("Window"),
            role: Role::Window,
            children: vec![
                system(MINIMIZE, tr("Minimize"), &["Mod+M", "Mod+Ctrl+M"]),
                Node::Standard(Standard::Zoom),
                Node::Separator,
                command("view.newWindow"),
                command("view.arrangeAll"),
                command("view.split"),
                Node::Separator,
                Node::Standard(Standard::BringAllToFront),
            ],
        },
        Menu { title: tr("Help"), role: Role::Help, children: vec![dialog("help.commands", "Commands", "commands"), command("file.accessibility")] },
    ];
    MenuBar { menus }
}

/// Only the shape, labels of submenus, and key equivalents require a rebuild. A checkmark,
/// enabled state or item label changes in place. Check items never change kind.
pub fn structure_key(bar: &MenuBar) -> u64 {
    fn walk(nodes: &[Node], h: &mut impl Hasher) {
        for node in nodes {
            std::mem::discriminant(node).hash(h);
            match node {
                Node::Item(it) => (&it.id, &it.shortcut, it.checked.is_some(), it.native_text).hash(h),
                Node::Submenu { label, children } => {
                    label.hash(h);
                    walk(children, h);
                }
                Node::Standard(s) => s.hash(h),
                Node::Separator => {}
            }
        }
    }
    let mut h = DefaultHasher::new();
    for m in &bar.menus {
        (&m.title, m.role).hash(&mut h);
        walk(&m.children, &mut h);
    }
    h.finish()
}

#[derive(Clone, Debug)]
pub enum Event {
    Click(String),
    Key(Chord),
}

/// The platform's menu: shown from the model, reporting what was chosen.
pub trait Backend {
    fn sync(&mut self, bar: &MenuBar);
    fn drain(&mut self) -> Vec<Event>;
    fn system_action(&mut self, id: &str);
}

/// The menu bar: its backend, the events it reported that no frame has taken yet, and the
/// clicks waiting to run.
pub struct NativeMenu {
    backend: Box<dyn Backend>,
    ctx: Context,
    pending: Vec<Event>,
    clicks: Vec<String>,
}

impl NativeMenu {
    pub fn new(backend: Box<dyn Backend>, ctx: Context) -> Self {
        Self { backend, ctx, pending: Vec::new(), clicks: Vec::new() }
    }

    /// Take the backend's events into the queue; true when something waits to be handled by
    /// the window in use (its frame takes them in [`raw_input`]).
    pub fn pull(&mut self) -> bool {
        self.pending.extend(self.backend.drain());
        !self.pending.is_empty() || !self.clicks.is_empty()
    }
}

/// A menu click in an egui text field is an egui editing event, not a document command.
fn text_event(app: &WordApp, id: &str) -> Option<egui::Event> {
    match id {
        "edit.copy" => Some(egui::Event::Copy),
        "edit.cut" => Some(egui::Event::Cut),
        "edit.paste" => app.services.clipboard_text.as_ref().and_then(|f| f()).map(egui::Event::Paste),
        "select.all" => Chord::parse("Mod+A")?.key_event(),
        "edit.undo" => Chord::parse("Mod+Z")?.key_event(),
        "edit.redo" => Chord::parse("Mod+Shift+Z")?.key_event(),
        _ => None,
    }
}

/// Put a swallowed key-down before its real key-up (even when both arrived in this frame).
/// Never invent a release: winit already delivers it, including for Command-V.
fn insert_press(raw: &mut egui::RawInput, event: egui::Event, key: Key) {
    let at = raw.events.iter().position(|e| matches!(e, egui::Event::Key { key: k, pressed: false, .. } if *k == key)).unwrap_or(raw.events.len());
    raw.events.insert(at, event);
}

/// Before the window in use begins its frame: the menu's key equivalents become the key presses
/// egui would have seen, clicks queue for [`run`].
pub fn raw_input(menu: &mut NativeMenu, app: &mut WordApp, raw: &mut egui::RawInput) {
    menu.pull();
    let ctx = menu.ctx.clone();
    for event in std::mem::take(&mut menu.pending) {
        match event {
            Event::Click(id) => {
                let bar = menu_bar(app, &ctx);
                if let Some(it) = bar.find(&id).filter(|it| it.enabled && !it.native_text) {
                    if editing(&it.command) && text_edit_focused(&ctx) || it.command == "edit.paste" && app.session.math.is_some() {
                        if let Some(event) = text_event(app, &it.command) {
                            raw.events.push(event);
                        }
                    } else {
                        menu.clicks.push(id);
                    }
                }
            }
            Event::Key(chord) => {
                let Some(press @ egui::Event::Key { key, .. }) = chord.key_event() else { continue };
                // The real release must not fall back to a document image paste after a
                // text-field paste or a key blocked by a modal dialog.
                if key == Key::V && chord.cmd {
                    app.canvas.pasted = true;
                }
                // Text fields retain their standard editing keys, even if the document keymap
                // reassigns them. egui needs Copy/Cut/Paste events, not just C/X/V key presses.
                let text_id = match (chord.cmd, chord.ctrl, chord.alt, chord.shift, key) {
                    (true, false, false, false, Key::C) => Some("edit.copy"),
                    (true, false, false, false, Key::X) => Some("edit.cut"),
                    (true, false, false, false, Key::V) => Some("edit.paste"),
                    (true, false, false, false, Key::A) => Some("select.all"),
                    (true, false, false, false, Key::Z) => Some("edit.undo"),
                    (true, false, false, true, Key::Z) => Some("edit.redo"),
                    _ => None,
                };
                if app.file_dialog.is_none()
                    && text_edit_focused(&ctx)
                    && let Some(id) = text_id
                {
                    if let Some(event) = text_event(app, id) {
                        insert_press(raw, event, key);
                    }
                    continue;
                }
                let combo = crate::keys::combo(chord.modifiers(), &chord.key, true);
                let command = app.session.keymap.resolve(&app.session.registry, &combo).map(|s| s.id);
                if let Some(id) = command {
                    if !allowed(app, &ctx, id) {
                        continue;
                    }
                    if text_edit_focused(&ctx) && editing(id) {
                        if let Some(event) = text_event(app, id) {
                            insert_press(raw, event, key);
                        }
                        continue;
                    }
                    if id == "edit.paste" && app.session.math.is_some() && text_event(app, id).is_none() {
                        continue;
                    }
                    // Run document clipboard actions through their UI command, regardless of
                    // canvas/button focus or a custom shortcut's modifiers. This also keeps
                    // formatted text and picture pastes on the same path as the ribbon.
                    if (matches!(id, "edit.copy" | "edit.cut" | "edit.paste") && app.session.math.is_none())
                        || (app.ui.backstage && menu_bar(app, &ctx).find(id).is_some())
                    {
                        menu.clicks.push(id.into());
                        continue;
                    }
                    let event = match id {
                        "edit.copy" | "edit.cut" | "edit.paste" => text_event(app, id).unwrap_or(press),
                        _ => press,
                    };
                    insert_press(raw, event, key);
                } else {
                    // Settings/Quit/system actions have native-only fallback shortcuts. Match
                    // the modifiers actually pressed, not the row AppKit happened to choose.
                    let bar = menu_bar(app, &ctx);
                    if let Some(it) = bar.items().into_iter().find(|it| it.enabled && it.shortcut.as_ref() == Some(&chord)) {
                        menu.clicks.push(it.id.clone());
                    } else if !modal(app) {
                        insert_press(raw, press, key);
                    }
                }
            }
        }
    }
}

/// Called after egui begins the frame, so clipboard output and viewport requests survive: the
/// queued clicks run in the window in use.
pub fn run(menu: &mut NativeMenu, app: &mut WordApp, ctx: &Context) {
    for id in std::mem::take(&mut menu.clicks) {
        // Recheck every queued click: a prior click can have opened a modal in this very frame.
        let bar = menu_bar(app, ctx);
        let Some(it) = bar.find(&id).filter(|it| it.enabled && !it.native_text) else { continue };
        match id.as_str() {
            HIDE | HIDE_OTHERS | SHOW_ALL => menu.backend.system_action(&id),
            MINIMIZE => ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(true)),
            // With a window host every window closes in turn, each asking about its changes;
            // alone, the window asks and closes, as its close button does.
            QUIT if app.session.windows.host => app.window_requests.push(WindowRequest::Quit),
            _ => {
                // Like choosing a ribbon/backstage action, return to the page when editing
                // it. Otherwise New or Insert from the start screen appears to do nothing.
                if shows_document(&id) || shows_document(&it.command) {
                    app.ui.backstage = false;
                    app.canvas.want_focus = true;
                }
                match app.run(&it.command, it.params.clone()) {
                    Ok(value) => {
                        if matches!(it.command.as_str(), "edit.copy" | "edit.cut")
                            && let Some(text) = value.get("text").and_then(Value::as_str).filter(|s| !s.is_empty())
                        {
                            ctx.copy_text(text.to_owned());
                        }
                    }
                    Err(error) => app.status(error),
                }
            }
        }
    }
}

/// After the frame of the window in use: the menu shows that window's state.
pub fn sync(menu: &mut NativeMenu, app: &WordApp, ctx: &Context) {
    let bar = menu_bar(app, ctx);
    menu.backend.sync(&bar);
}

#[cfg(test)]
#[path = "native_menu_tests.rs"]
mod tests;

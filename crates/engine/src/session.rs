//! The editing session: document, selection, undo/redo, layout, view state.

use std::sync::{Arc, OnceLock};

/// The system locale (BCP 47 tag) the shell found at startup, for Numeral › System.
static SYSTEM_LOCALE: OnceLock<String> = OnceLock::new();

/// Tell the engine the system locale (the shell calls this once at startup; layout never reads
/// the environment). Headless sessions without it show Western digits for Numeral › System.
pub fn set_system_locale(tag: &str) {
    let _ = SYSTEM_LOCALE.set(tag.to_string());
}

/// The system locale [`set_system_locale`] recorded, if the shell set one.
pub(crate) fn system_locale() -> Option<&'static str> {
    SYSTEM_LOCALE.get().map(String::as_str)
}

use serde::{Deserialize, Serialize};
use serde_json::Value;
use wordcraft_doc::edit::Fragment;
use wordcraft_doc::props::CharProps;
use wordcraft_doc::{Document, PartKind, Path, Pos, StoryRef, Table};
use wordcraft_layout::{DocLayout, LayoutCache, LayoutOptions, NumeralMode, ViewMode};

use crate::{CmdError, Registry};

/// A selection: anchor (where it started) and focus (where the caret is).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct Selection {
    pub anchor: Pos,
    pub focus: Pos,
}

impl Selection {
    pub fn caret(p: Pos) -> Self {
        Selection { anchor: p.clone(), focus: p }
    }
    pub fn is_collapsed(&self) -> bool {
        self.anchor == self.focus
    }
    /// (start, end) in document order.
    pub fn ordered(&self) -> (Pos, Pos) {
        if self.anchor <= self.focus { (self.anchor.clone(), self.focus.clone()) } else { (self.focus.clone(), self.anchor.clone()) }
    }
}

/// The word being typed, left without a spelling squiggle until the caret leaves it (#1552).
#[derive(Clone, Debug, PartialEq, Eq)]
struct TypedWord {
    /// Where the caret was when the word was typed.
    at: Pos,
    /// The word's byte range in its paragraph.
    start: usize,
    end: usize,
}

/// A column (block) selection: the same x range on every line between two corners.
#[derive(Clone, Debug, PartialEq)]
pub struct ColumnBlock {
    /// The stream selection the block was made with (its corners). The block is current only
    /// while `Session::sel` still equals it, so any ordinary caret move or selection drops it.
    pub sel: Selection,
    /// Page x of the anchor and focus corners (points); the block spans the range between.
    pub anchor_x: f32,
    pub focus_x: f32,
    /// One `(start, end)` per line, in document order, each inside one paragraph.
    pub segments: Vec<(Pos, Pos)>,
    /// Set when the block is whole table cells (Select Cell, Row, Column or Table, or the mouse in
    /// a table's selection areas): one segment per paragraph of the cells.
    pub cells: Option<CellRange>,
}

/// Whole table cells selected: rows `rows.0..=rows.1` and every cell of them that reaches into
/// grid columns `cols.0..=cols.1`.
#[derive(Clone, Debug, PartialEq)]
pub struct CellRange {
    pub story: StoryRef,
    pub table: Path,
    pub rows: (usize, usize),
    pub cols: (usize, usize),
    /// The cells are one run of the text (cells of one row, or whole rows), so the stream
    /// selection between the corners holds just them and commands act on that as usual.
    /// Otherwise (part of a column) text edits and formatting go cell by cell.
    pub stream: bool,
}

impl CellRange {
    /// The selected cells of table `t` (row, cell index), in document order.
    pub fn cells(&self, t: &Table) -> Vec<(usize, usize)> {
        let ((r0, r1), (g0, g1)) = (self.rows, self.cols);
        let mut out = Vec::new();
        for (r, row) in t.rows.iter().enumerate().take(r1.saturating_add(1)).skip(r0) {
            let mut g = row.first_grid();
            for (c, cell) in row.cells.iter().enumerate() {
                let end = g.saturating_add(cell.span());
                if g <= g1 && end > g0 {
                    out.push((r, c));
                }
                g = end;
            }
        }
        out
    }
}

/// A selection Word's table rule widened to whole cells or rows (`cmd::table::extend_selection`),
/// and where it really runs.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Widened {
    /// The selection it became; this is current while `Session::sel` still equals it.
    pub sel: Selection,
    /// Where the selection was really extended from and to: extending it again goes on from here.
    pub ends: Selection,
}

/// View state that commands can change (ribbon View tab, status bar).
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ViewState {
    pub zoom: f32,
    pub mode: ViewMode,
    pub read_mode: bool,
    /// View › Focus (#1678): the document alone on screen, with a slim bar to drop the ribbon down.
    pub focus_mode: bool,
    /// Full Screen View (#1678): the pages alone on screen. Never on together with `focus_mode`.
    #[serde(default)]
    pub full_screen_view: bool,
    pub marks: bool,
    pub ruler: bool,
    /// Crop marks (Word › Preferences › View › Crop marks; 裁ちトンボ): an L at each corner of
    /// the text area, on screen only. `None` is the region's default — on in Japan, as Word for
    /// Mac with Japanese editing shows them; see [`Session::crop_marks`].
    #[serde(default)]
    pub crop_marks: Option<bool>,
    /// Text boundaries (Word › Preferences › View › Text boundaries; 文字列表示範囲枠): a dotted
    /// box around every paragraph's slot in its column, on screen only; off until chosen, as in Word.
    #[serde(default)]
    pub text_boundaries: bool,
    /// View › Show › Gridlines: the drawing grid over the page's text area (on screen only).
    pub gridlines: bool,
    /// Table Layout › View Gridlines: outlines of table cells (on screen only); on by default, as in Word.
    #[serde(default = "on")]
    pub table_gridlines: bool,
    pub nav_pane: bool,
    pub styles_pane: bool,
    /// Style Inspector pane (Home › Styles).
    #[serde(default)]
    pub style_inspector: bool,
    pub comments_pane: bool,
    /// The Clipboard pane (Home › Clipboard): items collected by Copy and Cut.
    #[serde(default)]
    pub clipboard_pane: bool,
    /// SmartArt Design › Text Pane: the selected SmartArt graphic's items as an editable outline.
    #[serde(default)]
    pub smart_art_pane: bool,
    /// The Accessibility pane (Review › Check Accessibility): the checker's findings.
    #[serde(default)]
    pub accessibility_pane: bool,
    /// The Alt Text pane (Picture/Shape Format › Alt Text): the selected object's or table's alt text.
    #[serde(default)]
    pub alt_text_pane: bool,
    /// The Format Shape pane (Shape Fill › More Gradients…, a shape's Format Shape…): the
    /// selected shapes' fill.
    #[serde(default)]
    pub format_shape_pane: bool,
    pub multi_page: bool,
    /// One Page or Page Width was chosen last: keep one page per row when a typed or wheel zoom replaces the fit mode.
    #[serde(default)]
    pub single_column: bool,
    /// Zoom to fit: "pageWidth", "onePage", "multiplePages", or empty.
    pub fit: String,
    /// Web Layout's text width, which the UI keeps at the window's width (#1876); never saved.
    /// Draft and Outline don't use it: they keep a fixed 800 pt column.
    #[serde(skip, default = "default_web_width")]
    pub web_width: f32,
    /// Transient viewport metrics for reading pagination; never saved in a document or preferences.
    #[serde(skip, default = "default_read_width")]
    pub read_width: f32,
    #[serde(skip, default = "default_read_height")]
    pub read_height: f32,
    /// Reader-only layout controls; they are neither document formatting nor saved UI prefs.
    /// `None` chooses responsive column width and keeps the document's line spacing.
    #[serde(skip)]
    pub read_column_width: Option<f32>,
    #[serde(skip, default = "default_read_zoom")]
    pub read_zoom: f32,
    #[serde(skip)]
    pub read_line_spacing: Option<f32>,
    pub dark_mode: bool,
    pub show_markup: bool,
    pub track_changes_pane: bool,
    /// Check spelling and grammar as you type.
    pub proofing: bool,
    /// How digits look on screen (Word › File › Options › Advanced › Numeral).
    #[serde(default)]
    pub numeral: NumeralMode,
    /// Review › Hide Ink: ink strokes aren't shown on screen (they stay in the document).
    #[serde(default)]
    pub hide_ink: bool,
    /// Draw tab: the tool dragging on the page uses, and each pen's colour and thickness.
    #[serde(default)]
    pub draw: crate::cmd::draw::DrawState,
    /// Show field codes instead of their results (`view.fieldCodes`, Alt+F9).
    #[serde(default)]
    pub field_codes: bool,
    /// Print Layout › Hide White Space: pages run together, the top and bottom margins (with
    /// headers and footers) between pages collapse to a thin line. On screen only; the layout
    /// and page count are unchanged.
    #[serde(default)]
    pub hide_white_space: bool,
    /// Outline view: collapsed headings, Show Level, first lines, text formatting.
    #[serde(default)]
    pub outline: crate::cmd::outline::OutlineState,
    /// Developer › Design Mode: every content control shows its frame and title.
    #[serde(default)]
    pub design_mode: bool,
    /// View › Window › Full Screen (F11): the window fills the screen (#980). The front end
    /// carries it to the window.
    #[serde(default)]
    pub full_screen: bool,
}

fn on() -> bool {
    true
}

/// The text column of Draft and Outline, and of Web Layout in a headless session.
const DRAFT_WIDTH: f32 = 800.0;

fn default_web_width() -> f32 {
    DRAFT_WIDTH
}

fn default_read_width() -> f32 {
    300.0
}

fn default_read_height() -> f32 {
    500.0
}

fn default_read_zoom() -> f32 {
    1.0
}

impl Default for ViewState {
    fn default() -> Self {
        ViewState {
            zoom: 1.0,
            mode: ViewMode::Print,
            read_mode: false,
            focus_mode: false,
            full_screen_view: false,
            marks: false,
            ruler: true,
            crop_marks: None,
            text_boundaries: false,
            gridlines: false,
            table_gridlines: true,
            nav_pane: false,
            styles_pane: false,
            style_inspector: false,
            comments_pane: false,
            clipboard_pane: false,
            smart_art_pane: false,
            accessibility_pane: false,
            alt_text_pane: false,
            format_shape_pane: false,
            multi_page: false,
            single_column: false,
            fit: String::new(),
            web_width: default_web_width(),
            read_width: default_read_width(),
            read_height: default_read_height(),
            read_column_width: None,
            read_zoom: default_read_zoom(),
            read_line_spacing: None,
            dark_mode: false,
            show_markup: true,
            track_changes_pane: false,
            proofing: true,
            numeral: NumeralMode::default(),
            hide_ink: false,
            draw: Default::default(),
            field_codes: false,
            hide_white_space: false,
            outline: Default::default(),
            design_mode: false,
            full_screen: false,
        }
    }
}

/// Find/replace state.
/// The document windows the front end has open, one per document, as the desktop app reports
/// them to every window's session before each frame (`wordcraft_ui_egui::windows`). The session
/// owns no window: the View › Window commands check their parameters here and queue a request
/// ([`Session::push_ui_request`]) the front end carries out. Headless sessions, the browser and
/// the CLI have no window host: `host` stays false and those commands are disabled.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Windows {
    /// The front end can open, place and switch windows.
    pub host: bool,
    /// This window's id.
    pub this: u64,
    /// Every open window, in the order Switch Windows lists them (the order they were opened in).
    pub list: Vec<WindowEntry>,
    /// View Side by Side is on, with this window's partner.
    pub side_by_side: Option<u64>,
    /// Synchronous Scrolling is on; only ever while side by side.
    pub sync_scroll: bool,
}

/// One open window: its id and the title it shows (the document's name).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct WindowEntry {
    pub id: u64,
    pub title: String,
    /// The document's file, when it has one: a file already open comes to the front instead of
    /// opening twice.
    pub path: Option<String>,
    /// The document the window shows, when several windows show one (View › New Window): the
    /// host's key for it, the same in each of them; 0 for a document with one window.
    pub document: u64,
}

impl Windows {
    /// How many windows are open; one for a host that reported nothing.
    pub fn count(&self) -> usize {
        self.list.len().max(1)
    }
    /// The other windows, in the Switch Windows order.
    pub fn others(&self) -> impl Iterator<Item = &WindowEntry> {
        self.list.iter().filter(|w| w.id != self.this)
    }
    /// This window's document key ([`WindowEntry::document`]); 0 when it alone shows its document.
    pub fn document_of_this(&self) -> u64 {
        self.list.iter().find(|w| w.id == self.this).map_or(0, |w| w.document)
    }
    /// The other windows showing this window's document (View › New Window): closing this
    /// window then closes a view, not the document.
    pub fn others_on_this_document(&self) -> impl Iterator<Item = &WindowEntry> {
        let document = self.document_of_this();
        self.list.iter().filter(move |w| w.id != self.this && document != 0 && w.document == document)
    }
    /// The window `n` names: an id in the list, or else a 1-based position in it.
    pub fn resolve(&self, n: u64) -> Option<u64> {
        if self.list.iter().any(|w| w.id == n) {
            return Some(n);
        }
        usize::try_from(n).ok().and_then(|i| i.checked_sub(1)).and_then(|i| self.list.get(i)).map(|w| w.id)
    }
}

/// One view of the document (#322): the caret and selection, the View tab's state and what hangs
/// off the caret (the typed word, the equation being edited, the column selection, Go Back's
/// places…). A window, or a split pane, has one; the document, its undo history, its file and its
/// unsaved state are the session's, which several views share by taking turns
/// ([`Session::swap_view`]).
pub struct ViewSlot {
    pub sel: Selection,
    pub view: ViewState,
    pending: Option<CharProps>,
    find: FindState,
    goal_x: Option<f32>,
    page_hint: usize,
    painter: Option<(CharProps, wordcraft_doc::props::ParaProps, bool)>,
    hf_return: Option<Selection>,
    autocap: Option<crate::cmd::tools::AutoCap>,
    typing: Option<TypedWord>,
    math: Option<MathEdit>,
    math_latex: bool,
    math_normal_text: bool,
    also_selected: Vec<Pos>,
    column: Option<ColumnBlock>,
    column_mode: bool,
    widened: Option<Widened>,
    edit_marks: Vec<Pos>,
    go_back_step: usize,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct FindState {
    pub query: String,
    pub replace: String,
    pub match_case: bool,
    pub whole_word: bool,
    pub regex: bool,
    /// Word's "Use wildcards" (#543).
    pub wildcards: bool,
    /// Take Find what literally: no `^` codes (`special: false`).
    pub literal: bool,
    pub match_prefix: bool,
    pub match_suffix: bool,
    pub ignore_punct: bool,
    pub ignore_space: bool,
    /// Word's Match half/full-width forms (`match byte`).
    pub match_byte: bool,
    /// Word's あいまい検索 (`match fuzzy`); unset = Word's default (on when the document's East
    /// Asian language is Japanese).
    pub fuzzy: Option<bool>,
    /// Matches from the last search: (start, end).
    #[serde(skip)]
    pub results: Vec<(Pos, Pos)>,
    pub current: usize,
    /// Reading Highlight: mark every match of `query` in the body.
    pub highlight: bool,
    /// Highlighted matches, for the document revision they were found in.
    #[serde(skip)]
    pub highlights: Option<(u64, Vec<(Pos, Pos)>)>,
}

#[derive(Clone)]
struct Undo {
    label: String,
    doc: Document,
    sel: Selection,
    /// Formatting waiting at the caret (Bold with nothing selected), so Undo takes it back too.
    pending: Option<CharProps>,
    /// The column block or whole cells the selection was (undoing brings the selection back as
    /// it was).
    column: Option<ColumnBlock>,
}

/// What [`Session::restore`] needs to take commands back exactly: the document, the selection,
/// the undo and redo stacks (also the oldest undo steps the undo limit pushes out meanwhile), the
/// open typing group, a pending [`Session::join_next_undo`] and the dirty flag. See
/// [`Session::edit_snapshot`].
pub struct EditSnapshot {
    doc: Document,
    sel: Selection,
    history_len: usize,
    /// `Session::undo_evicted` when the snapshot was taken.
    evicted: u64,
    /// The oldest undo steps, kept only when the stack is near its limit (the commands that
    /// follow may push them out); at most [`SNAPSHOT_HEAD`].
    head: Vec<Arc<Undo>>,
    redo: Vec<Arc<Undo>>,
    typing_open: bool,
    join_next: bool,
    dirty: bool,
}

/// Undo steps that the commands run between [`Session::edit_snapshot`] and [`Session::restore`]
/// may push out of a full stack and still come back.
const SNAPSHOT_HEAD: usize = 4;

impl EditSnapshot {
    /// The document when the snapshot was taken.
    pub fn doc(&self) -> &Document {
        &self.doc
    }
    /// The dirty flag then.
    pub fn dirty(&self) -> bool {
        self.dirty
    }
    /// The number of undo steps then.
    pub fn undo_depth(&self) -> usize {
        self.history_len
    }
}

/// An editing session.
pub struct Session {
    pub doc: Document,
    pub sel: Selection,
    pub view: ViewState,
    /// Formatting picked with a collapsed caret, applied to the next typed text.
    pub pending: Option<CharProps>,
    pub path: Option<std::path::PathBuf>,
    /// Name given to a document not saved yet (File › Info › Rename): what Save suggests. A file
    /// name, not document metadata, so it is not part of the document or its undo history.
    pub file_name: Option<String>,
    pub dirty: bool,
    /// Where the custom dictionary is kept on disk, if the host wired one up. Only the desktop
    /// app does: headless sessions (MCP, `wordcraft-cli run`), tests and the web build leave
    /// `None`, so an agent's words stay in memory and never change the person's dictionary.
    /// Add to Dictionary and File › Options › Proofing save through it (#40).
    pub user_dict_path: Option<std::path::PathBuf>,
    pub clipboard: Option<Fragment>,
    /// A tracked cut the next paste can turn into a move (see `cmd::moves`).
    pub pending_move: Option<crate::cmd::moves::PendingMove>,
    /// Plain text mirror of the clipboard (for the system clipboard).
    pub clipboard_text: String,
    /// HTML for the system clipboard beside [`Self::clipboard_text`]: the copy's formatting for
    /// programs that read HTML, equations as MathML (#816); empty when nothing was copied.
    pub clipboard_html: String,
    /// Items collected by Copy and Cut this session, for the Clipboard pane.
    pub clip_history: crate::cmd::edit::ClipHistory,
    pub find: FindState,
    /// Page x the caret tries to keep on Up/Down.
    pub goal_x: Option<f32>,
    /// Page the caret was last on (headers/footers repeat on many pages).
    pub page_hint: usize,
    pub registry: Arc<Registry>,
    /// Author name for comments and tracked changes.
    pub author: String,
    /// Initials for comments (File › Options › General); empty = the name's initials.
    pub initials: String,
    /// Format painter: copied formatting waiting to be applied (and whether it stays on).
    pub painter: Option<(CharProps, wordcraft_doc::props::ParaProps, bool)>,
    /// Last message for the status bar / agents.
    pub status: String,
    /// Headers/footers `insert.createHeader`/`insert.createFooter` made for a page that had none
    /// (#960): `insert.closeHeader` takes those still empty away again, as Word does, and saving
    /// leaves them out. Kept after that, so one an undo brings back goes again on the next close.
    pub new_header_parts: Vec<u32>,
    /// The body selection a command left for a header or footer: `insert.closeHeader` puts it back.
    pub hf_return: Option<Selection>,
    /// Undo and redo steps. Shared so `run` can snapshot both stacks cheaply (a pointer per
    /// step) and put them back exactly when a command fails.
    history: Vec<Arc<Undo>>,
    /// Undo steps dropped so far because of the undo limit.
    undo_evicted: u64,
    redo: Vec<Arc<Undo>>,
    /// Typing is coalesced into one undo step until something else happens.
    typing_open: bool,
    /// The next mutating command joins the previous undo step (later frames of a drag).
    join_next: bool,
    /// The input method's composition shown inline (see `composition.rs`).
    pub(crate) composition: Option<crate::composition::Composition>,
    /// The newest automatic capitalisation, for Word's "Automatically add words to list" (#1857).
    pub(crate) autocap: Option<crate::cmd::tools::AutoCap>,
    pub(crate) rev: u64,
    /// What Restrict Editing's outright refusals depend on, for greying out commands
    /// ([`Session::why_disabled`]): computed once per document, revision and author instead of
    /// for every button on every frame.
    protect_facts: std::sync::Mutex<Option<((u64, u64, String), crate::cmd::protect::Facts)>>,
    /// Which document this is: changes when [`Session::set_document`] replaces it (open, new,
    /// mail merge, recover), never on an edit.
    document_id: u64,
    cache: LayoutCache,
    /// (revision, width, read height, view, read line spacing, layout, proofing, outline,
    /// (deletions left out, deletions marked Hidden, body deletions in balloons), numeral display, the word being typed).
    #[allow(clippy::type_complexity)]
    layout: Option<(
        u64,
        f32,
        f32,
        ViewMode,
        Option<u32>,
        Arc<DocLayout>,
        bool,
        Option<wordcraft_layout::outline::OutlineView>,
        (bool, wordcraft_layout::HideDeleted, bool),
        NumeralMode,
        Option<wordcraft_layout::TypingWord>,
    )>,
    /// The word being typed, left without a spelling squiggle ([`TypedWord`]).
    typing: Option<TypedWord>,
    /// Picture edits: edited media key → original media key (Reset Picture).
    pub originals: std::collections::HashMap<String, String>,
    /// Picture effects, styles and borders: drawn media key → what was drawn on which bitmap,
    /// so the next one replaces them instead of drawing over them (#2052).
    pub picture_looks: std::collections::HashMap<String, crate::cmd::objects::PictureLook>,
    /// Last mutating command (Repeat).
    pub last_command: Option<(String, Value)>,
    /// Macro being recorded: (name, steps).
    pub recording: Option<(String, Vec<(String, Value)>)>,
    pub macros: std::collections::BTreeMap<String, Vec<(String, Value)>>,
    /// Saved versions: (label, date, document).
    pub versions: Vec<(String, String, Document)>,
    /// Quick Parts / AutoText entries.
    pub building_blocks: std::collections::BTreeMap<String, Fragment>,
    pub autosave: bool,
    /// Citation style: APA, MLA, Chicago, IEEE.
    pub bib_style: String,
    /// Mail merge data source and preview.
    pub merge: crate::cmd::mailings::MergeState,
    /// Requests from commands to the UI (open a dialog, scroll…), drained by the front end.
    pub ui_requests: Vec<Value>,
    /// Read Aloud player (Review › Speech).
    pub read_aloud: crate::speech::ReadAloud,
    /// Preferences the front end saves between runs.
    pub prefs: Prefs,
    /// The system region (#838): the paper a new blank document starts on, the default
    /// measurement unit and the decimal separator. The front end sets it from the platform (the
    /// browser's locale on the web); headless sessions keep `en-US`. A region the user chose
    /// ([`Prefs::region`]) wins: read it with [`Session::region`].
    pub system_region: wordcraft_geom::Region,
    /// Custom keyboard shortcuts (`tools.customizeKeyboard`); the front end saves them too.
    pub keymap: crate::KeyMap,
    /// Custom ribbon and Quick Access Toolbar (`tools.customizeRibbon`); the front end saves it too.
    pub ribbon: crate::RibbonLayout,
    /// The language new blank documents (and opened plain text) are written in, a BCP 47 tag;
    /// `None` is WordCraft's default, US English. The front end sets it from the interface
    /// language with [`Session::set_editing_language`] (a Polish interface writes Polish).
    pub editing_lang: Option<String>,
    /// The East Asian editing language, a BCP 47 tag (`ja-JP`): Word for Mac's East Asian
    /// Languages preference. With Japanese, new blank documents start from Word's Japanese Normal
    /// template whatever the region, and a document that names no East Asian language is in
    /// Japanese (see [`fill_east_asian_language`]). The front end sets it from the system's
    /// preferred languages ([`east_asian_editing_language`]); `None` (headless sessions) leaves
    /// both to the region.
    pub east_asian_editing: Option<String>,
    /// The document windows the front end has open, as it reports them before each frame
    /// ([`Windows`]); the View › Window commands check their parameters against them.
    pub windows: Windows,
    /// Editing inside an equation: which one and the caret in it.
    pub math: Option<MathEdit>,
    /// Equations are typed in LaTeX rather than the linear format.
    pub math_latex: bool,
    /// Text typed into equations is normal (non-math) text.
    pub math_normal_text: bool,
    /// The password Word documents are saved with (File › Info › Protect Document › Encrypt with
    /// Password, `file.encrypt`); `None` saves them unencrypted. Kept from a password-protected
    /// file that was opened, cleared when the document is replaced.
    pub password: Option<crate::io::Password>,
    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) recovery: Option<crate::recovery::Recovery>,
    /// More pictures and shapes selected along with the one the selection holds (Shift+click),
    /// for Group. Cleared by any edit or selection change other than adding to it.
    pub also_selected: Vec<Pos>,
    /// Column (block) selection, if one was made (see [`Session::column_segments`]).
    pub column: Option<ColumnBlock>,
    /// Column selection mode (Ctrl+Shift+F8): caret movement extends the block.
    pub column_mode: bool,
    /// A selection widened to whole table cells or rows, and where it really runs (see
    /// [`Session::sel_ends`]).
    pub(crate) widened: Option<Widened>,
    /// Where the last edits happened, oldest first (at most [`EDIT_MARKS`]), for Go Back
    /// (Shift+F5). Positions aren't mapped through later edits; they're clamped when used.
    edit_marks: Vec<Pos>,
    /// How many Go Back presses since the last edit (which place comes next).
    go_back_step: usize,
}

/// The number of edit locations Go Back cycles through (Word: four).
const EDIT_MARKS: usize = 4;

/// The equation being edited.
#[derive(Clone, Debug, PartialEq)]
pub struct MathEdit {
    /// The equation object (its U+FFFC).
    pub at: Pos,
    /// The caret inside it.
    pub pos: wordcraft_doc::math_edit::MathPos,
}

/// Editing preferences that persist between runs (the front end saves and restores them).
/// File › Options sets most of them (`file.options`).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Prefs {
    /// Word Count includes text boxes, footnotes and endnotes (Word's default).
    pub count_notes: bool,
    /// Track Changes Options: what markup shows and how revisions are drawn (per user, as in
    /// Word).
    pub markup: wordcraft_layout::display::MarkupOptions,
    /// File › Print / Export: Print Markup. PDFs, prints and page images show comments in
    /// balloons beside each page and tracked changes as marked up; HTML, Markdown and text
    /// copies end with the comments (`file.printMarkup`).
    pub print_markup: bool,
    /// Language › Detect language automatically (per user, as in Word).
    pub detect_language: bool,
    /// Set Proofing Language › Set As Default: overrides the interface language for future
    /// blank documents and plain text. Older preferences keep following the interface.
    #[serde(deserialize_with = "lenient_default_language")]
    pub default_language: Option<String>,
    /// Typing over a selection replaces it; off, the typed text goes in front of it.
    pub typing_replaces_selection: bool,
    /// The Insert key switches overtype on and off.
    pub insert_key_overtype: bool,
    /// Overtype: typed characters replace the ones after the caret.
    pub overtype: bool,
    /// The unit rulers and dialogs show lengths in, when the user chose one (File › Options ›
    /// Advanced › Measurement units); `None` follows the region ([`Session::units`]). Saved under
    /// a new key; an older `units` value other than inches (the old default) carries over on load.
    #[serde(rename = "measurementUnits")]
    pub units: Option<wordcraft_geom::Unit>,
    /// The region the user chose for new documents, units and the decimal separator (a locale
    /// tag like `pl-PL`, File › Options › Advanced › Region); `None` follows the system.
    pub region: Option<String>,
    /// A folder searched for Hunspell dictionaries before the system locations (File › Options ›
    /// Proofing, #1063): an app started from Finder or the Dock doesn't see
    /// `WORDCRAFT_DICTIONARIES`. `None` searches the system locations only.
    pub dictionaries_folder: Option<String>,
    /// The user templates folder File › New lists personal templates from and Save as Word
    /// Template starts in (File › Options › Advanced › File locations, #2852). `None`: the app's
    /// own `Templates` folder in its settings folder.
    pub user_templates_folder: Option<String>,
    /// Formatting marks shown even while Show/Hide ¶ is off.
    pub marks: wordcraft_layout::display::FormattingMarks,
    /// Mark grammar errors as you type (with Check spelling as you type on).
    pub mark_grammar: bool,
    /// Check spelling as you type (File › Options › Proofing). The live value is
    /// `ViewState::proofing`; this is the copy that is saved and restored (#1792).
    pub check_spelling: bool,
    pub ignore_uppercase: bool,
    pub ignore_numbers: bool,
    pub ignore_internet: bool,
    /// AutoCorrect Options: replacements, capitalization and AutoFormat As You Type (per user, as
    /// in Word). A damaged saved value reads as the defaults.
    #[serde(deserialize_with = "crate::cmd::tools::lenient_autocorrect")]
    pub autocorrect: crate::cmd::tools::AutoCorrectPrefs,
}

impl Default for Prefs {
    fn default() -> Self {
        Prefs {
            count_notes: true,
            markup: Default::default(),
            print_markup: false,
            typing_replaces_selection: true,
            insert_key_overtype: false,
            overtype: false,
            units: Default::default(),
            region: None,
            dictionaries_folder: None,
            user_templates_folder: None,
            marks: Default::default(),
            mark_grammar: true,
            check_spelling: true,
            ignore_uppercase: true,
            ignore_numbers: true,
            ignore_internet: true,
            detect_language: true,
            default_language: None,
            autocorrect: Default::default(),
        }
    }
}

impl Prefs {
    /// What the spelling check skips.
    pub fn spelling(&self) -> wordcraft_proof::Options {
        wordcraft_proof::Options {
            ignore_uppercase: self.ignore_uppercase,
            ignore_numbers: self.ignore_numbers,
            ignore_internet: self.ignore_internet,
        }
    }
}

/// Ignore malformed saved language tags without discarding the other preferences.
fn lenient_default_language<'de, D: serde::Deserializer<'de>>(d: D) -> Result<Option<String>, D::Error> {
    let value = serde_json::Value::deserialize(d)?;
    Ok(value.as_str().and_then(wordcraft_proof::lang::normalize_tag))
}

/// The citation style a document records (References › Style), APA when it records none or
/// one we don't know.
/// The byte range of the word the caret at `off` ends or sits in, counting apostrophes and
/// hyphens as the spelling check does; `None` unless a letter or digit comes before the caret.
fn typed_range(text: &str, off: usize) -> Option<(usize, usize)> {
    let in_word = |c: char| wordcraft_doc::bidi::is_word_char(c) || matches!(c, '’' | '-');
    let before = text.get(..off)?;
    let start = before.char_indices().rev().take_while(|&(_, c)| in_word(c)).last().map(|(i, _)| i)?;
    let end = off.saturating_add(text.get(off..)?.chars().take_while(|&c| in_word(c)).map(char::len_utf8).sum::<usize>());
    text.get(start..off)?.chars().any(char::is_alphanumeric).then_some((start, end))
}

fn doc_bib_style(doc: &Document) -> String {
    doc.settings.bib_style.as_deref().and_then(crate::cmd::citations::find_style).map_or("APA", |st| st.name).to_string()
}

/// Most pending front-end requests a session keeps.
pub const MAX_UI_REQUESTS: usize = 64;
/// Maximum undo depth.
const MAX_UNDO: usize = 500;
/// How many steps the Undo button's list shows (#701); `edit.undoTo` reaches no further.
pub const UNDO_LIST_MAX: usize = 100;

impl Session {
    /// Queue a request for the front end. A headless session never drains the queue, so only
    /// the newest [`MAX_UI_REQUESTS`] are kept.
    pub fn push_ui_request(&mut self, req: Value) {
        if self.ui_requests.len() >= MAX_UI_REQUESTS {
            self.ui_requests.remove(0);
        }
        self.ui_requests.push(req);
    }

    /// File name of the persisted custom dictionary inside the settings folder.
    pub const USER_DICT_FILE: &str = "custom-dictionary.txt";

    /// Save the custom dictionary through [`Session::user_dict_path`], if the host wired one up,
    /// with `removed` (words just taken out) left out of the merge with the file. A failed save
    /// keeps the words in memory and never fails the command (#40).
    pub(crate) fn persist_user_dictionary(&self, removed: &[String]) {
        if let Some(path) = self.user_dict_path.as_deref()
            && let Err(e) = wordcraft_proof::save_user_dictionary(path, removed)
        {
            log::warn!("custom dictionary: save failed: {e}");
        }
    }

    pub fn new(doc: Document) -> Self {
        let start = doc.start_of(StoryRef::Body);
        let bib_style = doc_bib_style(&doc);
        Session {
            doc,
            sel: Selection::caret(start),
            view: ViewState::default(),
            pending: None,
            path: None,
            file_name: None,
            user_dict_path: None,
            dirty: false,
            clipboard: None,
            pending_move: None,
            clipboard_text: String::new(),
            clipboard_html: String::new(),
            clip_history: Default::default(),
            find: FindState::default(),
            goal_x: None,
            page_hint: 0,
            registry: Arc::new(crate::cmd::registry()),
            author: "WordCraft User".into(),
            initials: String::new(),
            painter: None,
            status: String::new(),
            new_header_parts: Vec::new(),
            hf_return: None,
            history: Vec::new(),
            undo_evicted: 0,
            redo: Vec::new(),
            typing_open: false,
            composition: None,
            join_next: false,
            autocap: None,
            rev: 1,
            protect_facts: std::sync::Mutex::new(None),
            document_id: 1,
            cache: LayoutCache::new(),
            layout: None,
            typing: None,
            originals: Default::default(),
            picture_looks: Default::default(),
            last_command: None,
            recording: None,
            macros: Default::default(),
            versions: Vec::new(),
            building_blocks: Default::default(),
            autosave: true,
            bib_style,
            merge: Default::default(),
            ui_requests: Vec::new(),
            math: None,
            math_latex: false,
            math_normal_text: false,
            also_selected: Vec::new(),
            prefs: Prefs::default(),
            system_region: Default::default(),
            keymap: crate::KeyMap::default(),
            ribbon: crate::RibbonLayout::default(),
            editing_lang: None,
            east_asian_editing: None,
            windows: Windows::default(),
            read_aloud: Default::default(),
            password: None,
            #[cfg(not(target_arch = "wasm32"))]
            recovery: None,
            column: None,
            column_mode: false,
            widened: None,
            edit_marks: Vec::new(),
            go_back_step: 0,
        }
    }

    /// Remember the caret as the place of an edit, for Go Back. Edits in the same paragraph as
    /// the last one (typing on) update that place rather than adding one.
    fn note_edit(&mut self) {
        let at = self.sel.focus.clone();
        match self.edit_marks.last_mut() {
            Some(last) if last.story == at.story && last.path == at.path => *last = at,
            _ => {
                self.edit_marks.push(at);
                if self.edit_marks.len() > EDIT_MARKS {
                    self.edit_marks.remove(0);
                }
            }
        }
        self.go_back_step = 0;
    }

    /// Go Back (Shift+F5): the next of the last edit places, most recent first, cycling; a place
    /// the caret is already at is skipped. `None` before any edit.
    pub fn go_back(&mut self) -> Option<Pos> {
        let n = self.edit_marks.len();
        for _ in 0..n {
            let i = n - 1 - self.go_back_step % n;
            self.go_back_step = self.go_back_step.wrapping_add(1);
            let Some(p) = self.edit_marks.get(i) else { continue };
            let p = self.doc.clamp(p);
            if p != self.sel.focus || n == 1 {
                return Some(p);
            }
        }
        self.edit_marks.last().map(|p| self.doc.clamp(p))
    }

    /// The line pieces of the current column selection, if there is one (the selection hasn't
    /// moved since the block was made).
    pub fn column_segments(&self) -> Option<&[(Pos, Pos)]> {
        // Cells that are one run of the text are an ordinary stream selection.
        self.column.as_ref().filter(|c| c.sel == self.sel && !c.cells.as_ref().is_some_and(|r| r.stream)).map(|c| c.segments.as_slice())
    }

    /// The whole table cells selected, if the selection is that (see [`CellRange`]).
    pub fn cell_range(&self) -> Option<&CellRange> {
        self.column.as_ref().filter(|c| c.sel == self.sel).and_then(|c| c.cells.as_ref())
    }

    /// The selection as it was made: where a drag, Shift+click or Shift+arrows started and where
    /// the caret went, even when Word's table rule widened it to whole cells or rows (`sel` holds
    /// those).
    pub fn sel_ends(&self) -> &Selection {
        self.widened.as_ref().filter(|w| w.sel == self.sel).map_or(&self.sel, |w| &w.ends)
    }

    pub fn selects_whole_story(&self) -> bool {
        let st = self.sel.focus.story;
        let (a, b) = self.sel.ordered();
        a == self.doc.start_of(st) && b == self.doc.end_of(st)
    }

    /// Whether the caret is in a header or footer.
    pub fn in_header_footer(&self) -> bool {
        let StoryRef::Part(id) = self.sel.focus.story else { return false };
        self.doc.parts.get(&id).is_some_and(|p| matches!(p.kind, PartKind::Header | PartKind::Footer))
    }

    /// Document revision (bumped by every change).
    pub fn rev(&self) -> u64 {
        self.rev
    }
    /// Which document is open (see `document_id`): lets a caller tell an edit from a replacement.
    pub fn document_id(&self) -> u64 {
        self.document_id
    }
    pub fn touch(&mut self) {
        self.rev = self.rev.wrapping_add(1);
        self.dirty = true;
    }

    /// Reading Highlight ranges for the current document (searched again after edits); empty when it's off.
    pub fn find_highlights(&mut self) -> &[(Pos, Pos)] {
        if !self.find.highlight {
            return &[];
        }
        if self.find.highlights.as_ref().is_none_or(|(r, _)| *r != self.rev) {
            let found = crate::cmd::edit::search(self, StoryRef::Body).unwrap_or_default();
            self.find.highlights = Some((self.rev, found));
        }
        self.find.highlights.as_ref().map(|(_, v)| v.as_slice()).unwrap_or(&[])
    }

    /// Whether the markup area beside the page can show balloons: markup shown in Print Layout
    /// (not Read Mode or pages side by side), comments not listed in the Comments pane instead,
    /// and Track Changes Options not set to show every revision inline.
    pub fn balloons_shown(&self) -> bool {
        let v = &self.view;
        v.show_markup
            && !v.comments_pane
            && !v.read_mode
            && !v.multi_page
            && v.mode == ViewMode::Print
            && self.prefs.markup.balloons != wordcraft_layout::display::BalloonMode::Inline
    }

    /// Show revisions in balloons, with the balloons on screen: tracked deletions in the body leave
    /// the text for balloons in the markup area. Where no balloons can be shown they stay inline,
    /// as in Word; so do deletions in headers, footers, notes and text boxes, which get no balloons.
    pub fn revisions_in_balloons(&self) -> bool {
        let m = &self.prefs.markup;
        self.balloons_shown() && m.balloons == wordcraft_layout::display::BalloonMode::Revisions && m.insertions_deletions
    }

    /// Initials for new comments: the ones set in File › Options, else the user name's.
    pub fn user_initials(&self) -> String {
        let set = self.initials.trim();
        if set.is_empty() { self.author.split_whitespace().filter_map(|w| w.chars().next()).collect() } else { set.to_string() }
    }

    /// The document's file name and full path for fields (`FILENAME`) and layout context.
    /// Returns `(filename, filepath)`. Unsaved documents with no explicit file name default
    /// consistently to `("Document1", "Document1")`. Document metadata title is never used
    /// as a filename fallback (#3449).
    pub fn resolved_filenames(&self) -> (String, String) {
        if let Some(path) = &self.path {
            let name = path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_else(|| path.to_string_lossy().to_string());
            let full = path.to_string_lossy().to_string();
            (name, full)
        } else if let Some(name) = &self.file_name {
            (name.clone(), name.clone())
        } else {
            ("Document1".to_string(), "Document1".to_string())
        }
    }

    /// The current layout (recomputed when the document or view changed).
    pub fn layout(&mut self) -> Arc<DocLayout> {
        let mode = if self.view.read_mode { ViewMode::Read } else { self.view.mode };
        let width = match mode {
            ViewMode::Read => self.view.read_width,
            ViewMode::Web => self.view.web_width,
            _ => DRAFT_WIDTH,
        };
        let height = if self.view.read_mode { self.view.read_height } else { 0.0 };
        let spacing = if self.view.read_mode { self.view.read_line_spacing.map(f32::to_bits) } else { None };
        // Deletions in balloons depend on the view (the Comments pane, Read Mode, side by side), so
        // a view change that moves them in or out of the text lays it out again.
        let hide_deleted = !self.view.show_markup;
        let deleted = (hide_deleted, self.prefs.markup.hidden_deletions(), self.revisions_in_balloons());
        let outline = (mode == ViewMode::Outline).then(|| self.view.outline.view());
        let typing = self.typing_word();
        if let Some((r, w, h, m, sp, l, pf, o, hd, n, t)) = &self.layout
            && *r == self.rev
            && (*w == width || mode == ViewMode::Print)
            && *h == height
            && *m == mode
            && *sp == spacing
            && *pf == self.view.proofing
            && *n == self.view.numeral
            && *hd == deleted
            && *o == outline
            && *t == typing
        {
            return l.clone();
        }
        let (fn_name, fn_path) = self.resolved_filenames();
        let (filename, filepath): (Option<Arc<str>>, Option<Arc<str>>) = (Some(Arc::from(fn_name.as_str())), Some(Arc::from(fn_path.as_str())));
        let opts = LayoutOptions {
            view: mode,
            web_width: width,
            read_height: height,
            read_line_spacing: spacing.map(f32::from_bits),
            show_hidden: self.view.marks || self.prefs.marks.hidden,
            hide_deleted,
            hide_marked: deleted.1,
            deletions_in_balloons: deleted.2,
            proofing: self.view.proofing,
            numeral: self.view.numeral.resolve(SYSTEM_LOCALE.get().map(String::as_str)),
            comment_balloons: false,
            field_codes: self.view.field_codes,
            outline: outline.clone(),
            proof: wordcraft_layout::ProofOptions { grammar: self.prefs.mark_grammar, spelling: self.prefs.spelling() },
            typing_word: typing,
            filename: filename.clone(),
            filepath: filepath.clone(),
        };
        let l = Arc::new(wordcraft_layout::layout(&self.doc, &mut self.cache, &opts));
        self.layout = Some((self.rev, width, height, mode, spacing, l.clone(), self.view.proofing, outline, deleted, self.view.numeral, typing));
        l
    }

    /// The recorded word while the caret and the word are as typing left them; else drops it.
    fn typing_word(&mut self) -> Option<wordcraft_layout::TypingWord> {
        let t = self.typing.as_ref()?;
        let p = if self.sel.is_collapsed() && self.sel.focus == t.at { self.doc.para_at(&t.at) } else { None };
        match p {
            Some(p) if typed_range(&p.text, t.at.off) == Some((t.start, t.end)) => {
                Some(wordcraft_layout::TypingWord { rev: p.rev, start: t.start, end: t.end })
            }
            _ => {
                self.typing = None;
                None
            }
        }
    }

    /// After a successful command: a `text.*` edit at the caret records the word being typed;
    /// any other caret move ends it.
    fn note_typing(&mut self, id: &str, before: Option<&Document>, sel_before: &Selection) {
        let at = &self.sel.focus;
        let edited = || before.is_some_and(|d| d.para_at(at).map(|p| &p.text) != self.doc.para_at(at).map(|p| &p.text));
        if id.starts_with("text.") && edited() {
            self.typing = self.word_being_typed();
        } else if self.sel != *sel_before {
            self.typing = None;
        }
    }

    /// The word the collapsed caret ends or sits in, as [`TypedWord`].
    fn word_being_typed(&self) -> Option<TypedWord> {
        if !self.sel.is_collapsed() {
            return None;
        }
        let at = self.sel.focus.clone();
        let (start, end) = typed_range(&self.doc.para_at(&at)?.text, at.off)?;
        Some(TypedWord { at, start, end })
    }
    /// A layout for output (PDF, images, print): no proofing marks, print view.
    pub fn export_layout(&self) -> Arc<DocLayout> {
        self.export_layout_with(false)
    }

    /// [`Session::export_layout`]; with `markup` (Print Markup) each page also has the markup
    /// area with the comments in balloons.
    pub fn export_layout_with(&self, markup: bool) -> Arc<DocLayout> {
        let (fn_name, fn_path) = self.resolved_filenames();
        let opts = LayoutOptions {
            view: ViewMode::Print,
            comment_balloons: markup && self.prefs.markup.comments,
            numeral: self.view.numeral.resolve(SYSTEM_LOCALE.get().map(String::as_str)),
            filename: Some(Arc::from(fn_name.as_str())),
            filepath: Some(Arc::from(fn_path.as_str())),
            ..Default::default()
        };
        Arc::new(wordcraft_layout::layout(&self.doc, &mut LayoutCache::new(), &opts))
    }

    /// Invalidate the cached layout (fonts changed etc.).
    pub fn relayout(&mut self) {
        self.layout = None;
        self.cache.clear();
    }

    /// Set the language new documents are written in ([`Session::editing_lang`]; `None` or a
    /// malformed tag: US English). A document nobody has touched yet — untitled, unchanged, no
    /// undo steps, no text — switches too, so the blank document a session starts with follows
    /// the saved proofing default, or the interface language when none is saved. Nothing else
    /// changes and no undo step is added.
    pub fn set_editing_language(&mut self, tag: Option<&str>) {
        let tag = self
            .prefs
            .default_language
            .as_deref()
            .and_then(wordcraft_proof::lang::normalize_tag)
            .or_else(|| tag.and_then(wordcraft_proof::lang::normalize_tag));
        if self.editing_lang == tag {
            return;
        }
        self.editing_lang = tag;
        let untouched = self.path.is_none() && !self.dirty && self.history.is_empty() && self.redo.is_empty();
        if untouched && self.doc.plain_text(StoryRef::Body).trim().is_empty() && self.doc.paragraph_count() <= 1 {
            self.doc.styles.default_chr.lang = Some(self.editing_lang.clone().unwrap_or_else(|| "en-US".into()));
            self.rev = self.rev.wrapping_add(1);
            self.relayout();
        }
    }

    /// Snapshot for undo before a change.
    pub fn checkpoint(&mut self, label: &str) {
        if label == "Typing" && self.typing_open {
            return;
        }
        self.typing_open = label == "Typing";
        self.history.push(Arc::new(Undo {
            label: label.to_string(),
            doc: self.doc.clone(),
            sel: self.sel.clone(),
            pending: self.pending.clone(),
            column: self.column.clone(),
        }));
        if self.history.len() > MAX_UNDO {
            self.history.remove(0);
            self.undo_evicted += 1;
        }
        self.redo.clear();
    }
    /// Record `doc`/`sel` as their own undo step after the fact and close the typing group, so
    /// Undo right after an automatic change (AutoFormat, AutoCorrect) reverts only that change.
    pub fn push_undo(&mut self, label: &str, doc: Document, sel: Selection) {
        self.history.push(Arc::new(Undo { label: label.to_string(), doc, sel, pending: self.pending.clone(), column: None }));
        if self.history.len() > MAX_UNDO {
            self.history.remove(0);
            self.undo_evicted += 1;
        }
        self.typing_open = false;
    }
    /// Make the next mutating command part of the previous undo step instead of a new one, so a
    /// drag that runs a command every frame is a single Undo. Call it on every frame of the drag
    /// except the first; it is consumed by the next `run`.
    pub fn join_next_undo(&mut self) {
        self.join_next = true;
    }
    /// Stamp the document's save metadata (modified time, last modified by, revision; created
    /// when it has none) the way Word does on every save. `file.save` calls it, and so does any
    /// front end that writes the bytes itself (the web build's download, #262). Like Word, the
    /// document also keeps the zoom it is shown at (#995).
    pub fn stamp_save(&mut self) {
        self.doc.settings.zoom = crate::cmd::view::saved_zoom(&self.view);
        let core = &mut self.doc.core;
        core.modified = crate::cmd::now_iso();
        if core.created.is_empty() {
            core.created = core.modified.clone();
        }
        core.last_modified_by = self.author.clone();
        core.revision = core.revision.saturating_add(1);
    }
    /// Whether typing is still collecting into one undo step (no other command since).
    pub fn typing_open(&self) -> bool {
        self.typing_open
    }
    /// Reopen the typing group when the last undo step is typing, so the keys typed next join
    /// it: after an input method's rewrite was joined to it (`join_next_undo` closes the group).
    pub fn continue_typing(&mut self) {
        if self.undo_label() == Some("Typing") {
            self.typing_open = true;
        }
    }
    /// Close an open typing group (caret moved, other command).
    pub fn close_typing(&mut self) {
        self.typing_open = false;
    }
    pub fn can_undo(&self) -> bool {
        !self.history.is_empty()
    }
    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }
    pub fn undo_label(&self) -> Option<&str> {
        self.history.last().map(|u| u.label.as_str())
    }
    pub fn redo_label(&self) -> Option<&str> {
        self.redo.last().map(|u| u.label.as_str())
    }
    /// Undo steps pushed so far, evicted ones included: only grows while steps are added.
    pub(crate) fn undo_steps_so_far(&self) -> u64 {
        self.history.len() as u64 + self.undo_evicted
    }
    /// The number of undo steps.
    pub fn undo_depth(&self) -> usize {
        self.history.len()
    }
    /// Where the undo history stands, for [`Session::merge_undo_since`].
    pub fn undo_mark(&self) -> u64 {
        (self.history.len() as u64).saturating_add(self.undo_evicted)
    }
    /// Makes the steps recorded since `mark` one undo step, the first: a drag that applied its
    /// value as it went (a slider in a pane) undoes in one go.
    pub fn merge_undo_since(&mut self, mark: u64) {
        let keep = mark.saturating_sub(self.undo_evicted).saturating_add(1);
        self.history.truncate(usize::try_from(keep).unwrap_or(usize::MAX));
    }
    pub fn undo_labels(&self) -> Vec<String> {
        self.history.iter().rev().map(|u| u.label.clone()).collect()
    }
    /// The Undo button's list (#701): the undo steps' labels (English, as commands name them),
    /// newest first, at most [`UNDO_LIST_MAX`].
    pub fn undo_history(&self) -> Vec<&str> {
        self.history.iter().rev().take(UNDO_LIST_MAX).map(|u| u.label.as_str()).collect()
    }
    /// Undo the newest `count` steps in one go, as picking an entry in the Undo list does (#701).
    /// More than the undo depth undoes everything; a step that would take the document's
    /// protection away stops it (only Stop Protection lifts it). Returns the steps undone.
    pub fn undo_steps(&mut self, count: usize) -> usize {
        let mut done = 0;
        while done < count && !self.step_lifts_protection(false) && self.undo() {
            done += 1;
        }
        done
    }
    pub fn undo(&mut self) -> bool {
        self.typing_open = false;
        // Undoing the automatic capitalisation's own step makes its word an exception; any
        // other Undo forgets the capital.
        let steps = self.history.len() as u64 + self.undo_evicted;
        if let Some(c) = self.autocap.take().filter(|c| c.depth == steps) {
            c.add_exceptions(&mut self.prefs.autocorrect);
        }
        let Some(u) = self.history.pop().map(Arc::unwrap_or_clone) else { return false };
        let cur = Undo {
            label: u.label.clone(),
            doc: std::mem::replace(&mut self.doc, u.doc),
            sel: std::mem::replace(&mut self.sel, u.sel),
            pending: std::mem::replace(&mut self.pending, u.pending),
            column: std::mem::replace(&mut self.column, u.column),
        };
        self.redo.push(Arc::new(cur));
        self.touch();
        true
    }
    /// Would Undo (or Redo) take the document's protection away? Only Stop Protection lifts it.
    pub fn step_lifts_protection(&self, redo: bool) -> bool {
        let next = if redo { self.redo.last() } else { self.history.last() };
        next.is_some_and(|u| crate::cmd::protect::protection_lifted(&self.doc, &u.doc))
    }
    pub fn redo(&mut self) -> bool {
        self.typing_open = false;
        let Some(u) = self.redo.pop().map(Arc::unwrap_or_clone) else { return false };
        let cur = Undo {
            label: u.label.clone(),
            doc: std::mem::replace(&mut self.doc, u.doc),
            sel: std::mem::replace(&mut self.sel, u.sel),
            pending: std::mem::replace(&mut self.pending, u.pending),
            column: std::mem::replace(&mut self.column, u.column),
        };
        self.history.push(Arc::new(cur));
        self.touch();
        true
    }
    /// Drop history (after open / new).
    pub fn reset_history(&mut self) {
        self.autocap = None;
        self.history.clear();
        self.redo.clear();
        self.typing_open = false;
    }

    /// The version snapshots belong to the document they were taken from: keeping the old ones
    /// would let a restore put the previous document's content into this one (#1327 follow-up).
    pub fn clear_versions(&mut self) {
        self.versions.clear();
    }

    /// The region in effect (#838): the one the user chose ([`Prefs::region`]), else the
    /// system's.
    pub fn region(&self) -> wordcraft_geom::Region {
        match self.prefs.region.as_deref() {
            Some(tag) => wordcraft_geom::Region::from_tag(tag),
            None => self.system_region.clone(),
        }
    }

    /// Whether crop marks are shown at the corners of the text area: the user's choice
    /// (`view.cropMarks`), else on in the Japan region (Word for Mac shows them with Japanese
    /// editing, observed 2026-10-19) and off elsewhere.
    pub fn crop_marks(&self) -> bool {
        self.view.crop_marks.unwrap_or_else(|| self.region().territory == "JP")
    }

    /// Whether the document asks for Microsoft's Aptos family (the theme's body or heading font,
    /// the document defaults' or a style's font) and no Aptos is installed. Word lays it out with
    /// Aptos; no open font has its widths, so a stand-in breaks lines and pages differently. The
    /// interface then points to Microsoft's free download (`ui.getAptos`): installed, it is used.
    pub fn missing_aptos(&self) -> bool {
        let aptos = |f: &str| f.trim().get(..5).is_some_and(|p| p.eq_ignore_ascii_case("aptos"));
        let s = &self.doc.styles;
        let uses = aptos(&self.doc.settings.minor_font)
            || aptos(&self.doc.settings.major_font)
            || s.default_chr.font.as_deref().is_some_and(aptos)
            || s.styles.iter().any(|st| st.chr.font.as_deref().is_some_and(aptos));
        uses && !wordcraft_fonts::FontDb::global().has_family("Aptos")
    }

    /// The English of the region in effect (`en-GB` for a British region), or `None` when the
    /// region's language isn't English, it names no territory, or it is the United States (US
    /// English is the default anyway). An English interface writes new documents in it, as Word
    /// does on a British or Canadian system (#1045).
    pub fn region_english(&self) -> Option<String> {
        let r = self.region();
        (r.language == "en" && !r.territory.is_empty() && r.territory != "US")
            .then(|| format!("en-{}", r.territory))
            .and_then(|t| wordcraft_proof::lang::normalize_tag(&t))
    }

    /// The unit rulers and dialogs show lengths in: the user's choice, else the region's.
    pub fn units(&self) -> wordcraft_geom::Unit {
        self.prefs.units.unwrap_or_else(|| self.region().units())
    }

    /// `pt` in [`Session::units`], written the region's way (`2,54 cm` in Poland).
    pub fn format_length(&self, pt: f32) -> String {
        self.region().format(self.units(), pt)
    }

    /// A new blank document (the Normal template) on the region's paper: Letter in the US and
    /// other Letter regions, A4 elsewhere (#838). Margins stay 2.54 cm (1") all round.
    pub fn blank_document(&self) -> Document {
        blank_document_in(&self.region(), self.east_asian_editing.as_deref())
    }

    /// Replace the document (open/new).
    pub fn set_document(&mut self, doc: Document) {
        self.composition = None;
        self.autocap = None;
        self.document_id = self.document_id.wrapping_add(1);
        self.bib_style = doc_bib_style(&doc);
        self.doc = doc;
        self.doc.ensure_nonempty();
        if let Some(tag) = &self.east_asian_editing {
            fill_east_asian_language(&mut self.doc, tag);
        }
        self.password = None;
        self.file_name = None;
        self.sel = Selection::caret(self.doc.start_of(StoryRef::Body));
        self.pending = None;
        self.typing = None;
        self.also_selected.clear();
        self.column = None;
        self.column_mode = false;
        self.widened = None;
        self.edit_marks.clear();
        self.new_header_parts.clear();
        self.hf_return = None;
        self.go_back_step = 0;
        self.view.outline.collapsed.clear();
        self.reset_history();
        self.clear_versions();
        self.touch();
        self.dirty = false;
        self.relayout();
    }

    /// A snapshot to put back with [`Session::restore`], for a caller that runs a command and
    /// may want to take it back without a trace (cheap: the document's blocks are shared).
    ///
    /// Covered: `doc`, `sel`, the undo stack (its length, and its oldest steps when it is near
    /// the limit), `redo`, the open typing group, a pending `join_next_undo` and `dirty`. `rev`,
    /// `cache` and `layout` are derived (`restore` bumps `rev`). The other fields are not
    /// covered: a caller that lets the command change them (the view, the find state, the
    /// clipboard, macros, the file path…) saves and puts them back itself.
    ///
    /// `restore` is exact when the commands in between only add undo steps (every mutating
    /// command adds at most one; up to [`SNAPSHOT_HEAD`] may push old steps out of a full
    /// stack). Taking steps off the stack (`edit.undo`) or clearing it (`file.new`,
    /// `file.open`) in between is not covered.
    ///
    /// The destructuring lists every field, so a new field does not build until someone decides
    /// whether a snapshot must cover it.
    pub fn edit_snapshot(&self) -> EditSnapshot {
        let Session {
            doc,
            sel,
            view: _,
            pending: _,
            // Provisional input-method text: cleared by every command, so never in a snapshot.
            composition: _,
            path: _,
            file_name: _,
            // The custom dictionary file, like the file path: host wiring, not an edit.
            user_dict_path: _,
            dirty,
            clipboard: _,
            // Like the clipboard: what the next paste can make a move of.
            pending_move: _,
            clipboard_text: _,
            clipboard_html: _,
            clip_history: _,
            find: _,
            goal_x: _,
            page_hint: _,
            registry: _,
            author: _,
            initials: _,
            painter: _,
            status: _,
            // Headers just created to edit: closing checks they still exist and are empty.
            new_header_parts: _,
            hf_return: _,
            history,
            undo_evicted,
            redo,
            typing_open,
            join_next,
            autocap: _,
            rev: _,
            protect_facts: _,
            cache: _,
            layout: _,
            // The word being typed, like `column`: valid only while `sel` is where typing left it.
            typing: _,
            originals: _,
            picture_looks: _,
            last_command: _,
            recording: _,
            macros: _,
            versions: _,
            building_blocks: _,
            autosave: _,
            bib_style: _,
            merge: _,
            ui_requests: _,
            document_id: _,
            read_aloud: _,
            prefs: _,
            system_region: _,
            keymap: _,
            ribbon: _,
            // A preference for new documents, like `prefs`: not part of this document.
            editing_lang: _,
            east_asian_editing: _,
            // What the front end reports about its windows: nothing of the document.
            windows: _,
            // Equation editing mode, like `view`: not part of the document or its history.
            math: _,
            math_latex: _,
            math_normal_text: _,
            // The save password, like the file path: not an edit to the document.
            password: _,
            #[cfg(not(target_arch = "wasm32"))]
                recovery: _,
            also_selected: _,
            // Column selection and a widened selection's ends, like `sel`'s shape: valid only while
            // `sel` matches them.
            column: _,
            column_mode: _,
            widened: _,
            // Go Back's places, like the caret history: not part of the document.
            edit_marks: _,
            go_back_step: _,
        } = self;
        let head = if history.len() + SNAPSHOT_HEAD > MAX_UNDO { history.iter().take(SNAPSHOT_HEAD).cloned().collect() } else { Vec::new() };
        EditSnapshot {
            doc: doc.clone(),
            sel: sel.clone(),
            history_len: history.len(),
            evicted: *undo_evicted,
            head,
            redo: redo.clone(),
            typing_open: *typing_open,
            join_next: *join_next,
            dirty: *dirty,
        }
    }

    /// Put a snapshot back: the commands run since leave no trace in the document, the
    /// selection or undo/redo (undo steps the limit pushed out meanwhile come back). The layout
    /// is recomputed.
    pub fn restore(&mut self, snap: EditSnapshot) {
        let EditSnapshot { doc, sel, history_len, evicted, head, redo, typing_open, join_next, dirty } = snap;
        let gone = usize::try_from(self.undo_evicted.saturating_sub(evicted)).unwrap_or(usize::MAX);
        if gone > 0 {
            let back = gone.min(head.len());
            self.history.splice(0..0, head.into_iter().take(back));
        }
        self.undo_evicted = evicted;
        self.autocap = None;
        self.doc = doc;
        self.sel = sel;
        self.also_selected.clear();
        self.history.truncate(history_len);
        self.redo = redo;
        self.typing_open = typing_open;
        self.join_next = join_next;
        self.touch();
        self.dirty = dirty;
    }

    /// Make the selection valid for the current document.
    /// A new view of the document ([`ViewSlot`]): the caret at the start, with this view's
    /// settings (zoom, mode, panes).
    pub fn new_view(&self) -> ViewSlot {
        ViewSlot {
            sel: Selection::caret(self.doc.start_of(StoryRef::Body)),
            view: self.view.clone(),
            pending: None,
            find: FindState::default(),
            goal_x: None,
            page_hint: 0,
            painter: None,
            hf_return: None,
            autocap: None,
            typing: None,
            math: None,
            math_latex: self.math_latex,
            math_normal_text: self.math_normal_text,
            also_selected: Vec::new(),
            column: None,
            column_mode: false,
            widened: None,
            edit_marks: Vec::new(),
            go_back_step: 0,
        }
    }

    /// A copy of this view, at the same place: what a new window or split pane on the document
    /// opens with, as Word's does.
    pub fn clone_view(&self) -> ViewSlot {
        let mut v = self.new_view();
        v.sel = self.sel.clone();
        v.goal_x = self.goal_x;
        v.page_hint = self.page_hint;
        v
    }

    /// Give the session `slot`'s view and take this one into `slot`: another window or pane on
    /// the document takes its turn. Typing in progress ends (the next word starts an undo step of
    /// its own) and the incoming selection is clamped to the document as it is now. While the
    /// input method composes, the composition's caret is the document's for every view: only the
    /// view settings and the find state change hands.
    pub fn swap_view(&mut self, slot: &mut ViewSlot) {
        self.close_typing();
        self.join_next = false;
        std::mem::swap(&mut self.view, &mut slot.view);
        std::mem::swap(&mut self.find, &mut slot.find);
        std::mem::swap(&mut self.goal_x, &mut slot.goal_x);
        std::mem::swap(&mut self.page_hint, &mut slot.page_hint);
        if self.composition.is_some() {
            return;
        }
        std::mem::swap(&mut self.sel, &mut slot.sel);
        std::mem::swap(&mut self.pending, &mut slot.pending);
        std::mem::swap(&mut self.painter, &mut slot.painter);
        std::mem::swap(&mut self.hf_return, &mut slot.hf_return);
        std::mem::swap(&mut self.autocap, &mut slot.autocap);
        std::mem::swap(&mut self.typing, &mut slot.typing);
        std::mem::swap(&mut self.math, &mut slot.math);
        std::mem::swap(&mut self.math_latex, &mut slot.math_latex);
        std::mem::swap(&mut self.math_normal_text, &mut slot.math_normal_text);
        std::mem::swap(&mut self.also_selected, &mut slot.also_selected);
        std::mem::swap(&mut self.column, &mut slot.column);
        std::mem::swap(&mut self.column_mode, &mut slot.column_mode);
        std::mem::swap(&mut self.widened, &mut slot.widened);
        std::mem::swap(&mut self.edit_marks, &mut slot.edit_marks);
        std::mem::swap(&mut self.go_back_step, &mut slot.go_back_step);
        self.clamp_selection();
        self.also_selected.retain(|p| self.doc.clamp(p) == *p);
        if self.hf_return.as_ref().is_some_and(|r| self.doc.clamp(&r.focus) != r.focus) {
            self.hf_return = None;
        }
    }

    pub fn clamp_selection(&mut self) {
        self.sel.anchor = self.doc.clamp(&self.sel.anchor);
        self.sel.focus = self.doc.clamp(&self.sel.focus);
        // The equation being edited must still be there (undo, edits elsewhere).
        if let Some(m) = &self.math {
            let eq = self.doc.para_at(&m.at).and_then(|p| match p.object_at(m.at.off) {
                Some(wordcraft_doc::InlineObject::Equation { math, .. }) => Some(wordcraft_doc::math_edit::clamp(&math.nodes, &m.pos)),
                _ => None,
            });
            match eq {
                Some(pos) => {
                    if let Some(m) = self.math.as_mut() {
                        m.pos = pos;
                    }
                }
                None => self.math = None,
            }
        }
    }

    /// Why command `id` can't run now, for greying it out: its own `enabled` check, then what
    /// Restrict Editing refuses outright. `None` when it can run (or doesn't exist).
    pub fn why_disabled(&self, id: &str) -> Option<&'static str> {
        let spec = self.registry.get(id)?;
        (spec.enabled)(self).or_else(|| if spec.mutates { crate::cmd::protect::greyed_out(self, id, || self.protect_facts()) } else { None })
    }

    /// [`crate::cmd::protect::Facts`] for the document as it is now, cached per revision.
    fn protect_facts(&self) -> crate::cmd::protect::Facts {
        let key = (self.document_id, self.rev, self.author.clone());
        let mut cache = self.protect_facts.lock().unwrap_or_else(|e| e.into_inner());
        if let Some((k, f)) = cache.as_ref()
            && *k == key
        {
            return *f;
        }
        let f = crate::cmd::protect::Facts::of(self);
        *cache = Some((key, f));
        f
    }

    /// Run a command by id with JSON params (the single entry point for the UI, CLI, MCP and
    /// control channel). Mutating commands are undoable; a failed command leaves the document as
    /// it was; a panic inside a command becomes an error.
    pub fn run(&mut self, id: &str, params: &Value) -> Result<Value, CmdError> {
        let join = std::mem::take(&mut self.join_next);
        let reg = self.registry.clone();
        let Some(spec) = reg.get(id) else { return Err(CmdError::Unknown(id.to_string())) };
        // The input method's provisional text leaves before anything edits, moves, selects, saves
        // or replaces the document; queries (format.state for the ribbon, every frame) leave it.
        if self.composition.is_some()
            && (spec.mutates || ["select.", "caret.", "file.", "edit.", "text.", "table."].iter().any(|p| id.starts_with(p)))
        {
            self.clear_composition();
        }
        if let Some(why) = (spec.enabled)(self) {
            return Err(CmdError::Disabled(format!("{id}: {why}")));
        }
        // Restrict Editing: refused outright, or allowed as long as it only edits the exceptions.
        let guard = if spec.mutates { crate::cmd::protect::check_before(self, id, params)? } else { false };
        // Quick Parts aren't part of the document, so a refused edit puts them back itself (the Spike).
        let blocks_before = guard.then(|| self.building_blocks.clone());
        if spec.mutates && self.doc.settings.protection.as_deref() == Some("trackedChanges") {
            self.doc.settings.track_changes = true;
        }
        // Macro recording and Repeat.
        let record = !matches!(id, "tools.recordMacro" | "tools.macros" | "edit.undo" | "edit.redo" | "edit.repeat")
            && (spec.mutates || id.starts_with("caret.") || id.starts_with("select."));
        // A password never lands in a recorded macro or Repeat.
        let kept = || {
            let mut p = params.clone();
            if let Some(m) = p.as_object_mut() {
                m.remove("password");
            }
            p
        };
        if record && let Some((_, steps)) = self.recording.as_mut() {
            steps.push((id.to_string(), kept()));
        }
        if spec.mutates && !matches!(id, "edit.undo" | "edit.redo" | "edit.repeat") {
            self.last_command = Some((id.to_string(), kept()));
        }
        // Typing, caret movement and selection outside the equation commands leave the equation.
        if self.math.is_some()
            && (id.starts_with("text.") || id.starts_with("caret.") || id.starts_with("select.") || id.starts_with("edit.paste"))
            && let Some(m) = self.math.take()
        {
            let off = m.at.off + wordcraft_doc::para::OBJ.len_utf8();
            self.sel = Selection::caret(Pos { off, ..m.at });
        }
        // The undo stacks are snapshotted whole, not as a length: commands run nested commands,
        // whose checkpoints, undos and redos (and evictions at the limit) must be put back too.
        let before_doc = if spec.mutates {
            Some((
                self.doc.clone(),
                self.sel.clone(),
                self.history.clone(),
                self.redo.clone(),
                self.typing_open,
                self.undo_evicted,
                self.column.clone(),
            ))
        } else {
            None
        };
        // These mutating commands can replace the open document and discard its saved versions.
        // Keep the versions until the command and the protected-region post-check have both
        // succeeded; rejected commands must leave the old document and its snapshots together.
        let replaces_document = matches!(id, "review.compare" | "review.combine" | "mailings.finish" | "mailings.envelopes" | "mailings.labels");
        let versions_before = replaces_document.then(|| self.versions.clone());
        let identity_before = replaces_document.then(|| {
            (
                self.path.clone(),
                self.file_name.clone(),
                self.document_id,
                self.password.clone(),
                self.dirty,
                self.bib_style.clone(),
                self.merge.preview,
            )
        });
        let typing = spec.id == "text.insert" || spec.id == "equation.type";
        if spec.mutates && !typing {
            self.typing_open = false;
        }
        if spec.mutates {
            let label = if typing { "Typing" } else { spec.label };
            if !join || self.history.is_empty() {
                self.checkpoint(label);
            }
        } else if !spec.id.starts_with("caret.") && !spec.id.starts_with("view.") {
            // Non-mutating commands other than caret movement keep the typing group.
        } else if spec.id.starts_with("caret.") {
            self.typing_open = false;
        }
        // Only the paste right after a tracked cut makes it a move.
        if (spec.mutates || matches!(id, "edit.undo" | "edit.undoTo" | "edit.redo")) && !matches!(id, "edit.cut" | "edit.paste" | "edit.pasteSpecial")
        {
            self.pending_move = None;
        }
        let sel_before = self.sel.clone();
        // A rejected command must not persist a new proofing default, including a rejection
        // by the protected-region check after its handler has succeeded. Undo stays document-only.
        let default_language_before = self.prefs.default_language.clone();
        let run = spec.run;
        let mutates = spec.mutates;
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| crate::cmd::column::dispatch(self, id, mutates, run, params)));
        let mut result = match result {
            Ok(r) => r,
            Err(p) => {
                let msg = p.downcast_ref::<&str>().map(|s| s.to_string()).or_else(|| p.downcast_ref::<String>().cloned()).unwrap_or_default();
                Err(CmdError::Failed(format!("{id} panicked: {msg}")))
            }
        };
        // Restrict Editing: outside the exceptions nothing may change.
        if result.is_ok()
            && guard
            && let Some((before, ..)) = &before_doc
            && !crate::cmd::protect::outside_unchanged(before, &self.doc, &self.author)
        {
            result = Err(CmdError::Disabled(format!("{id}: only the editable regions of this protected document can change")));
        }
        match &result {
            Ok(_) => {
                if spec.mutates {
                    // Formatting changed while tracking becomes a formatting revision.
                    if let Some((before, ..)) = &before_doc {
                        crate::cmd::fmt_revisions::record(self, id, before);
                    }
                    self.touch();
                    self.doc.ensure_nonempty();
                    self.doc.prune_text_boxes();
                    // Glued connectors follow the shapes they join.
                    if crate::cmd::connectors::reroutes_after(id) && crate::cmd::connectors::reroute(self) {
                        self.touch();
                    }
                }
                self.clamp_selection();
                if sel_before.focus.story == StoryRef::Body && self.in_header_footer() {
                    self.hf_return = Some(sel_before.clone());
                }
                // A selection command that moves the insertion point ends the typing group, like
                // caret movement: typing, Select, typing are two undo steps (#419).
                if !spec.mutates && self.sel != sel_before {
                    self.typing_open = false;
                }
                if spec.mutates && !matches!(id, "edit.undo" | "edit.redo") {
                    self.note_edit();
                }
                // Outline view: collapsed headings follow edits; the caret stays where it shows.
                crate::cmd::outline::after_command(self, &sel_before);
                // Extra selected objects last until something else is edited or selected; formatting
                // them keeps them selected, as in Word.
                if !matches!(id, "select.addObject" | "arrange.ungroup" | "shape.fill" | "shape.effects") && (spec.mutates || self.sel != sel_before)
                {
                    self.also_selected.clear();
                }
                self.note_typing(id, before_doc.as_ref().map(|b| &b.0), &sel_before);
            }
            Err(e) => {
                self.autocap = None;
                self.prefs.default_language = default_language_before;
                if let Some(versions) = versions_before {
                    self.versions = versions;
                }
                if let Some((path, file_name, document_id, password, dirty, bib_style, merge_preview)) = identity_before {
                    self.path = path;
                    self.file_name = file_name;
                    self.document_id = document_id;
                    self.password = password;
                    self.dirty = dirty;
                    self.bib_style = bib_style;
                    self.merge.preview = merge_preview;
                }
                if let Some((d, s, h, r, t, ev, c)) = before_doc {
                    self.doc = d;
                    self.sel = s;
                    self.history = h;
                    self.redo = r;
                    self.typing_open = t;
                    // The steps the command pushed out are back, so they no longer count as gone.
                    self.undo_evicted = ev;
                    self.column = c;
                }
                if let Some(b) = blocks_before {
                    self.building_blocks = b;
                }
                self.status = e.to_string();
            }
        }
        result
    }

    /// The paragraph path the caret is in.
    pub fn caret_path(&self) -> &Path {
        &self.sel.focus.path
    }
    pub fn story(&self) -> StoryRef {
        self.sel.focus.story
    }

    /// Formatting typing would use at the caret: the caret's, except run direction. New text
    /// takes its direction from its own letters, as on screen, so Latin typed or inserted into
    /// reopened Hebrew isn't saved as right-to-left; the DOCX writer marks right-to-left pieces
    /// itself (#1543). The bidi language stays (it only applies to complex-script text).
    /// Over a selection, as in Word, typing takes the first selected character's formatting,
    /// whichever way the selection was made (#2879).
    pub fn typing_props(&self) -> CharProps {
        let (a, _) = self.sel.ordered();
        let first = match self.doc.para_at(&a) {
            Some(p) if self.pending.is_none() && !self.sel.is_collapsed() && a.off < p.len() => Some(p.props_of_char(a.off)),
            _ => None,
        };
        let mut c = match first {
            Some(props) => {
                // The selection is replaced, so the new text isn't part of a link or a note number.
                let mut out = if props.link.is_some() { props.unlinked() } else { props.clone() };
                if matches!(out.style.as_deref(), Some("FootnoteReference" | "EndnoteReference")) {
                    out.style = None;
                }
                out
            }
            None => self.caret_props(),
        };
        c.rtl = None;
        c
    }

    /// The caret's formatting, as Reveal Formatting shows it.
    pub fn caret_props(&self) -> CharProps {
        if let Some(p) = &self.pending {
            return p.clone();
        }
        if let Some(p) = self.composition_props() {
            return p.clone();
        }
        let f = &self.sel.focus;
        let Some(p) = self.doc.para_at(f) else { return CharProps::default() };
        let props = p.props_at(f.off);
        // As in Word, text typed at either end of a hyperlink isn't part of it; only typing
        // inside the link extends it.
        let inside_link = props.link.is_some() && f.off > 0 && f.off < p.len() && p.props_of_char(f.off).link == props.link;
        // Text typed next to a symbol is in the text's font, not the symbol's (`w:sym`).
        let mut out = (if props.link.is_some() && !inside_link { props.unlinked() } else { props.clone() }).without_symbol();
        // A footnote or endnote number is raised by its character style; text typed after it is body text.
        if matches!(out.style.as_deref(), Some("FootnoteReference" | "EndnoteReference")) {
            out.style = None;
        }
        out
    }

    /// Words in the document, as the status bar shows them (see [`Prefs::count_notes`]).
    pub fn word_count(&self) -> usize {
        if self.prefs.count_notes { self.doc.word_count_including_notes() } else { self.doc.word_count() }
    }

    /// Selected plain text.
    pub fn selected_text(&self) -> String {
        // A column block or whole cells that aren't one run of the text: each piece's own text,
        // a line each, so the status bar and Word Count count those cells only.
        if let Some(segs) = self.column_segments() {
            return segs.iter().map(|(a, b)| self.doc.copy_range(a.min(b), a.max(b)).plain_text()).collect::<Vec<_>>().join("\n");
        }
        let (a, b) = self.sel.ordered();
        self.doc.copy_range(&a, &b).plain_text()
    }
}

/// A new blank document on `region`'s paper (see [`Session::blank_document`]). A Japanese
/// region gets Word's Japanese Normal template ([`japanese_normal`]); Chinese and Korean ones
/// the theme font language that picks their script's theme font for Han text.
pub fn blank_document(region: &wordcraft_geom::Region) -> Document {
    blank_document_in(region, None)
}

/// [`blank_document`] with `east_asian` as the East Asian editing language
/// ([`Session::east_asian_editing`]), which wins over the region's language: Japanese editing on
/// an English Mac in Japan starts from Word's Japanese Normal template, as Word for Mac does
/// there (observed 2026-10-20, interface English, region `en-JP`, East Asian Languages Japanese).
pub fn blank_document_in(region: &wordcraft_geom::Region, east_asian: Option<&str>) -> Document {
    let mut d = Document::new();
    let (_, w, h) = region.paper();
    d.last_section.page_w = w;
    d.last_section.page_h = h;
    let territory = region.territory.to_ascii_uppercase();
    match east_asian {
        Some(tag) if tag.to_ascii_lowercase().starts_with("ja") => japanese_normal(&mut d),
        Some(tag) => d.settings.theme_font_lang_ea = Some(tag.into()),
        None => match region.language.to_ascii_lowercase().as_str() {
            "ja" => japanese_normal(&mut d),
            "zh" => d.settings.theme_font_lang_ea = Some(if matches!(territory.as_str(), "TW" | "HK" | "MO") { "zh-TW" } else { "zh-CN" }.into()),
            "ko" => d.settings.theme_font_lang_ea = Some("ko-KR".into()),
            _ => {}
        },
    }
    d
}

/// The East Asian editing language among the system's preferred languages (BCP 47 tags, most
/// preferred first): the first Japanese, Chinese or Korean one, as `ja-JP`, `zh-CN` (`zh-TW` for
/// Traditional Chinese, Taiwan, Hong Kong and Macao) or `ko-KR`; `None` when there is none.
pub fn east_asian_editing_language<'a>(tags: impl IntoIterator<Item = &'a str>) -> Option<String> {
    tags.into_iter().find_map(|tag| {
        let tag = tag.to_ascii_lowercase();
        let mut parts = tag.split(['-', '_', '.', '@']);
        match parts.next()? {
            "ja" => Some("ja-JP".into()),
            "ko" => Some("ko-KR".into()),
            "zh" => Some(if parts.any(|p| matches!(p, "hant" | "tw" | "hk" | "mo")) { "zh-TW" } else { "zh-CN" }.into()),
            _ => None,
        }
    })
}

/// Give `doc` the East Asian language `tag` where it names none — in its document defaults and
/// as its theme font language — as Word for Mac does with that East Asian editing language: a
/// file whose defaults say only `w:lang w:val="en-US"` has its Japanese text in Japanese (the
/// object model's `language ID` of the paragraph), and Word saves it back with
/// `w:eastAsia="ja-JP"` in both (`lg1` probe, 2026-10-20). A language the file names stays.
pub fn fill_east_asian_language(doc: &mut Document, tag: &str) {
    if doc.styles.default_chr.lang_ea.is_none() {
        doc.styles.default_chr.lang_ea = Some(tag.into());
    }
    if doc.settings.theme_font_lang_ea.is_none() {
        doc.settings.theme_font_lang_ea = Some(tag.into());
    }
}

/// Word's Normal template with Japanese as the East Asian editing language, measured in Word
/// for Mac 16.113 (a Normal.dotm it created afresh): 10.5 pt body text (Latin and East Asian
/// sizes `w:sz="21"`), single line spacing with no space after and no widow control, a line grid
/// of 18 pt (`w:docGrid w:type="lines" w:linePitch="360"`), 42 pt default tabs, header and
/// footer 851 and 992 twips from the edge, 425 twips between columns, punctuation compression on, the East Asian language
/// `ja-JP` in the document defaults and `w:themeFontLang`, and Word's Japanese compatibility
/// switches (`w:doNotExpandShiftReturn`, `w:ulTrailSpace`, `w:spaceForUL`,
/// `w:balanceSingleByteDoubleByteWidth`, `w:doNotLeaveBackslashAlone`,
/// `w:adjustLineHeightInTable`, `w:useFELayout`). The paper follows the region as before.
/// The styles of Word's Japanese Normal.dotm where they differ from its English one (the files
/// Word for Mac saved after its built-in styles were applied through its object model with
/// Japanese editing, `st1`–`st3` and `fn1` probes, 2026-10-19): the headings are black (the
/// theme's text colour, where the English ones are its dark blue), in the major fonts (Aptos
/// Display and 游ゴシック Light) at every level, Heading 1–3 16, 14 and 12 pt with 14, 8 and 8 pt
/// before and 4 after, Heading 4–9 the text's size with 4 before and 2 after, none italic, all
/// kept with the next paragraph and on one page; Title and Subtitle are centred (Subtitle 8 pt
/// after); Quote has 8 pt after as well as before; Caption is bold and nothing else; Header and
/// Footer keep Normal's spacing, off the line grid, with the 234 and 468 pt tabs; TOC 1 has no
/// indent or spacing and TOC 2–9 are indented one more character a level; TOC Heading is Heading 1
/// at 12 pt in the automatic colour with no space before or after, not kept together; footnote
/// and endnote text are Normal off the grid (10.5 pt, where the English template's are 10 pt with
/// single spacing and no space after), the references superscript in both.
pub fn japanese_normal_styles(styles: &mut wordcraft_doc::styles::StyleSheet) {
    use wordcraft_doc::props::{Align, CharProps, ParaProps, Rgb, TextColor};
    let black = Some(TextColor::Rgb(Rgb(0, 0, 0)));
    for n in 1..=9u32 {
        let Some(style) = styles.get_mut(&format!("Heading{n}")) else { continue };
        let (size, before, after) = match n {
            1 => (Some(16.0), 14.0, 4.0),
            2 => (Some(14.0), 8.0, 4.0),
            3 => (Some(12.0), 8.0, 4.0),
            _ => (None, 4.0, 2.0),
        };
        style.para.space_before = Some(before);
        style.para.space_after = Some(after);
        style.para.keep_next = Some(true);
        style.para.keep_lines = Some(true);
        style.chr.font = Some(wordcraft_doc::styles::HEADING_FONT.into());
        style.chr.size = size;
        style.chr.italic = None;
        style.chr.color = black;
    }
    if let Some(style) = styles.get_mut("Title") {
        style.para.align = Some(Align::Center);
    }
    if let Some(style) = styles.get_mut("Subtitle") {
        style.para.align = Some(Align::Center);
        style.para.space_after = Some(8.0);
        style.chr.font = Some(wordcraft_doc::styles::HEADING_FONT.into());
    }
    if let Some(style) = styles.get_mut("Quote") {
        style.para.space_after = Some(8.0);
    }
    if let Some(style) = styles.get_mut("Caption") {
        style.para = ParaProps::default();
        style.chr = CharProps { bold: Some(true), ..Default::default() };
    }
    for id in ["Header", "Footer"] {
        if let Some(style) = styles.get_mut(id) {
            style.para = ParaProps { tabs: style.para.tabs.take(), snap_to_grid: Some(false), ..Default::default() };
        }
    }
    for n in 1..=9u32 {
        if let Some(style) = styles.get_mut(&format!("TOC{n}")) {
            style.para = ParaProps { indent_left_chars: (n > 1).then_some((n - 1) as f32), ..Default::default() };
        }
    }
    if let Some(style) = styles.get_mut("TOCHeading") {
        style.para.space_before = Some(0.0);
        style.para.space_after = Some(0.0);
        style.para.keep_lines = Some(false);
        style.chr.size = Some(12.0);
        style.chr.color = None;
    }
    for id in ["FootnoteText", "EndnoteText"] {
        if let Some(style) = styles.get_mut(id) {
            style.para = ParaProps { snap_to_grid: Some(false), ..Default::default() };
            style.chr = CharProps::default();
        }
    }
}

pub fn japanese_normal(d: &mut Document) {
    let chr = &mut d.styles.default_chr;
    chr.size = Some(10.5);
    chr.size_cs = Some(12.0);
    // Its Latin language is English (Japan), which Word has no proofing tools for: nothing in a
    // new Japanese document is marked as misspelt until a run is given another language.
    chr.lang = Some("en-JP".into());
    chr.lang_ea = Some("ja-JP".into());
    // Its document defaults: Kerning for fonts from 2 pt and standard and contextual ligatures.
    chr.kern = Some(2.0);
    chr.ligatures = Some(wordcraft_doc::props::Ligatures::STANDARD_CONTEXTUAL);
    let para = &mut d.styles.default_para;
    para.line_spacing = Some(wordcraft_doc::props::LineSpacing::Multiple(1.0));
    para.space_after = Some(0.0);
    para.widow_control = Some(false);
    d.settings.theme_font_lang_ea = Some("ja-JP".into());
    d.settings.punct_compression = wordcraft_doc::east_asian::PunctCompression::CompressPunctuation;
    d.settings.default_tab = 42.0;
    // Word's Japanese Normal.dotm sets these compatibility switches (its English one sets none):
    // a justified line ending with a manual line break is not spread, underlines run under
    // trailing spaces.
    d.settings.do_not_expand_shift_return = true;
    d.settings.ul_trail_space = true;
    d.settings.space_for_ul = true;
    d.settings.balance_single_byte_double_byte_width = true;
    d.settings.do_not_leave_backslash_alone = true;
    d.settings.adjust_line_height_in_table = true;
    d.settings.use_fe_layout = true;
    japanese_normal_styles(&mut d.styles);
    let sect = &mut d.last_section;
    sect.doc_grid = Some(wordcraft_doc::section::DocGrid { kind: wordcraft_doc::section::DocGridType::Lines, line_pitch: 18.0, char_space: 0 });
    sect.header = 851.0 / 20.0;
    sect.footer = 992.0 / 20.0;
    sect.columns.space = 425.0 / 20.0;
}

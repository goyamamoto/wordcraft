//! The editing session: document, selection, undo/redo, layout, view state.

use std::sync::Arc;

use serde_json::Value;
use wordcraft_doc::edit::Fragment;
use wordcraft_doc::props::CharProps;
use wordcraft_doc::{Document, PartKind, Path, Pos, StoryRef};
use wordcraft_layout::{DocLayout, LayoutCache, LayoutOptions, ViewMode};

use crate::{CmdError, ImeEditOutcome, Registry};

mod documents;
mod views;
pub use documents::{DocInfo, DocState};
use documents::{document_id_mark, next_document_id, untitled_name};
pub use views::WindowView;
mod history;
mod ime;
mod layout;
mod lifecycle;
mod query;
mod run;
mod snapshot;

use crate::doc_state;
use crate::ime as ime_state;
use crate::locale;
use crate::prefs;
use crate::snapshot as snapshot_types;
use crate::view;
pub(crate) use doc_state::Widened;
pub use doc_state::{CellRange, ColumnBlock, FindState, MathEdit, Selection, UNDO_LIST_MAX, next_math_session};
use doc_state::{TypedWord, Undo, doc_bib_style};
use ime_state::ImeCaretState;
pub use locale::set_system_locale;
pub(crate) use locale::system_locale;
pub use prefs::Prefs;
#[cfg(test)]
pub(crate) use query::japanese_normal;
pub use query::{blank_document, blank_document_in, east_asian_editing_language, fill_east_asian_language};
pub use snapshot_types::EditSnapshot;
use snapshot_types::{LayoutSlots, RunSnapshot};
pub use view::{StylesPaneDock, ViewState};

/// An editing session: the app-wide state (clipboard, preferences, macros, the command registry…)
/// and the open documents. The active document's state is in the session's own fields, which
/// every command works on; the other open documents wait in [`DocState`] values and are swapped
/// in when they become active (see [`Session::documents`]).
pub struct Session {
    pub doc: Document,
    pub sel: Selection,
    pub view: ViewState,
    /// Formatting picked with a collapsed caret, applied to the next typed text.
    pub pending: Option<CharProps>,
    /// Last highlighter choice for the shortcut; palette state, outside document undo.
    pub(crate) last_highlight: wordcraft_doc::props::Highlight,
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
    /// The next equation typing command joins a successful equation `deleteSurrounding`'s undo
    /// step. Unlike [`Self::join_next`], any other command consumes and discards it, so an
    /// unrelated Bold or document replacement can never be merged into the deletion.
    equation_join_next: bool,
    /// The next `text.insert` at this exact selection joins a successful body
    /// `DeleteSurrounding`'s undo step. Any other command consumes and discards it, so an
    /// intervening edit owns its own checkpoint instead of being merged into the deletion.
    ime_text_join: Option<Selection>,
    /// Whether the undo group has a step of its own for the next joined command to merge
    /// into. A command that changed nothing leaves the group without one, so the next
    /// command in the group starts the step the group needs.
    join_anchor: bool,
    /// The input method's composition shown inline (see `composition.rs`).
    pub(crate) composition: Option<crate::composition::Composition>,
    /// The newest automatic capitalisation, for Word's "Automatically add words to list" (#1857).
    pub(crate) autocap: Option<crate::cmd::tools::AutoCap>,
    pub(crate) rev: u64,
    /// Which document this is, unique in the process: changes when [`Session::set_document`]
    /// replaces it (an open into an untouched blank document, recover), never on an edit or when
    /// another document becomes active and this one comes back.
    document_id: u64,
    /// The number in an untitled document's name (`Document2`); 0 for one opened from a file.
    untitled: u32,
    /// `rev` when the document was made, for [`Session::is_untouched_blank`].
    made_rev: u64,
    /// What Restrict Editing's outright refusals depend on, for greying out commands
    /// ([`Session::why_disabled`]): computed once per document, revision and author instead of
    /// for every button on every frame.
    protect_facts: std::sync::Mutex<Option<((u64, u64, String), crate::cmd::protect::Facts)>>,
    /// How many times the document's body was replaced under the same [`Self::document_id`]
    /// (`document.setText`, version restore, compare, Undo/Redo). It is the body's IME
    /// replacement epoch: a composition must not survive the swap, even when the new body has
    /// the same text or the same bounded surrounding window.
    ime_body_epoch: u64,
    /// Engine-owned, checked, process-monotonic revision for IME receipts whose target cannot
    /// provide surrounding text. `None` means the allocator is exhausted (fail closed).
    ime_state_revision: Option<u64>,
    /// The caret/equation-session token the revision was last allocated for. It covers selection
    /// and caret changes without walking the document.
    ime_state_seen: Option<ImeCaretState>,
    /// The current command's explicit IME effect classification.
    ime_edit_outcome: ImeEditOutcome,
    /// Simulation of a globally exhausted IME revision allocator for front-end producer tests.
    /// Only [`Self::exhaust_ime_state_revision`] sets it; production builds do not compile it,
    /// and it only makes receipt classification fail closed.
    #[cfg(any(test, feature = "test-support"))]
    ime_state_exhausted: bool,
    /// A successful non-IME command changed document state since the last IME observation. It is
    /// drained before publishing a receipt and at observation time; it is never cleared by a later
    /// command. It covers mixed formatting/IME frames when available stamps ignore the revision.
    pending_ime_application_change: bool,
    /// Whether the current command was entered through [`Session::run_ime`]. Nested commands
    /// inherit the origin.
    ime_origin: bool,
    cache: LayoutCache,
    layout: LayoutSlots,
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
    /// ([`Windows`]); the View › Window commands that arrange windows check their parameters
    /// against them.
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
    /// The recovery worker every open document's copy is kept through (desktop hosts only).
    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) recovery: Option<crate::recovery::Recovery>,
    /// The active document's recovery copy.
    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) recovery_slot: crate::recovery::Slot,
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
    /// The open documents other than the active one, in no particular order.
    others: Vec<DocState>,
    /// Every open document's id, the active one's too, in the order they were opened.
    order: Vec<u64>,
    /// Untitled documents made so far, for the next one's name (`Document3`).
    untitled_count: u32,
}

/// The document windows the front end has open, as the desktop app reports them to the session
/// before each frame. The session owns no window: the View › Window commands that arrange
/// windows (Arrange All, View Side by Side, Synchronous Scrolling, Reset Window Position) check
/// their parameters here and queue a request ([`Session::push_ui_request`]) the front end carries
/// out. Headless sessions, the browser and the CLI have no window host: `host` stays false and
/// those commands are disabled.
#[derive(Clone, Debug, Default, PartialEq, serde::Serialize, serde::Deserialize)]
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
#[derive(Clone, Debug, Default, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct WindowEntry {
    pub id: u64,
    pub title: String,
    /// The document's file, when it has one.
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
    /// The other windows showing this window's document (View › New Window).
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

impl Session {
    /// File name of the persisted custom dictionary inside the settings folder.
    pub const USER_DICT_FILE: &str = "custom-dictionary.txt";
}

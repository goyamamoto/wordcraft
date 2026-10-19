#![allow(clippy::unnecessary_cast)]
use std::cell::{Cell, RefCell};
use std::collections::{HashMap, VecDeque};
use std::ops::Range;
use std::ptr;
use std::sync::Mutex;

use objc2::rc::{Retained, WeakId};
use objc2::runtime::{AnyObject, Sel};
use objc2::{declare_class, msg_send_id, mutability, sel, ClassType, DeclaredClass};
use objc2_app_kit::{
    NSApplication, NSCursor, NSEvent, NSEventPhase, NSResponder, NSTextInputClient,
    NSTrackingRectTag, NSView, NSViewFrameDidChangeNotification,
};
use objc2_foundation::{
    MainThreadMarker, NSArray, NSAttributedString, NSAttributedStringKey, NSCopying,
    NSMutableAttributedString, NSNotFound, NSNotificationCenter, NSObject, NSObjectProtocol,
    NSPoint, NSRange, NSRect, NSSize, NSString, NSUInteger,
};

use super::app_state::ApplicationDelegate;
use super::cursor::{default_cursor, invisible_cursor};
use super::event::{
    code_to_key, code_to_location, create_key_event, event_mods, lalt_pressed, ralt_pressed,
    scancode_to_physicalkey, KeyEventExtra,
};
use super::window::WinitWindow;
use super::DEVICE_ID;
use crate::dpi::{LogicalPosition, LogicalSize};
use crate::event::{
    DeviceEvent, ElementState, Ime, KeyEvent, Modifiers, MouseButton, MouseScrollDelta, TouchPhase,
    WindowEvent,
};
use crate::keyboard::{Key, KeyCode, KeyLocation, ModifiersState, NamedKey};
use crate::platform::macos::OptionAsAlt;

#[derive(Debug)]
struct CursorState {
    visible: bool,
    cursor: Retained<NSCursor>,
}

impl Default for CursorState {
    fn default() -> Self {
        Self { visible: true, cursor: default_cursor() }
    }
}

#[derive(Debug, Eq, PartialEq, Clone, Copy, Default)]
enum ImeState {
    #[default]
    /// The IME events are disabled, so only `ReceivedCharacter` is being sent to the user.
    Disabled,

    /// The ground state of enabled IME input. It means that both Preedit and regular keyboard
    /// input could be start from it.
    Ground,

    /// The IME is in preedit.
    Preedit,

    /// The text was just committed, so the next input from the keyboard must be ignored.
    Committed,
}

bitflags::bitflags! {
    #[derive(Debug, Clone, Copy, PartialEq)]
    struct ModLocationMask: u8 {
        const LEFT     = 0b0001;
        const RIGHT    = 0b0010;
    }
}
impl ModLocationMask {
    fn from_location(loc: KeyLocation) -> ModLocationMask {
        match loc {
            KeyLocation::Left => ModLocationMask::LEFT,
            KeyLocation::Right => ModLocationMask::RIGHT,
            _ => unreachable!(),
        }
    }
}

fn key_to_modifier(key: &Key) -> Option<ModifiersState> {
    match key {
        Key::Named(NamedKey::Alt) => Some(ModifiersState::ALT),
        Key::Named(NamedKey::Control) => Some(ModifiersState::CONTROL),
        Key::Named(NamedKey::Super) => Some(ModifiersState::SUPER),
        Key::Named(NamedKey::Shift) => Some(ModifiersState::SHIFT),
        _ => None,
    }
}

fn get_right_modifier_code(key: &Key) -> KeyCode {
    match key {
        Key::Named(NamedKey::Alt) => KeyCode::AltRight,
        Key::Named(NamedKey::Control) => KeyCode::ControlRight,
        Key::Named(NamedKey::Shift) => KeyCode::ShiftRight,
        Key::Named(NamedKey::Super) => KeyCode::SuperRight,
        _ => unreachable!(),
    }
}

// WordCraft patch (vendor/patches/winit/01-macos-ime-surrounding-text.md): the text around the
// caret, given by the application, so the input method can read committed text and ask for a
// range of it to be replaced (再変換 — reconversion of a selection — and context for live
// conversion). One window takes keyboard input at a time, so a process-wide store suffices and the
// application needs no handle to the window (eframe keeps it).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ImeSurroundingText {
    /// The text the input method may read (the caret's paragraph, say), UTF-8.
    pub text: String,
    /// The selection within `text` in UTF-16 code units; collapsed, it is the caret.
    pub selected: Range<usize>,
}

/// WordCraft patch: the marked text's place in the surrounding text — where it lies and its
/// length, in UTF-16 code units, and the committed text it stands for (a reconverted selection,
/// or nothing).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ImeMarked {
    pub location: usize,
    pub len: usize,
    pub replaced: String,
}

impl ImeSurroundingText {
    /// WordCraft patch: this text as the input method has just changed it, until the
    /// application reports it again: `text` put in place of `range` — UTF-16 code units of
    /// this text, the marked text `marked` included, as the input method counts — as marked
    /// text with the input method's selection `selected` within it, or committed (`None`).
    /// A range reaching into the marked text counts it as the committed text it stands for.
    /// Empty marked text takes the marked text out and selects what it stood for, as the
    /// application does. Returns the new text with its selection, and the marked text's place
    /// in it, if any.
    pub fn apply(
        &self,
        marked: Option<&ImeMarked>,
        range: Range<usize>,
        text: &str,
        selected: Option<Range<usize>>,
    ) -> (ImeSurroundingText, Option<ImeMarked>) {
        // The committed text: the marked text replaced by what it stands for, the range mapped
        // onto it (what lies before keeps its place, the marked text is what it stood for, what
        // follows moves by the difference).
        let mut units: Vec<u16> = self.text.encode_utf16().collect();
        let (m, l, r) = match marked {
            Some(k) => {
                let m = k.location.min(units.len());
                let l = k.len.min(units.len() - m);
                let put_back: Vec<u16> = k.replaced.encode_utf16().collect();
                let r = put_back.len();
                units.splice(m..m + l, put_back);
                (m, l, r)
            }
            None => (0, 0, 0),
        };
        let map = |x: usize| {
            if x <= m {
                x
            } else if x < m.saturating_add(l) {
                m.saturating_add(r)
            } else {
                x.saturating_sub(l).saturating_add(r)
            }
        };
        let start = map(range.start).min(units.len());
        let end = map(range.end).clamp(start, units.len());
        let replaced = String::from_utf16_lossy(&units[start..end]);
        let new: Vec<u16> = text.encode_utf16().collect();
        let n = new.len();
        match selected {
            // Dismissed: the committed text is back, what the marked text stood for selected.
            Some(_) if n == 0 => {
                (ImeSurroundingText { text: String::from_utf16_lossy(&units), selected: start..end }, None)
            }
            Some(sel) => {
                units.splice(start..end, new);
                let a = sel.start.min(n);
                let b = sel.end.clamp(a, n);
                let text = String::from_utf16_lossy(&units);
                (
                    ImeSurroundingText { text, selected: start + a..start + b },
                    Some(ImeMarked { location: start, len: n, replaced }),
                )
            }
            None => {
                units.splice(start..end, new);
                let caret = start + n;
                (ImeSurroundingText { text: String::from_utf16_lossy(&units), selected: caret..caret }, None)
            }
        }
    }
}

static IME_SURROUNDING: Mutex<Option<ImeSurroundingText>> = Mutex::new(None);
static IME_REPLACEMENT: Mutex<Option<Range<usize>>> = Mutex::new(None);

/// Tell the input method what text lies around the caret (`None`: nothing, as before the patch).
pub fn set_ime_surrounding_text(text: Option<ImeSurroundingText>) {
    if let Ok(mut s) = IME_SURROUNDING.lock() {
        *s = text;
    }
}

/// The range of the surrounding text (UTF-16 code units) the input method asked to replace with
/// its marked text, if `setMarkedText:selectedRange:replacementRange:` named one since the last
/// call; the `Ime::Preedit` event carrying that marked text follows.
pub fn take_ime_replacement_range() -> Option<Range<usize>> {
    IME_REPLACEMENT.lock().ok().and_then(|mut r| r.take())
}

fn ime_surrounding() -> Option<ImeSurroundingText> {
    IME_SURROUNDING.lock().ok().and_then(|s| s.clone())
}

/// What the input method is told lies around the caret right now.
pub fn ime_surrounding_text() -> Option<ImeSurroundingText> {
    ime_surrounding()
}

impl WinitView {
    /// WordCraft patch: the marked text's place in the surrounding text, while there is one.
    fn ime_marked(&self) -> Option<ImeMarked> {
        let len = self.ivars().marked_text.borrow().length() as usize;
        (len > 0).then(|| ImeMarked {
            location: self.ivars().marked_location.get(),
            len,
            replaced: self.ivars().marked_replaced.borrow().clone(),
        })
    }

    /// WordCraft patch: the range of the surrounding text (marked text included) an edit by the
    /// input method takes the place of: the replacement range it named, else the marked text,
    /// else the selection.
    fn ime_edit_range(&self, replacement_range: NSRange) -> Range<usize> {
        if replacement_range.location != NSNotFound as NSUInteger {
            let start = replacement_range.location as usize;
            return start..start.saturating_add(replacement_range.length as usize);
        }
        if let Some(k) = self.ime_marked() {
            return k.location..k.location.saturating_add(k.len);
        }
        ime_surrounding().map_or(0..0, |s| {
            let start = s.selected.start.min(s.selected.end);
            start..s.selected.end.max(start)
        })
    }

    /// WordCraft patch: the surrounding text follows an edit by the input method at once
    /// (`ImeSurroundingText::apply`), so what it reads next, before the application reports
    /// the text again, is the state it just made.
    fn ime_update_surrounding(&self, range: Range<usize>, text: &str, selected: Option<NSRange>) {
        let Some(s) = ime_surrounding() else { return };
        let selected = selected.map(|r| {
            let start = r.location as usize;
            start..start.saturating_add(r.length as usize)
        });
        let (next, marked) = s.apply(self.ime_marked().as_ref(), range, text, selected);
        set_ime_surrounding_text(Some(next));
        match marked {
            Some(k) => {
                self.ivars().marked_location.set(k.location);
                *self.ivars().marked_replaced.borrow_mut() = k.replaced;
            }
            None => self.ivars().marked_replaced.borrow_mut().clear(),
        }
    }
}

/// `range` of the UTF-16 form of `text`, clamped to it, as an `NSString`.
fn utf16_slice(text: &str, range: Range<usize>) -> (Retained<NSString>, NSRange) {
    let units: Vec<u16> = text.encode_utf16().collect();
    let start = range.start.min(units.len());
    let end = range.end.clamp(start, units.len());
    let slice = String::from_utf16_lossy(&units[start..end]);
    (NSString::from_str(&slice), NSRange::new(start as NSUInteger, (end - start) as NSUInteger))
}

fn get_left_modifier_code(key: &Key) -> KeyCode {
    match key {
        Key::Named(NamedKey::Alt) => KeyCode::AltLeft,
        Key::Named(NamedKey::Control) => KeyCode::ControlLeft,
        Key::Named(NamedKey::Shift) => KeyCode::ShiftLeft,
        Key::Named(NamedKey::Super) => KeyCode::SuperLeft,
        _ => unreachable!(),
    }
}

/// WordCraft patch: a record of what the input method asks of the view, written when
/// `WORDCRAFT_IME_TRACE` names a file (each call, its text and ranges, the IME state) — to see an
/// input method's real calls, which can't be reproduced without typing.
fn ime_trace(line: impl FnOnce() -> String) {
    use std::io::Write;
    static FILE: std::sync::OnceLock<Option<Mutex<std::fs::File>>> = std::sync::OnceLock::new();
    let file = FILE.get_or_init(|| {
        let path = std::env::var_os("WORDCRAFT_IME_TRACE")?;
        std::fs::OpenOptions::new().create(true).append(true).open(path).ok().map(Mutex::new)
    });
    if let Some(f) = file {
        if let Ok(mut f) = f.lock() {
            let _ = writeln!(f, "{}", line());
        }
    }
}

#[derive(Debug)]
pub struct ViewState {
    /// Strong reference to the global application state.
    app_delegate: Retained<ApplicationDelegate>,

    cursor_state: RefCell<CursorState>,
    ime_position: Cell<NSPoint>,
    ime_size: Cell<NSSize>,
    modifiers: Cell<Modifiers>,
    phys_modifiers: RefCell<HashMap<Key, ModLocationMask>>,
    tracking_rect: Cell<Option<NSTrackingRectTag>>,
    ime_state: Cell<ImeState>,
    input_source: RefCell<String>,

    /// True iff the application wants IME events.
    ///
    /// Can be set using `set_ime_allowed`
    ime_allowed: Cell<bool>,

    /// True if the current key event should be forwarded
    /// to the application, even during IME
    forward_key_to_app: Cell<bool>,

    marked_text: RefCell<Retained<NSMutableAttributedString>>,
    /// WordCraft patch: where the marked text lies in the surrounding text (UTF-16 code units),
    /// and the input method's selection within the marked text.
    marked_location: Cell<usize>,
    marked_selected: Cell<NSRange>,
    /// WordCraft patch: the committed text the marked text stands for (`ImeMarked::replaced`).
    marked_replaced: RefCell<String>,
    /// WordCraft patch: a key event is being interpreted (`interpretKeyEvents`): an
    /// `insertText:` then is the key's own text, which the key event reports.
    in_key_event: Cell<bool>,
    /// WordCraft patch: the characters of the key event being interpreted. An `insertText:`
    /// within it of other text is the input method's (see `insertText:`).
    key_text: RefCell<String>,
    accepts_first_mouse: bool,

    // Weak reference because the window keeps a strong reference to the view
    _ns_window: WeakId<WinitWindow>,

    /// The state of the `Option` as `Alt`.
    option_as_alt: Cell<OptionAsAlt>,
}

declare_class!(
    pub(super) struct WinitView;

    unsafe impl ClassType for WinitView {
        #[inherits(NSResponder, NSObject)]
        type Super = NSView;
        type Mutability = mutability::MainThreadOnly;
        const NAME: &'static str = "WinitView";
    }

    impl DeclaredClass for WinitView {
        type Ivars = ViewState;
    }

    unsafe impl WinitView {
        #[method(isFlipped)]
        fn is_flipped(&self) -> bool {
            // `winit` uses the upper-left corner as the origin.
            true
        }

        #[method(viewDidMoveToWindow)]
        fn view_did_move_to_window(&self) {
            trace_scope!("viewDidMoveToWindow");
            if let Some(tracking_rect) = self.ivars().tracking_rect.take() {
                self.removeTrackingRect(tracking_rect);
            }

            let rect = self.frame();
            let tracking_rect = unsafe {
                self.addTrackingRect_owner_userData_assumeInside(rect, self, ptr::null_mut(), false)
            };
            assert_ne!(tracking_rect, 0, "failed adding tracking rect");
            self.ivars().tracking_rect.set(Some(tracking_rect));
        }

        #[method(frameDidChange:)]
        fn frame_did_change(&self, _event: &NSEvent) {
            trace_scope!("frameDidChange:");
            if let Some(tracking_rect) = self.ivars().tracking_rect.take() {
                self.removeTrackingRect(tracking_rect);
            }

            let rect = self.frame();
            let tracking_rect = unsafe {
                self.addTrackingRect_owner_userData_assumeInside(rect, self, ptr::null_mut(), false)
            };
            assert_ne!(tracking_rect, 0, "failed adding tracking rect");
            self.ivars().tracking_rect.set(Some(tracking_rect));

            // Emit resize event here rather than from windowDidResize because:
            // 1. When a new window is created as a tab, the frame size may change without a window resize occurring.
            // 2. Even when a window resize does occur on a new tabbed window, it contains the wrong size (includes tab height).
            let logical_size = LogicalSize::new(rect.size.width as f64, rect.size.height as f64);
            let size = logical_size.to_physical::<u32>(self.scale_factor());
            self.queue_event(WindowEvent::Resized(size));
        }

        #[method(drawRect:)]
        fn draw_rect(&self, _rect: NSRect) {
            trace_scope!("drawRect:");

            // It's a workaround for https://github.com/rust-windowing/winit/issues/2640, don't replace with `self.window_id()`.
            if let Some(window) = self.ivars()._ns_window.load() {
                self.ivars().app_delegate.handle_redraw(window.id());
            }

            // This is a direct subclass of NSView, no need to call superclass' drawRect:
        }

        #[method(acceptsFirstResponder)]
        fn accepts_first_responder(&self) -> bool {
            trace_scope!("acceptsFirstResponder");
            true
        }

        // This is necessary to prevent a beefy terminal error on MacBook Pros:
        // IMKInputSession [0x7fc573576ff0 presentFunctionRowItemTextInputViewWithEndpoint:completionHandler:] : [self textInputContext]=0x7fc573558e10 *NO* NSRemoteViewController to client, NSError=Error Domain=NSCocoaErrorDomain Code=4099 "The connection from pid 0 was invalidated from this process." UserInfo={NSDebugDescription=The connection from pid 0 was invalidated from this process.}, com.apple.inputmethod.EmojiFunctionRowItem
        // TODO: Add an API extension for using `NSTouchBar`
        #[method_id(touchBar)]
        fn touch_bar(&self) -> Option<Retained<NSObject>> {
            trace_scope!("touchBar");
            None
        }

        #[method(resetCursorRects)]
        fn reset_cursor_rects(&self) {
            trace_scope!("resetCursorRects");
            let bounds = self.bounds();
            let cursor_state = self.ivars().cursor_state.borrow();
            // We correctly invoke `addCursorRect` only from inside `resetCursorRects`
            if cursor_state.visible {
                self.addCursorRect_cursor(bounds, &cursor_state.cursor);
            } else {
                self.addCursorRect_cursor(bounds, &invisible_cursor());
            }
        }
    }

    unsafe impl NSTextInputClient for WinitView {
        #[method(hasMarkedText)]
        fn has_marked_text(&self) -> bool {
            trace_scope!("hasMarkedText");
            self.ivars().marked_text.borrow().length() > 0
        }

        #[method(markedRange)]
        fn marked_range(&self) -> NSRange {
            trace_scope!("markedRange");
            let length = self.ivars().marked_text.borrow().length();
            if length > 0 {
                // WordCraft patch: in the surrounding text's coordinates.
                NSRange::new(self.ivars().marked_location.get() as NSUInteger, length)
            } else {
                // Documented to return `{NSNotFound, 0}` if there is no marked range.
                NSRange::new(NSNotFound as NSUInteger, 0)
            }
        }

        #[method(selectedRange)]
        fn selected_range(&self) -> NSRange {
            trace_scope!("selectedRange");
            // WordCraft patch: the selection in the surrounding text's coordinates — within the
            // marked text while there is one, else what the application reported.
            if unsafe { self.hasMarkedText() } {
                let sel = self.ivars().marked_selected.get();
                let len = self.ivars().marked_text.borrow().length();
                let location = sel.location.min(len);
                let length = sel.length.min(len - location);
                return NSRange::new(
                    (self.ivars().marked_location.get() as NSUInteger).saturating_add(location),
                    length,
                );
            }
            if let Some(s) = ime_surrounding() {
                let len = s.text.encode_utf16().count();
                let start = s.selected.start.min(len);
                let end = s.selected.end.clamp(start, len);
                return NSRange::new(start as NSUInteger, (end - start) as NSUInteger);
            }
            // Documented to return `{NSNotFound, 0}` if there is no selection.
            NSRange::new(NSNotFound as NSUInteger, 0)
        }

        #[method(setMarkedText:selectedRange:replacementRange:)]
        fn set_marked_text(
            &self,
            string: &NSObject,
            selected_range: NSRange,
            replacement_range: NSRange,
        ) {
            trace_scope!("setMarkedText:selectedRange:replacementRange:");
            ime_trace(|| {
                let text = if string.is_kind_of::<NSAttributedString>() {
                    let s: *const NSAttributedString = (string as *const NSObject).cast();
                    unsafe { &*s }.string().to_string()
                } else {
                    let s: *const NSString = (string as *const NSObject).cast();
                    unsafe { &*s }.to_string()
                };
                format!(
                    "setMarkedText {text:?} selected {}+{} replacement {}+{} hadMarked {} state {:?} inKey {}",
                    selected_range.location, selected_range.length, replacement_range.location as isize, replacement_range.length,
                    unsafe { self.hasMarkedText() }, self.ivars().ime_state.get(), self.ivars().in_key_event.get()
                )
            });

            // WordCraft patch: where the marked text goes. A replacement range names committed
            // text to replace (reconversion); otherwise it replaces the marked text, or the
            // selection when there is none. The application reads the range from
            // `take_ime_replacement_range` when the `Ime::Preedit` event arrives.
            let had_marked_text = unsafe { self.hasMarkedText() };
            // The range of the surrounding text (marked text included) the new marked text
            // takes the place of: the one named, else the marked text, else the selection.
            let replaces = self.ime_edit_range(replacement_range);
            if replacement_range.location != NSNotFound as NSUInteger {
                let start = replacement_range.location as usize;
                let end = start.saturating_add(replacement_range.length as usize);
                if let Ok(mut r) = IME_REPLACEMENT.lock() {
                    *r = Some(start..end);
                }
                self.ivars().marked_location.set(start);
            } else if !had_marked_text {
                let start = ime_surrounding().map_or(0, |s| s.selected.start.min(s.selected.end));
                self.ivars().marked_location.set(start);
            }
            self.ivars().marked_selected.set(selected_range);

            // SAFETY: This method is guaranteed to get either a `NSString` or a `NSAttributedString`.
            let (marked_text, string) = if string.is_kind_of::<NSAttributedString>() {
                let string: *const NSObject = string;
                let string: *const NSAttributedString = string.cast();
                let string = unsafe { &*string };
                (
                    NSMutableAttributedString::from_attributed_nsstring(string),
                    string.string(),
                )
            } else {
                let string: *const NSObject = string;
                let string: *const NSString = string.cast();
                let string = unsafe { &*string };
                (
                    NSMutableAttributedString::from_nsstring(string),
                    string.copy(),
                )
            };

            // WordCraft patch: what the input method reads next answers from the text as it
            // stands now — the application reports it again after the next frame.
            self.ime_update_surrounding(replaces, &string.to_string(), Some(selected_range));

            // Update marked text.
            *self.ivars().marked_text.borrow_mut() = marked_text;

            // Notify IME is active if application still doesn't know it.
            if self.ivars().ime_state.get() == ImeState::Disabled {
                *self.ivars().input_source.borrow_mut() = self.current_input_source();
                self.queue_event(WindowEvent::Ime(Ime::Enabled));
            }

            if unsafe { self.hasMarkedText() } {
                self.ivars().ime_state.set(ImeState::Preedit);
            } else {
                // In case the preedit was cleared, set IME into the Ground state.
                self.ivars().ime_state.set(ImeState::Ground);
            }

            let cursor_range = if string.is_empty() {
                // An empty string basically means that there's no preedit, so indicate that by
                // sending a `None` cursor range.
                None
            } else {
                // Clamp to string length to avoid NSRangeException from out-of-bounds
                // indices sent by macOS IME (e.g. native Pinyin, see
                // https://github.com/alacritty/alacritty/issues/8791).
                let len = string.length();
                let location = selected_range.location.min(len);
                let end = selected_range.end().min(len);
                // Convert the selected range from UTF-16 indices to UTF-8 indices.
                let sub_string_a = unsafe { string.substringToIndex(location) };
                let sub_string_b = unsafe { string.substringToIndex(end) };
                let lowerbound_utf8 = sub_string_a.len();
                let upperbound_utf8 = sub_string_b.len();
                Some((lowerbound_utf8, upperbound_utf8))
            };

            // Send WindowEvent for updating marked text
            self.queue_event(WindowEvent::Ime(Ime::Preedit(string.to_string(), cursor_range)));
        }

        #[method(unmarkText)]
        fn unmark_text(&self) {
            trace_scope!("unmarkText");
            ime_trace(|| format!("unmarkText marked {:?} state {:?}", self.ivars().marked_text.borrow().string().to_string(), self.ivars().ime_state.get()));
            // WordCraft patch: `unmarkText` commits the marked text as it stands (the
            // `NSTextInputClient` contract, what `NSTextView` and Word do); winit dropped it.
            let committed = self.ivars().marked_text.borrow().string().to_string();
            if !committed.is_empty() {
                let range = self.ime_edit_range(NSRange::new(NSNotFound as NSUInteger, 0));
                self.ime_update_surrounding(range, &committed, None);
            }
            *self.ivars().marked_text.borrow_mut() = NSMutableAttributedString::new();

            let input_context = self.inputContext().expect("input context");
            input_context.discardMarkedText();

            self.queue_event(WindowEvent::Ime(Ime::Preedit(String::new(), None)));
            if !committed.is_empty() && self.is_ime_enabled() {
                self.queue_event(WindowEvent::Ime(Ime::Commit(committed)));
            }
            if self.is_ime_enabled() {
                // Leave the Preedit self.ivars()
                self.ivars().ime_state.set(ImeState::Ground);
            } else {
                tracing::warn!("Expected to have IME enabled when receiving unmarkText");
            }
        }

        #[method_id(validAttributesForMarkedText)]
        fn valid_attributes_for_marked_text(&self) -> Retained<NSArray<NSAttributedStringKey>> {
            trace_scope!("validAttributesForMarkedText");
            NSArray::new()
        }

        #[method_id(attributedSubstringForProposedRange:actualRange:)]
        fn attributed_substring_for_proposed_range(
            &self,
            range: NSRange,
            actual_range: *mut NSRange,
        ) -> Option<Retained<NSAttributedString>> {
            trace_scope!("attributedSubstringForProposedRange:actualRange:");
            // WordCraft patch: the surrounding text the application reported, clamped to it.
            let slice = ime_surrounding().filter(|_| range.location != NSNotFound as NSUInteger).map(|s| {
                let start = range.location as usize;
                let end = start.saturating_add(range.length as usize);
                utf16_slice(&s.text, start..end)
            });
            match slice {
                Some((string, actual)) => {
                    if !actual_range.is_null() {
                        // SAFETY: AppKit passes either null or a pointer to an `NSRange` it owns.
                        unsafe { *actual_range = actual };
                    }
                    Some(NSAttributedString::from_nsstring(&string))
                }
                None => None,
            }
        }

        #[method(characterIndexForPoint:)]
        fn character_index_for_point(&self, _point: NSPoint) -> NSUInteger {
            trace_scope!("characterIndexForPoint:");
            0
        }

        #[method(firstRectForCharacterRange:actualRange:)]
        fn first_rect_for_character_range(
            &self,
            _range: NSRange,
            _actual_range: *mut NSRange,
        ) -> NSRect {
            trace_scope!("firstRectForCharacterRange:actualRange:");
            let rect = NSRect::new(
                self.ivars().ime_position.get(),
                self.ivars().ime_size.get()
            );
            // Return value is expected to be in screen coordinates, so we need a conversion here
            self.window()
                .convertRectToScreen(self.convertRect_toView(rect, None))
        }

        #[method(insertText:replacementRange:)]
        fn insert_text(&self, string: &NSObject, replacement_range: NSRange) {
            trace_scope!("insertText:replacementRange:");
            // WordCraft patch: a commit naming committed text to replace (an input method that
            // reconverts without marking first) is reported like a marked-text replacement.
            if replacement_range.location != NSNotFound as NSUInteger && unsafe { self.hasMarkedText() } {
                let start = replacement_range.location as usize;
                let end = start.saturating_add(replacement_range.length as usize);
                if let Ok(mut r) = IME_REPLACEMENT.lock() {
                    *r = Some(start..end);
                }
            }

            // SAFETY: This method is guaranteed to get either a `NSString` or a `NSAttributedString`.
            let string = if string.is_kind_of::<NSAttributedString>() {
                let string: *const NSObject = string;
                let string: *const NSAttributedString = string.cast();
                unsafe { &*string }.string().to_string()
            } else {
                let string: *const NSObject = string;
                let string: *const NSString = string.cast();
                unsafe { &*string }.to_string()
            };

            let is_control = string.chars().next().is_some_and(|c| c.is_control());
            ime_trace(|| {
                format!(
                    "insertText {string:?} replacement {}+{} hasMarked {} state {:?} inKey {} key {:?}",
                    replacement_range.location as isize, replacement_range.length, unsafe { self.hasMarkedText() },
                    self.ivars().ime_state.get(), self.ivars().in_key_event.get(), self.ivars().key_text.borrow()
                )
            });

            // Commit only if we have marked text.
            if unsafe { self.hasMarkedText() } && self.is_ime_enabled() && !is_control {
                // WordCraft patch: the committed text takes the marked text's place (or the
                // named range's) in the surrounding text at once, and the marked text is gone:
                // what the input method reads next is the committed state.
                let range = self.ime_edit_range(replacement_range);
                self.ime_update_surrounding(range, &string, None);
                *self.ivars().marked_text.borrow_mut() = NSMutableAttributedString::new();
                self.queue_event(WindowEvent::Ime(Ime::Preedit(String::new(), None)));
                self.queue_event(WindowEvent::Ime(Ime::Commit(string)));
                self.ivars().ime_state.set(ImeState::Committed);
            } else if !self.ivars().in_key_event.get() && self.is_ime_enabled() && !is_control && !string.is_empty() {
                // WordCraft patch: text an input method inserts on its own, outside any key
                // event and without marking it first (a direct insertion), is a commit too;
                // within a key event the key's own text comes with the key event, as before.
                let range = self.ime_edit_range(replacement_range);
                self.ime_update_surrounding(range, &string, None);
                self.queue_event(WindowEvent::Ime(Ime::Commit(string)));
            } else if self.ivars().in_key_event.get()
                && self.is_ime_enabled()
                && !is_control
                && !string.is_empty()
                && string != *self.ivars().key_text.borrow()
            {
                // WordCraft patch: within a key event, text other than the key's own characters is
                // the input method's — the rest of a conversion committed after its first clause,
                // whose marking this view no longer holds, or a character the input method makes
                // of the key (the full-width space): a commit, and the key itself isn't reported
                // (`Committed`). Before, it was taken for the key's text and dropped, and the
                // key (Return) went to the application instead.
                let range = self.ime_edit_range(replacement_range);
                self.ime_update_surrounding(range, &string, None);
                self.queue_event(WindowEvent::Ime(Ime::Commit(string)));
                self.ivars().ime_state.set(ImeState::Committed);
            }
        }

        // Basically, we're sent this message whenever a keyboard event that doesn't generate a "human
        // readable" character happens, i.e. newlines, tabs, and Ctrl+C.
        #[method(doCommandBySelector:)]
        fn do_command_by_selector(&self, _command: Sel) {
            trace_scope!("doCommandBySelector:");
            ime_trace(|| format!("doCommandBySelector {} state {:?}", _command.name(), self.ivars().ime_state.get()));
            // We shouldn't forward any character from just committed text, since we'll end up sending
            // it twice with some IMEs like Korean one. We'll also always send `Enter` in that case,
            // which is not desired given it was used to confirm IME input.
            if self.ivars().ime_state.get() == ImeState::Committed {
                return;
            }

            self.ivars().forward_key_to_app.set(true);

            if unsafe { self.hasMarkedText() } && self.ivars().ime_state.get() == ImeState::Preedit
            {
                // Leave preedit so that we also report the key-up for this key.
                self.ivars().ime_state.set(ImeState::Ground);
            }
        }
    }

    unsafe impl WinitView {
        #[method(keyDown:)]
        fn key_down(&self, event: &NSEvent) {
            // (WordCraft patch: see `in_key_event`.)
            trace_scope!("keyDown:");
            {
                let mut prev_input_source = self.ivars().input_source.borrow_mut();
                let current_input_source = self.current_input_source();
                if *prev_input_source != current_input_source && self.is_ime_enabled() {
                    *prev_input_source = current_input_source;
                    drop(prev_input_source);
                    self.ivars().ime_state.set(ImeState::Disabled);
                    self.queue_event(WindowEvent::Ime(Ime::Disabled));
                }
            }

            // Get the characters from the event.
            let old_ime_state = self.ivars().ime_state.get();
            self.ivars().forward_key_to_app.set(false);
            let event = replace_event(event, self.option_as_alt());

            // The `interpretKeyEvents` function might call
            // `setMarkedText`, `insertText`, and `doCommandBySelector`.
            // It's important that we call this before queuing the KeyboardInput, because
            // we must send the `KeyboardInput` event during IME if it triggered
            // `doCommandBySelector`. (doCommandBySelector means that the keyboard input
            // is not handled by IME and should be handled by the application)
            if self.ivars().ime_allowed.get() {
                let events_for_nsview = NSArray::from_slice(&[&*event]);
                // WordCraft patch: `insertText:` within is the key's own text (see `insertText:`).
                *self.ivars().key_text.borrow_mut() = unsafe { event.characters() }.map(|s| s.to_string()).unwrap_or_default();
                ime_trace(|| format!("keyDown {:?} state {:?} marked {:?}", self.ivars().key_text.borrow(), self.ivars().ime_state.get(), self.ivars().marked_text.borrow().string().to_string()));
                self.ivars().in_key_event.set(true);
                unsafe { self.interpretKeyEvents(&events_for_nsview) };
                self.ivars().in_key_event.set(false);

                // If the text was committed we must treat the next keyboard event as IME related.
                if self.ivars().ime_state.get() == ImeState::Committed {
                    // Remove any marked text, so normal input can continue.
                    *self.ivars().marked_text.borrow_mut() = NSMutableAttributedString::new();
                }
            }

            self.update_modifiers(&event, false);

            let had_ime_input = match self.ivars().ime_state.get() {
                ImeState::Committed => {
                    // Allow normal input after the commit.
                    self.ivars().ime_state.set(ImeState::Ground);
                    true
                }
                ImeState::Preedit => true,
                // `key_down` could result in preedit clear, so compare old and current state.
                _ => old_ime_state != self.ivars().ime_state.get(),
            };

            ime_trace(|| format!("  keyDown done state {:?} forward {}", self.ivars().ime_state.get(), !had_ime_input || self.ivars().forward_key_to_app.get()));
            if !had_ime_input || self.ivars().forward_key_to_app.get() {
                let key_event = create_key_event(&event, true, unsafe { event.isARepeat() });
                self.queue_event(WindowEvent::KeyboardInput {
                    device_id: DEVICE_ID,
                    event: key_event,
                    is_synthetic: false,
                });
            }
        }

        #[method(keyUp:)]
        fn key_up(&self, event: &NSEvent) {
            trace_scope!("keyUp:");

            let event = replace_event(event, self.option_as_alt());
            self.update_modifiers(&event, false);

            // We want to send keyboard input when we are currently in the ground state.
            if matches!(
                self.ivars().ime_state.get(),
                ImeState::Ground | ImeState::Disabled
            ) {
                self.queue_event(WindowEvent::KeyboardInput {
                    device_id: DEVICE_ID,
                    event: create_key_event(&event, false, false),
                    is_synthetic: false,
                });
            }
        }

        #[method(flagsChanged:)]
        fn flags_changed(&self, event: &NSEvent) {
            trace_scope!("flagsChanged:");

            self.update_modifiers(event, true);
        }

        #[method(insertTab:)]
        fn insert_tab(&self, _sender: Option<&AnyObject>) {
            trace_scope!("insertTab:");
            let window = self.window();
            if let Some(first_responder) = window.firstResponder() {
                if *first_responder == ***self {
                    window.selectNextKeyView(Some(self))
                }
            }
        }

        #[method(insertBackTab:)]
        fn insert_back_tab(&self, _sender: Option<&AnyObject>) {
            trace_scope!("insertBackTab:");
            let window = self.window();
            if let Some(first_responder) = window.firstResponder() {
                if *first_responder == ***self {
                    window.selectPreviousKeyView(Some(self))
                }
            }
        }

        // Allows us to receive Cmd-. (the shortcut for closing a dialog)
        // https://bugs.eclipse.org/bugs/show_bug.cgi?id=300620#c6
        #[method(cancelOperation:)]
        fn cancel_operation(&self, _sender: Option<&AnyObject>) {
            let mtm = MainThreadMarker::from(self);
            trace_scope!("cancelOperation:");

            let event = NSApplication::sharedApplication(mtm)
                .currentEvent()
                .expect("could not find current event");

            self.update_modifiers(&event, false);
            let event = create_key_event(&event, true, unsafe { event.isARepeat() });

            self.queue_event(WindowEvent::KeyboardInput {
                device_id: DEVICE_ID,
                event,
                is_synthetic: false,
            });
        }

        // In the past (?), `mouseMoved:` events were not generated when the
        // user hovered over a window from a separate window, and as such the
        // application might not know the location of the mouse in the event.
        //
        // To fix this, we emit `mouse_motion` inside of mouse click, mouse
        // scroll, magnify and other gesture event handlers, to ensure that
        // the application's state of where the mouse click was located is up
        // to date.
        //
        // See https://github.com/rust-windowing/winit/pull/1490 for history.

        #[method(mouseDown:)]
        fn mouse_down(&self, event: &NSEvent) {
            trace_scope!("mouseDown:");
            self.mouse_motion(event);
            self.mouse_click(event, ElementState::Pressed);
        }

        #[method(mouseUp:)]
        fn mouse_up(&self, event: &NSEvent) {
            trace_scope!("mouseUp:");
            self.mouse_motion(event);
            self.mouse_click(event, ElementState::Released);
        }

        #[method(rightMouseDown:)]
        fn right_mouse_down(&self, event: &NSEvent) {
            trace_scope!("rightMouseDown:");
            self.mouse_motion(event);
            self.mouse_click(event, ElementState::Pressed);
        }

        #[method(rightMouseUp:)]
        fn right_mouse_up(&self, event: &NSEvent) {
            trace_scope!("rightMouseUp:");
            self.mouse_motion(event);
            self.mouse_click(event, ElementState::Released);
        }

        #[method(otherMouseDown:)]
        fn other_mouse_down(&self, event: &NSEvent) {
            trace_scope!("otherMouseDown:");
            self.mouse_motion(event);
            self.mouse_click(event, ElementState::Pressed);
        }

        #[method(otherMouseUp:)]
        fn other_mouse_up(&self, event: &NSEvent) {
            trace_scope!("otherMouseUp:");
            self.mouse_motion(event);
            self.mouse_click(event, ElementState::Released);
        }

        // No tracing on these because that would be overly verbose

        #[method(mouseMoved:)]
        fn mouse_moved(&self, event: &NSEvent) {
            self.mouse_motion(event);
        }

        #[method(mouseDragged:)]
        fn mouse_dragged(&self, event: &NSEvent) {
            self.mouse_motion(event);
        }

        #[method(rightMouseDragged:)]
        fn right_mouse_dragged(&self, event: &NSEvent) {
            self.mouse_motion(event);
        }

        #[method(otherMouseDragged:)]
        fn other_mouse_dragged(&self, event: &NSEvent) {
            self.mouse_motion(event);
        }

        #[method(mouseEntered:)]
        fn mouse_entered(&self, _event: &NSEvent) {
            trace_scope!("mouseEntered:");
            self.queue_event(WindowEvent::CursorEntered {
                device_id: DEVICE_ID,
            });
        }

        #[method(mouseExited:)]
        fn mouse_exited(&self, _event: &NSEvent) {
            trace_scope!("mouseExited:");

            self.queue_event(WindowEvent::CursorLeft {
                device_id: DEVICE_ID,
            });
        }

        #[method(scrollWheel:)]
        fn scroll_wheel(&self, event: &NSEvent) {
            trace_scope!("scrollWheel:");

            self.mouse_motion(event);

            let delta = {
                let (x, y) = unsafe { (event.scrollingDeltaX(), event.scrollingDeltaY()) };
                if unsafe { event.hasPreciseScrollingDeltas() } {
                    let delta = LogicalPosition::new(x, y).to_physical(self.scale_factor());
                    MouseScrollDelta::PixelDelta(delta)
                } else {
                    MouseScrollDelta::LineDelta(x as f32, y as f32)
                }
            };

            // The "momentum phase," if any, has higher priority than touch phase (the two should
            // be mutually exclusive anyhow, which is why the API is rather incoherent). If no momentum
            // phase is recorded (or rather, the started/ended cases of the momentum phase) then we
            // report the touch phase.
            #[allow(non_upper_case_globals)]
            let phase = match unsafe { event.momentumPhase() } {
                NSEventPhase::MayBegin | NSEventPhase::Began => TouchPhase::Started,
                NSEventPhase::Ended | NSEventPhase::Cancelled => TouchPhase::Ended,
                _ => match unsafe { event.phase() } {
                    NSEventPhase::MayBegin | NSEventPhase::Began => TouchPhase::Started,
                    NSEventPhase::Ended | NSEventPhase::Cancelled => TouchPhase::Ended,
                    _ => TouchPhase::Moved,
                },
            };

            self.update_modifiers(event, false);

            self.ivars().app_delegate.maybe_queue_device_event(DeviceEvent::MouseWheel { delta });
            self.queue_event(WindowEvent::MouseWheel {
                device_id: DEVICE_ID,
                delta,
                phase,
            });
        }

        #[method(magnifyWithEvent:)]
        fn magnify_with_event(&self, event: &NSEvent) {
            trace_scope!("magnifyWithEvent:");

            self.mouse_motion(event);

            #[allow(non_upper_case_globals)]
            let phase = match unsafe { event.phase() } {
                NSEventPhase::Began => TouchPhase::Started,
                NSEventPhase::Changed => TouchPhase::Moved,
                NSEventPhase::Cancelled => TouchPhase::Cancelled,
                NSEventPhase::Ended => TouchPhase::Ended,
                _ => return,
            };

            self.queue_event(WindowEvent::PinchGesture {
                device_id: DEVICE_ID,
                delta: unsafe { event.magnification() },
                phase,
            });
        }

        #[method(smartMagnifyWithEvent:)]
        fn smart_magnify_with_event(&self, event: &NSEvent) {
            trace_scope!("smartMagnifyWithEvent:");

            self.mouse_motion(event);

            self.queue_event(WindowEvent::DoubleTapGesture {
                device_id: DEVICE_ID,
            });
        }

        #[method(rotateWithEvent:)]
        fn rotate_with_event(&self, event: &NSEvent) {
            trace_scope!("rotateWithEvent:");

            self.mouse_motion(event);

            #[allow(non_upper_case_globals)]
            let phase = match unsafe { event.phase() } {
                NSEventPhase::Began => TouchPhase::Started,
                NSEventPhase::Changed => TouchPhase::Moved,
                NSEventPhase::Cancelled => TouchPhase::Cancelled,
                NSEventPhase::Ended => TouchPhase::Ended,
                _ => return,
            };

            self.queue_event(WindowEvent::RotationGesture {
                device_id: DEVICE_ID,
                delta: unsafe { event.rotation() },
                phase,
            });
        }

        #[method(pressureChangeWithEvent:)]
        fn pressure_change_with_event(&self, event: &NSEvent) {
            trace_scope!("pressureChangeWithEvent:");

            self.queue_event(WindowEvent::TouchpadPressure {
                device_id: DEVICE_ID,
                pressure: unsafe { event.pressure() },
                stage: unsafe { event.stage() } as i64,
            });
        }

        // Allows us to receive Ctrl-Tab and Ctrl-Esc.
        // Note that this *doesn't* help with any missing Cmd inputs.
        // https://github.com/chromium/chromium/blob/a86a8a6bcfa438fa3ac2eba6f02b3ad1f8e0756f/ui/views/cocoa/bridged_content_view.mm#L816
        #[method(_wantsKeyDownForEvent:)]
        fn wants_key_down_for_event(&self, _event: &NSEvent) -> bool {
            trace_scope!("_wantsKeyDownForEvent:");
            true
        }

        #[method(acceptsFirstMouse:)]
        fn accepts_first_mouse(&self, _event: &NSEvent) -> bool {
            trace_scope!("acceptsFirstMouse:");
            self.ivars().accepts_first_mouse
        }
    }
);

impl WinitView {
    pub(super) fn new(
        app_delegate: &ApplicationDelegate,
        window: &WinitWindow,
        accepts_first_mouse: bool,
        option_as_alt: OptionAsAlt,
    ) -> Retained<Self> {
        let mtm = MainThreadMarker::from(window);
        let this = mtm.alloc().set_ivars(ViewState {
            app_delegate: app_delegate.retain(),
            cursor_state: Default::default(),
            ime_position: Default::default(),
            ime_size: Default::default(),
            modifiers: Default::default(),
            phys_modifiers: Default::default(),
            tracking_rect: Default::default(),
            ime_state: Default::default(),
            input_source: Default::default(),
            ime_allowed: Default::default(),
            forward_key_to_app: Default::default(),
            marked_text: Default::default(),
            marked_location: Cell::new(0),
            marked_selected: Cell::new(NSRange::new(0, 0)),
            marked_replaced: RefCell::new(String::new()),
            in_key_event: Cell::new(false),
            key_text: RefCell::new(String::new()),
            accepts_first_mouse,
            _ns_window: WeakId::new(&window.retain()),
            option_as_alt: Cell::new(option_as_alt),
        });
        let this: Retained<Self> = unsafe { msg_send_id![super(this), init] };

        this.setPostsFrameChangedNotifications(true);
        let notification_center = unsafe { NSNotificationCenter::defaultCenter() };
        unsafe {
            notification_center.addObserver_selector_name_object(
                &this,
                sel!(frameDidChange:),
                Some(NSViewFrameDidChangeNotification),
                Some(&this),
            )
        }

        *this.ivars().input_source.borrow_mut() = this.current_input_source();

        this
    }

    fn window(&self) -> Retained<WinitWindow> {
        // TODO: Simply use `window` property on `NSView`.
        // That only returns a window _after_ the view has been attached though!
        // (which is incompatible with `frameDidChange:`)
        //
        // unsafe { msg_send_id![self, window] }
        self.ivars()._ns_window.load().expect("view to have a window")
    }

    fn queue_event(&self, event: WindowEvent) {
        self.ivars().app_delegate.maybe_queue_window_event(self.window().id(), event);
    }

    fn scale_factor(&self) -> f64 {
        self.window().backingScaleFactor() as f64
    }

    fn is_ime_enabled(&self) -> bool {
        !matches!(self.ivars().ime_state.get(), ImeState::Disabled)
    }

    fn current_input_source(&self) -> String {
        self.inputContext()
            .expect("input context")
            .selectedKeyboardInputSource()
            .map(|input_source| input_source.to_string())
            .unwrap_or_default()
    }

    pub(super) fn cursor_icon(&self) -> Retained<NSCursor> {
        self.ivars().cursor_state.borrow().cursor.clone()
    }

    pub(super) fn set_cursor_icon(&self, icon: Retained<NSCursor>) {
        let mut cursor_state = self.ivars().cursor_state.borrow_mut();
        cursor_state.cursor = icon;
    }

    /// Set whether the cursor should be visible or not.
    ///
    /// Returns whether the state changed.
    pub(super) fn set_cursor_visible(&self, visible: bool) -> bool {
        let mut cursor_state = self.ivars().cursor_state.borrow_mut();
        if visible != cursor_state.visible {
            cursor_state.visible = visible;
            true
        } else {
            false
        }
    }

    pub(super) fn set_ime_allowed(&self, ime_allowed: bool) {
        if self.ivars().ime_allowed.get() == ime_allowed {
            return;
        }
        self.ivars().ime_allowed.set(ime_allowed);
        ime_trace(|| format!("setImeAllowed {ime_allowed} marked {:?}", self.ivars().marked_text.borrow().string().to_string()));
        if self.ivars().ime_allowed.get() {
            return;
        }

        // WordCraft patch: marked text the application stops taking input for is committed, as
        // `unmarkText` commits it, and the input method is told it's gone; it was dropped on this
        // side only, so the input method's later commit of it found no marked text here and was
        // lost.
        let committed = self.ivars().marked_text.borrow().string().to_string();
        if !committed.is_empty() {
            let range = self.ime_edit_range(NSRange::new(NSNotFound as NSUInteger, 0));
            self.ime_update_surrounding(range, &committed, None);
            if let Some(input_context) = self.inputContext() {
                input_context.discardMarkedText();
            }
            self.queue_event(WindowEvent::Ime(Ime::Preedit(String::new(), None)));
            self.queue_event(WindowEvent::Ime(Ime::Commit(committed)));
        }
        // Clear markedText
        *self.ivars().marked_text.borrow_mut() = NSMutableAttributedString::new();

        if self.ivars().ime_state.get() != ImeState::Disabled {
            self.ivars().ime_state.set(ImeState::Disabled);
            self.queue_event(WindowEvent::Ime(Ime::Disabled));
        }
    }

    pub(super) fn set_ime_cursor_area(&self, position: NSPoint, size: NSSize) {
        self.ivars().ime_position.set(position);
        self.ivars().ime_size.set(size);
        let input_context = self.inputContext().expect("input context");
        input_context.invalidateCharacterCoordinates();
    }

    /// Reset modifiers and emit a synthetic ModifiersChanged event if deemed necessary.
    pub(super) fn reset_modifiers(&self) {
        if !self.ivars().modifiers.get().state().is_empty() {
            self.ivars().modifiers.set(Modifiers::default());
            self.queue_event(WindowEvent::ModifiersChanged(self.ivars().modifiers.get()));
        }
    }

    pub(super) fn set_option_as_alt(&self, value: OptionAsAlt) {
        self.ivars().option_as_alt.set(value)
    }

    pub(super) fn option_as_alt(&self) -> OptionAsAlt {
        self.ivars().option_as_alt.get()
    }

    /// Update modifiers if `event` has something different
    fn update_modifiers(&self, ns_event: &NSEvent, is_flags_changed_event: bool) {
        use ElementState::{Pressed, Released};

        let current_modifiers = event_mods(ns_event);
        let prev_modifiers = self.ivars().modifiers.get();
        self.ivars().modifiers.set(current_modifiers);

        // This function was called form the flagsChanged event, which is triggered
        // when the user presses/releases a modifier even if the same kind of modifier
        // has already been pressed.
        //
        // When flags changed event has key code of zero it means that event doesn't carry any key
        // event, thus we can't generate regular presses based on that. The `ModifiersChanged`
        // later will work though, since the flags are attached to the event and contain valid
        // information.
        'send_event: {
            if is_flags_changed_event && unsafe { ns_event.keyCode() } != 0 {
                let scancode = unsafe { ns_event.keyCode() };
                let physical_key = scancode_to_physicalkey(scancode as u32);

                let logical_key = code_to_key(physical_key, scancode);
                // Ignore processing of unknown modifiers because we can't determine whether
                // it was pressed or release reliably.
                //
                // Furthermore, sometimes normal keys are reported inside flagsChanged:, such as
                // when holding Caps Lock while pressing another key, see:
                // https://github.com/alacritty/alacritty/issues/8268
                let Some(event_modifier) = key_to_modifier(&logical_key) else {
                    break 'send_event;
                };

                let mut event = KeyEvent {
                    location: code_to_location(physical_key),
                    logical_key: logical_key.clone(),
                    physical_key,
                    repeat: false,
                    // We'll correct this later.
                    state: Pressed,
                    text: None,
                    platform_specific: KeyEventExtra {
                        text_with_all_modifiers: None,
                        key_without_modifiers: logical_key.clone(),
                    },
                };

                let location_mask = ModLocationMask::from_location(event.location);

                let mut phys_mod_state = self.ivars().phys_modifiers.borrow_mut();
                let phys_mod =
                    phys_mod_state.entry(logical_key).or_insert(ModLocationMask::empty());

                let is_active = current_modifiers.state().contains(event_modifier);
                let mut events = VecDeque::with_capacity(2);

                // There is no API for getting whether the button was pressed or released
                // during this event. For this reason we have to do a bit of magic below
                // to come up with a good guess whether this key was pressed or released.
                // (This is not trivial because there are multiple buttons that may affect
                // the same modifier)
                if !is_active {
                    event.state = Released;
                    if phys_mod.contains(ModLocationMask::LEFT) {
                        let mut event = event.clone();
                        event.location = KeyLocation::Left;
                        event.physical_key = get_left_modifier_code(&event.logical_key).into();
                        events.push_back(WindowEvent::KeyboardInput {
                            device_id: DEVICE_ID,
                            event,
                            is_synthetic: false,
                        });
                    }
                    if phys_mod.contains(ModLocationMask::RIGHT) {
                        event.location = KeyLocation::Right;
                        event.physical_key = get_right_modifier_code(&event.logical_key).into();
                        events.push_back(WindowEvent::KeyboardInput {
                            device_id: DEVICE_ID,
                            event,
                            is_synthetic: false,
                        });
                    }
                    *phys_mod = ModLocationMask::empty();
                } else {
                    if *phys_mod == location_mask {
                        // Here we hit a contradiction:
                        // The modifier state was "changed" to active,
                        // yet the only pressed modifier key was the one that we
                        // just got a change event for.
                        // This seemingly means that the only pressed modifier is now released,
                        // but at the same time the modifier became active.
                        //
                        // But this scenario is possible if we released modifiers
                        // while the application was not in focus. (Because we don't
                        // get informed of modifier key events while the application
                        // is not focused)

                        // In this case we prioritize the information
                        // about the current modifier state which means
                        // that the button was pressed.
                        event.state = Pressed;
                    } else {
                        phys_mod.toggle(location_mask);
                        let is_pressed = phys_mod.contains(location_mask);
                        event.state = if is_pressed { Pressed } else { Released };
                    }

                    events.push_back(WindowEvent::KeyboardInput {
                        device_id: DEVICE_ID,
                        event,
                        is_synthetic: false,
                    });
                }

                drop(phys_mod_state);

                for event in events {
                    self.queue_event(event);
                }
            }
        }

        if prev_modifiers == current_modifiers {
            return;
        }

        self.queue_event(WindowEvent::ModifiersChanged(self.ivars().modifiers.get()));
    }

    fn mouse_click(&self, event: &NSEvent, button_state: ElementState) {
        let button = mouse_button(event);

        self.update_modifiers(event, false);

        self.queue_event(WindowEvent::MouseInput {
            device_id: DEVICE_ID,
            state: button_state,
            button,
        });
    }

    fn mouse_motion(&self, event: &NSEvent) {
        let window_point = unsafe { event.locationInWindow() };
        let view_point = self.convertPoint_fromView(window_point, None);
        let frame = self.frame();

        if view_point.x.is_sign_negative()
            || view_point.y.is_sign_negative()
            || view_point.x > frame.size.width
            || view_point.y > frame.size.height
        {
            let mouse_buttons_down = unsafe { NSEvent::pressedMouseButtons() };
            if mouse_buttons_down == 0 {
                // Point is outside of the client area (view) and no buttons are pressed
                return;
            }
        }

        let view_point = LogicalPosition::new(view_point.x, view_point.y);

        self.update_modifiers(event, false);

        self.queue_event(WindowEvent::CursorMoved {
            device_id: DEVICE_ID,
            position: view_point.to_physical(self.scale_factor()),
        });
    }
}

/// Get the mouse button from the NSEvent.
fn mouse_button(event: &NSEvent) -> MouseButton {
    // The buttonNumber property only makes sense for the mouse events:
    // NSLeftMouse.../NSRightMouse.../NSOtherMouse...
    // For the other events, it's always set to 0.
    // MacOS only defines the left, right and middle buttons, 3..=31 are left as generic buttons,
    // but 3 and 4 are very commonly used as Back and Forward by hardware vendors and applications.
    match unsafe { event.buttonNumber() } {
        0 => MouseButton::Left,
        1 => MouseButton::Right,
        2 => MouseButton::Middle,
        3 => MouseButton::Back,
        4 => MouseButton::Forward,
        n => MouseButton::Other(n as u16),
    }
}

// NOTE: to get option as alt working we need to rewrite events
// we're getting from the operating system, which makes it
// impossible to provide such events as extra in `KeyEvent`.
fn replace_event(event: &NSEvent, option_as_alt: OptionAsAlt) -> Retained<NSEvent> {
    let ev_mods = event_mods(event).state;
    let ignore_alt_characters = match option_as_alt {
        OptionAsAlt::OnlyLeft if lalt_pressed(event) => true,
        OptionAsAlt::OnlyRight if ralt_pressed(event) => true,
        OptionAsAlt::Both if ev_mods.alt_key() => true,
        _ => false,
    } && !ev_mods.control_key()
        && !ev_mods.super_key();

    if ignore_alt_characters {
        let ns_chars = unsafe {
            event.charactersIgnoringModifiers().expect("expected characters to be non-null")
        };

        unsafe {
            NSEvent::keyEventWithType_location_modifierFlags_timestamp_windowNumber_context_characters_charactersIgnoringModifiers_isARepeat_keyCode(
                event.r#type(),
                event.locationInWindow(),
                event.modifierFlags(),
                event.timestamp(),
                event.windowNumber(),
                None,
                &ns_chars,
                &ns_chars,
                event.isARepeat(),
                event.keyCode(),
            )
            .unwrap()
        }
    } else {
        event.copy()
    }
}

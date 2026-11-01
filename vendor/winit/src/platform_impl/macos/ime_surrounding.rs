//! WordCraft patch (vendor/patches/winit/01-macos-ime-surrounding-text.md): the text around the
//! caret, given by the application, so the input method can read committed text and ask for a
//! range of it to be replaced (再変換 — reconversion of a selection — and context for live
//! conversion). One window takes keyboard input at a time, so a process-wide store suffices and
//! the application needs no handle to the window (eframe keeps it).
//!
//! While the application reports text here, `WinitView` answers `NSTextInputClient` from it
//! instead of from `ImeDocument` (the text typed since the caret last moved, which is all the
//! view knows otherwise): the input method then counts in the application's paragraph, and a
//! replacement range it names is either handed back for the next `Ime::Preedit`
//! (`take_ime_replacement_range`, marked text) or turned into the deletion that goes with the
//! commit (`ImeDeleteSurrounding`, text inserted without marked text).
//!
//! Offsets are UTF-16 code units, as in `NSRange`.

use std::ops::Range;
use std::sync::Mutex;

use crate::platform::macos::ImeDeleteSurrounding;

/// The text around the caret and the selection within it.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ImeSurroundingText {
    /// The text the input method may read (the caret's paragraph, say), UTF-8.
    pub text: String,
    /// The selection within `text` in UTF-16 code units; collapsed, it is the caret.
    pub selected: Range<usize>,
}

/// The marked text's place in the surrounding text — where it lies and its length, in UTF-16
/// code units, and the committed text it stands for (a reconverted selection, or nothing).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ImeMarked {
    pub location: usize,
    pub len: usize,
    pub replaced: String,
}

impl ImeSurroundingText {
    /// This text as the input method has just changed it, until the application reports it
    /// again: `text` put in place of `range` — UTF-16 code units of this text, the marked text
    /// `marked` included, as the input method counts — as marked text with the input method's
    /// selection `selected` within it, or committed (`None`). A range reaching into the marked
    /// text counts it as the committed text it stands for. Empty marked text takes the marked
    /// text out and selects what it stood for, as the application does. Returns the new text
    /// with its selection, and the marked text's place in it, if any.
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
            },
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
            Some(_) if n == 0 => (
                ImeSurroundingText { text: String::from_utf16_lossy(&units), selected: start..end },
                None,
            ),
            Some(sel) => {
                units.splice(start..end, new);
                let a = sel.start.min(n);
                let b = sel.end.clamp(a, n);
                let text = String::from_utf16_lossy(&units);
                (
                    ImeSurroundingText { text, selected: start + a..start + b },
                    Some(ImeMarked { location: start, len: n, replaced }),
                )
            },
            None => {
                units.splice(start..end, new);
                let caret = start + n;
                (
                    ImeSurroundingText {
                        text: String::from_utf16_lossy(&units),
                        selected: caret..caret,
                    },
                    None,
                )
            },
        }
    }

    /// The selection, clamped to the text and ordered.
    pub fn selection(&self) -> Range<usize> {
        let len = self.text.encode_utf16().count();
        let start = self.selected.start.min(self.selected.end).min(len);
        start..self.selected.end.max(self.selected.start).clamp(start, len)
    }

    /// The deletion that goes with text an input method inserts without marked text in place of
    /// `range` (press-and-hold's accent picked with a digit, a dictation correction): the text of
    /// `range` before the selection and after it, in UTF-8 bytes — the selection itself is
    /// replaced by the commit anyway. `None` when `range` doesn't take in the selection, or
    /// deletes nothing.
    pub fn deletion(&self, range: Range<usize>) -> Option<ImeDeleteSurrounding> {
        let sel = self.selection();
        if range.start > sel.start || range.end < sel.end {
            return None;
        }
        // A range starting or ending inside a character takes the whole character.
        let from = byte_at(&self.text, range.start, false);
        let to = byte_at(&self.text, range.end, true);
        let sel_from = byte_at(&self.text, sel.start, false);
        let sel_to = byte_at(&self.text, sel.end, true).max(sel_from);
        let delete = ImeDeleteSurrounding {
            before_bytes: sel_from.saturating_sub(from),
            after_bytes: to.saturating_sub(sel_to),
        };
        (delete.before_bytes > 0 || delete.after_bytes > 0).then_some(delete)
    }
}

/// The byte offset of `text` at `units` UTF-16 code units, clamped to the text; inside a
/// character, the boundary before it (`up`: after it).
fn byte_at(text: &str, units: usize, up: bool) -> usize {
    let mut seen = 0;
    for (i, c) in text.char_indices() {
        if seen >= units {
            return i;
        }
        let next = seen + c.len_utf16();
        if next > units {
            return if up { i + c.len_utf8() } else { i };
        }
        seen = next;
    }
    text.len()
}

/// `range` of the UTF-16 form of `text`, clamped to it, with the range it actually covers.
pub(super) fn utf16_slice(text: &str, range: Range<usize>) -> (String, (usize, usize)) {
    let units: Vec<u16> = text.encode_utf16().collect();
    let start = range.start.min(units.len());
    let end = range.end.clamp(start, units.len());
    (String::from_utf16_lossy(&units[start..end]), (start, end - start))
}

static IME_SURROUNDING: Mutex<Option<ImeSurroundingText>> = Mutex::new(None);
static IME_REPLACEMENT: Mutex<Option<Range<usize>>> = Mutex::new(None);

/// Tell the input method what text lies around the caret (`None`: nothing, as before the patch).
pub fn set_ime_surrounding_text(text: Option<ImeSurroundingText>) {
    if let Ok(mut s) = IME_SURROUNDING.lock() {
        *s = text;
    }
}

/// What the input method is told lies around the caret right now.
pub fn ime_surrounding_text() -> Option<ImeSurroundingText> {
    IME_SURROUNDING.lock().ok().and_then(|s| s.clone())
}

/// The range of the surrounding text (UTF-16 code units) the input method asked to replace with
/// its marked text, if `setMarkedText:selectedRange:replacementRange:` named one since the last
/// call; the `Ime::Preedit` event carrying that marked text follows.
pub fn take_ime_replacement_range() -> Option<Range<usize>> {
    IME_REPLACEMENT.lock().ok().and_then(|mut r| r.take())
}

pub(super) fn set_ime_replacement_range(range: Range<usize>) {
    if let Ok(mut r) = IME_REPLACEMENT.lock() {
        *r = Some(range);
    }
}

/// A record of what the input method asks of the view, written when `WORDCRAFT_IME_TRACE` names
/// a file (each call, its text and ranges, the IME state) — to see an input method's real calls,
/// which can't be reproduced without typing.
pub(super) fn ime_trace(line: impl FnOnce() -> String) {
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

#[cfg(test)]
mod tests {
    use super::*;

    fn at(text: &str, selected: Range<usize>) -> ImeSurroundingText {
        ImeSurroundingText { text: text.to_owned(), selected }
    }

    #[test]
    fn an_inserted_replacement_deletes_around_the_caret_in_bytes() {
        // "café|": press-and-hold's è picked with a digit replaces é (3..4).
        let s = at("café", 4..4);
        assert_eq!(s.deletion(3..4), Some(ImeDeleteSurrounding { before_bytes: 2, after_bytes: 0 }));
        // A dictation correction around the caret: "日本|語" replacing 本語.
        let s = at("日本語", 2..2);
        assert_eq!(s.deletion(1..3), Some(ImeDeleteSurrounding { before_bytes: 3, after_bytes: 3 }));
        // The selection itself is replaced by the commit: nothing more to delete.
        let s = at("日本語", 1..3);
        assert_eq!(s.deletion(1..3), None);
        assert_eq!(s.deletion(0..3), Some(ImeDeleteSurrounding { before_bytes: 3, after_bytes: 0 }));
        // A range away from the caret can't be deleted relative to it.
        let s = at("日本語", 3..3);
        assert_eq!(s.deletion(0..1), None);
        // A surrogate pair counts two units; a range into it takes the whole character.
        let s = at("a𠮷", 3..3);
        assert_eq!(s.deletion(2..3), Some(ImeDeleteSurrounding { before_bytes: 4, after_bytes: 0 }));
        // Hostile ranges are clamped.
        let s = at("ab", 9..9);
        assert_eq!(s.deletion(0..usize::MAX), Some(ImeDeleteSurrounding { before_bytes: 2, after_bytes: 0 }));
    }

    #[test]
    fn slices_are_clamped_to_the_text() {
        assert_eq!(utf16_slice("a日本", 1..9), ("日本".to_owned(), (1, 2)));
        assert_eq!(utf16_slice("a日本", 9..12), (String::new(), (3, 0)));
    }
}

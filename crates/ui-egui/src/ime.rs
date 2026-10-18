//! What the input method may read of the document: the caret's paragraph and the selection in
//! it, reported to the platform each frame (`CanvasState::ime_surrounding`), and the range of it
//! the input method asks to replace with its composition (`CanvasState::ime_replace`).
//!
//! Word lets the input method read committed text: on macOS a selection can be reconverted
//! (再変換 — the Japanese input method marks the selected text's reading and offers candidates
//! for it) and live conversion reads the words before the caret. Both go through
//! `NSTextInputClient`'s `selectedRange` and `attributedSubstringForProposedRange:actualRange:`,
//! which winit answers with nothing; the vendored winit (`vendor/patches/winit/01-macos-ime-
//! surrounding-text.md`) answers from what the desktop app reports here, and hands the
//! replacement range of `setMarkedText:selectedRange:replacementRange:` back, which
//! [`select_range`] turns into the selection the composition then takes the place of
//! (`Session::set_composition`).
//!
//! Ranges are in UTF-16 code units, as the platform counts them.

use std::ops::Range;

use wordcraft_engine::{Selection, Session};

/// The text around the caret and the selection within it (UTF-16 code units).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ImeSurrounding {
    /// The caret's paragraph (objects as U+FFFC, as the document holds them).
    pub text: String,
    /// The selection, or the caret when collapsed.
    pub selected: Range<usize>,
}

/// The caret's paragraph and the selection within it. A selection reaching outside the paragraph
/// is reported as the caret (the input method could not replace it in one piece anyway).
pub fn surrounding(session: &Session) -> Option<ImeSurrounding> {
    if session.math.is_some() {
        return None;
    }
    let focus = &session.sel.focus;
    let para = session.doc.para_at(focus)?;
    let text = para.text.clone();
    let (a, b) = session.sel.ordered();
    let same = a.story == b.story && a.path == b.path;
    let (start, end) = if same { (a.off, b.off) } else { (focus.off, focus.off) };
    let start = byte_to_utf16(&text, start);
    let end = byte_to_utf16(&text, end).max(start);
    Some(ImeSurrounding { text, selected: start..end })
}

/// Make `range` (UTF-16 code units of the caret's paragraph) the selection, so the composition
/// takes its place; false (and nothing changes) when it doesn't fit the paragraph.
pub fn select_range(session: &mut Session, range: Range<usize>) -> bool {
    let focus = session.sel.focus.clone();
    let Some(para) = session.doc.para_at(&focus) else { return false };
    let units = para.text.encode_utf16().count();
    if range.start > range.end || range.end > units {
        return false;
    }
    let a = utf16_to_byte(&para.text, range.start);
    let b = utf16_to_byte(&para.text, range.end);
    session.sel = Selection { anchor: wordcraft_doc::Pos { off: a, ..focus.clone() }, focus: wordcraft_doc::Pos { off: b, ..focus } };
    true
}

/// UTF-16 code units before byte offset `byte` of `text` (clamped to the text, and to a
/// character boundary).
pub fn byte_to_utf16(text: &str, byte: usize) -> usize {
    let mut byte = byte.min(text.len());
    while !text.is_char_boundary(byte) {
        byte -= 1;
    }
    text.get(..byte).map_or(0, |s| s.encode_utf16().count())
}

/// The byte offset of `text` after `units` UTF-16 code units (clamped to the text; a unit inside
/// a surrogate pair lands after the pair).
pub fn utf16_to_byte(text: &str, units: usize) -> usize {
    let mut seen = 0;
    for (i, c) in text.char_indices() {
        if seen >= units {
            return i;
        }
        seen += c.len_utf16();
    }
    text.len()
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn utf16_offsets_round_trip() {
        let t = "a𠮷b";
        assert_eq!(byte_to_utf16(t, 0), 0);
        assert_eq!(byte_to_utf16(t, 1), 1);
        assert_eq!(byte_to_utf16(t, 5), 3);
        assert_eq!(byte_to_utf16(t, 6), 4);
        assert_eq!(byte_to_utf16(t, 2), 1, "inside a character: the boundary before");
        assert_eq!(byte_to_utf16(t, 99), 4);
        assert_eq!(utf16_to_byte(t, 0), 0);
        assert_eq!(utf16_to_byte(t, 1), 1);
        assert_eq!(utf16_to_byte(t, 2), 5, "half a surrogate pair: after it");
        assert_eq!(utf16_to_byte(t, 3), 5);
        assert_eq!(utf16_to_byte(t, 4), 6);
        assert_eq!(utf16_to_byte(t, 99), 6);
        assert_eq!(byte_to_utf16("", 3), 0);
        assert_eq!(utf16_to_byte("", 3), 0);
    }

    #[test]
    fn the_input_method_reads_the_paragraph_and_the_selection_in_it() {
        let mut s = Session::new(wordcraft_doc::Document::from_text("a日本語b\ncd"));
        s.run("select.text", &json!({"text": "日本語"})).unwrap();
        assert_eq!(surrounding(&s), Some(ImeSurrounding { text: "a日本語b".into(), selected: 1..4 }));
        s.run("caret.end", &json!({})).unwrap();
        assert_eq!(surrounding(&s), Some(ImeSurrounding { text: "a日本語b".into(), selected: 5..5 }));
        // Across paragraphs: the caret only.
        s.run("caret.down", &json!({"extend": true})).unwrap();
        let r = surrounding(&s).unwrap();
        assert_eq!(r.text, "cd");
        assert!(r.selected.is_empty());
        // Reconversion: the range the input method names becomes the selection.
        s.run("caret.up", &json!({})).unwrap();
        assert!(select_range(&mut s, 1..4));
        assert_eq!(s.selected_text(), "日本語");
        assert!(!select_range(&mut s, 3..9), "outside the paragraph");
        assert!(!select_range(&mut s, std::ops::Range { start: 4, end: 1 }), "reversed");
        assert_eq!(s.selected_text(), "日本語", "unchanged");
        assert!(select_range(&mut s, 5..5));
        assert!(s.sel.is_collapsed());
    }
}

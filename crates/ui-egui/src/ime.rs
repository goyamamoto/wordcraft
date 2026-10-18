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

/// The committed text a `range` the input method named stands for, with the composition taken
/// out of the document. The input method counts in the text it was shown, composition included;
/// while one is shown it may name a range reaching into committed text — live conversion marks
/// the words before its marked text again to convert them together, and a commit may name the
/// text it replaces — so the range is mapped through the composition: what lies before it keeps
/// its place, the composition itself stands for the text it replaced (a reconverted selection,
/// put back), what follows moves by the difference. Without a composition the range is as given.
pub fn committed_range(session: &mut Session, range: Range<usize>) -> Range<usize> {
    let Some(c) = session.composition().cloned() else { return range };
    let Some(para) = session.doc.para_at(&c.start) else { return range };
    let m = byte_to_utf16(&para.text, c.start.off);
    let l = byte_to_utf16(&para.text, c.end.off).saturating_sub(m);
    session.clear_composition();
    // The selection is back to what the composition replaced (within one paragraph), or the caret.
    let (a, b) = session.sel.ordered();
    let r = match session.doc.para_at(&a) {
        Some(p) if a.story == b.story && a.path == b.path => byte_to_utf16(&p.text, b.off).saturating_sub(byte_to_utf16(&p.text, a.off)),
        _ => 0,
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
    let start = map(range.start);
    start..map(range.end).max(start)
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

    #[test]
    fn the_input_method_reads_the_whole_paragraph_in_every_story() {
        // A long committed paragraph is reported whole, with the caret counted in UTF-16 units
        // (an object placeholder and a surrogate pair each as the platform counts them).
        let long: String = "日本語の長い文章です".repeat(300);
        let mut s = Session::new(wordcraft_doc::Document::from_text(&long));
        s.run("caret.docEnd", &json!({})).unwrap();
        let r = surrounding(&s).unwrap();
        assert_eq!(r.text, long);
        assert_eq!(r.selected, 3000..3000);
        s.run("caret.left", &json!({})).unwrap();
        assert_eq!(surrounding(&s).unwrap().selected, 2999..2999);
        // A table cell.
        let mut s = Session::new(wordcraft_doc::Document::new());
        s.run("insert.table", &json!({"rows": 2, "cols": 2})).unwrap();
        s.run("text.insert", &json!({"text": "セルの文"})).unwrap();
        assert!(s.sel.focus.path != wordcraft_doc::Path::top(0), "in a cell: {:?}", s.sel.focus);
        assert_eq!(surrounding(&s), Some(ImeSurrounding { text: "セルの文".into(), selected: 4..4 }));
        s.run("select.text", &json!({"text": "の文"})).unwrap();
        assert_eq!(surrounding(&s).unwrap().selected, 2..4);
        // A header.
        let mut s = Session::new(wordcraft_doc::Document::from_text("本文"));
        s.run("insert.header", &json!({"text": "ヘッダーの文"})).unwrap();
        assert!(matches!(s.sel.focus.story, wordcraft_doc::StoryRef::Part(_)), "{:?}", s.sel.focus);
        let r = surrounding(&s).unwrap();
        assert_eq!(r.text, "ヘッダーの文");
        assert!(r.selected.is_empty() && r.selected.start <= 6);
        // A text box, and the body paragraph holding its anchor (an object placeholder: one unit).
        let mut s = Session::new(wordcraft_doc::Document::from_text("𠮷野家"));
        s.run("caret.docEnd", &json!({})).unwrap();
        s.run("insert.textBox", &json!({"text": "箱の中の文"})).unwrap();
        if matches!(s.sel.focus.story, wordcraft_doc::StoryRef::Part(_)) {
            let r = surrounding(&s).unwrap();
            assert_eq!(r.text, "箱の中の文");
            assert!(r.selected.end <= 5);
        }
        s.run("caret.set", &json!({"pos": wordcraft_doc::Pos::body(0, 0)})).unwrap();
        s.run("caret.end", &json!({})).unwrap();
        let r = surrounding(&s).unwrap();
        assert!(r.text.starts_with("𠮷野家") && r.text.contains('\u{FFFC}'), "{:?}", r.text);
        assert_eq!(r.selected.start, r.text.encode_utf16().count(), "the caret counts the pair as two units and the anchor as one");
        assert!(select_range(&mut s, 0..2), "the pair");
        assert_eq!(s.selected_text(), "𠮷");
        // Editing an equation: nothing (the input method types into it directly).
        let mut s = Session::new(wordcraft_doc::Document::from_text("x"));
        s.run("insert.equation", &json!({})).unwrap();
        assert!(s.math.is_some());
        assert_eq!(surrounding(&s), None);
    }

    #[test]
    fn a_range_named_while_composing_maps_through_the_composition() {
        // "a日本b", the caret before b: the composition ご shows as a日本ごb (the input method's
        // text); a range it names there stands for committed text once the composition is out.
        let mut s = Session::new(wordcraft_doc::Document::from_text("a日本b"));
        s.run("caret.set", &json!({"pos": wordcraft_doc::Pos::body(0, 7)})).unwrap();
        assert_eq!(committed_range(&mut s, 1..3), 1..3, "no composition: as given");
        s.set_composition("ご", None).unwrap();
        assert_eq!(s.doc.plain_text(wordcraft_doc::StoryRef::Body), "a日本ごb");
        let r = surrounding(&s).unwrap();
        assert_eq!((r.text.as_str(), r.selected), ("a日本ごb", 4..4), "the composition is in the text the input method reads");
        // Live conversion marks 日本ご (1..4) again: 日本 (1..3) is the committed text it stands for.
        assert_eq!(committed_range(&mut s, 1..4), 1..3);
        assert!(s.composition().is_none() && s.doc.plain_text(wordcraft_doc::StoryRef::Body) == "a日本b");
        // After the composition: moved back by its length. Inside it: the (empty) text it replaced.
        s.set_composition("ご", None).unwrap();
        assert_eq!(committed_range(&mut s, 4..5), 3..4);
        s.set_composition("ごはん", None).unwrap();
        assert_eq!(committed_range(&mut s, 3..5), 3..3);
        // A composition that replaced a selection (再変換): it stands for that text, put back.
        s.run("select.text", &json!({"text": "日本"})).unwrap();
        s.set_composition("にほん", Some(0..3)).unwrap();
        assert_eq!(s.doc.plain_text(wordcraft_doc::StoryRef::Body), "aにほんb");
        assert_eq!(committed_range(&mut s, 0..4), 0..3, "a + the composition = a日本");
        assert_eq!(s.doc.plain_text(wordcraft_doc::StoryRef::Body), "a日本b");
        s.set_composition("にほん", Some(0..3)).unwrap();
        assert_eq!(committed_range(&mut s, 4..5), 3..4, "b after it");
        assert_eq!(s.selected_text(), "日本", "the selection it replaced is back");
    }
}

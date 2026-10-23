//! The input method's composition (変換前の文字: the text being typed and converted before it is
//! committed), shown inline in the document as Word does — at the caret, in the typing font,
//! underlined, the clause being converted with a thick underline — instead of beside it in the
//! interface font (which drew □ for Japanese whenever the CJK interface font was not loaded).
//!
//! The composition is provisional text: it never reaches the undo history, the dirty flag, the
//! file or the next command. [`Session::set_composition`] puts it at the caret (replacing the
//! previous composition), [`Session::clear_composition`] takes it out, and every non-pure command
//! takes it out before it runs (a committed composition arrives as `text.insert`, which goes
//! through typing: AutoCorrect, tracked changes, the undo step).
//!
//! A selection within one paragraph is replaced by the composition — as the input method expects
//! (its marked text takes the selection's place; on macOS, 再変換 of committed text selects it and
//! marks it) and as Word shows it. The selected text is put back when the composition goes, so a
//! dismissed composition leaves the document untouched, and the commit's `text.insert` replaces
//! the selection for good.

use wordcraft_doc::edit::Fragment;
use wordcraft_doc::props::Underline;
use wordcraft_doc::{CharProps, Pos};

use crate::{CmdError, Selection, Session};

/// Longest composition kept, in characters (input methods compose a clause or a sentence).
pub const MAX_COMPOSITION: usize = 1024;

/// Where the composition sits.
#[derive(Clone, Debug, PartialEq)]
pub struct Composition {
    /// Its first character.
    pub start: Pos,
    /// Its end (after the last character).
    pub end: Pos,
    /// The text shown.
    pub text: String,
    /// The selection before the composition began, put back when it ends.
    sel_before: Selection,
    /// Whether the document was dirty before (the composition never dirties it).
    dirty_before: bool,
    /// The selected text the composition took the place of (a selection within one paragraph),
    /// put back at `start` when the composition goes.
    replaced: Option<Fragment>,
    /// The formatting typing had where the composition began (the composition's own, less its
    /// underlines): what typing has while it shows, and what the commit is typed with.
    props: CharProps,
    /// The paragraph mark's formatting before the composition: put back when the composition
    /// goes, since emptying a paragraph gives its mark the deleted text's formatting — the
    /// composition's underline would stay on the mark, and on everything typed after it.
    mark_before: CharProps,
}

impl Session {
    /// The composition being shown, if any.
    pub fn composition(&self) -> Option<&Composition> {
        self.composition.as_ref()
    }

    /// Show `text` as the composition at the caret, replacing the one shown. `active` is the
    /// input method's selection in it (a character range of `text`): the clause being converted,
    /// drawn with a thick underline, or — empty — where the input method's own caret stands in
    /// text not yet converted (moved with the arrow keys, to type into the middle of the
    /// composition); the document's caret goes to its end. Empty text takes the composition out
    /// (the input method was dismissed).
    pub fn set_composition(&mut self, text: &str, active: Option<std::ops::Range<usize>>) -> Result<(), CmdError> {
        let text: String = text.chars().filter(|c| !c.is_control() || *c == '\t').take(MAX_COMPOSITION).collect();
        let (sel_before, dirty_before) = match self.composition.take() {
            Some(c) => {
                self.remove_composition(&c);
                (c.sel_before, c.dirty_before)
            }
            None => (self.sel.clone(), self.dirty),
        };
        if text.is_empty() {
            self.sel = sel_before;
            self.dirty = dirty_before;
            self.bump_rev();
            return Ok(());
        }
        // Where typing would go: the focus end of the selection, past any field end there — or,
        // for a selection within one paragraph, its start, in place of the selected text.
        let (a, b) = sel_before.ordered();
        let one_para = a != b && a.story == b.story && a.path == b.path;
        let start = if one_para { a.clone() } else { self.doc.past_field_ends(&sel_before.focus) };
        let replaced = if one_para {
            let frag = self.doc.copy_range(&a, &b);
            match self.doc.delete_range(&a, &b) {
                Ok(_) => Some(frag),
                Err(_) => None,
            }
        } else {
            None
        };
        let mut props = self.typing_props();
        props.fmt_change = None;
        props.ins = None;
        props.del = None;
        let mark_before = self.doc.para_at(&start).map(|p| p.mark.clone()).unwrap_or_default();
        let n = text.chars().count();
        let selected = active.map(|r| r.start.min(n)..r.end.min(n).max(r.start.min(n)));
        let active = selected.clone().filter(|r| r.start < r.end);
        // Up to three runs: before the clause, the clause (thick), after it.
        let mut at = start.clone();
        let mut parts: Vec<(String, bool)> = Vec::new();
        for (j, c) in text.chars().enumerate() {
            let thick = active.as_ref().is_some_and(|r| r.contains(&j));
            match parts.last_mut() {
                Some((s, t)) if *t == thick => s.push(c),
                _ => parts.push((c.to_string(), thick)),
            }
        }
        let result = (|| -> Result<Pos, CmdError> {
            for (part, thick) in &parts {
                let mut p = props.clone();
                p.underline = Some(if *thick { Underline::Thick } else { Underline::Single });
                at = self.doc.insert_text(&at, part, &p)?;
            }
            Ok(at.clone())
        })();
        match result {
            Ok(end) => {
                // The caret follows the input method's: the end of the clause being converted, or
                // its caret in text not yet converted (the candidate window opens there).
                let caret = match &selected {
                    Some(r) => Pos { off: start.off.saturating_add(text.chars().take(r.end).map(char::len_utf8).sum()), ..start.clone() },
                    None => end.clone(),
                };
                self.sel = Selection::caret(caret);
                self.composition = Some(Composition { start, end, text, sel_before, dirty_before, replaced, props, mark_before });
                self.dirty = dirty_before;
                self.bump_rev();
                Ok(())
            }
            Err(e) => {
                // Put back what went in before the failure, so nothing provisional stays.
                if at != start {
                    let _ = self.doc.delete_range(&start, &at);
                }
                if let Some(frag) = &replaced {
                    let _ = self.doc.insert_fragment(&start, frag);
                }
                if let Ok(p) = self.doc.para_mut(start.story, &start.path) {
                    p.mark = mark_before;
                }
                self.sel = sel_before;
                self.dirty = dirty_before;
                self.bump_rev();
                Err(e)
            }
        }
    }

    /// Take the composition out of the document (the input method committed or was dismissed);
    /// the selection is as it was before the composition began.
    pub fn clear_composition(&mut self) {
        if let Some(c) = self.composition.take() {
            self.remove_composition(&c);
            self.sel = c.sel_before;
            self.dirty = c.dirty_before;
            self.bump_rev();
        }
    }

    fn remove_composition(&mut self, c: &Composition) {
        let _ = self.doc.delete_range(&c.start, &c.end);
        if let Some(frag) = &c.replaced {
            let _ = self.doc.insert_fragment(&c.start, frag);
        }
        if let Ok(p) = self.doc.para_mut(c.start.story, &c.start.path) {
            p.mark = c.mark_before.clone();
        }
    }

    /// The formatting typing has while the composition shows: what it had where the composition
    /// began (its underlines are the composition's, not the text's — Word's Underline button
    /// stays off while composing).
    pub(crate) fn composition_props(&self) -> Option<&CharProps> {
        let c = self.composition.as_ref()?;
        let f = &self.sel.focus;
        (f.story == c.start.story && f.path == c.start.path && f.off >= c.start.off && f.off <= c.end.off).then_some(&c.props)
    }

    /// A new document revision for the layout, without dirtying the document.
    pub(crate) fn bump_rev(&mut self) {
        self.rev = self.rev.wrapping_add(1);
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;
    use wordcraft_doc::StoryRef;

    use super::*;

    fn run(s: &mut Session, id: &str, v: serde_json::Value) -> serde_json::Value {
        s.run(id, &v).unwrap_or_else(|e| panic!("{id}: {e}"))
    }
    fn text(s: &Session) -> String {
        s.doc.plain_text(StoryRef::Body)
    }

    /// The composition shows at the caret, underlined, and is provisional: no undo step, no dirty
    /// flag, gone on clear, replaced by the committed text through `text.insert`.
    #[test]
    fn composition_is_shown_inline_and_never_kept() {
        let mut s = Session::new(wordcraft_doc::Document::from_text("ab"));
        run(&mut s, "caret.right", json!({}));
        s.dirty = false;
        s.set_composition("にほんご", Some(0..2)).unwrap();
        assert_eq!(text(&s), "aにほんごb");
        assert!(!s.dirty, "a composition never dirties the document");
        assert!(!s.can_undo(), "no undo step");
        let p = s.doc.para_at(&s.sel.focus).unwrap();
        assert_eq!(p.props_of_char(1).underline, Some(Underline::Thick), "the clause being converted");
        assert_eq!(p.props_of_char("aにほ".len()).underline, Some(Underline::Single));
        assert_eq!(s.sel.focus.off, "aにほ".len(), "the caret follows the clause");
        // A new preedit replaces the old one.
        s.set_composition("日本語", None).unwrap();
        assert_eq!(text(&s), "a日本語b");
        assert_eq!(s.sel.focus.off, "a日本語".len());
        // Dismissed: as if nothing had been typed.
        s.set_composition("", None).unwrap();
        assert_eq!(text(&s), "ab");
        assert_eq!(s.sel.focus.off, 1);
        assert!(s.composition().is_none());
        // Committed: cleared, then typed through the normal path (one undo step, dirty).
        s.set_composition("にほんご", None).unwrap();
        s.clear_composition();
        run(&mut s, "text.insert", json!({"text": "日本語"}));
        assert_eq!(text(&s), "a日本語b");
        assert!(s.dirty);
        assert_eq!(s.doc.para_at(&s.sel.focus).unwrap().props_of_char(1).underline, None, "no underline survives");
        run(&mut s, "edit.undo", json!({}));
        assert_eq!(text(&s), "ab");
    }

    /// The input method's caret in text not yet converted (an empty selection in the composition,
    /// moved with the arrow keys): the document's caret stands there, and nothing is drawn thick;
    /// what the input method types there comes as the whole composition again.
    #[test]
    fn the_caret_stands_where_the_input_method_moved_it_in_the_composition() {
        let mut s = Session::new(wordcraft_doc::Document::from_text("ab"));
        run(&mut s, "caret.right", json!({}));
        s.set_composition("にほご", Some(3..3)).unwrap();
        assert_eq!(s.sel.focus.off, "aにほご".len(), "typing: the caret at the end");
        // Left arrow twice: the input method's caret after に.
        s.set_composition("にほご", Some(1..1)).unwrap();
        assert_eq!(text(&s), "aにほごb");
        assert_eq!(s.sel.focus.off, "aに".len(), "the caret follows the input method's");
        let p = s.doc.para_at(&s.sel.focus).unwrap();
        assert!((1.."aにほご".len()).step_by(3).all(|i| p.props_of_char(i).underline == Some(Underline::Single)), "no clause is being converted");
        // ん typed there: the input method sends the whole composition with its caret after ん.
        s.set_composition("にほんご", Some(3..3)).unwrap();
        assert_eq!(text(&s), "aにほんごb");
        assert_eq!(s.sel.focus.off, "aにほん".len());
        // A range past the end stays in the composition.
        s.set_composition("にほんご", Some(9..9)).unwrap();
        assert_eq!(s.sel.focus.off, "aにほんご".len());
        s.clear_composition();
        assert_eq!((text(&s), s.sel.focus.off), ("ab".to_string(), 1));
    }

    /// A selection within one paragraph is replaced by the composition (the input method's marked
    /// text takes its place: 再変換 selects committed text and marks its reading), put back when
    /// the composition is dismissed, and replaced for good by the committed text.
    #[test]
    fn composition_takes_the_place_of_a_selection() {
        let mut s = Session::new(wordcraft_doc::Document::from_text("a日本語b"));
        run(&mut s, "select.text", json!({"text": "日本語"}));
        s.dirty = false;
        s.set_composition("にほんご", Some(0..4)).unwrap();
        assert_eq!(text(&s), "aにほんごb", "the selected text is gone while the composition shows");
        assert_eq!(s.sel.focus.off, "aにほんご".len());
        assert!(!s.dirty);
        assert!(!s.can_undo());
        s.set_composition("日本", None).unwrap();
        assert_eq!(text(&s), "a日本b");
        // Dismissed: the selected text and the selection are back.
        s.set_composition("", None).unwrap();
        assert_eq!(text(&s), "a日本語b");
        assert_eq!(s.selected_text(), "日本語");
        assert!(!s.dirty);
        // Committed: the selection is replaced through typing (one undo step).
        s.set_composition("にっぽんご", None).unwrap();
        s.clear_composition();
        run(&mut s, "text.insert", json!({"text": "日本"}));
        assert_eq!(text(&s), "a日本b");
        assert!(s.dirty);
        run(&mut s, "edit.undo", json!({}));
        assert_eq!(text(&s), "a日本語b");
        // A selection holding an object (a field) comes back whole.
        let mut s = Session::new(wordcraft_doc::Document::from_text("x y"));
        run(&mut s, "select.text", json!({"text": "y"}));
        run(&mut s, "insert.field", json!({"instr": "PAGE", "result": "1"}));
        run(&mut s, "select.all", json!({}));
        let before = s.doc.para_at(&s.sel.focus).unwrap().clone();
        s.set_composition("あ", None).unwrap();
        assert_eq!(text(&s), "あ");
        s.set_composition("", None).unwrap();
        assert_eq!(s.doc.para_at(&s.sel.focus).unwrap().text, before.text);
        assert_eq!(s.doc.para_at(&s.sel.focus).unwrap().objects.len(), before.objects.len(), "the field is back");
        // A selection across paragraphs is left alone: the composition goes at the focus.
        let mut s = Session::new(wordcraft_doc::Document::from_text("ab\ncd"));
        run(&mut s, "caret.right", json!({}));
        run(&mut s, "caret.down", json!({"extend": true}));
        s.set_composition("あ", None).unwrap();
        assert_eq!(text(&s), "ab\ncあd");
        s.set_composition("", None).unwrap();
        assert_eq!(text(&s), "ab\ncd");
    }

    /// Typing's pending formatting (Bold pressed with nothing selected) shows on the composition
    /// and still applies to the committed text.
    #[test]
    fn composition_takes_the_pending_formatting() {
        let mut s = Session::new(wordcraft_doc::Document::from_text(""));
        run(&mut s, "format.bold", json!({}));
        s.set_composition("あ", None).unwrap();
        assert_eq!(s.doc.para_at(&s.sel.focus).unwrap().props_of_char(0).bold, Some(true));
        s.clear_composition();
        run(&mut s, "text.insert", json!({"text": "亜"}));
        assert_eq!(s.doc.para_at(&s.sel.focus).unwrap().props_of_char(0).bold, Some(true));
    }

    /// A composition in an empty paragraph leaves no underline behind: emptying a paragraph gives
    /// its mark the deleted text's formatting, so the composition's underline stayed on the mark
    /// — a new document's first typing with the input method ended with Underline on in the
    /// ribbon and under everything typed after. While the composition shows, typing's formatting
    /// (the ribbon's buttons) is what it was where it began, as in Word.
    #[test]
    fn a_composition_leaves_no_underline_on_the_paragraph_mark() {
        let mut s = Session::new(wordcraft_doc::Document::from_text(""));
        assert_eq!(run(&mut s, "format.state", json!({}))["underline"], json!(false));
        s.set_composition("にほんご", Some(0..2)).unwrap();
        assert_eq!(run(&mut s, "format.state", json!({}))["underline"], json!(false), "composing: the ribbon shows no underline");
        assert_eq!(s.typing_props().underline, None);
        s.set_composition("にほんご", None).unwrap();
        assert_eq!(run(&mut s, "format.state", json!({}))["underline"], json!(false));
        s.clear_composition();
        let p = s.doc.para_at(&s.sel.focus).unwrap();
        assert_eq!((p.text.as_str(), p.mark.underline), ("", None), "the mark is as it was");
        run(&mut s, "text.insert", json!({"text": "日本語"}));
        let p = s.doc.para_at(&s.sel.focus).unwrap();
        assert_eq!(p.props_of_char(0).underline, None, "the commit is typed without underline");
        assert_eq!(run(&mut s, "format.state", json!({}))["underline"], json!(false));
        // Dismissed (empty marked text) in an empty paragraph: the same.
        let mut s = Session::new(wordcraft_doc::Document::from_text(""));
        s.set_composition("あ", None).unwrap();
        s.set_composition("", None).unwrap();
        assert_eq!(s.doc.para_at(&s.sel.focus).unwrap().mark.underline, None);
        assert_eq!(run(&mut s, "format.state", json!({}))["underline"], json!(false));
        // A mark that was underlined stays underlined; a composition over bold pending
        // formatting shows bold, as before, and the commit is bold.
        let mut s = Session::new(wordcraft_doc::Document::from_text(""));
        run(&mut s, "format.underline", json!({}));
        s.set_composition("あ", None).unwrap();
        assert_eq!(run(&mut s, "format.state", json!({}))["underline"], json!(true), "the text's own underline shows");
        s.clear_composition();
        assert_eq!(run(&mut s, "format.state", json!({}))["underline"], json!(true));
        let mut s = Session::new(wordcraft_doc::Document::from_text(""));
        run(&mut s, "format.bold", json!({}));
        s.set_composition("あ", None).unwrap();
        assert_eq!(run(&mut s, "format.state", json!({}))["bold"], json!(true));
        s.clear_composition();
        run(&mut s, "text.insert", json!({"text": "亜"}));
        assert_eq!(s.doc.para_at(&s.sel.focus).unwrap().props_of_char(0).bold, Some(true));
    }

    /// Any command other than a query takes the composition out first, so no undo snapshot,
    /// save or edit ever holds provisional text.
    #[test]
    fn commands_take_the_composition_out_first() {
        let mut s = Session::new(wordcraft_doc::Document::from_text("ab"));
        s.set_composition("あ", None).unwrap();
        assert_eq!(run(&mut s, "format.state", json!({}))["bold"], json!(false));
        assert_eq!(text(&s), "あab", "a query leaves it");
        run(&mut s, "select.all", json!({}));
        assert_eq!(text(&s), "ab");
        assert!(s.composition().is_none());
        s.set_composition("あ", None).unwrap();
        run(&mut s, "format.bold", json!({}));
        assert_eq!(text(&s), "ab");
        run(&mut s, "edit.undo", json!({}));
        assert_eq!(text(&s), "ab", "the undo step never held the composition");
        let bytes = crate::io::save_bytes("x.docx", &s.doc).unwrap();
        assert!(!bytes.is_empty());
    }

    /// With text selected the composition shows in its place; the selection is back for the
    /// commit, which replaces it as typing does.
    #[test]
    fn composition_over_a_selection_keeps_it_for_the_commit() {
        let mut s = Session::new(wordcraft_doc::Document::from_text("abc"));
        run(&mut s, "select.all", json!({}));
        s.set_composition("あ", None).unwrap();
        assert_eq!(text(&s), "あ");
        s.clear_composition();
        assert_eq!(text(&s), "abc");
        assert!(!s.sel.is_collapsed());
        run(&mut s, "text.insert", json!({"text": "亜"}));
        assert_eq!(text(&s), "亜");
    }

    /// Hostile compositions: long, control characters, a bad clause range, an impossible caret.
    #[test]
    fn hostile_compositions_never_panic() {
        let mut s = Session::new(wordcraft_doc::Document::from_text("x"));
        s.set_composition(&"あ".repeat(5000), Some(usize::MAX..usize::MAX)).unwrap();
        assert_eq!(text(&s).chars().count(), 1 + MAX_COMPOSITION);
        let reversed = std::ops::Range { start: 5, end: 2 };
        s.set_composition("a\u{0}b\r\nc", Some(reversed)).unwrap();
        assert_eq!(text(&s), "abcx");
        s.set_composition("", Some(0..9)).unwrap();
        assert_eq!(text(&s), "x");
        s.sel = Selection::caret(Pos { story: StoryRef::Body, path: wordcraft_doc::Path::top(99), off: 99 });
        assert!(s.set_composition("あ", None).is_err());
        assert!(s.composition().is_none());
        s.clear_composition();
    }
}

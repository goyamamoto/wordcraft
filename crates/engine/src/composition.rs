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

use wordcraft_doc::Pos;
use wordcraft_doc::props::Underline;

use crate::{CmdError, Selection, Session};

/// Longest composition kept, in characters (input methods compose a clause or a sentence).
pub const MAX_COMPOSITION: usize = 1024;

/// Where the composition sits.
#[derive(Clone, Debug, PartialEq, Eq)]
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
}

impl Session {
    /// The composition being shown, if any.
    pub fn composition(&self) -> Option<&Composition> {
        self.composition.as_ref()
    }

    /// Show `text` as the composition at the caret, replacing the one shown. `active` is the
    /// clause being converted (a character range of `text`), drawn with a thick underline. Empty
    /// text takes the composition out (the input method was dismissed).
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
        // Where typing would go: the focus end of the selection, past any field end there.
        let start = self.doc.past_field_ends(&sel_before.focus);
        let mut props = self.typing_props();
        props.fmt_change = None;
        props.ins = None;
        props.del = None;
        let n = text.chars().count();
        let active = active.map(|r| r.start.min(n)..r.end.min(n)).filter(|r| r.start < r.end);
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
                // The caret follows the clause being converted (the candidate window opens there).
                let caret = match &active {
                    Some(r) => Pos { off: start.off.saturating_add(text.chars().take(r.end).map(char::len_utf8).sum()), ..start.clone() },
                    None => end.clone(),
                };
                self.sel = Selection::caret(caret);
                self.composition = Some(Composition { start, end, text, sel_before, dirty_before });
                self.dirty = dirty_before;
                self.bump_rev();
                Ok(())
            }
            Err(e) => {
                // Put back what went in before the failure, so nothing provisional stays.
                if at != start {
                    let _ = self.doc.delete_range(&start, &at);
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

    /// With text selected the selection stays (the commit replaces it, as typing does); the
    /// composition goes after its focus end.
    #[test]
    fn composition_over_a_selection_keeps_it_for_the_commit() {
        let mut s = Session::new(wordcraft_doc::Document::from_text("abc"));
        run(&mut s, "select.all", json!({}));
        s.set_composition("あ", None).unwrap();
        assert_eq!(text(&s), "abcあ");
        s.clear_composition();
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

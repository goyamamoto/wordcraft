//! vendor/winit patch 01 (`vendor/patches/winit/01-macos-ime-surrounding-text.md`): the macOS
//! input method can read the text around the caret and reconvert committed text (再変換), as in
//! Word for Mac. A re-vendored copy without the patch would silently lose it.

#[test]
fn winit_answers_the_input_method_from_the_reported_surrounding_text() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let view = std::fs::read_to_string(root.join("vendor/winit/src/platform_impl/macos/view.rs")).unwrap();
    assert!(view.contains("pub fn set_ime_surrounding_text(text: Option<ImeSurroundingText>)"), "the surrounding-text store");
    assert!(view.contains("pub fn take_ime_replacement_range() -> Option<Range<usize>>"), "the replacement range");
    assert!(view.contains("NSRange::new(self.ivars().marked_location.get() as NSUInteger, length)"), "markedRange lies in the reported text");
    assert!(
        view.contains("if let Some(s) = ime_surrounding() {\n                let len = s.text.encode_utf16().count();"),
        "selectedRange answers the reported selection"
    );
    assert!(view.contains("utf16_slice(&s.text, start..end)"), "attributedSubstringForProposedRange answers the reported text");
    // The reported text follows the input method's own edits at once (see the behaviour test below).
    assert!(view.contains("pub fn apply(\n        &self,\n        marked: Option<&ImeMarked>,"), "ImeSurroundingText::apply");
    assert!(
        view.contains("self.ime_update_surrounding(replaces, &string.to_string(), Some(selected_range));"),
        "setMarkedText updates the surrounding text"
    );
    assert!(view.contains("self.ime_update_surrounding(range, &committed, None);"), "unmarkText commits the marked text");
    assert!(view.contains("self.queue_event(WindowEvent::Ime(Ime::Commit(committed)));"), "unmarkText commits the marked text");
    assert!(view.contains("*self.ivars().marked_text.borrow_mut() = NSMutableAttributedString::new();\n                self.queue_event(WindowEvent::Ime(Ime::Preedit(String::new(), None)));"), "insertText drops the marked text it committed");
    assert!(
        view.contains("} else if !self.ivars().in_key_event.get() && self.is_ime_enabled() && !is_control && !string.is_empty() {"),
        "insertText outside a key event commits unmarked text"
    );
    assert!(view.contains("self.ivars().in_key_event.set(true);\n                unsafe { self.interpretKeyEvents(&events_for_nsview) };\n                self.ivars().in_key_event.set(false);"), "key events are marked");
    assert!(
        view.contains("replacement_range: NSRange,\n        ) {\n            trace_scope!(\"setMarkedText:selectedRange:replacementRange:\");"),
        "setMarkedText reads its replacement range"
    );
    assert!(!view.contains("TODO: Use _replacement_range"), "the replacement range is no longer ignored");

    let platform = std::fs::read_to_string(root.join("vendor/winit/src/platform/macos.rs")).unwrap();
    assert!(platform.contains("pub fn set_ime_surrounding_text(text: Option<ImeSurroundingText>)"), "public on winit::platform::macos");
    assert!(platform.contains("pub fn take_ime_replacement_range() -> Option<std::ops::Range<usize>>"));
    assert!(platform.contains("pub fn ime_surrounding_text() -> Option<ImeSurroundingText>"));
    assert!(platform.contains("pub use crate::platform_impl::{ImeMarked, ImeSurroundingText};"));

    // The desktop app uses it: the canvas reports the paragraph, the hook takes the range.
    let main = std::fs::read_to_string(root.join("apps/wordcraft/src/main.rs")).unwrap();
    assert!(main.contains("winit::platform::macos::set_ime_surrounding_text("));
    assert!(main.contains("winit::platform::macos::take_ime_replacement_range()"));
}

/// The text the input method reads right after it changed the marked text, before the next
/// frame reports the document again (an input method that reads the text left of the caret
/// after `setMarkedText("")` saw its own dismissed suggestion as committed text; Word for Mac and
/// `NSTextView` answer the changed state at once). Ranges in UTF-16 code units.
#[cfg(target_os = "macos")]
#[test]
fn the_reported_text_follows_the_input_methods_edits_at_once() {
    use winit::platform::macos::{ImeMarked, ImeSurroundingText};
    let doc = |text: &str, sel: std::ops::Range<usize>| ImeSurroundingText { text: text.into(), selected: sel };
    let left = |s: &ImeSurroundingText| {
        let units: Vec<u16> = s.text.encode_utf16().collect();
        String::from_utf16_lossy(&units[..s.selected.start.min(units.len())])
    };

    // 1. A suggestion shown as marked text at the end of 今日は雨だった。 (8 units), then taken
    //    back with `setMarkedText("")` before any frame: the caret is at 8 and the text before it
    //    the committed sentence, nothing of the suggestion.
    let s0 = doc("今日は雨だった。", 8..8);
    let (s1, m1) = s0.apply(None, 8..8, "明日は晴れるでしょう。", Some(0..0));
    assert_eq!((s1.text.as_str(), s1.selected.clone()), ("今日は雨だった。明日は晴れるでしょう。", 8..8));
    assert_eq!(m1, Some(ImeMarked { location: 8, len: 11, replaced: String::new() }));
    let (s2, m2) = s1.apply(m1.as_ref(), 8..19, "", Some(0..0));
    assert_eq!((s2.text.as_str(), s2.selected.clone(), m2), ("今日は雨だった。", 8..8, None));
    assert_eq!(left(&s2), "今日は雨だった。");

    // 2. The same with the "no continuation" notice: no （ is left before the caret.
    let (s1, m1) = s0.apply(None, 8..8, "（続きを思いつきませんでした）", Some(0..0));
    let (s2, _) = s1.apply(m1.as_ref(), 8..8 + 15, "", Some(0..0));
    assert!(!left(&s2).contains('（'), "{:?}", left(&s2));
    assert_eq!(s2.selected, 8..8);
    // A generating "…" shown with the selection at its end, then taken back.
    let (s1, m1) = s0.apply(None, 8..8, "…", Some(1..1));
    assert_eq!(s1.selected, 9..9);
    let (s2, _) = s1.apply(m1.as_ref(), 8..9, "", Some(0..0));
    assert_eq!((s2.text.as_str(), s2.selected.clone()), ("今日は雨だった。", 8..8));

    // 3. にほんご marked, then `insertText("日本語")` before any frame: the caret is right after
    //    日本語 and the text before it ends with it.
    let (s1, m1) = s0.apply(None, 8..8, "にほんご", Some(0..4));
    assert_eq!((s1.text.as_str(), s1.selected.clone()), ("今日は雨だった。にほんご", 8..12));
    let (s2, m2) = s1.apply(m1.as_ref(), 8..12, "日本語", None);
    assert_eq!((s2.text.as_str(), s2.selected.clone(), m2), ("今日は雨だった。日本語", 11..11, None));
    assert!(left(&s2).ends_with("日本語"));

    // 4. Reconversion: the selected committed 日本 replaced by marked にほん with a replacement
    //    range; dismissed, 日本 is back and selected (the application puts it back too);
    //    committed, the commit takes its place.
    let s0 = doc("a日本b", 1..3);
    let (s1, m1) = s0.apply(None, 1..3, "にほん", Some(0..3));
    assert_eq!((s1.text.as_str(), s1.selected.clone()), ("aにほんb", 1..4));
    assert_eq!(m1, Some(ImeMarked { location: 1, len: 3, replaced: "日本".into() }));
    let (s2, m2) = s1.apply(m1.as_ref(), 1..4, "", Some(0..0));
    assert_eq!((s2.text.as_str(), s2.selected.clone(), m2), ("a日本b", 1..3, None));
    let (s3, _) = s1.apply(m1.as_ref(), 1..4, "二本", None);
    assert_eq!((s3.text.as_str(), s3.selected.clone()), ("a二本b", 3..3));
    // Live conversion: ご marked after 日本, then 日本ご (1..4, reaching into committed text)
    // marked again as にほんご: it stands for 日本; dismissed, 日本 is back.
    let s0 = doc("a日本b", 3..3);
    let (s1, m1) = s0.apply(None, 3..3, "ご", Some(1..1));
    assert_eq!((s1.text.as_str(), s1.selected.clone()), ("a日本ごb", 4..4));
    let (s2, m2) = s1.apply(m1.as_ref(), 1..4, "にほんご", Some(4..4));
    assert_eq!((s2.text.as_str(), s2.selected.clone()), ("aにほんごb", 5..5));
    assert_eq!(m2, Some(ImeMarked { location: 1, len: 4, replaced: "日本".into() }));
    let (s3, m3) = s2.apply(m2.as_ref(), 1..5, "", Some(0..0));
    assert_eq!((s3.text.as_str(), s3.selected.clone(), m3), ("a日本b", 1..3, None));
    // Committed as 日本語: in place of 日本 and the marked text, the caret after it.
    let (s4, _) = s2.apply(m2.as_ref(), 1..5, "日本語", None);
    assert_eq!((s4.text.as_str(), s4.selected.clone()), ("a日本語b", 4..4));
    // `unmarkText`: the marked text committed as it stands.
    let (s5, m5) = s2.apply(m2.as_ref(), 1..5, "にほんご", None);
    assert_eq!((s5.text.as_str(), s5.selected.clone(), m5), ("aにほんごb", 5..5, None));

    // Hostile ranges and selections are clamped, never panic.
    let s0 = doc("𠮷野家", 2..2);
    let (s1, m1) = s0.apply(None, 9..99, "x", Some(5..9));
    assert_eq!((s1.text.as_str(), s1.selected.clone()), ("𠮷野家x", 5..5));
    let stale = ImeMarked { location: 50, len: 50, replaced: "zz".into() };
    let (s2, _) = s1.apply(Some(&stale), 0..1, "", Some(0..0));
    assert_eq!(s2.text, "𠮷野家xzz");
    let _ = s1.apply(m1.as_ref(), std::ops::Range { start: 4, end: 1 }, "y", None);
    let _ = s1.apply(m1.as_ref(), usize::MAX - 1..usize::MAX, "y", Some(usize::MAX..usize::MAX));
}

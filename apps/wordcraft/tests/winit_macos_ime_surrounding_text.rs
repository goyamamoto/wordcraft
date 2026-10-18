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
    assert!(
        view.contains("replacement_range: NSRange,\n        ) {\n            trace_scope!(\"setMarkedText:selectedRange:replacementRange:\");"),
        "setMarkedText reads its replacement range"
    );
    assert!(!view.contains("TODO: Use _replacement_range"), "the replacement range is no longer ignored");

    let platform = std::fs::read_to_string(root.join("vendor/winit/src/platform/macos.rs")).unwrap();
    assert!(platform.contains("pub fn set_ime_surrounding_text(text: Option<ImeSurroundingText>)"), "public on winit::platform::macos");
    assert!(platform.contains("pub fn take_ime_replacement_range() -> Option<std::ops::Range<usize>>"));

    // The desktop app uses it: the canvas reports the paragraph, the hook takes the range.
    let main = std::fs::read_to_string(root.join("apps/wordcraft/src/main.rs")).unwrap();
    assert!(main.contains("winit::platform::macos::set_ime_surrounding_text("));
    assert!(main.contains("winit::platform::macos::take_ime_replacement_range()"));
}

# macOS: the input method reads the text around the caret (再変換)

**Files:** `src/platform_impl/macos/view.rs`, `src/platform_impl/macos/mod.rs`,
`src/platform/macos.rs`. **Guard:** `apps/wordcraft/tests/winit_macos_ime_surrounding_text.rs`.

winit 0.30.13's `NSTextInputClient` tells the input method nothing about the document:
`selectedRange` answers `{NSNotFound, 0}`, `attributedSubstringForProposedRange:actualRange:`
answers `nil`, `markedRange` counts from 0, and the `replacementRange` of
`setMarkedText:selectedRange:replacementRange:` and `insertText:replacementRange:` is ignored
(its own `TODO`). So the macOS Japanese input method cannot reconvert committed text (select it,
press かな twice or choose 再変換): it reads no selection, gets no text for it, and the marked
text it would put in the selection's place goes at the caret. Live conversion gets no context
either. Word for Mac answers all of these.

The patch adds a process-wide store the application fills once a frame (eframe never hands the
application a `Window`, and one window takes keyboard input at a time):

- `winit::platform::macos::ImeSurroundingText { text, selected }` and
  `set_ime_surrounding_text(Option<…>)`: the caret's paragraph and the selection in it, in
  UTF-16 code units. `None` restores the old answers.
- `selectedRange` answers the reported selection — or, while there is marked text, the input
  method's own selection within it, offset by where the marked text lies.
- `attributedSubstringForProposedRange:actualRange:` answers the requested slice of the reported
  text, clamped, and fills `actualRange`.
- `markedRange` lies at `marked_location` in the reported text: the replacement range's start
  when the input method named one, else the selection's start when the marked text began.
- `setMarkedText:…replacementRange:` (and `insertText:replacementRange:` while marked text
  exists) records a replacement range that isn't `NSNotFound`;
  `winit::platform::macos::take_ime_replacement_range()` hands it to the application, which
  selects that text before applying the `Ime::Preedit` that follows, so the composition takes
  its place.

Nothing changes for applications that never call `set_ime_surrounding_text`. The patch is
marked `WordCraft patch` at each site.

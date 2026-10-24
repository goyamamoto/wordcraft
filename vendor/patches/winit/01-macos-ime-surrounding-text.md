# macOS: the input method reads the text around the caret (再変換)

**Files:** `src/platform_impl/macos/view.rs`, `src/platform_impl/macos/mod.rs`,
`src/platform/macos.rs`. **Guard:** `apps/wordcraft/tests/it/winit_macos_ime_surrounding_text.rs`.

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

The store follows the input method's own edits at once, before the application reports the
text again at the next frame (`ImeSurroundingText::apply`, `WinitView::ime_update_surrounding`):
an input method reads `selectedRange` and `attributedSubstringForProposedRange:` right after
`setMarkedText:` (Ogura's "continue writing" takes back its suggestion with empty marked text
and reads the text before the caret again — and read the dismissed suggestion as committed
text; Word for Mac and `NSTextView` answer the changed state at once).

- `setMarkedText:` puts the new marked text in place of the range it replaces — the named
  replacement range, else the marked text, else the selection — and selects the input method's
  `selectedRange` within it. A range reaching into the marked text counts it as the committed
  text it stands for (`ImeMarked::replaced`: a reconverted selection, or nothing), as the
  application's `committed_range` does. Empty marked text takes the marked text out and selects
  what it stood for, as `Session::set_composition` puts a replaced selection back.
- `insertText:` while marked puts the committed text in place of the marked text (or the named
  range), the caret after it, and drops the marked text at once (winit kept it until the next
  key event, so `selectedRange` still answered within it).
- `unmarkText` commits the marked text as it stands — the `NSTextInputClient` contract, what
  `NSTextView` and Word do — where winit dropped it: an `Ime::Commit` of the marked text follows
  the empty `Ime::Preedit`.
- `insertText:` outside a key event with no marked text (an input method inserting on its own,
  without marking first) is an `Ime::Commit` too; within `interpretKeyEvents` it is the key's own
  text, which the key event reports, as before (`in_key_event`).
- `winit::platform::macos::ime_surrounding_text()` reads the store (tests).

The in-place state is a bridge to the next frame, which overwrites it with what the document
holds (AutoCorrect and the like included). Nothing changes for applications that never call
`set_ime_surrounding_text`. The patch is marked `WordCraft patch` at each site.

## Committing a conversion clause by clause (2026-10-20)

Reported by the owner: in a conversion of several clauses, committing the first clause lost the
rest, where Word for Mac keeps typing it. Two places in `view.rs` could drop it:

- `insertText:` within a key event took any text for the key's own characters and dropped it
  (the key event reports them). An input method committing text this view no longer held as
  marked (the rest of the conversion) or making another character of the key (a full-width
  space) lost it, and the key (Return) went to the application. Now text other than the key's
  characters (`key_text`, recorded in `keyDown:`) is a commit, and the key isn't reported.
- `set_ime_allowed(false)` cleared the marked text on this side only; the input method kept it
  and committed it later into nothing. Now the marked text is committed (`Ime::Preedit("")`,
  `Ime::Commit`) and the input method is told it's gone (`discardMarkedText`), as `unmarkText`
  does. WordCraft also keeps the IME allowed every frame the document has the keyboard
  (`CanvasState::ime_rect`), so egui doesn't turn it off between frames.

`WORDCRAFT_IME_TRACE=<file>` writes each `keyDown:`, `insertText:`, `setMarkedText:`,
`unmarkText`, `doCommandBySelector:` and `set_ime_allowed` call (text, ranges, IME state) to the
file: an input method's real calls can't be reproduced without typing.


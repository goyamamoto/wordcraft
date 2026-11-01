# macOS: the input method reads the text around the caret (再変換)

**Files:** `src/platform_impl/macos/ime_surrounding.rs` (new), `src/platform_impl/macos/view.rs`,
`src/platform_impl/macos/mod.rs`, `src/platform/macos.rs`. **Guard:**
`apps/wordcraft/tests/it/winit_macos_ime_surrounding_text.rs`.

winit's `NSTextInputClient` knows nothing of the document. storytold/winit's W22 gives it a
caret: `ImeDocument`, the text typed since the caret last moved, so `selectedRange` is the end of
it and `attributedSubstringForProposedRange:` answers from it — enough for press-and-hold accents
and dictation, which replace what they just inserted. It is not enough for the Japanese input
method to reconvert committed text (select it, press かな twice or choose 再変換): the selection is
never reported, text typed before the last caret move (or in another session) can't be read, and
a replacement range anywhere but just before the caret is ignored, so the marked text goes at the
caret. Live conversion gets no context before the caret either. Word for Mac answers all of these.

The patch adds a process-wide store the application fills once a frame (eframe never hands the
application a `Window`, and one window takes keyboard input at a time). While it holds text, the
view counts in it instead of in `ImeDocument`; with nothing reported (`None`) the fork's
behaviour is untouched.

- `winit::platform::macos::ImeSurroundingText { text, selected }` and
  `set_ime_surrounding_text(Option<…>)`: the caret's paragraph and the selection in it, in
  UTF-16 code units.
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
  its place. In this case the fork's W22/W26 deletion is not made (it would delete the same text
  a second time).
- `insertText:replacementRange:` without marked text (press-and-hold's accent picked with a digit,
  a dictation correction) turns the range into the deletion that goes with the commit
  (`ImeSurroundingText::deletion`: the part of the range before and after the selection, in UTF-8
  bytes), which the application takes with the fork's `take_ime_delete_surrounding` (W26) — the
  fork's own mechanism, counted in the reported text.

The store follows the input method's own edits at once, before the application reports the
text again at the next frame (`ImeSurroundingText::apply`, `WinitView::ime_update_surrounding`):
an input method reads `selectedRange` and `attributedSubstringForProposedRange:` right after
`setMarkedText:` (an input method's "continue writing" takes back its suggestion with empty marked text
and reads the text before the caret again — and read the dismissed suggestion as committed
text; Word for Mac and `NSTextView` answer the changed state at once).

- `setMarkedText:` puts the new marked text in place of the range it replaces — the named
  replacement range, else the marked text, else the selection — and selects the input method's
  `selectedRange` within it. A range reaching into the marked text counts it as the committed
  text it stands for (`ImeMarked::replaced`: a reconverted selection, or nothing), as the
  application's `committed_range` does. Empty marked text takes the marked text out and selects
  what it stood for, as `Session::set_composition` puts a replaced selection back.
- `insertText:` puts the committed text in place of the marked text (or the named range, or the
  selection), the caret after it, and drops the marked text at once (winit kept it until the end
  of the key event, so `selectedRange` still answered within it).
- `reset_ime` (the fork's W24, a cancel) takes the marked text out and selects what it stood for.

## `unmarkText` commits

`unmarkText` commits the marked text as it stands — the `NSTextInputClient` contract, what
`NSTextView` and Word do — where winit (and the fork) drop it: an `Ime::Commit` of the marked
text follows the empty `Ime::Preedit`, and a key being handled isn't reported again. Two guards
keep it to one commit: an `unmarkText` the view causes itself by discarding the marked text
(`reset_ime`, `set_ime_allowed(false)`: `discarding_marked_text`) doesn't commit, and an
`insertText:` of the same text right after it (input methods that call `unmarkText` and then
`insertText:`, the case W6's `pending_commit` was made for) isn't committed again
(`unmarked_commit`, cleared by the next key or `setMarkedText:`).

## Committing a conversion clause by clause (2026-10-20)

Reported by the owner: in a conversion of several clauses, committing the first clause lost the
rest, where Word for Mac keeps typing it. Two places could drop it:

- `insertText:` within a key event took any text for the key's own characters and dropped it.
  The fork now does what this patch did (W22 `plain_insert`: text other than the key's characters
  is a commit, and the key isn't reported — `committed_in_key_down`), so that part of the patch
  is gone; the guard checks the fork's code.
- `set_ime_allowed(false)` cleared the marked text on this side only; the input method kept it
  and committed it later into nothing. The fork's W7 now tells the input method to discard it;
  this patch also commits it first (`Ime::Preedit("")`, `Ime::Commit`), as `unmarkText` does.
  egui-winit's stock interruption of a composition (a click on the canvas:
  `set_ime_allowed(false)` then `(true)`) therefore commits it on macOS, as Word does when the
  caret moves. WordCraft also keeps the IME allowed every frame the document has the keyboard
  (`CanvasState::ime_rect`), so egui doesn't turn it off between frames.

`WORDCRAFT_IME_TRACE=<file>` writes each `keyDown:`, `insertText:`, `setMarkedText:`,
`unmarkText`, `doCommandBySelector:`, `set_ime_allowed` and `reset_ime` call (text, ranges, IME
state) to the file: an input method's real calls can't be reproduced without typing.

## Re-applied on storytold/winit (2026-11-01)

First written on winit 0.30.13 (commits 483aa57e, 5eed4001, e1a06277). Re-applied on the fork's
view, which had rewritten the same methods: the store, `apply` and the trace moved to their own
module (`ime_surrounding.rs`); `in_key_event`/`key_text` were dropped for the fork's
`in_key_down`/`key_characters`/`plain_insert`, and `marked_selected` for its `marked_selection`;
the fork's `ImeDocument` stays the answer when nothing is reported. The fork's own tests pass
with the patch applied (in a checkout of the fork: `cargo test --lib`, `--test ime_patches`,
`--test macos_keyboard`; its source guards read `view.rs`, so keep `let delete =
self.replaced(replacement_range);` in `setMarkedText:`). Comments marked
`WordCraft patch` without a reference to this file (W6, W8–W10) are the fork's own.

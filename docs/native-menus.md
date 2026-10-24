# Native macOS menu bar

> **Last reviewed:** 2026-10-24 · **Last updated:** 2026-10-24 · **Change:** major (first version: the menu bar adapted from storytold/wordcraft#985, with Tools, Table and Word for Mac's Window menu, on the window host) · **Target:** Microsoft Word (Microsoft 365) for Mac 16.113.4

On macOS the desktop app installs the menu bar Word for Mac has at the top of the screen,
alongside the ribbon (never replacing it): WordCraft, File, Edit, View, Insert, Format, Tools,
Table, Window and Help, in Word's order. Every item runs the same command the ribbon does.
Windows, Linux/BSD and the web keep the ribbon alone, as Word for Windows has no menu bar.

| Menu | Items |
|---|---|
| WordCraft | About WordCraft, Settings… (⌘,), Services, Hide, Hide Others, Show All, Quit (⌘Q). macOS titles this menu after the bundle (`WordCraft`; a bare development binary reads `wordcraft`) |
| File | New, New from Template…, Open…, Open Recent ▸, Close, Save, Save As…, Save as Template…, Export PDF…, Export Page as PNG…, Share, Properties, Print… |
| Edit | Undo, Redo, Cut, Copy, Paste, Paste Special…, Select All, Find, Find Next, Find Previous, Replace…, Go To… |
| View | Print Layout, Web Layout, Draft, Outline, Read Mode, Focus (checked); Ruler, Gridlines, Show/Hide ¶, Navigation, Styles and Comments panes (checked); Zoom…, Zoom In, Zoom Out, 100%, Page Width |
| Insert | Page Break, Blank Page, Cover Page; Table…, Pictures…, Chart…, Icons…, Text Box; Edit Header, Edit Footer, Page Number; Comment, Equation, Symbol…, Date & Time…, Cross-reference…, Link…, Bookmark… |
| Format | Font…, Paragraph…; Bold, Italic, Underline, Strikethrough, Subscript, Superscript (checked); Grow Font, Shrink Font, Clear All Formatting; Alignment ▸ (checked); Bullets, Numbering (checked); Increase and Decrease Indent |
| Tools | Spelling & Grammar…, Thesaurus…, Hyphenation…, Language…, Word Count…, AutoCorrect Options…; Track Changes (checked), Compare Documents…, Combine Documents…, Restrict Editing; Macros, Templates and Add-ins…, Customize Keyboard…, Customize Ribbon… |
| Table | Insert ▸ (Table…, rows above/below, columns left/right), Delete ▸, Select ▸; Merge Cells, Split Cells…, Split Table; AutoFit, Convert ▸ (Text to Table…, Table to Text…), Sort…, Formula…; Table Properties… |
| Window | Minimize (⌘M), Zoom; New Window, Arrange All, Split; Bring All to Front. Then what macOS adds to an app's windows menu (Fill, Center, Move & Resize ▸, Full Screen Tile ▸, Remove Window from Set, Move to iPad) and the open document windows, the one in front checked. Observed in Word for Mac on 2026-10-24: View Side by Side, Synchronous Scrolling and Reset Window Position are on the ribbon only. Minimize and Zoom are the app's own rows, not AppKit's `performMiniaturize:`/`performZoom:` ones, because macOS puts its tiling rows right after a `performZoom:` row, where Word has them after Bring All to Front; so those two rows have no system icon. Increase Indent is ⌃⇧M on the Mac (as in Word for Mac), which leaves ⌘M to Minimize |
| Help | Commands (command search; registered as the help menu, so macOS's menu search works), Check Accessibility |

New Window and Split come from the document-windows work (#322) and are disabled with the
reason until a session has two views of one document. The Window
menu's list of documents replaces the ribbon's Switch Windows on macOS (Word for Mac has no such
button; Word for Windows does, because Windows has no menu bar), so that button is not drawn
while the menu bar is installed.

## Command routing

`crates/ui-egui/src/native_menu.rs` owns a platform-free menu model (`MenuBar`, `Node`, `Item`,
`Chord`) built from the window in use (`menu_bar`): labels come from the command registry in
the interface language, key equivalents from the effective keymap (user assignments and
removals included), enabled state from the same rule the ribbon uses, checkmarks from the same
selection and view state the ribbon reads. The desktop app injects `apps/wordcraft/src/mac_menu.rs`,
a macOS-only `muda` backend; the engine and the other platforms have no AppKit dependency.

The menu is one for the process while the app has a window per document (`windows::Host`,
#322): the host builds it from the window in use, runs its items in that window's frame, and
syncs its enabled and checked state after that window's frame. A menu event wakes the root
viewport, which asks the window in use to repaint and take it. Minimize minimizes the window in
use; Quit asks about unsaved changes in every window in turn (Cancel keeps the app open) and
never goes through AppKit's own Quit selector, which would ask nothing.

Menu actions use `WordApp::run`, keeping the user dialogs and system clipboard handling. A
dialog or an open system file dialog disables document actions, which are checked again when
queued actions run; Read Mode disables insertion, formatting and table commands. Focused egui
text fields receive their own copy/cut/paste/select-all/undo/redo events, including in dialogs;
those do not edit the document. While a system file dialog is open its Edit items use AppKit's
responder-chain selectors, so the dialog's own fields keep native editing, and the egui items
return when it closes.

Key equivalents use only command/control shortcuts and function keys; ordinary typing and
Option characters are left to the editor. Command-H (Replace) keeps its meaning where the keymap
binds it: Hide, Minimize, Settings and Quit take their Mac keys only when the keymap leaves them
free (Hide and Minimize try Control-Command as well). A key equivalent AppKit took is given back
to egui as the key press it was, placed before its real key-up, which winit still delivers; a
Command-V so handled cannot paste a second picture on its release.

Enabled state, labels and checkmarks update in place; the tree is rebuilt only when its shape,
language or shortcuts change. If the menu bar cannot be installed, the ribbon alone remains.

## Provenance and checks

The model, backend and AppKit test are adapted from storytold/wordcraft#985 (YTwsy), which
adapted [PhotoCraft at 67e4b8a](https://github.com/storytold/photocraft/tree/67e4b8a499b55ef728b1da700005382c45e4677d);
the Tools and Table menus follow #1254's lists through the registry's labels. The copied portions
are covered by the MIT notice in `NOTICE`.

```sh
cargo test -p wordcraft-ui-egui --lib native_menu
cargo test -p wordcraft --test mac_menu_appkit
```

The UI tests cover the layout and that every item is a registered command, routing of clicks
and key equivalents, selection state and checkmarks, the effective shortcuts, modal races, text
fields, text and picture paste, Quit's save/cancel flow (and with a window host, that Quit is
the host's). The AppKit test runs on the main thread without desktop keystrokes: it installs the
real menu bar, reads NSMenu back (Word for Mac's ten menus, the Window menu's items, the windows
and help menu registrations), performs key equivalents and click/Return actions, checks submenu
restoration, the language change and save/cancel, and native editing selectors during file
dialogs. It is a no-op on other platforms.

## Revision history

| Date | Change | Summary |
|---|---|---|
| 2026-10-24 | trivial | Window: Zoom is the app's own row, so macOS's tiling rows follow Bring All to Front as in Word; Minimize is ⌘M (Increase Indent moved to ⌃⇧M on the Mac, Word for Mac's key) |
| 2026-10-24 | major | First version, adapted from #985 with Tools, Table and the Window menu as Word for Mac has it (its own items observed in Word for Mac the same day), on the window host (#322) |

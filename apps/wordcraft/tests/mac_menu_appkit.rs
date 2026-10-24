//! Adapted from storytold/wordcraft#985 (YTwsy), after PhotoCraft at
//! 67e4b8a499b55ef728b1da700005382c45e4677d (MIT; see NOTICE).
//! The macOS menu bar end to end, on the main thread (`harness = false`): install it through the
//! real muda backend (`src/mac_menu.rs`), read AppKit's menu bar back, edit a document and see the
//! items update in place, then choose items the two ways AppKit can: a key equivalent (a real ⌘R
//! key-down through `-[NSMenu performKeyEquivalent:]`) must reach egui as the key press it was, and
//! a click (`-[NSMenu performActionForItemAtIndex:]`) must run as a click, also when the click is
//! ↩ inside the open menu.

// The app's own module, built into this test (cargo builds harness = false tests with cfg(test), so its
// unit tests compile here too without a harness to run them).
#[cfg(target_os = "macos")]
#[path = "../src/mac_menu.rs"]
#[allow(unused_imports, dead_code)]
mod mac_menu;

#[cfg(target_os = "macos")]
fn main() {
    use objc2::MainThreadMarker;
    use objc2::rc::Retained;
    use objc2_app_kit::{NSApplication, NSEvent, NSEventMask, NSEventModifierFlags, NSEventSubtype, NSEventType, NSMenu};
    use objc2_foundation::{NSPoint, NSString};
    use serde_json::json;
    use wordcraft_ui_egui::WordApp;
    use wordcraft_ui_egui::native_menu::{self, NativeMenu};

    let mtm = MainThreadMarker::new().expect("harness = false runs on the main thread");
    let nsapp = NSApplication::sharedApplication(mtm);
    nsapp.finishLaunching();

    let titles = |menu: &NSMenu| -> Vec<String> {
        (0..menu.numberOfItems()).filter_map(|i| menu.itemAtIndex(i)).filter(|it| !it.isSeparatorItem()).map(|it| it.title().to_string()).collect()
    };
    let submenu = |menu: &NSMenu, title: &str| -> Retained<NSMenu> {
        (0..menu.numberOfItems())
            .filter_map(|i| menu.itemAtIndex(i))
            .find(|it| it.title().to_string() == title)
            .and_then(|it| it.submenu())
            .unwrap_or_else(|| panic!("no {title} submenu in {:?}", titles(menu)))
    };
    let index = |menu: &NSMenu, title: &str| -> isize {
        (0..menu.numberOfItems())
            .find(|&i| menu.itemAtIndex(i).is_some_and(|it| it.title().to_string() == title))
            .unwrap_or_else(|| panic!("no {title} in {:?}", titles(menu)))
    };
    let main_menu = || nsapp.mainMenu().expect("a main menu");
    // Make `event` AppKit's current event, as the run loop does before dispatching it.
    let dequeue = |event: &NSEvent| {
        nsapp.postEvent_atStart(event, true);
        let mode = NSString::from_str("kCFRunLoopDefaultMode");
        let got = nsapp.nextEventMatchingMask_untilDate_inMode_dequeue(NSEventMask::Any, None, &mode, true).expect("the posted event");
        assert_eq!(got.r#type(), event.r#type());
        got
    };
    let key_down = |chars: &str, code: u16, flags: NSEventModifierFlags| {
        let s = NSString::from_str(chars);
        NSEvent::keyEventWithType_location_modifierFlags_timestamp_windowNumber_context_characters_charactersIgnoringModifiers_isARepeat_keyCode(
            NSEventType::KeyDown,
            NSPoint::new(0.0, 0.0),
            flags,
            0.0,
            0,
            None,
            &s,
            &s,
            false,
            code,
        )
        .expect("a key event")
    };
    let other = || {
        NSEvent::otherEventWithType_location_modifierFlags_timestamp_windowNumber_context_subtype_data1_data2(
            NSEventType::ApplicationDefined,
            NSPoint::new(0.0, 0.0),
            NSEventModifierFlags::empty(),
            0.0,
            0,
            None,
            NSEventSubtype::ApplicationActivated.0,
            0,
            0,
        )
        .expect("an application-defined event")
    };

    let mut app = WordApp::new(wordcraft_engine::Session::new(wordcraft_doc::Document::new()), wordcraft_ui_egui::Services::default());
    let ctx = egui::Context::default();
    app.ui.language = "en".into();
    app.canvas.focused = true;
    app.menu_bar_installed = true;
    app.session.keymap.assign(&app.session.registry.clone(), "view.ruler", "Mod+R").expect("custom key");
    let mut menu: NativeMenu = mac_menu::install(&ctx, &app).expect("the menu bar installed");
    let frame = |app: &mut WordApp, menu: &mut NativeMenu, mut raw: egui::RawInput| {
        native_menu::raw_input(menu, app, &mut raw);
        app.raw_input_hook(&mut raw);
        let mut out = ctx.run_ui(raw, |ui| {
            native_menu::run(menu, app, ui.ctx());
            wordcraft_ui_egui::keys::canvas_events(app, ui.ctx());
            native_menu::sync(menu, app, ui.ctx());
        });
        out.textures_delta.clear();
    };

    // The Mac layout, as AppKit has it: Word for Mac's ten menus.
    let bar = main_menu();
    let tops: Vec<String> = (0..bar.numberOfItems()).filter_map(|i| bar.itemAtIndex(i)?.submenu()).map(|m| m.title().to_string()).collect();
    assert_eq!(tops, ["WordCraft", "File", "Edit", "View", "Insert", "Format", "Tools", "Table", "Window", "Help"]);
    let app_menu = submenu(&bar, "WordCraft");
    let names = titles(&app_menu);
    for want in ["About WordCraft", "Settings…", "Hide WordCraft", "Quit WordCraft"] {
        assert!(names.iter().any(|n| n == want), "{want} missing from the app menu: {names:?}");
    }
    let quit = app_menu.itemAtIndex(index(&app_menu, "Quit WordCraft")).expect("Quit");
    assert_eq!(quit.keyEquivalent().to_string(), "q");
    assert_eq!(quit.keyEquivalentModifierMask(), NSEventModifierFlags::Command);
    assert!(!titles(&submenu(&bar, "File")).iter().any(|n| n == "Exit"), "Exit moved to the app menu as Quit");
    assert_eq!(nsapp.windowsMenu().map(|m| m.title().to_string()).as_deref(), Some("Window"), "AppKit knows the Window menu");
    assert_eq!(nsapp.helpMenu().map(|m| m.title().to_string()).as_deref(), Some("Help"), "AppKit knows the Help menu");
    // The Window menu as Word for Mac has it; the tiling items and the open windows AppKit adds itself.
    let window = submenu(&bar, "Window");
    let names = titles(&window);
    let ours: Vec<&str> = names.iter().map(String::as_str).take(6).collect();
    assert_eq!(ours, ["Minimize", "Zoom", "New Window", "Arrange All", "Split", "Bring All to Front"], "{names:?}");
    assert!(!window.itemAtIndex(index(&window, "Arrange All")).expect("Arrange All").isEnabled(), "no window host in this test");

    // State updates keep the installed NSMenu object, including AppKit's added window rows.
    let undo_enabled = |bar: &NSMenu| {
        let edit = submenu(bar, "Edit");
        edit.itemAtIndex(index(&edit, "Undo")).expect("Undo").isEnabled()
    };
    assert!(!undo_enabled(&bar));
    app.run("text.insert", json!({"text": "native menu test"})).expect("edit document");
    frame(&mut app, &mut menu, egui::RawInput::default());
    assert!(Retained::as_ptr(&bar) == Retained::as_ptr(&main_menu()), "no rebuild on an edit");
    assert!(undo_enabled(&main_menu()));

    // A key equivalent: ⌘R reaches egui as ⌘R and toggles View › Ruler once.
    let rulers = app.session.view.ruler;
    let press = dequeue(&key_down("r", 15, NSEventModifierFlags::Command));
    assert!(main_menu().performKeyEquivalent(&press), "⌘R is View › Ruler's key equivalent");
    let mut raw = egui::RawInput::default();
    native_menu::raw_input(&mut menu, &mut app, &mut raw);
    app.raw_input_hook(&mut raw);
    let keys: Vec<_> = raw
        .events
        .iter()
        .filter_map(|e| if let egui::Event::Key { key, pressed, modifiers, .. } = e { Some((*key, *pressed, modifiers.command)) } else { None })
        .collect();
    // AppKit took only the key-down; winit delivers the key-up itself (#1638).
    assert_eq!(keys, [(egui::Key::R, true, true)], "the key press");
    let mut out = ctx.run_ui(raw, |ui| {
        native_menu::run(&mut menu, &mut app, ui.ctx());
        wordcraft_ui_egui::keys::canvas_events(&mut app, ui.ctx());
        native_menu::sync(&mut menu, &app, ui.ctx());
    });
    out.textures_delta.clear();
    assert_eq!(app.session.view.ruler, !rulers, "⌘R toggled Ruler once");

    // A click: View › Ruler chosen with the mouse runs the command.
    let view = submenu(&main_menu(), "View");
    let at = index(&view, "Ruler");
    dequeue(&other());
    view.performActionForItemAtIndex(at);
    let mut raw = egui::RawInput::default();
    native_menu::raw_input(&mut menu, &mut app, &mut raw);
    assert!(raw.events.is_empty(), "a click is no key press: {:?}", raw.events);
    frame(&mut app, &mut menu, raw);
    assert_eq!(app.session.view.ruler, rulers, "the click toggled Ruler back");

    // ↩ inside the open menu is a key-down without ⌘: still a click.
    dequeue(&key_down("\r", 36, NSEventModifierFlags::empty()));
    view.performActionForItemAtIndex(at);
    let mut raw = egui::RawInput::default();
    native_menu::raw_input(&mut menu, &mut app, &mut raw);
    assert!(raw.events.is_empty(), "↩ in the menu is a click: {:?}", raw.events);
    frame(&mut app, &mut menu, raw);
    assert_eq!(app.session.view.ruler, !rulers, "↩ on Ruler toggled it");

    // Modal changes update children and parent submenus in place, then restore them.
    let format = submenu(&main_menu(), "Format");
    let alignment = format.itemAtIndex(index(&format, "Alignment")).expect("Alignment");
    assert!(alignment.isEnabled());
    app.run("ui.dialog", json!({"name": "about"})).expect("open modal");
    frame(&mut app, &mut menu, egui::RawInput::default());
    assert!(!alignment.isEnabled(), "the submenu is disabled with its children");
    assert!(!quit.isEnabled(), "Quit cannot bypass a modal");
    app.dialog = None;
    frame(&mut app, &mut menu, egui::RawInput::default());
    assert!(alignment.isEnabled());
    assert!(quit.isEnabled());

    // The row AppKit chooses must not override the modifiers actually held.
    let reg = app.session.registry.clone();
    app.session.keymap.remove(&reg, "view.ruler", "Mod+R").expect("remove key");
    app.session.keymap.assign(&reg, "view.ruler", "Mod+Shift+R").expect("shifted key");
    app.session.keymap.assign(&reg, "para.alignRight", "Mod+R").expect("plain key");
    frame(&mut app, &mut menu, egui::RawInput::default());
    let rulers = app.session.view.ruler;
    dequeue(&key_down("r", 15, NSEventModifierFlags::Command));
    let view = submenu(&main_menu(), "View");
    view.performActionForItemAtIndex(index(&view, "Ruler"));
    let mut raw = egui::RawInput::default();
    native_menu::raw_input(&mut menu, &mut app, &mut raw);
    frame(&mut app, &mut menu, raw);
    assert_eq!(app.session.view.ruler, rulers, "plain Command-R is not Shift-Command-R");
    let para = app.session.doc.para_at(&app.session.sel.focus).expect("paragraph");
    assert_eq!(app.session.doc.styles.resolve_para(&para.props).align, wordcraft_doc::Align::Right);

    // Quit follows the same save/cancel path as closing the window, never a predefined
    // AppKit Quit selector that could terminate before WordCraft has asked about dirty text.
    dequeue(&key_down("q", 12, NSEventModifierFlags::Command));
    assert!(main_menu().performKeyEquivalent(&nsapp.currentEvent().expect("current key")));
    let mut raw = egui::RawInput::default();
    native_menu::raw_input(&mut menu, &mut app, &mut raw);
    frame(&mut app, &mut menu, raw);
    assert_eq!(app.dialog.as_ref().map(|d| d.name()), Some("saveChanges"));
    assert!(!app.quit_requested);
    app.run("ui.saveChanges", json!({"answer": "cancel"})).expect("cancel quit");
    assert!(!app.quit_requested);

    // A native file sheet uses Cocoa editing selectors, then restores the custom egui rows.
    let pending = std::rc::Rc::new(std::cell::RefCell::new(None));
    let answer = pending.clone();
    app.services.file_dialog = Some(Box::new(move |_| {
        let (tx, rx) = std::sync::mpsc::channel();
        *answer.borrow_mut() = Some(tx);
        rx
    }));
    app.run("file.open", json!({})).expect("pending native picker");
    frame(&mut app, &mut menu, egui::RawInput::default());
    let edit = submenu(&main_menu(), "Edit");
    assert_eq!(edit.itemAtIndex(index(&edit, "Copy")).expect("Copy").action(), Some(objc2::sel!(copy:)));
    assert_eq!(edit.itemAtIndex(index(&edit, "Paste")).expect("Paste").action(), Some(objc2::sel!(paste:)));
    assert_eq!(edit.itemAtIndex(index(&edit, "Select All")).expect("Select All").action(), Some(objc2::sel!(selectAll:)));
    pending.borrow_mut().take().expect("answer sender").send(Ok(Vec::new())).expect("cancel picker");
    // The real host calls logic every frame to receive the file dialog's answer.
    let mut out = ctx.run_ui(egui::RawInput::default(), |ui| app.logic(ui.ctx()));
    out.textures_delta.clear();
    frame(&mut app, &mut menu, egui::RawInput::default());
    let edit = submenu(&main_menu(), "Edit");
    assert_ne!(edit.itemAtIndex(index(&edit, "Copy")).expect("Copy").action(), Some(objc2::sel!(copy:)));

    // Language changes rebuild the labels and keep the native roles registered.
    app.ui.language = "ja".into();
    wordcraft_ui_egui::i18n::set_current(wordcraft_ui_egui::i18n::Lang::from_pref("ja"));
    frame(&mut app, &mut menu, egui::RawInput::default());
    let japanese = wordcraft_ui_egui::i18n::tr(wordcraft_ui_egui::i18n::Lang::from_pref("ja"), "Window").to_string();
    assert_eq!(nsapp.windowsMenu().map(|m| m.title().to_string()), Some(japanese), "the Window menu follows the interface language");

    println!(
        "mac_menu_appkit: ok (roles, live state, key equivalents, click/Return, modal submenus, save/cancel, native text selectors, localization)"
    );
}

#[cfg(not(target_os = "macos"))]
fn main() {}

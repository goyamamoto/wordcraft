//! View tab and status bar: views, zoom, show/hide, panes.

use serde_json::{Value, json};
use wordcraft_doc::{Zoom, ZoomFit};
use wordcraft_layout::{NumeralMode, ViewMode};

use super::sel_result;
use crate::{CmdError, CmdResult, CommandSpec, Session, p};

fn toggle(s: &mut Session, v: &Value, f: fn(&mut crate::ViewState) -> &mut bool) -> CmdResult {
    let cur = *f(&mut s.view);
    *f(&mut s.view) = p::bool(v, "value").unwrap_or(!cur);
    Ok(json!({"value": *f(&mut s.view)}))
}

pub fn specs() -> Vec<CommandSpec> {
    vec![
        CommandSpec::new("view.marks", "Show/Hide ¶", "Home › Paragraph", |s, v| {
            let r = toggle(s, v, |x| &mut x.marks);
            s.relayout();
            r
        })
        .key("Mod+Shift+8 / Mod+8")
        .pure(),
        CommandSpec::new("view.printLayout", "Print Layout", "View › Views", |s, _| mode(s, ViewMode::Print, false)).key("Mod+Alt+P").pure(),
        CommandSpec::new("view.webLayout", "Web Layout", "View › Views", |s, _| mode(s, ViewMode::Web, false)).pure(),
        CommandSpec::new("view.draft", "Draft", "View › Views", |s, _| mode(s, ViewMode::Draft, false)).key("Mod+Alt+N").pure(),
        // The body as an outline of its headings, with the Outlining tab (`outline.*`).
        CommandSpec::new("view.outline", "Outline", "View › Views", |s, _| mode(s, ViewMode::Outline, false)).key("Mod+Alt+O").pure(),
        CommandSpec::new("view.readMode", "Read Mode", "View › Views", |s, _| mode(s, ViewMode::Print, true)).pure(),
        CommandSpec::new("view.readColumnWidth", "Reading Column Width", "Reading › Layout", read_column_width)
            .params(r#"{"value": "auto" | "narrow" | "medium" | "wide" | "page"}"#)
            .pure(),
        CommandSpec::new("view.readTextSize", "Reading Text Size", "Reading › Layout", read_text_size)
            .params(r#"{"value": percent (75–150)}"#)
            .pure(),
        CommandSpec::new("view.readLineSpacing", "Reading Line Spacing", "Reading › Layout", read_line_spacing)
            .params(r#"{"value": "document" | 1.0 | 1.25 | 1.5 | 2.0}"#)
            .pure(),
        // Print Layout: hide the white space (margins, headers, footers) between pages. Screen only.
        CommandSpec::new("view.hideWhiteSpace", "Hide White Space", "View › Show", |s, v| toggle(s, v, |x| &mut x.hide_white_space))
            .params(r#"{"value"?: bool}"#)
            .pure(),
        // Focus and Full Screen View (#1678) fill the screen; turning one on turns the other off.
        // Full Screen View has no key of its own: Customize Keyboard binds one.
        CommandSpec::new("view.focus", "Focus", "View › Immersive", |s, v| {
            let r = toggle(s, v, |x| &mut x.focus_mode);
            s.view.full_screen_view &= !s.view.focus_mode;
            r
        })
        .params(r#"{"value"?: bool}"#)
        .pure(),
        CommandSpec::new("view.fullScreenView", "Full Screen View", "View › Immersive", |s, v| {
            let r = toggle(s, v, |x| &mut x.full_screen_view);
            s.view.focus_mode &= !s.view.full_screen_view;
            r
        })
        .params(r#"{"value"?: bool}"#)
        .pure(),
        CommandSpec::new("view.ruler", "Ruler", "View › Show", |s, v| toggle(s, v, |x| &mut x.ruler)).pure(),
        // Crop marks at the corners of the text area (screen only; Word › Preferences › View).
        // Without a value, toggles the setting in effect (the region's default until chosen).
        CommandSpec::new("view.cropMarks", "Crop marks", "File › Options › Display", |s, v| {
            let on = p::bool(v, "value").unwrap_or(!s.crop_marks());
            s.view.crop_marks = Some(on);
            Ok(json!({"value": on}))
        })
        .params(r#"{"value"?: bool}"#)
        .pure(),
        // Dotted boxes around the paragraphs' slots (screen only; Word › Preferences › View).
        CommandSpec::new("view.textBoundaries", "Text boundaries", "File › Options › Display", |s, v| toggle(s, v, |x| &mut x.text_boundaries))
            .params(r#"{"value"?: bool}"#)
            .pure(),
        // The drawing grid over the page (screen only; never printed or exported).
        CommandSpec::new("view.gridlines", "Gridlines", "View › Show", |s, v| toggle(s, v, |x| &mut x.gridlines))
            .params(r#"{"value"?: bool}"#)
            .pure(),
        // Outlines of table cells (screen only; never printed or exported).
        CommandSpec::new("table.viewGridlines", "View Gridlines", "Table Layout › Table", |s, v| toggle(s, v, |x| &mut x.table_gridlines))
            .params(r#"{"value"?: bool}"#)
            .pure(),
        CommandSpec::new("view.navigationPane", "Navigation Pane", "View › Show", |s, v| toggle(s, v, |x| &mut x.nav_pane)).pure(),
        CommandSpec::new("view.zoom", "Zoom", "View › Zoom", zoom)
            .params(r#"{"value": percent (10-500) | "pageWidth" | "onePage" | "multiplePages"}"#)
            .pure(),
        CommandSpec::new("view.zoom100", "100%", "View › Zoom", |s, _| zoom(s, &json!({"value": 100}))).pure(),
        CommandSpec::new("view.zoomIn", "Zoom In", "View › Zoom", |s, _| {
            let z = ((s.view.zoom * 10.0).round() / 10.0 + 0.1).min(5.0);
            zoom(s, &json!({"value": z * 100.0}))
        })
        .pure(),
        CommandSpec::new("view.zoomOut", "Zoom Out", "View › Zoom", |s, _| {
            let z = ((s.view.zoom * 10.0).round() / 10.0 - 0.1).max(0.1);
            zoom(s, &json!({"value": z * 100.0}))
        })
        .pure(),
        CommandSpec::new("view.onePage", "One Page", "View › Zoom", |s, _| zoom(s, &json!({"value": "onePage"}))).pure(),
        CommandSpec::new("view.multiplePages", "Multiple Pages", "View › Zoom", |s, _| zoom(s, &json!({"value": "multiplePages"}))).pure(),
        CommandSpec::new("view.pageWidth", "Page Width", "View › Zoom", |s, _| zoom(s, &json!({"value": "pageWidth"}))).pure(),
        CommandSpec::new("view.darkMode", "Switch Modes", "View › Dark Mode", |s, v| toggle(s, v, |x| &mut x.dark_mode)).pure(),
        // The interface theme itself (light, dark, or follow the OS), next to dark pages (#1861).
        // The session can't paint: like split and new windows, this queues a request the front
        // end applies to its theme setting (which persists per user). Without a value it toggles
        // light and dark, as the manual switch did.
        CommandSpec::new("view.interfaceTheme", "Interface Theme", "View › Dark Mode", |s, v| match v.get("value") {
            None => {
                s.push_ui_request(json!({"toggle": "interfaceTheme"}));
                Ok(json!({"value": "toggle"}))
            }
            Some(Value::String(name)) => match name.as_str() {
                "light" | "dark" | "system" => {
                    s.push_ui_request(json!({"interfaceTheme": name}));
                    Ok(json!({"value": name}))
                }
                _ => Err(CmdError::Params("interface theme: light|dark|system".into())),
            },
            _ => Err(CmdError::Params("interface theme: light|dark|system".into())),
        })
        .params(r#"{"value"?: "light" | "dark" | "system"; without it, toggles light and dark}"#)
        .pure(),
        CommandSpec::new("view.stylesPane", "Styles Pane", "Home › Styles", |s, v| toggle(s, v, |x| &mut x.styles_pane)).pure(),
        CommandSpec::new("view.commentsPane", "Comments Pane", "Review › Comments", |s, v| toggle(s, v, |x| &mut x.comments_pane)).pure(),
        CommandSpec::new("view.accessibilityPane", "Accessibility Pane", "Review › Accessibility", |s, v| {
            toggle(s, v, |x| &mut x.accessibility_pane)
        })
        .params(r#"{"value"?: bool}"#)
        .pure(),
        CommandSpec::new("view.altTextPane", "Alt Text Pane", "Review › Accessibility", |s, v| toggle(s, v, |x| &mut x.alt_text_pane))
            .params(r#"{"value"?: bool}"#)
            .pure(),
        CommandSpec::new("view.formatShapePane", "Format Shape Pane", "Shape Format › Shape Styles", |s, v| {
            toggle(s, v, |x| &mut x.format_shape_pane)
        })
        .params(r#"{"value"?: bool}"#)
        .pure(),
        // View › Window (#322). The session owns no window: the desktop app does, one per
        // document (`wordcraft_ui_egui::windows`), and reports the open ones in
        // [`Session::windows`] before each frame. These commands check their parameters against
        // that list and queue a request the front end carries out; without a window host
        // (headless sessions, the browser, the CLI) they are disabled.
        //
        // New Window: a second window on this document (the host shares the session between
        // them, each window with a view of its own, `Session::swap_view`); Split: two panes on it
        // in this window. The front end carries both out.
        CommandSpec::new("view.newWindow", "New Window", "View › Window", |s, _| {
            s.push_ui_request(json!({"open": "newWindow"}));
            sel_result(s)
        })
        .when(needs_window_host)
        .pure(),
        CommandSpec::new("view.split", "Split", "View › Window", |s, _| {
            s.push_ui_request(json!({"toggle": "split"}));
            sel_result(s)
        })
        .key("Mod+Alt+S")
        .pure(),
        CommandSpec::new("view.arrangeAll", "Arrange All", "View › Window", |s, _| {
            s.push_ui_request(json!({"window": "arrangeAll"}));
            Ok(json!({"windows": s.windows.count()}))
        })
        .when(needs_window_host)
        .pure(),
        CommandSpec::new("view.sideBySide", "View Side by Side", "View › Window", side_by_side)
            .params(r#"{"value"?: bool, "with"?: window id (another open window, needed with three or more open; the one other window otherwise)}"#)
            .when(needs_two_windows)
            .pure(),
        CommandSpec::new("view.syncScroll", "Synchronous Scrolling", "View › Window", |s, v| {
            let on = p::bool(v, "value").unwrap_or(!s.windows.sync_scroll);
            s.windows.sync_scroll = on;
            s.push_ui_request(json!({"window": "syncScroll", "value": on}));
            Ok(json!({"value": on}))
        })
        .params(r#"{"value"?: bool}"#)
        .when(needs_side_by_side)
        .pure(),
        CommandSpec::new("view.resetWindowPosition", "Reset Window Position", "View › Window", |s, _| {
            s.push_ui_request(json!({"window": "resetPosition"}));
            Ok(json!({"with": s.windows.side_by_side}))
        })
        .when(needs_side_by_side)
        .pure(),
        CommandSpec::new("view.switchWindows", "Switch Windows", "View › Window", |s, v| {
            let Some(n) = p::u64(v, "window") else {
                return Err(CmdError::Params("`window` (a window id, or its number in the Switch Windows list) is required".into()));
            };
            let Some(to) = s.windows.resolve(n) else { return Err(CmdError::Params(format!("no window {n} is open"))) };
            s.push_ui_request(json!({"window": "switch", "to": to}));
            Ok(json!({"window": to}))
        })
        .params(r#"{"window": window id, or its 1-based number in the Switch Windows list}"#)
        .when(needs_window_host)
        .pure(),
        // The window fills the screen, OS-level full screen (#980). F11 as in browsers and most
        // desktop apps; ⌃⌘F first on macOS, where Ctrl is a key of its own (elsewhere Mod is Ctrl,
        // so "Mod+Ctrl+F" would read Ctrl+Ctrl+F). Escape also leaves it. The front end applies it.
        CommandSpec::new("view.fullScreen", "Full Screen", "View › Window", |s, v| toggle(s, v, |x| &mut x.full_screen))
            .params(r#"{"value"?: bool}"#)
            .key(super::mac_or("Mod+Ctrl+F / F11", "F11"))
            .when(full_screen_available)
            .pure(),
        CommandSpec::new("view.state", "View State", "View", |s, _| serde_json::to_value(&s.view).map_err(|e| CmdError::Failed(e.to_string())))
            .pure(),
        // Word › File › Options › Advanced › Numeral: how digits look on screen, in print, PDFs
        // and page images (display only; the document text keeps the typed codepoints).
        CommandSpec::new("view.numeral", "Numerals", "File › Options › Advanced", |s, v| match v.get("value") {
            None => Ok(json!({"value": s.view.numeral})),
            Some(Value::String(name)) => {
                let Some(mode) = NumeralMode::parse(name) else {
                    return Err(CmdError::Params("numeral mode: arabic|hindi|context|system".into()));
                };
                s.view.numeral = mode;
                s.relayout();
                Ok(json!({"value": s.view.numeral}))
            }
            Some(_) => Err(CmdError::Params("numeral mode: arabic|hindi|context|system".into())),
        })
        .params(r#"{"value"?: "arabic|hindi|context|system"}"#)
        .pure(),
    ]
}

/// The browser keeps full screen to itself (its own F11); the web app can't ask for it.
/// The View › Window commands work in the desktop app, which has windows ([`Session::windows`]).
fn needs_window_host(s: &Session) -> Option<&'static str> {
    (!s.windows.host).then_some("Only in the desktop app, which has windows")
}

/// View Side by Side needs another document window to compare with, as in Word.
fn needs_two_windows(s: &Session) -> Option<&'static str> {
    needs_window_host(s).or_else(|| (s.windows.others().next().is_none()).then_some("Open another document first"))
}

/// Synchronous Scrolling and Reset Window Position only work while View Side by Side is on, as
/// in Word.
fn needs_side_by_side(s: &Session) -> Option<&'static str> {
    needs_window_host(s).or_else(|| s.windows.side_by_side.is_none().then_some("Turn on View Side by Side first"))
}

/// View › Window › View Side by Side: pair this window with another, which turns Synchronous
/// Scrolling on too, as in Word; off again restores both windows. With one other window open
/// it is the partner; with more, `with` names it, or the front end asks which (Word's Compare
/// Side by Side dialog).
fn side_by_side(s: &mut Session, v: &Value) -> CmdResult {
    let on = p::bool(v, "value").unwrap_or(s.windows.side_by_side.is_none());
    if !on {
        s.windows.side_by_side = None;
        s.windows.sync_scroll = false;
        s.push_ui_request(json!({"window": "sideBySide", "value": false}));
        return Ok(json!({"value": false}));
    }
    let others: Vec<u64> = s.windows.others().map(|w| w.id).collect();
    let with = match p::u64(v, "with") {
        Some(n) => match s.windows.resolve(n).filter(|id| others.contains(id)) {
            Some(id) => id,
            None => return Err(CmdError::Params(format!("`with` must name another open window, not {n}"))),
        },
        None => match others.as_slice() {
            [one] => *one,
            [] => return Err(CmdError::Failed("Open another document first".into())),
            _ => {
                s.push_ui_request(json!({"window": "chooseSideBySide"}));
                return Ok(json!({"pending": "sideBySideWith"}));
            }
        },
    };
    s.windows.side_by_side = Some(with);
    s.windows.sync_scroll = true;
    s.push_ui_request(json!({"window": "sideBySide", "value": true, "with": with}));
    Ok(json!({"value": true, "with": with}))
}

fn full_screen_available(_: &Session) -> Option<&'static str> {
    cfg!(target_arch = "wasm32").then_some("Use the browser's full screen (F11)")
}

fn mode(s: &mut Session, m: ViewMode, read: bool) -> CmdResult {
    if read && !s.view.read_mode {
        // Reading controls are session-only, and reset on each entry to the view.
        s.view.read_column_width = None;
        s.view.read_zoom = 1.0;
        s.view.read_line_spacing = None;
    }
    s.view.mode = m;
    s.view.read_mode = read;
    s.relayout();
    Ok(json!({"mode": m, "readMode": read}))
}

fn read_column_width(s: &mut Session, v: &Value) -> CmdResult {
    let width = match v.get("value").and_then(Value::as_str) {
        Some("auto") => None,
        Some("narrow") => Some(180.0),
        Some("medium") => Some(240.0),
        Some("wide") => Some(350.0),
        Some("page") => Some(612.0),
        _ => return Err(CmdError::Params("column width must be auto, narrow, medium, wide, or page".into())),
    };
    s.view.read_column_width = width;
    Ok(json!({"width": width}))
}

fn read_text_size(s: &mut Session, v: &Value) -> CmdResult {
    let Some(pct) = v.get("value").and_then(Value::as_f64) else {
        return Err(CmdError::Params("reading text size must be a percentage".into()));
    };
    if !pct.is_finite() || !(75.0..=150.0).contains(&pct) {
        return Err(CmdError::Params("reading text size must be 75–150%".into()));
    }
    s.view.read_zoom = pct as f32 / 100.0;
    Ok(json!({"percent": pct}))
}

fn read_line_spacing(s: &mut Session, v: &Value) -> CmdResult {
    let value = v.get("value");
    let spacing = match value {
        Some(Value::String(s)) if s == "document" => None,
        Some(Value::Number(n)) if matches!(n.as_f64(), Some(1.0 | 1.25 | 1.5 | 2.0)) => n.as_f64().map(|n| n as f32),
        _ => return Err(CmdError::Params("reading line spacing must be document, 1, 1.25, 1.5, or 2".into())),
    };
    s.view.read_line_spacing = spacing;
    Ok(json!({"spacing": spacing}))
}

/// The zoom a document is saved with (#995): the fit mode, else the percentage; `None` at 100%.
pub(crate) fn saved_zoom(v: &crate::ViewState) -> Option<Zoom> {
    let fit = match v.fit.as_str() {
        "onePage" => Some(ZoomFit::OnePage),
        "pageWidth" => Some(ZoomFit::PageWidth),
        "textWidth" => Some(ZoomFit::TextWidth),
        _ => None,
    };
    let percent = if v.zoom.is_finite() { (v.zoom * 100.0).round().clamp(10.0, 500.0) as u32 } else { 100 };
    Some(Zoom { percent, fit }).filter(|z| z.percent != 100 || z.fit.is_some())
}

/// Show an opened document at the zoom it was saved with.
pub(crate) fn restore_zoom(s: &mut Session, z: Zoom) {
    let fit = match z.fit {
        Some(ZoomFit::OnePage) => "onePage",
        Some(ZoomFit::PageWidth) => "pageWidth",
        Some(ZoomFit::TextWidth) => "textWidth",
        None => "",
    };
    s.view.zoom = z.percent.clamp(10, 500) as f32 / 100.0;
    s.view.fit = fit.into();
    s.view.multi_page = false;
    s.view.single_column = fit == "onePage" || fit == "pageWidth";
}

fn zoom(s: &mut Session, v: &Value) -> CmdResult {
    match v.get("value") {
        Some(Value::String(f)) if ["pageWidth", "onePage", "multiplePages", "textWidth"].contains(&f.as_str()) => {
            s.view.fit = f.clone();
            s.view.multi_page = f == "multiplePages";
            s.view.single_column = f == "onePage" || f == "pageWidth";
        }
        Some(x) => {
            let pct = x.as_f64().ok_or_else(|| CmdError::Params("zoom percent or fit mode".into()))? as f32;
            if !(10.0..=500.0).contains(&pct) {
                return Err(CmdError::Params("zoom must be 10–500%".into()));
            }
            s.view.zoom = pct / 100.0;
            s.view.fit.clear();
            s.view.multi_page = false;
        }
        None => {
            s.push_ui_request(json!({"open": "zoom"}));
        }
    }
    Ok(json!({"zoom": s.view.zoom, "fit": s.view.fit}))
}

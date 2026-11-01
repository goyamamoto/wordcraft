use super::{Dialog, Value, WordApp, json, vec2};

/// The Font dialog preview's size: the Size box's points between 6 and 72, else 12. `"NaN"` and
/// `"inf"` parse as floats too, and a NaN font size trips epaint's glyph assertion (#3663).
pub(super) fn preview_size(size: &str) -> f32 {
    size.trim().parse::<f32>().ok().filter(|pt| pt.is_finite()).unwrap_or(12.0).clamp(6.0, 72.0)
}

/// The Font dialog's emphasis marks: `w:em` value and label.
pub const EMPHASIS_MARKS: [(&str, &str); 5] = [("none", "None"), ("dot", "Dot"), ("comma", "Comma"), ("circle", "Circle"), ("underDot", "Under dot")];

/// Word's Underline style list in the Font dialog: `(none)`, `Words only` and the sixteen line
/// styles, each drawn as a sample of itself as Word lists them. `label` names it for assistive
/// technology.
pub(super) fn underline_style_combo(ui: &mut egui::Ui, underline: &mut String, color: egui::Color32, label: egui::Id) {
    use wordcraft_doc::props::Underline;
    let current = Underline::from_ooxml(underline);
    let text = |u: Underline| match u {
        Underline::None => tl!("(none)"),
        Underline::Words => tl!("Words only"),
        // A space keeps the row height; the sample is painted over it.
        _ => " ",
    };
    let r = craft_ui::ComboBox::from_id_salt("font_ul").width(180.0).selected_text(text(current)).show_ui(ui, |ui| {
        ui.set_min_width(170.0);
        for u in Underline::ALL {
            let selected = u == current;
            let label = text(u);
            let resp = if label.trim().is_empty() {
                let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width().max(160.0), 18.0), egui::Sense::click());
                if selected || resp.hovered() {
                    let fill = if selected { ui.visuals().selection.bg_fill } else { ui.visuals().widgets.hovered.weak_bg_fill };
                    ui.painter().rect_filled(rect, 2.0, fill);
                }
                paint_underline_sample(ui.painter(), rect.shrink2(vec2(6.0, 0.0)), u, ui.visuals().text_color());
                resp.widget_info(|| egui::WidgetInfo::selected(egui::Role::Button, true, selected, u.ooxml()));
                resp
            } else {
                ui.selectable_label(selected, label)
            };
            if resp.clicked() {
                *underline = u.ooxml().to_string();
            }
        }
    });
    let r_response = r.response.labelled_by(label);
    if text(current).trim().is_empty() {
        let rect = r_response.rect;
        let sample = egui::Rect::from_min_max(egui::pos2(rect.min.x + 8.0, rect.min.y), egui::pos2(rect.max.x - 24.0, rect.max.y));
        paint_underline_sample(ui.painter(), sample, current, color);
    }
}

/// A sample of underline `kind` across `rect` (on its centre line), as the Font dialog's list
/// and preview show it: heavy styles two pixels, the others one, Word's patterns in proportion.
pub(super) fn paint_underline_sample(p: &egui::Painter, rect: egui::Rect, kind: wordcraft_doc::props::Underline, color: egui::Color32) {
    use wordcraft_doc::props::Underline;
    if rect.width() < 2.0 {
        return;
    }
    let (x0, x1, y) = (rect.left(), rect.right(), rect.center().y);
    let thin = egui::Stroke::new(1.0, color);
    let heavy = egui::Stroke::new(2.0, color);
    let stroke = if kind.is_heavy() { heavy } else { thin };
    let line = |yy: f32, s: egui::Stroke| {
        p.line_segment([egui::pos2(x0, yy), egui::pos2(x1, yy)], s);
    };
    let dashes = |yy: f32, s: egui::Stroke, d: &[f32], g: &[f32]| {
        p.extend(egui::Shape::dashed_line_with_offset(&[egui::pos2(x0, yy), egui::pos2(x1, yy)], s, d, g, 0.0));
    };
    let wave = |yy: f32, amp: f32, s: egui::Stroke| {
        let n = ((x1 - x0) / 3.0).ceil().clamp(1.0, 400.0) as usize;
        let pts: Vec<egui::Pos2> =
            (0..=n).map(|i| egui::pos2(x0 + (x1 - x0) * i as f32 / n as f32, yy + if i % 2 == 0 { -amp } else { amp })).collect();
        p.add(egui::Shape::line(pts, s));
    };
    match kind {
        Underline::None | Underline::Words => {}
        Underline::Single | Underline::Thick => line(y, stroke),
        Underline::Double => {
            line(y - 1.5, thin);
            line(y + 1.5, thin);
        }
        Underline::Dotted | Underline::DottedHeavy => dashes(y, stroke, &[2.0], &[2.0]),
        Underline::Dash | Underline::DashedHeavy => dashes(y, stroke, &[6.0], &[3.0]),
        Underline::DashLong | Underline::DashLongHeavy => dashes(y, stroke, &[12.0], &[6.0]),
        Underline::DotDash | Underline::DashDotHeavy => dashes(y, stroke, &[5.0, 1.5], &[1.5, 1.5]),
        Underline::DotDotDash | Underline::DashDotDotHeavy => dashes(y, stroke, &[4.0, 1.5, 1.5], &[1.5, 1.5, 1.5]),
        Underline::Wave => wave(y, 1.5, thin),
        Underline::WavyHeavy => wave(y, 1.5, heavy),
        Underline::DoubleWave => {
            wave(y - 2.0, 1.0, thin);
            wave(y + 2.0, 1.0, thin);
        }
    }
}

/// The character properties the Font dialog's fields stand for.
pub(super) fn font_props(d: &Dialog) -> serde_json::Map<String, Value> {
    let Dialog::Font {
        font,
        font_ea,
        size,
        bold,
        italic,
        underline,
        strike,
        sup,
        sub,
        small_caps,
        caps,
        hidden,
        color,
        spacing,
        kern,
        kern_at,
        em,
        ..
    } = d
    else {
        return serde_json::Map::new();
    };
    let mut props = serde_json::Map::new();
    props.insert("bold".into(), json!(*bold));
    props.insert("italic".into(), json!(*italic));
    props.insert("strike".into(), json!(*strike));
    props.insert("underline".into(), json!(underline));
    let vert = if *sup {
        "superscript"
    } else if *sub {
        "subscript"
    } else {
        "baseline"
    };
    props.insert("vertAlign".into(), json!(vert));
    props.insert("smallCaps".into(), json!(*small_caps));
    props.insert("caps".into(), json!(*caps));
    props.insert("hidden".into(), json!(*hidden));
    props.insert("spacing".into(), json!(*spacing));
    if EMPHASIS_MARKS.iter().any(|(v, _)| v == em) {
        props.insert("em".into(), json!(em));
    }
    match kern {
        Some(true) => {
            props.insert("kern".into(), json!(kern_at.clamp(1.0, 1638.0)));
        }
        Some(false) => {
            props.insert("kern".into(), json!(0.0));
        }
        None => {}
    }
    if !font.trim().is_empty() {
        props.insert("font".into(), json!(font.trim()));
    }
    if !font_ea.trim().is_empty() {
        props.insert("fontEa".into(), json!(font_ea.trim()));
    }
    if let Ok(s) = size.trim().parse::<f64>() {
        props.insert("size".into(), json!(s));
    }
    if let Some(c) = wordcraft_doc::Rgb::parse(color) {
        props.insert("color".into(), json!({"Rgb": [c.0, c.1, c.2]}));
    } else if color.eq_ignore_ascii_case("auto") {
        // Automatic, from the colour field's drop-down (#606).
        props.insert("color".into(), json!("Auto"));
    }
    props
}

/// Is `c` Chinese, Japanese or Korean text (ideographs, kana, Hangul, CJK punctuation, fullwidth
/// forms)?
pub(super) fn east_asian_char(c: char) -> bool {
    matches!(u32::from(c), 0x1100..=0x11FF | 0x2E80..=0x9FFF | 0xA960..=0xA97F | 0xAC00..=0xD7FF | 0xF900..=0xFAFF | 0xFE30..=0xFE4F | 0xFF00..=0xFFEF | 0x20000..=0x3FFFF)
}

/// The Font dialog's OK: one formatting step that sets only the properties the user changed, so
/// formatting the dialog can't show or left alone stays as it is. Fonts are set by their stored
/// (English) names, the Latin font for Persian/Arabic text too (as `format.font`).
pub(super) fn apply_font(app: &mut WordApp, d: &Dialog) {
    let Dialog::Font { start, .. } = d else { return };
    let mut changed: serde_json::Map<String, Value> = font_props(d).into_iter().filter(|(k, v)| start.get(k) != Some(v)).collect();
    if let Some(f) = changed.get("font").and_then(Value::as_str).map(str::to_string) {
        let f = app.previews.font_stored(&f);
        changed.insert("font".into(), json!(f));
        changed.insert("fontCs".into(), json!(f));
    }
    if let Some(f) = changed.get("fontEa").and_then(Value::as_str).map(str::to_string) {
        changed.insert("fontEa".into(), json!(app.previews.font_stored(&f)));
    }
    if !changed.is_empty() {
        let _ = app.run("format.set", json!({"props": changed}));
    }
}

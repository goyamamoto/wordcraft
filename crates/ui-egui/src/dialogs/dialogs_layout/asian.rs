use super::{Serialize, Ui, Value, WordApp, json, run, semibold};

// ---------------------------------------------------------------------------------------------
// Asian Layout (Home › Paragraph › Asian Layout)

/// What `format.state` says about the selection's `w:eastAsianLayout`, for the dialogs.
fn east_asian_layout(app: &WordApp) -> Value {
    wordcraft_engine::cmd::format::state(&app.session).get("eastAsianLayout").cloned().unwrap_or(Value::Null)
}

/// The selected text, one line (the word at the caret counts, as the commands take it).
fn selection_text(app: &WordApp) -> String {
    app.session.selected_text().replace(['\r', '\n'], " ")
}

/// OK, a Remove button (enabled when `removable`) and Cancel; returns (ok, remove, cancel).
fn buttons_with_remove(ui: &mut Ui, removable: bool) -> (bool, bool, bool) {
    let mut r = (false, false, false);
    ui.add_space(8.0);
    ui.horizontal(|ui| {
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui.button(tl!("Cancel")).clicked() || crate::dialogs::cancel_key_pressed(ui) {
                r.2 = true;
            }
            if ui.add_enabled(removable, egui::Button::new(tl!("Remove"))).clicked() {
                r.1 = true;
            }
            if ui.add(crate::widgets::primary_button(ui, tl!("OK"))).clicked() {
                r.0 = true;
            }
        });
    });
    r
}

/// Horizontal in Vertical (縦中横): the selected text lies horizontal in vertical text.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HorizontalInVerticalForm {
    pub text: String,
    /// Fit in line (`w:vertCompress`).
    pub fit: bool,
    /// The selection already has it (Remove is enabled).
    pub current: bool,
}

impl HorizontalInVerticalForm {
    pub fn read(app: &WordApp) -> HorizontalInVerticalForm {
        let l = east_asian_layout(app);
        let current = l.get("vert").and_then(Value::as_bool).unwrap_or(false);
        let fit = l.get("vertCompress").and_then(Value::as_bool).unwrap_or(false);
        HorizontalInVerticalForm { text: selection_text(app), fit, current }
    }
}

pub(super) fn horizontal_in_vertical_ui(app: &mut WordApp, ui: &mut Ui, f: &mut HorizontalInVerticalForm) -> bool {
    ui.horizontal(|ui| {
        ui.label(tl!("Text:"));
        ui.label(egui::RichText::new(f.text.as_str()).font(semibold(12.5)));
    });
    ui.checkbox(&mut f.fit, tl!("Fit in line"));
    let (ok, remove, cancel) = buttons_with_remove(ui, f.current);
    if ok {
        return run(app, "format.horizontalInVertical", json!({"compress": f.fit}));
    }
    if remove {
        return run(app, "format.horizontalInVertical", json!({"off": true}));
    }
    cancel
}

/// Combine Characters (組み文字): up to six characters in one character's cell.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CombineCharactersForm {
    pub text: String,
    pub current: bool,
}

impl CombineCharactersForm {
    pub fn read(app: &WordApp) -> CombineCharactersForm {
        let current = east_asian_layout(app).get("combineChars").and_then(Value::as_bool).unwrap_or(false);
        CombineCharactersForm { text: selection_text(app), current }
    }
    /// Word's limit: one to six characters.
    pub fn fits(&self) -> bool {
        (1..=wordcraft_doc::props::EastAsianLayout::COMBINE_CHARS_MAX).contains(&self.text.chars().count())
    }
}

pub(super) fn combine_characters_ui(app: &mut WordApp, ui: &mut Ui, f: &mut CombineCharactersForm) -> bool {
    ui.horizontal(|ui| {
        ui.label(tl!("Text:"));
        ui.label(egui::RichText::new(f.text.as_str()).font(semibold(12.5)));
    });
    if !f.fits() {
        ui.label(egui::RichText::new(tl!("Select one to six characters.")).weak());
    }
    let (ok, remove, cancel) = buttons_with_remove(ui, f.current);
    if ok {
        return run(app, "format.combineCharacters", json!({}));
    }
    if remove {
        return run(app, "format.combineCharacters", json!({"off": true}));
    }
    cancel
}

/// The bracket choices of Two Lines in One: OOXML name and label.
pub const COMBINE_BRACKETS: [(&str, &str); 5] = [("none", "None"), ("round", "( )"), ("square", "[ ]"), ("angle", "< >"), ("curly", "{ }")];

/// Two Lines in One (割注): the selected text as two half-size lines, optionally in brackets.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TwoLinesInOneForm {
    pub text: String,
    /// `none|round|square|angle|curly`.
    pub brackets: String,
    pub current: bool,
}

impl TwoLinesInOneForm {
    pub fn read(app: &WordApp) -> TwoLinesInOneForm {
        let l = east_asian_layout(app);
        let current = l.get("combine").and_then(Value::as_bool).unwrap_or(false);
        let brackets = l.get("combineBrackets").and_then(Value::as_str).filter(|_| current).unwrap_or("none").to_string();
        TwoLinesInOneForm { text: selection_text(app), brackets, current }
    }
}

pub(super) fn two_lines_in_one_ui(app: &mut WordApp, ui: &mut Ui, f: &mut TwoLinesInOneForm) -> bool {
    ui.horizontal(|ui| {
        ui.label(tl!("Text:"));
        ui.label(egui::RichText::new(f.text.as_str()).font(semibold(12.5)));
    });
    ui.label(tl!("Enclose in characters:"));
    for (v, l) in COMBINE_BRACKETS {
        let label = if v == "none" { tl!(l).to_string() } else { l.to_string() };
        ui.radio_value(&mut f.brackets, v.to_string(), label);
    }
    let (ok, remove, cancel) = buttons_with_remove(ui, f.current);
    if ok {
        return run(app, "format.twoLinesInOne", json!({"brackets": f.brackets}));
    }
    if remove {
        return run(app, "format.twoLinesInOne", json!({"off": true}));
    }
    cancel
}

/// Adjust Width (文字の均等割り付け): the selected text spread to a width in characters (at
/// the text's font size) or points.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AdjustWidthForm {
    pub text: String,
    /// The text's font size (what one character measures).
    pub size: f32,
    /// The current width in points: the block's, else the text's natural length in characters
    /// at the font size.
    pub current_width: f32,
    /// The selection already has a width (Remove is enabled).
    pub current: bool,
    /// New width in the chosen unit.
    pub width: f32,
    /// Unit: true = characters, false = points.
    pub chars: bool,
}

impl AdjustWidthForm {
    pub fn read(app: &WordApp) -> AdjustWidthForm {
        let st = wordcraft_engine::cmd::format::state(&app.session);
        let size = st.get("size").and_then(Value::as_f64).map(|v| v as f32).filter(|v| v.is_finite() && *v > 0.0).unwrap_or(10.5);
        let fit = st.get("fitText").and_then(Value::as_f64).map(|v| v as f32).filter(|v| v.is_finite() && *v > 0.0);
        let text = selection_text(app);
        let n = text.chars().count() as f32;
        let current_width = fit.unwrap_or(n * size);
        AdjustWidthForm { text, size, current_width, current: fit.is_some(), width: current_width / size, chars: true }
    }
    /// `format.fitText` parameters for OK.
    pub fn params(&self) -> Value {
        if self.chars { json!({"chars": self.width}) } else { json!({"width": self.width}) }
    }
}

pub(super) fn adjust_width_ui(app: &mut WordApp, ui: &mut Ui, f: &mut AdjustWidthForm) -> bool {
    ui.horizontal(|ui| {
        ui.label(tl!("Text:"));
        ui.label(egui::RichText::new(f.text.as_str()).font(semibold(12.5)));
    });
    egui::Grid::new("adjust_width").num_columns(3).show(ui, |ui| {
        ui.label(tl!("Current text width:"));
        let current = if f.chars { f.current_width / f.size } else { f.current_width };
        ui.label(format!("{current:.2}"));
        ui.label(if f.chars { tl!("Characters") } else { tl!("Points") });
        ui.end_row();
        let width_label = ui.label(tl!("New text width:"));
        let range = if f.chars { 0.1..=200.0 } else { 0.1..=1584.0 };
        ui.add(egui::DragValue::new(&mut f.width).speed(if f.chars { 0.05 } else { 0.5 }).range(range).max_decimals(2));
        let shown = if f.chars { tl!("Characters") } else { tl!("Points") };
        let combo = craft_ui::ComboBox::from_id_salt("adjust_width_unit").selected_text(shown).show_ui(ui, |ui| {
            if ui.selectable_label(f.chars, tl!("Characters")).clicked() && !f.chars {
                f.chars = true;
                f.width /= f.size.max(0.1);
            }
            if ui.selectable_label(!f.chars, tl!("Points")).clicked() && f.chars {
                f.chars = false;
                f.width *= f.size;
            }
        });
        combo.response.labelled_by(width_label.id);
        ui.end_row();
    });
    let (ok, remove, cancel) = buttons_with_remove(ui, f.current);
    if ok {
        return run(app, "format.fitText", f.params());
    }
    if remove {
        return run(app, "format.fitText", json!({"off": true}));
    }
    cancel
}

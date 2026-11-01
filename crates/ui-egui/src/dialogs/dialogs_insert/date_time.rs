//! Date and Time (Insert › Text › Date & Time).

use super::field::{caret_is_japanese, date_formats};
use super::{Serialize, Stroke, Tokens, Ui, Value, WordApp, buttons, json};

/// The Date and Time dialog: Word's list of formats for a language (the text at the caret's,
/// Japanese on Japanese text), Update automatically (a DATE field instead of text), and OK runs
/// `insert.dateTime`.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DateTimeForm {
    /// The picture to insert.
    pub format: String,
    /// Japanese formats (和暦, 曜日…) rather than the English list.
    pub japanese: bool,
    /// Insert a DATE field that updates, not plain text.
    pub update: bool,
}

impl DateTimeForm {
    pub fn new(app: &WordApp) -> Self {
        let japanese = caret_is_japanese(app);
        Self { format: date_formats(japanese).first().copied().unwrap_or("M/d/yyyy").to_string(), japanese, update: false }
    }

    /// Switch the language's list; the first format of the new list is selected.
    pub fn set_japanese(&mut self, japanese: bool) {
        if self.japanese != japanese {
            self.japanese = japanese;
            self.format = date_formats(japanese).first().copied().unwrap_or("M/d/yyyy").to_string();
        }
    }

    pub fn params(&self) -> Value {
        json!({"format": self.format, "update": self.update})
    }
}

pub(super) fn date_time_ui(app: &mut WordApp, ui: &mut Ui, f: &mut DateTimeForm) -> bool {
    let t = Tokens::get(ui.ctx());
    ui.set_width(440.0);
    // The Japanese samples (和暦, 曜日) need the CJK interface font (else □).
    if f.japanese {
        app.want_system_cjk = true;
    }
    ui.horizontal_top(|ui| {
        ui.vertical(|ui| {
            ui.set_max_width(250.0);
            ui.label(tl!("Available formats:"));
            egui::Frame::new().stroke(Stroke::new(1.0, t.border)).inner_margin(2.0).show(ui, |ui| {
                egui::ScrollArea::vertical()
                    .id_salt("date_time_formats")
                    .max_height(230.0)
                    .min_scrolled_height(230.0)
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        ui.set_min_width(220.0);
                        for pic in date_formats(f.japanese) {
                            let sample = wordcraft_engine::cmd::insert::format_date(pic);
                            if ui.selectable_label(f.format == *pic, sample).on_hover_text(*pic).clicked() {
                                f.format = (*pic).to_string();
                            }
                        }
                    });
            });
        });
        ui.add_space(12.0);
        ui.vertical(|ui| {
            ui.set_min_width(170.0);
            let language_label = ui.label(tl!("Language:"));
            let shown = if f.japanese { tl!("Japanese") } else { tl!("English (United States)") };
            let combo = craft_ui::ComboBox::from_id_salt("date_time_language").selected_text(shown).width(150.0).show_ui(ui, |ui| {
                let mut ja = f.japanese;
                ui.selectable_value(&mut ja, false, tl!("English (United States)"));
                ui.selectable_value(&mut ja, true, tl!("Japanese"));
                f.set_japanese(ja);
            });
            combo.response.labelled_by(language_label.id);
            ui.add_space(8.0);
            ui.checkbox(&mut f.update, tl!("Update automatically"));
        });
    });
    let (ok, cancel) = buttons(ui, "OK", "Cancel", !f.format.trim().is_empty());
    if ok {
        match app.run("insert.dateTime", f.params()) {
            Ok(_) => return true,
            Err(e) => app.status(e),
        }
    }
    cancel
}

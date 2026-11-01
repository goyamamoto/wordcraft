use super::{MeasureExt, Serialize, Ui, Value, WordApp, json, semibold};

/// Page Setup's Document Grid fields as Word's 文字数と行数 tab holds them: the grid kind (0 no
/// grid, 1 lines only, 2 lines and characters, 3 text snaps to characters), characters per line
/// with their pitch (字送り), lines per page with their pitch (行送り), and whether the character
/// pitch is the default one (the Normal style's size, `w:charSpace` 0). Counts and pitches follow
/// each other as in Word ([`reconcile_page_grid`]); `extent` is the text area (width, height)
/// the counts were taken over, so a margin edited in the dialog recounts them.
#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PageGrid {
    pub kind: u8,
    pub chars: u32,
    pub char_pitch: f32,
    pub default_pitch: bool,
    pub lines: u32,
    pub line_pitch: f32,
    pub extent: (f32, f32),
}

impl PageGrid {
    /// Whether OK has a grid to apply: the kind, a pitch or the default-pitch choice changed
    /// (the counts follow the pitches, and the extent is the margins' business).
    pub(super) fn differs(&self, was: &PageGrid) -> bool {
        self.kind != was.kind
            || self.default_pitch != was.default_pitch
            || (self.char_pitch - was.char_pitch).abs() > 1e-4
            || (self.line_pitch - was.line_pitch).abs() > 1e-4
    }
}

/// Updates the grid fields after an edit as Word's Page Setup does: the field that changed since
/// `prev` drives its partner — a count sets its pitch to the text extent over the count (Word for
/// Mac writes 30 lines of a 648-pt text height as a 432-twip pitch, 2026-10-19), a pitch sets its
/// count to the extent over the pitch rounded down, and Use default pitch puts the Normal style's
/// size back. `tw` and `th` are the text area's width and height in points, `size` the Normal
/// style's font size.
pub fn reconcile_page_grid(g: &mut PageGrid, prev: &PageGrid, tw: f32, th: f32, size: f32) {
    use wordcraft_doc::section::{MAX_GRID_PITCH, MIN_GRID_PITCH};
    let count = |extent: f32, pitch: f32| ((extent / pitch.max(MIN_GRID_PITCH) + 0.001).floor() as u32).clamp(1, 1000);
    let pitch = |extent: f32, n: u32| (extent / n.max(1) as f32).clamp(MIN_GRID_PITCH, MAX_GRID_PITCH);
    let size = if size.is_finite() { size.clamp(MIN_GRID_PITCH, MAX_GRID_PITCH) } else { 10.5 };
    let (tw, th) = (if tw.is_finite() { tw.max(18.0) } else { 18.0 }, if th.is_finite() { th.max(18.0) } else { 18.0 });
    let moved = (g.extent.0 - tw).abs() > 0.01 || (g.extent.1 - th).abs() > 0.01;
    g.extent = (tw, th);
    let mut recount = moved;
    if g.default_pitch && !prev.default_pitch {
        g.char_pitch = size;
        recount = true;
    } else if g.chars != prev.chars {
        g.char_pitch = pitch(tw, g.chars);
        g.default_pitch = false;
        recount = true;
    } else if (g.char_pitch - prev.char_pitch).abs() > 1e-4 {
        g.default_pitch = false;
        recount = true;
    }
    if !g.char_pitch.is_finite() {
        g.char_pitch = size;
        recount = true;
    }
    g.char_pitch = g.char_pitch.clamp(MIN_GRID_PITCH, MAX_GRID_PITCH);
    if recount || g.chars == 0 {
        g.chars = count(tw, g.char_pitch);
    }
    let mut recount = moved;
    if g.lines != prev.lines {
        g.line_pitch = pitch(th, g.lines);
        recount = true;
    } else if (g.line_pitch - prev.line_pitch).abs() > 1e-4 {
        recount = true;
    }
    if !g.line_pitch.is_finite() {
        g.line_pitch = 18.0;
        recount = true;
    }
    g.line_pitch = g.line_pitch.clamp(MIN_GRID_PITCH, MAX_GRID_PITCH);
    if recount || g.lines == 0 {
        g.lines = count(th, g.line_pitch);
    }
}

/// Labels of Page Setup's paper size (#1433), in one place in case Word's wording differs.
pub(crate) const PAPER_LABELS: PaperLabels =
    PaperLabels { group: "Paper", size: "Paper size:", custom: "Custom size", width: "Width:", height: "Height:", more: "More Paper Sizes…" };

pub(crate) struct PaperLabels {
    pub group: &'static str,
    pub size: &'static str,
    pub custom: &'static str,
    pub width: &'static str,
    pub height: &'static str,
    /// The Size menu's last item, opening Page Setup.
    pub more: &'static str,
}

/// The named paper size `paper` (in the measurement unit, either orientation) is, or None for a
/// custom size.
pub(crate) fn paper_entry(paper: (f32, f32)) -> Option<&'static str> {
    let ppu = crate::options::unit().pt_per_unit();
    wordcraft_geom::paper_name(paper.0 * ppu, paper.1 * ppu)
}

/// The commands Page Setup's OK runs for the paper, orientation and margins (#1433), all in the
/// measurement unit as the dialog shows them. Refuses when the margins don't fit the paper.
/// Otherwise it orders them so that none is refused on the way: the size first
/// when the section's margins fit it, else the margins first, else margins of 0 in between.
/// `layout.size` keeps the section's orientation; `layout.orientation` then turns the page and
/// its margins to what the dialog shows. A size equal to the opened one (either way round,
/// within 0.01 pt) doesn't run `layout.size`.
pub(super) fn page_layout_commands(
    app: &WordApp,
    (paper, was): ((f32, f32), (f32, f32)),
    landscape: bool,
    (top, bottom, left, right): (f32, f32, f32, f32),
) -> Result<Vec<(&'static str, Value)>, &'static str> {
    use wordcraft_engine::cmd::page::{MARGINS_DONT_FIT, margins_fit, sect};
    let ppu = crate::options::unit().pt_per_unit();
    let (paper, was) = ((paper.0 * ppu, paper.1 * ppu), (was.0 * ppu, was.1 * ppu));
    let m = (top * ppu, bottom * ppu, left * ppu, right * ppu);
    let cur = sect(&app.session);
    if !margins_fit(paper, m, cur.gutter) {
        return Err(MARGINS_DONT_FIT);
    }
    let turned = |on: bool, (a, b): (f32, f32)| if on { (a.max(b), a.min(b)) } else { (a.min(b), a.max(b)) };
    let ((w0, h0), (w1, h1)) = (turned(false, paper), turned(false, was));
    let new_size = (w0 - w1).abs() >= 0.01 || (h0 - h1).abs() >= 0.01;
    let size = ("layout.size", json!({"width": paper.0, "height": paper.1}));
    let orientation = ("layout.orientation", json!({"value": if landscape { "landscape" } else { "portrait" }}));
    let margins = ("layout.margins", json!({"top": m.0, "bottom": m.1, "left": m.2, "right": m.3}));
    let old_margins = (cur.margin_top, cur.margin_bottom, cur.margin_left, cur.margin_right);
    Ok(if !new_size || margins_fit(turned(cur.landscape, paper), old_margins, cur.gutter) {
        let mut v = if new_size { vec![size] } else { Vec::new() };
        v.extend([orientation, margins]);
        v
    } else if margins_fit(turned(landscape, (cur.page_w, cur.page_h)), m, cur.gutter) {
        vec![orientation, margins, size]
    } else {
        vec![orientation, ("layout.margins", json!({"top": 0.0, "bottom": 0.0, "left": 0.0, "right": 0.0})), size, margins]
    })
}

/// Run a dialog's commands as one undo step. If one fails, put the document and the undo stack
/// back as they were (nothing applied) and return its error.
pub(super) fn run_as_one_step(app: &mut WordApp, commands: Vec<(&str, Value)>) -> Result<(), String> {
    let snapshot = app.session.edit_snapshot();
    for (i, (id, params)) in commands.into_iter().enumerate() {
        if i > 0 {
            app.session.join_next_undo();
        }
        if let Err(e) = app.run(id, params) {
            app.session.restore(snapshot);
            return Err(e);
        }
    }
    Ok(())
}

/// Page Setup's Paper group (#1433): a Paper size list (the named sizes and Custom size) and
/// the width and height. The fields follow the orientation (the wide side is the width in
/// landscape), and a typed size that is wider than tall turns the orientation to landscape.
pub(super) fn page_paper_group(ui: &mut Ui, paper: &mut (f32, f32), landscape: &mut bool, custom: &mut bool) {
    if (*landscape && paper.0 < paper.1) || (!*landscape && paper.0 > paper.1) {
        *paper = (paper.1, paper.0);
    }
    let unit = crate::options::unit();
    let ppu = unit.pt_per_unit();
    let l = &PAPER_LABELS;
    ui.label(egui::RichText::new(tl!(l.group)).font(semibold(12.5)));
    ui.horizontal(|ui| {
        let label = ui.label(tl!(l.size));
        let entry = paper_entry(*paper).filter(|_| !*custom);
        let shown = tl!(entry.unwrap_or(l.custom));
        let combo = craft_ui::ComboBox::from_id_salt("ps_paper_size").width(180.0).height(400.0).selected_text(shown).show_ui(ui, |ui| {
            for (n, w, h) in wordcraft_geom::PAPER_SIZES {
                if ui.selectable_label(entry == Some(*n), tl!(n)).clicked() {
                    let (w, h) = (w / ppu, h / ppu);
                    *paper = if *landscape { (w.max(h), w.min(h)) } else { (w.min(h), w.max(h)) };
                    *custom = false;
                }
            }
            // Custom size keeps the numbers to edit, and stays chosen while they match a name.
            if ui.selectable_label(entry.is_none(), tl!(l.custom)).clicked() {
                *custom = true;
            }
        });
        combo.response.labelled_by(label.id);
    });
    egui::Grid::new("ps_paper").num_columns(4).show(ui, |ui| {
        use wordcraft_engine::cmd::page::{MAX_PAGE_SIDE, MIN_PAGE_SIDE};
        let range = MIN_PAGE_SIDE / ppu..=MAX_PAGE_SIDE / ppu;
        let width_label = ui.label(tl!(l.width));
        let w = ui
            .add(egui::DragValue::new(&mut paper.0).speed(3.6 / ppu).range(range.clone()).measure(unit).max_decimals(2))
            .labelled_by(width_label.id);
        let height_label = ui.label(tl!(l.height));
        let h = ui.add(egui::DragValue::new(&mut paper.1).speed(3.6 / ppu).range(range).measure(unit).max_decimals(2)).labelled_by(height_label.id);
        if (w.changed() || h.changed()) && paper.0 != paper.1 {
            *landscape = paper.0 > paper.1;
        }
        ui.end_row();
    });
}

/// Page Setup's "Vertical alignment:" box (Word's Layout tab).
pub(super) fn page_valign_combo(ui: &mut egui::Ui, valign: &mut wordcraft_doc::section::PageVAlign) {
    use wordcraft_doc::section::PageVAlign;
    const CHOICES: [(PageVAlign, &str); 4] =
        [(PageVAlign::Top, "Top"), (PageVAlign::Center, "Center"), (PageVAlign::Justify, "Justified"), (PageVAlign::Bottom, "Bottom")];
    ui.horizontal(|ui| {
        let label = ui.label(tl!("Vertical alignment:"));
        let shown = CHOICES.iter().find(|(v, _)| v == valign).map_or("", |(_, l)| tl!(l));
        let combo = craft_ui::ComboBox::from_id_salt("ps_valign").selected_text(shown).show_ui(ui, |ui| {
            for (v, l) in CHOICES {
                ui.selectable_value(valign, v, tl!(l));
            }
        });
        combo.response.labelled_by(label.id);
    });
}

/// `layout.verticalAlign`'s name for a value.
pub(super) fn page_valign_value(v: wordcraft_doc::section::PageVAlign) -> &'static str {
    use wordcraft_doc::section::PageVAlign;
    match v {
        PageVAlign::Top => "top",
        PageVAlign::Center => "center",
        PageVAlign::Justify => "justify",
        PageVAlign::Bottom => "bottom",
    }
}

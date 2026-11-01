use super::bidi::{Dir, bidi_levels, visual_order};
use super::builder::{Builder, left_out, overstrike};
use super::chars::caps_char;
use super::grid::{
    COMPRESSIBLE, CellClass, EA_CHAR, EA_CLOSING, LINE_END_CENTRED, LINE_END_COMPRESS, LINE_END_SQUEEZE, OPENING, auto_space, border_room,
    cell_classes, chain_flags, cluster_char, compress_punctuation, ea_space_flags, east_asian, fit_text_blocks, is_east_asian, snap_chars,
    space_widths,
};
use super::justify::hyphenation_points;
use super::lines::{break_lines, word_drop_cap_top};
use super::model::{ClKind, Cluster, Enclosed, Glyph, Label, LabelRun, ParaEnv, RubyLayout, undrawable_bullet};
use super::proof::{ProofRun, is_list_item, proof_issues};
use super::{
    Align, Arc, COLUMN_BREAK, CharProps, Document, FIELD_SHADING, InlineObject, LINE_BREAK, LineSpacing, NoteKind, OBJ, PAGE_BREAK, PTabAlign,
    PTabBase, ParaLayout, Paragraph, ResolvedChar, ResolvedPara, TabLeader, field_code_text, field_text_on, reference_link,
};

/// A run's formatting resolved in the paragraph's style and table.
struct Resolver<'a> {
    doc: &'a Document,
    style: Option<&'a str>,
    table: Option<&'a CharProps>,
}

impl Resolver<'_> {
    fn char(&self, c: &CharProps) -> Arc<ResolvedChar> {
        Arc::new(self.doc.styles.resolve_char_in(self.style, self.table, c))
    }
}

/// What is the same all through laying a paragraph out.
struct Cx<'a> {
    p: &'a Paragraph,
    env: &'a ParaEnv<'a>,
    rp: &'a ResolvedPara,
    res: Resolver<'a>,
    math_props: wordcraft_doc::math::MathProps,
    /// The first character, if it is to be a drop cap.
    drop_first: Option<char>,
}

/// Where a cluster for an object or a break goes in the paragraph: its text, style and first glyph.
#[derive(Clone, Copy)]
struct Slot {
    start: usize,
    end: usize,
    style: u32,
    g: u32,
}

impl Slot {
    fn cluster(self, kind: ClKind, adv: f32, (obj_h, obj_d): (f32, f32)) -> Cluster {
        Cluster {
            start: self.start,
            end: self.end,
            adv,
            kind,
            style: self.style,
            g0: self.g,
            g1: self.g,
            break_after: false,
            obj_h,
            obj_d,
            dot: false,
        }
    }
}

/// The paragraph as shaped so far, with what the layout takes from its contents.
struct Shaped<'a> {
    b: Builder<'a>,
    has_page_fields: bool,
    notes: Vec<(usize, u32)>,
    obj_index: usize,
    drop_cap: Option<(usize, u8, f32)>,
    drop_cap_w: f32,
    /// How far the drop cap's ink reaches below its baseline (see `ParaLayout::drop_cap_ink`).
    drop_cap_ink: f32,
    maths: Vec<(usize, Arc<crate::math::MathLayout>)>,
    enclosures: Vec<(usize, Enclosed)>,
    rubies: Vec<RubyLayout>,
    displays: Vec<(usize, wordcraft_doc::math::MathJc)>,
    ptabs: Vec<(usize, PTabAlign, PTabBase, TabLeader)>,
    eq_counter: u32,
    /// The byte ranges of runs left out of the layout, in order, adjacent runs merged.
    left: Vec<std::ops::Range<usize>>,
    /// The byte ranges of objects with nothing to show (bookmarks, comment anchors), in order.
    bare: Vec<std::ops::Range<usize>>,
    /// Each run's proofing language and whether it is checked (spelling, grammar, hyphenation).
    runs: Vec<ProofRun>,
    /// The end of the Asian layout block (割注, 組み文字, 縦中横) laid out as one piece: the runs
    /// up to it are done.
    block_end: usize,
}

fn is_special(c: char) -> bool {
    matches!(c, '\t' | LINE_BREAK | PAGE_BREAK | COLUMN_BREAK | OBJ | '\r')
}

/// The paragraph's formatting: its style's, the table's and the list level's indents.
fn paragraph_props(p: &Paragraph, env: &ParaEnv) -> ResolvedPara {
    let doc = env.doc;
    let mut rp = doc.styles.resolve_para_in(&p.props, env.table.map(|t| &t.para));
    if let Some(spacing) = env.read_line_spacing.filter(|s| s.is_finite() && (1.0..=2.0).contains(s)) {
        rp.line_spacing = wordcraft_doc::props::LineSpacing::Multiple(spacing);
    }
    let (para_style, table_chr) = (p.props.style.as_deref(), env.table.map(|t| &t.chr));
    // Spacing in lines (`w:beforeLines`/`w:afterLines`): a line is the section grid's pitch, or
    // the Normal style's size without one (Word for Mac, measured with 18 and 24 pt grids and
    // with Normal at 12 pt; the paragraph's own 20 pt made no difference). It wins over points.
    if rp.space_before_lines.is_some() || rp.space_after_lines.is_some() {
        let unit = env.grid.line.unwrap_or_else(|| {
            let size = doc.styles.resolve_char(Some("Normal"), &CharProps::default()).size;
            if size.is_finite() { size.clamp(1.0, 1584.0) } else { 12.0 }
        });
        if let Some(l) = rp.space_before_lines {
            rp.space_before = (l * unit).clamp(0.0, 1584.0);
            rp.before_auto = false;
        }
        if let Some(l) = rp.space_after_lines {
            rp.space_after = (l * unit).clamp(0.0, 1584.0);
            rp.after_auto = false;
        }
    }
    // Indents in characters, as Word for Mac measures them (`ci1`–`ci3` probes, 2026-10-19): the
    // left and right indents in the Normal style's size, the first-line and hanging indents in
    // the paragraph's first character's size (its mark's when it is empty), each plus the
    // character grid's gap (`w:charSpace`) when the section has one: 1 ch = 11.7 pt on Word's
    // 18-pt lines-and-characters grid at 10.5 pt, 17.2 for 16-pt text.
    if rp.indent_chars.iter().any(|c| *c != 0.0) {
        let gap = env.grid.char_gap();
        let normal = doc.styles.resolve_char(Some("Normal"), &CharProps::default()).size;
        let first = p.runs.iter().find(|r| r.len > 0).map_or(&p.mark, |r| &r.props);
        let first = doc.styles.resolve_char_in(para_style, table_chr, first).size;
        rp.set_char_units(normal + gap, first + gap);
    }
    list_indents(&mut rp, p, env);
    rp
}

/// List level indents apply unless the paragraph sets its own, or, for a list from the style,
/// the style giving the list (or one based on it) does (`indents_over_list`). A paragraph
/// that switches its style's list off (`num: 0`) drops the styles' indents, as Word does.
fn list_indents(rp: &mut ResolvedPara, p: &Paragraph, env: &ParaEnv) {
    let own_left = p.props.indent_left.is_some() || p.props.indent_left_chars.is_some_and(|c| c != 0.0);
    let own_first = p.props.indent_first.is_some() || p.props.indent_first_chars.is_some_and(|c| c != 0.0);
    if let Some((_, lvl)) = &env.label {
        let (style_left, style_first) =
            if p.props.numbering.is_some() { (false, false) } else { env.doc.styles.indents_over_list(p.props.style.as_deref().unwrap_or("Normal")) };
        if !own_left && !style_left {
            rp.indent_left = lvl.indent;
        }
        if !own_first && !style_first {
            rp.indent_first = -lvl.hanging;
        }
    } else if p.props.numbering.is_some_and(|n| n.num == 0) {
        if !own_left {
            rp.indent_left = 0.0;
        }
        if !own_first {
            rp.indent_first = 0.0;
        }
    }
}

/// The first character, if it is to be a drop cap.
fn drop_cap_char(p: &Paragraph, rp: &ResolvedPara) -> Option<char> {
    (rp.drop_cap > 0)
        .then(|| p.text.chars().next())
        .flatten()
        .filter(|c| !matches!(*c, '\t' | LINE_BREAK | PAGE_BREAK | COLUMN_BREAK | OBJ | '\r' | ' '))
        .filter(|c| p.text.len() > c.len_utf8())
}

/// A field's result with the link of the reference it makes, if its run has none.
fn linked(rc: &Arc<ResolvedChar>, instr: &str) -> Arc<ResolvedChar> {
    match reference_link(instr) {
        Some(link) if rc.link.is_none() => {
            let mut linked = (**rc).clone();
            linked.link = Some(link);
            Arc::new(linked)
        }
        _ => rc.clone(),
    }
}

impl<'a> Shaped<'a> {
    fn new(b: Builder<'a>, eq_counter: u32) -> Self {
        Shaped {
            b,
            has_page_fields: false,
            notes: Vec::new(),
            obj_index: 0,
            drop_cap: None,
            drop_cap_w: 0.0,
            drop_cap_ink: 0.0,
            maths: Vec::new(),
            enclosures: Vec::new(),
            rubies: Vec::new(),
            displays: Vec::new(),
            ptabs: Vec::new(),
            eq_counter,
            left: Vec::new(),
            bare: Vec::new(),
            runs: Vec::new(),
            block_end: 0,
        }
    }

    fn push(&mut self, slot: Slot, kind: ClKind, adv: f32, h: f32) {
        self.b.clusters.push(slot.cluster(kind, adv, (h, 0.0)));
    }

    /// Shape one run of the paragraph.
    fn run(&mut self, cx: &Cx, range: std::ops::Range<usize>, props: &CharProps) {
        let rc = cx.res.char(props);
        self.runs.push(ProofRun::of(range.clone(), &rc));
        if range.end <= self.block_end {
            return;
        }
        let Some(text) = cx.p.text.get(range.clone()) else { return };
        if self.asian_layout_block(cx, &rc, &range) {
            return;
        }
        if left_out(cx.env.doc, &rc, self.b.env.show_hidden, self.b.env.hide_deleted) {
            self.left_out_run(cx, &rc, &range, text);
        } else {
            self.visible_run(cx, (&rc, props), &range, text);
        }
    }

    /// An Asian layout block (割注, 組み文字, 縦中横) starting with this run, laid out as one
    /// piece; false when the run is none, or the block holds a tab, a break or an object.
    fn asian_layout_block(&mut self, cx: &Cx, rc: &Arc<ResolvedChar>, range: &std::ops::Range<usize>) -> bool {
        let p = cx.p;
        let Some(l) = rc.east_asian_layout else { return false };
        if left_out(cx.env.doc, rc, self.b.env.show_hidden, self.b.env.hide_deleted) {
            return false;
        }
        // Adjacent runs of the same block (Word splits a block it also formats) are one piece.
        let mut end = range.end;
        for (r2, p2) in p.run_ranges().skip_while(|(r, _)| r.start < range.end) {
            if r2.start != end || cx.res.char(p2).east_asian_layout != Some(l) {
                break;
            }
            end = r2.end;
        }
        let Some(t) = p.text.get(range.start..end).filter(|t| !t.is_empty() && !t.chars().any(is_special)) else { return false };
        if l.vert {
            self.b.shape_turned(t, range.start, end, rc);
        } else if l.combine {
            self.b.shape_stacked(t, range.start, end, rc, false, l.combine_brackets.chars());
        } else if l.combine_chars {
            self.b.shape_stacked(t, range.start, end, rc, true, None);
        } else {
            self.b.shape(t, range.start, rc, None);
        }
        self.block_end = end;
        true
    }

    /// A run left out of the layout: markers for its characters, so offsets stay.
    fn left_out_run(&mut self, cx: &Cx, rc: &Arc<ResolvedChar>, range: &std::ops::Range<usize>, text: &str) {
        match self.left.last_mut() {
            Some(last) if last.end == range.start => last.end = range.end,
            _ => self.left.push(range.clone()),
        }
        let si = self.b.style(rc, None, false, None);
        let g = self.b.glyphs.len() as u32;
        // Word still prints the note of a hidden reference mark (and numbers it); a deleted
        // reference is gone from the final text, note and all.
        let keeps_notes = !self.b.env.hide_deleted.hides(cx.env.doc, rc.del);
        for (i, c) in text.char_indices() {
            let mut note = None;
            if c == OBJ {
                if let Some(InlineObject::NoteRef { id, .. }) = cx.p.objects.get(self.obj_index)
                    && keeps_notes
                {
                    note = Some(*id);
                }
                self.obj_index += 1;
            }
            if let Some(id) = note {
                self.notes.push((self.b.clusters.len(), id));
            }
            let slot = Slot { start: range.start + i, end: range.start + i + c.len_utf8(), style: si, g };
            self.push(slot, ClKind::Marker, 0.0, 0.0);
        }
    }

    /// A run shown: its text, with the breaks and objects in it as clusters of their own.
    fn visible_run(&mut self, cx: &Cx, (rc, props): (&Arc<ResolvedChar>, &CharProps), range: &std::ops::Range<usize>, text: &str) {
        let mut seg = self.drop_cap_letter(cx, rc, text, range.start);
        for (i, c) in text.char_indices() {
            if i < seg || !is_special(c) {
                continue;
            }
            if let Some(s) = text.get(seg..i) {
                self.b.shape(s, range.start + seg, rc, None);
            }
            seg = i + c.len_utf8();
            let style = self.b.style(rc, None, false, None);
            let slot = Slot { start: range.start + i, end: range.start + i + c.len_utf8(), style, g: self.b.glyphs.len() as u32 };
            self.special(cx, c, slot, (rc, props));
        }
        if let Some(s) = text.get(seg..) {
            self.b.shape(s, range.start + seg, rc, None);
        }
    }

    /// Drop cap: the first character, sized as Word for Mac sizes it (`dc1`–`dc5` probes,
    /// 2026-10-19): the letter keeps the size its run asks for when that is larger than the
    /// text's (a Word file stores the size Word computed); else it is sized, in half points
    /// rounded down, so that its ink spans from `word_drop_cap_top` above the first line's
    /// baseline to line N's baseline (漢 38.5 pt, あ 42.5 and W 50.5 for three 10.5-pt lines of
    /// 游明朝 and Aptos; T 56 for three 12-pt Times New Roman lines). The text starts at the
    /// letter's advance (no gap), and the letter's ink bottom sits on line N's baseline
    /// (`drop_cap_ink`). Returns how much of the run's text it took.
    fn drop_cap_letter(&mut self, cx: &Cx, rc: &Arc<ResolvedChar>, text: &str, start: usize) -> usize {
        let rp = cx.rp;
        let Some(c) = cx.drop_first.filter(|_| start == 0) else { return 0 };
        let Some(first) = text.get(..c.len_utf8()) else { return 0 };
        let lines = f32::from(rp.drop_cap);
        // The text beside the letter: the run after it when the letter is a run of its own
        // (a Word file's frame run), else this run.
        let body = if text.len() == c.len_utf8() {
            cx.p.run_ranges().find(|(r, _)| r.start == c.len_utf8()).map_or_else(|| rc.clone(), |(_, cp)| cx.res.char(cp))
        } else {
            rc.clone()
        };
        let mut big = (**rc).clone();
        // A Word file's frame run carries the lowering Word computed (`w:position`): the
        // letter is placed by its ink here instead.
        big.position = 0.0;
        big.vert_align = wordcraft_doc::props::VertAlign::Baseline;
        if let Some(f) = &rp.drop_cap_font {
            big.font = f.clone();
        }
        let before = self.b.clusters.len();
        if rc.size <= body.size + 0.5 {
            big.size = self.drop_cap_size(cx, (c, first), &body, big.clone(), lines);
        }
        self.b.shape(first, 0, &Arc::new(big), None);
        let w: f32 = self.b.clusters.get(before..).map(|c| c.iter().map(|c| c.adv).sum()).unwrap_or(0.0);
        self.drop_cap_ink = self.ink_below(before);
        // Dropped: the first lines step aside by the letter and its distance from the text.
        // In margin: they keep their indent and the letter hangs left of them.
        let indent = if rp.drop_cap_margin { 0.0 } else { w + rp.drop_cap_distance };
        self.drop_cap = Some((self.b.clusters.len() - before, rp.drop_cap, indent));
        self.drop_cap_w = w;
        // The letter hangs beside the text; lines flow at the indent `break_lines` adds.
        for c in self.b.clusters.iter_mut().skip(before) {
            c.adv = 0.0;
        }
        c.len_utf8()
    }

    /// The size of a drop cap letter `first` (the character `c`) whose ink spans `lines` lines
    /// of `body` text (see [`Shaped::drop_cap_letter`]).
    fn drop_cap_size(&mut self, cx: &Cx, (c, first): (char, &str), body: &Arc<ResolvedChar>, mut big: ResolvedChar, lines: f32) -> f32 {
        let rp = cx.rp;
        let (before, gbefore) = (self.b.clusters.len(), self.b.glyphs.len());
        // The text's line pitch, as `break_lines` sizes its lines.
        let pitch = {
            let si = self.b.style(body, None, false, None);
            let natural = self.b.styles.get(si as usize).map_or(body.size * 1.2, |s| s.ascent + s.descent);
            let grid = Some(self.b.env.grid).filter(|g| g.line.is_some() && rp.snap_to_grid);
            match (rp.line_spacing, grid) {
                (LineSpacing::Exactly(v), _) => v,
                (LineSpacing::Multiple(m), Some(g)) => g.snap_height(natural) * m,
                (LineSpacing::Multiple(m), None) => natural * m,
                (LineSpacing::AtLeast(v), Some(g)) => g.snap_height(natural).max(v),
                (LineSpacing::AtLeast(v), None) => natural.max(v),
            }
        };
        // The family the letter was asked for: Office's Yu fonts have their own rule.
        let requested: Option<String> = if east_asian(c) {
            big.font_ea.clone().or_else(|| cx.env.doc.settings.east_asian_font(false, big.lang_ea.as_deref(), first).map(str::to_string))
        } else {
            Some(big.font.clone())
        };
        // Shape at the text's size to learn the letter's face and ink, then size it.
        big.size = body.size;
        self.b.shape(first, 0, &Arc::new(big), None);
        let b = &self.b;
        let sized =
            b.clusters.get(before).and_then(|c| Some((b.styles.get(c.style as usize)?, b.glyphs.get(c.g0 as usize)?.gid))).and_then(|(st, gid)| {
                let (_, y0, _, y1) = wordcraft_fonts::math::glyph_bounds(&st.face, gid)?;
                let h = (y1 - y0) / st.face.upem as f32;
                (h > 0.05).then(|| ((lines - 1.0) * pitch + word_drop_cap_top(requested.as_deref(), &st.face) * body.size) / h)
            });
        self.b.clusters.truncate(before);
        self.b.glyphs.truncate(gbefore);
        sized
            .map(|s| (s * 2.0).floor() / 2.0)
            .unwrap_or_else(|| {
                // Nothing to measure (a blank glyph): a capital's ~0.7 em spans the lines.
                let line_h = (body.size * 1.2).max(1.0);
                ((lines - 1.0) * line_h + body.size * 0.7) / 0.7
            })
            .min(1000.0)
            .max(body.size)
    }

    /// How far below its baseline the ink of the cluster `k` reaches (negative: above it).
    fn ink_below(&self, k: usize) -> f32 {
        let b = &self.b;
        b.clusters
            .get(k)
            .and_then(|c| Some((b.styles.get(c.style as usize)?, b.glyphs.get(c.g0 as usize)?.gid)))
            .and_then(|(st, gid)| wordcraft_fonts::math::glyph_bounds(&st.face, gid).map(|(_, y0, _, _)| y0 * st.size / st.face.upem as f32))
            .filter(|v| v.is_finite())
            .unwrap_or(0.0)
    }

    /// The cluster for the special character `c` (a tab, a break or an object).
    fn special(&mut self, cx: &Cx, c: char, slot: Slot, (rc, props): (&Arc<ResolvedChar>, &CharProps)) {
        match c {
            '\t' => self.push(slot, ClKind::Tab, 0.0, 0.0),
            LINE_BREAK | '\r' => self.push(slot, ClKind::LineBreak, 0.0, 0.0),
            // Word ignores explicit page breaks in cells, preserving their source offsets.
            PAGE_BREAK if cx.env.table.is_some() => self.push(slot, ClKind::Marker, 0.0, 0.0),
            PAGE_BREAK => self.push(slot, ClKind::PageBreak, 0.0, 0.0),
            COLUMN_BREAK => self.push(slot, ClKind::ColumnBreak, 0.0, 0.0),
            _ => {
                let k = self.obj_index;
                self.obj_index += 1;
                self.object(cx, k, slot, (rc, props));
            }
        }
    }

    /// The cluster for the object `k`.
    fn object(&mut self, cx: &Cx, k: usize, slot: Slot, (rc, props): (&Arc<ResolvedChar>, &CharProps)) {
        let Some(o) = cx.p.objects.get(k) else {
            self.bare_marker(slot);
            return;
        };
        match o {
            _ if o.is_drawing() => self.drawing(cx, (k, o), slot),
            InlineObject::Field { .. } => self.field(cx, (k, o), slot, rc),
            InlineObject::NoteRef { kind, id, custom } => self.note_ref(cx, (*kind, *id, custom), slot, rc),
            InlineObject::Equation { linear, display, math } => self.equation(cx, k, slot, rc, (linear, *display, math)),
            // A note separator's line: a short one, or one across the column.
            _ if o.as_note_separator().is_some() => {
                let cont = o.as_note_separator() == Some(true);
                let maxw = (cx.env.width - cx.rp.indent_left.max(0.0) - cx.rp.indent_right.max(0.0)).max(1.0);
                self.push(slot, ClKind::Object(k), if cont { maxw } else { crate::NOTE_SEP_W.min(maxw) }, 0.0);
            }
            InlineObject::Ruby { .. } => self.ruby(cx, o, slot, (rc, props)),
            InlineObject::Opaque { text, .. } => self.b.shape_atomic(text, slot.start, slot.end, rc),
            InlineObject::PositionalTab { align, relative_to, leader } => {
                self.push(slot, ClKind::Tab, 0.0, 0.0);
                self.ptabs.push((self.b.clusters.len() - 1, *align, *relative_to, *leader));
            }
            _ => self.bare_marker(slot),
        }
    }

    fn bare_marker(&mut self, slot: Slot) {
        self.bare.push(slot.start..slot.end);
        self.push(slot, ClKind::Marker, 0.0, 0.0);
    }

    /// A picture, shape or text box: inline, as wide and high as it is; a floating one takes no room.
    fn drawing(&mut self, cx: &Cx, (k, o): (usize, &InlineObject), slot: Slot) {
        let env = cx.env;
        // A hidden one takes no room, and with no height its line doesn't draw it.
        if let Some((w, saved_h, float)) = o.frame()
            && float.wrap == wordcraft_doc::para::Wrap::Inline
            && !float.hidden
        {
            // A picture wider than the line keeps its size, as in Word: it runs past
            // the margin and the page edge clips it (see `ParaEnv::fit_objects`).
            let maxw = (env.width - cx.rp.indent_left.max(0.0) - cx.rp.indent_right.max(0.0)).max(18.0);
            let h = env.fitted.iter().find(|(i, _)| *i == k).map_or(saved_h, |(_, h)| *h);
            let (w, h) = (w.clamp(1.0, 4000.0), h.clamp(1.0, 4000.0));
            let s = if env.fit_objects && w > maxw { maxw / w } else { 1.0 };
            // The line makes room for effects (shadows) around the picture too, and
            // for a rotated one's bounds, as Word does.
            let [el, et, er, eb] = float.effect_extent();
            let (px, py) = float.spin_pad(w * s, h * s);
            self.push(slot, ClKind::Object(k), w * s + el + er + 2.0 * px, h * s + et + eb + 2.0 * py);
        } else {
            self.push(slot, ClKind::Object(k), 0.0, 0.0);
        }
    }

    /// A field: a legacy form field, its code in braces, or its result.
    fn field(&mut self, cx: &Cx, (k, o): (usize, &InlineObject), slot: Slot, rc: &Arc<ResolvedChar>) {
        let InlineObject::Field { instr, result, code, .. } = o else { return };
        let env = cx.env;
        let form = if env.field_codes != *code { None } else { o.legacy_form() };
        if let Some(wordcraft_doc::legacy_form::LegacyForm::CheckBox { size, .. }) = form {
            self.check_box((k, slot), rc, size);
        } else if let Some(text) = form.as_ref().and_then(|f| f.shown_text()) {
            // A drop-down shows its selected entry, whatever result was saved.
            self.b.shape_atomic(text, slot.start, slot.end, rc);
        } else if env.field_codes != *code {
            // The code in braces on grey, like Word's field shading.
            let mut shaded = (**rc).clone();
            if shaded.highlight.is_none() && shaded.shading.is_none() {
                shaded.shading = Some(FIELD_SHADING);
            }
            self.b.shape_atomic(&field_code_text(instr), slot.start, slot.end, &Arc::new(shaded));
        } else {
            self.field_result(cx, (instr, result), slot, rc);
        }
    }

    /// Word draws a legacy check box as a square as tall as the text's line (its ascent and
    /// descent), scaled to an explicit size, and takes that much room on the line (see
    /// `display::check_box`).
    fn check_box(&mut self, (k, slot): (usize, Slot), rc: &Arc<ResolvedChar>, size: Option<f32>) {
        let (a, d) = self.b.styles.get(slot.style as usize).map_or((rc.size * 0.8, rc.size * 0.2), |s| (s.ascent, s.descent));
        let scale = size.map_or(1.0, |sz| sz / rc.size.max(0.5));
        let (a, d) = (a * scale, d * scale);
        self.b.clusters.push(slot.cluster(ClKind::Object(k), a + d, (a.max(0.01), d)));
    }

    fn field_result(&mut self, cx: &Cx, (instr, result): (&str, &str), slot: Slot, rc: &Arc<ResolvedChar>) {
        let (start, end) = (slot.start, slot.end);
        match wordcraft_doc::eq::parse_overstrike(instr) {
            Some(o) => overstrike(&mut self.b, &o, instr, start, end, rc, &mut self.enclosures),
            None => {
                let (t, page_dep) = field_text_on(instr, result, cx.env.fields, cx.env.fields.page_at(start));
                self.has_page_fields |= page_dep;
                self.b.shape_atomic(&t, start, end, &linked(rc, instr));
            }
        }
    }

    /// A footnote or endnote reference: its mark, raised.
    fn note_ref(&mut self, cx: &Cx, (kind, id, custom): (NoteKind, u32, &str), slot: Slot, rc: &Arc<ResolvedChar>) {
        let doc = cx.env.doc;
        let num = if custom.is_empty() {
            let fmt = match kind {
                NoteKind::Footnote => doc.settings.footnote_format,
                NoteKind::Endnote => doc.settings.endnote_format,
            };
            cx.env.fields.note_mark(id, fmt)
        } else {
            custom.into()
        };
        let mut sup = (**rc).clone();
        sup.vert_align = wordcraft_doc::props::VertAlign::Superscript;
        self.b.shape_atomic(&num, slot.start, slot.end, &Arc::new(sup));
        self.notes.push((self.b.clusters.len().saturating_sub(1), id));
    }

    fn equation(&mut self, cx: &Cx, k: usize, slot: Slot, rc: &Arc<ResolvedChar>, (linear, display, math): (&str, bool, &wordcraft_doc::math::Math)) {
        let avail = (cx.env.width - cx.rp.indent_left.max(0.0) - cx.rp.indent_right.max(0.0)).max(12.0);
        let ml = crate::math::layout_equation(math, linear, rc, display, avail, &cx.math_props, &mut self.eq_counter);
        // The line is at least as tall as the text around it.
        let (a, d) = self.b.styles.get(slot.style as usize).map(|s| (s.ascent, s.descent)).unwrap_or((0.0, 0.0));
        self.b.clusters.push(slot.cluster(ClKind::Object(k), ml.width, (ml.ascent.max(a).max(0.01), ml.descent.max(d))));
        if display {
            self.displays.push((self.b.clusters.len() - 1, math.jc));
        }
        self.maths.push((k, Arc::new(ml)));
    }

    fn ruby(&mut self, cx: &Cx, o: &InlineObject, slot: Slot, (rc, props): (&Arc<ResolvedChar>, &CharProps)) {
        let InlineObject::Ruby { base, ruby, align, size, raise, props: ruby_props, .. } = o else { return };
        let mut rr = (*cx.res.char(&props.clone().overlaid(ruby_props))).clone();
        let size = if size.is_finite() { size.clamp(1.0, 1584.0) } else { rc.size * 0.5 };
        (rr.size, rr.size_cs, rr.position) = (size, size, 0.0);
        rr.vert_align = wordcraft_doc::props::VertAlign::Baseline;
        // Word for Mac draws the ruby a whole number of points above the base's baseline
        // (`rb4`/`rb5`: hpsRaise 1, 3, 5, 7, 9, 11, 13 → 0, 1, 2, 3, 4, 5, 6 pt; a raise of 0 puts
        // it on the baseline).
        let raise = if raise.is_finite() { raise.clamp(0.0, 1584.0).floor() } else { rc.size };
        if let Some(r) = self.b.ruby(base, ruby, *align, raise, (slot.start, slot.end), (rc, &Arc::new(rr))) {
            self.rubies.push(r);
        }
    }
}

/// The text break opportunities and hyphenation points come from: the paragraph's as laid out.
/// Hidden text that is not shown and, without markup, tracked deletions are left out, so a space
/// or soft hyphen there is no place to break, and words are hyphenated as they read. An object
/// with nothing to show is left out too: its U+FFFC has the line-break class CB, but a bookmark
/// never moves a break.
struct BreakText<'a> {
    text: std::borrow::Cow<'a, str>,
    /// For each char of `text`, its end there and in the paragraph.
    ends: Vec<(usize, usize)>,
    skipped: bool,
}

impl<'a> BreakText<'a> {
    fn new(source: &'a str, mut skip: Vec<std::ops::Range<usize>>) -> Self {
        skip.sort_unstable_by_key(|range| range.start);
        let (text, ends) = break_text(source, &skip);
        BreakText { text, ends, skipped: !skip.is_empty() }
    }

    /// The offset in the paragraph of the offset `i` in the text.
    fn to_para(&self, i: usize) -> Option<usize> {
        if self.skipped { self.ends.binary_search_by_key(&i, |end| end.0).ok().and_then(|k| self.ends.get(k)).map(|end| end.1) } else { Some(i) }
    }
}

/// Mark the clusters a line may break after; never after a `glued` one (inside a fit-text block
/// or a bracketed 割注).
fn mark_break_opportunities(cx: &Cx, clusters: &mut [Cluster], bt: &BreakText, glued: &[usize]) {
    let opps: std::collections::HashSet<usize> =
        break_positions(&bt.text, cx.rp, kinsoku_level(cx)).into_iter().filter_map(|i| bt.to_para(i)).collect();
    for c in clusters.iter_mut() {
        c.break_after = opps.contains(&c.end) || matches!(c.kind, ClKind::Object(_) | ClKind::Tab);
    }
    for k in glued {
        if let Some(c) = clusters.get_mut(*k) {
            c.break_after = false;
        }
    }
}

/// The kinsoku rules for the paragraph: custom lists (the settings' `w:noLineBreaksBefore`/
/// `After`) — the text's East Asian language's, else the first — or Word's standard or high
/// level (`w:strictFirstAndLastChars`).
fn kinsoku_level<'d>(cx: &Cx<'d>) -> crate::kinsoku::Level<'d> {
    let doc = cx.env.doc;
    let lists = &doc.settings.kinsoku_chars;
    let lang = cx.res.char(&cx.p.mark).lang_ea.clone();
    lang.as_deref()
        .and_then(|l| lists.iter().find(|k| k.lang.eq_ignore_ascii_case(l)))
        .or_else(|| lists.first())
        .map(|k| crate::kinsoku::Level::Custom { no_start: &k.no_start, no_end: &k.no_end })
        .unwrap_or(if doc.settings.strict_kinsoku { crate::kinsoku::Level::Strict } else { crate::kinsoku::Level::Standard })
}

/// The list label, shaped; none on a paragraph containing only a page break: Word hides it there.
/// The list counter has already advanced, so the following item keeps its number.
fn list_label(cx: &Cx, b: &mut Builder, mark_style: u32) -> Option<Label> {
    let (p, rp) = (cx.p, cx.rp);
    let break_only =
        b.clusters.iter().any(|c| c.kind == ClKind::PageBreak) && b.clusters.iter().all(|c| matches!(c.kind, ClKind::PageBreak | ClKind::Marker));
    let (text, lvl) = cx.env.label.as_ref().filter(|_| !break_only)?;
    let mut lc = p.mark.clone().overlaid(&lvl.chr);
    // A list number is never kerned: Word for Mac draws an ア． label two em wide where the
    // same ア． in text is kerned (`nl1`/`kn6` probes, 10.5-pt 游明朝, 2026-10-19).
    lc.kern = Some(0.0);
    let rc = cx.res.char(&lc);
    // A bullet no installed font can draw would come from whatever icon font has its
    // private-use code (Font Awesome's Twitter logo at U+F081, #1683): it shows the level's
    // default bullet instead.
    let fixed = undrawable_bullet(text, lvl, &rc)
        .then(|| wordcraft_doc::numbering::default_bullet(usize::from(rp.numbering.map_or(0, |n| n.level))).to_string());
    let text: &str = fixed.as_deref().unwrap_or(text);
    // A Symbol/Wingdings bullet (U+F0B7…) is drawn as its Unicode equivalent when the font
    // isn't installed (shaping maps it); the label's text reads as that character too.
    let shown = |t: &str| -> String { wordcraft_doc::symbol_fonts::map_text(&rc.font, t) };
    // The number of an East Asian format in the East Asian font (nl1 probe: ① in 游明朝).
    let cl = shape_label(b, text, &rc, rp.bidi, lvl.format.east_asian());
    let mut glyphs = Vec::new();
    let mut runs: Vec<LabelRun> = Vec::new();
    let mut ascent = 0.0f32;
    let mut x = 0.0;
    let mut style = mark_style;
    for c in &cl.clusters {
        style = c.style;
        if let Some(st) = b.styles.get(style as usize) {
            ascent = ascent.max(st.ascent + st.shift.max(0.0));
        }
        let g0 = glyphs.len();
        for g in cl.glyphs.get(c.g0 as usize..c.g1 as usize).into_iter().flatten() {
            glyphs.push(Glyph { dx: g.dx + x, ..*g });
        }
        // A label in an All Caps paragraph reads as the capitals drawn (#1473).
        let chars: String = shown(text.get(c.start..c.end).unwrap_or("")).chars().map(|ch| caps_char(&rc, ch)).collect();
        match runs.last_mut() {
            Some(r) if r.style == c.style => {
                r.glyphs.end = glyphs.len();
                r.text.push_str(&chars);
            }
            _ => runs.push(LabelRun { style: c.style, glyphs: g0..glyphs.len(), text: chars }),
        }
        x += c.adv;
    }
    Some(Label { text: shown(text), ascent, style, glyphs, runs, x: 0.0, width: x })
}

/// A label's clusters (in visual order) and the glyphs they index, shaped and taken back out of
/// the builder.
struct LabelShape {
    clusters: Vec<Cluster>,
    glyphs: Vec<Glyph>,
}

/// Shape a label's `text`. A right-to-left paragraph's label reads right to left too ("1." shows
/// as ".1"), and its brackets are mirrored (UAX #9 L4), which the shaper does for the runs the
/// levels report as right to left ("(" drawn as ")"), so "(1)" reads "(1)" and not ")1(".
/// `ea_hint`: the label is drawn in the East Asian font (see `Builder::shape_with`).
fn shape_label(b: &mut Builder, text: &str, rc: &Arc<ResolvedChar>, bidi: bool, ea_hint: bool) -> LabelShape {
    let lv = if bidi { bidi_levels(text, true) } else { Vec::new() };
    let before_c = b.clusters.len();
    let before_g = b.glyphs.len();
    b.shape_with(text, 0, rc, None, if lv.is_empty() { Dir::Guess } else { Dir::Own(&lv) }, ea_hint);
    let mut clusters: Vec<Cluster> = b.clusters.drain(before_c..).collect();
    if !lv.is_empty() {
        let cl_lv: Vec<u8> = clusters.iter().map(|c| lv.get(c.start).copied().unwrap_or(1)).collect();
        let order = visual_order(&cl_lv);
        clusters = order.iter().filter_map(|&i| clusters.get(i).cloned()).collect();
    }
    // The clusters index glyphs by position in the builder: keep the label's, with them rebased.
    let glyphs: Vec<Glyph> = b.glyphs.get(before_g..).map(<[Glyph]>::to_vec).unwrap_or_default();
    for c in &mut clusters {
        c.g0 = c.g0.saturating_sub(before_g as u32);
        c.g1 = c.g1.saturating_sub(before_g as u32);
    }
    b.glyphs.truncate(before_g);
    LabelShape { clusters, glyphs }
}

/// Where the paragraph's graphemes would be split by the shaping: shaping happens separately per
/// font/style segment. Segment-local grapheme starts can therefore land inside a paragraph
/// grapheme (e.g. an emoji whose components use different fallback faces). The drawing segments
/// are retained and only those illegal edit stops recorded.
fn joined_cluster_starts(p: &Paragraph, clusters: &[Cluster]) -> Vec<usize> {
    if p.text.is_ascii() {
        return Vec::new();
    }
    let boundaries = wordcraft_doc::cluster::cluster_starts(&p.text);
    let mut joined: Vec<usize> =
        clusters.iter().filter_map(|c| (c.start < p.text.len() && boundaries.binary_search(&c.start).is_err()).then_some(c.start)).collect();
    joined.sort_unstable();
    joined.dedup();
    joined
}

/// Lay out one paragraph.
pub fn layout_para(p: &Paragraph, env: &ParaEnv) -> ParaLayout {
    let doc = env.doc;
    let rp = paragraph_props(p, env);
    let res = Resolver { doc, style: p.props.style.as_deref(), table: env.table.map(|t| &t.chr) };
    let levels = bidi_levels(&p.text, rp.bidi);
    let b = Builder {
        env,
        styles: Vec::new(),
        style_index: Default::default(),
        glyphs: Vec::new(),
        clusters: Vec::new(),
        levels,
        shown: Vec::new(),
        glued: Vec::new(),
        tcy: Vec::new(),
    };
    let cx = Cx { p, env, rp: &rp, res, math_props: doc.settings.math.clone().unwrap_or_default(), drop_first: drop_cap_char(p, &rp) };
    let mut sh = Shaped::new(b, env.eq_number);
    let mark_rc = cx.res.char(&p.mark);
    let mark_style = sh.b.style(&mark_rc, None, false, None);
    for (range, props) in p.run_ranges() {
        sh.run(&cx, range, props);
    }
    let east = east_asian_spacing(&mut sh.b, p, &rp);
    let mut skip = sh.left.clone();
    skip.extend(sh.bare.iter().cloned());
    let bt = BreakText::new(&p.text, skip);
    let glued = std::mem::take(&mut sh.b.glued);
    mark_break_opportunities(&cx, &mut sh.b.clusters, &bt, &glued);
    let label = list_label(&cx, &mut sh.b, mark_style);
    let joined_cluster_starts = joined_cluster_starts(p, &sh.b.clusters);
    let Shaped { b, has_page_fields, notes, drop_cap, drop_cap_w, drop_cap_ink, maths, enclosures, rubies, displays, ptabs, left, runs, .. } = sh;
    let mut pl = ParaLayout {
        rp,
        styles: b.styles,
        glyphs: b.glyphs,
        clusters: b.clusters,
        shown: b.shown,
        lines: Vec::new(),
        label,
        height: 0.0,
        text_len: p.text.len(),
        joined_cluster_starts,
        has_page_fields,
        notes,
        issues: env.proofing.map(|o| proof_issues(p, &runs, &o, env.typing_word, is_list_item(doc, p))).unwrap_or_default(),
        drop_cap,
        hyph_after: Vec::new(),
        bidi_levels: b.levels,
        drop_cap_w,
        drop_cap_ink,
        maths,
        rubies,
        displays,
        ptabs,
        left_out: Vec::new(),
        enclosures,
        tcy: b.tcy,
        openings: Vec::new(),
        tail_compress: Vec::new(),
        tail_squeeze: Vec::new(),
        tail_centred: Vec::new(),
        cell_classes: east.cell_classes,
        cell_autos: east.cell_autos,
        ea_flags: east.ea_flags,
        align_dy: Vec::new(),
        vertical: env.vertical,
        auto_space: east.auto_space,
    };
    line_end_marks(&mut pl, &p.text);
    let auto = env.doc.settings.auto_hyphenation && !pl.rp.suppress_hyphens;
    pl.hyph_after = hyphenation_points(&bt.text, |i| bt.to_para(i), &pl, auto, env.doc.settings.hyphenate_caps, &runs);
    // A paragraph whose text and mark are all a deletion the layout leaves out (a deleted
    // paragraph in the final text, or moved-away text marked Hidden) is gone: it takes no room.
    // So is one whose mark is hidden and whose text is empty or hidden too: Word shows nothing of
    // it, not even its list number, though the number is counted (#3982).
    let mark_left_out = env.hide_deleted.hides(doc, mark_rc.del) || (mark_rc.hidden && !env.show_hidden);
    let gone = mark_left_out && (p.text.is_empty() || left.first() == Some(&(0..p.text.len())));
    pl.left_out = left;
    keep_graphemes_whole(&mut pl);
    break_lines(&mut pl, env, mark_style, env.label.as_ref().map(|(_, l)| l));
    if gone {
        collapse(&mut pl);
    }
    pl
}

/// What East Asian typography sets in a shaped paragraph, for the line breaking.
struct EastAsianSpacing {
    cell_classes: Vec<CellClass>,
    cell_autos: Vec<f32>,
    ea_flags: Vec<u8>,
    /// The automatic spaces, by cluster: the line fitting leaves the one after a line's last
    /// character out (#2358).
    auto_space: Vec<(usize, f32)>,
}

/// The East Asian spacing of the shaped clusters, as Word for Mac sets it: spaces' widths, the
/// character grid, automatic spacing between East Asian and Latin text, character borders'
/// room, punctuation compression and fit-text blocks.
fn east_asian_spacing(b: &mut Builder, p: &Paragraph, rp: &ResolvedPara) -> EastAsianSpacing {
    let env = b.env;
    let snapped = rp.snap_to_grid && env.grid.chars.is_some();
    let snap_cells = snapped && env.grid.cell.is_some();
    // Spaces take the East Asian width only with `w:balanceSingleByteDoubleByteWidth` (Word's
    // Japanese template sets it; `cp6` probes, 2026-10-19: without it あ a and ab␣␣cd keep the
    // Latin space, 2.7 pt, in every grid).
    let balance = env.doc.settings.balance_single_byte_double_byte_width;
    let ea_space = if balance { ea_space_flags(b, &p.text, snap_cells) } else { vec![false; b.clusters.len()] };
    space_widths(b, &ea_space, &p.text);
    let compresses = env.doc.settings.punct_compression.compresses();
    let chain = if snap_cells && compresses { chain_flags(b, &p.text) } else { Vec::new() };
    if snapped {
        snap_chars(b, &p.text, &env.grid, &chain, balance);
    }
    let autos = if rp.auto_space_de || rp.auto_space_dn {
        auto_space(b, &p.text, rp.auto_space_de, rp.auto_space_dn, &env.grid, snapped)
    } else {
        Vec::new()
    };
    border_room(b);
    let (cell_classes, cell_autos) = if snap_cells {
        let mut cell_autos = vec![0.0f32; b.clusters.len()];
        for (k, a) in &autos {
            if let Some(v) = cell_autos.get_mut(*k) {
                *v += a;
            }
        }
        (cell_classes(b, &p.text, &ea_space, &chain), cell_autos)
    } else {
        (Vec::new(), Vec::new())
    };
    if compresses {
        compress_punctuation(b, &p.text, snap_cells);
    }
    let ea_flags: Vec<u8> = b
        .clusters
        .iter()
        .map(|c| match cluster_char(&p.text, c) {
            Some(ch) if is_east_asian(ch) => EA_CHAR | if COMPRESSIBLE.contains(ch) { EA_CLOSING } else { 0 },
            _ => 0,
        })
        .collect();
    fit_text_blocks(b, &p.text);
    EastAsianSpacing { cell_classes, cell_autos, ea_flags, auto_space: autos }
}

/// The opening brackets `w:topLinePunct` halves at a line's start, and the closing marks a
/// justified or distributed line may end with past its edge (see `break_lines`).
fn line_end_marks(pl: &mut ParaLayout, text: &str) {
    let marks = |pl: &ParaLayout, set: &str| -> Vec<u32> {
        pl.clusters.iter().enumerate().filter(|(_, c)| cluster_char(text, c).is_some_and(|ch| set.contains(ch))).map(|(k, _)| k as u32).collect()
    };
    if pl.rp.top_line_punct {
        pl.openings = marks(pl, OPENING);
    }
    if matches!(pl.rp.align, Align::Justify | Align::Distribute) {
        pl.tail_compress = marks(pl, LINE_END_COMPRESS);
        pl.tail_squeeze = marks(pl, LINE_END_SQUEEZE);
        pl.tail_centred = marks(pl, LINE_END_CENTRED);
    }
}

/// Neither ordinary nor hyphenated wrapping may split a drawing group inside a grapheme.
fn keep_graphemes_whole(pl: &mut ParaLayout) {
    for c in &mut pl.clusters {
        c.break_after &= pl.joined_cluster_starts.binary_search(&c.end).is_err();
    }
    pl.hyph_after.retain(|&k| pl.clusters.get(k as usize).is_some_and(|c| pl.joined_cluster_starts.binary_search(&c.end).is_err()));
    for k in pl.hyph_after.clone() {
        if let Some(c) = pl.clusters.get_mut(k as usize) {
            c.break_after = true;
        }
    }
}

/// The width of the word that starts at each cluster: the clusters up to the first break
/// opportunity that isn't a hyphenation point, their advance, spaces aside.
///
/// The word is the same set the scan this replaces gathered, which stopped at that first
/// opportunity rather than running on to the end of the paragraph.
///
/// Every word is measured once, before the line is broken, so a word that has to be measured
/// again — it doesn't fit the span beside a floating object, so it moves on to the next row —
/// costs a lookup rather than another pass over it (#18, #3412).
///
/// The width is the sum of the advances, the negative ones included: a font that pulls a glyph
/// back takes the width off again, so a word whose advances alternate is measured by that sum
/// and not by how wide it gets on the way. The sum is taken over the whole word for that
/// reason, and the words after a break are left out of it: they are words of their own.
///
/// The sum is added up from the word's end rather than its start, which is the same number in
/// all but the last bits of a float: the sum a float holds depends on the order it is added in,
/// and the two differ by far less than the tenth of a point the comparison that uses the width
/// allows.
pub(crate) fn word_widths(clusters: &[Cluster], hyph_after: &[u32]) -> Vec<f32> {
    let n = clusters.len();
    let mut width = vec![0.0f32; n + 1];
    for k in (0..n).rev() {
        let c = &clusters[k];
        let adv = if c.kind == ClKind::Space { 0.0 } else { c.adv };
        // The word ends at a break opportunity. A hyphenation point isn't one: the break there
        // is optional, so the word runs on to the next.
        width[k] = if c.break_after && hyph_after.binary_search(&(k as u32)).is_err() { adv } else { adv + width[k + 1] };
    }
    width
}

/// Lays a paragraph out as nothing: no height, no spacing, no label, its lines (kept for the
/// caret and hit testing) all at its top with no height.
fn collapse(pl: &mut ParaLayout) {
    for l in &mut pl.lines {
        l.top = 0.0;
        l.height = 0.0;
        l.baseline = 0.0;
    }
    pl.height = 0.0;
    pl.rp.space_before = 0.0;
    pl.rp.space_after = 0.0;
    pl.label = None;
    // Its shading and borders would be drawn across the page at no height.
    pl.rp.shading = None;
    pl.rp.borders = None;
}

fn break_text<'a>(source: &'a str, skip: &[std::ops::Range<usize>]) -> (std::borrow::Cow<'a, str>, Vec<(usize, usize)>) {
    // The text and, for each char of it, its end there and in the paragraph.
    if skip.is_empty() {
        (source.into(), Vec::new())
    } else {
        let (mut text, mut ends) = (String::new(), Vec::new());
        let mut next = skip.iter().peekable();
        for (i, c) in source.char_indices() {
            // `skip` is in order: skip the ranges that end here, then is `i` in the next one?
            while next.next_if(|r| r.end <= i).is_some() {}
            if next.peek().is_some_and(|r| r.start <= i) {
                continue;
            }
            text.push(c);
            ends.push((text.len(), i + c.len_utf8()));
        }
        (text.into(), ends)
    }
}

fn break_positions(text: &str, rp: &wordcraft_doc::resolve::ResolvedPara, level: crate::kinsoku::Level) -> Vec<usize> {
    let mut found = Vec::new();
    for (i, o) in unicode_linebreak::linebreaks(text) {
        // Word keeps "and/or" and web addresses whole: no break right after a slash (a word
        // too long for the line still breaks anywhere).
        let after_slash =
            text.get(..i).is_some_and(|t| t.ends_with('/')) && text.get(i..).and_then(|t| t.chars().next()).is_some_and(char::is_alphanumeric);
        if (o == unicode_linebreak::BreakOpportunity::Allowed || i < text.len()) && !after_slash {
            found.push(i);
        }
    }
    // UAX #14 (LB13) also forbids a break between a space and a slash, so with the break after the
    // slash gone as well, "caption /word" had no break at all and the word split mid-letter. Word
    // wraps "/word" whole to the next line: break before a slash that starts a word.
    let before_slash: Vec<usize> = text
        .match_indices('/')
        .filter(|&(i, _)| {
            text.get(..i).and_then(|t| t.chars().next_back()).is_some_and(|c| c.is_whitespace() && !matches!(c, '\u{a0}' | '\u{2007}' | '\u{202f}'))
                && text.get(i + 1..).and_then(|t| t.chars().next()).is_some_and(char::is_alphanumeric)
        })
        .map(|(i, _)| i)
        .collect();
    if !before_slash.is_empty() {
        found.extend(before_slash);
    }

    // Word breaks after a hyphen even when a digit follows ("2016-" / "2022"), where UAX #14
    // keeps a hyphen and a number together (LB25): add those positions back. Only U+002D before
    // ASCII digits is added here; a non-breaking hyphen (U+2011) and a minus sign (U+2212) stay
    // unbreakable, and the other dashes keep their ordinary UAX #14 breaks.
    let hyphen_breaks: Vec<usize> = text
        .match_indices('-')
        .filter_map(|(pos, _)| {
            let i = pos + 1;
            text.get(i..).and_then(|t| t.chars().next()).is_some_and(|n| n.is_ascii_digit()).then_some(i)
        })
        .collect();
    if !hyphen_breaks.is_empty() {
        found.extend(hyphen_breaks);
        found.sort_unstable();
        found.dedup();
    }
    // Myanmar has no spaces between words: it wraps between syllables.
    if text.chars().any(|c| matches!(u32::from(c), 0x1000..=0x109F | 0xA9E0..=0xA9FF | 0xAA60..=0xAA7F)) {
        found.extend(wordcraft_doc::cluster::myanmar_breaks(text));
        found.sort_unstable();
        found.dedup();
    }
    // Asian typography: kinsoku and breaking Latin words anywhere.
    crate::kinsoku::apply(text, &mut found, rp.kinsoku, rp.word_wrap, level);
    found
}

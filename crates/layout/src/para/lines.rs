use super::bidi::{even_row, reorder_line};
use super::grid::{CellClass, EA_CLOSING, EMPHASIS_ROOM, Exclusion, Grid, justify_shares};
use super::justify::{HYPHENATION_ZONE, hyphen_glyph, shrink_line_spaces, space_room};
use super::model::{ClKind, Cluster, Line, LineEnd, PAGE_BREAK_MARK_INSET, ParaEnv};
use super::paragraph::{layout_para, word_widths};
use super::tabs::{next_tab, pl_text_at, ptab_stop, resolve_tab};
use super::{
    Align, Arc, Document, FieldCtx, Level, LevelSuffix, LineSpacing, MathJc, ParaLayout, Paragraph, TabAlign, TabLeader, TabStop, TextAlignment,
    VecDeque, WrapText,
};
use crate::NumeralMode;

/// Narrow cells and float spans must retain their usable width.
const MIN_LINE_WIDTH: f32 = 1.0;

/// What a paragraph's lines are broken with: the paragraph's measures and the state the lines
/// are broken in, from one to the next.
struct Breaker<'e> {
    level: Option<&'e Level>,
    rp: wordcraft_doc::resolve::ResolvedPara,
    mark_style: u32,
    exclusions: &'e [Exclusion],
    n: usize,
    width: f32,
    base_right: f32,
    first_left: f32,
    default_tab: f32,
    hanging_at: Option<f32>,
    /// Word 2013+ (compatibility mode 15) fits more text on a justified line by shrinking its
    /// spaces (`space_room`); never on lines with tabs.
    shrink_spaces: bool,
    /// Word stretches a justified line ending in a manual line break unless the document says not to.
    expand_break: bool,
    /// Hyphenation Options: the zone, and how many lines in a row may end with a hyphen.
    zone: f32,
    hyph_limit: usize,
    /// The width of every word, measured once: the word beside a float is measured again for
    /// every row it is tried on, and measuring it again costs a lookup rather than another pass
    /// over the word (#18).
    widths: Vec<f32>,
    /// A right-to-left paragraph is laid out from its start edge, as if mirrored: indents, tabs,
    /// the list label and alignment are measured from the right. Lines are mirrored back (and
    /// bidirectional text reordered) once broken; floating objects are mirrored in.
    rtl_para: bool,
    /// The document grid's line pitch, for paragraphs that snap to it.
    grid: Option<Grid>,
    /// The section's document grid, whether the paragraph snaps to it or not.
    doc_grid: Grid,
    /// Snapping to characters: the cell the lines are fitted to (`distribute_cells`).
    snap_cells: Option<f32>,
    /// Word's closing marks at the end of a line (`LINE_END_COMPRESS`): on justified and
    /// distributed lines — not on a character grid with kinsoku off, where the mark starts the
    /// next line (`hp5`), not in 縦書き, where Word for Mac moves the mark on with the character
    /// before it (`hp8`), nor in snapped cells or right-to-left paragraphs (not measured).
    tail_rule: bool,
    /// A top-to-bottom section's column (see `extents`).
    vertical: bool,
    bidi: bool,
    est_h: f32,
    table_h: f32,
    first_x0: Option<f32>,
    hcache: Vec<(u32, u32, f32)>,
    lines: Vec<Line>,
    /// The label is drawn on line 0 or, after an opening page break, line 1. Apply that
    /// line's alignment once both lines are known, before mirroring right-to-left labels.
    label_shifts: [f32; 2],
    /// The first cluster of the next line.
    i: usize,
    top: f32,
    first: bool,
    /// The rest of the current row: spans on the far side of floating objects, left to right.
    row_rest: VecDeque<(f32, f32)>,
    row_first: usize,
    /// Spans skipped in a row because the next word didn't fit (see `Scan::skip`).
    skipped: usize,
    /// A left tab past the right indent that wrapped to start the next line.
    wrapped_tab: Option<usize>,
}

/// One line being broken: where it is, and what has been put on it.
struct Scan {
    /// The span's left and right edges, and the row's left edge.
    left: f32,
    right_edge: f32,
    row_lo: f32,
    /// The span is narrowed by a floating object.
    narrowed: bool,
    /// A line already sits on this row (a span before this one may have been skipped).
    beside: bool,
    c0: usize,
    line_start_x: f32,
    x: f32,
    xs: Vec<f32>,
    leaders: Vec<(usize, TabLeader)>,
    /// Cluster index after which we may break.
    last_break: Option<usize>,
    /// The last non-hyphen break and the x after it.
    last_plain: Option<(usize, f32)>,
    /// Where the space before the current word starts.
    word_from: f32,
    /// After `hyph_limit` hyphenated lines in a row this one may not end with a hyphen.
    hyph_ok: bool,
    end: LineEnd,
    /// The cluster being looked at.
    j: usize,
    /// Tab cluster, stop, and x where the tab started.
    pending_tab: Option<(usize, TabStop, f32)>,
    /// The width of the spaces so far on this line.
    space_w: f32,
    has_tab: bool,
    /// The first word doesn't fit a span narrowed by a floating object: it moves on whole.
    skip: bool,
    /// A closing mark hung past the edge, and one kept at half its width (with its advance).
    hung: Option<usize>,
    half_at: Option<(usize, f32)>,
}

/// How a closing mark that doesn't fit stays at the end of a line (`LINE_END_COMPRESS`).
#[derive(Clone, Copy, PartialEq)]
enum Tail {
    No,
    /// At half its width, its glyph whole: the line's content ends half an em before its end.
    Half,
    /// Whole, past the line's end: it takes no room on the line and sits at the edge once the
    /// line is spread.
    Hang,
    /// At its full width, past the edge: the line is over-full and not spread.
    Squeeze,
    /// Drawn that far back into the line, its advance shorter by as much: the line is over-full
    /// by at most a quarter of the mark's em and not spread.
    Pull(f32),
}

/// Whether anything but markers (a bookmark, say) is among the clusters `c0..j`: an empty line
/// has no room to wrap from.
fn has_content(pl: &ParaLayout, c0: usize, j: usize) -> bool {
    (c0..j).any(|k| pl.clusters.get(k).is_some_and(|c| c.kind != ClKind::Marker))
}

/// A line's first item stays on it even when it doesn't fit; markers before it (a content
/// control's start, a bookmark) and floating objects' anchors take no room and don't count (Word:
/// a picture a little wider than the line, after an anchor, stays on the first line rather than
/// leaving the anchor a line of its own).
fn takes_room(c: &Cluster) -> bool {
    c.kind != ClKind::Marker && !(matches!(c.kind, ClKind::Object(_)) && c.adv == 0.0 && c.obj_h == 0.0)
}

/// The width up to the first '.' of the text after the tab at `tj`, up to cluster `j`.
fn width_to_decimal(pl: &ParaLayout, tj: usize, j: usize) -> Option<f32> {
    let mut w = 0.0;
    for k in tj + 1..=j {
        let cc = pl.clusters.get(k)?;
        if pl_text_at(cc) {
            return Some(w);
        }
        w += cc.adv;
    }
    None
}

/// The objects' exclusions mirrored across a line of `width`, for a right-to-left paragraph.
fn mirrored_exclusions(exclusions: &[Exclusion], width: f32) -> Vec<Exclusion> {
    exclusions
        .iter()
        .map(|e| Exclusion {
            left: width - e.right,
            right: width - e.left,
            contour: e.contour.as_ref().map(|c| Arc::new(c.mirrored(e.right - e.left))),
            // Mirrored, the object's left side is the line's right.
            side: match e.side {
                WrapText::Left => WrapText::Right,
                WrapText::Right => WrapText::Left,
                s => s,
            },
            ..e.clone()
        })
        .collect()
}

/// Break a paragraph into lines, measure each line, apply the paragraph's alignment and place
/// the list label. Fills `pl.lines`, `pl.height` and the label's position.
pub(super) fn break_lines(pl: &mut ParaLayout, env: &ParaEnv, mark_style: u32, level: Option<&Level>) {
    let width = env.width.max(MIN_LINE_WIDTH);
    let mirrored = pl.rp.bidi.then(|| mirrored_exclusions(env.exclusions, width));
    let exclusions = mirrored.as_deref().unwrap_or(env.exclusions);
    let mut bk = Breaker::new(pl, (env, exclusions), mark_style, level);
    loop {
        let mut sc = bk.begin_line(pl);
        sc.scan(&mut bk, pl);
        if sc.skip {
            bk.skip_row(&sc);
            continue;
        }
        let end = bk.close_line(pl, sc);
        if bk.next_line(pl, end) {
            break;
        }
    }
    bk.finish(pl);
}

/// Snap to characters (`w:docGrid snapToChars`): lays a line out again as Word for Mac does
/// (measured 2026-10-19 on 468 pt lines of 10.5 pt 游明朝 and Aptos with 11.7 and 18 pt cells,
/// `cg2`/`cg9`/`cg11`/`cg12`/`cg14`/`cg16`/`cg17` samples). The line is a row of units: an East
/// Asian character is its cells (`snap_chars`), a run of Latin text with its spaces a block of as
/// many whole cells as its width needs, the text centred in them (abc in two 11.7 pt cells sits
/// 3.2 pt in; a block the line breaks inside keeps its own width), two or more closing marks one
/// after another a compressed chain in whole cells (。、。 is two 11.7 pt cells: half an em, half
/// an em and an em, a share between each). The units are spread over the line — the last ending
/// at the right edge, trailing spaces hanging past it with the last block's right padding (quick
/// brown fox.␣ as a paragraph puts the full stop at the right edge, the q 3.0 pt in) — by sharing
/// the leftover equally: one share after every unit but the last (none between two Hangul
/// characters: 한글 stay 11.7 pt apart), one for every East Asian-width space, one plus a twelfth
/// of its size for every Latin space between two words (あ a あ puts the a twice as far from the
/// first あ as from the second; a paragraph of 20 あ and "end" has its characters 22.1 pt apart).
/// The automatic space between East Asian and Latin letters stays outside the cells. Lines with
/// tabs keep the usual layout. Returns whether the line was laid out again.
fn distribute_cells(pl: &ParaLayout, xs: &mut Vec<f32>, (c0, c1): (usize, usize), (x0, right): (f32, f32), cell: f32, wraps: bool) -> bool {
    if c1 <= c0 || c1 > pl.clusters.len() || !(cell.is_finite() && cell > 0.0) || xs.len() != c1 - c0 + 1 {
        return false;
    }
    let cl = CellLine { pl, c0, c1, last: c1, cell };
    // Trailing spaces and markers hang past the right edge: the line's content ends before them.
    let mut last = c1;
    while last > c0 && matches!(cl.kind(last - 1), Some(ClKind::Space) | Some(ClKind::Marker)) {
        last -= 1;
    }
    if last == c0 {
        return false;
    }
    let cl = CellLine { last, ..cl };
    let Some(units) = cl.units(wraps) else { return false };
    if units.is_empty() {
        return false;
    }
    let (shares, fixed, total) = cl.shares(&units);
    // The automatic spaces come out of what the cells leave; a line its cells fill exactly has
    // none (38 あ and "end" on a 40-cell line put the e 3.1 pt into its cells, as a lone block).
    let autos: f32 = (c0..last).map(|k| cl.auto(k)).sum();
    let room = right - x0 - (total - autos) - fixed;
    let kept = if autos > 0.0 && room.is_finite() && room < autos { (room / autos).clamp(0.0, 1.0) } else { 1.0 };
    let slack = room - autos * kept;
    let share = if shares > 0 && slack.is_finite() && slack > 0.0 { slack / shares as f32 } else { 0.0 };
    let (pos, x) = cl.place(&units, x0, share, kept);
    if !(x.is_finite() && pos.iter().all(|v| v.is_finite())) {
        return false;
    }
    *xs = pos;
    xs.push(x);
    true
}

/// A unit of a snap-to-characters line (see [`distribute_cells`]).
enum Unit {
    Cell(usize),
    /// Clusters `k0..k1`, the block's width on the line (its cells; its own width when the
    /// line breaks inside it, or with its right padding hanging after trailing spaces),
    /// the text's offset in it and the automatic space after it.
    Block {
        k0: usize,
        k1: usize,
        width: f32,
        pad: f32,
        auto: f32,
    },
    /// Closing marks `k0..k1` compressed into whole cells, a share between each.
    Chain {
        k0: usize,
        k1: usize,
        width: f32,
        pad: f32,
    },
}

/// The clusters `c0..c1` of a line fitted to cells of `cell`, the content ending at `last`.
struct CellLine<'p> {
    pl: &'p ParaLayout,
    c0: usize,
    c1: usize,
    last: usize,
    cell: f32,
}

impl CellLine<'_> {
    fn class(&self, k: usize) -> CellClass {
        self.pl.cell_classes.get(k).copied().unwrap_or(CellClass::Other)
    }

    fn kind(&self, k: usize) -> Option<ClKind> {
        self.pl.clusters.get(k).map(|c| c.kind)
    }

    fn adv(&self, k: usize) -> f32 {
        self.pl.clusters.get(k).map_or(0.0, |c| c.adv.max(0.0))
    }

    fn auto(&self, k: usize) -> f32 {
        self.pl.cell_autos.get(k).copied().unwrap_or(0.0).max(0.0)
    }

    fn size(&self, k: usize) -> f32 {
        self.pl.clusters.get(k).and_then(|c| self.pl.styles.get(c.style as usize)).map_or(0.0, |st| st.size).clamp(0.0, 1584.0)
    }

    fn in_cells(&self, natural: f32) -> f32 {
        (natural / self.cell - 0.001).ceil().clamp(1.0, 10_000.0) * self.cell
    }

    /// A Latin space between two words of its block (ending at `k1`) stretches, with a twelfth
    /// of its size more.
    fn internal(&self, k: usize, k1: usize) -> bool {
        self.class(k) == CellClass::LatinSpace
            && (k + 1..k1).find(|q| self.kind(*q) != Some(ClKind::Marker)).is_some_and(|q| self.class(q) == CellClass::Latin)
    }

    /// The line's units; `None` when a cluster can't be fitted to cells (the usual layout stays).
    fn units(&self, wraps: bool) -> Option<Vec<Unit>> {
        use CellClass::{Cell, EastAsianSpace, Hangul, Latin, LatinSpace, Mark, Other};
        let (c1, last) = (self.c1, self.last);
        let trailing: f32 = (last..c1).filter(|k| self.kind(*k) == Some(ClKind::Space)).map(|k| self.adv(k)).sum();
        let mut units: Vec<Unit> = Vec::new();
        let mut k = self.c0;
        while k < last {
            match self.class(k) {
                Other => {
                    if self.kind(k) != Some(ClKind::Marker) {
                        return None;
                    }
                    k += 1;
                }
                Cell | Hangul => {
                    units.push(Unit::Cell(k));
                    k += 1;
                }
                Mark => {
                    let k0 = k;
                    while k < last && self.class(k) == Mark {
                        k += 1;
                    }
                    let natural: f32 = (k0..k).map(|q| self.adv(q)).sum();
                    if !natural.is_finite() || natural <= 0.0 {
                        return None;
                    }
                    let width = self.in_cells(natural);
                    units.push(Unit::Chain { k0, k1: k, width, pad: ((width - natural) / 2.0).max(0.0) });
                }
                Latin | LatinSpace | EastAsianSpace => {
                    let k0 = k;
                    while k < last && (matches!(self.class(k), Latin | LatinSpace | EastAsianSpace) || self.kind(k) == Some(ClKind::Marker)) {
                        k += 1;
                    }
                    units.push(self.block((k0, k), trailing, wraps)?);
                }
            }
        }
        Some(units)
    }

    /// The Latin block `k0..k1`.
    fn block(&self, (k0, k1): (usize, usize), trailing: f32, wraps: bool) -> Option<Unit> {
        use CellClass::{EastAsianSpace, Latin, LatinSpace};
        let auto = self.auto(k1 - 1);
        let natural: f32 = (k0..k1).map(|k| self.adv(k)).sum::<f32>() - auto;
        if !natural.is_finite() {
            return None;
        }
        // The line breaks inside the block (its text goes on): no cells, no padding.
        let cut = wraps
            && k1 == self.last
            && (self.c1..self.pl.clusters.len())
                .find(|q| self.kind(*q) != Some(ClKind::Marker))
                .is_some_and(|q| matches!(self.class(q), Latin | LatinSpace | EastAsianSpace));
        let (width, pad) = if cut || natural <= 0.0 {
            (natural.max(0.0), 0.0)
        } else if k1 == self.last && trailing > 0.0 {
            // The trailing spaces count for the block's cells, then hang past the right
            // edge with the right padding: the text ends at the edge.
            let pad = ((self.in_cells(natural + trailing) - natural - trailing) / 2.0).max(0.0);
            (pad + natural, pad)
        } else {
            let w = self.in_cells(natural);
            (w, ((w - natural) / 2.0).max(0.0))
        };
        Some(Unit::Block { k0, k1, width, pad, auto })
    }

    /// Two Hangul characters stay together: no share between them.
    fn glued(&self, units: &[Unit], ui: usize) -> bool {
        matches!((units.get(ui), units.get(ui + 1)), (Some(Unit::Cell(a)), Some(Unit::Cell(b))) if self.class(*a) == CellClass::Hangul && self.class(*b) == CellClass::Hangul)
    }

    /// How many shares of the leftover the units take, the fixed extra of their Latin spaces and
    /// their total width.
    fn shares(&self, units: &[Unit]) -> (usize, f32, f32) {
        let n_units = units.len();
        let (mut shares, mut fixed, mut total) = (0usize, 0.0f32, 0.0f32);
        for (ui, u) in units.iter().enumerate() {
            if ui + 1 < n_units && !self.glued(units, ui) {
                shares += 1;
            }
            match u {
                Unit::Cell(k) => total += self.adv(*k),
                Unit::Chain { k0, k1, width, .. } => {
                    total += width;
                    shares += (k1 - k0).saturating_sub(1);
                }
                Unit::Block { k0, k1, width, auto, .. } => {
                    total += width + auto;
                    for k in *k0..*k1 {
                        match self.class(k) {
                            CellClass::EastAsianSpace => shares += 1,
                            CellClass::LatinSpace if self.internal(k, *k1) => {
                                shares += 1;
                                fixed += self.size(k) / 12.0;
                            }
                            _ => {}
                        }
                    }
                }
            }
        }
        (shares, fixed, total)
    }

    /// Each cluster's x from `x0`, a `share` between units, `kept` of the automatic spaces; and
    /// the line's end.
    fn place(&self, units: &[Unit], x0: f32, share: f32, kept: f32) -> (Vec<f32>, f32) {
        let c0 = self.c0;
        let extra = if share > 0.0 { 1.0 } else { 0.0 };
        let mut pos = vec![x0; self.c1 - c0];
        let mut x = x0;
        let n_units = units.len();
        for (ui, u) in units.iter().enumerate() {
            let after = if ui + 1 < n_units && !self.glued(units, ui) { share } else { 0.0 };
            match u {
                Unit::Cell(k) => {
                    if let Some(v) = pos.get_mut(*k - c0) {
                        *v = x;
                    }
                    x += self.adv(*k) - self.auto(*k) * (1.0 - kept) + after;
                }
                Unit::Chain { k0, k1, width, pad } => {
                    let mut cur = x + pad;
                    for k in *k0..*k1 {
                        if let Some(v) = pos.get_mut(k - c0) {
                            *v = cur;
                        }
                        cur += self.adv(k) + if k + 1 < *k1 { share } else { 0.0 };
                    }
                    x += width + share * (k1 - k0).saturating_sub(1) as f32 + after;
                }
                Unit::Block { k0, k1, width, pad, auto } => {
                    let bx = x;
                    let mut cur = x + pad;
                    let mut stretched = 0.0;
                    for k in *k0..*k1 {
                        if let Some(v) = pos.get_mut(k - c0) {
                            *v = cur;
                        }
                        cur += self.adv(k) - if k + 1 == *k1 { *auto } else { 0.0 };
                        let s = match self.class(k) {
                            CellClass::EastAsianSpace => share,
                            CellClass::LatinSpace if self.internal(k, *k1) => share + extra * self.size(k) / 12.0,
                            _ => 0.0,
                        };
                        cur += s;
                        stretched += s;
                    }
                    x = bx + width + stretched + auto * kept + after;
                }
            }
        }
        for k in self.last..self.c1 {
            if let Some(v) = pos.get_mut(k - c0) {
                *v = x;
            }
            x += self.adv(k);
        }
        (pos, x)
    }
}

impl<'e> Breaker<'e> {
    fn new(pl: &ParaLayout, (env, exclusions): (&'e ParaEnv<'e>, &'e [Exclusion]), mark_style: u32, level: Option<&'e Level>) -> Self {
        let rp = pl.rp.clone();
        let width = env.width.max(MIN_LINE_WIDTH);
        let settings = &env.doc.settings;
        let snap_cells = env.grid.cell.filter(|c| rp.snap_to_grid && c.is_finite() && *c > 0.0 && pl.cell_classes.len() == pl.clusters.len());
        let tail_rule = !rp.bidi
            && !pl.vertical
            && snap_cells.is_none()
            && matches!(rp.align, Align::Justify | Align::Distribute)
            && (rp.kinsoku || !(rp.snap_to_grid && env.grid.chars.is_some()));
        let est_h = pl
            .styles
            .get(mark_style as usize)
            .map(|st| (st.ascent + st.descent) * if let LineSpacing::Multiple(m) = rp.line_spacing { m } else { 1.0 })
            .unwrap_or(14.0)
            .max(1.0);
        Breaker {
            level,
            mark_style,
            exclusions,
            n: pl.clusters.len(),
            width,
            base_right: (width - rp.indent_right).max(1.0),
            first_left: rp.indent_left + rp.indent_first,
            default_tab: settings.default_tab,
            hanging_at: if rp.indent_first < 0.0 { Some(rp.indent_left) } else { None },
            shrink_spaces: rp.align == Align::Justify && settings.compat_mode >= wordcraft_doc::COMPAT_MODE_CURRENT,
            expand_break: !settings.do_not_expand_shift_return,
            zone: Some(settings.hyphenation_zone).filter(|z| z.is_finite() && *z >= 0.0).unwrap_or(HYPHENATION_ZONE).min(1584.0),
            hyph_limit: settings.consecutive_hyphen_limit as usize,
            widths: word_widths(&pl.clusters, &pl.hyph_after),
            rtl_para: rp.bidi,
            grid: Some(env.grid).filter(|g| g.line.is_some() && rp.snap_to_grid),
            doc_grid: env.grid,
            snap_cells,
            tail_rule,
            vertical: env.vertical,
            bidi: rp.bidi || !pl.bidi_levels.is_empty(),
            est_h,
            // Exact spacing fixes the table-contact boundary even when the mark has other metrics.
            // Keep the existing estimate for shapes and for non-exact spacing.
            table_h: if let LineSpacing::Exactly(h) = rp.line_spacing { h } else { est_h },
            rp,
            first_x0: None,
            hcache: Vec::new(),
            lines: Vec::new(),
            label_shifts: [0.0; 2],
            i: 0,
            top: 0.0,
            first: true,
            row_rest: VecDeque::new(),
            row_first: 0,
            skipped: 0,
            wrapped_tab: None,
        }
    }

    /// The left edge of the row's first span: the indent, behind a drop cap on its lines.
    fn row_left(&self, pl: &ParaLayout) -> f32 {
        let lo = if self.first { self.first_left } else { self.rp.indent_left };
        match pl.drop_cap {
            Some((_, dl, dw)) if dw > 0.0 && self.lines.len() < dl as usize => self.first_left.max(self.rp.indent_left) + dw,
            _ => lo,
        }
    }

    /// The next span of text: the rest of the row, else the first of the next row (flowing around
    /// floating objects: the spans beside them, or below them). Its left and right edges, and
    /// whether a floating object narrows it.
    fn next_span(&mut self, pl: &ParaLayout) -> (f32, f32, bool) {
        let narrowed = !self.row_rest.is_empty();
        if let Some((left, right)) = self.row_rest.pop_front() {
            return (left, right, narrowed);
        }
        let mut guard = 0;
        loop {
            let lo = self.row_left(pl);
            match row_spans(self.exclusions, self.top, self.est_h, self.table_h, lo, self.base_right).map(|mut spans| (spans.pop_front(), spans)) {
                Ok((Some((a, b)), rest)) => {
                    let narrowed = !rest.is_empty() || a > lo || b < self.base_right;
                    self.row_rest = rest;
                    self.row_first = self.lines.len();
                    return (a, b.max(a + MIN_LINE_WIDTH), narrowed);
                }
                Err(below) if below > self.top && guard < 50 => {
                    self.top = below;
                    guard += 1;
                }
                Ok((None, _)) | Err(_) => {
                    self.row_first = self.lines.len();
                    return (lo, self.base_right.max(lo + MIN_LINE_WIDTH), narrowed);
                }
            }
        }
    }

    /// The start of a line: its span, and the list label if it is the first.
    fn begin_line(&mut self, pl: &mut ParaLayout) -> Scan {
        let beside = !self.row_rest.is_empty() && self.lines.len() > self.row_first;
        let row_lo = if self.first { self.first_left } else { self.rp.indent_left };
        let (left, right_edge, narrowed) = self.next_span(pl);
        let x = if self.first { self.place_label(pl, left) } else { left };
        let x = x - self.opening_hang(pl);
        Scan {
            left,
            right_edge,
            row_lo,
            narrowed,
            beside,
            c0: self.i,
            line_start_x: x,
            x,
            xs: Vec::new(),
            leaders: Vec::new(),
            last_break: None,
            last_plain: None,
            word_from: left,
            hyph_ok: self.hyph_limit == 0 || self.lines.iter().rev().take_while(|l| l.hyphen.is_some()).count() < self.hyph_limit,
            end: LineEnd::Para,
            j: self.i,
            pending_tab: None,
            space_w: 0.0,
            has_tab: false,
            skip: false,
            hung: None,
            half_at: None,
        }
    }

    /// Compress punctuation at the start of a line (`w:topLinePunct`): an opening bracket that
    /// starts a line takes half its cell, its blank left half hung into the margin (or the
    /// indent). Word for Mac: 「 at a wrapped or first line starts 0.5 em before the left edge,
    /// the next character 0.5 em after it (half the pitch on a character grid, where the
    /// characters after it sit off the grid), so the line holds half an em more; only the first
    /// of 「「 moves; after a list number nothing moves. A bracket already shrunk by punctuation
    /// compression has no left half left to hang.
    fn opening_hang(&self, pl: &ParaLayout) -> f32 {
        let rp = &self.rp;
        if !rp.top_line_punct || self.rtl_para || (self.first && pl.label.is_some()) {
            return 0.0;
        }
        (self.i..self.n)
            .find(|k| pl.clusters.get(*k).is_none_or(|c| c.kind != ClKind::Marker))
            .filter(|k| pl.openings.binary_search(&(*k as u32)).is_ok())
            .and_then(|k| pl.clusters.get(k))
            .map(|c| {
                let size = pl.styles.get(c.style as usize).map_or(c.adv, |st| st.size);
                // Its cell: the character's advance on a grid, else an em; `adv` when wider.
                let cell = if rp.snap_to_grid && self.doc_grid.chars.is_some() { self.doc_grid.ea_advance(size) } else { size };
                (c.adv - cell / 2.0).clamp(0.0, cell / 2.0)
            })
            .filter(|h| h.is_finite())
            .unwrap_or(0.0)
    }

    /// Place the label on the first line, `left` being the first-line indent. Returns where the
    /// text starts.
    fn place_label(&self, pl: &mut ParaLayout, left: f32) -> f32 {
        let Some(lab) = pl.label.as_mut() else { return left };
        // The number is aligned at the first-line indent: its left edge, centre or right edge.
        lab.x = match self.level.map(|l| l.align) {
            Some(Align::Center) => left - lab.width / 2.0,
            Some(Align::Right) => left - lab.width,
            _ => left,
        };
        let end = lab.x + lab.width;
        match self.level.map(|l| l.suffix).unwrap_or(LevelSuffix::Tab) {
            LevelSuffix::Tab => {
                // A number ending exactly at the hanging indent (１． two em wide under a
                // two-character hanging indent) puts the text there, where a typed tab would
                // go on to the next stop (Word for Mac, `nl1`/`tb1` probes, 2026-10-19).
                let own = self.level.and_then(|l| l.tab).filter(|p| p.is_finite());
                if let Some(p) = self.hanging_at.into_iter().chain(own).find(|p| (p - end).abs() <= 0.05) {
                    p
                } else {
                    let t = next_tab(end, &self.rp.tabs, self.default_tab, self.hanging_at).pos;
                    // The level's own tab stop after the number, when it comes first.
                    match own.filter(|p| *p > end + 0.01) {
                        Some(p) if p < t => p,
                        _ => t,
                    }
                }
            }
            LevelSuffix::Space => end + pl.styles.get(lab.style as usize).map(|s| s.size * 0.25).unwrap_or(3.0),
            LevelSuffix::Nothing => end,
        }
    }

    /// The first word doesn't fit the span beside a floating object: try the row's next span,
    /// or the next row.
    fn skip_row(&mut self, sc: &Scan) {
        self.skipped += 1;
        if !self.row_rest.is_empty() {
            return;
        }
        self.top = if self.lines.len() > self.row_first {
            self.top + even_row(self.lines.get_mut(self.row_first..).unwrap_or_default())
        } else {
            // Nothing on this row: go below the first object that ends, as when no span is wide enough.
            self.exclusions
                .iter()
                .filter(|e| e.intersects_row(self.top, self.est_h, self.table_h, sc.row_lo, self.base_right))
                .map(|e| e.bottom)
                .reduce(f32::min)
                .unwrap_or(self.top + self.est_h)
        };
    }

    /// Finish the line scanned: lay out its clusters, measure and align it, and add it.
    /// Returns how it ends.
    fn close_line(&mut self, pl: &mut ParaLayout, mut sc: Scan) -> LineEnd {
        if sc.end == LineEnd::Para || sc.end == LineEnd::Wrap {
            let xc = sc.x;
            resolve_tab(pl, &mut sc.pending_tab, &mut sc.xs, sc.c0, xc, &mut sc.x);
        }
        if sc.j >= self.n && sc.end != LineEnd::Wrap && !matches!(sc.end, LineEnd::LineBreak | LineEnd::PageBreak | LineEnd::ColumnBreak) {
            sc.end = LineEnd::Para;
        }
        let (c1, j, hyphen) = self.settle_clusters(pl, &mut sc);
        // Snap to characters: the line's units are fitted to cells and spread over the line.
        let wraps = sc.end == LineEnd::Wrap;
        let distributed =
            self.snap_cells.is_some_and(|cell| distribute_cells(pl, &mut sc.xs, (sc.c0, c1), (sc.line_start_x, sc.right_edge), cell, wraps));
        self.text_alignment(pl, (sc.c0, c1));
        let v = self.measure(pl, (sc.c0, c1));
        self.align(pl, &mut sc, c1, distributed);
        let start = pl.clusters.get(sc.c0).map(|c| c.start).unwrap_or(pl.text_len);
        let stop = if c1 > sc.c0 { pl.clusters.get(c1 - 1).map(|c| c.end).unwrap_or(pl.text_len) } else { start };
        if self.first_x0.is_none() {
            self.first_x0 = sc.xs.first().copied();
        }
        let end = sc.end;
        let mut line = Line {
            top: self.top,
            height: v.height,
            baseline: v.baseline,
            text_asc: v.text_asc,
            text_desc: v.text_desc,
            c0: sc.c0,
            c1,
            xs: sc.xs,
            leaders: sc.leaders,
            end,
            start,
            stop,
            left: sc.left,
            right: sc.right_edge,
            hyphen,
            beside: sc.beside,
            vis: Vec::new(),
            rtl: self.rtl_para,
        };
        if self.bidi {
            reorder_line(pl, &mut line, self.width);
        }
        self.lines.push(line);
        self.first = false;
        self.skipped = 0;
        self.i = j;
        end
    }

    /// The line's clusters up to `c1` (at least one, so the paragraph progresses) with an x each
    /// and the line's end, a hyphen after the last if it wrapped at a hyphenation point. Returns
    /// the end of the clusters, where the next line starts and the hyphen.
    fn settle_clusters(&mut self, pl: &ParaLayout, sc: &mut Scan) -> (usize, usize, Option<(u32, u32, f32)>) {
        let (c0, n) = (sc.c0, self.n);
        let c1 = sc.j.max(c0);
        let (c1, j) = if c1 == c0 && c0 < n { (c0 + 1, c0 + 1) } else { (c1, sc.j) };
        while sc.xs.len() < c1 - c0 {
            let last = sc.xs.last().copied().unwrap_or(sc.line_start_x);
            let adv = pl.clusters.get(c0 + sc.xs.len().saturating_sub(1)).map(|c| c.adv).unwrap_or(0.0);
            sc.xs.push(if sc.xs.is_empty() { sc.line_start_x } else { last + adv });
        }
        sc.xs.truncate(c1 - c0);
        let hyphen = (sc.end == LineEnd::Wrap && c1 > c0 && pl.hyph_after.binary_search(&((c1 - 1) as u32)).is_ok())
            .then(|| pl.clusters.get(c1 - 1).map(|c| hyphen_glyph(pl, c.style, &mut self.hcache)))
            .flatten();
        let end_x = match (sc.xs.last(), c1.checked_sub(1).and_then(|k| pl.clusters.get(k))) {
            (Some(lx), Some(c)) => lx + c.adv + hyphen.map_or(0.0, |h| h.2),
            _ => sc.line_start_x,
        };
        sc.xs.push(end_x);
        (c1, j, hyphen)
    }

    /// Text alignment (`w:textAlignment`): runs of different sizes line up on their ascent lines
    /// (top), the centres of their ascent–descent boxes (center) or their descents (bottom)
    /// instead of their baselines, as measured in Word for Mac with 游明朝 and Aptos at 10.5, 24
    /// and 48 pt (top: the small run rises by the ascenders' difference, 0.88 em a point of size
    /// for 游明朝; center: by half the difference of ascender minus descender).
    fn text_alignment(&self, pl: &mut ParaLayout, (c0, c1): (usize, usize)) {
        let rp = &self.rp;
        if matches!(rp.text_alignment, TextAlignment::Auto | TextAlignment::Baseline) {
            return;
        }
        let text_cl = |c: &Cluster| !matches!(c.kind, ClKind::Marker | ClKind::Space | ClKind::Tab);
        let (mut ga, mut gd) = (0.0f32, 0.0f32);
        for c in pl.clusters.get(c0..c1).into_iter().flatten().filter(|c| text_cl(c)) {
            if let Some(st) = pl.styles.get(c.style as usize) {
                ga = ga.max(st.glyph_ascent + st.shift);
                gd = gd.max(st.descent - st.shift);
            }
        }
        if pl.align_dy.len() < pl.clusters.len() {
            pl.align_dy.resize(pl.clusters.len(), 0.0);
        }
        for k in c0..c1 {
            let Some(c) = pl.clusters.get(k) else { continue };
            let Some(st) = pl.styles.get(c.style as usize) else { continue };
            let up = ga - (st.glyph_ascent + st.shift);
            let down = gd - (st.descent - st.shift);
            let dy = match rp.text_alignment {
                TextAlignment::Top => up,
                TextAlignment::Center => (up - down) / 2.0,
                TextAlignment::Bottom => -down,
                TextAlignment::Auto | TextAlignment::Baseline => 0.0,
            };
            let dy = if text_cl(c) { dy } else { 0.0 };
            if let Some(d) = pl.align_dy.get_mut(k) {
                *d = dy;
            }
        }
    }

    /// The line's ascent and descent over its baseline from the clusters `c0..c1`, and the
    /// tallest picture on it (which sits on the baseline).
    fn extents(&self, pl: &ParaLayout, (c0, c1): (usize, usize)) -> Extents {
        let rp = &self.rp;
        let mut ex = Extents { asc: 0.0, desc: 0.0, obj_asc: 0.0, em_above: 0.0, em_below: 0.0 };
        let mut any = false;
        let dropped = pl.drop_cap.map_or(0, |d| d.0);
        for (k, c) in pl.clusters.get(c0..c1).into_iter().flatten().enumerate() {
            // Spaces and tabs never make a line taller, whatever their size: Word sizes a line by
            // its other characters (a line of only them takes the paragraph mark's), measured in
            // Word for Windows with 9 pt text and 11–20 pt tabs, spaces and ideographic spaces.
            // Floating and hidden drawings have no inline extent; their anchor font takes no height.
            let anchor = matches!(c.kind, ClKind::Object(_)) && c.adv == 0.0 && c.obj_h == 0.0;
            if matches!(c.kind, ClKind::Marker | ClKind::Space | ClKind::Tab) || anchor || c0 + k < dropped {
                continue;
            }
            let Some(st) = pl.styles.get(c.style as usize) else { continue };
            // The run's baseline shift plus its text alignment offset.
            let shift = st.shift + pl.align_dy.get(c0 + k).copied().unwrap_or(0.0);
            // A picture sits on the baseline of its run: the line keeps that font's ascent
            // and descent, and grows above the baseline to fit a taller picture.
            let (a, d) = if matches!(c.kind, ClKind::Object(_)) && c.obj_h > 0.0 {
                if pl.maths.iter().any(|(k, _)| c.kind == ClKind::Object(*k)) {
                    // Exact spacing keeps the run's baseline; tall math may overflow the
                    // fixed line box without moving adjacent text up by its denominator.
                    if matches!(rp.line_spacing, LineSpacing::Exactly(_)) { (st.ascent, st.descent) } else { (c.obj_h, c.obj_d) }
                } else {
                    ex.obj_asc = ex.obj_asc.max(c.obj_h);
                    (st.ascent, st.descent)
                }
            } else if self.vertical {
                // A vertical column: the em square of each character is centred on the
                // column's centre line and the run's line (ascent + descent) spreads equally
                // either side of it — 0.715 em each way for Word's 游明朝 — and a ruby's own
                // line runs beside the base's, its centre `raise` towards the right (the
                // ascent side): the column reaches raise + ascent − 0.38 em of the ruby's size
                // that way. Measured in Word for Mac: 10.5-pt columns 15.0 pt apart (16.04
                // with a 5-pt ruby raised 5, 19.2 with a 7-pt ruby raised 7), each centred
                // in whole 18-pt grid lines (the 7-pt ruby's column takes two), a 20-pt
                // character sharing the 10.5-pt text's centre line.
                let half = (st.ascent + st.descent) / 2.0;
                let ruby_reach = pl
                    .rubies
                    .iter()
                    .find(|r| r.cluster == c0 + k)
                    .and_then(|r| r.glyphs.first().and_then(|(s, _)| pl.styles.get(*s as usize)).map(|rs| r.raise + rs.ascent - rs.size * 0.38))
                    .unwrap_or(0.0);
                (half.max(ruby_reach), half)
            } else {
                // `obj_h` is set on text clusters under ruby text and emphasis marks, which the
                // line grows to fit; `obj_d` under emphasis marks below the text. A superscript
                // or subscript's line is as tall as full-size text (`line_extent`); the text
                // alignment offset moves it.
                let (a, d) = st.line_extent();
                let dy = shift - st.shift;
                ((a + dy.max(0.0)).max(c.obj_h + shift), (d + (-dy).max(0.0)).max(c.obj_d - shift))
            };
            ex.asc = ex.asc.max(a);
            ex.desc = ex.desc.max(d);
            any = true;
            // Emphasis marks: room above and below the line, added after the grid sizes it.
            if c.kind == ClKind::Text && st.rc.em != wordcraft_doc::props::EmphasisMark::None {
                if st.rc.em.below() {
                    ex.em_below = ex.em_below.max(st.size * EMPHASIS_ROOM);
                } else {
                    ex.em_above = ex.em_above.max(st.size * EMPHASIS_ROOM);
                }
            }
        }
        if (!any || (ex.asc == 0.0 && ex.desc == 0.0))
            && let Some(st) = pl.styles.get(self.mark_style as usize)
        {
            ex.asc = st.ascent;
            ex.desc = st.descent;
        }
        ex
    }

    /// The line's height, baseline and text extent.
    fn measure(&self, pl: &ParaLayout, range: (usize, usize)) -> Measure {
        let rp = &self.rp;
        let Extents { mut asc, mut desc, obj_asc, em_above, em_below } = self.extents(pl, range);
        let text_natural = asc + desc;
        let (text_asc, text_desc) = (asc.max(obj_asc), desc);
        let text_pictures = if obj_asc > asc { obj_asc + desc } else { 0.0 };
        let mut label_extra = 0.0;
        if self.first
            && let Some(lab) = &pl.label
        {
            if !matches!(rp.line_spacing, LineSpacing::Exactly(_)) {
                // Automatic/minimum spacing keeps text descent. Only the label's ascent
                // overflow adds room, unscaled by the automatic spacing multiplier.
                label_extra = (lab.ascent - asc).max(0.0);
                asc = asc.max(lab.ascent);
            } else {
                for st in lab.runs.iter().filter_map(|r| pl.styles.get(r.style as usize)) {
                    asc = asc.max(st.ascent);
                    desc = desc.max(st.descent);
                }
            }
        }
        // Keep the picture floor independent of list-label metrics and line-spacing multipliers.
        // shortcut: Word leaves less than a full descent below a line of only pictures (0.5pt to
        // 1.9pt in docxide-pdf's header fixtures case121/case123, where the descent is 3.3pt), and
        // no rule tried so far (none, only the text's descent) matched both; it needs Word probes.
        let natural = asc + desc;
        let pictures = if obj_asc > asc { obj_asc + desc } else { 0.0 };
        // Document grid: the line takes whole line pitches, its text centred in them.
        let grid = self.grid.as_ref();
        let (height, centred) = match (rp.line_spacing, grid) {
            (LineSpacing::Multiple(m), Some(g)) => ((g.snap_height(natural) * m).max(g.snap_height(pictures)), true),
            (LineSpacing::AtLeast(v), Some(g)) => (g.snap_height(natural.max(pictures)).max(v), true),
            (LineSpacing::Multiple(m), None) => ((text_natural * m + label_extra).max(text_pictures), false),
            (LineSpacing::AtLeast(v), None) => (natural.max(text_pictures).max(v), false),
            (LineSpacing::Exactly(v), _) => (v, false),
        };
        let baseline = if centred {
            let up = asc.max(obj_asc);
            self.top + ((height - up - desc) / 2.0).max(0.0) + up
        } else {
            self.top + baseline_offset(rp.line_spacing, asc, height, desc).max(obj_asc.min(height - desc))
        };
        // Emphasis marks (傍点) ask for `EMPHASIS_ROOM` on their side of the text. Measured in
        // Word for Mac: without a grid the line grows by the room on that side; in a grid a
        // line whose slack above the text (half of what the whole pitches leave over) holds the
        // room keeps its height and baseline for marks above the text, and for marks below it
        // moves the text up by the room instead (16-pt text in two 18-pt pitches: dots leave
        // the baseline 23.3 pt below the line's top, under dots lift it to 17.5); a line
        // without that slack grows by the whole room on the mark's side, its text staying put
        // (10.5-pt text in one pitch: 21.84 pt tall, the baseline 3.84 lower under dots above
        // it). Exact spacing is exact: the marks get no room.
        let exact = matches!(rp.line_spacing, LineSpacing::Exactly(_));
        let pad = if centred { ((height - asc.max(obj_asc) - desc) / 2.0).max(0.0) } else { 0.0 };
        let (mut baseline, mut height) = (baseline, height);
        if !exact {
            if em_above > pad {
                baseline += em_above;
                height += em_above;
            }
            if em_below > pad {
                height += em_below;
            } else {
                baseline -= em_below;
            }
        }
        Measure { height, baseline, text_asc, text_desc }
    }

    /// Alignment / justification of the line (trailing spaces hang).
    /// `distributed`: the line was fitted to the character grid's cells (`distribute_cells`).
    fn align(&mut self, pl: &mut ParaLayout, sc: &mut Scan, c1: usize, distributed: bool) {
        let rp = &self.rp;
        let c0 = sc.c0;
        let content_end = content_end(pl, sc, c1);
        let slack = sc.right_edge - content_end;
        let last_tab = (c0..c1).rev().find(|k| pl.clusters.get(*k).is_some_and(|c| c.kind == ClKind::Tab));
        let align_from = last_tab.map(|k| k + 1).unwrap_or(c0);
        let display = pl.displays.iter().find(|(ci, _)| *ci >= c0 && *ci < c1).map(|d| d.1);
        let shift = match (display, rp.align) {
            (Some(MathJc::Left), _) => 0.0,
            (Some(MathJc::Right), _) => slack,
            (Some(_), _) => slack / 2.0,
            (None, Align::Center) => slack / 2.0,
            (None, Align::Right) => slack,
            _ => 0.0,
        };
        if shift > 0.0 && slack > 0.0 && !distributed {
            for v in sc.xs.iter_mut().skip(align_from - c0) {
                *v += shift;
            }
            // After a tab the label stays, as the text before the tab does.
            if last_tab.is_none()
                && let Some(label_shift) = self.label_shifts.get_mut(self.lines.len())
            {
                *label_shift = shift;
            }
        }
        // A justified line ending with a manual line break is spread like a wrapped one unless
        // the document says not to (`w:doNotExpandShiftReturn`, set by Word's Japanese
        // template); a line ending with a page or column break, and the paragraph's last line,
        // keep their spacing. Distributed text spreads every line.
        // A line ending with a compressed or hung closing mark is spread even as the paragraph's
        // last line (Word for Mac: 44 あ and 。 alone in a paragraph, the 。 half an em into the
        // margin and the あ spread by the leftover).
        let tail_end = sc.hung.is_some_and(|k| k < c1) || sc.half_at.is_some_and(|(k, _)| k < c1);
        let stretch_end = sc.end == LineEnd::Wrap || tail_end || (sc.end == LineEnd::LineBreak && self.expand_break);
        let justify = display.is_none() && !distributed && ((rp.align == Align::Justify && stretch_end) || rp.align == Align::Distribute);
        // A line that only fits with shrunk spaces is shrunk, the last line of the paragraph too.
        let shrink = display.is_none() && self.shrink_spaces && slack < 0.0 && last_tab.is_none();
        if shrink {
            shrink_line_spaces(pl, &mut sc.xs, c0, c1, content_end, slack);
        } else if justify && slack > 0.0 {
            let half_at = sc.half_at.map(|(k, _)| k);
            justify_line(pl, &mut sc.xs, (c0, c1, align_from), (content_end, slack, sc.right_edge), (rp.align, half_at));
        }
    }

    /// After a line: whether the paragraph is done. Else the next line starts, below the row
    /// when the line ends it.
    fn next_line(&mut self, pl: &ParaLayout, end: LineEnd) -> bool {
        // Text that wrapped continues on the row's far side; anything else ends the row.
        if end != LineEnd::Wrap || self.i >= self.n {
            self.row_rest.clear();
        }
        if self.row_rest.is_empty() {
            self.top += even_row(self.lines.get_mut(self.row_first..).unwrap_or_default());
        }
        if self.i >= self.n {
            // A paragraph ending with a page break keeps its mark on the break's row, at the
            // right margin (Word: the ↵ follows the dotted "Page Break" rule on the same line);
            // one ending with a line or column break gets an empty last line (after a column
            // break at the top of the next column).
            if end == LineEnd::PageBreak && self.page_break_mark_line(pl) {
                return true;
            }
            if matches!(end, LineEnd::LineBreak | LineEnd::PageBreak | LineEnd::ColumnBreak) {
                self.empty_last_line(pl);
            }
            return true;
        }
        self.lines.len() > 200_000
    }

    /// The paragraph mark's line after a closing page break: on the break's row, at the right
    /// margin. False when there is no line to share.
    fn page_break_mark_line(&mut self, pl: &ParaLayout) -> bool {
        let (width, base_right, n) = (self.width, self.base_right, self.n);
        let Some(prev) = self.lines.last() else { return false };
        let (x0, left, right) = if self.rtl_para {
            let x0 = (width - base_right + PAGE_BREAK_MARK_INSET).min(prev.visual_end_x() - 2.0);
            (x0, width - base_right, x0)
        } else {
            let x0 = (base_right - PAGE_BREAK_MARK_INSET).max(prev.visual_end_x() + 2.0);
            (x0, x0, base_right)
        };
        let line = Line {
            top: prev.top,
            height: prev.height,
            baseline: prev.baseline,
            text_asc: prev.text_asc,
            text_desc: prev.text_desc,
            c0: n,
            c1: n,
            xs: vec![x0],
            leaders: Vec::new(),
            end: LineEnd::Para,
            start: pl.text_len,
            stop: pl.text_len,
            left,
            right,
            hyphen: None,
            beside: true,
            vis: Vec::new(),
            rtl: self.rtl_para,
        };
        self.lines.push(line);
        true
    }

    fn empty_last_line(&mut self, pl: &ParaLayout) {
        let rp = &self.rp;
        let grid = self.grid.as_ref();
        let (asc, desc) = pl.styles.get(self.mark_style as usize).map(|s| (s.ascent, s.descent)).unwrap_or((10.0, 3.0));
        let natural = asc + desc;
        let h = match (rp.line_spacing, grid) {
            (LineSpacing::Multiple(m), Some(g)) => g.snap_height(natural) * m,
            (LineSpacing::AtLeast(v), Some(g)) => g.snap_height(natural).max(v),
            (LineSpacing::Multiple(m), None) => natural * m,
            (LineSpacing::AtLeast(v), None) => natural.max(v),
            (LineSpacing::Exactly(v), _) => v,
        };
        let (width, x0) = (self.width, rp.indent_left);
        let (x0, left, right) = if self.rtl_para { (width - x0, width - self.base_right, width - x0) } else { (x0, x0, self.base_right) };
        self.lines.push(Line {
            top: self.top,
            height: h,
            baseline: self.top + if grid.is_some() { h - desc } else { baseline_offset(rp.line_spacing, asc, h, desc) },
            text_asc: asc,
            text_desc: desc,
            c0: self.n,
            c1: self.n,
            xs: vec![x0],
            leaders: Vec::new(),
            end: LineEnd::Para,
            start: pl.text_len,
            stop: pl.text_len,
            left,
            right,
            hyphen: None,
            beside: false,
            vis: Vec::new(),
            rtl: self.rtl_para,
        });
        self.top += h;
    }

    /// Hang the drop cap left of the text with its ink bottom on line N's baseline (Word for Mac:
    /// 漢's baseline 2.6 pt above the third line's at 38.5 pt, 二's 6.4 pt below the second line's
    /// at 90.5, T's on it; `dc1`–`dc3` probes, 2026-10-19).
    fn hang_drop_cap(&mut self, pl: &mut ParaLayout) {
        let rp = &self.rp;
        let lines = &self.lines;
        let Some((nc, dl, dw)) = pl.drop_cap else { return };
        let (Some(l0), Some(ln)) = (lines.first(), lines.get((dl as usize).min(lines.len()).saturating_sub(1))) else { return };
        let short = lines.len() < dl as usize;
        let dy = if short { (dl as f32 - 1.0) * l0.height } else { ln.baseline - l0.baseline } + pl.drop_cap_ink;
        let lead = self.first_x0.unwrap_or(0.0) - self.first_left.max(rp.indent_left);
        // In margin, the letter's right edge stays the distance away from the text's edge.
        let dx = if rp.drop_cap_margin { lead.max(0.0) + pl.drop_cap_w + rp.drop_cap_distance } else { dw.min(lead) };
        // Toward the start edge: left, or right (and from the letter's right edge) when mirrored.
        let dx = if self.rtl_para { pl.drop_cap_w - dx } else { dx };
        if short {
            self.top = self.top.max(l0.top + l0.height * dl as f32);
        }
        for c in pl.clusters.get(..nc).unwrap_or(&[]).to_vec() {
            for g in pl.glyphs.get_mut(c.g0 as usize..c.g1 as usize).into_iter().flatten() {
                g.dy -= dy;
                g.dx -= dx;
            }
        }
    }

    /// Store the lines and the paragraph's height, and move the label into place.
    fn finish(mut self, pl: &mut ParaLayout) {
        self.hang_drop_cap(pl);
        pl.height = self.top;
        pl.lines = std::mem::take(&mut self.lines);
        let label_shift = self.label_shifts.get(pl.label_line()).copied().unwrap_or(0.0);
        if let Some(lab) = pl.label.as_mut() {
            lab.x += label_shift;
            if self.rtl_para {
                lab.x = self.width - lab.x - lab.width;
            }
        }
    }
}

/// A line's ascent and descent over the baseline, and the tallest picture on it.
struct Extents {
    asc: f32,
    desc: f32,
    obj_asc: f32,
    /// Emphasis marks' room above and below the text.
    em_above: f32,
    em_below: f32,
}

/// A line's height and baseline, and the extent of its text.
struct Measure {
    height: f32,
    baseline: f32,
    text_asc: f32,
    text_desc: f32,
}

/// Where the line's content ends: trailing spaces and markers hang.
fn content_end(pl: &ParaLayout, sc: &Scan, c1: usize) -> f32 {
    let mut content_end = sc.xs.last().copied().unwrap_or(sc.line_start_x);
    for k in (sc.c0..c1).rev() {
        if sc.hung == Some(k) {
            // The hung mark takes no room: the content ends where it starts.
            content_end = sc.xs.get(k - sc.c0).copied().unwrap_or(content_end);
            continue;
        }
        match pl.clusters.get(k).map(|c| c.kind) {
            Some(ClKind::Space) | Some(ClKind::Marker) | Some(ClKind::LineBreak) | Some(ClKind::PageBreak) | Some(ClKind::ColumnBreak) => {
                content_end = sc.xs.get(k - sc.c0).copied().unwrap_or(content_end);
            }
            _ => {
                // The automatic space after the line's last character goes with the wrap.
                content_end -= pl.auto_space_after(k);
                break;
            }
        }
    }
    content_end
}

/// Stretch the line to its right edge (`right_edge`, `slack` past its content): Word's shares
/// of the leftover between its content clusters (`justify_shares`), or with `align`
/// Distribute (均等割り付け) equal shares between its characters. `half_at`: a closing mark kept
/// at half its width at the line's end.
fn justify_line(
    pl: &ParaLayout,
    xs: &mut [f32],
    (c0, c1, align_from): (usize, usize, usize),
    (content_end, slack, right_edge): (f32, f32, f32),
    (align, half_at): (Align, Option<usize>),
) {
    // The line's content after the last tab: the clusters before the trailing spaces and
    // breaks (markers take no room and no share).
    let content: Vec<usize> = (align_from..c1)
        .filter(|k| pl.clusters.get(*k).is_some_and(|c| c.kind != ClKind::Marker) && xs.get(k - c0).copied().unwrap_or(f32::MAX) < content_end - 0.01)
        .collect();
    // What each content cluster's right side gains.
    let extras: Vec<f32> = if align == Align::Distribute {
        // 均等割り付け: the leftover shared equally between the characters.
        let cnt = content.len().saturating_sub(1).max(1) as f32;
        content.iter().enumerate().map(|(i, _)| if i + 1 < content.len() { slack / cnt } else { 0.0 }).collect()
    } else {
        justify_shares(pl, &content, slack)
    };
    // The last content cluster's width on the line, before the spread.
    let last_width = content.last().and_then(|&l| Some(content_end - xs.get(l - c0)?));
    // A distributed line holding a single character has nothing to spread it between:
    // Word for Mac centres it in the line (a, 一, １, 。, W alone, after an indent, and a
    // paragraph's last line of one character, its PDFs read 2026-10-20).
    let centred = if align == Align::Distribute && content.len() == 1 { content.first().copied() } else { None };
    let mut add = 0.0;
    let mut ci = 0;
    for k in align_from..c1 {
        if centred == Some(k) {
            add += slack / 2.0;
        }
        if let Some(v) = xs.get_mut(k - c0) {
            *v += add;
        }
        if content.get(ci) == Some(&k) {
            add += extras.get(ci).copied().unwrap_or(0.0);
            ci += 1;
        }
    }
    if let Some(v) = xs.last_mut() {
        *v += add;
    }
    // A closing mark narrowed at the line's end (。 before the break, 10.2 pt of its 12 in
    // Word) shares the leftover at its narrowed width but ends the line with its whole
    // em box at the edge: Word puts it 1.8 pt back from its share.
    if align == Align::Justify
        && let Some(&last) = content.last()
        && half_at.is_none_or(|k| k != last)
        && let Some(c) = pl.clusters.get(last)
        && pl.ea_flags.get(last).is_some_and(|f| f & EA_CLOSING != 0)
        && let Some(width) = last_width
    {
        let size = pl.styles.get(c.style as usize).map_or(0.0, |st| st.size).clamp(0.0, 1584.0);
        let prev_end =
            content.len().checked_sub(2).and_then(|i| content.get(i)).and_then(|&p| Some(xs.get(p - c0)? + pl.clusters.get(p)?.adv.max(0.0)));
        if width < size - 0.01
            && right_edge - size >= prev_end.unwrap_or(f32::MIN)
            && let Some(v) = xs.get_mut(last - c0)
        {
            *v = right_edge - size;
        }
    }
}

impl Scan {
    /// Put clusters on the line until it is full or ends.
    fn scan(&mut self, bk: &mut Breaker, pl: &mut ParaLayout) {
        while self.j < bk.n && !self.step(bk, pl) {}
    }

    /// Put the cluster at `j` on the line. True when the line ends here.
    fn step(&mut self, bk: &mut Breaker, pl: &mut ParaLayout) -> bool {
        let Some(c) = pl.clusters.get(self.j).cloned() else { return true };
        match c.kind {
            ClKind::LineBreak | ClKind::PageBreak | ClKind::ColumnBreak => {
                resolve_tab(pl, &mut self.pending_tab, &mut self.xs, self.c0, self.x, &mut self.x);
                self.xs.push(self.x);
                self.end = match c.kind {
                    ClKind::LineBreak => LineEnd::LineBreak,
                    ClKind::PageBreak => LineEnd::PageBreak,
                    _ => LineEnd::ColumnBreak,
                };
                self.j += 1;
                true
            }
            ClKind::Tab => self.tab(bk, pl),
            _ => self.cluster(bk, pl, &c),
        }
    }

    /// A tab: its stop, and the room to it. True when the line ends before it.
    fn tab(&mut self, bk: &mut Breaker, pl: &mut ParaLayout) -> bool {
        let j = self.j;
        self.has_tab = true;
        resolve_tab(pl, &mut self.pending_tab, &mut self.xs, self.c0, self.x, &mut self.x);
        let rp = &bk.rp;
        let ptab = pl.ptabs.iter().find(|t| t.0 == j).map(|&(_, a, b, l)| ptab_stop(a, b, l, bk.width, (rp.indent_left, rp.indent_right)));
        let stop = ptab.unwrap_or_else(|| next_tab(self.x, &rp.tabs, bk.default_tab, bk.hanging_at));
        // A positional tab whose place the text has passed goes there on the next line
        // (ECMA-376 §17.3.3.23); a line with nothing but markers on it is empty.
        if ptab.is_some() && stop.pos < self.x - 0.01 && has_content(pl, self.c0, j) {
            self.end = LineEnd::Wrap;
            return true;
        }
        self.xs.push(self.x);
        if ptab.is_none() && stop.pos > self.right_edge + 0.01 && stop.align == TabAlign::Left {
            // Not when only markers (a bookmark, say) are before it: the line is empty.
            if has_content(pl, self.c0, j) {
                // Tab past the right indent wraps: like Word, it starts the next line
                // and goes to the first stop there (a TOC entry's leader and page number).
                self.xs.pop();
                self.end = LineEnd::Wrap;
                bk.wrapped_tab = Some(j);
                return true;
            }
            if bk.wrapped_tab == Some(j) {
                // Still past the right indent on the line it wrapped to: it takes no
                // room, and its text starts at the indent rather than on a line of its own.
                if let Some(c) = pl.clusters.get_mut(j) {
                    c.adv = 0.0;
                }
                self.last_break = Some(j);
                self.j += 1;
                return false;
            }
        }
        self.place_tab(pl, stop);
        self.last_break = Some(j);
        self.j += 1;
        false
    }

    /// Give the tab at `j` its room to `stop`, or wait for the text after it (a right, centre or
    /// decimal tab).
    fn place_tab(&mut self, pl: &mut ParaLayout, stop: TabStop) {
        let j = self.j;
        match stop.align {
            TabAlign::Left | TabAlign::Bar | TabAlign::Clear => {
                let w = (stop.pos - self.x).max(0.0);
                if let Some(c) = pl.clusters.get_mut(j) {
                    c.adv = w;
                }
                self.x += w;
            }
            _ => self.pending_tab = Some((j, stop, self.x)),
        }
        if stop.leader != TabLeader::None {
            self.leaders.push((j, stop.leader));
        }
    }

    /// Whether text after a right/centre tab grows leftwards into the tab's space first.
    fn absorbs(&self, pl: &ParaLayout, advance: f32) -> bool {
        self.pending_tab.is_some_and(|(tj, stop, _)| {
            let room = pl.clusters.get(tj).map(|t| t.adv).unwrap_or(0.0);
            match stop.align {
                TabAlign::Right | TabAlign::Decimal => room >= advance,
                TabAlign::Center => room >= advance / 2.0 && self.x + advance / 2.0 <= self.right_edge + 0.01,
                _ => false,
            }
        })
    }

    /// A text or object cluster. True when the line ends before or at it.
    fn cluster(&mut self, bk: &mut Breaker, pl: &mut ParaLayout, c: &Cluster) -> bool {
        let (j, c0) = (self.j, self.c0);
        // A display equation sits on a line of its own.
        let is_display = pl.displays.iter().any(|(ci, _)| *ci == j);
        if is_display && has_content(pl, c0, j) {
            self.end = LineEnd::Wrap;
            return true;
        }
        // A grapheme can have several drawing segments; fit it as one unit while keeping
        // each segment's font, glyphs and position for painting.
        let advance = pl.atomic_advance(j);
        let absorbs = self.absorbs(pl, advance);
        let shrink = if bk.shrink_spaces && !self.has_tab {
            space_room(self.space_w, self.x + advance - pl.auto_space_after(j) - self.word_from)
        } else {
            0.0
        };
        let fits = absorbs
            || self.x + advance - pl.auto_space_after(j) <= self.right_edge + shrink + 0.01
            || c.kind == ClKind::Space
            || c.kind == ClKind::Marker;
        // (Scanned only when something doesn't fit: a long run of hidden text is markers.)
        let has_room_taken = || (c0..j).any(|k| pl.clusters.get(k).is_some_and(takes_room));
        let tail = if !fits && bk.tail_rule && c.kind == ClKind::Text && has_room_taken() { self.tail(bk, pl, c, advance) } else { Tail::No };
        let fits = fits || tail != Tail::No;
        if !fits && pl.is_edit_boundary(c.start) && has_room_taken() {
            self.wrap(bk, pl);
            return true;
        }
        self.xs.push(self.x);
        match tail {
            Tail::Half => {
                let half = c.adv / 2.0;
                if let Some(cl) = pl.clusters.get_mut(j) {
                    cl.adv = half;
                }
                self.half_at = Some((j, c.adv));
                self.x += half;
            }
            Tail::Hang => self.hung = Some(j),
            Tail::Pull(d) => {
                if let Some(cl) = pl.clusters.get_mut(j) {
                    cl.adv = (cl.adv - d).max(0.0);
                    for g in pl.glyphs.get_mut(cl.g0 as usize..cl.g1 as usize).into_iter().flatten() {
                        g.dx -= d;
                    }
                }
                self.x += (c.adv - d).max(0.0);
            }
            Tail::No | Tail::Squeeze => self.x += c.adv,
        }
        if c.kind == ClKind::Space {
            self.space_w += c.adv;
        }
        self.shift_pending_tab(pl);
        if is_display {
            self.end_display(pl, bk.n);
            return true;
        }
        if c.break_after {
            self.note_break(bk, pl, c, shrink);
        }
        self.j += 1;
        false
    }

    /// A closing mark that doesn't fit may still end the line (Word for Mac, `hp4`–`hp9`): 。、」…
    /// at half its width when that fits, else hung whole past the edge, one a line (句読点の
    /// ぶら下げ); with the option off, or for 〕〉》, at its full width where the half would fit;
    /// ：；・ likewise, pulled back into the line by whatever of the overflow exceeds a quarter em.
    /// Anything else moves on with the character before it (kinsoku).
    fn tail(&self, bk: &Breaker, pl: &ParaLayout, c: &Cluster, advance: f32) -> Tail {
        let j = self.j as u32;
        let compress = pl.tail_compress.binary_search(&j).is_ok();
        let squeeze = compress || pl.tail_squeeze.binary_search(&j).is_ok();
        let centred = pl.tail_centred.binary_search(&j).is_ok();
        let half = advance / 2.0;
        let half_fits = half.is_finite() && self.x + half <= self.right_edge + 0.01;
        if compress && bk.rp.overflow_punct {
            if half_fits {
                Tail::Half
            } else if self.hung.is_none() {
                Tail::Hang
            } else {
                Tail::No
            }
        } else if squeeze && half_fits {
            Tail::Squeeze
        } else if centred && half_fits {
            Tail::Pull((self.x + c.adv - self.right_edge - c.adv / 4.0).max(0.0))
        } else {
            Tail::No
        }
    }

    /// Text after a decimal/centre/right tab shifts as it grows.
    fn shift_pending_tab(&mut self, pl: &mut ParaLayout) {
        let Some((tj, stop, tx)) = self.pending_tab else { return };
        let seg_w = self.x - tx;
        let shift = match stop.align {
            TabAlign::Right => stop.pos - tx - seg_w,
            TabAlign::Center => stop.pos - tx - seg_w / 2.0,
            // Width up to the first '.' in the segment.
            TabAlign::Decimal => stop.pos - tx - width_to_decimal(pl, tj, self.j).unwrap_or(seg_w),
            _ => 0.0,
        };
        let shift = shift.max(0.0);
        let old = pl.clusters.get(tj).map(|c| c.adv).unwrap_or(0.0);
        if (shift - old).abs() > 0.001 {
            let delta = shift - old;
            if let Some(c) = pl.clusters.get_mut(tj) {
                c.adv = shift;
            }
            let from = tj + 1 - self.c0;
            for v in self.xs.iter_mut().skip(from) {
                *v += delta;
            }
            self.x += delta;
        }
    }

    /// A display equation ends its line: markers right after it stay on its line; anything else
    /// starts a new one.
    fn end_display(&mut self, pl: &ParaLayout, n: usize) {
        self.j += 1;
        while self.j < n && pl.clusters.get(self.j).is_some_and(|c| c.kind == ClKind::Marker) {
            self.xs.push(self.x);
            self.j += 1;
        }
        self.end = if self.j >= n { LineEnd::Para } else { LineEnd::Wrap };
    }

    /// The cluster (already on the line) is followed by a break opportunity, or a hyphenation point.
    fn note_break(&mut self, bk: &mut Breaker, pl: &ParaLayout, c: &Cluster, shrink: f32) {
        let j = self.j;
        if pl.hyph_after.binary_search(&(j as u32)).is_err() {
            self.last_break = Some(j);
            self.last_plain = Some((j, self.x));
            self.word_from = if c.kind == ClKind::Space { self.x - c.adv } else { self.x };
        } else if self.hyph_ok
            && !pl.clusters.get(j + 1).is_some_and(|n| n.kind == ClKind::Text && n.g1 <= n.g0)
            && self.x + hyphen_glyph(pl, c.style, &mut bk.hcache).2 <= self.right_edge + shrink + 0.01
        {
            self.last_break = Some(j);
        }
    }

    /// The cluster at `j` doesn't fit: wrap, backing up to the last break opportunity on the line.
    fn wrap(&mut self, bk: &mut Breaker, pl: &mut ParaLayout) {
        let c0 = self.c0;
        self.end = LineEnd::Wrap;
        // Hyphenate only when the plain break leaves more than the hyphenation zone empty.
        if let (Some(bk_i), Some((pj, px))) = (self.last_break, self.last_plain)
            && pj >= c0
            && pj < bk_i
            && pl.hyph_after.binary_search(&(bk_i as u32)).is_ok()
            && self.right_edge - px <= bk.zone
        {
            self.last_break = Some(pj);
        }
        self.tab_with_its_text(pl);
        if let Some(b) = self.last_break.filter(|b| *b >= c0) {
            self.xs.truncate(b + 1 - c0);
            self.j = b + 1;
            // A closing mark kept past the break moves on at its full width.
            if let Some((k, adv)) = self.half_at.filter(|(k, _)| *k >= self.j) {
                if let Some(cl) = pl.clusters.get_mut(k) {
                    cl.adv = adv;
                }
                self.half_at = None;
            }
            self.hung = self.hung.filter(|k| *k < self.j);
            // Recompute x for the kept clusters.
            self.x = self.xs.last().copied().unwrap_or(self.line_start_x) + pl.clusters.get(b).map(|c| c.adv).unwrap_or(0.0);
        } else {
            self.break_long_word(bk, pl);
        }
    }

    /// The text after a right, centre or decimal tab doesn't fit: like Word, the tab goes to the
    /// next line with its text (a TOC entry's page number stays at the right margin, after a
    /// leader) instead of ending this line.
    fn tab_with_its_text(&mut self, pl: &ParaLayout) {
        if let Some((tj, ..)) = self.pending_tab
            && self.last_break == Some(tj)
            && has_content(pl, self.c0, tj)
        {
            self.last_break = Some(tj - 1);
            self.pending_tab = None;
            self.leaders.retain(|(k, _)| *k < tj);
        }
    }

    /// No opportunity to break: break here (a long word), unless the span is beside a floating
    /// object and the word fits the full line; then it tries the row's next span, or the next row.
    /// The word runs to the next break that isn't a hyphenation point.
    fn break_long_word(&mut self, bk: &Breaker, pl: &ParaLayout) {
        let (c0, j) = (self.c0, self.j);
        let full = bk.base_right - self.row_lo;
        self.skip = self.narrowed && bk.skipped < 50 && bk.widths.get(c0).is_some_and(|w| *w <= full + 0.01);
        // Never split a ligature's letters or a grapheme's drawing segments across
        // lines: back up to their first segment (unless it starts the line).
        let glyphless = |k: usize| pl.clusters.get(k).is_some_and(|c| c.kind == ClKind::Text && c.g1 <= c.g0);
        let mut b = j;
        while b > c0 + 1 && (glyphless(b) || pl.clusters.get(b).is_some_and(|c| !pl.is_edit_boundary(c.start))) {
            b -= 1;
        }
        while b > c0 && pl.clusters.get(b).is_some_and(|c| !pl.is_edit_boundary(c.start)) {
            b -= 1;
        }
        if b < j
            && b > c0
            && !self.skip
            && let Some(bx) = self.xs.get(b - c0).copied()
        {
            self.xs.truncate(b - c0);
            self.x = bx;
            self.j = b;
        }
    }
}

/// Narrowest span text flows into beside a floating object, points. Word puts an empty
/// paragraph beside a floating table in a 21.3pt gap but not in an 18.6pt one.
const MIN_SPAN: f32 = 20.0;

/// How far below a line's top its baseline sits. Multiple spacing at or above single keeps the
/// text at the top of the line and puts the extra leading below the baseline; below single it
/// shrinks the whole box, ascent included (Word's output for docxide-pdf's fixtures: 1.15-spaced
/// Times New Roman 12pt sits one ascent below the line top, not 1.8pt lower). Exact and at-least
/// spacing keep the descent below the baseline and the rest above it.
fn baseline_offset(spacing: LineSpacing, asc: f32, height: f32, desc: f32) -> f32 {
    match spacing {
        LineSpacing::Multiple(m) => asc * m.min(1.0),
        LineSpacing::AtLeast(_) => height - desc,
        // Word for Mac puts the baseline of an exact line 80% of the way down whatever the font:
        // 24 pt below the top of a 30-pt line of 10.5-pt 游明朝 and of 12-pt Times New Roman, 6.4
        // below that of an 8-pt line of the Times (`dc3` probe, 2026-10-19).
        LineSpacing::Exactly(_) => height * 0.8,
    }
}

/// How far above the first line's baseline a drop cap's ink reaches, in em of the text's size:
/// the top of the line's em box, 1 em above the bottom of the face's `hhea` descent (Times New
/// Roman, Arial, Georgia, Verdana, Courier New, Comic Sans MS, Impact, Apple SD Gothic Neo,
/// Songti SC, Meiryo, Osaka and ＭＳ 明朝/ゴシック in Word for Mac — `dc1`–`dc5` probes, 2026-10-19,
/// each within the half-point rounding); Office's own 游明朝/游ゴシック put it 0.45 em up and Toppan
/// Bunkyu 0.04 (measured: their tables aren't ours to read). `requested` is the family the
/// letter was asked for, `face` the one drawing it.
pub(super) fn word_drop_cap_top(requested: Option<&str>, face: &wordcraft_fonts::FontFace) -> f32 {
    let yu = |f: &str| wordcraft_fonts::word::is_office_yu(f);
    let toppan = |f: &str| f.starts_with("Toppan Bunkyu");
    if requested.is_some_and(yu) || yu(&face.family) {
        return 0.45;
    }
    if requested.is_some_and(toppan) || toppan(&face.family) {
        return 0.04;
    }
    (1.0 - (face.hhea_descent() / face.upem.max(1.0)) as f32).clamp(0.05, 1.0)
}

/// The font size Word for Mac gives the drop cap letter of `p` (see `layout_para`): the paragraph
/// laid out with the letter's own size cleared, so that the text's size and font decide. `None`
/// when the paragraph has no drop cap or nothing to drop.
pub fn drop_cap_size(doc: &Document, p: &Paragraph) -> Option<f32> {
    let first = p.text.chars().next()?;
    let mut p = p.clone();
    p.format(0, first.len_utf8(), &|c| c.size = None).ok()?;
    let fields = FieldCtx::default();
    let env = ParaEnv {
        doc,
        width: 468.0,
        label: None,
        fields: &fields,
        show_hidden: false,
        field_codes: false,
        hide_deleted: Default::default(),
        table: None,
        proofing: None,
        typing_word: None,
        numeral: NumeralMode::default(),
        exclusions: &[],
        eq_number: 0,
        read_line_spacing: None,
        grid: Grid::default(),
        fit_objects: false,
        vertical: false,
        fitted: &[],
    };
    let pl = layout_para(&p, &env);
    pl.drop_cap.filter(|d| d.0 > 0)?;
    pl.clusters.first().and_then(|c| pl.styles.get(c.style as usize)).map(|s| s.size)
}

/// The spans of a row at `top` (about `h` tall) between `lo` and `hi` that text can use around
/// the exclusions, left to right (Square wrapping uses both sides of an object). `Err(y)`: none
/// here, try again at `y` (below what's in the way).
fn row_spans(exclusions: &[Exclusion], top: f32, h: f32, table_h: f32, lo: f32, hi: f32) -> Result<VecDeque<(f32, f32)>, f32> {
    let here: Vec<&Exclusion> = exclusions.iter().filter(|e| e.intersects_row(top, h, table_h, lo, hi)).collect();
    if let Some(below) = here.iter().filter(|e| e.top_bottom).map(|e| e.bottom).reduce(f32::max) {
        return Err(below);
    }
    // What each object takes of the row: its whole area, or (Tight, Through) the stretches its
    // outline takes, as (left, right, bottom of the object's area).
    let mut taken: Vec<(f32, f32, f32)> = Vec::new();
    for e in &here {
        let first = taken.len();
        match &e.contour {
            None => taken.push((e.left, e.right, e.bottom)),
            Some(c) => taken.extend(c.occupied(top - e.top, top + h - e.top).into_iter().map(|(a, b)| (a + e.left, b + e.left, e.bottom))),
        }
        // Text on one side only: the object's other side of the row is taken too.
        let Some(mine) = taken.get(first..).filter(|t| !t.is_empty() && !e.side.is_both()) else { continue };
        let left = mine.iter().map(|t| t.0).fold(f32::INFINITY, f32::min);
        let right = mine.iter().map(|t| t.1).fold(f32::NEG_INFINITY, f32::max);
        let text_left = match e.side {
            WrapText::Left => true,
            WrapText::Right => false,
            // The side with more room on this line (left on a tie).
            _ => left - lo >= hi - right,
        };
        taken.truncate(first);
        taken.push(if text_left { (left, hi.max(right), e.bottom) } else { (lo.min(left), right, e.bottom) });
    }
    taken.retain(|(l, r, _)| *r > lo && *l < hi);
    if taken.is_empty() {
        return Ok(VecDeque::from([(lo, hi)]));
    }
    let mut spans = vec![(lo, hi)];
    for &(left, right, _) in &taken {
        spans = spans
            .into_iter()
            .flat_map(|(a, b)| if right <= a || left >= b { vec![(a, b)] } else { vec![(a, left.min(b)), (right.max(a), b)] })
            .collect();
    }
    spans.retain(|(a, b)| b - a >= MIN_SPAN);
    if spans.is_empty() {
        // Nothing wide enough: the line goes below the first object that ends.
        return Err(taken.iter().map(|t| t.2).reduce(f32::min).unwrap_or(top));
    }
    Ok(spans.into())
}

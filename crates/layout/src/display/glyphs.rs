use super::page::revision_color;
use super::paint::Stroke;
use super::{ClKind, Document, Draw, HashMap, Paint, ParaLayout, PathSeg, Rect, Rgb, ShapeKind, Spin, TextColor, Underline};

/// Each author's place among `authors` in order of first appearance (which colour marks their
/// changes and comments), found in one pass.
pub(crate) fn author_order<'a>(authors: impl Iterator<Item = &'a str>) -> HashMap<&'a str, u32> {
    let mut seen = HashMap::new();
    for a in authors {
        crate::work::steps(1);
        let n = u32::try_from(seen.len()).unwrap_or(u32::MAX);
        seen.entry(a).or_insert(n);
    }
    seen
}

/// The colours of the document's revision authors ([`author_order`]), worked out once for a
/// drawing, when its first tracked change is drawn, not once for every change.
pub(super) struct Authors<'a> {
    doc: &'a Document,
    order: std::cell::OnceCell<HashMap<&'a str, u32>>,
}

impl<'a> Authors<'a> {
    pub(super) fn new(doc: &'a Document) -> Self {
        Authors { doc, order: std::cell::OnceCell::new() }
    }

    /// The colour of revision `r`'s author.
    pub(super) fn color(&self, r: u32) -> Rgb {
        let order = self.order.get_or_init(|| author_order(self.doc.revisions.iter().map(|v| v.author.as_str())));
        revision_color(self.doc.revisions.get(r as usize).and_then(|v| order.get(v.author.as_str()).copied()).unwrap_or(0))
    }
}

/// How far a legacy check box's square sits inside the room it takes on its line, on every side.
pub const CHECK_BOX_INSET: f32 = 1.16;

/// A legacy form check box (`FORMCHECKBOX`) in `frame`, the room it takes on its line (as wide
/// as the text's line is tall): a square inset [`CHECK_BOX_INSET`] with a 0.72 pt border, and,
/// checked, an X from corner to corner in 0.48 pt lines, in the text's colour. Measured on Word's
/// PDF export: a 9.96 pt square for 10 pt Calibri, 22.08 pt for 20 pt.
pub fn check_box(frame: Rect, checked: bool, color: Rgb, alpha: f32, out: &mut Vec<Draw>) {
    let i = CHECK_BOX_INSET.min(frame.w / 4.0).min(frame.h / 4.0).max(0.0);
    let sq = Rect::new(frame.x + i, frame.y + i, (frame.w - 2.0 * i).max(0.5), (frame.h - 2.0 * i).max(0.5));
    let line =
        |x0: f32, y0: f32, x1: f32, y1: f32, width: f32| Draw::Line { x0, y0, x1, y1, width, color, stroke: Stroke::Solid, alpha, text_bg: None };
    let (l, t, r, b) = (sq.x, sq.y, sq.x + sq.w, sq.y + sq.h);
    out.push(Draw::Shape {
        rect: sq,
        kind: ShapeKind::Rectangle,
        fill: None,
        stroke: Some(color),
        stroke_width: 0.72,
        effects: Default::default(),
        corner: None,
        dash: Vec::new(),
        cap: Default::default(),
    });
    if checked {
        out.extend([line(l, t, r, b, 0.48), line(r, t, l, b, 0.48)]);
    }
}

/// Automatic colour is black, or white on a dark background.
pub fn text_color(c: &TextColor, background: Option<Rgb>) -> Rgb {
    match c {
        TextColor::Rgb(r) => *r,
        TextColor::Auto => {
            if background.is_some_and(|b| b.luma() < 100.0) {
                Rgb::WHITE
            } else {
                Rgb::BLACK
            }
        }
    }
}

pub fn upright_in_vertical(c: char) -> bool {
    matches!(c,
        '\u{1100}'..='\u{11FF}'
        | '\u{2E80}'..='\u{2FFF}'
        | '\u{3001}'..='\u{3007}'
        | '\u{3012}'..='\u{3013}'
        | '\u{3020}'..='\u{302F}'
        | '\u{3031}'..='\u{303F}'
        | '\u{3040}'..='\u{30FB}'
        | '\u{30FD}'..='\u{30FF}'
        | '\u{3100}'..='\u{31EF}'
        | '\u{31F0}'..='\u{4DBF}'
        | '\u{4E00}'..='\u{9FFF}'
        | '\u{A960}'..='\u{A97F}'
        | '\u{AC00}'..='\u{D7FF}'
        | '\u{F900}'..='\u{FAFF}'
        | '\u{FE30}'..='\u{FE4F}'
        | '\u{FF01}'..='\u{FF07}'
        | '\u{FF0A}'..='\u{FF0C}'
        | '\u{FF0E}'..='\u{FF3A}'
        | '\u{FF3C}'
        | '\u{FF3E}'..='\u{FF5A}'
        | '\u{FFE0}'..='\u{FFE6}'
        | '\u{20000}'..='\u{3FFFF}'
    )
}

/// An emphasis mark centred at (`x`, `y`) for text of `size`: a filled dot, a sesame dot (the
/// comma-like ﹅ Word draws for "comma"), an open circle, or a filled dot below the text.
///
/// Sized as Word for Mac draws them with 游明朝 (ink measured at 8–36 pt): a dot 0.12 em
/// across, a ring 0.19 em across with a stroke a fortieth of an em, a sesame dot in a 0.22-em box.
/// One emphasis mark centred on `(x, y)` (the mark's ink centre), in the line's frame. Word for
/// Mac draws the marks as glyphs — a full-width full stop and ideographic comma, a ring above
/// and a full-width full stop under the text, upright in a vertical column too — so the comma is
/// drawn upright on the page: thin at the upper left, round at the lower right, turned back
/// against a vertical frame's turn.
pub(super) fn emphasis_mark(em: wordcraft_doc::props::EmphasisMark, x: f32, y: f32, size: f32, color: Rgb, vertical: bool) -> Option<Draw> {
    use wordcraft_doc::props::EmphasisMark;
    let size = size.clamp(1.0, 1638.0);
    match em {
        EmphasisMark::None => None,
        EmphasisMark::Dot | EmphasisMark::UnderDot => {
            let d = size * 0.12;
            Some(Draw::Shape {
                rect: Rect::new(x - d / 2.0, y - d / 2.0, d, d),
                kind: ShapeKind::Ellipse,
                fill: Some(Paint::solid(color)),
                stroke: None,
                stroke_width: 0.0,
                effects: Default::default(),
                corner: None,
                dash: Vec::new(),
                cap: Default::default(),
            })
        }
        EmphasisMark::Circle => {
            let d = size * 0.17;
            Some(Draw::Shape {
                rect: Rect::new(x - d / 2.0, y - d / 2.0, d, d),
                kind: ShapeKind::Ellipse,
                fill: None,
                stroke: Some(color),
                stroke_width: (size * 0.025).max(0.3),
                effects: Default::default(),
                corner: None,
                dash: Vec::new(),
                cap: Default::default(),
            })
        }
        EmphasisMark::Comma => {
            // A sesame dot (ごま点): the ideographic comma's teardrop, a thin tip at the upper left
            // running into a round head at the lower right; its ink box is 0.22 × 0.23 em around
            // the centre, as measured in Word for Mac. The shape is built upright (y down) and
            // turned a quarter back when the frame is a vertical column, so it stands upright on
            // the page as Word's glyph does.
            let at = |dx: f32, dy: f32| -> (f32, f32) {
                let (dx, dy) = if vertical { (dy, -dx) } else { (dx, dy) };
                (x + dx * size, y + dy * size)
            };
            let (hx, hy, r) = (0.045, 0.05, 0.062);
            let k = r * 0.5523;
            let tip = at(-0.11, -0.115);
            let right = at(hx + r, hy);
            let bottom = at(hx, hy + r);
            let left = at(hx - r, hy);
            let c = |dx: f32, dy: f32| at(dx, dy);
            let segs = vec![
                PathSeg::Move(tip.0, tip.1),
                PathSeg::Cubic(c(-0.04, -0.105).0, c(-0.04, -0.105).1, c(hx + r, hy - 0.08).0, c(hx + r, hy - 0.08).1, right.0, right.1),
                PathSeg::Cubic(c(hx + r, hy + k).0, c(hx + r, hy + k).1, c(hx + k, hy + r).0, c(hx + k, hy + r).1, bottom.0, bottom.1),
                PathSeg::Cubic(c(hx - k, hy + r).0, c(hx - k, hy + r).1, c(hx - r, hy + k).0, c(hx - r, hy + k).1, left.0, left.1),
                PathSeg::Cubic(
                    c(hx - r - 0.015, hy - 0.035).0,
                    c(hx - r - 0.015, hy - 0.035).1,
                    c(-0.105, -0.05).0,
                    c(-0.105, -0.05).1,
                    tip.0,
                    tip.1,
                ),
                PathSeg::Close,
            ];
            Some(Draw::Path { segs, fill: Some(Paint::solid(color)), stroke: None, stroke_width: 0.0, dash: Vec::new(), cap: Default::default() })
        }
    }
}

/// Where a format run's lines go, in the line's frame.
#[derive(Clone, Copy, Debug)]
pub(super) struct DecoGeo {
    vertical: bool,
    /// Horizontal text: the underline's top edge (y) and thickness. A vertical column: the
    /// line's near edge (v, facing the text; the line lies beyond it, towards the page's right).
    ul: f32,
    ul_t: f32,
    /// The strikethrough's centre (y; a vertical column's centre line) and thickness.
    st: f32,
    st_t: f32,
    /// An East Asian face draws some of the clusters: its wave hangs from the line's top
    /// edge and swings wider than a Latin face's.
    ea: bool,
}

/// The line geometry for the clusters `k0..k1` of `line` (a format run, or one cluster for
/// word-by-word underlining) about `baseline` (the run's baseline y; a vertical column's centre
/// line v), as Word for Mac draws it, measured on 2026-10-19 with 游明朝 and Aptos:
/// - Horizontal text: each face's own position and thickness
///   (`wordcraft_fonts::word::decoration_metrics`); a run mixing faces (abc漢字) gets one line
///   at the mean of their positions, the thickest of their thicknesses (1.44 pt below the
///   baseline for 10.5-pt Aptos at 0.48 and 游明朝 at 2.16).
/// - A vertical column: a 0.72-pt line at every size, its near edge 0.025 em beyond the
///   standing characters' em square (0.30 pt at 10.5, 0.44 at 16, 0.48 at 24); lying Latin
///   text's line is its ascent beyond its baseline (9.6 pt right of 10.5-pt Aptos's, 14.9 of
///   16-pt's); a mixed run's line is the outer one. The strikethrough runs along the standing
///   em squares' centre (the column's centre line, `v_centre` toward the ascent side), the
///   faces' own thickness.
pub(super) fn decoration_geometry(
    pl: &ParaLayout,
    para: Option<&wordcraft_doc::Paragraph>,
    line: &crate::para::Line,
    k0: usize,
    k1: usize,
    baseline: f32,
) -> DecoGeo {
    // The faces drawing the clusters, each with whether any of its characters stands upright
    // in a vertical column.
    let mut faces: Vec<(u32, &crate::para::StyleRun, bool)> = Vec::new();
    for k in k0.max(line.c0)..k1.min(line.c1) {
        let Some(c) = pl.clusters.get(k) else { continue };
        if !matches!(c.kind, ClKind::Text | ClKind::Space) {
            continue;
        }
        let Some(st) = pl.styles.get(c.style as usize) else { continue };
        let stands = c.kind == ClKind::Text
            && para.and_then(|p| p.text.get(c.start..c.end)).is_some_and(|t| t.chars().any(|ch| upright_in_vertical(ch) || !ch.is_ascii()));
        match faces.iter_mut().find(|(id, ..)| *id == st.face.id()) {
            Some(f) => f.2 |= stands,
            None => faces.push((st.face.id(), st, stands)),
        }
    }
    let n = faces.len().max(1) as f32;
    let st_t = faces.iter().map(|(_, st, _)| st.deco.strike_thickness).fold(0.0f32, f32::max).max(0.1);
    let ea = faces.iter().any(|(_, st, _)| st.face.get().is_east_asian());
    if pl.vertical {
        // The standing em squares sit `v_centre` toward the ascent side of the centre line.
        let near = faces
            .iter()
            .map(
                |(_, st, stands)| {
                    if *stands { baseline - st.v_centre - 0.525 * st.size } else { baseline + (st.ascent - st.descent) / 2.0 - st.glyph_ascent }
                },
            )
            .fold(f32::INFINITY, f32::min);
        let size = faces.iter().map(|(_, st, _)| st.size).fold(0.0f32, f32::max);
        let ul = if near.is_finite() { near } else { baseline - 0.525 * size };
        let standing: Vec<f32> = faces.iter().filter(|(_, _, stands)| *stands).map(|(_, st, _)| st.v_centre).collect();
        let v = if standing.is_empty() { 0.0 } else { standing.iter().sum::<f32>() / standing.len() as f32 };
        return DecoGeo { vertical: true, ul, ul_t: 0.72, st: baseline - v, st_t, ea };
    }
    let ul = baseline + faces.iter().map(|(_, st, _)| st.deco.underline_top).sum::<f32>() / n;
    let ul_t = faces.iter().map(|(_, st, _)| st.deco.underline_thickness).fold(0.0f32, f32::max).max(0.1);
    let st = baseline - faces.iter().map(|(_, st, _)| st.deco.strike_top - st.deco.strike_thickness / 2.0).sum::<f32>() / n;
    DecoGeo { vertical: false, ul, ul_t, st, st_t, ea }
}

/// The underline of `kind` from `x0` to `x1` with geometry `geo`, as Word for Mac draws it
/// (measured with 游明朝 at 8–48 pt and Aptos at 10.5–24):
/// - thick: twice the thickness, at least 1.2 pt, from the same top edge (1.2 pt for 10.5-pt
///   游明朝 and for Aptos at every size, 2.4 at 24-pt 游明朝); in a column 1.44 pt, growing
///   towards the text;
/// - double: a second line below the first, their centres twice the thickness apart, at least
///   1.2 pt (1.2 at 10.5, 1.68 at 16, 2.4 at 24 for 游明朝; 1.2 for Aptos); 0.72 pt apart in a
///   column;
/// - dotted and dashed: Word's fixed patterns (`Stroke::Dashes`);
/// - wavy: a 0.24-pt zigzag of 1.2-pt runs; on an East Asian face 1.35 thicknesses peak to
///   peak, its ink hanging from the line's top edge (游明朝: 0.48 at 8–11 pt, 0.72 at 12–14, 0.96
///   at 16, 1.2 at 18–20, 1.44 at 24, 1.92 at 28, 2.16 at 32, 2.64 at 36, 3.36 at 48; Hiragino
///   1.2 at 18, 2.64 at 36), on a Latin face 0.75 thicknesses standing on it (Aptos 0.48 up to
///   24 pt, 0.72 at 36, 0.96 at 48; Times New Roman 0.72 at 18, 1.44 at 36; Arial 0.96 and
///   1.92), at least 0.48; the heavy wave the same zigzag 0.96 thick; the double wave two waves
///   sharing that room, each half the swing less a stroke (at least 0.24), half the room apart
///   (游明朝 at 48 pt: 1.44-pt waves 1.92 apart; at 24: 0.48-pt waves 0.96 apart; at 10.5: 0.24
///   and 0.24); in a column 0.72 pt peak to peak and 0.24 thick at every size.
#[allow(clippy::too_many_arguments)]
pub(super) fn underline_draws(kind: Underline, x0: f32, x1: f32, geo: &DecoGeo, color: Rgb, alpha: f32, text_bg: Option<Rgb>, out: &mut Vec<Draw>) {
    // Word's patterns, points, the same at every size (measured at 10.5 and 24 pt).
    let pattern = match kind {
        Underline::Dotted | Underline::DottedHeavy => Stroke::Dashes { on: 1.44, off: 1.44, dots: 0 },
        Underline::Dash | Underline::DashedHeavy => Stroke::Dashes { on: 3.84, off: 1.92, dots: 0 },
        Underline::DashLong | Underline::DashLongHeavy => Stroke::Dashes { on: 7.92, off: 3.84, dots: 0 },
        Underline::DotDash | Underline::DashDotHeavy => Stroke::Dashes { on: 2.88, off: 0.96, dots: 1 },
        Underline::DotDotDash | Underline::DashDotDotHeavy => Stroke::Dashes { on: 2.16, off: 0.72, dots: 2 },
        _ => Stroke::Solid,
    };
    let mut line = |y: f32, width: f32, stroke: Stroke| out.push(Draw::Line { x0, y0: y, x1, y1: y, width, color, stroke, alpha, text_bg });
    let DecoGeo { vertical, ul, ul_t, ea, .. } = *geo;
    // A wave's stroke is 0.24 pt at every size, a heavy wave's 0.96 (measured at 10.5 and 24 pt).
    const WAVE: f32 = 0.24;
    const HEAVY_WAVE: f32 = 0.96;
    if vertical {
        let centre = ul - 0.36;
        let wave = Stroke::Zigzag { amp: 0.36, step: 1.2 };
        match kind {
            Underline::Double => {
                line(centre, 0.72, Stroke::Solid);
                line(centre - 1.44, 0.72, Stroke::Solid);
            }
            Underline::Wave => line(ul - 0.12 - 0.36, WAVE, wave),
            Underline::WavyHeavy => line(ul - 0.12 - 0.36, HEAVY_WAVE, wave),
            Underline::DoubleWave => {
                line(ul - 0.12 - 0.18, WAVE, Stroke::Zigzag { amp: 0.18, step: 1.2 });
                line(ul - 0.12 - 0.54, WAVE, Stroke::Zigzag { amp: 0.18, step: 1.2 });
            }
            _ if kind.is_heavy() => line(ul, 1.44, pattern),
            _ => line(centre, 0.72, pattern),
        }
        return;
    }
    let t = ul_t;
    let centre = ul + t / 2.0;
    let heavy = (2.0 * t).max(1.2);
    // A wave's swing (peak to peak of the zigzag's centre line) and the room its ink takes: an
    // East Asian face's wave hangs from the line's top edge, a Latin face's stands on it.
    let swing = (if ea { 1.35 } else { 0.75 } * t).max(0.48);
    let room = swing + WAVE;
    let wave_centre = if ea { ul + room / 2.0 } else { ul - room / 2.0 };
    let zigzag = Stroke::Zigzag { amp: swing / 2.0, step: 1.2 };
    match kind {
        Underline::Double => {
            line(centre, t, Stroke::Solid);
            line(centre + heavy, t, Stroke::Solid);
        }
        Underline::Wave => line(wave_centre, WAVE, zigzag),
        Underline::WavyHeavy => line(wave_centre, HEAVY_WAVE, zigzag),
        Underline::DoubleWave => {
            // Two waves sharing the single wave's room: each half the swing less a stroke (at
            // least 0.24), their tops half the room apart.
            let small = ((swing - WAVE) / 2.0).max(WAVE);
            let amp = small / 2.0;
            let first = if ea { ul + WAVE / 2.0 + amp } else { ul - room + WAVE / 2.0 + amp };
            let zigzag = Stroke::Zigzag { amp, step: 1.2 };
            line(first, WAVE, zigzag);
            line(first + room / 2.0, WAVE, zigzag);
        }
        _ if kind.is_heavy() => line(ul + heavy / 2.0, heavy, pattern),
        _ => line(centre, t, pattern),
    }
}

/// The strikethrough from `x0` to `x1`: one line on `geo.st`, or two a thickness apart either
/// side of it (Word's double strikethrough: 10.5-pt 游明朝's lines centred 3.6 and 4.56 pt above
/// the baseline, the single one at 4.08).
#[allow(clippy::too_many_arguments)]
pub(super) fn strike_draws(double: bool, x0: f32, x1: f32, geo: &DecoGeo, color: Rgb, alpha: f32, text_bg: Option<Rgb>, out: &mut Vec<Draw>) {
    let t = geo.st_t;
    let mut line = |y: f32| out.push(Draw::Line { x0, y0: y, x1, y1: y, width: t, color, stroke: Stroke::Solid, alpha, text_bg });
    if double {
        line(geo.st - t);
        line(geo.st + t);
    } else {
        line(geo.st);
    }
}

/// Top-to-bottom text drawn turned 90° clockwise, with its East Asian characters turned back
/// upright, each about the centre of its em square (`tbRl`: ideographs stand, Latin lies).
/// Glyph runs are split where uprightness changes, so the text keeps its order.
///
/// A glyph the font substituted for vertical text (OpenType `vert`: the vertical forms of
/// 、。「」（）ー and the small kana) is a standing form already and stands as it is, with no
/// correction. Word for Mac keeps the ellipsis and dashes (U+2010–U+2027) lying along the line
/// instead: those draw their horizontal form turned.
pub(super) fn upright_east_asian(items: Vec<Draw>) -> Vec<Draw> {
    let mut out = Vec::with_capacity(items.len());
    for d in items {
        match d {
            Draw::Scaled { x, sx, items } => out.push(Draw::Scaled { x, sx, items: upright_east_asian(items) }),
            d => stand_up(d, None, None, &mut out),
        }
    }
    out
}

/// Stands the East Asian characters of a glyph run `d` up (see [`upright_east_asian`]) into
/// `out`. With `centre` — the centre line of a vertical column, in the run's unturned frame
/// (`ParaLayout::vertical`) — each standing character's em square is centred on it whatever its
/// size, as Word lays a vertical column out; without it the run lies on a horizontal baseline
/// turned onto its side, and each em square stands about its own centre, 0.38 em above that
/// baseline. Other draws pass through.
///
/// `origin_em`: where Word hangs the glyphs of one of its own table fonts named for this run
/// (`StyleRun::named`), in em below the pen, in place of the face's own vertical origins.
pub(super) fn stand_up(d: Draw, centre: Option<f32>, origin_em: Option<f32>, out: &mut Vec<Draw>) {
    let Draw::Glyphs { face, size, mut glyphs, color, alpha, synth_bold, synth_italic, text, link, ranges, text_bg, x_scale } = d else {
        out.push(d);
        return;
    };
    let char_at = |k: usize| ranges.get(k).and_then(|r| text.get(r.clone())).and_then(|t| t.chars().next());
    // A vertical alternate: the glyph is not the character's horizontal one.
    let alternate = |k: usize, gid: u32| char_at(k).is_some_and(|c| !c.is_whitespace() && face.glyph_for(c) != gid && gid != 0);
    if ranges.len() == glyphs.len() {
        for k in 0..glyphs.len() {
            if let Some(c) = char_at(k)
                && lies_in_vertical(c)
                && let Some(g) = glyphs.get_mut(k)
                && alternate(k, g.0)
            {
                g.0 = face.glyph_for(c);
            }
        }
    }
    let up = |k: usize| char_at(k).is_some_and(|c| glyphs.get(k).is_some_and(|g| stands_in_vertical(c, &face, g.0)));
    if ranges.len() != glyphs.len() || !(0..glyphs.len()).any(up) {
        out.push(Draw::Glyphs { face, size, glyphs, color, alpha, synth_bold, synth_italic, text, link, ranges, text_bg, x_scale });
        return;
    }
    let piece = |k0: usize, k1: usize| -> Draw {
        let gs = glyphs.get(k0..k1).unwrap_or(&[]).to_vec();
        let rs = ranges.get(k0..k1).unwrap_or(&[]);
        let (b0, b1) = (rs.iter().map(|r| r.start).min().unwrap_or(0), rs.iter().map(|r| r.end).max().unwrap_or(0));
        let t = text.get(b0..b1).unwrap_or("").to_string();
        let rs = rs.iter().map(|r| r.start.saturating_sub(b0)..r.end.saturating_sub(b0)).collect();
        Draw::Glyphs { face, size, glyphs: gs, color, alpha, synth_bold, synth_italic, text: t, link: link.clone(), ranges: rs, text_bg, x_scale }
    };
    let mut k0 = 0;
    while k0 < glyphs.len() {
        let u = up(k0);
        let k1 = (k0 + 1..glyphs.len()).find(|&k| up(k) != u).unwrap_or(glyphs.len());
        if !u {
            out.push(piece(k0, k1));
        } else {
            for k in k0..k1 {
                let Some(&(gid, gx, base)) = glyphs.get(k) else { continue };
                // The glyph's own box: its horizontal advance wide (an em for a full-width
                // character, 0.865 em for Apple SD Gothic Neo's Hangul — never the layout's
                // distance to the next glyph, which a compressed mark shortens and distributed
                // ruby letters or expanded spacing lengthen), hanging from its pen by its
                // vertical origin (`FontFace::vertical_origin`: 0.88 em for Hiragino and 游明朝,
                // 0.9 for Apple SD Gothic Neo, 0.80 for Songti SC). Standing it up about the
                // box's centre puts the box across the column centred on `centre` and its
                // origin at the pen, as Word for Mac places them — every glyph of a run on one
                // line across the column, the last one too (vt6/vt7, 2026-10-19).
                let k_em = size / face.upem.max(1.0) as f32;
                let hadv = (face.advance(gid) as f32 * k_em).clamp(0.0, size * 4.0);
                let origin = origin_em.map_or_else(|| face.vertical_origin(gid) as f32 * k_em, |o| o * size).clamp(-size * 4.0, size * 4.0);
                let lift = origin - hadv / 2.0;
                let (cx, cy) = (gx + hadv / 2.0, centre.unwrap_or(base - lift));
                let mut g = piece(k, k + 1);
                // On a column's centre line the box stands centred: its baseline moves below the
                // centre by the origin less half the box first.
                if let Some(c) = centre
                    && let Draw::Glyphs { glyphs, .. } = &mut g
                {
                    for (_, _, y) in glyphs.iter_mut() {
                        *y = c + lift;
                    }
                }
                // The ideographic comma and full stop sit in the upper right of their square in
                // vertical text (lower left in horizontal): moved there before standing up —
                // unless the font's vertical form (which sits there already) was used.
                if let Draw::Glyphs { text, glyphs, .. } = &mut g
                    && matches!(text.chars().next(), Some('\u{3001}' | '\u{3002}' | '\u{FF0C}' | '\u{FF0E}'))
                    && !alternate(k, gid)
                {
                    for (_, x, y) in glyphs.iter_mut() {
                        *x += size * 0.55;
                        *y -= size * 0.55;
                    }
                }
                out.push(Draw::Rotated { cx, cy, spin: Spin::new(-90.0, false, false), items: vec![g] });
            }
        }
        k0 = k1;
    }
}

/// Whether `c` stands upright in top-to-bottom East Asian text (`tbRl`) rather than turning with
/// the line: ideographs, kana, Hangul, full-width letters and digits, and the ideographic comma
/// and full stop (an approximation of Unicode's vertical orientation, UAX #50). Brackets, dashes
/// and the prolonged sound mark turn with the line, as their vertical forms do.
/// Dashes and quotation marks (U+2010–U+2027) lie along a vertical column even when the font
/// has vertical alternates for them.
/// Characters that lie on their side in a vertical column whatever vertical alternate the font
/// offers: the dashes and ellipses of General Punctuation, and the half-width katakana, which
/// Word for Mac lays lying at half an em each (`hw2`, 2026-10-19).
fn lies_in_vertical(c: char) -> bool {
    matches!(c, '\u{2010}'..='\u{2027}' | '\u{FF61}'..='\u{FF9F}')
}

/// Does character `c`, shaped as glyph `gid` of `face`, stand upright in a vertical column?
/// East Asian characters do ([`upright_in_vertical`]), and so does any character the font gave a
/// vertical alternate (`vert`: its glyph is not the character's horizontal one) unless it lies
/// by rule ([`lies_in_vertical`]).
pub fn stands_in_vertical(c: char, face: &wordcraft_fonts::FontFace, gid: u32) -> bool {
    upright_in_vertical(c) || (!lies_in_vertical(c) && !c.is_whitespace() && gid != 0 && face.glyph_for(c) != gid)
}

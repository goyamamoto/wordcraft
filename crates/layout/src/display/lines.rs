use super::glyphs::{
    Authors, check_box, decoration_geometry, emphasis_mark, stand_up, strike_draws, text_color, underline_draws, upright_in_vertical,
};
use super::objects::{graphic_draws, inline_rect, object_draws, spun};
use super::options::DisplayOptions;
use super::page::{char_box, run_box};
use super::paint::Stroke;
use super::{
    Block, Border, ClKind, DeleteMark, Document, Draw, InlineObject, InsertMark, LineEnd, MItem, ParaLayout, Path, Rect, Rgb, SectionStart, Spin,
    StoryRef, TextColor, Underline,
};
use wordcraft_doc::Paragraph;

/// Append display items for the half-open line range `l0..l1` of a laid-out paragraph.
/// `(x, y)` positions the paragraph's left origin and the first selected line's top; later lines
/// retain their layout offsets. Includes glyphs, inline objects, decorations and requested markup.
/// `down` keeps vertical East Asian glyphs upright before the caller turns the resulting draws.
pub(super) struct LineRender<'a, 'authors, 'author_doc> {
    pub doc: &'a Document,
    pub story: StoryRef,
    pub path: &'a Path,
    pub pl: &'a ParaLayout,
    pub range: std::ops::Range<usize>,
    pub origin: (f32, f32),
    pub opts: &'a DisplayOptions,
    pub authors: &'authors Authors<'author_doc>,
    pub alpha: f32,
    pub down: bool,
}

struct LineContext<'a, 'authors, 'author_doc> {
    doc: &'a Document,
    story: StoryRef,
    path: &'a Path,
    pl: &'a ParaLayout,
    para: Option<&'a Paragraph>,
    line: &'a crate::para::Line,
    li: usize,
    x: f32,
    top: f32,
    base: f32,
    opts: &'a DisplayOptions,
    authors: &'authors Authors<'author_doc>,
    alpha: f32,
    down: bool,
    auto: Option<Rgb>,
    /// The end of the line's content (before its trailing spaces, marks and breaks), and where an
    /// underline stops (see `lines`).
    content_tail: usize,
    tail: usize,
}

impl LineContext<'_, '_, '_> {
    fn ink(&self, color: &TextColor, background: Option<Rgb>) -> Rgb {
        match (color, self.auto) {
            (TextColor::Auto, Some(rgb)) => rgb,
            _ => text_color(color, background),
        }
    }
}

pub(super) fn lines(input: LineRender<'_, '_, '_>, out: &mut Vec<Draw>) {
    let LineRender { doc, story, path, pl, range, origin: (x, y), opts, authors, alpha, down } = input;
    let (l0, l1) = (range.start, range.end);
    let Some(first) = pl.lines.get(l0) else { return };
    let para = doc.para(story, path);
    // Inside a text box whose shape style gives its text a colour, that is what automatic means.
    let auto = match story {
        StoryRef::Part(id) => doc.parts.get(&id).and_then(|p| p.body.auto_color),
        StoryRef::Body => None,
    };
    for li in l0..l1 {
        let Some(line) = pl.lines.get(li) else { continue };
        let top = y + (line.top - first.top);
        let base = top + (line.baseline - line.top);
        // The line's trailing spaces, marks and breaks: an underline stops before them — unless
        // the document underlines trailing spaces (`w:ulTrailSpace`, Word's Japanese default):
        // then the spaces before a line break, at the paragraph's end and the one a line wraps
        // at are underlined too, the last no further than the line's right edge. Found once per
        // line (a line of hidden fragments is all marks).
        let trailing = |k: usize| {
            pl.clusters
                .get(k)
                .is_some_and(|c| matches!(c.kind, ClKind::Space | ClKind::Marker | ClKind::LineBreak | ClKind::PageBreak | ClKind::ColumnBreak))
        };
        let mut content_tail = line.c1;
        while content_tail > line.c0 && trailing(content_tail - 1) {
            content_tail -= 1;
        }
        let mut tail = line.c1;
        while tail > content_tail && !(doc.settings.ul_trail_space && pl.clusters.get(tail - 1).is_some_and(|c| c.kind == ClKind::Space)) {
            tail -= 1;
        }
        let ctx = LineContext { doc, story, path, pl, para, line, li, x, top, base, opts, authors, alpha, down, auto, content_tail, tail };
        comment_backgrounds(&ctx, out);
        character_backgrounds(&ctx, out);
        draw_label_and_hyphen(&ctx, out);
        draw_glyph_runs(&ctx, out);
        proofing_marks(&ctx, out);
        tab_leaders(&ctx, out);
        inline_objects(&ctx, out);
        enclosures(&ctx, out);
        draw_rubies(&ctx, out);
        formatting_marks(&ctx, out);
    }
}

fn collect_glyph_run(ctx: &LineContext<'_, '_, '_>, start: usize, style_index: u32, style: &crate::para::StyleRun) -> GlyphRun {
    let (pl, para, line, x, base) = (ctx.pl, ctx.para, ctx.line, ctx.x, ctx.base);
    let si = style_index;
    let st = style;
    let mut k = start;
    let run = pl.clusters.get(start..line.c1.min(pl.clusters.len())).unwrap_or_default();
    let (n_glyphs, n_bytes) = run
        .iter()
        .take_while(|cluster| cluster.style == si)
        .filter(|cluster| matches!(cluster.kind, ClKind::Text | ClKind::Space))
        .fold((0usize, 0usize), |(glyphs, bytes), cluster| {
            (glyphs + cluster.g1.saturating_sub(cluster.g0) as usize, bytes + cluster.end.saturating_sub(cluster.start))
        });
    let mut glyphs = Vec::with_capacity(n_glyphs);
    let mut text = String::with_capacity(n_bytes);
    let mut ranges: Vec<std::ops::Range<usize>> = Vec::with_capacity(n_glyphs);
    let symbol_font = wordcraft_doc::symbol_fonts::is_symbol_font(&st.rc.font);
    // Horizontal-in-vertical clusters, drawn turned after the run (with its colour).
    let mut turned: Vec<(usize, f32)> = Vec::new();
    // The run's text alignment offset (`w:textAlignment`), the same for every cluster of a run on a line.
    let dy = pl.align_dy.get(start).copied().unwrap_or(0.0);
    while k < line.c1 {
        let Some(c) = pl.clusters.get(k) else { break };
        if c.style != si {
            break;
        }
        let cx = x + line.cl_left(k).unwrap_or(0.0);
        if let Ok(i) = pl.tcy.binary_search_by_key(&k, |t| t.0) {
            turned.push((k, pl.tcy.get(i).map(|t| t.1).unwrap_or(0.0)));
            k += 1;
            continue;
        }
        if matches!(c.kind, ClKind::Text | ClKind::Space) {
            let a = text.len();
            // An object's cluster (a field, a note number) stands for the text it shows.
            let mut shown = None;
            if let Ok(i) = pl.shown.binary_search_by_key(&k, |(i, _, _)| *i)
                && let Some((_, s, r)) = pl.shown.get(i)
            {
                // All Caps field results read as the capitals drawn (#1473). `r` holds
                // byte ranges into `s`, so a character whose capital has another UTF-8
                // length keeps its own form and every range still lands where it did.
                text.extend(s.chars().map(|ch| {
                    let up = crate::para::caps_char(&st.rc, ch);
                    if up.len_utf8() == ch.len_utf8() { up } else { ch }
                }));
                shown = Some(r);
            } else if let Some(p) = para {
                let t = p.text.get(c.start..c.end).unwrap_or("");
                // Symbol-font characters (Wingdings…) read as the Unicode symbols drawn.
                if symbol_font {
                    text.push_str(&wordcraft_doc::symbol_fonts::map_text(&st.rc.font, t));
                } else if st.rc.caps || st.rc.small_caps {
                    // All Caps and Small Caps read as the capitals drawn (#1473), mapped
                    // as drawn, so a glyph never stands for two cases and ß reads as SS (#1581).
                    for ch in t.chars() {
                        crate::para::push_caps(&st.rc, ch, &mut text);
                    }
                } else {
                    text.push_str(t);
                }
            }
            let b = text.len();
            let cg = pl.glyphs.get(c.g0 as usize..c.g1 as usize).unwrap_or(&[]);
            if let Some(shown) = shown
                && cg.len() > 1
            {
                // A whole field result is one cluster of many glyphs: each glyph gets the
                // text it was shaped from (a ligature its letters), not the whole text as
                // one ligature. Without that, a character each (the last takes any remainder).
                let bounds: Vec<usize> = text.get(a..b).unwrap_or("").char_indices().map(|(i, _)| a + i).collect();
                let known = shown.len() == cg.len();
                for (n, g) in cg.iter().enumerate() {
                    let r = match shown.get(n).filter(|_| known) {
                        Some(r) => (a + r.start).min(b)..(a + r.end).min(b),
                        None => {
                            let from = bounds.get(n).or(bounds.last()).copied().unwrap_or(a);
                            let to = if n + 1 == cg.len() { b } else { bounds.get(n + 1).copied().unwrap_or(b) };
                            from..to
                        }
                    };
                    glyphs.push((g.gid, cx + g.dx, base - st.shift - dy - g.dy));
                    ranges.push(r.start..r.end.max(r.start));
                }
                k += 1;
                continue;
            }
            if cg.is_empty() {
                // No glyph of its own (the second letter of a ligature such as لا): its
                // text belongs to the glyphs before it.
                let from = ranges.last().map(|r| r.start);
                for r in ranges.iter_mut().rev().take_while(|r| Some(r.start) == from) {
                    r.end = b;
                }
            }
            for g in cg {
                glyphs.push((g.gid, cx + g.dx, base - st.shift - dy - g.dy));
                ranges.push(a..b);
            }
        }
        k += 1;
    }
    GlyphRun { end: k, glyphs, text, ranges, turned, dy }
}

/// A run of clusters of one style on a line, gathered for drawing.
struct GlyphRun {
    /// The cluster after the run.
    end: usize,
    glyphs: Vec<(u32, f32, f32)>,
    text: String,
    ranges: Vec<std::ops::Range<usize>>,
    /// 縦中横 clusters, with their natural width: drawn turned after the run.
    turned: Vec<(usize, f32)>,
    /// The run's text alignment offset.
    dy: f32,
}

fn draw_glyph_runs(ctx: &LineContext<'_, '_, '_>, out: &mut Vec<Draw>) {
    let (doc, pl, line, x, base, opts, authors, alpha, down) =
        (ctx.doc, ctx.pl, ctx.line, ctx.x, ctx.base, ctx.opts, ctx.authors, ctx.alpha, ctx.down);
    let mut k = line.c0;
    while k < line.c1 {
        let Some(c) = pl.clusters.get(k) else { break };
        let si = c.style;
        let Some(st) = pl.styles.get(si as usize) else {
            k += 1;
            continue;
        };
        let run_start = k;
        let GlyphRun { end: run_end, mut glyphs, text, ranges, turned, dy } = collect_glyph_run(ctx, run_start, si, st);
        k = run_end;
        let rc = &st.rc;
        let m = &opts.revisions;
        // Without markup a deletion is never drawn as ordinary text, even in a layout that
        // kept it; nor when its mark is Hidden.
        if rc.del.is_some() && (!opts.markup || m.hides_deletion(doc.move_name(rc.del).is_some())) {
            continue;
        }
        let shown = opts.markup && m.insertions_deletions;
        let del = rc.del.filter(|_| shown);
        let ins = rc.ins.filter(|_| shown && del.is_none());
        let author = |r: u32| authors.color(r);
        // Moved text: the Moves marks and colours (by default green, double strikethrough
        // where it was, double underline where it went).
        let moved_to = doc.move_name(ins).is_some();
        let moved_from = del.is_some() && doc.move_name(del).is_some();
        // Moved text marked (none) is plain text, its colour setting aside (Word for Mac).
        let plain_move = (moved_from && m.move_from_mark == DeleteMark::None) || (moved_to && m.move_to_mark == InsertMark::None);
        let color = match (ins, del) {
            _ if plain_move => text_color(&rc.color, rc.shading.or(rc.highlight)),
            (_, Some(r)) if moved_from => m.move_from_color.unwrap_or_else(|| author(r)),
            (Some(r), _) if moved_to => m.move_to_color.unwrap_or_else(|| author(r)),
            (Some(r), _) => m.insert_color.unwrap_or_else(|| author(r)),
            (None, Some(r)) => m.delete_color.unwrap_or_else(|| author(r)),
            _ => ctx.ink(&rc.color, rc.shading.or(rc.highlight)),
        };
        // The highlight or shading drawn under this run (as filled in the backgrounds pass).
        let text_bg = rc.highlight.or(rc.shading);
        let ins_mark = ins.map(|_| if moved_to { m.move_to_mark } else { m.insert_mark });
        let del_mark = del.map(|_| if moved_from { m.move_from_mark } else { m.delete_mark });
        if del_mark == Some(DeleteMark::Caret) {
            // A caret where the text was, instead of the text.
            if let Some(cx) = line.cl_left(run_start).map(|c| x + c) {
                let h = st.size * 0.3;
                let w = (st.size / 18.0).max(0.6);
                out.push(Draw::Line {
                    x0: cx - h * 0.6,
                    y0: base + 1.0,
                    x1: cx,
                    y1: base + 1.0 - h,
                    width: w,
                    color,
                    stroke: Stroke::Solid,
                    alpha,
                    text_bg,
                });
                out.push(Draw::Line {
                    x0: cx,
                    y0: base + 1.0 - h,
                    x1: cx + h * 0.6,
                    y1: base + 1.0,
                    width: w,
                    color,
                    stroke: Stroke::Solid,
                    alpha,
                    text_bg,
                });
            }
            continue;
        }
        if del_mark == Some(DeleteMark::Hash) {
            // Each deleted character shows as #.
            let gid = st.face.glyph_for('#');
            for g in &mut glyphs {
                g.0 = gid;
            }
        }
        let synth_bold = st.synth_bold || ins_mark == Some(InsertMark::Bold) || del_mark == Some(DeleteMark::Bold);
        let synth_italic = st.synth_italic || ins_mark == Some(InsertMark::Italic) || del_mark == Some(DeleteMark::Italic);
        // Text effects draw the letters from their outlines; the glyphs stay, invisible, for
        // the PDF's text. Not for colour glyphs (no outlines), top-to-bottom text with upright
        // characters (their outlines would lie on their side), `#` deletion marks, or WordArt
        // in revision colours.
        let upright = down && text.chars().any(upright_in_vertical);
        let art =
            (!upright && !glyphs.is_empty() && del_mark != Some(DeleteMark::Hash) && (rc.text_effects.is_none() || ins.is_none() && del.is_none()))
                .then(|| crate::wordart::run_effects(rc, color))
                .flatten()
                .filter(|_| !st.face.has_color_glyphs());
        let has_art = art.is_some();
        if let Some(mut fx) = art {
            if synth_bold {
                crate::wordart::embolden(&mut fx, color, st.size);
            }
            let path = crate::wordart::glyph_path(&st.face, st.size, &glyphs, synth_italic);
            out.push(Draw::Art { path, fx: Box::new(fx), color, alpha, text_bg });
        }
        let look = RunLook { color, alpha: if has_art { 0.0 } else { alpha }, synth_bold, synth_italic, text_bg };
        draw_run_glyphs(ctx, st, (glyphs, text, ranges), dy, &look, out);
        draw_turned(ctx, st, &turned, dy, &look, out);
        let underline = match (ins_mark, del_mark) {
            (Some(InsertMark::Underline), _) | (_, Some(DeleteMark::Underline)) => Underline::Single,
            (Some(InsertMark::DoubleUnderline), _) | (_, Some(DeleteMark::DoubleUnderline)) => Underline::Double,
            _ => rc.underline,
        };
        let double = rc.double_strike || del_mark == Some(DeleteMark::DoubleStrikethrough);
        let struck = rc.strike || double || ins_mark == Some(InsertMark::Strikethrough) || del_mark == Some(DeleteMark::Strikethrough);
        let deco = Decoration { underline, struck, double, color, ucolor: rc.underline_color.unwrap_or(color), alpha, text_bg };
        decorations(ctx, st, (run_start, run_end), dy, &deco, out);
        emphasis_marks(ctx, st, (run_start, run_end), dy, color, out);
    }
}

/// How a run's glyphs are painted.
struct RunLook {
    color: Rgb,
    alpha: f32,
    synth_bold: bool,
    synth_italic: bool,
    text_bg: Option<Rgb>,
}

/// The run's glyphs. A vertical column: lying text sits on a baseline (ascent − descent) / 2
/// from the column's centre line (`base`), standing characters centre on it. Character scaling
/// stretches the glyphs about the run's first glyph: the run is drawn inside a `Draw::Scaled`
/// from its unscaled positions.
fn draw_run_glyphs(
    ctx: &LineContext<'_, '_, '_>,
    st: &crate::para::StyleRun,
    (mut glyphs, text, ranges): (Vec<(u32, f32, f32)>, String, Vec<std::ops::Range<usize>>),
    dy: f32,
    look: &RunLook,
    out: &mut Vec<Draw>,
) {
    if glyphs.is_empty() {
        return;
    }
    let (pl, x, base) = (ctx.pl, ctx.x, ctx.base);
    let rc = &st.rc;
    let lying = if pl.vertical { (st.ascent - st.descent) / 2.0 } else { 0.0 };
    for g in &mut glyphs {
        g.2 += lying;
    }
    let sx = rc.scale / 100.0;
    let scaled = (sx.is_finite() && (sx - 1.0).abs() > 1e-4 && sx > 0.0).then(|| glyphs.first().map_or(x, |g| g.1));
    if let Some(x0) = scaled {
        for g in &mut glyphs {
            g.1 = x0 + (g.1 - x0) / sx;
        }
    }
    let item = Draw::Glyphs {
        face: st.face,
        size: st.size,
        glyphs,
        color: look.color,
        alpha: look.alpha,
        synth_bold: look.synth_bold,
        synth_italic: look.synth_italic,
        text,
        link: rc.link.clone(),
        ranges,
        text_bg: look.text_bg,
        // Character scaling is the `Draw::Scaled` below.
        x_scale: 1.0,
    };
    let mut items = Vec::new();
    let sink = if scaled.is_some() { &mut items } else { &mut *out };
    if pl.vertical {
        stand_up(item, Some(base - st.shift - dy - st.v_centre), st.named.map(|n| n.v_origin as f32), sink);
    } else {
        sink.push(item);
    }
    if let Some(x0) = scaled {
        out.push(Draw::Scaled { x: x0, sx, items });
    }
}

/// 縦中横: the run's glyphs centred in the cluster's box, turned a quarter anticlockwise about
/// its centre. A turned (`tbRl`) page turns them back upright, reading left to right across the
/// column; on a horizontal page they read bottom to top, as Word draws them.
fn draw_turned(ctx: &LineContext<'_, '_, '_>, st: &crate::para::StyleRun, turned: &[(usize, f32)], dy: f32, look: &RunLook, out: &mut Vec<Draw>) {
    let (pl, para, line, x, base) = (ctx.pl, ctx.para, ctx.line, ctx.x, ctx.base);
    for &(k, natural) in turned {
        let (Some(c), Some(left)) = (pl.clusters.get(k), line.cl_left(k)) else { continue };
        let cg = pl.glyphs.get(c.g0 as usize..c.g1 as usize).unwrap_or(&[]);
        if cg.is_empty() {
            continue;
        }
        let (text, shown) = match pl.shown.binary_search_by_key(&k, |(i, _, _)| *i).ok().and_then(|i| pl.shown.get(i)) {
            Some((_, t, r)) => (t.clone(), Some(r)),
            None => (para.and_then(|p| p.text.get(c.start..c.end)).unwrap_or("").to_string(), None),
        };
        let cx = x + left + (c.adv - natural) / 2.0;
        // The run's line box (ascent + descent) turns about its centre: on a vertical
        // column's centre line (`base`), else about the baseline's line box.
        let lying = if pl.vertical { (st.ascent - st.descent) / 2.0 } else { 0.0 };
        let cy = base - st.shift - dy + lying - (st.ascent - st.descent) / 2.0;
        let glyphs: Vec<(u32, f32, f32)> = cg.iter().map(|g| (g.gid, cx + g.dx, base - st.shift - dy + lying - g.dy)).collect();
        let ranges: Vec<std::ops::Range<usize>> = match shown.filter(|r| r.len() == cg.len()) {
            Some(r) => r.iter().map(|r| r.start.min(text.len())..r.end.min(text.len())).collect(),
            None => cg.iter().map(|_| 0..text.len()).collect(),
        };
        let item = Draw::Glyphs {
            face: st.face,
            size: st.size,
            glyphs,
            color: look.color,
            alpha: ctx.alpha,
            synth_bold: st.synth_bold,
            synth_italic: st.synth_italic,
            text,
            link: st.rc.link.clone(),
            ranges,
            text_bg: look.text_bg,
            x_scale: 1.0,
        };
        out.push(Draw::Rotated { cx: x + left + c.adv / 2.0, cy, spin: Spin::new(-90.0, false, false), items: vec![item] });
    }
}

/// A run's underline and strikethrough.
struct Decoration {
    underline: Underline,
    struck: bool,
    double: bool,
    color: Rgb,
    ucolor: Rgb,
    alpha: f32,
    text_bg: Option<Rgb>,
}

/// Decorations across the run `run_start..run_end` (an underline stops at the line's `tail`).
/// Right-to-left text inside a run can split it into several visual spans.
fn decorations(
    ctx: &LineContext<'_, '_, '_>,
    st: &crate::para::StyleRun,
    (run_start, run_end): (usize, usize),
    dy: f32,
    d: &Decoration,
    out: &mut Vec<Draw>,
) {
    let (pl, para, line, x, base) = (ctx.pl, ctx.para, ctx.line, ctx.x, ctx.base);
    let end_k = run_end.min(ctx.tail).max(run_start);
    let clip = (end_k > ctx.content_tail).then_some(line.right);
    let spans = if d.underline != Underline::None || d.struck { line.spans(run_start, end_k) } else { Vec::new() };
    if spans.is_empty() {
        return;
    }
    let (alpha, text_bg) = (d.alpha, d.text_bg);
    // One line for the whole format run (the clusters around this one with the same
    // format, whatever faces draw them), at the position Word gives it.
    let same_rc = |k: usize| {
        pl.clusters.get(k).and_then(|c| pl.styles.get(c.style as usize)).is_some_and(|s| std::sync::Arc::ptr_eq(&s.rc, &st.rc) || *s.rc == *st.rc)
    };
    let mut fr0 = run_start;
    while fr0 > line.c0 && same_rc(fr0 - 1) {
        fr0 -= 1;
    }
    let mut fr1 = run_end;
    while fr1 < line.c1 && same_rc(fr1) {
        fr1 += 1;
    }
    let geo = decoration_geometry(pl, para, line, fr0, fr1, base - st.shift - dy);
    for (x0, x1) in spans.iter().map(|(a, b)| (x + a, x + clip.map_or(*b, |r| b.min(r)))).filter(|(a, b)| b > a) {
        if d.underline == Underline::Words {
            // Each word under its own line, where the face drawing it puts it (in one
            // run Word draws a Latin word's line where its font says and a Japanese
            // word's where the Japanese font's rule does).
            let in_span = |kk: usize| -> Option<(f32, f32)> {
                let c = pl.clusters.get(kk)?;
                if c.kind != ClKind::Text {
                    return None;
                }
                let a = x + line.cl_left(kk)?;
                let b = x + line.cl_right(kk)?;
                // Outside this span: drawn with its own.
                (a >= x0 - 0.01 && b <= x1 + 0.01 && b > a).then_some((a, b))
            };
            let mut kk = run_start;
            while kk < end_k {
                let Some((a, mut b)) = in_span(kk) else {
                    kk += 1;
                    continue;
                };
                // The word: the letters up to the next space or the span's end.
                let k0 = kk;
                kk += 1;
                while let Some((_, b2)) = in_span(kk).filter(|_| kk < end_k) {
                    b = b.max(b2);
                    kk += 1;
                }
                let g = decoration_geometry(pl, para, line, k0, kk, base - st.shift - dy);
                underline_draws(Underline::Single, a, b, &g, d.ucolor, alpha, text_bg, out);
            }
        } else if d.underline != Underline::None {
            underline_draws(d.underline, x0, x1, &geo, d.ucolor, alpha, text_bg, out);
        }
        if d.struck {
            strike_draws(d.double, x0, x1, &geo, d.color, alpha, text_bg, out);
        }
    }
}

/// Emphasis marks (傍点): one mark over (or under) each character, in the text's colour, where
/// Word for Mac draws them (ink measured at 8–36 pt with 游明朝, in ems of the text): above the
/// baseline a dot's centre 1.075 em, a comma's 1.03 em (0.05 em right of the character's
/// centre), a circle's 0.974 em; an under dot's 0.31 em below. In a vertical column, from the
/// column's centre line towards the page's right: a dot 0.705 em, a comma 0.695 em, a circle
/// 0.97 em (0.13 em towards the text's end); an under dot 0.585 em towards the left.
fn emphasis_marks(
    ctx: &LineContext<'_, '_, '_>,
    st: &crate::para::StyleRun,
    (run_start, run_end): (usize, usize),
    dy: f32,
    color: Rgb,
    out: &mut Vec<Draw>,
) {
    use wordcraft_doc::props::EmphasisMark;
    let (pl, line, x, base) = (ctx.pl, ctx.line, ctx.x, ctx.base);
    let em = st.rc.em;
    if em == EmphasisMark::None {
        return;
    }
    let s = st.size.clamp(1.0, 1638.0);
    // In a column the marks sit beside the standing em squares, which centre
    // `v_centre` toward the ascent side of the centre line.
    let y = base - st.shift - dy - if pl.vertical { st.v_centre } else { 0.0 };
    for kk in run_start..run_end {
        let (Some(c), Some(a), Some(b)) = (pl.clusters.get(kk), line.cl_left(kk), line.cl_right(kk)) else { continue };
        if c.kind != ClKind::Text || b <= a {
            continue;
        }
        let u = x + (a + b) / 2.0;
        let (mx, my) = match (pl.vertical, em) {
            (true, EmphasisMark::Dot) => (u, y - 0.705 * s),
            (true, EmphasisMark::Comma) => (u, y - 0.695 * s),
            (true, EmphasisMark::Circle) => (u + 0.13 * s, y - 0.97 * s),
            (true, _) => (u, y + 0.585 * s),
            (false, EmphasisMark::Dot) => (u, y - 1.075 * s),
            (false, EmphasisMark::Comma) => (u + 0.05 * s, y - 1.025 * s),
            (false, EmphasisMark::Circle) => (u, y - 0.955 * s),
            (false, _) => (u, y + 0.31 * s),
        };
        out.extend(emphasis_mark(em, mx, my, s, color, pl.vertical));
    }
}

fn comment_backgrounds(ctx: &LineContext<'_, '_, '_>, out: &mut Vec<Draw>) {
    let Some(para) = ctx.para.filter(|_| ctx.opts.markup && ctx.opts.revisions.comments) else { return };
    let mut open: Option<usize> = None;
    let mut ranges = Vec::new();
    for off in para.object_offsets() {
        match para.object_at(off) {
            Some(InlineObject::CommentStart { .. }) => open = open.or(Some(off)),
            Some(InlineObject::CommentEnd { .. }) => ranges.push((open.take().unwrap_or(0), off)),
            _ => {}
        }
    }
    if let Some(start) = open {
        ranges.push((start, para.len()));
    }
    for (start, end) in ranges {
        if end <= ctx.line.start || start >= ctx.line.stop {
            continue;
        }
        for (x0, x1) in ctx.pl.x_spans(ctx.li, start.max(ctx.line.start), end.min(ctx.line.stop)) {
            if x1 > x0 {
                out.push(Draw::Fill {
                    rect: Rect::new(ctx.x + x0, ctx.top, x1 - x0, ctx.line.height),
                    color: Rgb(0xEF, 0xE3, 0xF7),
                    alpha: ctx.alpha,
                    text_bg: false,
                });
            }
        }
    }
}

/// Backgrounds first: a highlight at the line's height; character shading on the run's own line —
/// its ascent and descent at its baseline, in a vertical column the line's height centred on the
/// em squares' centre line — as Word for Mac paints it (`cb1`/`cbv1` probes, 2026-10-19);
/// character borders group equal adjacent borders into one box around that line (the tallest
/// run's), drawn outside it and its distance from the text, in the room the clusters keep for it
/// (`para::border_room`).
fn character_backgrounds(ctx: &LineContext<'_, '_, '_>, out: &mut Vec<Draw>) {
    let mut open: Option<(Border, f32, f32, f32, f32)> = None;
    for k in ctx.line.c0..ctx.line.c1 {
        let (Some(cluster), Some(left), Some(right)) = (ctx.pl.clusters.get(k), ctx.line.cl_left(k), ctx.line.cl_right(k)) else { continue };
        let Some(style) = ctx.pl.styles.get(cluster.style as usize) else { continue };
        if cluster.kind == ClKind::Marker {
            continue;
        }
        let (rt, rb) = run_box(ctx.pl, style, ctx.base);
        if let Some(color) = style.rc.highlight {
            out.push(Draw::Fill {
                rect: Rect::new(ctx.x + left, ctx.top, (right - left).max(0.0), ctx.line.height),
                color,
                alpha: ctx.alpha,
                text_bg: true,
            });
        } else if let Some(color) = style.rc.shading {
            out.push(Draw::Fill {
                rect: Rect::new(ctx.x + left, rt, (right - left).max(0.0), (rb - rt).max(0.0)),
                color,
                alpha: ctx.alpha,
                text_bg: true,
            });
        }
        let border = style.rc.border;
        if let Some((open_border, x0, x1, t, btm)) = open.as_mut()
            && Some(*open_border) == border
        {
            // Right-to-left text grows leftwards.
            *x0 = x0.min(ctx.x + left);
            *x1 = x1.max(ctx.x + right);
            *t = t.min(rt);
            *btm = btm.max(rb);
            continue;
        }
        if let Some((previous, x0, x1, t, btm)) = open.take() {
            char_box(&previous, x0, x1, t, btm, ctx.alpha, out);
        }
        open = border.map(|border| (border, ctx.x + left, ctx.x + right, rt, rb));
    }
    if let Some((border, x0, x1, t, btm)) = open {
        char_box(&border, x0, x1, t, btm, ctx.alpha, out);
    }
}

fn draw_label_and_hyphen(ctx: &LineContext<'_, '_, '_>, out: &mut Vec<Draw>) {
    if ctx.li == ctx.pl.label_line()
        && let Some(label) = &ctx.pl.label
    {
        let gx = ctx.x + label.x;
        for run in &label.runs {
            let Some(style) = ctx.pl.styles.get(run.style as usize) else { continue };
            out.push(Draw::Glyphs {
                face: style.face,
                size: style.size,
                glyphs: label
                    .glyphs
                    .get(run.glyphs.clone())
                    .into_iter()
                    .flatten()
                    .map(|g| (g.gid, gx + g.dx, ctx.base - style.shift - g.dy))
                    .collect(),
                color: ctx.ink(&style.rc.color, None),
                alpha: ctx.alpha,
                synth_bold: style.synth_bold,
                synth_italic: style.synth_italic,
                text: run.text.clone(),
                link: None,
                ranges: Vec::new(),
                text_bg: None,
                x_scale: 1.0,
            });
        }
    }
    if let Some((style_index, glyph, advance)) = ctx.line.hyphen
        && let Some(style) = ctx.pl.styles.get(style_index as usize)
    {
        let hx = ctx.x + ctx.line.hyphen_x(advance);
        out.push(Draw::Glyphs {
            face: style.face,
            size: style.size,
            glyphs: vec![(glyph, hx, ctx.base - style.shift)],
            color: ctx.ink(&style.rc.color, None),
            alpha: ctx.alpha,
            synth_bold: style.synth_bold,
            synth_italic: style.synth_italic,
            text: "-".into(),
            link: None,
            ranges: Vec::new(),
            text_bg: None,
            x_scale: style.rc.scale / 100.0,
        });
    }
}

fn formatting_marks(ctx: &LineContext<'_, '_, '_>, out: &mut Vec<Draw>) {
    let (doc, story, path, para, pl, line, x, base, opts, authors, alpha) =
        (ctx.doc, ctx.story, ctx.path, ctx.para, ctx.pl, ctx.line, ctx.x, ctx.base, ctx.opts, ctx.authors, ctx.alpha);
    let marks = opts.always.shown(opts.marks);
    if marks.any() {
        let msize = pl.styles.first().map(|s| s.size).unwrap_or(11.0);
        for k in line.c0..line.c1 {
            let Some(c) = pl.clusters.get(k) else { continue };
            let cx = x + line.cl_left(k).unwrap_or(0.0);
            let nx = x + line.cl_right(k).unwrap_or(cx - x);
            let size = pl.styles.get(c.style as usize).map(|s| s.size).unwrap_or(msize);
            match c.kind {
                ClKind::Space if marks.spaces && is_ideographic_space(pl, c) => {
                    if let Some(st) = pl.styles.get(c.style as usize) {
                        let centre = if pl.vertical { base - st.v_centre } else { base - (ideographic_box_top(&st.face) - 0.5) * size };
                        out.push(ideographic_space_mark((cx + nx) / 2.0, centre, size));
                    }
                }
                ClKind::Space if marks.spaces => {
                    out.push(Draw::Mark { x: (cx + nx) / 2.0 - size * 0.12, baseline: base - size * 0.08, size, ch: '·', color: None })
                }
                ClKind::Tab if marks.tabs => out.push(tab_mark(cx, nx, base, size, line.rtl)),
                ClKind::LineBreak if marks.paragraphs && opts.japanese_marks => out.push(line_break_mark(cx + 1.0, base, size)),
                ClKind::LineBreak if marks.paragraphs => out.push(Draw::Mark { x: cx + 1.0, baseline: base, size, ch: '↵', color: None }),
                ClKind::PageBreak | ClKind::ColumnBreak if marks.paragraphs => {
                    // The paragraph mark after a page break shares the break's row: the
                    // rule stops under it (Word for Mac: 2.75 pt into the mark's box).
                    let mark_row = pl.lines.get(ctx.li + 1).filter(|m| m.beside && m.c0 == m.c1 && m.end == LineEnd::Para);
                    let (x0, x1) = if line.rtl {
                        (mark_row.map_or(x + line.left, |m| x + m.xs.first().copied().unwrap_or(line.left) - 2.75), cx - 1.0)
                    } else {
                        (cx + 1.0, mark_row.map_or(x + line.right, |m| x + m.xs.first().copied().unwrap_or(line.right) + 2.75))
                    };
                    let (rule, label) =
                        if c.kind == ClKind::PageBreak { (BreakRule::Page, "Page Break") } else { (BreakRule::Column, "Column Break") };
                    break_mark(rule, (x0, x1), base, size, label, alpha, out);
                }
                _ => {}
            }
        }
        // A deleted paragraph mark that isn't shown (no markup, or its mark is Hidden) draws no ¶.
        let mark_gone =
            para.is_some_and(|p| p.mark.del.is_some() && (!opts.markup || opts.revisions.hides_deletion(doc.move_name(p.mark.del).is_some())));
        if line.end == LineEnd::Para && marks.paragraphs && !mark_gone {
            // The mark follows the line's visual end, past every glyph, even where the
            // logically last text runs against the paragraph's direction.
            let ex = x + line.visual_end_x();
            let size = pl.lines.first().map(|_| msize).unwrap_or(msize);
            // A right-to-left paragraph's mark sits at its end, on the left.
            let mark_w = if opts.japanese_marks { RETURN_MARK_W } else { size * 0.6 };
            let mx = if line.rtl { ex - 1.0 - mark_w } else { ex + 1.0 };
            // A tracked (inserted or deleted) paragraph mark is drawn in its author's colour.
            let m = &opts.revisions;
            let by_author = |r: u32| authors.color(r);
            let color = para.filter(|_| opts.markup && m.insertions_deletions).and_then(|p| match (p.mark.ins, p.mark.del) {
                (_, Some(r)) if doc.move_name(Some(r)).is_some() => Some(m.move_from_color.unwrap_or_else(|| by_author(r))),
                (Some(r), None) if doc.move_name(Some(r)).is_some() => Some(m.move_to_color.unwrap_or_else(|| by_author(r))),
                (_, Some(r)) => Some(m.delete_color.unwrap_or_else(|| by_author(r))),
                (Some(r), None) => Some(m.insert_color.unwrap_or_else(|| by_author(r))),
                _ => None,
            });
            // The last paragraph of a section ends in a section break instead of a plain ¶.
            if let Some(start) = section_break_after(doc, story, path) {
                let (x0, x1) = if line.rtl { (x + line.left, ex - 1.0) } else { (ex + 1.0, x + line.right) };
                break_mark(BreakRule::Section, (x0, x1), base, size, section_break_label(start), alpha, out);
            } else if opts.japanese_marks {
                out.push(return_mark(mx, base, color));
            } else {
                out.push(Draw::Mark { x: mx, baseline: base, size, ch: '¶', color });
            }
        }
    }
}

fn draw_rubies(ctx: &LineContext<'_, '_, '_>, out: &mut Vec<Draw>) {
    let (pl, line, opts, x, base, alpha) = (ctx.pl, ctx.line, ctx.opts, ctx.x, ctx.base, ctx.alpha);
    for r in pl.rubies.iter().filter(|r| (line.c0..line.c1).contains(&r.cluster)) {
        let Some(bst) = pl.clusters.get(r.cluster).and_then(|c| pl.styles.get(c.style as usize)) else { continue };
        if bst.rc.del.is_some() && !opts.markup {
            continue;
        }
        let cx = x + line.cl_left(r.cluster).unwrap_or(0.0);
        // The ruby's baseline `raise` above the base text's — in a vertical column, the
        // ruby's centre line that far to the right of the base's em squares' centre.
        let y = base - bst.shift - pl.align_dy.get(r.cluster).copied().unwrap_or(0.0) - r.raise - if pl.vertical { bst.v_centre } else { 0.0 };
        let mut i = 0;
        while let Some(&(style, _)) = r.glyphs.get(i) {
            let n = r.glyphs.get(i..).map_or(0, |g| g.iter().take_while(|(s, _)| *s == style).count()).max(1);
            let run = r.glyphs.get(i..i + n).unwrap_or(&[]);
            let ranges = r.ranges.get(i..i + n).map(<[_]>::to_vec).unwrap_or_default();
            i += n;
            let Some(st) = pl.styles.get(style as usize) else { continue };
            let lying = if pl.vertical { (st.ascent - st.descent) / 2.0 } else { 0.0 };
            let item = Draw::Glyphs {
                face: st.face,
                size: st.size,
                glyphs: run.iter().map(|(_, g)| (g.gid, cx + g.dx, y + lying - g.dy)).collect(),
                color: ctx.ink(&st.rc.color, st.rc.shading.or(st.rc.highlight)),
                alpha,
                synth_bold: st.synth_bold,
                synth_italic: st.synth_italic,
                text: r.text.clone(),
                link: None,
                ranges,
                text_bg: st.rc.highlight.or(st.rc.shading),
                x_scale: 1.0,
            };
            if pl.vertical {
                stand_up(item, Some(y - st.v_centre), st.named.map(|n| n.v_origin as f32), out);
            } else {
                out.push(item);
            }
        }
    }
}

fn inline_objects(ctx: &LineContext<'_, '_, '_>, out: &mut Vec<Draw>) {
    let (doc, story, path, pl, para, line, x, base, opts, alpha) =
        (ctx.doc, ctx.story, ctx.path, ctx.pl, ctx.para, ctx.line, ctx.x, ctx.base, ctx.opts, ctx.alpha);
    for k in line.c0..line.c1 {
        let Some(c) = pl.clusters.get(k) else { continue };
        let ClKind::Object(oi) = c.kind else { continue };
        if c.obj_h <= 0.0 {
            continue;
        }
        // A dragged inline object is left out too: its preview stands in for it.
        if opts.hide_object.as_ref().is_some_and(|p| p.story == story && p.path == *path && p.off == c.start) {
            continue;
        }
        let cx = x + line.cl_left(k).unwrap_or(0.0);
        let obj = para.and_then(|p| p.objects.get(oi));
        let rect = inline_rect(obj, cx, base, c.adv, c.obj_h);
        let spin = obj.and_then(InlineObject::frame).map(|(_, _, f)| f.spin()).unwrap_or_default();
        match obj {
            Some(o @ (InlineObject::Image { .. } | InlineObject::Shape { .. } | InlineObject::Group { .. })) => {
                object_draws(o, rect, Spin::default(), alpha, opts, out)
            }
            Some(InlineObject::Graphic { graphic, alt, .. }) => {
                let mut draws = Vec::new();
                spun(spin, rect, graphic_draws(doc, graphic, rect, alpha), &mut draws);
                out.push(Draw::Figure { alt: alt.clone(), kind: Some(graphic.kind), draws })
            }
            Some(InlineObject::Equation { .. }) => {
                if let Some((_, ml)) = pl.maths.iter().find(|(k, _)| *k == oi) {
                    equation(&ml.items, cx, base, alpha, opts.placeholders, out);
                }
            }
            Some(o @ InlineObject::Field { .. }) => {
                if let Some(wordcraft_doc::legacy_form::LegacyForm::CheckBox { checked, .. }) = o.legacy_form() {
                    let color = pl.styles.get(c.style as usize).map_or(Rgb::BLACK, |st| text_color(&st.rc.color, None));
                    let frame = Rect::new(cx, base - c.obj_h, c.adv, c.obj_h + c.obj_d);
                    check_box(frame, checked, color, alpha, out);
                }
            }
            _ => {}
        }
    }
}

fn proofing_marks(ctx: &LineContext<'_, '_, '_>, out: &mut Vec<Draw>) {
    for (start, end, grammar) in &ctx.pl.issues {
        if *end <= ctx.line.start || *start >= ctx.line.stop {
            continue;
        }
        for (x0, x1) in ctx.pl.x_spans(ctx.li, (*start).max(ctx.line.start), (*end).min(ctx.line.stop)) {
            if x1 - x0 < 1.0 {
                continue;
            }
            let y = ctx.base + 2.5;
            let color = if *grammar { Rgb(0x2B, 0x57, 0xC0) } else { Rgb(0xE0, 0x24, 0x24) };
            out.push(Draw::Line {
                x0: ctx.x + x0,
                y0: y,
                x1: ctx.x + x1,
                y1: y,
                width: 0.8,
                color,
                stroke: if *grammar { Stroke::Double } else { Stroke::Wave },
                alpha: ctx.alpha,
                text_bg: None,
            });
        }
    }
}

fn tab_leaders(ctx: &LineContext<'_, '_, '_>, out: &mut Vec<Draw>) {
    for (k, leader) in &ctx.line.leaders {
        let (Some(cluster), Some(left), Some(right)) = (ctx.pl.clusters.get(*k), ctx.line.cl_left(*k), ctx.line.cl_right(*k)) else { continue };
        let Some(style) = ctx.pl.styles.get(cluster.style as usize) else { continue };
        let Some(ch) = leader.char() else { continue };
        let gid = style.face.glyph_for(ch);
        let advance = (style.face.advance(gid) as f32 * style.size / style.face.upem.max(1.0) as f32).max(1.0);
        // The leader characters sit on a grid of their advance from the page's left edge, so
        // consecutive lines line up: the first at the grid position at or after the text's
        // end, the last the one that ends at or before the tab's text (Word for Mac, `tl1`
        // probe, 2026-10-19: Aptos dots 2.999 pt apart from 89.98 after `abc` ends at 89.02,
        // hyphens 3.574 apart from 89.34, underscores 4.829 from 91.76; the last dot ends
        // exactly where a right-aligned `7` starts and 0.39 pt before `12`).
        let (start, end) = (ctx.x + left, ctx.x + right);
        let mut gx = (start / advance - 1e-3).ceil() * advance;
        let mut glyphs = Vec::new();
        while gx + advance <= end + 1e-3 && glyphs.len() < 2000 {
            glyphs.push((gid, gx, ctx.base));
            gx += advance;
        }
        out.push(Draw::Glyphs {
            face: style.face,
            size: style.size,
            glyphs,
            color: ctx.ink(&style.rc.color, None),
            alpha: ctx.alpha,
            synth_bold: false,
            synth_italic: false,
            text: String::new(),
            link: None,
            ranges: Vec::new(),
            text_bg: None,
            x_scale: 1.0,
        });
    }
}

fn enclosures(ctx: &LineContext<'_, '_, '_>, out: &mut Vec<Draw>) {
    for (k, enclosure) in &ctx.pl.enclosures {
        if *k < ctx.line.c0 || *k >= ctx.line.c1 {
            continue;
        }
        let (Some(cluster), Some(left)) = (ctx.pl.clusters.get(*k), ctx.line.cl_left(*k)) else { continue };
        let Some(style) = ctx.pl.styles.get(cluster.style as usize) else { continue };
        let dy = ctx.pl.align_dy.get(*k).copied().unwrap_or(0.0);
        let rect = Rect::new(ctx.x + left + enclosure.x, ctx.base - style.shift - dy - enclosure.top, enclosure.w, enclosure.h);
        out.push(Draw::Shape {
            rect,
            kind: enclosure.kind,
            fill: None,
            stroke: Some(ctx.ink(&style.rc.color, None)),
            stroke_width: enclosure.stroke,
            effects: Default::default(),
            corner: None,
            dash: Vec::new(),
            cap: Default::default(),
        });
    }
}

/// The kind of section break that ends at body paragraph `path`: the start type of the section
/// that follows it. `None` for any other paragraph, and for the document's last section.
fn section_break_after(doc: &Document, story: StoryRef, path: &Path) -> Option<SectionStart> {
    if story != StoryRef::Body || path.0.len() != 1 {
        return None;
    }
    let i = *path.0.first()? as usize;
    match doc.body.get(i).map(|b| &**b) {
        Some(Block::Para(p)) if p.section.is_some() => {}
        _ => return None,
    }
    let secs = doc.sections();
    let k = secs.iter().position(|(end, _)| *end == i)?;
    secs.get(k + 1).map(|(_, s)| s.start)
}

/// On-screen name of a section break.
pub fn section_break_label(start: SectionStart) -> &'static str {
    match start {
        SectionStart::NextPage => "Section Break (Next Page)",
        SectionStart::Continuous => "Section Break (Continuous)",
        SectionStart::EvenPage => "Section Break (Even Page)",
        SectionStart::OddPage => "Section Break (Odd Page)",
        SectionStart::NextColumn => "Section Break (Next Column)",
    }
}

/// Width of a [`Draw::MarkText`] label.
pub fn mark_text_width(text: &str, size: f32) -> f32 {
    let face = wordcraft_fonts::word::resolve("Source Sans 3", false, false).face;
    let k = size / face.upem.max(1.0) as f32;
    text.chars().map(|c| face.advance(face.glyph_for(c)) as f32 * k).sum()
}

/// The rule of a page, column or section break mark, as Word for Mac draws them (`bk1`/`bk2`
/// probes, 10.5-pt Yu Mincho, 2026-10-19): a page break's is a 2-pt band hanging from the
/// baseline, a checkerboard of 1-pt cells, dark (#373737) and grey (#C7C7C7); a column break's
/// two rows of 1-pt dark dots 4 pt apart, the lower row 2 pt behind the upper; a section
/// break's the page break's band and, 2 pt below it, one more row of the checkerboard.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BreakRule {
    Page,
    Column,
    Section,
}

const BREAK_DOT: Rgb = Rgb(0x37, 0x37, 0x37);
const BREAK_CELL: Rgb = Rgb(0xC7, 0xC7, 0xC7);

/// `rows` rows of Word's checkerboard from `top` down between `x0` and `x1`.
fn checker_rows(x0: f32, x1: f32, top: f32, rows: usize, alpha: f32, out: &mut Vec<Draw>) {
    if !(x0.is_finite() && x1.is_finite() && top.is_finite()) || x1 - x0 < 1.0 {
        return;
    }
    out.push(Draw::Fill { rect: Rect::new(x0, top, x1 - x0, rows as f32), color: BREAK_CELL, alpha, text_bg: false });
    for r in 0..rows {
        let y = top + r as f32 + 0.5;
        let sx = x0 + (r % 2) as f32;
        let stroke = Stroke::Dashes { on: 1.0, off: 1.0, dots: 0 };
        out.push(Draw::Line { x0: sx, y0: y, x1, y1: y, width: 1.0, color: BREAK_DOT, stroke, alpha, text_bg: None });
    }
}

/// The rule of `rule` between `x0` and `x1`, hanging from `base`.
fn break_rule(rule: BreakRule, x0: f32, x1: f32, base: f32, alpha: f32, out: &mut Vec<Draw>) {
    match rule {
        BreakRule::Page => checker_rows(x0, x1, base, 2, alpha, out),
        BreakRule::Section => {
            checker_rows(x0, x1, base, 2, alpha, out);
            checker_rows(x0, x1, base + 4.0, 1, alpha, out);
        }
        BreakRule::Column => {
            if !(x0.is_finite() && x1.is_finite() && base.is_finite()) || x1 - x0 < 1.0 {
                return;
            }
            for (r, shift) in [(0.0, 0.0), (1.0, 2.0)] {
                let y = base + r + 0.5;
                let stroke = Stroke::Dashes { on: 1.0, off: 3.0, dots: 0 };
                out.push(Draw::Line { x0: x0 + shift, y0: y, x1, y1: y, width: 1.0, color: BREAK_DOT, stroke, alpha, text_bg: None });
            }
        }
    }
}

/// A break mark between `x0` and `x1`: its rule with the break's name centred in it on the
/// baseline (just the name when there is no room for the rule).
pub fn break_mark(rule: BreakRule, (x0, x1): (f32, f32), base: f32, size: f32, text: &str, alpha: f32, out: &mut Vec<Draw>) {
    let lsize = (size * 0.8).clamp(6.0, 11.0);
    let w = mark_text_width(text, lsize);
    let gap = lsize * 0.4;
    if x1 - x0 <= w + 4.0 * gap {
        out.push(Draw::MarkText { x: x0, baseline: base, size: lsize, text: text.into() });
        return;
    }
    let tx = (x0 + x1 - w) / 2.0;
    for (a, b) in [(x0, tx - gap), (tx + w + gap, x1)] {
        break_rule(rule, a, b, base, alpha, out);
    }
    out.push(Draw::MarkText { x: tx, baseline: base, size: lsize, text: text.into() });
}

/// An equation's glyphs and rules with its origin at (`x`, `base`).
fn equation(items: &[MItem], x: f32, base: f32, alpha: f32, screen: bool, out: &mut Vec<Draw>) {
    for it in items {
        match it {
            MItem::Glyphs { face, size, color, synth_bold, synth_italic, glyphs, text } => out.push(Draw::Glyphs {
                face: *face,
                size: *size,
                glyphs: glyphs.iter().map(|(g, gx, gy)| (*g, x + gx, base - gy)).collect(),
                color: *color,
                alpha,
                synth_bold: *synth_bold,
                synth_italic: *synth_italic,
                text: text.clone(),
                link: None,
                // Equation text maps back through the font (no per-glyph ranges from the math layout).
                ranges: Vec::new(),
                text_bg: None,
                x_scale: 1.0,
            }),
            MItem::Rect { x: rx, y, w, h, color } => {
                out.push(Draw::Fill { rect: Rect::new(x + rx, base - y - h, *w, *h), color: *color, alpha, text_bg: false })
            }
            MItem::Line { x0, y0, x1, y1, width, color, dotted } => out.push(Draw::Line {
                x0: x + x0,
                y0: base - y0,
                x1: x + x1,
                y1: base - y1,
                width: *width,
                color: *color,
                stroke: if *dotted { Stroke::Dotted } else { Stroke::Solid },
                alpha,
                text_bg: None,
            }),
            MItem::ScreenOnly(inner) => {
                if screen {
                    equation(std::slice::from_ref(inner), x, base, alpha, screen, out);
                }
            }
        }
    }
}

/// Word's tab mark (→) as Word for Mac draws it (12–72 pt, `sp4` probe, 2026-10-19): an arrow
/// 0.6 em long and 0.46 em tall centred in the tab's cell from `cx` to `nx`, its shaft 0.083 em
/// thick to the tip and an open head whose two strokes reach 0.28 em back from the tip; in a
/// narrow cell the arrow shrinks to the cell's width less 3.5 pt (a 48-pt tab in an 18-pt cell
/// is 14.5 pt long: its head alone), and below twice its height it shrinks in height too (a
/// 72-pt tab in a 6-pt cell is a 2 × 4 pt head). In a right-to-left line it points left.
pub fn tab_mark(cx: f32, nx: f32, base: f32, size: f32, rtl: bool) -> Draw {
    let size = if size.is_finite() { size.clamp(1.0, 1638.0) } else { 11.0 };
    let (cx, nx) = (if cx.is_finite() { cx } else { 0.0 }, if nx.is_finite() { nx } else { 0.0 });
    let (left, right) = (cx.min(nx), cx.max(nx));
    let cell = right - left;
    let w = (size * 0.6).min(cell - 3.5).max(1.0);
    let h = (size * 0.46).min(2.0 * w);
    let head = (size * 0.28).min(w);
    let stroke = (size * 0.083).min(h / 2.0).max(0.1);
    let mid = (left + right) / 2.0;
    let (tail, tip) = if rtl { (mid + w / 2.0, mid - w / 2.0) } else { (mid - w / 2.0, mid + w / 2.0) };
    let back = if rtl { tip + head } else { tip - head };
    let y = base - h / 2.0;
    let lines = vec![vec![(tail, y), (tip, y)], vec![(back, y - h / 2.0), (tip, y), (back, y + h / 2.0)]];
    Draw::MarkLines { lines, fills: Vec::new(), width: stroke, color: None }
}

/// The width and height of Word's Japanese paragraph mark, points (6.5 × 6 pt at every size).
pub const RETURN_MARK_W: f32 = 6.5;
pub const RETURN_MARK_H: f32 = 6.0;

/// Word's paragraph mark with Japanese editing (段落記号 ↵) as Word for Mac draws it at 12, 24, 48
/// and 72 pt alike (2026-10-19): a fixed 6.5 × 6 pt hooked arrow in 1-pt strokes standing on
/// the baseline — the stem down the right edge from the top to the bar 2.75 pt above the
/// baseline, the bar leftwards to the tip, and the head's two diagonals 2.25 pt long. `x` is
/// its left edge; `color` a tracked paragraph mark's reviser colour.
pub fn return_mark(x: f32, base: f32, color: Option<Rgb>) -> Draw {
    let (x, base) = (if x.is_finite() { x } else { 0.0 }, if base.is_finite() { base } else { 0.0 });
    let top = base - RETURN_MARK_H;
    let bar = base - 2.75;
    let lines = vec![
        vec![(x + 6.0, top), (x + 6.0, bar)],
        vec![(x + 0.25, bar), (x + 6.0, bar)],
        vec![(x + 2.5, bar - 2.25), (x + 0.25, bar), (x + 2.5, bar + 2.25)],
    ];
    Draw::MarkLines { lines, fills: Vec::new(), width: 1.0, color }
}

/// Word's manual line break mark with Japanese editing (↓) as Word for Mac draws it (12–72 pt,
/// 2026-10-19): 0.4375 em wide and 0.708 em tall above the baseline, a stem 0.056 em thick down
/// to a filled triangular head 0.2 em tall. `x` is its left edge, `size` the text's size.
pub fn line_break_mark(x: f32, base: f32, size: f32) -> Draw {
    let size = if size.is_finite() { size.clamp(1.0, 1638.0) } else { 11.0 };
    let (x, base) = (if x.is_finite() { x } else { 0.0 }, if base.is_finite() { base } else { 0.0 });
    let cx = x + size * 0.21875;
    let head = base - size * 0.2;
    let lines = vec![vec![(cx, base - size * 0.708), (cx, head)]];
    let fills = vec![vec![(cx - size * 0.21875, head), (cx + size * 0.21875, head), (cx, base - size * 0.01)]];
    Draw::MarkLines { lines, fills, width: (size * 0.056).max(0.5), color: None }
}

/// The grey of Word for Mac's ideographic-space mark (read from its window, 2026-10-19).
pub const IDEOGRAPHIC_SPACE_MARK: Rgb = Rgb(0x6D, 0x6D, 0x6D);

/// The formatting mark of an ideographic space (全角スペース) as Word for Mac draws it with
/// Show/Hide ¶ on (24, 36 and 72 pt Yu Mincho, Yu Gothic and MS Mincho, 2026-10-19): a hollow
/// square 3 pt narrower than the em and 0.75 em + 1 pt tall, centred on the character's cell
/// (`cx`) and on the font's ideographic em box (`centre`), in a fixed grey whatever the text's
/// colour; in a 縦書き column it turns with the column. `size` is the font size in points.
pub fn ideographic_space_mark(cx: f32, centre: f32, size: f32) -> Draw {
    let size = if size.is_finite() { size.clamp(1.0, 1638.0) } else { 11.0 };
    let w = (size - 3.0).max(1.0);
    let h = size * 0.75 + 1.0;
    let (cx, centre) = (if cx.is_finite() { cx } else { 0.0 }, if centre.is_finite() { centre } else { 0.0 });
    Draw::MarkBox { rect: Rect::new(cx - w / 2.0, centre - h / 2.0, w, h), color: IDEOGRAPHIC_SPACE_MARK }
}

/// Whether space cluster `c` is an ideographic space (U+3000): its one glyph is the style's
/// face's glyph for it.
fn is_ideographic_space(pl: &ParaLayout, c: &crate::para::Cluster) -> bool {
    let Some(st) = pl.styles.get(c.style as usize) else { return false };
    let gid = st.face.glyph_for('\u{3000}');
    gid != 0 && c.g1 == c.g0.saturating_add(1) && pl.glyphs.get(c.g0 as usize).is_some_and(|g| g.gid == gid)
}

/// The top of a font's ideographic em box above the baseline, in ems (its vertical origin:
/// 0.88 for the Yu fonts and Hiragino, 0.8 for Songti); 0.88 where the font gives none.
fn ideographic_box_top(face: &wordcraft_fonts::FaceRef) -> f32 {
    let gid = face.glyph_for('\u{3000}');
    let gid = if gid == 0 { face.glyph_for('一') } else { gid };
    if gid == 0 {
        return 0.88;
    }
    let v = (face.vertical_origin(gid) / face.upem.max(1.0)) as f32;
    if v.is_finite() && (0.5..=1.2).contains(&v) { v } else { 0.88 }
}

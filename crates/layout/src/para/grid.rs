use super::builder::Builder;
use super::model::{ClKind, Cluster, StyleRun};
use super::{Arc, CharProps, Document, ParaLayout, WrapText};

/// The section's document grid as layout applies it (`w:docGrid`, ECMA-376 §17.6.5).
///
/// Measured in Word for Mac (10.5 pt 游明朝 and Aptos, 2026-10-19, `cg1`–`cg16` samples):
/// **lines and characters** (`linesAndChars`, pitch = default size + `w:charSpace`): an East
/// Asian character advances by its size plus the grid's gap (pitch − default size), its glyph at
/// the start of that (a 14 pt character on a 14 pt pitch over 10.5 takes 17.5 pt; nothing is
/// centred and nothing snaps back to the grid after Latin text); every Latin character and
/// space advances by its own width plus half the gap; a space next to East Asian text takes
/// half its size plus half the gap; the automatic space between East Asian and Latin letters
/// is a quarter of (the size of the character before the boundary + the gap); a compressed
/// mark takes half of (size + gap). **Snap to characters** (`snapToChars`): the text width is
/// divided into whole cells (`cell`); an East Asian character takes as many cells as its size
/// needs, centred; a run of Latin text takes whole cells, centred, and the line's leftover is
/// shared out between the units (see `distribute_cells`).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Grid {
    /// Line pitch, points: line heights snap to multiples of it.
    pub line: Option<f32>,
    /// Character pitch, points (the default size plus `w:charSpace`).
    pub chars: Option<f32>,
    /// The default font size the character pitch is based on, points.
    pub font_size: f32,
    /// `snapToChars`: all text snaps to the character grid, not just East Asian characters.
    pub snap_all: bool,
    /// `snapToChars`: the width of a cell, the text width divided into whole pitches.
    pub cell: Option<f32>,
}

impl Grid {
    /// The grid of section `s` of `doc` (its default font size from the Normal style).
    pub fn of(doc: &Document, s: &wordcraft_doc::SectionProps) -> Grid {
        let Some(line) = s.grid_line_pitch() else { return Grid::default() };
        let size = doc.styles.resolve_char(Some("Normal"), &CharProps::default()).size;
        let font_size = if size.is_finite() { size.clamp(1.0, 1584.0) } else { 10.5 };
        let chars = s.grid_char_pitch(font_size);
        let snap_all = s.doc_grid.is_some_and(|g| g.kind == wordcraft_doc::section::DocGridType::SnapToChars);
        let cell = if snap_all { s.grid_char_cell(font_size).filter(|c| c.is_finite() && *c > 0.0) } else { None };
        Grid { line: Some(line), chars, font_size, snap_all, cell }
    }
    /// A cache key for paragraph layouts.
    pub fn key(&self) -> (u32, u32, u32, bool, u32) {
        (
            self.line.map_or(0, f32::to_bits),
            self.chars.map_or(0, f32::to_bits),
            self.font_size.to_bits(),
            self.snap_all,
            self.cell.map_or(0, f32::to_bits),
        )
    }
    /// The gap a lines-and-characters grid adds to every East Asian character: the pitch less
    /// the default size (0 without a character grid or when snapping to characters).
    pub fn gap(&self) -> f32 {
        match (self.chars, self.cell) {
            (Some(pitch), None) => (pitch - self.font_size).max(-self.font_size),
            _ => 0.0,
        }
    }
    /// What a character pitch (`w:charSpace`, lines-and-characters or snap-to-characters) adds
    /// to the default size: the gap an indent in characters takes per character (Word for Mac,
    /// measured on a lines-and-characters grid; 0 without a character pitch).
    pub fn char_gap(&self) -> f32 {
        match self.chars {
            Some(pitch) if pitch.is_finite() => (pitch - self.font_size).clamp(-self.font_size, 1584.0),
            _ => 0.0,
        }
    }
    /// The advance of an East Asian character of `size` points on the grid: its size plus the
    /// gap (lines and characters), whole cells (snap to characters), its size (no grid).
    pub fn ea_advance(&self, size: f32) -> f32 {
        let size = if size.is_finite() { size.clamp(0.0, 1584.0) } else { 0.0 };
        match (self.chars, self.cell) {
            (Some(_), Some(cell)) => (size / cell - 0.001).ceil().clamp(1.0, 64.0) * cell,
            (Some(_), None) => (size + self.gap()).max(0.0),
            _ => size,
        }
    }
    /// `h` rounded up to whole line pitches (unchanged without a line grid).
    pub fn snap_height(&self, h: f32) -> f32 {
        match self.line {
            Some(p) if h.is_finite() => ((h / p - 0.001).ceil().clamp(1.0, 10_000.0) * p).min(1e6),
            _ => h,
        }
    }
}

/// East Asian characters the character grid places on its pitch: CJK ideographs, kana, Hangul,
/// CJK punctuation, full-width forms and half-width forms (ｱｲｳ: half a cell each in Word for
/// Mac, centred in a whole one when snapping to characters).
pub fn is_east_asian(c: char) -> bool {
    matches!(c as u32,
        0x1100..=0x11FF | 0x2E80..=0x2FDF | 0x2FF0..=0x303F | 0x3040..=0x30FF | 0x3100..=0x312F | 0x3130..=0x318F
        | 0x3190..=0x31FF | 0x3200..=0x4DBF | 0x4E00..=0x9FFF | 0xA960..=0xA97F | 0xAC00..=0xD7FF | 0xF900..=0xFAFF
        | 0xFE30..=0xFE4F | 0xFF01..=0xFF60 | 0xFF61..=0xFFDC | 0xFFE0..=0xFFE6 | 0xFFE8..=0xFFEE | 0x20000..=0x3FFFF)
}

/// Justification without a character grid, as Word for Mac spreads a line (`cp2` probes, Yu
/// Mincho 12 pt, no grid and a line grid alike): the leftover goes in shares, one between two
/// characters one of which is East Asian (a space next to an East Asian character takes a share
/// on each side: あいう えお puts 80.4 pt between あ and い and 160.8 around the space), one for
/// a space after a Latin word before another — plus a twelfth of its size when a Latin word
/// comes before the space — and none between two Latin words' letters (短い行 with Latin 終わり:
/// 41.1 pt between 短 and い, 82.2 after 行, 42.5 between with and Latin, 40.9 after Latin). A
/// Latin-only line puts the leftover in its spaces equally, as before. Word's own positions
/// wander by up to 1.5 pt from these shares (its rounding); the shares are the rule. Returns
/// what each content cluster's right side gains.
pub(super) fn justify_shares(pl: &ParaLayout, content: &[usize], slack: f32) -> Vec<f32> {
    let ea = |k: usize| pl.ea_flags.get(k).is_some_and(|f| f & EA_CHAR != 0);
    let space = |k: usize| pl.clusters.get(k).is_some_and(|c| c.kind == ClKind::Space);
    let size = |k: usize| pl.clusters.get(k).and_then(|c| pl.styles.get(c.style as usize)).map_or(0.0, |st| st.size).clamp(0.0, 1584.0);
    let mut shares = vec![0.0f32; content.len()];
    let mut bonus = vec![0.0f32; content.len()];
    for i in 0..content.len().saturating_sub(1) {
        let (Some(&a), Some(&b)) = (content.get(i), content.get(i + 1)) else { break };
        if ea(a) || ea(b) {
            shares[i] = 1.0;
        } else if space(a) {
            shares[i] = 1.0;
            if i > 0 && content.get(i - 1).is_some_and(|p| !ea(*p)) {
                bonus[i] = size(a) / 12.0;
            }
        }
    }
    let n: f32 = shares.iter().sum();
    let fixed: f32 = bonus.iter().sum();
    if n <= 0.0 || !slack.is_finite() {
        return vec![0.0; content.len()];
    }
    if fixed >= slack {
        // Not even the twelfths fit: the leftover goes in their proportion.
        return bonus.iter().map(|b| if fixed > 0.0 { slack * b / fixed } else { 0.0 }).collect();
    }
    let share = (slack - fixed) / n;
    shares.iter().zip(&bonus).map(|(s, b)| s * share + b).collect()
}

/// Character grid: East Asian characters (all text with `snapToChars`) take whole cells of the
/// pitch, the glyph centred in them. A character at the default size takes one cell, a larger one
/// as many as its size needs; Latin text stays proportional unless everything snaps.
/// 均等割り付け (`w:fitText`): consecutive clusters of one block (same id and width) are spread
/// to exactly the block's width — the surplus shared equally between the characters, none after
/// the last — or, when the text is wider, narrowed to it (advances only: the glyphs keep their
/// outlines, where Word also narrows them). Measured in Word for Mac at 10.5 pt: 均等割付 fit to
/// 100 pt has its characters 29.8 pt apart; abc fit to 60 pt has 21.5 pt gaps; 均等割付 fit to
/// 30 pt is drawn at 71%. The spacing Word stores in the block's runs (`w:spacing`) is what it
/// computed for the same spread, so it doesn't count. A line never breaks inside a block.
/// Word's character spacing (`w:spacing`) counts twice on an East Asian character: measured in
/// Word for Mac, 均等割付 expanded by 10 pt has 20 pt between its characters, abcd 10 pt.
pub(super) fn spacing_for(spacing: f32, ch: char) -> f32 {
    if wordcraft_doc::east_asian::is_east_asian(ch) { spacing * 2.0 } else { spacing }
}

pub(super) fn fit_text_blocks(b: &mut Builder, text: &str) {
    let fit_of = |b: &Builder, k: usize| b.clusters.get(k).and_then(|c| b.styles.get(c.style as usize)).and_then(|s| s.rc.fit_text);
    let mut i = 0;
    while i < b.clusters.len() {
        let Some(ft) = fit_of(b, i) else {
            i += 1;
            continue;
        };
        let mut j = i + 1;
        while j < b.clusters.len() && fit_of(b, j) == Some(ft) {
            j += 1;
        }
        let spacing = |b: &Builder, k: usize| {
            b.clusters
                .get(k)
                .and_then(|c| {
                    let s = b.styles.get(c.style as usize)?.rc.spacing;
                    let ch = text.get(c.start..).and_then(|t| t.chars().next()).unwrap_or(' ');
                    Some(spacing_for(s, ch))
                })
                .unwrap_or(0.0)
        };
        let natural: f32 = (i..j).map(|k| b.clusters.get(k).map(|c| c.adv - spacing(b, k)).unwrap_or(0.0).max(0.0)).sum();
        let n = j - i;
        let width = ft.width.clamp(0.1, 1584.0);
        if width >= natural {
            let gap = if n > 1 { (width - natural) / (n - 1) as f32 } else { width - natural };
            for k in i..j {
                let s = spacing(b, k);
                if let Some(c) = b.clusters.get_mut(k) {
                    c.adv = (c.adv - s).max(0.0) + if k + 1 < j || n == 1 { gap } else { 0.0 };
                }
            }
        } else if natural > 0.0 {
            let scale = width / natural;
            for k in i..j {
                let s = spacing(b, k);
                let (g0, g1) = b.clusters.get(k).map(|c| (c.g0 as usize, c.g1 as usize)).unwrap_or((0, 0));
                for g in b.glyphs.get_mut(g0..g1).into_iter().flatten() {
                    g.dx *= scale;
                }
                if let Some(c) = b.clusters.get_mut(k) {
                    c.adv = (c.adv - s).max(0.0) * scale;
                }
            }
        }
        b.glued.extend(i..j.saturating_sub(1));
        i = j;
    }
}

/// The room an emphasis mark (傍点) asks for on its side of the text, in ems of the marked
/// text: measured in Word for Mac at 10.5 and 16 pt with and without a document grid (3.6–3.84
/// pt at 10.5, 5.76 at 16, the same for dots, commas, circles and under dots, in horizontal
/// lines and vertical columns alike). How the line takes it is decided where the line is sized
/// (`layout_para`): without a grid the line grows by it on the mark's side; in a grid the
/// text's own slack can absorb it.
pub(crate) const EMPHASIS_ROOM: f32 = 0.36;

/// Which spaces (U+0020) take the East Asian width — half their font size — as measured in Word
/// for Mac with `w:balanceSingleByteDoubleByteWidth` set, as its Japanese template does (without
/// it no space does: `layout_para`; 10.5 pt 游明朝 and Aptos, `cg7`/`cg10`/`cg15` samples, 2026-10-19): a space next to an
/// East Asian character, through other spaces, and every space of a run of two or more (あ␣a,
/// a␣あ, あ␣␣a, a␣␣あ, x␣␣y and ab␣␣cd all take 5.25 pt a space; x␣y takes Aptos's 2.14), with
/// or without a lines-and-characters grid. Snapping to characters (`snap_all`) measures a single
/// space after Latin text and before an East Asian character with the Latin font (a␣あ, Word␣文:
/// the Latin block's cell padding says so) and, of two such, only the second as East Asian.
pub(super) fn ea_space_flags(b: &Builder, text: &str, snap_all: bool) -> Vec<bool> {
    let n = b.clusters.len();
    let mut flags = vec![false; n];
    let char_at = |k: usize| b.clusters.get(k).and_then(|c| text.get(c.start..c.end)).and_then(|t| t.chars().next());
    let is_space = |k: usize| b.clusters.get(k).is_some_and(|c| c.kind == ClKind::Space) && char_at(k) == Some(' ');
    let is_marker = |k: usize| b.clusters.get(k).is_some_and(|c| c.kind == ClKind::Marker);
    let ea_at = |k: usize| b.clusters.get(k).is_some_and(|c| matches!(c.kind, ClKind::Text | ClKind::Space)) && char_at(k).is_some_and(is_east_asian);
    let mut i = 0;
    while i < n {
        if !is_space(i) {
            i += 1;
            continue;
        }
        let mut j = i + 1;
        while j < n && (is_space(j) || is_marker(j)) {
            j += 1;
        }
        let spaces: Vec<usize> = (i..j).filter(|k| is_space(*k)).collect();
        let left_ea = (0..i).rev().find(|k| !is_marker(*k)).is_some_and(ea_at);
        let right_ea = (j..n).find(|k| !is_marker(*k)).is_some_and(ea_at);
        let run = spaces.len();
        for (q, k) in spaces.iter().enumerate() {
            let ea = if snap_all { left_ea || (run >= 2 && (q > 0 || !right_ea)) } else { left_ea || right_ea || run >= 2 };
            if let Some(f) = flags.get_mut(*k) {
                *f = ea;
            }
        }
        i = j;
    }
    flags
}

/// Gives the East Asian spaces their width (plus their character spacing): half the full-width
/// advance — 一's, in the column direction — of the face the space is drawn in, else of the
/// East Asian face nearest it, else half the size. Word for Mac (`hk2`–`hk3`, `vk2`–`vk3` probes,
/// 2026-10-19): a space of Apple SD Gothic Neo, whose Hangul and ideographs advance 0.865 em,
/// takes 4.54 pt at 10.5 pt and 10.38 at 24 (its own space glyph is 0.263 em), 6.3 pt in a
/// vertical column (its vertical advance is 1.2 em); one of AppleGothic or 游明朝 5.25 either
/// way; one Times New Roman draws between that font's kana takes 4.55, Apple SD Gothic Neo's.
pub(super) fn space_widths(b: &mut Builder, ea_space: &[bool], text: &str) {
    if !ea_space.iter().any(|f| *f) {
        return;
    }
    let n = b.clusters.len();
    let ea_style = |c: &Cluster| cluster_char(text, c).filter(|ch| is_east_asian(*ch)).map(|_| c.style);
    let mut before = vec![None; n];
    let mut last = None;
    for (k, c) in b.clusters.iter().enumerate() {
        if let Some(f) = before.get_mut(k) {
            *f = last;
        }
        last = ea_style(c).or(last);
    }
    let mut after = vec![None; n];
    let mut next = None;
    for (k, c) in b.clusters.iter().enumerate().rev() {
        next = ea_style(c).or(next);
        if let Some(f) = after.get_mut(k) {
            *f = next;
        }
    }
    let vertical = b.env.vertical;
    let half_em = |st: &StyleRun, size: f32| -> Option<f32> {
        let gid = st.face.glyph_for('一');
        if gid == 0 {
            return None;
        }
        let adv = if vertical { st.face.vertical_advance(gid).unwrap_or_else(|| st.face.advance(gid)) } else { st.face.advance(gid) };
        let w = (adv / st.face.upem.max(1.0)) as f32 * size * 0.5;
        (w.is_finite() && w > 0.0).then_some(w)
    };
    for k in 0..n {
        if !ea_space.get(k).copied().unwrap_or(false) {
            continue;
        }
        let Some(st) = b.clusters.get(k).and_then(|c| b.styles.get(c.style as usize)) else { continue };
        if !st.size.is_finite() {
            continue;
        }
        let neighbour = before.get(k).copied().flatten().or(after.get(k).copied().flatten());
        let half = half_em(st, st.size)
            .or_else(|| neighbour.and_then(|si| b.styles.get(si as usize)).and_then(|es| half_em(es, st.size)))
            .unwrap_or(st.size * 0.5);
        if let Some(c) = b.clusters.get_mut(k) {
            c.adv = (half + spacing_for(st.rc.spacing, ' ')).max(0.0);
        }
    }
}

/// Character grid for paragraphs that snap to it (see [`Grid`] for the measurements). Lines and
/// characters: an East Asian character advances by its own advance plus the grid's gap, its
/// glyph at the start; single-byte characters — Latin text, spaces and half-width forms — gain
/// half the gap each with `w:balanceSingleByteDoubleByteWidth` (`balance`, Word's Japanese
/// template) and the whole gap without it (`cp6` probes, Word for Mac 2026-10-19: on an 11.7 pt
/// pitch over 10.5 pt, b follows a by 5.63 with the switch and 6.23 without, ｱ takes 5.85 and
/// 6.45). Snap to characters: an East Asian character takes whole cells with its advance centred
/// in them (a half-width ｱ sits 3.2 pt into an 11.7 pt cell); a mark of a compressed chain
/// (`chain`) keeps its em, and Latin text its advances: `distribute_cells` fits them to cells
/// line by line.
pub(super) fn snap_chars(b: &mut Builder, text: &str, grid: &Grid, chain: &[bool], balance: bool) {
    if grid.chars.is_none() {
        return;
    }
    let gap = grid.gap();
    let single_byte_gap = if balance { gap / 2.0 } else { gap };
    for (k, c) in b.clusters.iter_mut().enumerate() {
        if !matches!(c.kind, ClKind::Text | ClKind::Space) || c.adv <= 0.0 {
            continue;
        }
        let Some(ch) = text.get(c.start..).and_then(|t| t.chars().next()) else { continue };
        let spacing = b.styles.get(c.style as usize).map_or(0.0, |st| spacing_for(st.rc.spacing, ch));
        if is_east_asian(ch) {
            if chain.get(k).copied().unwrap_or(false) {
                continue;
            }
            let natural = (c.adv - spacing).max(0.0);
            let adv = if grid.cell.is_none() && is_half_width_form(ch) {
                (natural + single_byte_gap).max(0.0) + spacing
            } else {
                grid.ea_advance(natural) + spacing
            };
            if grid.cell.is_some() {
                let shift = (adv - spacing - natural) / 2.0;
                for g in b.glyphs.get_mut(c.g0 as usize..c.g1 as usize).into_iter().flatten() {
                    g.dx += shift;
                }
            }
            c.adv = adv;
        } else if grid.cell.is_none() {
            c.adv += single_byte_gap;
        }
    }
}

/// Character borders take room, as Word for Mac sets them (`cb1`/`cbv1` probes, 2026-10-19): the
/// text of a bordered run moves in by the border's width plus its distance from the text
/// (`w:space`), and what follows moves on by as much after it — a 0.5-pt border adds a point to
/// the run, a 1.5-pt one three, a 0.5-pt border 4 pt from the text nine. Runs with equal borders
/// share one box, so the room sits at the ends of the group only. The paragraph mark is never
/// boxed.
pub(super) fn border_room(b: &mut Builder) {
    let border_of = |k: usize| -> Option<wordcraft_doc::props::Border> {
        let c = b.clusters.get(k)?;
        if c.kind == ClKind::Marker {
            return None;
        }
        b.styles.get(c.style as usize).and_then(|st| st.rc.border)
    };
    let n = b.clusters.len();
    let ends: Vec<(usize, f32, bool, bool)> = (0..n)
        .filter_map(|k| {
            let bd = border_of(k)?;
            let room = (bd.width.max(0.0) + bd.space.max(0.0)).min(50.0);
            if room <= 0.0 {
                return None;
            }
            let first = k == 0 || border_of(k - 1) != Some(bd);
            let last = k + 1 >= n || border_of(k + 1) != Some(bd);
            (first || last).then_some((k, room, first, last))
        })
        .collect();
    for (k, room, first, last) in ends {
        let Some(c) = b.clusters.get_mut(k) else { continue };
        if first {
            c.adv += room;
            for g in b.glyphs.get_mut(c.g0 as usize..c.g1 as usize).into_iter().flatten() {
                g.dx += room;
            }
        }
        if last {
            c.adv += room;
        }
    }
}

/// A half-width form (ｱｲｳ, ｡｢｣ and the half-width symbols): a single-byte character to the
/// character grid.
pub(super) fn is_half_width_form(c: char) -> bool {
    matches!(c as u32, 0xFF61..=0xFFDC | 0xFFE8..=0xFFEE)
}

/// What a cluster is to a snap-to-characters line (see `distribute_cells`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CellClass {
    /// Takes no part: markers, tabs, breaks.
    Other,
    /// A unit of whole cells: an East Asian character (the ideographic space too) or an object.
    Cell,
    /// A Hangul character: a cell that stays beside the next Hangul cell.
    Hangul,
    /// A closing mark of a compressed chain (。、」… one after another): the chain is one unit.
    Mark,
    /// Latin text: part of a block that takes whole cells.
    Latin,
    /// A Latin-width space inside a block.
    LatinSpace,
    /// A space with the East Asian width (`ea_space_flags`) inside a block.
    EastAsianSpace,
}

/// The marks of compressed chains (`cell_classes`, `snap_chars`): two or more closing marks one
/// after another (Word for Mac, `cg17`: 。」, 。、。 and 」、。、 before 「; not 、「, 」「 or ・。).
pub(super) fn chain_flags(b: &Builder, text: &str) -> Vec<bool> {
    let mut flags = vec![false; b.clusters.len()];
    for i in 1..b.clusters.len() {
        let (Some(prev), Some(next)) = (b.clusters.get(i - 1), b.clusters.get(i)) else { continue };
        let (Some(pc), Some(nc)) = (cluster_char(text, prev), cluster_char(text, next)) else { continue };
        if COMPRESSIBLE.contains(pc) && COMPRESSIBLE.contains(nc) {
            for k in [i - 1, i] {
                if let Some(f) = flags.get_mut(k) {
                    *f = true;
                }
            }
        }
    }
    flags
}

pub(super) fn cell_classes(b: &Builder, text: &str, ea_space: &[bool], chain: &[bool]) -> Vec<CellClass> {
    b.clusters
        .iter()
        .enumerate()
        .map(|(k, c)| match c.kind {
            ClKind::Object(_) => CellClass::Cell,
            ClKind::Text | ClKind::Space => match text.get(c.start..c.end).and_then(|t| t.chars().next()) {
                Some(_) if chain.get(k).copied().unwrap_or(false) => CellClass::Mark,
                Some(ch) if is_hangul(ch) => CellClass::Hangul,
                Some(ch) if is_east_asian(ch) => CellClass::Cell,
                Some(' ') if c.kind == ClKind::Space => {
                    if ea_space.get(k).copied().unwrap_or(false) {
                        CellClass::EastAsianSpace
                    } else {
                        CellClass::LatinSpace
                    }
                }
                Some(_) if c.kind == ClKind::Space => CellClass::LatinSpace,
                Some(_) => CellClass::Latin,
                None => CellClass::Other,
            },
            _ => CellClass::Other,
        })
        .collect()
}

/// The first character of a text cluster.
pub(super) fn cluster_char(text: &str, c: &Cluster) -> Option<char> {
    (c.kind == ClKind::Text).then(|| text.get(c.start..c.end)).flatten().and_then(|t| t.chars().next())
}

/// An East Asian letter for the automatic spacing: an ideograph, kana (half-width too), Hangul
/// — not punctuation (Word adds no space after 。 before Latin text) and not a full-width Latin
/// letter or digit (measured in Word for Mac, `cg17`/`cg18`: ＡＢＣabc, １２３abc and abcＡＢＣ
/// get none; ｱｲｳabc and 한글abc get the quarter em).
pub(super) fn ea_letter(c: char) -> bool {
    !matches!(c as u32, 0xFF01..=0xFF5E) && (crate::kinsoku::is_east_asian(c) || is_east_asian(c) || is_hangul(c)) && c.is_alphanumeric()
}

/// A Hangul syllable or jamo: on a snap-to-characters line two of them stay together (Word for
/// Mac spreads Korean text by its words, not its characters).
pub(super) fn is_hangul(c: char) -> bool {
    matches!(c as u32, 0x1100..=0x11FF | 0x3130..=0x318F | 0xA960..=0xA97F | 0xAC00..=0xD7FF)
}

/// Word's "Adjust space between Asian and Latin text" and "… between Asian text and numbers"
/// (`w:autoSpaceDE`, `w:autoSpaceDN`; both on unless a paragraph turns them off): a quarter of the
/// font size of the character before the boundary between an East Asian letter and a Latin
/// letter (`letters`) or a digit (`digits`) next to it, when no space separates them. Measured
/// in Word for Mac: 漢字Latin漢字123漢字 at 10.5 pt has 2.6 pt gaps, 1/4 em, at each of the four
/// boundaries; on a lines-and-characters grid the grid's gap joins the size (a 14 pt pitch over
/// 10.5 pt: 3.5 after a 10.5 pt character, 4.4 after a 14 pt one, 2.9 after an 8 pt one);
/// snapping to characters the space is 15/64 of the size (2.46 at 10.5 pt, 3.28 at 14), whatever
/// the cell. The space is added to the advance of the character before the boundary; returned
/// per cluster for the snap-to-characters layout, which lays its Latin blocks out again.
pub(super) fn auto_space(b: &mut Builder, text: &str, letters: bool, digits: bool, grid: &Grid, snapped: bool) -> Vec<(usize, f32)> {
    let latin =
        |c: char| !crate::kinsoku::is_east_asian(c) && !is_east_asian(c) && ((letters && c.is_alphabetic()) || (digits && c.is_ascii_digit()));
    // A field or note number is classed by the text it shows: its last character before the
    // boundary, its first after it (#2358).
    let shown: std::collections::BTreeMap<usize, &str> = b.shown.iter().map(|s| (s.0, s.1.as_str())).collect();
    let ends = |i: usize, c: &Cluster| -> Option<(char, char)> {
        match shown.get(&i) {
            Some(t) if c.kind == ClKind::Text => t.chars().next().zip(t.chars().next_back()),
            _ => cluster_char(text, c).map(|ch| (ch, ch)),
        }
    };
    let mut gaps: Vec<(usize, f32)> = Vec::new();
    for i in 1..b.clusters.len() {
        let (Some(prev), Some(next)) = (b.clusters.get(i - 1), b.clusters.get(i)) else { continue };
        let (Some((_, pc)), Some((nc, _))) = (ends(i - 1, prev), ends(i, next)) else { continue };
        // A 縦中横 run takes exactly the font's line height along the column (measured).
        if b.tcy.iter().any(|t| t.0 == i - 1 || t.0 == i) {
            continue;
        }
        if !((ea_letter(pc) && latin(nc)) || (latin(pc) && ea_letter(nc))) {
            continue;
        }
        // Character scaling (`w:w`) scales the space with the characters (Word for Mac: 3 pt beside
        // 24-pt 游明朝 at 50 %, 12 at 200 %, across a line and along a column — `hw1`, `hw2`).
        let size = b.styles.get(prev.style as usize).map_or(0.0, |st| st.size * (st.rc.scale / 100.0).clamp(0.01, 6.0));
        // The East Asian character's advance per em scales the space: an em for a full-width
        // character; 0.865 for Apple SD Gothic Neo's Hangul across a line and 1.2 standing in a
        // column (Word for Mac: 5.19 and 7.2 pt beside its 24-pt Hangul, 2026-10-19).
        let ea = if ea_letter(pc) { prev } else { next };
        let per_em = b
            .glyphs
            .get(ea.g0 as usize)
            .zip(b.styles.get(ea.style as usize))
            .map_or(1.0, |(g, st)| {
                let adv =
                    if b.env.vertical { st.face.vertical_advance(g.gid).unwrap_or_else(|| st.face.advance(g.gid)) } else { st.face.advance(g.gid) };
                // One of Word's own table fonts by name: its pitch (Malgun Gothic 1.0625 em along
                // a column: a 6.375-pt space at 24 pt, `kr3`).
                match st.named {
                    Some(n) if b.env.vertical => n.v_advance as f32,
                    Some(wordcraft_fonts::word::NamedEa { advance: Some(a), .. }) => a as f32,
                    _ => (adv / st.face.upem.max(1.0)) as f32,
                }
            })
            .clamp(0.25, 4.0);
        if size.is_finite() && size > 0.0 {
            let space = match (snapped, grid.chars, grid.cell) {
                (true, Some(_), Some(_)) => size * 15.0 / 64.0,
                (true, Some(_), None) => (size + grid.gap()) / 4.0,
                _ => size * per_em / 4.0,
            };
            gaps.push((i - 1, space.max(0.0)));
        }
    }
    for (i, gap) in &gaps {
        if let Some(c) = b.clusters.get_mut(*i) {
            c.adv += gap;
        }
    }
    gaps
}

/// Full-width closing punctuation that Word's "compress punctuation" (`w:characterSpacingControl`)
/// narrows to half width when another full-width punctuation mark follows it. Measured in Word
/// for Mac (Japanese Normal template): in 「括弧」と（丸括弧）、句読点。。。、、、 the ） before 、
/// and every 。 and 、 but the last take half an em; a 、 before 句 keeps its full width.
pub(super) const COMPRESSIBLE: &str = "、。，．）」』】〕〉》〙〗〟］｝";
/// `ParaLayout::ea_flags`: an East Asian character; a closing mark compression may narrow.
pub const EA_CHAR: u8 = 1;
pub const EA_CLOSING: u8 = 2;
pub(super) const FULL_WIDTH_PUNCT: &str = "、。，．・：；！？）」』】〕〉》〙〗〟］｝（「『【〔〈《〘〖〝［｛";
/// Full-width opening brackets: after any full-width mark one loses its blank left half (Word
/// for Mac: 「「い, 「（い, 『「い, （（い, ：「い, ・「い all put い 1.5 em after the first mark, and
/// あ×44、「 wraps as あ×43 / あ、「 — the 、 keeps its width, so it is the bracket that shrinks).
/// With `w:topLinePunct` one at the start of a line takes half its cell (see `break_lines`).
pub(super) const OPENING: &str = "（「『【〔〈《〘〖〝［｛";

/// Closing marks that end a justified or distributed line as Word for Mac ends it (`hp1`–`hp7`
/// probes with Japanese editing, 2026-10-19; see `break_lines`): one that doesn't fit the line
/// stays on it at half its width when that fits (its glyph whole, the blank right half past the
/// edge, the line spread to the edge), else hangs whole past the edge — 句読点のぶら下げ,
/// `w:overflowPunct` — one a line, the line spread to the edge before it; with the option off
/// it stays only at its full width where the half would fit, and the line is not spread. A
/// left-, centre- or right-aligned line moves it on with the character before it, as every
/// 縦書き column does (`hp8`); ！ and ？ move on too; … ー and small kana start the next line
/// (standard kinsoku).
pub(super) const LINE_END_COMPRESS: &str = "、。，．」』）］｝】";
/// Closing marks Word for Mac keeps at the end of a justified line at their full width where
/// half an em would fit, the line not spread, with the option on or off (〕〉》 after 44 あ at
/// 10.5 pt; 〙〗〟 by their kind).
pub(super) const LINE_END_SQUEEZE: &str = "〕〉》〙〗〟";
/// Centred marks (中点類) Word for Mac keeps at the end of a justified line where half an em
/// would fit, the line not spread, pulled back into the line by whatever of the overflow exceeds
/// a quarter of their em, with the option on or off (`hp9` probe, 2026-10-19: after 44 あ at
/// 10.5 pt on a 468-pt line, 4.5 pt over, ：；・ are drawn 1.88 pt back over the last あ and end
/// 2.6 past the margin; 2.5 over, flush and 2.5 past; at 12 pt 6 over, 3 back; 6.5 over at 10.5
/// or 8 at 14 pt, they move on with the あ before them; a left-aligned line moves them on).
pub(super) const LINE_END_CENTRED: &str = "：；・";

/// Which marks a run's East Asian language (`w:lang/@w:eastAsia`) lets Word for Mac compress
/// (`hk2`–`hk4` probes, 2026-10-19, 游明朝, Hiragino Mincho ProN, AppleGothic and AppleMyungjo at
/// 10.5 pt): Japanese compresses the closing marks before any full-width mark but ！ and ？
/// (」！, ．！, 、？ keep their width; ：「, ；「 and ・「 close up); Simplified Chinese counts ！ as a
/// closing mark too (」！「 closes up both, 。！ and ！。 and ！！ the first; ！「 the ！) and neither
/// ？ nor ・ as a mark at all (？「, 。？ and ・「 keep their width); Traditional Chinese is
/// Japanese without ・; Korean, English and every other tag compress nothing at all (not even
/// 游明朝's 。「); no tag at all follows the Japanese editing language. A mark whose face draws
/// it narrower than its em (Apple SD Gothic Neo's 。 at 0.773 em, 「 at 0.33) is never compressed,
/// nor is a mark that follows a Hangul letter in the same face (다。「, 다」。: the marks after
/// a strong character take its script, as Unicode itemises them, and Korean compresses nothing;
/// abc。「, ａｂｃ。「, １２。「, αβ。「 and あ」。 close up, as do marks after a Hangul letter drawn
/// in another face — a fallback face or another run's).
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum CompressRules {
    Japanese,
    Simplified,
    Traditional,
    None,
}

impl CompressRules {
    fn for_lang(lang: Option<&str>) -> Self {
        let Some(tag) = lang.map(str::trim).filter(|t| !t.is_empty()) else { return Self::Japanese };
        match wordcraft_doc::east_asian::script_of_lang(tag) {
            Some("Jpan") => Self::Japanese,
            Some("Hans") => Self::Simplified,
            Some("Hant") => Self::Traditional,
            _ => Self::None,
        }
    }
    /// A closing mark this language narrows to half its em before another mark.
    fn closing(self, c: char) -> bool {
        match self {
            Self::None => false,
            Self::Simplified => COMPRESSIBLE.contains(c) || c == '！',
            Self::Japanese | Self::Traditional => COMPRESSIBLE.contains(c),
        }
    }
    /// A full-width mark that closes up with a closing mark before it or an opening bracket
    /// after it.
    fn mark(self, c: char) -> bool {
        FULL_WIDTH_PUNCT.contains(c)
            && match self {
                Self::None => false,
                Self::Japanese => !matches!(c, '！' | '？'),
                Self::Traditional => !matches!(c, '！' | '？' | '・'),
                Self::Simplified => !matches!(c, '？' | '・'),
            }
    }
}

/// Per cluster: the nearest character of a strong script before it in the same face is Hangul
/// (see [`CompressRules`]). Empty when the text has no Hangul.
pub(super) fn hangul_context(b: &Builder, text: &str) -> Vec<bool> {
    if !text.chars().any(is_hangul) {
        return Vec::new();
    }
    let mut out = vec![false; b.clusters.len()];
    let mut korean = false;
    let mut face: Option<u32> = None;
    for (k, c) in b.clusters.iter().enumerate() {
        let f = b.styles.get(c.style as usize).map(|st| st.face.id());
        if f != face {
            korean = false;
            face = f;
        }
        if let Some(ch) = cluster_char(text, c) {
            if is_hangul(ch) {
                korean = true;
            } else if ch.is_alphabetic() {
                korean = false;
            }
        }
        if let Some(o) = out.get_mut(k) {
            *o = korean;
        }
    }
    out
}

/// The cluster's glyph advances a whole em in its face (a mark a proportional face draws
/// narrower is never compressed).
pub(super) fn full_width_glyph(b: &Builder, c: &Cluster) -> bool {
    let (Some(st), Some(g)) = (b.styles.get(c.style as usize), b.glyphs.get(c.g0 as usize)) else { return false };
    c.g1 > c.g0 && st.face.advance(g.gid) >= st.face.upem * 0.95
}

/// `cells`: the paragraph snaps to characters, where Word for Mac was measured (`cg2`/`cg9`,
/// 2026-10-19) to compress a closing mark only before another closing mark (。、) and to keep an
/// opening bracket after a mark (、「) in its whole cell.
pub(super) fn compress_punctuation(b: &mut Builder, text: &str, cells: bool) {
    let mut halve: Vec<usize> = Vec::new();
    let mut shrink_left: Vec<usize> = Vec::new();
    let korean = hangul_context(b, text);
    for i in 1..b.clusters.len() {
        let (Some(prev), Some(next)) = (b.clusters.get(i - 1), b.clusters.get(i)) else { continue };
        let (Some(pc), Some(nc)) = (cluster_char(text, prev), cluster_char(text, next)) else { continue };
        let rules = |c: &Cluster| CompressRules::for_lang(b.styles.get(c.style as usize).and_then(|st| st.rc.lang_ea.as_deref()));
        let (pr, nr) = (rules(prev), rules(next));
        if korean.get(i - 1).copied().unwrap_or(false) || korean.get(i).copied().unwrap_or(false) {
            continue;
        }
        if cells {
            if pr.closing(pc) && nr.closing(nc) && full_width_glyph(b, prev) {
                halve.push(i - 1);
            }
        } else if OPENING.contains(nc) && nr.mark(nc) && pr.mark(pc) {
            if full_width_glyph(b, next) {
                shrink_left.push(i);
            }
        } else if pr.closing(pc) && nr.mark(nc) && full_width_glyph(b, prev) {
            halve.push(i - 1);
        }
    }
    for i in halve {
        if let Some(c) = b.clusters.get_mut(i)
            && c.adv.is_finite()
        {
            // Closing marks sit in the left half of their cell: the glyph stays, the cell shrinks.
            c.adv /= 2.0;
        }
    }
    for i in shrink_left {
        if let Some(c) = b.clusters.get_mut(i)
            && c.adv.is_finite()
        {
            // Opening brackets sit in the right half: the cell loses its left half, the glyph
            // moves left into it.
            c.adv /= 2.0;
            let half = c.adv;
            for g in b.glyphs.get_mut(c.g0 as usize..c.g1 as usize).into_iter().flatten() {
                g.dx -= half;
            }
        }
    }
}

/// An area text wraps around.
#[derive(Clone, Debug, PartialEq)]
pub struct Exclusion {
    pub top: f32,
    pub bottom: f32,
    pub left: f32,
    pub right: f32,
    /// Text only above and below (no text beside it).
    pub top_bottom: bool,
    /// Tight and Through wrapping: the outline text follows inside the area (relative to its
    /// top-left); `None` = the whole area.
    pub contour: Option<Arc<crate::contour::Contour>>,
    /// The sides of the object text may go on (Square, Tight, Through).
    pub side: WrapText,
    /// A floating table counts a line touching its top as overlapping; shapes do not (#3977).
    pub table: bool,
}

impl Exclusion {
    /// Whether this area keeps text out of the row, including exact top contact for tables.
    pub(super) fn intersects_row(&self, top: f32, h: f32, table_h: f32, lo: f32, hi: f32) -> bool {
        let reaches = if self.table { self.top <= top + table_h } else { self.top < top + h };
        self.bottom > top && reaches && self.right > lo && self.left < hi
    }
}

/// Text Word sets in the East Asian font: CJK ideographs, kana, bopomofo, CJK punctuation and
/// fullwidth forms, and Hangul.
pub(super) fn east_asian(c: char) -> bool {
    crate::kinsoku::is_east_asian(c) || matches!(u32::from(c), 0x1100..=0x11FF | 0x3130..=0x318F | 0xAC00..=0xD7AF)
}

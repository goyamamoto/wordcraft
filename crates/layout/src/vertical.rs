//! Vertical sections (Layout › Text Direction, `w:textDirection` in `w:sectPr`).
//!
//! A vertical section is laid out unturned on a page turned onto its side — lines as long as the
//! page's text area is tall, stacking across its width — and each finished page is then turned
//! onto the real page, as a table cell's turned text is: top-to-bottom lines stack from the right
//! (`tbRl`), bottom-to-top ones from the left (`btLr`). Pictures, shapes and text boxes keep
//! standing upright where the turn carries them; headers and footers stay horizontal.

use wordcraft_doc::StoryRef;
use wordcraft_doc::props::{Borders, TextDirection};
use wordcraft_doc::section::SectionProps;
use wordcraft_geom::{Point, Rect};

use crate::{Page, Placed, turn_point, turn_rect};

/// The page a section running `turn` is laid out on, unturned: the real page `s` on its side,
/// its margins turned with it. `body_top` is where the real page's body starts (below a tall
/// header). Columns divide the turned page's width, so they stack down the real page.
pub(crate) fn unturned_sect(s: &SectionProps, body_top: f32) -> SectionProps {
    let mut v = s.clone();
    v.page_w = s.page_h;
    v.page_h = s.page_w;
    v.gutter = 0.0;
    let (top, right, bottom, left) = (body_top, s.margin_right, s.margin_bottom, s.margin_left + s.gutter);
    // The unturned frame's (u, v) lands at (W - v, u) for `Down` and (v, H - u) for `Up`: its
    // top edge is the real right edge (left for `Up`), its left edge the real top (bottom).
    match s.text_direction {
        TextDirection::Horizontal => return v,
        TextDirection::Down => {
            (v.margin_top, v.margin_left, v.margin_bottom, v.margin_right) = (right, top, left, bottom);
            v.page_borders = s.page_borders.as_ref().map(|b| Borders { top: b.right, left: b.top, bottom: b.left, right: b.bottom, ..*b });
        }
        TextDirection::Up => {
            (v.margin_top, v.margin_left, v.margin_bottom, v.margin_right) = (left, bottom, right, top);
            v.page_borders = s.page_borders.as_ref().map(|b| Borders { top: b.left, left: b.bottom, bottom: b.right, right: b.top, ..*b });
        }
    }
    v
}

/// Where the unturned frame's origin lands on the real `w` × `h` page.
fn origin(turn: TextDirection, w: f32, h: f32) -> (f32, f32) {
    match turn {
        TextDirection::Horizontal => (0.0, 0.0),
        TextDirection::Down => (w, 0.0),
        TextDirection::Up => (0.0, h),
    }
}

/// Mirror an unturned page's text area top to bottom, line by line, so that once turned its
/// lines stack from the real page's left edge to the right (`tbRlV`, see
/// [`SectionProps::stack_left_to_right`]). Each line keeps its own layout — the side its ruby,
/// underlines and emphasis marks are on — and moves to the mirror image of its place; a
/// paragraph's space before and after swap sides with it. A text box's text moves with its box.
fn stack_left_to_right(page: &mut Page) {
    let (t, b) = (page.body.y, page.body.bottom());
    let flip = |y: f32, h: f32| t + b - y - h;
    // How far each text box moves: its text follows it rather than being mirrored line by line.
    let mut boxes: Vec<(u32, f32)> = Vec::new();
    for it in &page.items {
        if let Placed::Object { rect, text_box: Some(id), .. } = it {
            boxes.push((*id, flip(rect.y, rect.h) - rect.y));
        }
    }
    let mut out = Vec::with_capacity(page.items.len());
    for it in std::mem::take(&mut page.items) {
        match it {
            Placed::Lines { story: StoryRef::Part(id), .. } if boxes.iter().any(|(b, _)| *b == id) => {
                let mut it = it;
                if let Some((_, dy)) = boxes.iter().find(|(b, _)| matches!(&it, Placed::Lines { story: StoryRef::Part(i), .. } if i == b)) {
                    it.translate(0.0, *dy);
                }
                out.push(it);
            }
            Placed::Lines { story, path, para, l0, l1, x, y, turn, slot } if !turn.is_turned() => {
                let Some(first_top) = para.lines.get(l0).map(|l| l.top) else { continue };
                for k in l0..l1 {
                    let Some(line) = para.lines.get(k) else { break };
                    let top = y + (line.top - first_top);
                    // The space before goes with the first line, after with the last; mirrored,
                    // each ends up on the other side of its line.
                    let slot =
                        crate::Slot { w: slot.w, before: if k + 1 == l1 { slot.after } else { 0.0 }, after: if k == l0 { slot.before } else { 0.0 } };
                    out.push(Placed::Lines {
                        story,
                        path: path.clone(),
                        para: para.clone(),
                        l0: k,
                        l1: k + 1,
                        x,
                        y: flip(top, line.height),
                        turn,
                        slot,
                    });
                }
            }
            mut it => {
                if let Some(r) = bounds(&it) {
                    it.translate(0.0, flip(r.y, r.h) - r.y);
                }
                out.push(it);
            }
        }
    }
    page.items = out;
}

/// Turn a page laid out unturned (see [`unturned_sect`]) onto the real `phys` page.
pub(crate) fn turn_page(page: &mut Page, turn: TextDirection, phys: &SectionProps) {
    if !turn.is_turned() {
        return;
    }
    if turn == TextDirection::Down && phys.stack_left_to_right {
        stack_left_to_right(page);
    }
    let (ox, oy) = origin(turn, phys.page_w, phys.page_h);
    page.w = phys.page_w;
    page.h = phys.page_h;
    page.body = turn_rect(turn, ox, oy, page.body);
    // Pictures, shapes and text boxes stand upright: their centres move with the turn. A text
    // box's text moves with its box (it runs as the box says, not as the page does).
    let upright = |r: Rect| {
        let (cx, cy) = turn_point(turn, ox, oy, r.x + r.w / 2.0, r.y + r.h / 2.0);
        Rect::new(cx - r.w / 2.0, cy - r.h / 2.0, r.w, r.h)
    };
    let mut boxes: Vec<(u32, Rect, (f32, f32))> = Vec::new();
    for it in &mut page.items {
        if let Placed::Object { rect, origin, text_box, .. } = it {
            let r = upright(*rect);
            if let Some(id) = text_box {
                boxes.push((*id, *rect, (r.x - rect.x, r.y - rect.y)));
            }
            *rect = r;
            (origin.x, origin.y) = turn_point(turn, ox, oy, origin.x, origin.y);
        }
    }
    let in_box = |it: &Placed| -> Option<(f32, f32)> {
        if let Placed::Lines { story: StoryRef::Part(id), .. } = it
            && let Some((_, _, d)) = boxes.iter().find(|(b, _, _)| b == id)
        {
            return Some(*d);
        }
        let r = bounds(it)?;
        boxes
            .iter()
            .find(|(_, b, _)| b.expand(0.5).contains(Point::new(r.x, r.y)) && b.expand(0.5).contains(Point::new(r.right(), r.bottom())))
            .map(|(_, _, d)| *d)
    };
    for it in &mut page.items {
        if matches!(it, Placed::Object { .. }) {
            continue;
        }
        if let Some((dx, dy)) = in_box(it) {
            it.translate(dx, dy);
            continue;
        }
        match it {
            Placed::Image { rect, .. } | Placed::Shape { rect, .. } | Placed::Graphic { rect, .. } => *rect = upright(*rect),
            _ => it.turn(turn, ox, oy),
        }
    }
}

/// An item's unturned page area (lines: their band; `None` for turned lines, which a text box
/// in the section holds).
fn bounds(it: &Placed) -> Option<Rect> {
    match it {
        Placed::Lines { para, l0, l1, x, y, turn, .. } if !turn.is_turned() => {
            let first = para.lines.get(*l0)?;
            let last = para.lines.get(l1.checked_sub(1)?)?;
            let right = para.lines.get(*l0..*l1).unwrap_or(&[]).iter().map(|l| l.right).fold(0.0f32, f32::max);
            Some(Rect::new(*x, *y, right, last.top + last.height - first.top))
        }
        Placed::Lines { .. } => it.turned_bounds(),
        Placed::Fill { rect, .. }
        | Placed::Image { rect, .. }
        | Placed::Shape { rect, .. }
        | Placed::Graphic { rect, .. }
        | Placed::Clip { rect, .. }
        | Placed::Cell { rect, .. }
        | Placed::Object { rect, .. } => Some(*rect),
        Placed::Rule { x0, y0, x1, y1, .. } => Some(Rect::new(x0.min(*x1), y0.min(*y1), (x1 - x0).abs(), (y1 - y0).abs())),
    }
}

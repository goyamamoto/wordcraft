//! Document grid (`w:docGrid`): line pitch and character pitch (#391). Assertions are relations
//! to the pitch, never font metrics (fonts differ between machines; CJK glyphs may be missing).

use super::*;
use wordcraft_doc::props::LineSpacing;
use wordcraft_doc::section::{DocGrid, DocGridType};

fn lay(doc: &Document) -> DocLayout {
    layout(doc, &mut LayoutCache::new(), &LayoutOptions::default())
}

fn with_grid(text: &str, kind: DocGridType, char_space: i32) -> Document {
    let mut d = Document::from_text(text);
    for b in d.body.iter_mut() {
        if let Block::Para(p) = std::sync::Arc::make_mut(b) {
            p.props.space_before = Some(0.0);
            p.props.space_after = Some(0.0);
            p.props.line_spacing = Some(LineSpacing::Multiple(1.0));
        }
    }
    d.last_section.doc_grid = Some(DocGrid { kind, line_pitch: 18.0, char_space });
    // As Word's Japanese Normal: single-byte characters gain half the grid's gap.
    d.settings.balance_single_byte_double_byte_width = true;
    d
}

/// (first-page lines, every line height) of a layout.
fn lines(l: &DocLayout) -> (usize, Vec<f32>) {
    let mut first = 0;
    let mut hs = Vec::new();
    for (pi, p) in l.pages.iter().enumerate() {
        for it in &p.items {
            if let Placed::Lines { para, l0, l1, .. } = it {
                if pi == 0 {
                    first += l1 - l0;
                }
                hs.extend(para.lines[*l0..*l1].iter().map(|line| line.height));
            }
        }
    }
    (first, hs)
}

/// Word's `w:adjustLineHeightInTable` (`cp5` probes, Word for Mac 2026-10-19, an 18 pt line grid):
/// with it a table cell's lines snap to the pitch like the body's (94.7, 112.7, 130.7…); without
/// it — Word's English template — they keep their natural heights (93.3, 108.2, 123.3: 15.1 pt
/// apart for 10.5 pt Yu Mincho) while the body stays on the grid.
#[test]
fn table_text_snaps_to_the_line_grid_only_with_adjust_line_height_in_table() {
    let cell_heights = |adjust: bool| -> (Vec<f32>, Vec<f32>) {
        let mut d = with_grid("Body line\nAfter", DocGridType::Lines, 0);
        let mut t = wordcraft_doc::Table::new(1, 1, 300.0);
        let mut cell = wordcraft_doc::Paragraph::with_text("Cell line one\nCell line two", wordcraft_doc::CharProps::default());
        cell.props = d.body[0].as_para().unwrap().props.clone();
        t.rows[0].cells[0].blocks = vec![wordcraft_doc::para_block(cell)];
        d.body.insert(1, std::sync::Arc::new(Block::Table(t)));
        d.settings.adjust_line_height_in_table = adjust;
        let l = lay(&d);
        let (mut body, mut cell) = (Vec::new(), Vec::new());
        for it in &l.pages[0].items {
            if let Placed::Lines { para, path, l0, l1, .. } = it {
                let hs = para.lines[*l0..*l1].iter().map(|line| line.height);
                if path.0.len() > 1 { cell.extend(hs) } else { body.extend(hs) }
            }
        }
        (body, cell)
    };
    let (body, cell) = cell_heights(true);
    assert_eq!((body.len(), cell.len()), (2, 2), "{body:?} {cell:?}");
    assert!(body.iter().chain(&cell).all(|h| (h - 18.0).abs() < 1e-3), "all on the pitch: body {body:?} cell {cell:?}");
    let (body, cell) = cell_heights(false);
    assert!(body.iter().all(|h| (h - 18.0).abs() < 1e-3), "the body stays on the grid: {body:?}");
    assert!(cell.iter().all(|h| *h < 17.0 && *h > 10.0), "the cell's lines take their natural height: {cell:?}");
}

#[test]
fn line_grid_snaps_line_heights_and_fixes_lines_per_page() {
    let text = vec!["Grid line"; 80].join("\n");
    let d = with_grid(&text, DocGridType::Lines, 0);
    let (first, hs) = lines(&lay(&d));
    for h in &hs {
        let cells = h / 18.0;
        assert!((cells - cells.round()).abs() < 1e-3 && cells >= 1.0, "line height {h} is not a multiple of the pitch");
    }
    // Text at the default size fits one pitch: 648 pt of text height holds 36 lines.
    let h = hs[0];
    assert_eq!(first, (648.0 / h + 1e-3).floor() as usize, "heights {h}");
    // Turning the paragraphs' snapToGrid off leaves their natural line heights.
    let mut off = d.clone();
    for b in off.body.iter_mut() {
        if let Block::Para(p) = std::sync::Arc::make_mut(b) {
            p.props.snap_to_grid = Some(false);
        }
    }
    let plain = with_grid(&text, DocGridType::Default, 0);
    assert_eq!(lines(&lay(&off)).1[0], lines(&lay(&plain)).1[0]);
}

#[test]
fn character_grid_places_east_asian_text_on_the_pitch() {
    let text = "日本語の文書abc";
    // The default font size plus 2.5 pt: a pitch unlike any font's advance.
    let lines = wordcraft_doc::SectionProps { doc_grid: Some(DocGrid { kind: DocGridType::Lines, ..Default::default() }), ..Default::default() };
    let size = para::Grid::of(&Document::from_text(""), &lines).font_size;
    let cs = DocGrid::char_space_for(size + 2.5, size);
    let d = with_grid(text, DocGridType::LinesAndChars, cs);
    let pitch = d.last_section.grid_char_pitch(size).unwrap();
    assert!((pitch - (size + 2.5)).abs() < 0.01);
    let l = lay(&d);
    let plain = lay(&with_grid(text, DocGridType::Lines, cs));
    let first =
        |l: &DocLayout| l.pages[0].items.iter().find_map(|it| if let Placed::Lines { para, .. } = it { Some(para.clone()) } else { None }).unwrap();
    let (g, p) = (first(&l), first(&plain));
    let line = &g.lines[0];
    let latin = text.find('a').unwrap();
    let last_ea = text.chars().take_while(|c| *c != 'a').count() - 1;
    for (k, c) in g.clusters.iter().enumerate() {
        let x = line.xs[k] - line.xs[0];
        if c.start < latin {
            // Ideographs and kana sit on the pitch, one cell each (Word for Mac: their size plus
            // the grid's gap); the last gains the automatic space before the Latin text, a
            // quarter of the pitch there (a quarter of the size without a character grid).
            assert!((x - pitch * k as f32).abs() < 0.01, "cluster {k} at {x}, pitch {pitch}");
            let auto = if k == last_ea { pitch / 4.0 } else { 0.0 };
            assert!((c.adv - pitch - auto).abs() < 0.01, "cluster {k}: {} vs {pitch} + {auto}", c.adv);
        } else {
            // Latin text keeps its own advances plus half the gap each (measured in Word for Mac).
            assert!((c.adv - p.clusters[k].adv - 1.25).abs() < 0.01, "latin cluster {k}: {} vs {}", c.adv, p.clusters[k].adv);
        }
    }
}

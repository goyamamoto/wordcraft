//! Vertical sections (`w:textDirection` in `w:sectPr`): lines run down the page and stack from
//! the right (`tbRl`), or run up and stack from the left (`btLr`).

use super::*;
use wordcraft_doc::Pos;

fn lay(doc: &Document) -> DocLayout {
    let mut c = LayoutCache::new();
    layout(doc, &mut c, &LayoutOptions::default())
}

fn vertical_doc(dir: TextDirection) -> Document {
    let text = "Vertical sections lay their lines out down the page, one after another from right to left. ".repeat(4);
    let mut d = Document::from_text(&format!("{text}\n日本語のテキスト"));
    d.last_section.text_direction = dir;
    d
}

/// Body lines on page 0: (x, y, turn) of each piece.
fn body_lines(l: &DocLayout) -> Vec<(f32, f32, TextDirection)> {
    l.pages[0]
        .items
        .iter()
        .filter_map(|it| match it {
            Placed::Lines { story: StoryRef::Body, x, y, turn, .. } => Some((*x, *y, *turn)),
            _ => None,
        })
        .collect()
}

#[test]
fn top_to_bottom_section_stacks_lines_from_the_right() {
    let d = vertical_doc(TextDirection::Down);
    let l = lay(&d);
    let page = &l.pages[0];
    // The page keeps its size; the text area is the real one (inside the margins).
    assert_eq!((page.w, page.h), (d.last_section.page_w, d.last_section.page_h));
    assert!(page.body.x >= d.last_section.margin_left - 0.5 && page.body.right() <= page.w - d.last_section.margin_right + 0.5);
    let lines = body_lines(&l);
    assert!(lines.len() >= 2);
    assert!(lines.iter().all(|(.., t)| *t == TextDirection::Down));
    // The first paragraph's first line is the rightmost, the next paragraph left of it.
    let p0 = Pos::body(0, 0);
    let c0 = l.caret(&p0).unwrap();
    let c1 = l.caret(&Pos::body(1, 0)).unwrap();
    assert!(c1.x < c0.x, "second paragraph left of the first: {c1:?} vs {c0:?}");
    assert!(c0.x <= page.body.right() + 0.5 && c0.top >= page.body.y - 0.5);
    // Characters progress downward along a line; the caret lies across it.
    let c5 = l.caret(&Pos::body(0, 2)).unwrap();
    assert!(c5.top > c0.top && (c5.x - c0.x).abs() < 0.5, "{c5:?} vs {c0:?}");
    assert!(c0.width > c0.height);
    // Line two of the first paragraph is left of line one.
    let p = d.para(StoryRef::Body, &Path::top(0)).unwrap();
    let wrap = l.index.get(&(StoryRef::Body, Path::top(0))).map(|v| v[0]).unwrap();
    let Placed::Lines { para, .. } = &l.pages[wrap.0].items[wrap.1] else { panic!() };
    assert!(para.lines.len() >= 2 && para.lines[1].start < p.len());
    let c_next = l.caret(&Pos::body(0, para.lines[1].start + 1)).unwrap();
    assert!(c_next.x < c0.x - 1.0);
    // Clicking a caret position finds it again.
    for off in [0, 2, para.lines[1].start + 1] {
        let c = l.caret(&Pos::body(0, off)).unwrap();
        let hit = l.hit(c.page, c.x + c.width / 2.0, c.top + 0.2, StoryRef::Body).unwrap();
        assert_eq!(hit.off, off);
    }
    // Clicking in the empty space below a short line lands on that line (its end).
    let c_cjk = l.caret(&Pos::body(1, 0)).unwrap();
    let below = l.hit(0, c_cjk.x + c_cjk.width / 2.0, page.body.bottom() - 2.0, StoryRef::Body).unwrap();
    assert_eq!(below.path, Path::top(1));
}

#[test]
fn bottom_to_top_section_stacks_lines_from_the_left() {
    let l = lay(&vertical_doc(TextDirection::Up));
    assert!(body_lines(&l).iter().all(|(.., t)| *t == TextDirection::Up));
    let c0 = l.caret(&Pos::body(0, 0)).unwrap();
    let c1 = l.caret(&Pos::body(1, 0)).unwrap();
    let c5 = l.caret(&Pos::body(0, 2)).unwrap();
    assert!(c1.x > c0.x, "second paragraph right of the first");
    assert!(c5.top < c0.top, "characters progress upward");
}

#[test]
fn east_asian_characters_stand_upright_in_top_to_bottom_text() {
    let l = lay(&vertical_doc(TextDirection::Down));
    let d = vertical_doc(TextDirection::Down);
    let draws = display::page_display(&d, &l.pages[0], &Default::default());
    let turned: Vec<&Vec<display::Draw>> =
        draws.iter().filter_map(|x| if let display::Draw::Turned { items, .. } = x { Some(items) } else { None }).collect();
    let text_of = |ds: &[display::Draw]| -> String {
        ds.iter()
            .map(|x| match x {
                display::Draw::Glyphs { text, .. } => text.clone(),
                display::Draw::Rotated { items, .. } => {
                    items.iter().map(|i| if let display::Draw::Glyphs { text, .. } = i { text.clone() } else { String::new() }).collect()
                }
                _ => String::new(),
            })
            .collect()
    };
    // Each ideograph turned back upright on its own; the Latin text lies with the line.
    let cjk = turned.iter().find(|ds| text_of(ds).contains('日')).expect("the Japanese line is drawn turned");
    let upright = cjk.iter().filter(|x| matches!(x, display::Draw::Rotated { spin, .. } if spin.deg == 270.0)).count();
    assert_eq!(upright, "日本語のテキスト".chars().count());
    assert!(turned.iter().any(|ds| text_of(ds).contains("Vertical") && ds.iter().all(|x| !matches!(x, display::Draw::Rotated { .. }))));
}

#[test]
fn headers_stay_horizontal_and_a_vertical_section_starts_a_page() {
    use wordcraft_doc::section::SectionStart;
    let mut d = Document::from_text("Horizontal\nVertical");
    // Section 1 ends after paragraph 0 (continuous), section 2 is vertical.
    let first = SectionProps { start: SectionStart::Continuous, ..Default::default() };
    d.para_mut(StoryRef::Body, &Path::top(0)).unwrap().section = Some(Box::new(first));
    d.last_section.start = SectionStart::Continuous;
    d.last_section.text_direction = TextDirection::Down;
    let l = lay(&d);
    assert_eq!(l.pages.len(), 2);
    assert_eq!(l.caret(&Pos::body(0, 0)).unwrap().width, 0.0);
    assert!(l.caret(&Pos::body(1, 0)).unwrap().width > 0.0);
}

#[test]
fn horizontal_in_vertical_turns_the_run_in_one_cell_the_height_of_the_font() {
    // Word for Mac, 10.5 pt: "12" set 縦中横 occupies one cell of the font's line height
    // (ascent + descent) along the column, the figures turned upright and centred; drawn at
    // their natural width whether or not the run asks to compress.
    if !wordcraft_fonts::word::resolve("游明朝", false, false).face.covers('語') {
        return;
    }
    let props = |ea: Option<wordcraft_doc::props::EastAsianLayout>| wordcraft_doc::CharProps {
        font: Some("游明朝".into()),
        font_ea: Some("游明朝".into()),
        size: Some(10.5),
        lang_ea: Some("ja-JP".into()),
        east_asian_layout: ea,
        ..Default::default()
    };
    let tcy = wordcraft_doc::props::EastAsianLayout { vert: true, vert_compress: true, ..Default::default() };
    let mut p = wordcraft_doc::Paragraph::default();
    for (t, ea) in [("第", None), ("12", Some(tcy)), ("章", None)] {
        p.text.push_str(t);
        p.runs.push(wordcraft_doc::Run { len: t.len(), props: props(ea) });
    }
    let mut d = Document::from_text("");
    d.body = vec![wordcraft_doc::para_block(p)];
    d.last_section.text_direction = TextDirection::Down;
    let l = lay(&d);
    let para = l
        .pages
        .iter()
        .flat_map(|p| p.items.iter())
        .find_map(|it| if let Placed::Lines { para, .. } = it { Some(para.as_ref()) } else { None })
        .unwrap();
    let c = para.clusters.iter().find(|c| c.start == 3 && c.end == 5).expect("one cluster for 12");
    let st = &para.styles[c.style as usize];
    assert!((c.adv - (st.ascent + st.descent)).abs() < 0.05, "advance {} = line height {}", c.adv, st.ascent + st.descent);
    assert!(c.adv > 11.0 && c.adv < 17.0, "{}", c.adv);
    assert_eq!(para.tcy.len(), 1);
    let natural = para.tcy[0].1;
    assert!(natural > 9.0 && natural < 12.0, "two figures: {natural}");
    let draws = display::page_display(&d, &l.pages[0], &Default::default());
    let turned: Vec<&display::Draw> =
        draws.iter().filter_map(|x| if let display::Draw::Turned { items, .. } = x { Some(items) } else { None }).flatten().collect();
    let rotated: Vec<(f32, f32, &Vec<display::Draw>)> = turned
        .iter()
        .filter_map(|x| match x {
            display::Draw::Rotated { cx, cy, spin, items } if spin.deg == 270.0 => Some((*cx, *cy, items)),
            _ => None,
        })
        .filter(|(.., items)| items.iter().any(|i| matches!(i, display::Draw::Glyphs { text, .. } if text == "12")))
        .collect();
    assert_eq!(rotated.len(), 1, "the figures turn as one piece");
    let (cx, cy, items) = rotated[0];
    let Some(display::Draw::Glyphs { glyphs, size, .. }) = items.first() else { panic!() };
    assert_eq!(glyphs.len(), 2);
    assert!((*size - 10.5).abs() < 0.01);
    // Centred: the run starts half its natural width left of the centre; the centre sits on
    // the text box's middle.
    let left = x_of_cluster(para, c);
    assert!((glyphs[0].1 - (left + (c.adv - natural) / 2.0)).abs() < 0.05);
    assert!((cx - (left + c.adv / 2.0)).abs() < 0.05);
    assert!((cy - (glyphs[0].2 - (st.ascent - st.descent) / 2.0)).abs() < 0.05);
    assert!(!turned.iter().any(|x| matches!(x, display::Draw::Glyphs { text, .. } if text.contains("12"))), "not drawn flat as well");
    // Horizontal text draws the same turned piece (reading bottom to top), as Word does.
    d.last_section.text_direction = TextDirection::Horizontal;
    let l = lay(&d);
    let draws = display::page_display(&d, &l.pages[0], &Default::default());
    assert_eq!(draws.iter().filter(|x| matches!(x, display::Draw::Rotated { spin, .. } if spin.deg == 270.0)).count(), 1);
}

fn x_of_cluster(para: &crate::para::ParaLayout, c: &crate::para::Cluster) -> f32 {
    let k = para.clusters.iter().position(|x| std::ptr::eq(x, c)).unwrap();
    let line = para.lines.iter().find(|l| (l.c0..l.c1).contains(&k)).unwrap();
    line.cl_left(k).unwrap()
}

/// Top-to-bottom text takes the font's vertical forms (OpenType `vert`) for the punctuation
/// Word for Mac substitutes — 、。「」（）ー and the small kana stand upright as their vertical
/// glyphs, with no correction — while the ellipsis and the dash keep their horizontal form
/// turned along the line, as Word draws them (`plan/word/samples/ja-layout-v.pdf`).
#[test]
fn vertical_text_takes_the_fonts_vertical_forms_and_turns_the_dashes() {
    let mut d = Document::from_text("Vertical. \n日本語、縦書き。「括弧」ーぁっ（丸）…―");
    d.last_section.text_direction = TextDirection::Down;
    let l = lay(&d);
    let draws = display::page_display(&d, &l.pages[0], &Default::default());
    // Every glyph drawn turned: (character, glyph id, its face, whether it stands upright).
    let mut seen: Vec<(char, u32, wordcraft_fonts::FaceRef, bool)> = Vec::new();
    let mut take = |ds: &[display::Draw], upright: bool| {
        for x in ds {
            if let display::Draw::Glyphs { face, glyphs, text, ranges, .. } = x {
                for (g, r) in glyphs.iter().zip(ranges) {
                    if let Some(c) = text.get(r.clone()).and_then(|t| t.chars().next()) {
                        seen.push((c, g.0, *face, upright));
                    }
                }
            }
        }
    };
    for x in &draws {
        if let display::Draw::Turned { items, .. } = x {
            for it in items {
                match it {
                    display::Draw::Rotated { items, .. } => take(items, true),
                    other => take(std::slice::from_ref(other), false),
                }
            }
        }
    }
    let of = |c: char| seen.iter().find(|s| s.0 == c).unwrap_or_else(|| panic!("{c} is drawn"));
    for c in ['日', '、', '。', '「', '」', 'ー', 'ぁ', 'っ', '（', '）'] {
        assert!(of(c).3, "{c} stands upright");
    }
    for c in ['V', 'e', '…', '―'] {
        let (_, gid, face, upright) = *of(c);
        assert!(!upright, "{c} lies along the line");
        assert_eq!(gid, face.glyph_for(c), "{c} keeps its horizontal form");
    }
    // Where the font has vertical forms (every CJK font on macOS does), the brackets and the
    // prolonged sound mark are drawn as those forms, not as the horizontal glyphs.
    let (_, gid, face, _) = *of('「');
    if face.has_layout_tables() && cfg!(target_os = "macos") {
        assert_ne!(gid, face.glyph_for('「'), "「 takes its vertical form");
        assert_ne!(of('ー').1, face.glyph_for('ー'), "ー takes its vertical form");
        assert_ne!(of('、').1, face.glyph_for('、'), "、 takes its vertical form");
    }
}

/// A top-to-bottom section of 10.5-pt 游明朝 paragraphs (Word's Japanese default), with a
/// ruby's `(size, raise)` on 漢字 in a 前漢字後 paragraph or a 20-pt 大 in 前大後; `None`
/// without a Japanese font on this machine (then the checks are moot).
fn ja_vertical_doc(paras: &[Option<(f32, f32)>], big: bool, grid: Option<f32>) -> Option<Document> {
    use wordcraft_doc::para::RubyAlign;
    if !wordcraft_fonts::word::resolve("游明朝", false, false).face.covers('語') {
        return None;
    }
    let chr = |size: f32| CharProps {
        font: Some("游明朝".into()),
        font_ea: Some("游明朝".into()),
        size: Some(size),
        lang_ea: Some("ja-JP".into()),
        ..Default::default()
    };
    let mut d = Document::from_text("");
    d.body.clear();
    for (i, p) in paras.iter().enumerate() {
        let (text, size) = match (p, big && i == paras.len() - 1) {
            (Some(_), _) => ("前後", 10.5),
            (None, true) => ("前大後", 10.5),
            (None, false) => ("次の行", 10.5),
        };
        let mut para = Paragraph::with_text(text, chr(size));
        para.props.line_spacing = Some(wordcraft_doc::props::LineSpacing::Multiple(1.0));
        if big && i == paras.len() - 1 {
            para.runs = vec![
                wordcraft_doc::Run { len: "前".len(), props: chr(10.5) },
                wordcraft_doc::Run { len: "大".len(), props: chr(20.0) },
                wordcraft_doc::Run { len: "後".len(), props: chr(10.5) },
            ];
        }
        d.body.push(wordcraft_doc::para_block(para));
        if let Some((size, raise)) = p {
            let ruby = InlineObject::Ruby {
                base: "漢字".into(),
                ruby: "かんじ".into(),
                align: RubyAlign::DistributeSpace,
                size: *size,
                raise: *raise,
                base_size: 10.5,
                lang: "ja-JP".into(),
                props: Default::default(),
            };
            d.insert_object(&Pos::body(i, "前".len()), ruby, &chr(10.5)).unwrap();
        }
    }
    d.last_section.text_direction = TextDirection::Down;
    d.last_section.doc_grid =
        grid.map(|p| wordcraft_doc::section::DocGrid { kind: wordcraft_doc::section::DocGridType::Lines, line_pitch: p, char_space: 0 });
    Some(d)
}

/// The body paragraphs' first lines on page 0 in order: (height, centre line below the top).
fn columns(l: &DocLayout) -> Vec<(f32, f32)> {
    let mut v: Vec<(usize, f32, f32)> = l.pages[0]
        .items
        .iter()
        .filter_map(|it| match it {
            Placed::Lines { story: StoryRef::Body, path, para, .. } => {
                assert!(para.vertical, "laid out as a vertical column");
                let line = para.lines.first()?;
                Some((path.0[0] as usize, line.height, line.baseline - line.top))
            }
            _ => None,
        })
        .collect();
    v.sort_by_key(|x| x.0);
    v.into_iter().map(|(_, h, c)| (h, c)).collect()
}

/// Vertical columns measured in Word for Mac (10.5-pt 游明朝, single spacing, 2026-10-18): a
/// column is the text's line (1.43 em, 15.0 pt) centred on the characters' em squares, 7.5 pt
/// each side of its centre line; a ruby runs beside its base, its centre line `raise` to the
/// right, and the column reaches raise + ascent − 0.38 em of the ruby's size that way (a 5-pt
/// ruby raised 5: 16.04 pt from the previous column's centre; a 7-pt one raised 7: 19.2); a
/// 20-pt character shares the centre line of the 10.5-pt text round it (its column 28.6 wide).
#[test]
fn vertical_columns_centre_em_squares_and_run_rubies_beside_them_as_word_does() {
    let Some(d) = ja_vertical_doc(&[None, Some((5.0, 5.0)), None, Some((7.0, 7.0)), None, None], true, None) else { return };
    let l = lay(&d);
    let cols = columns(&l);
    assert_eq!(cols.len(), 6, "{cols:?}");
    let near = |a: f32, b: f32| (a - b).abs() < 0.05;
    // A plain column: the 1.43 em line, 0.715 em each side of its centre.
    assert!(near(cols[0].0, 15.015) && near(cols[0].1, 7.5075), "plain column {:?}", cols[0]);
    assert!(near(cols[2].0, 15.015) && near(cols[2].1, 7.5075), "{:?}", cols[2]);
    // 5-pt ruby raised 5: reaches 5 + 5.255 − 1.9 = 8.355 right of the centre.
    assert!(near(cols[1].0, 8.355 + 7.5075) && near(cols[1].1, 8.355), "5-pt ruby column {:?}", cols[1]);
    // 7-pt ruby raised 7: 7 + 7.357 − 2.66 = 11.697.
    assert!(near(cols[3].0, 11.697 + 7.5075) && near(cols[3].1, 11.697), "7-pt ruby column {:?}", cols[3]);
    // The 20-pt character's line, centred.
    assert!(near(cols[5].0, 28.6) && near(cols[5].1, 14.3), "20-pt column {:?}", cols[5]);
    // Drawn: every East Asian character stands centred on its column's centre line, the ruby
    // letters on theirs, `raise` to the right (above, before the turn) of the base's.
    let draws = display::page_display(&d, &l.pages[0], &Default::default());
    let turned: Vec<&Vec<display::Draw>> =
        draws.iter().filter_map(|x| if let display::Draw::Turned { items, .. } = x { Some(items) } else { None }).collect();
    let stood = |items: &[display::Draw]| -> Vec<(String, f32, f32)> {
        items
            .iter()
            .filter_map(|x| match x {
                display::Draw::Rotated { cy, items, .. } => match items.first() {
                    Some(display::Draw::Glyphs { text, size, .. }) => Some((text.clone(), *size, *cy)),
                    _ => None,
                },
                _ => None,
            })
            .collect()
    };
    let with_ruby = turned.iter().map(|i| stood(i)).find(|s| s.iter().any(|(t, _, _)| t == "か")).expect("the ruby paragraph");
    let base_cy = with_ruby.iter().find(|(t, _, _)| t == "漢").map(|x| x.2).unwrap();
    let ruby_cy = with_ruby.iter().filter(|(t, s, _)| t == "か" && *s == 5.0).map(|x| x.2).next().unwrap();
    assert!(near(base_cy, 8.355) && near(ruby_cy, 8.355 - 5.0), "base {base_cy} ruby {ruby_cy}");
    assert!(with_ruby.iter().filter(|(_, s, _)| *s == 10.5).all(|(_, _, cy)| near(*cy, base_cy)), "{with_ruby:?}");
    let big = turned.iter().map(|i| stood(i)).find(|s| s.iter().any(|(t, _, _)| t == "大")).expect("the 20-pt paragraph");
    assert!(big.iter().all(|(_, _, cy)| near(*cy, 14.3)), "{big:?}");
}

/// With Word's 18-pt line grid the same columns take whole grid lines, centred in them: the
/// 5-pt ruby's column one line (its centre 0.42 pt left of the line's middle, the ruby's reach
/// shared out), the 7-pt ruby's two (measured in Word for Mac: the base 20.4 pt from the pair's
/// right edge, the ruby 13.4).
#[test]
fn vertical_columns_take_whole_grid_lines_centred() {
    let Some(d) = ja_vertical_doc(&[None, Some((5.0, 5.0)), None, Some((7.0, 7.0)), None, Some((7.0, 3.0))], false, Some(18.0)) else { return };
    let cols = columns(&lay(&d));
    assert_eq!(cols.len(), 6, "{cols:?}");
    let near = |a: f32, b: f32| (a - b).abs() < 0.05;
    assert!(near(cols[0].0, 18.0) && near(cols[0].1, 9.0), "{:?}", cols[0]);
    assert!(near(cols[1].0, 18.0) && near(cols[1].1, 9.0 + (8.355 - 7.5075) / 2.0), "5-pt ruby {:?}", cols[1]);
    assert!(near(cols[3].0, 36.0) && near(cols[3].1, 18.0 + (11.697 - 7.5075) / 2.0), "7-pt ruby {:?}", cols[3]);
    // A 7-pt ruby raised only 3 fits its line: 3 + 7.357 − 2.66 = 7.697.
    assert!(near(cols[5].0, 18.0) && near(cols[5].1, 9.0 + (7.697 - 7.5075) / 2.0), "7-pt ruby raised 3 {:?}", cols[5]);
}

/// Underlines, strikethroughs and 傍点 in a 縦書き column, measured in Word for Mac (10.5-pt
/// 游明朝, 2026-10-19): an underline is a 0.72-pt line at every size whose near edge lies
/// 0.025 em beyond the standing em square on the page's right (5.88 pt from the column's centre
/// line); a double underline's second line lies 0.72 further out; a wave is a 0.24-pt zigzag of
/// 1.2-pt runs 0.72 peak to peak, its near peak 0.12 beyond that edge; a strikethrough runs
/// along the column's centre line at the font's thickness; a dot mark's centre sits 0.705 em
/// right of the centre line, a circle 0.97 em (0.13 em along, towards the text's end), an under
/// dot 0.585 em to the left.
#[test]
fn vertical_underlines_strikethroughs_and_marks_sit_as_word_for_mac_draws_them() {
    use wordcraft_doc::props::{EmphasisMark, Underline};
    if !wordcraft_fonts::word::resolve("游明朝", false, false).face.covers('語') {
        return;
    }
    let chr = |f: &dyn Fn(&mut CharProps)| {
        let mut p = CharProps {
            font: Some("游明朝".into()),
            font_ea: Some("游明朝".into()),
            size: Some(10.5),
            lang_ea: Some("ja-JP".into()),
            ..Default::default()
        };
        f(&mut p);
        p
    };
    let mut d = Document::from_text("");
    d.body.clear();
    let marked: Vec<CharProps> = vec![
        chr(&|p| p.underline = Some(Underline::Single)),
        chr(&|p| p.underline = Some(Underline::Double)),
        chr(&|p| p.underline = Some(Underline::Wave)),
        chr(&|p| p.strike = Some(true)),
        chr(&|p| p.em = Some(EmphasisMark::Dot)),
        chr(&|p| p.em = Some(EmphasisMark::Circle)),
        chr(&|p| p.em = Some(EmphasisMark::UnderDot)),
    ];
    for props in &marked {
        let mut para = Paragraph::with_text("前印後", chr(&|_| {}));
        para.props.line_spacing = Some(wordcraft_doc::props::LineSpacing::Multiple(1.0));
        para.runs = vec![
            wordcraft_doc::Run { len: "前".len(), props: chr(&|_| {}) },
            wordcraft_doc::Run { len: "印".len(), props: props.clone() },
            wordcraft_doc::Run { len: "後".len(), props: chr(&|_| {}) },
        ];
        d.body.push(wordcraft_doc::para_block(para));
    }
    d.last_section.text_direction = TextDirection::Down;
    let l = lay(&d);
    let draws = display::page_display(&d, &l.pages[0], &Default::default());
    // One turned frame per paragraph, in order: (centre line, lines (v, width, stroke), marks (u, v)).
    let frames: Vec<(f32, Vec<(f32, f32, display::Stroke)>, Vec<(f32, f32)>)> = draws
        .iter()
        .filter_map(|x| {
            let display::Draw::Turned { items, .. } = x else { return None };
            let base = items.iter().find_map(|i| match i {
                display::Draw::Rotated { cy, items, .. } if matches!(items.first(), Some(display::Draw::Glyphs { text, .. }) if text == "印") => {
                    Some(*cy)
                }
                _ => None,
            })?;
            let mut lines: Vec<(f32, f32, display::Stroke)> = items
                .iter()
                .filter_map(|i| match i {
                    display::Draw::Line { y0, y1, width, stroke, .. } if (y0 - y1).abs() < 1e-3 => Some((*y0 - base, *width, *stroke)),
                    _ => None,
                })
                .collect();
            lines.sort_by(|a, b| b.0.total_cmp(&a.0));
            let marks: Vec<(f32, f32)> = items
                .iter()
                .filter_map(|i| match i {
                    display::Draw::Shape { rect, fill, stroke, .. } if fill.is_some() || stroke.is_some() => {
                        Some((rect.x + rect.w / 2.0, rect.y + rect.h / 2.0 - base))
                    }
                    _ => None,
                })
                .collect();
            Some((base, lines, marks))
        })
        .collect();
    assert_eq!(frames.len(), marked.len(), "{frames:?}");
    let near = |a: f32, b: f32| (a - b).abs() < 0.02;
    // Underline: the near edge 0.525 em from the centre (towards the page's right, smaller v), 0.72 wide.
    let (_, v, _) = &frames[0];
    assert!(v.len() == 1 && near(v[0].0, -5.5125 - 0.36) && near(v[0].1, 0.72) && v[0].2 == display::Stroke::Solid, "underline {v:?}");
    let (_, v, _) = &frames[1];
    assert!(v.len() == 2 && near(v[0].0, -5.8725) && near(v[1].0, -5.8725 - 1.44) && near(v[1].1, 0.72), "double {v:?}");
    let (_, v, _) = &frames[2];
    assert!(v.len() == 1 && near(v[0].0, -5.5125 - 0.48) && near(v[0].1, 0.24), "wave {v:?}");
    assert!(matches!(v[0].2, display::Stroke::Zigzag { amp, step } if near(amp, 0.36) && near(step, 1.2)), "wave {v:?}");
    // Strikethrough: on the centre line, the font's thickness.
    let (_, v, _) = &frames[3];
    assert!(v.len() == 1 && near(v[0].0, 0.0) && near(v[0].1, 0.4725), "strike {v:?}");
    // Marks: one per marked character, beside the column.
    let (_, _, m) = &frames[4];
    assert!(m.len() == 1 && near(m[0].1, -0.705 * 10.5), "dot {m:?}");
    let (_, _, m) = &frames[5];
    assert!(m.len() == 1 && near(m[0].1, -0.97 * 10.5), "circle {m:?}");
    let (_, _, m) = &frames[6];
    assert!(m.len() == 1 && near(m[0].1, 0.585 * 10.5), "under dot {m:?}");
    // The circle sits 0.13 em further along the column than the character's centre, the dot on it.
    let dot_u = frames[4].2[0].0;
    let circle_u = frames[5].2[0].0;
    assert!(near(circle_u - dot_u, 0.13 * 10.5), "dot at {dot_u}, circle at {circle_u}");
}

/// A column's upright em squares sit half the font's win ascent − descent on the ascent side of
/// the lying Latin's baseline, which is the line's ascent from the column's top edge — so they
/// sit off the column's centre line by half the win-minus-hhea asymmetry, each run by its own
/// size: measured in Word for Mac on 2026-10-19 (`vt3`–`vt5` probes, 12 + 36 pt mixed in one
/// column, ungridded): Hiragino Mincho ProN (win 0.985/0.182, hhea 0.88/0.12) 0.0215 em toward
/// the ascent side, Toppan Bunkyu Mincho (1.295/0.326) 0.1045 em, Songti SC (0.86/0.14 against
/// hhea 1.06/0.34) and Apple SD Gothic Neo none; the Latin baseline (ascent − descent) / 2 from
/// the centre line. In the column's unturned frame, from its ascent edge: Hiragino's 36-pt
/// character centred 22.8 pt in (the 12-pt one 23.28), its 36-pt Latin on a baseline 37.2 in
/// (the 12-pt 28.08); Toppan's 19.92 / 22.32 and 37.2 / 28.08; Songti's 32.88 / 32.88 and
/// 45.84 / 37.2 — every column 46.8 wide (1.3 × 36), Songti's 65.52 (1.82 × 36).
#[test]
fn words_korean_fonts_stack_by_name_in_vertical_columns_as_word_for_mac_does() {
    // Word for Mac (kr3, kr5, vt3; 24 pt, no grid), whatever faces stand in: Malgun Gothic's
    // Hangul 25.5 pt apart with each baseline 22.01 below its pen (the lying 1 after 글 at 31.88:
    // a 6.375 space, a quarter of the pitch) and its columns 41.52 apart; Batang, GulimChe and MS
    // Mincho an em apart, 20.62/20.57 below the pen, the 1 at 30.0, columns 31.2 apart.
    // (family, text, baselines along the column, the lying 1's pen).
    let cases: Vec<(&str, &str, Vec<f32>, Option<f32>)> = vec![
        ("Malgun Gothic", "한국어행", vec![22.01, 47.51, 73.01, 98.51], None),
        ("Malgun Gothic", "글123행", vec![22.01], Some(31.875)),
        ("Batang", "한국어행", vec![20.62, 44.62, 68.62, 92.62], None),
        ("Batang", "글123행", vec![20.62], Some(30.0)),
        ("GulimChe", "한국어행", vec![20.62, 44.62, 68.62, 92.62], None),
        ("MS Mincho", "縦書き行", vec![20.57, 44.57, 68.57, 92.57], None),
    ];
    if !wordcraft_fonts::word::resolve("Batang", false, false).face.covers('한')
        || !wordcraft_fonts::word::resolve("MS Mincho", false, false).face.covers('縦')
    {
        return;
    }
    let mut d = Document::from_text("");
    d.body.clear();
    for (family, text, ..) in &cases {
        let chr = CharProps {
            font: Some((*family).into()),
            font_ea: Some((*family).into()),
            size: Some(24.0),
            lang_ea: Some("ko-KR".into()),
            ..Default::default()
        };
        let mut para = Paragraph::with_text(text, chr);
        para.props.line_spacing = Some(wordcraft_doc::props::LineSpacing::Multiple(1.0));
        para.props.space_before = Some(0.0);
        para.props.space_after = Some(0.0);
        d.body.push(wordcraft_doc::para_block(para));
    }
    d.last_section.text_direction = TextDirection::Down;
    let l = lay(&d);
    let draws = display::page_display(&d, &l.pages[0], &Default::default());
    let turned: Vec<(f32, &Vec<display::Draw>)> =
        draws.iter().filter_map(|x| if let display::Draw::Turned { x, items, .. } = x { Some((*x, items)) } else { None }).collect();
    assert_eq!(turned.len(), cases.len(), "one turned column per paragraph");
    let near = |a: f32, b: f32| (a - b).abs() < 0.3;
    // Columns 41.52 apart for Malgun Gothic (its 1.73-em line), 31.2 for the others.
    let pitch = |i: usize| (turned[i + 1].0 - turned[i].0).abs();
    assert!(near(pitch(0), 41.52) && near(pitch(2), 31.2), "column pitches {} {}", pitch(0), pitch(2));
    for ((_, items), (family, text, baselines, one)) in turned.iter().zip(cases.iter()) {
        let mut stood: Vec<f32> = Vec::new();
        let mut lying: Vec<(String, f32)> = Vec::new();
        for x in items.iter() {
            match x {
                display::Draw::Rotated { cx, cy, items, .. } => {
                    if let Some(display::Draw::Glyphs { glyphs, .. }) = items.first()
                        && let Some(g) = glyphs.first()
                    {
                        stood.push(cx + (g.2 - cy));
                    }
                }
                display::Draw::Glyphs { text, glyphs, .. } => {
                    if let Some(g) = glyphs.first() {
                        lying.push((text.clone(), g.1));
                    }
                }
                _ => {}
            }
        }
        for (i, want) in baselines.iter().enumerate() {
            assert!(stood.get(i).is_some_and(|b| near(*b, *want)), "{family} {text}: baselines {stood:?} (Word {baselines:?})");
        }
        if let Some(one) = one {
            let got = lying.iter().find(|(t, _)| t.starts_with('1')).map(|x| x.1);
            assert!(got.is_some_and(|x| near(x, *one)), "{family} {text}: 1 at {got:?} (Word {one})");
        }
    }
}

#[test]
fn vertical_columns_hang_glyphs_from_their_vertical_origins_as_word_for_mac_does() {
    // Measured in Word for Mac (vt6/vt7, 2026-10-19; 24 pt, no grid): an upright glyph's baseline
    // sits its vertical origin below the column's pen (VORG, else yMax + top side bearing), the
    // pen moves on by its vmtx advance (Apple SD Gothic Neo 1.2 em), the automatic space beside a
    // lying Latin run is a quarter of that advance (7.2 there, 6.0 elsewhere), and every glyph of
    // a run stands centred on one line across the column — its own advance box, so the last one
    // too. (family, text, first baseline, the lying 1's pen, last baseline) below the column top.
    let cases: Vec<(&str, &str, f32, f32, f32)> = vec![
        ("Apple SD Gothic Neo", "글123행", 21.6, 36.0, 99.02),
        ("AppleMyungjo", "글123행", 20.77, 30.0, 102.94),
        ("AppleGothic", "글123행", 20.33, 30.11, 105.76),
        ("Songti SC", "漢123字", 19.2, 30.0, 88.97),
        ("Hiragino Mincho ProN", "縦123行", 21.12, 30.0, 99.46),
    ];
    let mut d = Document::from_text("");
    d.body.clear();
    let mut used = Vec::new();
    for (family, text, ..) in &cases {
        let r = wordcraft_fonts::word::resolve(family, false, false);
        if r.substituted || !text.chars().all(|c| r.face.covers(c)) {
            continue;
        }
        used.push(*family);
        let chr = CharProps {
            font: Some((*family).into()),
            font_ea: Some((*family).into()),
            size: Some(24.0),
            lang_ea: Some("ja-JP".into()),
            ..Default::default()
        };
        let mut para = Paragraph::with_text(text, chr);
        para.props.line_spacing = Some(wordcraft_doc::props::LineSpacing::Multiple(1.0));
        para.props.space_before = Some(0.0);
        para.props.space_after = Some(0.0);
        d.body.push(wordcraft_doc::para_block(para));
    }
    if used.is_empty() {
        return;
    }
    d.last_section.text_direction = TextDirection::Down;
    let l = lay(&d);
    let draws = display::page_display(&d, &l.pages[0], &Default::default());
    let turned: Vec<&Vec<display::Draw>> =
        draws.iter().filter_map(|x| if let display::Draw::Turned { items, .. } = x { Some(items) } else { None }).collect();
    assert_eq!(turned.len(), used.len(), "one turned column per paragraph");
    let near = |a: f32, b: f32| (a - b).abs() < 0.3;
    for (items, (family, text, first, one, last)) in turned.iter().zip(cases.iter().filter(|c| used.contains(&c.0))) {
        // Stood glyphs: (text, baseline along the column, centre across it, centre less pen along it).
        let mut stood: Vec<(String, f32, f32, f32)> = Vec::new();
        let mut lying: Vec<(String, f32)> = Vec::new();
        for x in items.iter() {
            match x {
                display::Draw::Rotated { cx, cy, items, .. } => {
                    if let Some(display::Draw::Glyphs { text, glyphs, face, size, .. }) = items.first()
                        && let Some(g) = glyphs.first()
                    {
                        let hadv = face.advance(g.0) as f32 * size / face.upem as f32;
                        stood.push((text.clone(), cx + (g.2 - cy), cy - (g.1 - cx), (cx - g.1) - hadv / 2.0));
                    }
                }
                display::Draw::Glyphs { text, glyphs, .. } => {
                    if let Some(g) = glyphs.first() {
                        lying.push((text.clone(), g.1));
                    }
                }
                _ => {}
            }
        }
        let (Some(a), Some(z)) = (stood.first(), stood.last()) else { panic!("{family}: {stood:?}") };
        let Some(one_x) = lying.iter().find(|(t, _)| t.starts_with('1')).map(|x| x.1) else { panic!("{family}: lying {lying:?}") };
        assert!(near(a.1, *first), "{family} {text}: first baseline {} (Word {first})", a.1);
        assert!(near(one_x, *one), "{family} {text}: 1 at {one_x} (Word {one})");
        assert!(near(z.1, *last), "{family} {text}: last baseline {} (Word {last})", z.1);
        assert!(near(a.2, z.2) && a.3.abs() < 0.05 && z.3.abs() < 0.05, "{family} {text}: across {} {} (box offsets {} {})", a.2, z.2, a.3, z.3);
    }
}

#[test]
fn vertical_em_squares_follow_their_fonts_win_metrics_as_word_for_mac_places_them() {
    let mut d = Document::from_text("");
    d.body.clear();
    let fonts: Vec<(&str, &str, &str, &str, f32, f32, f32, f32, f32)> = vec![
        // family, small, big, last, column width, big centre, small centre, big Latin baseline, small Latin baseline
        ("Hiragino Mincho ProN", "縦", "大", "行", 46.8, 22.8, 23.28, 37.2, 28.08),
        ("Toppan Bunkyu Mincho", "縦", "大", "行", 46.8, 19.92, 22.32, 37.2, 28.08),
        ("Songti SC", "中", "大", "行", 65.52, 32.88, 32.88, 45.84, 37.2),
    ];
    let mut used = Vec::new();
    for (family, a, b, c, ..) in &fonts {
        let r = wordcraft_fonts::word::resolve(family, false, false);
        if r.substituted || !r.face.covers(b.chars().next().unwrap()) {
            continue;
        }
        used.push(*family);
        let chr = |size: f32| CharProps {
            font: Some((*family).into()),
            font_ea: Some((*family).into()),
            size: Some(size),
            lang_ea: Some("ja-JP".into()),
            ..Default::default()
        };
        let text = format!("{a}Ab{b}Cd{c}");
        let mut para = Paragraph::with_text(&text, chr(12.0));
        para.props.line_spacing = Some(wordcraft_doc::props::LineSpacing::Multiple(1.0));
        para.props.space_before = Some(0.0);
        para.props.space_after = Some(0.0);
        para.runs = vec![
            wordcraft_doc::Run { len: a.len() + "Ab".len(), props: chr(12.0) },
            wordcraft_doc::Run { len: b.len() + "Cd".len(), props: chr(36.0) },
            wordcraft_doc::Run { len: c.len(), props: chr(12.0) },
        ];
        d.body.push(wordcraft_doc::para_block(para));
    }
    if used.is_empty() {
        return;
    }
    d.last_section.text_direction = TextDirection::Down;
    let l = lay(&d);
    let draws = display::page_display(&d, &l.pages[0], &Default::default());
    let turned: Vec<&Vec<display::Draw>> =
        draws.iter().filter_map(|x| if let display::Draw::Turned { items, .. } = x { Some(items) } else { None }).collect();
    assert_eq!(turned.len(), used.len(), "one turned column per paragraph");
    let near = |a: f32, b: f32| (a - b).abs() < 0.3;
    for (items, (family, a, b, _c, width, big_c, small_c, big_base, small_base)) in turned.iter().zip(fonts.iter().filter(|f| used.contains(&f.0))) {
        let mut stood: Vec<(String, f32, f32)> = Vec::new();
        let mut lying: Vec<(String, f32, f32)> = Vec::new();
        for x in items.iter() {
            match x {
                display::Draw::Rotated { cy, items, .. } => {
                    if let Some(display::Draw::Glyphs { text, size, .. }) = items.first() {
                        stood.push((text.clone(), *size, *cy));
                    }
                }
                display::Draw::Glyphs { text, size, glyphs, .. } => {
                    if let Some(g) = glyphs.first() {
                        lying.push((text.clone(), *size, g.2));
                    }
                }
                _ => {}
            }
        }
        let find = |v: &[(String, f32, f32)], t: &str, s: f32| v.iter().find(|(x, sz, _)| x == t && (*sz - s).abs() < 0.01).map(|x| x.2);
        let (cb, cs) = (find(&stood, b, 36.0), find(&stood, a, 12.0));
        let (lb, ls) = (
            lying.iter().find(|(t, s, _)| t.starts_with('C') && (*s - 36.0).abs() < 0.01).map(|x| x.2),
            lying.iter().find(|(t, s, _)| t.starts_with('A') && (*s - 12.0).abs() < 0.01).map(|x| x.2),
        );
        let (Some(cb), Some(cs), Some(lb), Some(ls)) = (cb, cs, lb, ls) else {
            panic!("{family}: stood {stood:?} lying {lying:?}");
        };
        // The column's width is the 36-pt run's line.
        let cols = columns(&l);
        assert!(cols.iter().any(|(h, _)| near(*h, *width)), "{family}: columns {cols:?} expected one {width} wide");
        assert!(near(cb, *big_c) && near(cs, *small_c), "{family}: 36-pt centre {cb} (Word {big_c}), 12-pt {cs} (Word {small_c})");
        assert!(
            near(lb, *big_base) && near(ls, *small_base),
            "{family}: 36-pt Latin baseline {lb} (Word {big_base}), 12-pt {ls} (Word {small_base})"
        );
    }
}

/// Word for Mac (`hw2`, 2026-10-19; 24 pt, no grid): 縦AaWi1 ｱ行 in MS Mincho and MS Gothic — the
/// lying A starts 6 pt (the automatic space) after 縦's em, then every character 12 pt further,
/// the space and the half-width ｱ too, and 行 stands 12 pt after ｱ: pens at 30, 42, …, 102, 114.
#[test]
fn half_width_characters_lie_half_an_em_apart_along_a_column_as_word_for_mac_sets_them() {
    for font in ["MS Mincho", "MS Gothic"] {
        let r = wordcraft_fonts::word::resolve(font, false, false);
        if !r.face.covers('縦') {
            continue;
        }
        let chr = CharProps { font: Some(font.into()), font_ea: Some(font.into()), size: Some(24.0), ..Default::default() };
        let mut d = Document::from_text("");
        d.body.clear();
        let mut para = Paragraph::with_text("縦AaWi1 ｱ行", chr);
        para.props.line_spacing = Some(wordcraft_doc::props::LineSpacing::Multiple(1.0));
        d.body.push(wordcraft_doc::para_block(para));
        d.last_section.text_direction = TextDirection::Down;
        let l = lay(&d);
        let Some(Placed::Lines { para, .. }) = l.pages[0].items.iter().find(|i| matches!(i, Placed::Lines { .. })) else { panic!("no text") };
        let line = &para.lines[0];
        let lefts: Vec<f32> = (0..para.clusters.len()).map(|k| line.cl_left(k).unwrap() - line.cl_left(0).unwrap()).collect();
        let want = [0.0, 30.0, 42.0, 54.0, 66.0, 78.0, 90.0, 102.0, 114.0];
        for (k, w) in want.iter().enumerate() {
            assert!((lefts[k] - w).abs() < 0.1, "{font}: cluster {k} at {} along the column (Word {w}); {lefts:?}", lefts[k]);
        }
    }
}

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

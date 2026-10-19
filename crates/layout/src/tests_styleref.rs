//! STYLEREF fields in headers and footers (#775): each page shows its own result.

use super::*;
use wordcraft_doc::para::InlineObject;
use wordcraft_doc::props::{CharProps, NumRef};
use wordcraft_doc::{ListKind, PartKind, Pos, para_block};

fn field(instr: &str) -> InlineObject {
    InlineObject::Field { instr: instr.into(), result: "stale".into(), locked: false, dirty: false, code: false, form: String::new() }
}

/// Body paragraphs: `(style, text, page break before)`; a header with one STYLEREF per code.
fn doc(paras: &[(&str, &str, bool)], codes: &[&str]) -> Document {
    let mut d = Document::from_text(&paras.iter().map(|p| p.1).collect::<Vec<_>>().join("\n"));
    for (i, (style, _, brk)) in paras.iter().enumerate() {
        let p = d.para_mut(StoryRef::Body, &Path::top(i)).unwrap();
        p.props.style = Some((*style).to_string());
        p.props.page_break_before = Some(*brk);
    }
    let blocks = codes
        .iter()
        .map(|c| {
            let mut p = Paragraph::new();
            p.insert_object(0, field(c), &CharProps::default()).unwrap();
            para_block(p)
        })
        .collect();
    let id = d.add_part(PartKind::Header, blocks);
    d.last_section.headers.default = Some(id);
    d
}

fn shown(items: &[Placed]) -> Vec<String> {
    items.iter().flat_map(|it| if let Placed::Lines { para, .. } = it { para.shown.iter().map(|s| s.1.clone()).collect() } else { vec![] }).collect()
}

fn headers(d: &Document) -> Vec<Vec<String>> {
    let mut c = LayoutCache::new();
    layout(d, &mut c, &LayoutOptions::default()).pages.iter().map(|p| shown(&p.header)).collect()
}

const CHAPTERS: &[(&str, &str, bool)] = &[
    ("Heading1", "Beginnings", false),
    ("Normal", "Text of one.", false),
    ("Heading1", "Middles", true),
    ("Normal", "Text of two.", false),
    // A page with no heading: the header keeps the chapter before it.
    ("Normal", "More of two.", true),
    ("Heading1", "Endings", true),
    ("Heading2", "A section", false),
];

#[test]
fn each_page_s_header_shows_its_chapter() {
    let d = doc(CHAPTERS, &[r#"STYLEREF "Heading 1" \* MERGEFORMAT"#, "STYLEREF 1", "STYLEREF heading1 \\l"]);
    let h = headers(&d);
    assert_eq!(h.len(), 4);
    for (page, want) in ["Beginnings", "Middles", "Middles", "Endings"].iter().enumerate() {
        assert_eq!(h[page], [*want, *want, *want], "page {}", page + 1);
    }
}

#[test]
fn search_from_the_bottom_with_l() {
    let paras = [("Heading2", "First", false), ("Heading2", "Second", false), ("Normal", "Body", false), ("Heading2", "Third", true)];
    let d = doc(&paras, &[r#"STYLEREF "Heading 2""#, r#"STYLEREF "Heading 2" \l"#]);
    assert_eq!(headers(&d), [vec!["First", "Second"], vec!["Third", "Third"]]);
}

#[test]
fn a_missing_style_or_no_paragraph_of_it_is_an_error() {
    let d = doc(CHAPTERS, &[r#"STYLEREF "No Such Style""#, r#"STYLEREF "Heading 3""#, "STYLEREF 12"]);
    let err = styleref::NO_TEXT;
    assert_eq!(headers(&d)[0], [err, err, err]);
    assert_eq!(err, "Error: no paragraph in this style.");
}

#[test]
fn before_the_first_heading_the_next_one_shows() {
    let paras = [("Normal", "Preface", false), ("Heading1", "Chapter", true)];
    let d = doc(&paras, &[r#"STYLEREF "Heading 1""#, r#"STYLEREF "Heading 1" \p"#]);
    assert_eq!(headers(&d), [vec!["Chapter", "below"], vec!["Chapter", "below"]]);
}

#[test]
fn character_styles_and_paragraph_numbers() {
    let mut d = doc(CHAPTERS, &[r#"STYLEREF Strong"#, r#"STYLEREF "Heading 1" \n"#, r#"STYLEREF "Heading 2" \w"#, r#"STYLEREF "Heading 1" \s"#]);
    // Bold a word on page 2 with the Strong character style.
    d.format_range(&Pos::body(3, 8), &Pos::body(3, 11), &|c: &mut CharProps| c.style = Some("Strong".into())).unwrap();
    // Number the headings 1., 2., 3. and the section under them 3.1 (full context).
    let num = d.numbering.add_list(ListKind::Numbered);
    for (i, lvl) in [(0, 0u8), (2, 0), (5, 0), (6, 1)] {
        d.para_mut(StoryRef::Body, &Path::top(i)).unwrap().props.numbering = Some(NumRef { num, level: lvl });
    }
    let h = headers(&d);
    assert_eq!(h[1], ["two", "2", "3.a", "2"]);
    assert_eq!(h[3], ["two", "3", "3.a", "3"]);
}

/// Editing a heading updates the headers that show it, through the paragraph cache.
#[test]
fn headers_follow_heading_edits() {
    let mut d = doc(CHAPTERS, &[r#"STYLEREF "Heading 1""#]);
    let mut c = LayoutCache::new();
    let first = layout(&d, &mut c, &LayoutOptions::default());
    assert_eq!(shown(&first.pages[1].header), ["Middles"]);
    let p = d.para_mut(StoryRef::Body, &Path::top(2)).unwrap();
    p.insert_text(0, "The ", &CharProps::default()).unwrap();
    let again = layout(&d, &mut c, &LayoutOptions::default());
    assert_eq!(shown(&again.pages[1].header), ["The Middles"]);
    assert_eq!(shown(&again.pages[0].header), ["Beginnings"]);
}

/// The style index is built once and reused while the body and the styles stay the same (long
/// documents lay out on every keystroke); an edited heading or a changed style rebuilds it.
#[test]
fn style_index_is_reused_until_the_body_or_styles_change() {
    let mut d = doc(CHAPTERS, &[r#"STYLEREF "Heading 1""#]);
    let mut c = LayoutCache::new();
    let index = |c: &LayoutCache| c.styleref.as_ref().map(|(_, _, ix)| ix.clone()).unwrap();
    let pages =
        |d: &Document, c: &mut LayoutCache| layout(d, c, &LayoutOptions::default()).pages.iter().map(|p| shown(&p.header)).collect::<Vec<_>>();
    let before = pages(&d, &mut c);
    let first = index(&c);
    assert_eq!(pages(&d, &mut c), before);
    assert!(Arc::ptr_eq(&first, &index(&c)), "nothing changed: the same index");
    // Rename the first chapter: rebuilt, and the header shows the new title.
    let p = d.para_mut(StoryRef::Body, &Path::top(0)).unwrap();
    p.text = "Openings".into();
    p.runs = vec![wordcraft_doc::para::Run { len: p.text.len(), props: Default::default() }];
    let after = pages(&d, &mut c);
    assert!(!Arc::ptr_eq(&first, &index(&c)), "an edited heading rebuilds it");
    assert!(after[0].iter().any(|t| t.contains("Openings")), "{after:?}");
    // A style change rebuilds it too.
    let second = index(&c);
    if let Some(st) = d.styles.styles.iter_mut().find(|s| s.id == "Heading1") {
        st.name = "Chapter Title".into();
    }
    let _ = pages(&d, &mut c);
    assert!(!Arc::ptr_eq(&second, &index(&c)), "a style change rebuilds it");
}

/// `STYLEREF 1` finds the style named `heading 1` when its id is localized (#1286).
#[test]
fn heading_level_matches_a_localized_style_id() {
    let paras: Vec<(&str, &str, bool)> = CHAPTERS.iter().map(|(s, t, b)| (if *s == "Heading1" { "berschrift1" } else { *s }, *t, *b)).collect();
    let mut d = doc(&paras, &["STYLEREF 1"]);
    let st = d.styles.styles.iter_mut().find(|s| s.id == "Heading1").unwrap();
    st.id = "berschrift1".into();
    st.name = "heading 1".into();
    let h = headers(&d);
    assert_eq!(h[0], ["Beginnings"]);
    assert_eq!(h[3], ["Endings"]);
}

/// Typing in body text that no STYLEREF shows keeps every header paragraph cached (#1286); an
/// edited heading still updates the headers that show it.
#[test]
fn body_edits_keep_styleref_headers_cached() {
    let mut d = doc(CHAPTERS, &[r#"STYLEREF "Heading 1""#, r#"STYLEREF "Heading 1" \p"#]);
    let mut c = LayoutCache::new();
    let opts = LayoutOptions::default();
    let _ = layout(&d, &mut c, &opts);
    // Type in a body paragraph: only that paragraph is laid out again.
    d.para_mut(StoryRef::Body, &Path::top(1)).unwrap().insert_text(0, "More ", &CharProps::default()).unwrap();
    let misses = c.misses;
    let again = layout(&d, &mut c, &opts);
    assert_eq!(c.misses, misses + 1, "only the edited body paragraph misses the cache");
    assert_eq!(shown(&again.pages[1].header), ["Middles", "below"]);
    // Edit a heading: the headers showing it change.
    d.para_mut(StoryRef::Body, &Path::top(2)).unwrap().insert_text(0, "The ", &CharProps::default()).unwrap();
    let edited = layout(&d, &mut c, &opts);
    assert_eq!(shown(&edited.pages[1].header), ["The Middles", "below"]);
    // Page 3 has no heading: the one before it shows, above the header.
    assert_eq!(shown(&edited.pages[2].header), ["The Middles", "above"]);
    assert_eq!(shown(&edited.pages[0].header), ["Beginnings", "below"]);
}

/// Page numbers that include the chapter number, as Word for Mac shows them (`ch3`–`ch6`
/// probes, 2026-10-19): `Heading 1` numbered through its style by Word's 第１章 list (a
/// full-width level), so a PAGE field shows the level's number, the separator and the page
/// number — `１-1`, `１-2`, `２-3`, `２-4`: page numbers go on; a footer takes the last heading on
/// the page (`２-1` where chapter 2 starts below chapter 1's text), a body field the chapter at
/// its paragraph, a preface before the first chapter the first chapter's number; a period
/// separator gives `１.1`; a heading numbered paragraph by paragraph gives no chapter number. The
/// body's PAGE fields also show the section's number format (`ii`).
#[test]
fn page_numbers_include_the_chapter_number_as_word_for_mac_does() {
    use wordcraft_doc::section::{ChapterNumbers, ChapterSep, NumFormat};
    let paras =
        [("Normal", "Preface", false), ("Heading1", "One", true), ("Normal", "Text", false), ("Heading1", "Two", true), ("Normal", "More", true)];
    let mut d = doc(&paras, &["PAGE"]);
    for i in [0, 2, 4] {
        d.para_mut(StoryRef::Body, &Path::top(i)).unwrap().insert_object(0, field("PAGE"), &CharProps::default()).unwrap();
    }
    let num = d.numbering.add_list(ListKind::JaChapter);
    d.styles.get_mut("Heading1").unwrap().para.numbering = Some(NumRef { num, level: 0 });
    d.last_section.page_num_chapter = Some(ChapterNumbers { level: 1, sep: ChapterSep::Hyphen });
    let body = |d: &Document| {
        let mut c = LayoutCache::new();
        layout(d, &mut c, &LayoutOptions::default()).pages.iter().map(|p| shown(&p.items).concat()).collect::<Vec<_>>()
    };
    assert_eq!(headers(&d), [vec!["１-1"], vec!["１-2"], vec!["２-3"], vec!["２-4"]]);
    assert_eq!(body(&d), ["１-1", "１-2", "", "２-4"]);
    d.last_section.page_num_chapter = Some(ChapterNumbers { level: 1, sep: ChapterSep::Period });
    assert_eq!(headers(&d)[0], ["１.1"]);
    assert_eq!(body(&d)[3], "２.4");
    // Numbered paragraph by paragraph, not through the style: Word shows no chapter number.
    d.styles.get_mut("Heading1").unwrap().para.numbering = None;
    for i in [1, 3] {
        d.para_mut(StoryRef::Body, &Path::top(i)).unwrap().props.numbering = Some(NumRef { num, level: 0 });
    }
    assert_eq!(headers(&d), [vec!["1"], vec!["2"], vec!["3"], vec!["4"]]);
    // No chapter numbers: the body shows the section's format like the header does.
    d.last_section.page_num_chapter = None;
    d.last_section.page_num_format = NumFormat::LowerRoman;
    assert_eq!(body(&d), ["i", "ii", "", "iv"]);
    assert_eq!(headers(&d)[2], ["iii"]);

    // Two chapters starting on one page: the footer shows the last one's number, the body
    // fields the chapter at each paragraph.
    let paras =
        [("Normal", "Preface", false), ("Heading1", "One", false), ("Normal", "Text", false), ("Heading1", "Two", false), ("Normal", "More", false)];
    let mut d = doc(&paras, &["PAGE"]);
    for i in [0, 2, 4] {
        d.para_mut(StoryRef::Body, &Path::top(i)).unwrap().insert_object(0, field("PAGE"), &CharProps::default()).unwrap();
    }
    let num = d.numbering.add_list(ListKind::JaChapter);
    d.styles.get_mut("Heading1").unwrap().para.numbering = Some(NumRef { num, level: 0 });
    d.last_section.page_num_chapter = Some(ChapterNumbers { level: 1, sep: ChapterSep::Hyphen });
    assert_eq!(headers(&d), [vec!["２-1"]]);
    assert_eq!(body(&d), ["１-1１-1２-1"]);
}

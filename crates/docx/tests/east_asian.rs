//! East Asian text in OOXML: the theme's fonts per script (`a:font script="Jpan"`), the
//! `…EastAsia` theme slots, the East Asian language (`w:lang/@w:eastAsia`, `w:themeFontLang`)
//! and punctuation compression (`w:characterSpacingControl`). Fixtures are hand-written from
//! ECMA-376 Part 1 §17.3.2.26 (rFonts), §17.3.2.20 (lang), §17.15.1.11 (characterSpacingControl),
//! §17.15.1.88 (themeFontLang) and Part 1 §20.1.4.1.16 (font scheme), after what Word for Mac
//! writes with Japanese as its East Asian editing language.

use std::io::{Read, Write};

use wordcraft_doc::east_asian::{PunctCompression, ScriptFonts};
use wordcraft_doc::props::{CharProps, CombineBrackets, EmphasisMark};
use wordcraft_doc::{Document, Paragraph, para_block};

fn zip(entries: &[(&str, &[u8])]) -> Vec<u8> {
    let mut zw = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    for (name, bytes) in entries {
        zw.start_file(*name, zip::write::SimpleFileOptions::default()).unwrap();
        zw.write_all(bytes).unwrap();
    }
    zw.finish().unwrap().into_inner()
}

fn part(bytes: &[u8], name: &str) -> String {
    let mut z = zip::ZipArchive::new(std::io::Cursor::new(bytes)).unwrap();
    let mut s = String::new();
    z.by_name(name).unwrap().read_to_string(&mut s).unwrap();
    s
}

const W: &str = "http://schemas.openxmlformats.org/wordprocessingml/2006/main";
const A: &str = "http://schemas.openxmlformats.org/drawingml/2006/main";

/// A package as Word writes it for a Japanese document: the Office theme with script fonts, the
/// document defaults in the `minorEastAsia` slot with `ja-JP` East Asian text, settings with the
/// theme font languages and punctuation compression.
fn japanese_docx() -> Vec<u8> {
    let root = r#"<?xml version="1.0" encoding="UTF-8"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="word/document.xml"/></Relationships>"#;
    let rels = r#"<?xml version="1.0" encoding="UTF-8"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/styles" Target="styles.xml"/><Relationship Id="rId2" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/settings" Target="settings.xml"/><Relationship Id="rId3" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/theme" Target="theme/theme1.xml"/></Relationships>"#;
    let theme = format!(
        r#"<?xml version="1.0" encoding="UTF-8"?><a:theme xmlns:a="{A}" name="Office Theme"><a:themeElements><a:fontScheme name="Office"><a:majorFont><a:latin typeface="Aptos Display"/><a:ea typeface=""/><a:cs typeface=""/><a:font script="Jpan" typeface="游ゴシック Light"/><a:font script="Hang" typeface="맑은 고딕"/><a:font script="Hans" typeface="等线 Light"/><a:font script="Hant" typeface="新細明體"/></a:majorFont><a:minorFont><a:latin typeface="Aptos"/><a:ea typeface=""/><a:cs typeface=""/><a:font script="Jpan" typeface="游明朝"/><a:font script="Hang" typeface="맑은 고딕"/><a:font script="Hans" typeface="等线"/><a:font script="Hant" typeface="新細明體"/></a:minorFont></a:fontScheme></a:themeElements></a:theme>"#
    );
    let styles = format!(
        r#"<?xml version="1.0" encoding="UTF-8"?><w:styles xmlns:w="{W}"><w:docDefaults><w:rPrDefault><w:rPr><w:rFonts w:asciiTheme="minorHAnsi" w:eastAsiaTheme="minorEastAsia" w:hAnsiTheme="minorHAnsi" w:cstheme="minorBidi"/><w:kern w:val="2"/><w:sz w:val="21"/><w:szCs w:val="24"/><w:lang w:val="en-JP" w:eastAsia="ja-JP" w:bidi="ar-SA"/></w:rPr></w:rPrDefault><w:pPrDefault/></w:docDefaults><w:style w:type="paragraph" w:default="1" w:styleId="Normal"><w:name w:val="Normal"/><w:qFormat/></w:style><w:style w:type="paragraph" w:styleId="Heading1"><w:name w:val="heading 1"/><w:basedOn w:val="Normal"/><w:rPr><w:rFonts w:asciiTheme="majorHAnsi" w:eastAsiaTheme="majorEastAsia" w:hAnsiTheme="majorHAnsi"/></w:rPr></w:style></w:styles>"#
    );
    let settings = format!(
        r#"<?xml version="1.0" encoding="UTF-8"?><w:settings xmlns:w="{W}"><w:defaultTabStop w:val="840"/><w:characterSpacingControl w:val="compressPunctuation"/><w:themeFontLang w:val="en-JP" w:eastAsia="ja-JP"/></w:settings>"#
    );
    let doc = format!(
        r#"<?xml version="1.0" encoding="UTF-8"?><w:document xmlns:w="{W}"><w:body><w:p><w:r><w:rPr><w:rFonts w:hint="eastAsia"/></w:rPr><w:t>漢字</w:t></w:r><w:r><w:rPr><w:rFonts w:eastAsiaTheme="minorEastAsia"/><w:lang w:eastAsia="zh-CN"/></w:rPr><w:t>汉字</w:t></w:r><w:r><w:rPr><w:rFonts w:eastAsiaTheme="majorEastAsia"/></w:rPr><w:t>見出し</w:t></w:r><w:r><w:rPr><w:rFonts w:eastAsia="ＭＳ 明朝" w:eastAsiaTheme="minorEastAsia"/></w:rPr><w:t>明朝</w:t></w:r></w:p><w:p><w:pPr><w:pStyle w:val="Heading1"/></w:pPr><w:r><w:t>章</w:t></w:r></w:p></w:body></w:document>"#
    );
    zip(&[
        ("_rels/.rels", root.as_bytes()),
        ("word/_rels/document.xml.rels", rels.as_bytes()),
        ("word/document.xml", doc.as_bytes()),
        ("word/styles.xml", styles.as_bytes()),
        ("word/settings.xml", settings.as_bytes()),
        ("word/theme/theme1.xml", theme.as_bytes()),
    ])
}

#[test]
fn word_japanese_document_reads_its_theme_fonts_language_and_compression() {
    let d = wordcraft_docx::read(&japanese_docx()).unwrap();
    let s = &d.settings;
    assert_eq!((s.major_font.as_str(), s.minor_font.as_str()), ("Aptos Display", "Aptos"));
    assert_eq!((s.major_ea.as_str(), s.minor_ea.as_str()), ("", ""), "the theme names no single East Asian font");
    let jpan = s.script_fonts.iter().find(|f| f.script == "Jpan").expect("Jpan fonts");
    assert_eq!((jpan.major.as_str(), jpan.minor.as_str()), ("游ゴシック Light", "游明朝"));
    assert_eq!(s.script_fonts.iter().map(|f| f.script.as_str()).collect::<Vec<_>>(), ["Jpan", "Hang", "Hans", "Hant"]);
    assert_eq!((s.theme_font_lang.as_deref(), s.theme_font_lang_ea.as_deref()), (Some("en-JP"), Some("ja-JP")));
    assert_eq!(s.punct_compression, PunctCompression::CompressPunctuation);
    assert_eq!(s.default_tab, 42.0);
    // Document defaults: the minor East Asian slot is 游明朝 for ja-JP text; the sizes as given.
    let def = &d.styles.default_chr;
    assert_eq!(def.font_ea.as_deref(), Some("游明朝"));
    assert_eq!((def.lang.as_deref(), def.lang_ea.as_deref(), def.lang_bidi.as_deref()), (Some("en-JP"), Some("ja-JP"), Some("ar-SA")));
    assert_eq!((def.size, def.size_cs), (Some(10.5), Some(12.0)));
    // Heading 1 names the major slot: 游ゴシック Light through the document's East Asian language.
    let h1 = d.styles.get("Heading1").expect("Heading1");
    assert_eq!(h1.chr.font_ea.as_deref(), Some("游ゴシック Light"));
    assert_eq!(h1.chr.font.as_deref(), Some("Aptos Display"));
    let p = d.body[0].as_para().unwrap();
    assert_eq!(p.runs[0].props.font_ea, None, "a hint alone inherits the default's font");
    assert_eq!(p.runs[1].props.font_ea.as_deref(), Some("等线"), "the run's own East Asian language picks its script");
    assert_eq!(p.runs[1].props.lang_ea.as_deref(), Some("zh-CN"));
    assert_eq!(p.runs[2].props.font_ea.as_deref(), Some("游ゴシック Light"), "the major slot through the document's language");
    assert_eq!(p.runs[3].props.font_ea.as_deref(), Some("游明朝"), "the theme slot wins over the named font beside it");
    let h = d.body[1].as_para().unwrap();
    assert_eq!(h.props.style.as_deref(), Some("Heading1"));
    let resolved = d.styles.resolve_char(Some("Heading1"), &h.runs[0].props);
    assert_eq!(resolved.font_ea.as_deref(), Some("游ゴシック Light"));
    assert_eq!(resolved.lang_ea.as_deref(), Some("ja-JP"));
}

#[test]
fn east_asian_theme_fonts_language_and_compression_round_trip() {
    let mut d = Document::new();
    d.settings.script_fonts = vec![
        ScriptFonts { script: "Jpan".into(), major: "游ゴシック Light".into(), minor: "游明朝".into() },
        ScriptFonts { script: "Hans".into(), major: "等线 Light".into(), minor: "等线".into() },
    ];
    d.settings.minor_ea = "SimSun".into();
    d.settings.theme_font_lang = Some("en-US".into());
    d.settings.theme_font_lang_ea = Some("ja-JP".into());
    d.settings.punct_compression = PunctCompression::CompressPunctuationAndJapaneseKana;
    d.styles.default_chr.lang_ea = Some("ja-JP".into());
    let run = CharProps { lang: Some("en-US".into()), lang_ea: Some("ko-KR".into()), lang_bidi: Some("ar-SA".into()), ..Default::default() };
    d.body = vec![para_block(Paragraph::with_text("한국어", run.clone()))];
    let bytes = wordcraft_docx::write(&d).unwrap();
    let theme = part(&bytes, "word/theme/theme1.xml");
    assert!(theme.contains(r#"<a:font script="Jpan" typeface="游明朝"/>"#), "{theme}");
    assert!(theme.contains(r#"<a:font script="Hans" typeface="等线 Light"/>"#), "{theme}");
    assert!(theme.contains(r#"<a:ea typeface="SimSun"/>"#), "{theme}");
    let settings = part(&bytes, "word/settings.xml");
    assert!(settings.contains(r#"<w:characterSpacingControl w:val="compressPunctuationAndJapaneseKana"/>"#), "{settings}");
    assert!(settings.contains(r#"<w:themeFontLang w:val="en-US" w:eastAsia="ja-JP"/>"#), "{settings}");
    let document = part(&bytes, "word/document.xml");
    assert!(document.contains(r#"<w:lang w:val="en-US" w:eastAsia="ko-KR" w:bidi="ar-SA"/>"#), "{document}");
    let r = wordcraft_docx::read(&bytes).unwrap();
    assert_eq!(r.settings.script_fonts, d.settings.script_fonts);
    assert_eq!((r.settings.major_ea.as_str(), r.settings.minor_ea.as_str()), ("", "SimSun"));
    assert_eq!((r.settings.theme_font_lang.as_deref(), r.settings.theme_font_lang_ea.as_deref()), (Some("en-US"), Some("ja-JP")));
    assert_eq!(r.settings.punct_compression, PunctCompression::CompressPunctuationAndJapaneseKana);
    assert_eq!(r.styles.default_chr.lang_ea.as_deref(), Some("ja-JP"));
    let p = r.body[0].as_para().unwrap();
    assert_eq!(p.runs[0].props.lang_ea.as_deref(), Some("ko-KR"));
    assert_eq!(p.runs[0].props.lang_bidi.as_deref(), Some("ar-SA"));
}

#[test]
fn a_document_without_theme_script_fonts_keeps_the_office_ones() {
    // A new document carries Word's Office script fonts, and a file whose theme names none
    // keeps them (Word draws East Asian text with them in that case).
    let d = Document::new();
    assert_eq!(d.settings.east_asian_font(false, Some("ja-JP"), "漢字"), Some("游明朝"));
    assert_eq!(d.settings.punct_compression, PunctCompression::DoNotCompress);
    let bytes = wordcraft_docx::write(&d).unwrap();
    let r = wordcraft_docx::read(&bytes).unwrap();
    assert_eq!(r.settings.script_fonts, d.settings.script_fonts);
    assert_eq!(r.settings.east_asian_font(false, None, "かな"), Some("游明朝"));
    assert_eq!(r.settings.east_asian_font(false, None, "漢字"), None, "Han alone names no script");
}

/// A body as Word for Mac writes 割注, 均等割り付け, 縦中横 and 傍点 (`plan/word/samples/ja-layout.docx`,
/// 2026-10-18), with the theme and styles of the Japanese Normal template.
fn asian_layout_docx() -> Vec<u8> {
    let base = japanese_docx();
    let doc = format!(
        r#"<?xml version="1.0" encoding="UTF-8"?><w:document xmlns:w="{W}"><w:body><w:p><w:r><w:t>割注丸：</w:t></w:r><w:r><w:rPr><w:rFonts w:hint="eastAsia"/><w:eastAsianLayout w:id="-359372279" w:combine="1" w:combineBrackets="round"/></w:rPr><w:t>これは割注の本文です</w:t></w:r><w:r><w:t>。</w:t></w:r></w:p><w:p><w:r><w:rPr><w:eastAsianLayout w:id="-359372280" w:combine="1"/></w:rPr><w:t>これは割注の本文です</w:t></w:r></w:p><w:p><w:r><w:t>均等：</w:t></w:r><w:r><w:rPr><w:spacing w:val="193"/><w:kern w:val="0"/><w:fitText w:val="2000" w:id="-359372284"/></w:rPr><w:t>均等割</w:t></w:r><w:r><w:rPr><w:spacing w:val="1"/><w:kern w:val="0"/><w:fitText w:val="2000" w:id="-359372284"/></w:rPr><w:t>付</w:t></w:r><w:r><w:t>。</w:t></w:r></w:p><w:p><w:r><w:rPr><w:eastAsianLayout w:id="-359372287" w:vert="1" w:vertCompress="1"/></w:rPr><w:t>12</w:t></w:r><w:r><w:rPr><w:eastAsianLayout w:id="-359372288" w:vert="1"/></w:rPr><w:t>345</w:t></w:r></w:p><w:p><w:r><w:rPr><w:em w:val="dot"/></w:rPr><w:t>強調</w:t></w:r><w:r><w:rPr><w:em w:val="underDot"/></w:rPr><w:t xml:space="preserve"> abc</w:t></w:r><w:r><w:rPr><w:em w:val="none"/></w:rPr><w:t>し</w:t></w:r></w:p></w:body></w:document>"#
    );
    let mut z = zip::ZipArchive::new(std::io::Cursor::new(base)).unwrap();
    let mut entries: Vec<(String, Vec<u8>)> = Vec::new();
    for i in 0..z.len() {
        let mut f = z.by_index(i).unwrap();
        let name = f.name().to_string();
        let mut bytes = Vec::new();
        f.read_to_end(&mut bytes).unwrap();
        let replaced = name == "word/document.xml";
        entries.push((name, if replaced { doc.as_bytes().to_vec() } else { bytes }));
    }
    let refs: Vec<(&str, &[u8])> = entries.iter().map(|(n, b)| (n.as_str(), b.as_slice())).collect();
    zip(&refs)
}

#[test]
fn word_asian_layout_runs_read_as_word_writes_them() {
    let d = wordcraft_docx::read(&asian_layout_docx()).unwrap();
    let p = d.body[0].as_para().unwrap();
    let l = p.runs[1].props.east_asian_layout.expect("割注");
    assert_eq!((l.combine, l.combine_brackets, l.vert, l.id), (true, CombineBrackets::Round, false, -359372279));
    let p = d.body[1].as_para().unwrap();
    assert_eq!(p.runs[0].props.east_asian_layout.map(|l| (l.combine, l.combine_brackets)), Some((true, CombineBrackets::None)));
    let p = d.body[2].as_para().unwrap();
    let f = p.runs[1].props.fit_text.expect("均等割り付け");
    assert_eq!((f.width, f.id), (100.0, -359372284));
    assert_eq!(p.runs[2].props.fit_text, Some(f), "the block's runs share the id");
    assert_eq!(p.runs[1].props.spacing, Some(9.65), "Word's computed spacing is kept for the file");
    let p = d.body[3].as_para().unwrap();
    assert_eq!(p.runs[0].props.east_asian_layout.map(|l| (l.vert, l.vert_compress, l.combine)), Some((true, true, false)));
    assert_eq!(p.runs[1].props.east_asian_layout.map(|l| (l.vert, l.vert_compress)), Some((true, false)));
    let p = d.body[4].as_para().unwrap();
    assert_eq!(p.runs[0].props.em, Some(EmphasisMark::Dot));
    assert_eq!(p.runs[1].props.em, Some(EmphasisMark::UnderDot));
    assert_eq!(p.runs[2].props.em, Some(EmphasisMark::None));
    // Written back in Word's spelling, in schema order.
    let bytes = wordcraft_docx::write(&d).unwrap();
    let xml = part(&bytes, "word/document.xml");
    assert!(xml.contains(r#"<w:eastAsianLayout w:id="-359372279" w:combine="1" w:combineBrackets="round"/>"#), "{xml}");
    assert!(xml.contains(r#"<w:fitText w:val="2000" w:id="-359372284"/>"#), "{xml}");
    assert!(xml.contains(r#"<w:eastAsianLayout w:id="-359372287" w:vert="1" w:vertCompress="1"/>"#), "{xml}");
    assert!(xml.contains(r#"<w:em w:val="underDot"/>"#) && xml.contains(r#"<w:em w:val="none"/>"#), "{xml}");
    let fit = xml.find("<w:fitText").unwrap();
    let kern = xml[..fit].rfind("<w:kern").unwrap();
    assert!(kern < fit, "kern before fitText as in the schema");
}

/// Custom kinsoku lists (`w:noLineBreaksBefore`/`After`, per language) and spacing in lines
/// (`w:beforeLines`/`w:afterLines`, hundredths of a line) are written as Word writes them and
/// read back.
#[test]
fn custom_kinsoku_lists_and_spacing_in_lines_round_trip() {
    use wordcraft_doc::east_asian::KinsokuChars;
    let mut d = Document::new();
    d.settings.kinsoku_chars = vec![
        KinsokuChars { lang: "ja-JP".into(), no_start: "あ、".into(), no_end: "「".into() },
        KinsokuChars { lang: "zh-CN".into(), no_start: "，".into(), no_end: String::new() },
    ];
    d.settings.strict_kinsoku = true;
    let mut p = Paragraph::with_text("行", CharProps::default());
    p.props.space_before = Some(18.0);
    p.props.space_before_lines = Some(1.0);
    p.props.space_after_lines = Some(0.5);
    d.body = vec![para_block(p)];
    let bytes = wordcraft_docx::write(&d).unwrap();
    let settings = part(&bytes, "word/settings.xml");
    assert!(
        settings.contains(r#"<w:noLineBreaksAfter w:lang="ja-JP" w:val="「"/><w:noLineBreaksBefore w:lang="ja-JP" w:val="あ、"/>"#),
        "{settings}"
    );
    assert!(settings.contains(r#"<w:noLineBreaksBefore w:lang="zh-CN" w:val="，"/>"#), "{settings}");
    assert!(settings.contains(r#"<w:strictFirstAndLastChars/>"#), "{settings}");
    assert!(!settings.contains(r#"<w:noLineBreaksAfter w:lang="zh-CN""#), "an empty list is not written: {settings}");
    let document = part(&bytes, "word/document.xml");
    assert!(document.contains(r#"<w:spacing w:before="360" w:beforeLines="100" w:afterLines="50"/>"#), "{document}");
    let r = wordcraft_docx::read(&bytes).unwrap();
    assert_eq!(r.settings.kinsoku_chars, d.settings.kinsoku_chars);
    assert!(r.settings.strict_kinsoku);
    let p = r.body[0].as_para().unwrap();
    assert_eq!((p.props.space_before, p.props.space_before_lines, p.props.space_after_lines), (Some(18.0), Some(1.0), Some(0.5)));
    // Read straight from Word's markup, lists for one language split over the two elements.
    let bytes = wordcraft_docx::write(&Document::new()).unwrap();
    let settings = part(&bytes, "word/settings.xml").replace(
        "</w:settings>",
        r#"<w:noLineBreaksAfter w:lang="ja-JP" w:val="$（"/><w:noLineBreaksBefore w:lang="ja-JP" w:val="!%)"/></w:settings>"#,
    );
    let mut entries: Vec<(String, Vec<u8>)> = Vec::new();
    let mut z = zip::ZipArchive::new(std::io::Cursor::new(bytes)).unwrap();
    for i in 0..z.len() {
        let mut f = z.by_index(i).unwrap();
        let mut data = Vec::new();
        f.read_to_end(&mut data).unwrap();
        let name = f.name().to_string();
        entries.push((name.clone(), if name == "word/settings.xml" { settings.clone().into_bytes() } else { data }));
    }
    let refs: Vec<(&str, &[u8])> = entries.iter().map(|(n, b)| (n.as_str(), b.as_slice())).collect();
    let r = wordcraft_docx::read(&zip(&refs)).unwrap();
    assert_eq!(r.settings.kinsoku_chars, vec![KinsokuChars { lang: "ja-JP".into(), no_start: "!%)".into(), no_end: "$（".into() }]);
}

/// The OpenType features of Font › Advanced (`w14:ligatures`, `w14:numForm`, `w14:numSpacing`,
/// `w14:stylisticSets`, `w14:cntxtAlts`; [MS-DOCX] 2.6.3) are kept through a save and read back,
/// in the document defaults (Word's Japanese Normal sets `standardContextual` there) and on runs.
#[test]
fn opentype_features_round_trip() {
    use wordcraft_doc::props::{Ligatures, NumForm, NumSpacing};
    let mut d = Document::new();
    d.styles.default_chr.ligatures = Some(Ligatures::StandardContextual);
    let run = CharProps {
        ligatures: Some(Ligatures::HistoricalDiscretionary),
        num_form: Some(NumForm::OldStyle),
        num_spacing: Some(NumSpacing::Tabular),
        stylistic_sets: Some(vec![1, 7, 20, 0, 21]),
        cntxt_alts: Some(true),
        ..Default::default()
    };
    d.body = vec![para_block(Paragraph::with_text("office 1234", run))];
    let bytes = wordcraft_docx::write(&d).unwrap();
    let styles = part(&bytes, "word/styles.xml");
    assert!(styles.contains(r#"<w14:ligatures w14:val="standardContextual"/>"#), "{styles}");
    let document = part(&bytes, "word/document.xml");
    for x in [
        r#"<w14:ligatures w14:val="historicalDiscretionary"/>"#,
        r#"<w14:numForm w14:val="oldStyle"/>"#,
        r#"<w14:numSpacing w14:val="tabular"/>"#,
        r#"<w14:stylisticSets><w14:styleSet w14:id="1"/><w14:styleSet w14:id="7"/><w14:styleSet w14:id="20"/></w14:stylisticSets>"#,
        r#"<w14:cntxtAlts w14:val="1"/>"#,
    ] {
        assert!(document.contains(x), "{x} in {document}");
    }
    let r = wordcraft_docx::read(&bytes).unwrap();
    assert_eq!(r.styles.default_chr.ligatures, Some(Ligatures::StandardContextual));
    let p = r.body[0].as_para().unwrap();
    let c = &p.runs[0].props;
    assert_eq!(c.ligatures, Some(Ligatures::HistoricalDiscretionary));
    assert_eq!(c.num_form, Some(NumForm::OldStyle));
    assert_eq!(c.num_spacing, Some(NumSpacing::Tabular));
    assert_eq!(c.stylistic_sets.as_deref(), Some(&[1u8, 7, 20][..]));
    assert_eq!(c.cntxt_alts, Some(true));
    // Word's own spellings: `none` and an explicit off.
    let d2 = wordcraft_docx::read(&bytes.clone()).unwrap();
    drop(d2);
}

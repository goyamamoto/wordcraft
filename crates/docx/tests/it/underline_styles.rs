//! Word's eighteen underline styles (ECMA-376 Part 1 §17.18.99 `ST_Underline`): each reads as
//! itself, writes its own name and reads back; a value WordCraft doesn't know reads as a single
//! underline, as Word draws it. Fixtures are hand-written.
use std::io::{Read, Write};
use wordcraft_doc::props::Underline;

fn zip(entries: &[(&str, &[u8])]) -> Vec<u8> {
    let mut zw = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    for (name, bytes) in entries {
        zw.start_file(*name, zip::write::SimpleFileOptions::default()).unwrap();
        zw.write_all(bytes).unwrap();
    }
    zw.finish().unwrap().into_inner()
}

/// A package whose body is `body`.
fn docx(body: &str) -> Vec<u8> {
    let root = r#"<?xml version="1.0" encoding="UTF-8"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="word/document.xml"/></Relationships>"#;
    let doc = format!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body>{body}</w:body></w:document>"#
    );
    zip(&[("_rels/.rels", root.as_bytes()), ("word/document.xml", doc.as_bytes())])
}

fn underlines(d: &wordcraft_doc::Document) -> Vec<Underline> {
    d.body.iter().filter_map(|b| b.as_para()).map(|p| p.props_of_char(0).underline.unwrap_or(Underline::None)).collect()
}

#[test]
fn every_underline_style_reads_writes_and_reads_back() {
    let run = |val: &str| format!(r#"<w:p><w:r><w:rPr><w:u w:val="{val}"/></w:rPr><w:t>下線</w:t></w:r></w:p>"#);
    let body: String = Underline::ALL.iter().map(|u| run(u.ooxml())).chain([run("zigzag")]).collect();
    let d = wordcraft_docx::read(&docx(&body)).unwrap();
    let mut want = Underline::ALL.to_vec();
    want.push(Underline::Single);
    assert_eq!(underlines(&d), want);
    let mut z = zip::ZipArchive::new(std::io::Cursor::new(wordcraft_docx::write(&d).unwrap())).unwrap();
    let mut xml = String::new();
    z.by_name("word/document.xml").unwrap().read_to_string(&mut xml).unwrap();
    for u in Underline::ALL {
        if u != Underline::None {
            let attr = format!(r#"<w:u w:val="{}""#, u.ooxml());
            assert!(xml.contains(&attr), "{attr} missing: {xml}");
        }
    }
    let inner = &xml[xml.find("<w:body>").unwrap() + 8..xml.find("</w:body>").unwrap()];
    assert_eq!(underlines(&wordcraft_docx::read(&docx(inner)).unwrap()), want);
}

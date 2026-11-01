use super::header_footer::hf_part;
use super::{
    Align, Block, CharProps, CmdError, CmdResult, InlineObject, Paragraph, Rgb, Selection, Session, StoryRef, TabAlign, TabStop, TextColor, Value,
    delete_selection, json, p, para_block, sel_result, type_text,
};

pub(super) fn page_number(s: &mut Session, v: &Value) -> CmdResult {
    let position = p::str(v, "position").unwrap_or("bottom");
    let align = match p::str(v, "align").unwrap_or("center") {
        "left" => Align::Left,
        "right" => Align::Right,
        _ => Align::Center,
    };
    let x_of_y = p::str(v, "format").is_some_and(|f| f.contains("of"));
    let field = InlineObject::Field { instr: "PAGE".into(), result: "1".into(), locked: false, dirty: false, code: false, form: String::new() };
    if position == "current" {
        let props = s.typing_props();
        let at = delete_selection(s)?;
        let end = s.doc.insert_object(&at, field, &props)?;
        s.sel = Selection::caret(end);
        return sel_result(s);
    }
    let header = position == "top";
    let id = hf_part(s, header);
    let mut para = Paragraph::new().styled(if header { "Header" } else { "Footer" });
    para.props.align = Some(align);
    para.props.tabs = Some(vec![TabStop { pos: 0.0, align: TabAlign::Clear, leader: Default::default() }]);
    if x_of_y {
        para.insert_text(0, "Page  of ", &CharProps::default())?;
        para.insert_object(5, field, &CharProps { bold: Some(true), ..Default::default() })?;
        let end = para.len();
        para.insert_object(
            end,
            InlineObject::Field { instr: "NUMPAGES".into(), result: "1".into(), locked: false, dirty: false, code: false, form: String::new() },
            &CharProps { bold: Some(true), ..Default::default() },
        )?;
    } else {
        para.insert_object(0, field, &CharProps::default())?;
    }
    s.doc.set_story(StoryRef::Part(id), vec![para_block(para)])?;
    Ok(json!({"story": id}))
}

pub(super) fn date_time(s: &mut Session, v: &Value) -> CmdResult {
    let fmt = p::str(v, "format").unwrap_or("M/d/yyyy");
    let text = format_date(fmt);
    if p::bool(v, "update").unwrap_or(false) {
        let props = s.typing_props();
        let at = delete_selection(s)?;
        let end = s.doc.insert_object(
            &at,
            InlineObject::Field { instr: format!("DATE \\@ \"{fmt}\""), result: text, locked: false, dirty: false, code: false, form: String::new() },
            &props,
        )?;
        s.sel = Selection::caret(end);
    } else {
        type_text(s, &text)?;
    }
    sel_result(s)
}

/// Format today's date with a Word picture (`M/d/yyyy`, `MMMM d, yyyy`, `yyyy-MM-dd`, `dddd`…)
/// in English names, local time.
pub fn format_date(fmt: &str) -> String {
    format_date_in(fmt, None)
}

/// Format today's date, local time, with a Word picture in the names of language `lang` (the
/// field's text language, `w:lang`; see [`format_date_parts_in`]).
pub fn format_date_in(fmt: &str, lang: Option<&str>) -> String {
    let local = crate::cmd::now_local_secs();
    let days = local.div_euclid(86_400);
    let t = local.rem_euclid(86_400) as u32;
    let (y, m, d) = crate::cmd::civil_from_days(days);
    format_date_parts_in(fmt, lang, y, m as usize, d, (t / 3600, t / 60 % 60, t % 60), days)
}

/// [`format_date_parts_in`] with English names.
pub fn format_date_parts(fmt: &str, y: i64, m: usize, d: u32, hms: (u32, u32, u32), days_since: i64) -> String {
    format_date_parts_in(fmt, None, y, m, d, hms, days_since)
}

/// Format a date and time with a Word picture; `days_since` (days since 1970-01-01) gives the weekday.
///
/// Word's tokens, as Word for Mac evaluates them (observed 2026-10-18): `y`/`Y` year, `M` month,
/// `d`/`D` day (`ddd`, `dddd` the weekday), `H` and `h` hours, `m` minutes, `s`/`S` seconds,
/// `am/pm` (any case) AM or PM, `'…'` literal text, and the Japanese tokens of
/// [`crate::cmd::wareki`] — `ggg`/`gg`/`g`/`GG`/`G` the era, `e`/`ee`/`E` the era year, `EE` the year
/// in kanji, `aaa` the weekday's kanji, `A` and `O` the day and month in kanji, `ampm` 午前/午後.
/// A run longer than a token's longest form is cut into tokens from the left (`gggg` = `ggg` +
/// `g`, `MMMMM` = `MMMM` + `M`, `ddddd` = `dddd` + `d`), as Word does. Other characters are
/// copied. The names follow the field's text language `lang` (`w:lang/@w:val`, `dt1`–`dt4`
/// probes, 2026-10-19; [`crate::cmd::dates`]): Japanese writes `dddd` 月曜日, `ddd` 月, `MMMM` and
/// `MMM` 10月, and `am/pm` of any case 午前 or 午後, where English writes Monday, Mon, October,
/// Oct, AM or PM (`AM/PM h時mm分` reads 午後 8時43分 in a Japanese run and PM 8時43分 in an
/// English one), and French, German, Spanish, Italian, Portuguese, Russian, Dutch, Chinese and
/// Korean their own names. The era, kanji and `ampm` tokens read the same in Japanese, English
/// and the European languages; Chinese writes `e`/`ee` as the western year and `E`/`EE` as its
/// kanji digits (二〇二六; 二○二六 in Traditional Chinese, whose `g`, `gg` and `ggg` are empty)
/// and copies `aaa`; Korean writes `aaa` as 일월화수목금토, `gg`/`ggg` as 단기 (`g` a stray
/// character in Word, nothing here), and
/// fails the whole picture on `E` (`Error! Unknown character in picture string.`).
pub fn format_date_parts_in(fmt: &str, lang: Option<&str>, y: i64, m: usize, d: u32, hms: (u32, u32, u32), days_since: i64) -> String {
    format_picture(fmt, crate::cmd::dates::DateLang::of(lang), y, m, d, hms, days_since)
}

/// [`format_date_parts_in`] with the language already resolved.
pub fn format_picture(
    fmt: &str,
    lang: crate::cmd::dates::DateLang,
    y: i64,
    m: usize,
    d: u32,
    (hh, mm, ss): (u32, u32, u32),
    days_since: i64,
) -> String {
    use crate::cmd::dates::{PICTURE_ERROR, era_token, half_day, kanji_year, month_name, weekday_char, weekday_name};
    use crate::cmd::wareki::{era_of, kanji_number};
    let wd = days_since.saturating_add(4).rem_euclid(7) as usize;
    let pm = hh >= 12;
    let era = era_of(y, m.clamp(1, 12) as u32, d);
    // The year within the era; the western year before 明治.
    let era_year = era.map(|(_, n)| n).unwrap_or(y);
    let era_text = |f: fn(&crate::cmd::wareki::Era) -> &'static str| era.map(|(e, _)| f(e)).unwrap_or("");
    let japanese_eras = lang.japanese_eras();
    let mut out = String::new();
    let chars: Vec<char> = fmt.chars().collect();
    let word = |i: usize, w: &str| chars.get(i..i + w.chars().count()).is_some_and(|c| c.iter().collect::<String>() == w);
    let mut i = 0;
    while i < chars.len() {
        let c = chars.get(i).copied().unwrap_or(' ');
        // `am/pm` (any case) is one token: the language's AM or PM (`AM/PM` and `am/pm` differ
        // in UK English and Spanish).
        if chars.get(i..i + 5).is_some_and(|w| w.iter().collect::<String>().eq_ignore_ascii_case("am/pm")) {
            out.push_str(half_day(lang, c.is_uppercase(), pm));
            i += 5;
            continue;
        }
        // `ampm` (each half lowercase or uppercase, as Word takes it) is 午前 or 午後.
        if (word(i, "am") || word(i, "AM")) && (word(i + 2, "pm") || word(i + 2, "PM")) {
            out.push_str(if pm { "午後" } else { "午前" });
            i += 4;
            continue;
        }
        // `'text'` is copied without the quotes.
        if c == '\'' {
            let end = chars.get(i + 1..).and_then(|rest| rest.iter().position(|&x| x == '\''));
            match end {
                Some(len) => {
                    out.extend(chars.get(i + 1..i + 1 + len).unwrap_or(&[]));
                    i += len + 2;
                }
                None => {
                    out.extend(chars.get(i + 1..).unwrap_or(&[]));
                    i = chars.len();
                }
            }
            continue;
        }
        let mut n = 1;
        while chars.get(i + n) == Some(&c) {
            n += 1;
        }
        // (characters taken, text) — a run longer than the longest token is cut from the left.
        let (take, text): (usize, String) = match (c, n) {
            ('y' | 'Y', 4..) => (n, format!("{y:04}")),
            ('y' | 'Y', _) => (n, format!("{:02}", y.rem_euclid(100))),
            ('M', 4..) => (4, month_name(lang, m, true)),
            ('M', 3) => (n, month_name(lang, m, false)),
            ('M', 2) => (n, format!("{m:02}")),
            ('M', 1) => (n, m.to_string()),
            ('d' | 'D', 4..) => (4, weekday_name(lang, wd, true)),
            ('d' | 'D', 3) => (n, weekday_name(lang, wd, false)),
            ('d' | 'D', 2) => (n, format!("{d:02}")),
            ('d' | 'D', 1) => (n, d.to_string()),
            ('H', 2..) => (n, format!("{hh:02}")),
            ('H', _) => (n, hh.to_string()),
            ('h', 2..) => (n, format!("{:02}", if hh.is_multiple_of(12) { 12 } else { hh % 12 })),
            ('h', _) => (n, (if hh.is_multiple_of(12) { 12 } else { hh % 12 }).to_string()),
            ('m', 2..) => (n, format!("{mm:02}")),
            ('m', _) => (n, mm.to_string()),
            ('s' | 'S', 2..) => (n, format!("{ss:02}")),
            ('s' | 'S', _) => (n, ss.to_string()),
            ('g', _) if era_token(lang, n.min(3)).is_some() => (n.min(3), era_token(lang, n.min(3)).unwrap_or("").to_string()),
            ('g', 3..) => (3, era_text(|e| e.name).to_string()),
            ('g', 2) => (2, era_text(|e| e.short).to_string()),
            ('g', _) => (1, era_text(|e| e.latin).to_string()),
            ('G', 2..) => (2, era_text(|e| e.name).to_string()),
            ('G', _) => (1, era_text(|e| e.short).to_string()),
            ('e', _) if !japanese_eras && lang != crate::cmd::dates::DateLang::Ko => (n.min(2), y.to_string()),
            ('e', 2..) => (2, format!("{era_year:02}")),
            ('e', _) => (1, era_year.to_string()),
            ('E', _) if lang == crate::cmd::dates::DateLang::Ko => return PICTURE_ERROR.to_string(),
            ('E', _) if !japanese_eras => (n.min(2), kanji_year(lang, y)),
            ('E', 2..) => (2, kanji_year(lang, y)),
            ('E', _) => (1, kanji_number(era_year)),
            ('a', 3..) => match weekday_char(lang, wd) {
                Some(k) => (3, k.to_string()),
                None => (n, std::iter::repeat_n(c, n).collect()),
            },
            ('a', _) => (n, "a".to_string()),
            ('A', _) => (1, kanji_number(i64::from(d))),
            ('O', _) => (1, kanji_number(m as i64)),
            _ => (n, std::iter::repeat_n(c, n).collect()),
        };
        out.push_str(&text);
        i += take.max(1);
    }
    out
}

/// Insert › Symbol: the character at the caret, in its own font when `font` names one (the rest of
/// the text keeps the caret's font).
pub(super) fn symbol(s: &mut Session, v: &Value) -> CmdResult {
    let c = p::req_str(v, "char")?;
    if c.is_empty() || c.chars().count() > 64 {
        return Err(CmdError::Params("char: 1–64 characters".into()));
    }
    let font = p::str(v, "font").map(str::trim).filter(|f| !f.is_empty());
    let Some(font) = font else {
        type_text(s, c)?;
        return sel_result(s);
    };
    if font.len() > 256 {
        return Err(CmdError::Params("font: a family name".into()));
    }
    let base = s.typing_props();
    // A symbol font's character remembers it as its own font (`w:sym`), so changing the text's
    // font later keeps the symbol; other fonts are ordinary run formatting. `w:sym` holds a 16-bit
    // code, so a character beyond the Basic Multilingual Plane stays ordinary text in that font.
    // Each character decides for itself, so a mixed string keeps its symbols and its text apart.
    let font_is_symbol = wordcraft_doc::symbol_fonts::is_symbol_font(font);
    let mut r = Ok(());
    let mut buf = [0u8; 4];
    for ch in c.chars() {
        let mut props = base.clone();
        let private_use = ('\u{F020}'..='\u{F0FF}').contains(&ch);
        if u32::from(ch) <= 0xFFFF && (font_is_symbol || private_use) {
            props.sym_font = Some(font.to_string());
        } else {
            props.font = Some(font.to_string());
        }
        s.pending = Some(props);
        r = type_text(s, ch.encode_utf8(&mut buf));
        if r.is_err() {
            break;
        }
    }
    // Typing on after the symbol goes on in the text's own font.
    s.pending = Some(base);
    r?;
    sel_result(s)
}

pub(super) fn field(s: &mut Session, v: &Value) -> CmdResult {
    let instr = p::req_str(v, "instr")?.trim().to_string();
    if instr.is_empty() || instr.len() > 4096 {
        return Err(CmdError::Params("bad field code".into()));
    }
    let result = p::str(v, "result").unwrap_or("").to_string();
    let props = s.typing_props();
    let at = delete_selection(s)?;
    let end =
        s.doc.insert_object(&at, InlineObject::Field { instr, result, locked: false, dirty: false, code: false, form: String::new() }, &props)?;
    s.sel = Selection::caret(end);
    let _ = crate::cmd::references::update_fields(s);
    sel_result(s)
}

pub(super) fn cover_page(s: &mut Session, v: &Value) -> CmdResult {
    let title = p::str(v, "title").unwrap_or("Document Title").to_string();
    let subtitle = p::str(v, "subtitle").unwrap_or("Document subtitle").to_string();
    let author = p::str(v, "author").unwrap_or(&s.author).to_string();
    let accent = Rgb(0x15, 0x60, 0x82);
    let mut blocks = Vec::new();
    for _ in 0..8 {
        blocks.push(Block::Para(Paragraph::new()));
    }
    let mut t =
        Paragraph::with_text(&title, CharProps { size: Some(44.0), color: Some(TextColor::Rgb(accent)), ..Default::default() }).styled("Title");
    t.props.borders = Some(wordcraft_doc::props::Borders {
        bottom: Some(wordcraft_doc::props::Border { style: wordcraft_doc::props::BorderStyle::Single, width: 2.0, color: Some(accent), space: 6.0 }),
        ..Default::default()
    });
    blocks.push(Block::Para(t));
    blocks.push(Block::Para(Paragraph::with_text(&subtitle, CharProps::default()).styled("Subtitle")));
    for _ in 0..14 {
        blocks.push(Block::Para(Paragraph::new()));
    }
    blocks
        .push(Block::Para(Paragraph::with_text(&author, CharProps { bold: Some(true), color: Some(TextColor::Rgb(accent)), ..Default::default() })));
    let mut date = Paragraph::with_text(&format_date("MMMM d, yyyy"), CharProps::default());
    date.insert_text(date.len(), "\u{000C}", &CharProps::default())?;
    blocks.push(Block::Para(date));
    let frag = wordcraft_doc::edit::Fragment { blocks, ..Default::default() };
    let start = s.doc.start_of(StoryRef::Body);
    let at = s.doc.split_paragraph(&start)?;
    let _ = at;
    s.doc.insert_fragment(&start, &frag)?;
    s.sel = Selection::caret(s.doc.start_of(StoryRef::Body));
    sel_result(s)
}

/// Why Insert › Drop Cap is off at the caret, as in Word: in a table cell, in an empty paragraph
/// and in one that starts with a space.
pub(super) fn drop_cap_off(s: &Session) -> Option<&'static str> {
    if s.sel.focus.path.cell().is_some() {
        return Some("drop caps can't go in a table");
    }
    let first = s.doc.para_at(&s.sel.focus).and_then(|p| p.text.chars().next());
    match first {
        None => Some("the paragraph is empty"),
        Some(c) if c.is_whitespace() => Some("the paragraph starts with a space"),
        Some(_) => None,
    }
}

/// Insert › Drop Cap and Drop Cap Options: none, or the letter dropped into the text or hung in
/// the margin, `lines` tall, `distance` points from the text, in `font` (unset: the text's).
/// None takes all of it away, the letter's font with it.
pub(super) fn drop_cap(s: &mut Session, v: &Value) -> CmdResult {
    let lines = v.get("lines").and_then(Value::as_f64).filter(|l| l.is_finite());
    let none = p::str(v, "position") == Some("none") || lines == Some(0.0);
    let margin = p::str(v, "position") == Some("margin");
    let lines = lines.map_or(3.0, |l| l.round().clamp(1.0, 10.0)) as u8;
    let distance = p::f32(v, "distance").map_or(0.0, |d| d.clamp(0.0, wordcraft_doc::resolve::MAX_DROP_CAP_DISTANCE));
    // A family name, not a paragraph of text.
    let font = p::str(v, "font").map(str::trim).filter(|f| !f.is_empty()).map(|f| f.chars().take(64).collect::<String>());
    crate::cmd::para::fmt(s, &|p| {
        p.drop_cap = (!none).then_some(lines);
        p.drop_cap_margin = (!none && margin).then_some(true);
        p.drop_cap_distance = (!none && distance > 0.0).then_some(distance);
        p.drop_cap_font = font.clone().filter(|_| !none);
    })?;
    // Word sizes the letter itself and stores the size in its run (the Font box shows 38.5 for
    // three lines of 10.5-pt 游明朝); none gives it the text's size back.
    let story = s.sel.focus.story;
    for path in crate::cmd::selected_paths(s) {
        let size = s.doc.para(story, &path).filter(|_| !none).and_then(|para| wordcraft_layout::drop_cap_size(&s.doc, para));
        let para = s.doc.para_mut(story, &path)?;
        if let Some(first) = para.text.chars().next() {
            para.format(0, first.len_utf8(), &|c| c.size = size)?;
            para.touch();
        }
    }
    sel_result(s)
}

/// Insert › Drop Cap at the caret: the menu's highlight and the Drop Cap dialog's values.
#[derive(Clone, Debug, PartialEq)]
pub struct DropCapState {
    /// `none`, `dropped` or `margin`.
    pub position: &'static str,
    /// Lines to drop (3 when there is no drop cap).
    pub lines: u8,
    /// Distance from the text, points.
    pub distance: f32,
    /// The letter's own font, if it has one.
    pub font: Option<String>,
    /// The font of the paragraph's first character.
    pub text_font: String,
}

pub fn drop_cap_state(s: &Session) -> DropCapState {
    let para = s.doc.para_at(&s.sel.focus);
    let props = para.map(|p| p.props.clone()).unwrap_or_default();
    let rp = s.doc.styles.resolve_para(&props);
    let first = para.map(|p| p.props_at(0).clone()).unwrap_or_default();
    let text_font = s.doc.styles.resolve_char_in(props.style.as_deref(), None, &first).font;
    DropCapState {
        position: match (rp.drop_cap, rp.drop_cap_margin) {
            (0, _) => "none",
            (_, true) => "margin",
            _ => "dropped",
        },
        lines: if rp.drop_cap == 0 { 3 } else { rp.drop_cap },
        distance: if rp.drop_cap == 0 { 0.0 } else { rp.drop_cap_distance },
        font: rp.drop_cap_font.filter(|_| rp.drop_cap > 0),
        text_font,
    }
}

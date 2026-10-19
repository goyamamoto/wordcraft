//! Date fields as Word for Mac evaluates them: the names and default pictures of a field's text
//! language, the calendars of `\h` and `\s`, and the document's creation and save times.
//!
//! Measured with Japanese editing on 2026-10-19 (`dt2`–`dt4` probes: fields updated through
//! Word's object model, results read from the files it saved; `CREATEDATE` and `SAVEDATE` fields
//! with the document's `dcterms:created`/`modified` set to each month and weekday gave the full
//! name tables). A field's names follow its run's text language (`w:lang/@w:val`; the East Asian
//! tag changes nothing): Word knows English (US and UK), Japanese, Simplified and Traditional
//! Chinese, Korean, French, German, Spanish, Italian, Portuguese, Russian and Dutch — any other tag
//! reads as US English. A field without a `\@` switch takes its language's short date (Word for
//! Mac: the system's CLDR short pattern for the language, `10/19/26` in US English, `19/10/2026`
//! in UK English, `2026/10/19` in Japanese, `2026. 10. 19.` in Korean, `19.10.26` in German) or
//! Word's own time (`8:57 PM`; `午後8時57分`; `오후 8시 57분`; `20时57分`; `8:57 pm` in UK English;
//! `8:57 p. m.` in Spanish; Traditional Chinese takes the English `8:57 PM`); `CREATEDATE`,
//! `SAVEDATE` and `PRINTDATE` without a switch read `2026/01/02 12:04:00` whatever the language
//! (the system's medium date and time, measured on a Japanese Mac), their seconds zero.
//! `DATE \@ ""` is empty. `\h` (Hijri) and `\s` (Saka) change the date in English runs only
//! (`5/8/48` and `7/27/48` in US English, `08/05/1448` in UK English); every other language shows
//! the Gregorian date. `EDITTIME` reads `0`.

use super::wareki::{kanji_digits, kanji_number};

/// The language a date field's names and default pictures come from (`w:lang/@w:val`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DateLang {
    /// US English, and every language Word for Mac has no names for.
    EnUs,
    /// UK English: `am/pm` in lowercase, `dd/MM/yyyy`.
    EnGb,
    Ja,
    ZhCn,
    ZhTw,
    Ko,
    Fr,
    De,
    Es,
    It,
    Pt,
    Ru,
    Nl,
}

impl DateLang {
    /// The language of a BCP 47 tag (`ja-JP`, `en-GB`, `zh-Hant-TW`, `pt-BR`); `None` or an
    /// unknown tag reads as US English.
    pub fn of(tag: Option<&str>) -> Self {
        let Some(tag) = tag else { return Self::EnUs };
        let mut parts = tag.trim().split(['-', '_']);
        let lang = parts.next().unwrap_or_default().to_ascii_lowercase();
        let rest: Vec<String> = parts.map(str::to_ascii_lowercase).collect();
        match lang.as_str() {
            "ja" => Self::Ja,
            "zh" => {
                if rest.iter().any(|p| p == "hant" || p == "tw" || p == "hk" || p == "mo") {
                    Self::ZhTw
                } else {
                    Self::ZhCn
                }
            }
            "ko" => Self::Ko,
            "fr" => Self::Fr,
            "de" => Self::De,
            "es" => Self::Es,
            "it" => Self::It,
            "pt" => Self::Pt,
            "ru" => Self::Ru,
            "nl" => Self::Nl,
            "en" if rest.iter().any(|p| p == "gb") => Self::EnGb,
            _ => Self::EnUs,
        }
    }

    /// Whether the era, era-year and kanji tokens read as Japanese: everywhere but Chinese and
    /// Korean, where Word for Mac gives them other meanings (see [`era_token`]).
    pub fn japanese_eras(self) -> bool {
        !matches!(self, Self::ZhCn | Self::ZhTw | Self::Ko)
    }
}

const EN_MONTHS: [&str; 12] =
    ["January", "February", "March", "April", "May", "June", "July", "August", "September", "October", "November", "December"];
const EN_DAYS: [&str; 7] = ["Sunday", "Monday", "Tuesday", "Wednesday", "Thursday", "Friday", "Saturday"];
const FR_MONTHS: [&str; 12] =
    ["janvier", "février", "mars", "avril", "mai", "juin", "juillet", "août", "septembre", "octobre", "novembre", "décembre"];
const FR_MONTHS_SHORT: [&str; 12] = ["janv.", "févr.", "mars", "avr.", "mai", "juin", "juil.", "août", "sept.", "oct.", "nov.", "déc."];
const FR_DAYS: [&str; 7] = ["dimanche", "lundi", "mardi", "mercredi", "jeudi", "vendredi", "samedi"];
const FR_DAYS_SHORT: [&str; 7] = ["dim.", "lun.", "mar.", "mer.", "jeu.", "ven.", "sam."];
const DE_MONTHS: [&str; 12] = ["Januar", "Februar", "März", "April", "Mai", "Juni", "Juli", "August", "September", "Oktober", "November", "Dezember"];
const DE_MONTHS_SHORT: [&str; 12] = ["Jan", "Feb", "Mär", "Apr", "Mai", "Jun", "Jul", "Aug", "Sep", "Okt", "Nov", "Dez"];
const DE_DAYS: [&str; 7] = ["Sonntag", "Montag", "Dienstag", "Mittwoch", "Donnerstag", "Freitag", "Samstag"];
const DE_DAYS_SHORT: [&str; 7] = ["So", "Mo", "Di", "Mi", "Do", "Fr", "Sa"];
const ES_MONTHS: [&str; 12] =
    ["enero", "febrero", "marzo", "abril", "mayo", "junio", "julio", "agosto", "septiembre", "octubre", "noviembre", "diciembre"];
const ES_MONTHS_SHORT: [&str; 12] = ["ene", "feb", "mar", "abr", "may", "jun", "jul", "ago", "sept", "oct", "nov", "dic"];
const ES_DAYS: [&str; 7] = ["domingo", "lunes", "martes", "miércoles", "jueves", "viernes", "sábado"];
const ES_DAYS_SHORT: [&str; 7] = ["dom", "lun", "mar", "mié", "jue", "vie", "sáb"];
const IT_MONTHS: [&str; 12] =
    ["gennaio", "febbraio", "marzo", "aprile", "maggio", "giugno", "luglio", "agosto", "settembre", "ottobre", "novembre", "dicembre"];
const IT_MONTHS_SHORT: [&str; 12] = ["gen", "feb", "mar", "apr", "mag", "giu", "lug", "ago", "set", "ott", "nov", "dic"];
const IT_DAYS: [&str; 7] = ["domenica", "lunedì", "martedì", "mercoledì", "giovedì", "venerdì", "sabato"];
const IT_DAYS_SHORT: [&str; 7] = ["dom", "lun", "mar", "mer", "gio", "ven", "sab"];
const PT_MONTHS: [&str; 12] =
    ["janeiro", "fevereiro", "março", "abril", "maio", "junho", "julho", "agosto", "setembro", "outubro", "novembro", "dezembro"];
const PT_MONTHS_SHORT: [&str; 12] = ["jan", "fev", "mar", "abr", "mai", "jun", "jul", "ago", "set", "out", "nov", "dez"];
const PT_DAYS: [&str; 7] = ["domingo", "segunda-feira", "terça-feira", "quarta-feira", "quinta-feira", "sexta-feira", "sábado"];
const PT_DAYS_SHORT: [&str; 7] = ["dom", "seg", "ter", "qua", "qui", "sex", "sáb"];
const RU_MONTHS: [&str; 12] = ["январь", "февраль", "март", "апрель", "май", "июнь", "июль", "август", "сентябрь", "октябрь", "ноябрь", "декабрь"];
const RU_MONTHS_SHORT: [&str; 12] = ["янв.", "февр.", "март", "апр.", "май", "июнь", "июль", "авг.", "сент.", "окт.", "нояб.", "дек."];
const RU_DAYS: [&str; 7] = ["воскресенье", "понедельник", "вторник", "среда", "четверг", "пятница", "суббота"];
const RU_DAYS_SHORT: [&str; 7] = ["вс", "пн", "вт", "ср", "чт", "пт", "сб"];
const NL_MONTHS: [&str; 12] =
    ["januari", "februari", "maart", "april", "mei", "juni", "juli", "augustus", "september", "oktober", "november", "december"];
const NL_MONTHS_SHORT: [&str; 12] = ["jan", "feb", "mrt", "apr", "mei", "jun", "jul", "aug", "sep", "okt", "nov", "dec"];
const NL_DAYS: [&str; 7] = ["zondag", "maandag", "dinsdag", "woensdag", "donderdag", "vrijdag", "zaterdag"];
const NL_DAYS_SHORT: [&str; 7] = ["zo", "ma", "di", "wo", "do", "vr", "za"];
/// The weekday characters of Japanese (`aaa`, 曜日) and Chinese (星期), Sunday first.
const JA_DAYS: [&str; 7] = ["日", "月", "火", "水", "木", "金", "土"];
const ZH_DAYS: [&str; 7] = ["日", "一", "二", "三", "四", "五", "六"];
const KO_DAYS: [&str; 7] = ["일", "월", "화", "수", "목", "금", "토"];

fn pick<const N: usize>(table: &[&'static str; N], i: usize) -> &'static str {
    table.get(i).copied().unwrap_or("")
}

/// A month's name (`MMMM`) or short name (`MMM`) in a language: `October`/`Oct`, `octobre`/`oct.`,
/// `10月` for both in Japanese and Traditional Chinese, `十月`/`10月` in Simplified Chinese,
/// `10월` in Korean. Months are 1–12.
pub fn month_name(lang: DateLang, m: usize, long: bool) -> String {
    let i = m.saturating_sub(1).min(11);
    let m = i + 1;
    let (long_t, short_t): (&[&str; 12], &[&str; 12]) = match lang {
        DateLang::Ja | DateLang::ZhTw => return format!("{m}月"),
        DateLang::ZhCn => return if long { format!("{}月", kanji_number(m as i64)) } else { format!("{m}月") },
        DateLang::Ko => return format!("{m}월"),
        DateLang::EnUs | DateLang::EnGb => {
            let n = pick(&EN_MONTHS, i);
            return if long { n.to_string() } else { n.get(..3).unwrap_or(n).to_string() };
        }
        DateLang::Fr => (&FR_MONTHS, &FR_MONTHS_SHORT),
        DateLang::De => (&DE_MONTHS, &DE_MONTHS_SHORT),
        DateLang::Es => (&ES_MONTHS, &ES_MONTHS_SHORT),
        DateLang::It => (&IT_MONTHS, &IT_MONTHS_SHORT),
        DateLang::Pt => (&PT_MONTHS, &PT_MONTHS_SHORT),
        DateLang::Ru => (&RU_MONTHS, &RU_MONTHS_SHORT),
        DateLang::Nl => (&NL_MONTHS, &NL_MONTHS_SHORT),
    };
    pick(if long { long_t } else { short_t }, i).to_string()
}

/// A weekday's name (`dddd`) or short name (`ddd`): `Monday`/`Mon`, `月曜日`/`月`, `星期一`/`周一`
/// (`週一` in Traditional Chinese), `월요일`/`월`, `lundi`/`lun.`. `wd` counts from Sunday = 0.
pub fn weekday_name(lang: DateLang, wd: usize, long: bool) -> String {
    let i = wd % 7;
    let (long_t, short_t): (&[&str; 7], &[&str; 7]) = match lang {
        DateLang::Ja => return if long { format!("{}曜日", pick(&JA_DAYS, i)) } else { pick(&JA_DAYS, i).to_string() },
        DateLang::ZhCn => return format!("{}{}", if long { "星期" } else { "周" }, pick(&ZH_DAYS, i)),
        DateLang::ZhTw => return format!("{}{}", if long { "星期" } else { "週" }, pick(&ZH_DAYS, i)),
        DateLang::Ko => return if long { format!("{}요일", pick(&KO_DAYS, i)) } else { pick(&KO_DAYS, i).to_string() },
        DateLang::EnUs | DateLang::EnGb => {
            let n = pick(&EN_DAYS, i);
            return if long { n.to_string() } else { n.get(..3).unwrap_or(n).to_string() };
        }
        DateLang::Fr => (&FR_DAYS, &FR_DAYS_SHORT),
        DateLang::De => (&DE_DAYS, &DE_DAYS_SHORT),
        DateLang::Es => (&ES_DAYS, &ES_DAYS_SHORT),
        DateLang::It => (&IT_DAYS, &IT_DAYS_SHORT),
        DateLang::Pt => (&PT_DAYS, &PT_DAYS_SHORT),
        DateLang::Ru => (&RU_DAYS, &RU_DAYS_SHORT),
        DateLang::Nl => (&NL_DAYS, &NL_DAYS_SHORT),
    };
    pick(if long { long_t } else { short_t }, i).to_string()
}

/// The weekday character of the `aaa` token: the Japanese 日月火水木金土 everywhere but Korean
/// (일월화수목금토) and Chinese, where `aaa` is copied as it is (`None`).
pub fn weekday_char(lang: DateLang, wd: usize) -> Option<&'static str> {
    match lang {
        DateLang::ZhCn | DateLang::ZhTw => None,
        DateLang::Ko => Some(pick(&KO_DAYS, wd % 7)),
        _ => Some(pick(&JA_DAYS, wd % 7)),
    }
}

/// The half of the day: `AM/PM` (`upper`) and `am/pm` in a language — `AM`/`PM` in US English
/// for both, `am`/`pm` for the lowercase token in UK English, `A. M.`/`a. m.` (narrow no-break
/// spaces) in Spanish, 午前/午後, 上午/下午, 오전/오후 for both.
pub fn half_day(lang: DateLang, upper: bool, pm: bool) -> &'static str {
    match (lang, upper, pm) {
        (DateLang::Ja, _, false) => "午前",
        (DateLang::Ja, _, true) => "午後",
        (DateLang::ZhCn | DateLang::ZhTw, _, false) => "上午",
        (DateLang::ZhCn | DateLang::ZhTw, _, true) => "下午",
        (DateLang::Ko, _, false) => "오전",
        (DateLang::Ko, _, true) => "오후",
        (DateLang::Es, true, false) => "A.\u{202F}M.",
        (DateLang::Es, true, true) => "P.\u{202F}M.",
        (DateLang::Es, false, false) => "a.\u{202F}m.",
        (DateLang::Es, false, true) => "p.\u{202F}m.",
        (DateLang::EnGb, false, false) => "am",
        (DateLang::EnGb, false, true) => "pm",
        (_, _, false) => "AM",
        (_, _, true) => "PM",
    }
}

/// The era tokens `g`, `gg`, `ggg` where they don't read as the Japanese era: Korean `gg`/`ggg`
/// are 단기 and Traditional Chinese `gg`/`ggg` empty; `g` in both is a stray character in Word
/// (歽 in one field, nothing in another — uninitialised text), nothing here. `None` for the
/// languages that take the Japanese era (English, Japanese, Simplified Chinese…).
pub fn era_token(lang: DateLang, n: usize) -> Option<&'static str> {
    match (lang, n) {
        (DateLang::Ko, 1) | (DateLang::ZhTw, 1) => Some(""),
        (DateLang::Ko, _) => Some("단기"),
        (DateLang::ZhTw, _) => Some(""),
        _ => None,
    }
}

/// The year in kanji digits as `E`/`EE` write it in Chinese: 二〇二六 (〇) in Simplified Chinese and
/// 二○二六 (○) in Traditional Chinese and Japanese.
pub fn kanji_year(lang: DateLang, y: i64) -> String {
    let digits = kanji_digits(y);
    if lang == DateLang::ZhCn { digits.replace('○', "〇") } else { digits }
}

/// Word's result for a picture it can't read (`E` in a Korean run).
pub const PICTURE_ERROR: &str = "Error! Unknown character in picture string.";

/// A field's default picture, without a `\@` switch, and the language its names come from:
/// the language's short date for `DATE`, Word's time for `TIME` (Traditional Chinese takes the
/// US English `8:57 PM`), `yyyy/MM/dd H:mm:ss` for the document's dates whatever the language.
pub fn default_picture(field: &str, lang: DateLang) -> (&'static str, DateLang) {
    match field {
        "TIME" => match lang {
            DateLang::Ja => ("AM/PMh時m分", lang),
            DateLang::Ko => ("AM/PM h시 m분", lang),
            DateLang::ZhCn => ("H时m分", lang),
            DateLang::ZhTw => ("h:mm AM/PM", DateLang::EnUs),
            DateLang::EnGb | DateLang::Es => ("h:mm am/pm", lang),
            _ => ("h:mm AM/PM", lang),
        },
        "DATE" => (
            match lang {
                DateLang::EnUs => "M/d/yy",
                DateLang::EnGb | DateLang::Fr | DateLang::Pt => "dd/MM/yyyy",
                DateLang::Ja => "yyyy/MM/dd",
                DateLang::ZhCn | DateLang::ZhTw => "yyyy/M/d",
                DateLang::Ko => "yyyy. M. d.",
                DateLang::De => "dd.MM.yy",
                DateLang::Es => "d/M/yy",
                DateLang::It => "dd/MM/yy",
                DateLang::Ru => "dd.MM.yyyy",
                DateLang::Nl => "dd-MM-yyyy",
            },
            lang,
        ),
        _ => ("yyyy/MM/dd H:mm:ss", lang),
    }
}

/// The calendar a field's `\h` or `\s` switch asks for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Calendar {
    Gregorian,
    /// `\h`: the tabular Islamic calendar (Word's `5/8/48` for 2026-10-19).
    Hijri,
    /// `\s`: the Indian national calendar (`7/27/48` for 2026-10-19).
    Saka,
}

impl Calendar {
    /// The calendar of a field's switches in a language: Word for Mac honours `\h` and `\s` in
    /// English runs only.
    pub fn of(instr: &str, lang: DateLang) -> Self {
        let has = |sw: &str| instr.split_whitespace().any(|w| w.eq_ignore_ascii_case(sw));
        if !matches!(lang, DateLang::EnUs | DateLang::EnGb) {
            Self::Gregorian
        } else if has("\\h") {
            Self::Hijri
        } else if has("\\s") {
            Self::Saka
        } else {
            Self::Gregorian
        }
    }

    /// (year, month, day) of a day (days since 1970-01-01) in this calendar.
    pub fn civil(self, days: i64) -> (i64, u32, u32) {
        match self {
            Self::Gregorian => super::civil_from_days(days),
            Self::Hijri => hijri_from_days(days),
            Self::Saka => saka_from_days(days),
        }
    }
}

/// Days since 1970-01-01 as (year, month, day) of the tabular Islamic calendar with the
/// astronomical epoch (15 July 622, Julian), which gives Word's 1448-05-08 for 2026-10-19: a 30-year
/// cycle of 10 631 days whose years 2, 5, 7, 10, 13, 16, 18, 21, 24, 26 and 29 have 355 days,
/// months of 30 and 29 days in turn, the twelfth 30 in a leap year.
pub fn hijri_from_days(days: i64) -> (i64, u32, u32) {
    // 1 Muharram 1 AH = JDN 1948439 = 1970-01-01 (JDN 2440588) − 492 149 days.
    let mut rem = days.saturating_add(492_149);
    let cycles = rem.div_euclid(10_631);
    rem = rem.rem_euclid(10_631);
    let mut year = cycles.saturating_mul(30);
    let leap = |n: i64| (11 * n + 14).rem_euclid(30) < 11;
    let mut n = 1;
    while n <= 30 {
        let len = if leap(n) { 355 } else { 354 };
        if rem < len {
            break;
        }
        rem -= len;
        n += 1;
    }
    year = year.saturating_add(n);
    let mut month = 1;
    while month < 12 {
        let len = if month % 2 == 1 { 30 } else { 29 };
        if rem < len {
            break;
        }
        rem -= len;
        month += 1;
    }
    (year, month, (rem + 1).clamp(1, 30) as u32)
}

/// Days since 1970-01-01 as (year, month, day) of the Indian national (Saka) calendar: the year
/// starts on 22 March (21 March in a Gregorian leap year, when Chaitra has 31 days), the first
/// six months after Chaitra have 31 days and the last six 30, and the year is the Gregorian one
/// less 78 (Word's 1948-07-27 for 2026-10-19).
pub fn saka_from_days(days: i64) -> (i64, u32, u32) {
    let (gy, _, _) = super::civil_from_days(days);
    let is_leap = |y: i64| (y % 4 == 0 && y % 100 != 0) || y % 400 == 0;
    let start_of = |y: i64| super::controls::days_from_civil(y, 3, if is_leap(y) { 21 } else { 22 });
    let year = if days < start_of(gy) { gy - 1 } else { gy };
    let mut rem = days.saturating_sub(start_of(year));
    let mut month = 1;
    while month < 12 {
        let len = match month {
            1 if is_leap(year) => 31,
            1 => 30,
            2..=6 => 31,
            _ => 30,
        };
        if rem < len {
            break;
        }
        rem -= len;
        month += 1;
    }
    (year - 78, month, (rem + 1).clamp(1, 31) as u32)
}

/// A W3CDTF / ISO 8601 timestamp (`2026-01-02T03:04:05Z`, `2026-01-02T12:04:05+09:00`,
/// `2026-01-02`) as seconds since the Unix epoch, UTC. `None` when it can't be read.
pub fn parse_w3cdtf(s: &str) -> Option<i64> {
    let s = s.trim();
    let mut num = |t: &str| t.parse::<i64>().ok();
    let (date, time) = s.split_once('T').unwrap_or((s, ""));
    let mut d = date.split('-');
    let y = num(d.next()?)?;
    let m = d.next().and_then(&mut num).unwrap_or(1);
    let day = d.next().and_then(&mut num).unwrap_or(1);
    if !(1..=12).contains(&m) || !(1..=31).contains(&day) {
        return None;
    }
    let (clock, zone) = match time.find(['Z', 'z', '+', '-']) {
        Some(i) => (time.get(..i).unwrap_or(""), time.get(i..).unwrap_or("")),
        None => (time, ""),
    };
    let mut c = clock.split(':');
    let hh = c.next().filter(|t| !t.is_empty()).and_then(&mut num).unwrap_or(0);
    let mm = c.next().and_then(&mut num).unwrap_or(0);
    let ss = c.next().and_then(|t| t.split('.').next()).and_then(&mut num).unwrap_or(0);
    let offset = match zone.chars().next() {
        Some('+' | '-') => {
            let sign = if zone.starts_with('-') { -1 } else { 1 };
            let body = zone.get(1..).unwrap_or("");
            let (zh, zm) = body.split_once(':').unwrap_or((body, "0"));
            sign * (num(zh).unwrap_or(0) * 3600 + num(zm).unwrap_or(0) * 60)
        }
        _ => 0,
    };
    let days = super::controls::days_from_civil(y, m as u32, day as u32);
    Some(days.saturating_mul(86_400).saturating_add(hh * 3600 + mm * 60 + ss).saturating_sub(offset))
}

/// The result of a date field (`DATE`, `TIME`, `CREATEDATE`, `SAVEDATE`, `PRINTDATE`,
/// `EDITTIME`) from its code, in the names of its run's language `lang`, as Word for Mac
/// evaluates it (see the module doc): `now` is the local clock (seconds since the epoch shifted
/// to local time by `offset`, the zone's seconds east of UTC), `created`/`modified` the
/// document's core properties (UTC timestamps, shown in local time).
pub fn field_result(instr: &str, lang: Option<&str>, now: i64, offset: i64, created: &str, modified: &str) -> String {
    let name = wordcraft_layout::fields::field_name(instr);
    if name == "EDITTIME" {
        return "0".to_string();
    }
    let lang = DateLang::of(lang);
    let stamp = |s: &str| parse_w3cdtf(s).map(|t| t.saturating_add(offset));
    // The document's dates keep no seconds (Word shows 12:04:00 for a file created at 12:04:05).
    let whole_minute = |t: i64| t - t.rem_euclid(60);
    let local = match name.as_str() {
        "CREATEDATE" => stamp(created).map(whole_minute).unwrap_or(now),
        "SAVEDATE" => stamp(modified).map(whole_minute).unwrap_or(now),
        "PRINTDATE" => whole_minute(now),
        _ => now,
    };
    let days = local.div_euclid(86_400);
    let t = local.rem_euclid(86_400) as u32;
    let (pic, names) = match super::references::date_picture(instr) {
        Some(p) => (p, lang),
        None => {
            let (p, l) = default_picture(&name, lang);
            (p.to_string(), l)
        }
    };
    let (y, m, d) = Calendar::of(instr, lang).civil(days);
    super::insert::format_picture(&pic, names, y, m as usize, d, (t / 3600, t / 60 % 60, t % 60), days)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cmd::controls::days_from_civil;

    #[test]
    fn languages_of_tags() {
        assert_eq!(DateLang::of(None), DateLang::EnUs);
        assert_eq!(DateLang::of(Some("en-JP")), DateLang::EnUs);
        assert_eq!(DateLang::of(Some("en-GB")), DateLang::EnGb);
        assert_eq!(DateLang::of(Some("ja-JP")), DateLang::Ja);
        assert_eq!(DateLang::of(Some("zh-CN")), DateLang::ZhCn);
        assert_eq!(DateLang::of(Some("zh-Hant-TW")), DateLang::ZhTw);
        assert_eq!(DateLang::of(Some("pt_BR")), DateLang::Pt);
        assert_eq!(DateLang::of(Some("xx")), DateLang::EnUs);
    }

    /// Word's `5/8/48` (Hijri 1448-05-08) and `7/27/48` (Saka 1948-07-27) for 2026-10-19.
    #[test]
    fn hijri_and_saka_match_word() {
        let d = days_from_civil(2026, 10, 19);
        assert_eq!(hijri_from_days(d), (1448, 5, 8));
        assert_eq!(saka_from_days(d), (1948, 7, 27));
        // Saka new year: 22 March 2026 is Chaitra 1, 1948; the day before Phalguna 30, 1947.
        assert_eq!(saka_from_days(days_from_civil(2026, 3, 22)), (1948, 1, 1));
        assert_eq!(saka_from_days(days_from_civil(2026, 3, 21)), (1947, 12, 30));
        assert_eq!(saka_from_days(days_from_civil(2024, 3, 21)), (1946, 1, 1));
        // Hijri days are never 0 or 31 and months stay 1–12 across the cycle.
        for k in -400_000..400_000i64 {
            if k % 97 != 0 {
                continue;
            }
            let (_, m, d) = hijri_from_days(k);
            assert!((1..=12).contains(&m) && (1..=30).contains(&d), "{k}: {m}/{d}");
        }
    }

    #[test]
    fn w3cdtf_timestamps() {
        assert_eq!(parse_w3cdtf("1970-01-01T00:00:00Z"), Some(0));
        assert_eq!(parse_w3cdtf("2026-01-02T03:04:05Z"), Some(days_from_civil(2026, 1, 2) * 86_400 + 3 * 3600 + 4 * 60 + 5));
        assert_eq!(parse_w3cdtf("2026-01-02T12:04:05+09:00"), parse_w3cdtf("2026-01-02T03:04:05Z"));
        assert_eq!(parse_w3cdtf("2026-01-02"), Some(days_from_civil(2026, 1, 2) * 86_400));
        assert_eq!(parse_w3cdtf(""), None);
        assert_eq!(parse_w3cdtf("2026-13-01"), None);
        assert_eq!(parse_w3cdtf("junk"), None);
    }

    /// The local clock of `y-m-d hh:mm` in a zone 9 hours east (Tokyo), as [`field_result`] takes it.
    fn tokyo(y: i64, m: u32, d: u32, hh: i64, mm: i64) -> i64 {
        days_from_civil(y, m, d) * 86_400 + hh * 3600 + mm * 60
    }

    fn at(instr: &str, lang: &str, now: i64) -> String {
        field_result(instr, Some(lang), now, 9 * 3600, "2026-01-02T03:04:05Z", "2026-03-04T20:06:07Z")
    }

    /// Word for Mac's names in every language it knows, from `CREATEDATE` fields in documents
    /// created on 2026-01-05 (Monday), 02-03, 03-04, 04-02, 05-01, 06-06, 07-05 (Sunday) and the
    /// first of August to December (`dt4` probes, 2026-10-19).
    #[test]
    fn names_in_every_language_word_for_mac_knows() {
        let months = |lang: &str, long: bool| (1..=12).map(|m| month_name(DateLang::of(Some(lang)), m, long)).collect::<Vec<_>>().join("|");
        let days = |lang: &str, long: bool| (0..7).map(|wd| weekday_name(DateLang::of(Some(lang)), wd, long)).collect::<Vec<_>>().join("|");
        assert_eq!(months("en-US", true), "January|February|March|April|May|June|July|August|September|October|November|December");
        assert_eq!(months("en-US", false), "Jan|Feb|Mar|Apr|May|Jun|Jul|Aug|Sep|Oct|Nov|Dec");
        assert_eq!(days("en-GB", true), "Sunday|Monday|Tuesday|Wednesday|Thursday|Friday|Saturday");
        assert_eq!(days("en-GB", false), "Sun|Mon|Tue|Wed|Thu|Fri|Sat");
        assert_eq!(months("ja-JP", true), "1月|2月|3月|4月|5月|6月|7月|8月|9月|10月|11月|12月");
        assert_eq!(months("ja-JP", false), months("ja-JP", true));
        assert_eq!(days("ja-JP", true), "日曜日|月曜日|火曜日|水曜日|木曜日|金曜日|土曜日");
        assert_eq!(days("ja-JP", false), "日|月|火|水|木|金|土");
        assert_eq!(months("zh-CN", true), "一月|二月|三月|四月|五月|六月|七月|八月|九月|十月|十一月|十二月");
        assert_eq!(months("zh-CN", false), "1月|2月|3月|4月|5月|6月|7月|8月|9月|10月|11月|12月");
        assert_eq!(days("zh-CN", true), "星期日|星期一|星期二|星期三|星期四|星期五|星期六");
        assert_eq!(days("zh-CN", false), "周日|周一|周二|周三|周四|周五|周六");
        assert_eq!(months("zh-TW", true), "1月|2月|3月|4月|5月|6月|7月|8月|9月|10月|11月|12月");
        assert_eq!(months("zh-TW", false), months("zh-TW", true));
        assert_eq!(days("zh-TW", true), "星期日|星期一|星期二|星期三|星期四|星期五|星期六");
        assert_eq!(days("zh-TW", false), "週日|週一|週二|週三|週四|週五|週六");
        assert_eq!(months("ko-KR", true), "1월|2월|3월|4월|5월|6월|7월|8월|9월|10월|11월|12월");
        assert_eq!(months("ko-KR", false), months("ko-KR", true));
        assert_eq!(days("ko-KR", true), "일요일|월요일|화요일|수요일|목요일|금요일|토요일");
        assert_eq!(days("ko-KR", false), "일|월|화|수|목|금|토");
        assert_eq!(months("fr-FR", true), "janvier|février|mars|avril|mai|juin|juillet|août|septembre|octobre|novembre|décembre");
        assert_eq!(months("fr-FR", false), "janv.|févr.|mars|avr.|mai|juin|juil.|août|sept.|oct.|nov.|déc.");
        assert_eq!(days("fr-FR", true), "dimanche|lundi|mardi|mercredi|jeudi|vendredi|samedi");
        assert_eq!(days("fr-FR", false), "dim.|lun.|mar.|mer.|jeu.|ven.|sam.");
        assert_eq!(months("de-DE", true), "Januar|Februar|März|April|Mai|Juni|Juli|August|September|Oktober|November|Dezember");
        assert_eq!(months("de-DE", false), "Jan|Feb|Mär|Apr|Mai|Jun|Jul|Aug|Sep|Okt|Nov|Dez");
        assert_eq!(days("de-DE", true), "Sonntag|Montag|Dienstag|Mittwoch|Donnerstag|Freitag|Samstag");
        assert_eq!(days("de-DE", false), "So|Mo|Di|Mi|Do|Fr|Sa");
        assert_eq!(months("es-ES", true), "enero|febrero|marzo|abril|mayo|junio|julio|agosto|septiembre|octubre|noviembre|diciembre");
        assert_eq!(months("es-ES", false), "ene|feb|mar|abr|may|jun|jul|ago|sept|oct|nov|dic");
        assert_eq!(days("es-ES", true), "domingo|lunes|martes|miércoles|jueves|viernes|sábado");
        assert_eq!(days("es-ES", false), "dom|lun|mar|mié|jue|vie|sáb");
        assert_eq!(months("it-IT", true), "gennaio|febbraio|marzo|aprile|maggio|giugno|luglio|agosto|settembre|ottobre|novembre|dicembre");
        assert_eq!(months("it-IT", false), "gen|feb|mar|apr|mag|giu|lug|ago|set|ott|nov|dic");
        assert_eq!(days("it-IT", true), "domenica|lunedì|martedì|mercoledì|giovedì|venerdì|sabato");
        assert_eq!(days("it-IT", false), "dom|lun|mar|mer|gio|ven|sab");
        assert_eq!(months("pt-BR", true), "janeiro|fevereiro|março|abril|maio|junho|julho|agosto|setembro|outubro|novembro|dezembro");
        assert_eq!(months("pt-BR", false), "jan|fev|mar|abr|mai|jun|jul|ago|set|out|nov|dez");
        assert_eq!(days("pt-BR", true), "domingo|segunda-feira|terça-feira|quarta-feira|quinta-feira|sexta-feira|sábado");
        assert_eq!(days("pt-BR", false), "dom|seg|ter|qua|qui|sex|sáb");
        assert_eq!(months("ru-RU", true), "январь|февраль|март|апрель|май|июнь|июль|август|сентябрь|октябрь|ноябрь|декабрь");
        assert_eq!(months("ru-RU", false), "янв.|февр.|март|апр.|май|июнь|июль|авг.|сент.|окт.|нояб.|дек.");
        assert_eq!(days("ru-RU", true), "воскресенье|понедельник|вторник|среда|четверг|пятница|суббота");
        assert_eq!(days("ru-RU", false), "вс|пн|вт|ср|чт|пт|сб");
        assert_eq!(months("nl-NL", true), "januari|februari|maart|april|mei|juni|juli|augustus|september|oktober|november|december");
        assert_eq!(months("nl-NL", false), "jan|feb|mrt|apr|mei|jun|jul|aug|sep|okt|nov|dec");
        assert_eq!(days("nl-NL", true), "zondag|maandag|dinsdag|woensdag|donderdag|vrijdag|zaterdag");
        assert_eq!(days("nl-NL", false), "zo|ma|di|wo|do|vr|za");
        // The whole picture of the probe, from the document created Monday 2026-01-05 09:07 and
        // saved 21:03, in a Japanese and a Spanish run.
        let jan5 = "2026-01-05T00:07:00Z";
        let c = |lang: &str| {
            field_result("CREATEDATE \\@ \"dddd|ddd|MMMM|MMM|AM/PM|am/pm|aaa|aaaa|E|e|g|gg|ggg|yyyy/M/d\"", Some(lang), 0, 9 * 3600, jan5, "")
        };
        assert_eq!(c("ja-JP"), "月曜日|月|1月|1月|午前|午前|月|月a|八|8|R|令|令和|2026/1/5");
        assert_eq!(c("es-ES"), "lunes|lun|enero|ene|A.\u{202F}M.|a.\u{202F}m.|月|月a|八|8|R|令|令和|2026/1/5");
        assert_eq!(c("zh-CN"), "星期一|周一|一月|1月|上午|上午|aaa|aaaa|二〇二六|2026|R|令|令和|2026/1/5");
        assert_eq!(c("zh-TW"), "星期一|週一|1月|1月|上午|上午|aaa|aaaa|二○二六|2026||||2026/1/5");
        assert_eq!(c("ko-KR"), PICTURE_ERROR);
        let s = |lang: &str| {
            field_result("SAVEDATE \\@ \"AM/PM|am/pm|A/P|h:mm|H:mm|yyyy/M/d|d|dd|M|MM\"", Some(lang), 0, 9 * 3600, "", "2026-01-05T12:03:00Z")
        };
        assert_eq!(s("en-GB"), "PM|pm|五/P|9:03|21:03|2026/1/5|5|05|1|01");
        assert_eq!(s("ko-KR"), "오후|오후|五/P|9:03|21:03|2026/1/5|5|05|1|01");
    }

    /// Korean and Chinese tokens (`dt3` probe, Monday 2026-10-19 20:53).
    #[test]
    fn korean_and_chinese_tokens_as_word_for_mac_writes_them() {
        let now = tokyo(2026, 10, 19, 20, 53);
        let f = |lang: &str, pic: &str| at(&format!("DATE \\@ \"{pic}\""), lang, now);
        let toks = [
            "dddd",
            "ddd",
            "MMMM",
            "MMM",
            "AM/PM",
            "am/pm",
            "A/P",
            "tt",
            "g",
            "gg",
            "ggg",
            "e",
            "ee",
            "aaa",
            "aaaa",
            "E",
            "EE",
            "H時m分",
            "yyyy年M月d日",
            "MMMMM",
            "ddddd",
        ];
        let all = |lang: &str| toks.iter().map(|t| f(lang, t)).collect::<Vec<_>>().join("|");
        assert_eq!(
            all("ko-KR"),
            format!(
                "월요일|월|10월|10월|오후|오후|十九/P|tt||단기|단기|8|08|월|월a|{PICTURE_ERROR}|{PICTURE_ERROR}|20時53分|2026年10月19日|10월10|월요일19"
            )
        );
        assert_eq!(
            all("zh-CN"),
            "星期一|周一|十月|10月|下午|下午|十九/P|tt|R|令|令和|2026|2026|aaa|aaaa|二〇二六|二〇二六|20時53分|2026年10月19日|十月10|星期一19"
        );
        assert_eq!(
            all("zh-TW"),
            "星期一|週一|10月|10月|下午|下午|十九/P|tt||||2026|2026|aaa|aaaa|二○二六|二○二六|20時53分|2026年10月19日|10月10|星期一19"
        );
        assert_eq!(f("ja-JP", "MMMMM|ddddd|EE|E"), "10月10|月曜日19|二○二六|八");
    }

    /// Default pictures by language (`dt3` probe at 20:57 and 21:01 on 2026-10-19, document
    /// created 2026-01-02T03:04:05Z and saved 2026-03-04T20:06:07Z).
    #[test]
    fn default_pictures_follow_the_fields_language() {
        let now = tokyo(2026, 10, 19, 20, 57);
        let row = |lang: &str| {
            ["DATE", "TIME", "CREATEDATE", "SAVEDATE", "DATE \\h", "DATE \\s"].iter().map(|i| at(i, lang, now)).collect::<Vec<_>>().join("|")
        };
        let doc = "2026/01/02 12:04:00|2026/03/05 5:06:00";
        assert_eq!(row("en-US"), format!("10/19/26|8:57 PM|{doc}|5/8/48|7/27/48"));
        assert_eq!(row("en-JP"), row("en-US"));
        assert_eq!(row("ja-JP"), format!("2026/10/19|午後8時57分|{doc}|2026/10/19|2026/10/19"));
        assert_eq!(row("zh-CN"), format!("2026/10/19|20时57分|{doc}|2026/10/19|2026/10/19"));
        assert_eq!(row("zh-TW"), format!("2026/10/19|8:57 PM|{doc}|2026/10/19|2026/10/19"));
        assert_eq!(row("ko-KR"), format!("2026. 10. 19.|오후 8시 57분|{doc}|2026. 10. 19.|2026. 10. 19."));
        assert_eq!(row("fr-FR"), format!("19/10/2026|8:57 PM|{doc}|19/10/2026|19/10/2026"));
        assert_eq!(row("de-DE"), format!("19.10.26|8:57 PM|{doc}|19.10.26|19.10.26"));
        assert_eq!(row("es-ES"), format!("19/10/26|8:57 p.\u{202F}m.|{doc}|19/10/26|19/10/26"));
        assert_eq!(row("en-GB"), format!("19/10/2026|8:57 pm|{doc}|08/05/1448|27/07/1948"));
        assert_eq!(row("it-IT"), format!("19/10/26|8:57 PM|{doc}|19/10/26|19/10/26"));
        assert_eq!(row("pt-BR"), format!("19/10/2026|8:57 PM|{doc}|19/10/2026|19/10/2026"));
        assert_eq!(row("ru-RU"), format!("19.10.2026|8:57 PM|{doc}|19.10.2026|19.10.2026"));
        assert_eq!(row("nl-NL"), format!("19-10-2026|8:57 PM|{doc}|19-10-2026|19-10-2026"));
        // Minutes unpadded in the East Asian times (21:01).
        let t = tokyo(2026, 10, 19, 21, 1);
        assert_eq!(at("TIME", "ja-JP", t), "午後9時1分");
        assert_eq!(at("TIME", "ko-KR", t), "오후 9시 1분");
        assert_eq!(at("TIME", "zh-CN", t), "21时1分");
        assert_eq!(at("TIME", "en-US", t), "9:01 PM");
        // `DATE \l`, `\* MERGEFORMAT` and `\@ ""`; `EDITTIME`; `PRINTDATE` is now, whole minutes.
        assert_eq!(at("DATE \\* MERGEFORMAT", "en-US", now), "10/19/26");
        assert_eq!(at("DATE \\@ \"\"", "ja-JP", now), "");
        assert_eq!(at("EDITTIME", "ja-JP", now), "0");
        assert_eq!(at("PRINTDATE", "fr-FR", now + 42), "2026/10/19 20:57:00");
        // A document without dates shows now.
        assert_eq!(field_result("CREATEDATE", Some("ja-JP"), now, 9 * 3600, "", ""), "2026/10/19 20:57:00");
    }
}

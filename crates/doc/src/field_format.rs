//! The `\* FORMAT` switch of a field (`{ PAGE \* DBNum2 }`, `{ SEQ 図 \* Aiueo }`, `{ = 12 \* Hex }`):
//! the number formats Word for Mac's field switches draw, with its rounding and error rules, as
//! its window shows them with Japanese editing (`= n \* SWITCH` fields updated in Word).
//!
//! The switch word is matched without regard to case; `roman` and `alphabetic` give lower-case
//! numerals when the word starts with a lower-case letter. A word Word doesn't know shows
//! [`UNKNOWN_SWITCH`]; a value a format can't draw shows [`NOT_REPRESENTABLE`].

use crate::cjk_numbers as cjk;
use crate::section;

/// What Word shows for a `\*` switch word it doesn't know (`\* Thai1`, `\* DBNum5`, `\* Chicago`).
pub const UNKNOWN_SWITCH: &str = "Error! Unknown switch argument.";
/// What Word shows for a value a format can't draw: a negative in a kana format, a roman numeral
/// past 32 767, a value that would round up from zero.
pub const NOT_REPRESENTABLE: &str = "Error! Number cannot be represented in specified format.";

/// A number format a field's `\*` switch names.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FieldNumFormat {
    /// `Arabic`: 1, 2, 3.
    Arabic,
    /// `ArabicDash`: - 1 -, - 2 -.
    ArabicDash,
    /// `roman` / `ROMAN`: i, ii … (upper-case when the switch word starts upper-case).
    Roman {
        upper: bool,
    },
    /// `alphabetic` / `ALPHABETIC`: a … z, aa … zz, aaa …
    Alphabetic {
        upper: bool,
    },
    /// `CardText`: one, twelve, two thousand twenty-four.
    CardText,
    /// `OrdText`: first, twelfth, twenty-first.
    OrdText,
    /// `Ordinal`: 1st, 12th, 21st.
    Ordinal,
    /// `Hex`: C, 17, 2F.
    Hex,
    /// `DollarText`: twelve and 00/100.
    DollarText,
    /// `Aiueo`: ア イ ウ … ン (46), then from ア again.
    Aiueo,
    /// `Iroha`: イ ロ ハ … ス ン (48), then from イ again.
    Iroha,
    /// `DBChar`: full-width digits of the value as written, decimals and sign included.
    DbChar,
    /// `DBNum1` / `DBNum4` / `KanjiNum1`: one ideograph per digit, 一〇一.
    DbNum1,
    /// `DBNum2` / `KanjiNum2`: Japanese counting, 百一, 二千二十四 (below a million).
    DbNum2,
    /// `DBNum3` / `KanjiNum3`: Japanese legal numerals, 壱百壱, 弐阡弐拾四.
    DbNum3,
    /// `CircleNum`: ① … ⑳, decimal past twenty.
    CircleNum,
    /// `Ganada`: 가 나 다 … 하 (14), cycling.
    Ganada,
    /// `Chosung`: ㄱ ㄴ ㄷ … ㅎ (14), cycling.
    Chosung,
    /// `GB1`: ⒈ … ⒛, `GB2`: ⑴ … ⒇, `GB3`: ① … ⑩, `GB4`: ㈠ … ㈩; decimal past the set.
    Gb1,
    Gb2,
    Gb3,
    Gb4,
    /// `Zodiac1`: the ten stems 甲 … 癸, decimal past ten.
    Zodiac1,
    /// `Zodiac2`: the twelve branches 子 … 亥 (Word writes the eleventh as 戍), decimal past twelve.
    Zodiac2,
    /// `Zodiac3`: the sexagenary cycle 甲子 … 癸亥, cycling.
    Zodiac3,
    /// `Hebrew1`: gematria, א … שצב (392).
    Hebrew1,
    /// `Hebrew2`: letter ordinals א … ת, then תא … (392 at most), led by a right-to-left mark.
    Hebrew2,
    /// `ChineseNum1`: 一百零一.
    ChineseNum1,
    /// `ChineseNum2`: 壹佰零壹.
    ChineseNum2,
    /// `ChineseNum3`: 一〇一.
    ChineseNum3,
    /// Formats WordCraft doesn't draw yet (`ArabicAbjad`, `ArabicAlpha`, `VietCardText`,
    /// `BahtText`): decimal.
    Decimal,
}

/// The `\*` switch words that change a field's letter case or keep its formatting rather than
/// naming a number format; they're skipped when looking for the number format.
fn is_case_switch(w: &str) -> bool {
    ["MERGEFORMAT", "CHARFORMAT", "Upper", "Lower", "FirstCap", "Caps"].iter().any(|k| k.eq_ignore_ascii_case(w))
}

impl FieldNumFormat {
    /// The format a switch word names, `None` for a word Word doesn't know.
    pub fn parse(word: &str) -> Option<FieldNumFormat> {
        let lower = word.to_ascii_lowercase();
        let upper_first = word.chars().next().is_some_and(|c| c.is_ascii_uppercase());
        Some(match lower.as_str() {
            "arabic" => FieldNumFormat::Arabic,
            "arabicdash" => FieldNumFormat::ArabicDash,
            "roman" => FieldNumFormat::Roman { upper: upper_first },
            "alphabetic" => FieldNumFormat::Alphabetic { upper: upper_first },
            "cardtext" => FieldNumFormat::CardText,
            "ordtext" => FieldNumFormat::OrdText,
            "ordinal" => FieldNumFormat::Ordinal,
            "hex" => FieldNumFormat::Hex,
            "dollartext" => FieldNumFormat::DollarText,
            "aiueo" => FieldNumFormat::Aiueo,
            "iroha" => FieldNumFormat::Iroha,
            "dbchar" => FieldNumFormat::DbChar,
            "dbnum1" | "dbnum4" | "kanjinum1" => FieldNumFormat::DbNum1,
            "dbnum2" | "kanjinum2" => FieldNumFormat::DbNum2,
            "dbnum3" | "kanjinum3" => FieldNumFormat::DbNum3,
            "circlenum" => FieldNumFormat::CircleNum,
            "ganada" => FieldNumFormat::Ganada,
            "chosung" => FieldNumFormat::Chosung,
            "gb1" => FieldNumFormat::Gb1,
            "gb2" => FieldNumFormat::Gb2,
            "gb3" => FieldNumFormat::Gb3,
            "gb4" => FieldNumFormat::Gb4,
            "zodiac1" => FieldNumFormat::Zodiac1,
            "zodiac2" => FieldNumFormat::Zodiac2,
            "zodiac3" => FieldNumFormat::Zodiac3,
            "hebrew1" => FieldNumFormat::Hebrew1,
            "hebrew2" => FieldNumFormat::Hebrew2,
            "chinesenum1" => FieldNumFormat::ChineseNum1,
            "chinesenum2" => FieldNumFormat::ChineseNum2,
            "chinesenum3" => FieldNumFormat::ChineseNum3,
            "arabicabjad" | "arabicalpha" | "vietcardtext" | "bahttext" => FieldNumFormat::Decimal,
            _ => return None,
        })
    }

    /// The number format a field instruction's `\*` switch names: [`Switch::None`] without one
    /// (or with only case switches), [`Switch::Unknown`] for a word Word doesn't know.
    pub fn of_instr(instr: &str) -> Switch {
        let mut it = instr.split_whitespace();
        while let Some(w) = it.next() {
            // `\* roman`, or written together, `\*roman` (and `\*roman\*MERGEFORMAT`).
            let Some(rest) = w.strip_prefix("\\*") else { continue };
            let v = if rest.is_empty() {
                let Some(v) = it.next() else { break };
                v
            } else {
                rest
            };
            let v = v.split('\\').next().unwrap_or(v);
            if is_case_switch(v) {
                continue;
            }
            return FieldNumFormat::parse(v).map_or(Switch::Unknown, Switch::Format);
        }
        Switch::None
    }

    /// The format that a `{ PAGE }`, `{ NUMPAGES }` or `{ SEQ }` field's instruction names, when
    /// it names one WordCraft knows.
    pub fn switch_of(instr: &str) -> Option<FieldNumFormat> {
        match FieldNumFormat::of_instr(instr) {
            Switch::Format(f) => Some(f),
            Switch::None | Switch::Unknown => None,
        }
    }

    /// `n` in this format, `None` when the format can't draw it.
    pub fn format_int(self, n: u32) -> Option<String> {
        const I32_MAX: u32 = i32::MAX as u32;
        if n > I32_MAX {
            return None;
        }
        let cycle = |set: &str, len: u32| -> Option<String> {
            if n == 0 {
                return Some("0".into());
            }
            let i = (n - 1) % len;
            set.chars().nth(i as usize).map(|c| c.to_string())
        };
        Some(match self {
            FieldNumFormat::Arabic | FieldNumFormat::Decimal => n.to_string(),
            FieldNumFormat::ArabicDash => format!("- {n} -"),
            FieldNumFormat::Roman { upper } => {
                if n > 32_767 {
                    return None;
                }
                let r = section::roman(n);
                if upper { r.to_uppercase() } else { r }
            }
            FieldNumFormat::Alphabetic { upper } => {
                if n > 780 {
                    return None;
                }
                let l = section::letters(n);
                if upper { l.to_uppercase() } else { l }
            }
            FieldNumFormat::CardText => {
                if n > 999_999 {
                    return None;
                }
                section::cardinal_text(n).to_lowercase()
            }
            FieldNumFormat::OrdText => {
                if n > 999_999 {
                    return None;
                }
                section::ordinal_text(n).to_lowercase()
            }
            FieldNumFormat::Ordinal => section::NumFormat::Ordinal.format(n),
            FieldNumFormat::Hex => format!("{n:X}"),
            FieldNumFormat::DollarText => {
                if n > 999_999 {
                    return None;
                }
                format!("{} and 00/100", section::cardinal_text(n).to_lowercase())
            }
            FieldNumFormat::Aiueo => cycle(cjk::AIUEO, 46)?,
            FieldNumFormat::Iroha => cycle(cjk::IROHA, 48)?,
            FieldNumFormat::DbChar => cjk::decimal_full_width(i64::from(n)),
            FieldNumFormat::DbNum1 | FieldNumFormat::ChineseNum3 => cjk::ideograph_digital(n),
            FieldNumFormat::DbNum2 => {
                if n >= 1_000_000 {
                    return None;
                }
                cjk::japanese_counting(n)
            }
            FieldNumFormat::DbNum3 => cjk::japanese_legal(n),
            FieldNumFormat::CircleNum => cjk::decimal_enclosed_circle(n),
            FieldNumFormat::Ganada => cycle(cjk::GANADA, 14)?,
            FieldNumFormat::Chosung => cycle(cjk::CHOSUNG, 14)?,
            FieldNumFormat::Gb1 => cjk::decimal_enclosed_fullstop(n),
            FieldNumFormat::Gb2 => cjk::decimal_enclosed_paren(n),
            FieldNumFormat::Gb3 => cjk::decimal_enclosed_circle_chinese(n),
            FieldNumFormat::Gb4 => cjk::enclosed(n, 0x3220, 10),
            FieldNumFormat::Zodiac1 => cjk::ideograph_traditional(n),
            FieldNumFormat::Zodiac2 => cjk::ideograph_zodiac(n),
            FieldNumFormat::Zodiac3 => cjk::ideograph_zodiac_traditional(n),
            FieldNumFormat::Hebrew1 => hebrew_gematria(n)?,
            FieldNumFormat::Hebrew2 => hebrew_ordinal(n)?,
            FieldNumFormat::ChineseNum1 => section::NumFormat::ChineseCountingThousand.format(n),
            FieldNumFormat::ChineseNum2 => section::NumFormat::ChineseLegalSimplified.format(n),
        })
    }

    /// Whether the format draws a negative value (`-5`, `-3rd`, `- -5 -`); the others show
    /// [`NOT_REPRESENTABLE`] for one.
    fn draws_negative(self) -> bool {
        matches!(self, FieldNumFormat::Arabic | FieldNumFormat::Decimal | FieldNumFormat::Ordinal | FieldNumFormat::ArabicDash)
    }

    /// A formula's value in this format, as Word shows it: rounded half away from zero (a value
    /// from 0.5 below 1 can't be shown), within the 32-bit range, negatives only where the format
    /// draws them; `DBChar` writes the value as it is in full-width digits.
    pub fn format_value(self, v: f64) -> String {
        if !v.is_finite() {
            return NOT_REPRESENTABLE.into();
        }
        if self == FieldNumFormat::DbChar {
            return full_width_text(&v.to_string());
        }
        if (0.5..1.0).contains(&v) {
            return NOT_REPRESENTABLE.into();
        }
        let r = v.round();
        if r.abs() > i32::MAX as f64 {
            return NOT_REPRESENTABLE.into();
        }
        let r = r as i32;
        if r < 0 {
            if !self.draws_negative() {
                return NOT_REPRESENTABLE.into();
            }
            let n = r.unsigned_abs();
            return match self {
                FieldNumFormat::ArabicDash => format!("- -{n} -"),
                FieldNumFormat::Ordinal => format!("-{}", section::NumFormat::Ordinal.format(n)),
                _ => format!("-{n}"),
            };
        }
        self.format_int(r.unsigned_abs()).unwrap_or_else(|| NOT_REPRESENTABLE.into())
    }
}

/// What a field instruction's `\*` switch names.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Switch {
    /// No `\*` switch, or only case switches (`\* MERGEFORMAT`, `\* Upper`).
    None,
    /// A number format.
    Format(FieldNumFormat),
    /// A word Word doesn't know; the field shows [`UNKNOWN_SWITCH`].
    Unknown,
}

/// Digits, the sign and the decimal point in their full-width forms.
fn full_width_text(s: &str) -> String {
    s.chars()
        .map(|c| match c {
            '0'..='9' => c.to_digit(10).and_then(|d| char::from_u32(0xFF10 + d)).unwrap_or(c),
            '-' => '－',
            '.' => '．',
            _ => c,
        })
        .collect()
}

const HEBREW_ONES: [char; 9] = ['א', 'ב', 'ג', 'ד', 'ה', 'ו', 'ז', 'ח', 'ט'];
const HEBREW_TENS: [char; 9] = ['י', 'כ', 'ל', 'מ', 'נ', 'ס', 'ע', 'פ', 'צ'];
const HEBREW_HUNDREDS: [char; 3] = ['ק', 'ר', 'ש'];
/// The 22 letters `Hebrew2` counts with (no final forms).
const HEBREW_LETTERS: [char; 22] = ['א', 'ב', 'ג', 'ד', 'ה', 'ו', 'ז', 'ח', 'ט', 'י', 'כ', 'ל', 'מ', 'נ', 'ס', 'ע', 'פ', 'צ', 'ק', 'ר', 'ש', 'ת'];
/// The largest value Word's Hebrew formats draw.
const HEBREW_MAX: u32 = 392;

/// Gematria as Word's `Hebrew1` writes it: hundreds ק ר ש, tens י … צ, ones א … ט, 15 and 16 as
/// טו and טז; nothing for zero or past 392.
fn hebrew_gematria(n: u32) -> Option<String> {
    if n == 0 || n > HEBREW_MAX {
        return None;
    }
    let mut out = String::new();
    if let Some(i) = (n / 100).checked_sub(1) {
        out.extend(HEBREW_HUNDREDS.get(i as usize));
    }
    let rest = n % 100;
    match rest {
        15 => out.push_str("טו"),
        16 => out.push_str("טז"),
        _ => {
            if let Some(i) = (rest / 10).checked_sub(1) {
                out.extend(HEBREW_TENS.get(i as usize));
            }
            if let Some(i) = (rest % 10).checked_sub(1) {
                out.extend(HEBREW_ONES.get(i as usize));
            }
        }
    }
    Some(out)
}

/// Letter ordinals as Word's `Hebrew2` writes them: א … ת, then a ת for each full 22 and the
/// letter of the remainder, led by a right-to-left mark; nothing for zero or past 392.
fn hebrew_ordinal(n: u32) -> Option<String> {
    if n == 0 || n > HEBREW_MAX {
        return None;
    }
    let (full, rest) = ((n - 1) / 22, (n - 1) % 22);
    let mut out = String::from("\u{200F}");
    out.extend(std::iter::repeat_n('ת', full as usize));
    out.extend(HEBREW_LETTERS.get(rest as usize));
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn f(sw: &str, v: f64) -> String {
        FieldNumFormat::parse(sw).map(|f| f.format_value(v)).unwrap_or_else(|| UNKNOWN_SWITCH.into())
    }

    fn row(sw: &str, vals: &[f64]) -> String {
        vals.iter().map(|&v| f(sw, v)).collect::<Vec<_>>().join(" | ")
    }

    const E: &str = "Error! Number cannot be represented in specified format.";

    /// `= n \* SWITCH` fields updated in Word for Mac with Japanese editing (1, 2, 5, 12, 23, 47,
    /// 101, 2024, 0).
    #[test]
    fn east_asian_switches_as_word_for_mac_writes_them() {
        let v = [1.0, 2.0, 5.0, 12.0, 23.0, 47.0, 101.0, 2024.0, 0.0];
        assert_eq!(row("Aiueo", &v), "ア | イ | オ | シ | ヌ | ア | ケ | ン | 0");
        assert_eq!(row("aiueo", &v), "ア | イ | オ | シ | ヌ | ア | ケ | ン | 0");
        assert_eq!(row("Iroha", &v), "イ | ロ | ホ | ヲ | ム | ス | ホ | チ | 0");
        assert_eq!(row("DBChar", &v), "１ | ２ | ５ | １２ | ２３ | ４７ | １０１ | ２０２４ | ０");
        assert_eq!(row("DBNum1", &v), "一 | 二 | 五 | 一二 | 二三 | 四七 | 一〇一 | 二〇二四 | 〇");
        assert_eq!(row("DBNum2", &v), "一 | 二 | 五 | 十二 | 二十三 | 四十七 | 百一 | 二千二十四 | 〇");
        assert_eq!(row("DBNum3", &v), "壱 | 弐 | 伍 | 壱拾弐 | 弐拾参 | 四拾七 | 壱百壱 | 弐阡弐拾四 | 〇");
        assert_eq!(row("DBNum4", &v), row("DBNum1", &v));
        assert_eq!(row("KanjiNum1", &v), row("DBNum1", &v));
        assert_eq!(row("KanjiNum2", &v), row("DBNum2", &v));
        assert_eq!(row("KanjiNum3", &v), row("DBNum3", &v));
        assert_eq!(row("dbnum1", &v), row("DBNum1", &v));
        assert_eq!(row("ArabicDash", &v), "- 1 - | - 2 - | - 5 - | - 12 - | - 23 - | - 47 - | - 101 - | - 2024 - | - 0 -");
        assert_eq!(row("CircleNum", &v), "① | ② | ⑤ | ⑫ | 23 | 47 | 101 | 2024 | 0");
        assert_eq!(row("Ganada", &v), "가 | 나 | 마 | 타 | 자 | 마 | 다 | 아 | 0");
        assert_eq!(row("Chosung", &v), "ㄱ | ㄴ | ㅁ | ㅌ | ㅈ | ㅁ | ㄷ | ㅇ | 0");
        assert_eq!(row("GB1", &v), "⒈ | ⒉ | ⒌ | ⒓ | 23 | 47 | 101 | 2024 | 0");
        assert_eq!(row("GB2", &v), "⑴ | ⑵ | ⑸ | ⑿ | 23 | 47 | 101 | 2024 | 0");
        assert_eq!(row("GB3", &v), "① | ② | ⑤ | 12 | 23 | 47 | 101 | 2024 | 0");
        assert_eq!(row("GB4", &v), "㈠ | ㈡ | ㈤ | 12 | 23 | 47 | 101 | 2024 | 0");
        assert_eq!(row("Zodiac1", &v), "甲 | 乙 | 戊 | 12 | 23 | 47 | 101 | 2024 | 0");
        assert_eq!(row("Zodiac2", &v), "子 | 丑 | 辰 | 亥 | 23 | 47 | 101 | 2024 | 0");
        assert_eq!(row("Zodiac3", &v), "甲子 | 乙丑 | 戊辰 | 乙亥 | 丙戍 | 庚戍 | 甲辰 | 丁未 | 0");
        assert_eq!(row("Hex", &v), "1 | 2 | 5 | C | 17 | 2F | 65 | 7E8 | 0");
        assert_eq!(row("Roman", &v), "I | II | V | XII | XXIII | XLVII | CI | MMXXIV | ");
        assert_eq!(row("roman", &[12.0]), "xii");
        assert_eq!(row("Hebrew1", &v), format!("א | ב | ה | יב | כג | מז | קא | {E} | {E}"));
        assert_eq!(
            row("Hebrew2", &v),
            format!("\u{200F}א | \u{200F}ב | \u{200F}ה | \u{200F}ל | \u{200F}תא | \u{200F}תתג | \u{200F}תתתתמ | {E} | {E}")
        );
        assert_eq!(
            row("DollarText", &v),
            "one and 00/100 | two and 00/100 | five and 00/100 | twelve and 00/100 | twenty-three and 00/100 | forty-seven and 00/100 | one hundred one and 00/100 | two thousand twenty-four and 00/100 | zero and 00/100"
        );
        for unknown in ["Thai1", "Thai2", "Thai3", "DBNum5", "Chicago"] {
            assert_eq!(f(unknown, 1.0), UNKNOWN_SWITCH, "{unknown}");
        }
    }

    /// The sets cycle (Aiueo by 46, Iroha by 48 with ン, Ganada and Chosung by 14, Zodiac3 by 60)
    /// or go decimal past their end (CircleNum, GB1–4, Zodiac1–2).
    #[test]
    fn cycles_and_ends_as_word_for_mac() {
        assert_eq!(row("Aiueo", &[46.0, 47.0, 92.0, 93.0, 100_000.0]), "ン | ア | ン | ア | レ");
        assert_eq!(row("Iroha", &[46.0, 47.0, 48.0, 49.0, 96.0, 97.0]), "セ | ス | ン | イ | ン | イ");
        assert_eq!(row("CircleNum", &[10.0, 20.0, 21.0, 50.0, 2_147_483_647.0]), "⑩ | ⑳ | 21 | 50 | 2147483647");
        assert_eq!(row("GB1", &[20.0, 21.0]), "⒛ | 21");
        assert_eq!(row("GB2", &[20.0, 21.0]), "⒇ | 21");
        assert_eq!(row("GB3", &[10.0, 11.0]), "⑩ | 11");
        assert_eq!(row("GB4", &[10.0, 11.0]), "㈩ | 11");
        assert_eq!(row("Zodiac1", &[10.0, 11.0]), "癸 | 11");
        assert_eq!(row("Zodiac2", &[10.0, 11.0, 12.0, 13.0]), "酉 | 戍 | 亥 | 13");
        assert_eq!(
            row("Zodiac3", &[60.0, 61.0, 120.0, 121.0, 11.0, 24.0, 35.0, 2_147_483_647.0]),
            "癸亥 | 甲子 | 癸亥 | 甲子 | 甲戍 | 丁亥 | 戊戍 | 庚午"
        );
        assert_eq!(row("Ganada", &[14.0, 15.0, 28.0, 29.0, 2_147_483_647.0]), "하 | 가 | 하 | 가 | 가");
        assert_eq!(row("Chosung", &[14.0, 15.0]), "ㅎ | ㄱ");
    }

    /// Japanese numerals: digit by digit to the 32-bit limit, counting below a million, legal
    /// numerals to the limit; DBChar writes the value as it is.
    #[test]
    fn japanese_numerals_as_word_for_mac() {
        assert_eq!(
            row("DBNum1", &[10.0, 100.0, 1000.0, 10000.0, 123_456_789.0, 1e9, 2_147_483_647.0, 2_147_483_648.0, 1e10]),
            format!("一〇 | 一〇〇 | 一〇〇〇 | 一〇〇〇〇 | 一二三四五六七八九 | 一〇〇〇〇〇〇〇〇〇 | 二一四七四八三六四七 | {E} | {E}")
        );
        assert_eq!(
            row(
                "DBNum2",
                &[10.0, 11.0, 20.0, 100.0, 110.0, 1000.0, 1001.0, 10000.0, 10001.0, 20000.0, 100_000.0, 999_999.0, 1_000_000.0, 123_456_789.0]
            ),
            format!("十 | 十一 | 二十 | 百 | 百十 | 千 | 千一 | 一万 | 一万一 | 二万 | 十万 | 九十九万九千九百九十九 | {E} | {E}")
        );
        assert_eq!(
            row("DBNum3", &[10.0, 11.0, 20.0, 100.0, 110.0, 1000.0, 1001.0, 10000.0, 10001.0, 20000.0, 100_000.0, 123_456_789.0, 1e9, 1e10]),
            format!(
                "壱拾 | 壱拾壱 | 弐拾 | 壱百 | 壱百壱拾 | 壱阡 | 壱阡壱 | 壱萬 | 壱萬壱 | 弐萬 | 壱拾萬 | 壱億弐阡参百四拾伍萬六阡七百八拾九 | 壱拾億 | {E}"
            )
        );
        assert_eq!(
            row("DBChar", &[-5.0, 3.5, 1_234_567.0, 1e10, 0.4, -0.5, 1e16, 1e17, 123.456, 2_147_483_648.0]),
            "－５ | ３．５ | １２３４５６７ | １００００００００００ | ０．４ | －０．５ | １００００００００００００００００ | １０００００００００００００００００ | １２３．４５６ | ２１４７４８３６４８"
        );
    }

    /// Word rounds half away from zero, can't show a value from 0.5 below 1, draws negatives only
    /// as Arabic, Ordinal and ArabicDash, and stops at the 32-bit limit.
    #[test]
    fn rounding_negatives_and_limits_as_word_for_mac() {
        assert_eq!(
            row("Arabic", &[0.6, 0.9, 0.99, 0.51, 1.4, 1.49, 1.5, 10.5, 0.49, -0.5, -1.5, -2.5, 1e9, 1e10, -5.0, 3.5, 2.5, 0.5]),
            format!("{E} | {E} | {E} | {E} | 1 | 1 | 2 | 11 | 0 | -1 | -2 | -3 | 1000000000 | {E} | -5 | 4 | 3 | {E}")
        );
        assert_eq!(
            row("Aiueo", &[0.6, 0.99, 1.49, 10.5, 46.5, 0.4, 0.5, 2.5, -1.0, 1.5]),
            format!("{E} | {E} | ア | サ | ア | 0 | {E} | ウ | {E} | イ")
        );
        assert_eq!(row("DBNum1", &[-5.0, 3.5, 46.0, 2.5, 0.4, 0.5, 0.6, -0.4]), format!("{E} | 四 | 四六 | 三 | 〇 | {E} | {E} | 〇"));
        assert_eq!(
            row("DBNum2", &[-5.0, 3.5, 2.25, 0.6, 999_999.5, 999_999.4, 2_147_483_647.0]),
            format!("{E} | 四 | 二 | {E} | {E} | 九十九万九千九百九十九 | {E}")
        );
        assert_eq!(row("DBNum3", &[-5.0, 3.5, 2.5]), format!("{E} | 四 | 参"));
        assert_eq!(row("CircleNum", &[-1.0, 1.5, 2.5, 0.4, 0.5, 0.6]), format!("{E} | ② | ③ | 0 | {E} | {E}"));
        assert_eq!(row("Hex", &[255.0, 4096.0, -5.0, 3.5, 2.5, 0.5, 4_294_967_295.0]), format!("FF | 1000 | {E} | 4 | 3 | {E} | {E}"));
        assert_eq!(row("ArabicDash", &[-5.0, 3.5, 0.6, -0.6, 2_147_483_648.0]), format!("- -5 - | - 4 - | {E} | - -1 - | {E}"));
        assert_eq!(
            row("Ordinal", &[2.5, -1.0, 0.0, 11.0, 12.0, 13.0, 21.0, 22.0, 23.0, 111.0, 112.0, 113.0, 0.6, 2_147_483_648.0, -2.5]),
            format!("3rd | -1st | 0th | 11th | 12th | 13th | 21st | 22nd | 23rd | 111th | 112th | 113th | {E} | {E} | -3rd")
        );
        assert_eq!(
            row("Roman", &[0.5, 4000.0, 5000.0, -1.0, 2.5, 3999.0, 0.6, 32767.0, 32768.0, 100_000.0]),
            format!("{E} | MMMM | MMMMM | {E} | III | MMMCMXCIX | {E} | {} | {E} | {E}", "M".repeat(32) + "DCCLXVII")
        );
        assert_eq!(row("Ganada", &[0.5, 2.5]), format!("{E} | 다"));
        assert_eq!(row("GB3", &[0.0, -1.0]), format!("0 | {E}"));
        assert_eq!(row("Iroha", &[-1.0]), E);
    }

    /// Words and letters: lower-case cardinals and ordinals up to 999 999, letters repeated up to
    /// 780, both with the switch word's case.
    #[test]
    fn text_and_letter_switches_as_word_for_mac() {
        assert_eq!(
            row(
                "CardText",
                &[12.0, 2024.0, 2.5, -1.0, 0.0, 1_000_000.0, 999_999.0, 100_000.0, 123_456.0, 1.0, 21.0, 100.0, 101.0, 1000.0, 1001.0, 1100.0]
            ),
            format!(
                "twelve | two thousand twenty-four | three | {E} | zero | {E} | nine hundred ninety-nine thousand nine hundred ninety-nine | one hundred thousand | one hundred twenty-three thousand four hundred fifty-six | one | twenty-one | one hundred | one hundred one | one thousand | one thousand one | one thousand one hundred"
            )
        );
        assert_eq!(
            row("OrdText", &[1.0, 2.0, 3.0, 11.0, 21.0, 100.0, 101.0, 1000.0, 999_999.0, 0.0, 2.5, -1.0]),
            format!(
                "first | second | third | eleventh | twenty-first | one hundredth | one hundred first | one thousandth | nine hundred ninety-nine thousand nine hundred ninety-ninth | zeroth | third | {E}"
            )
        );
        assert_eq!(
            row("ALPHABETIC", &[0.0, 27.0, -1.0, 2.5, 26.0, 52.0, 53.0, 702.0, 703.0, 780.0, 781.0]),
            format!(" | AA | {E} | C | Z | ZZ | AAA | {} | {} | {} | {E}", "Z".repeat(27), "A".repeat(28), "Z".repeat(30))
        );
        assert_eq!(row("alphabetic", &[0.0, 1.0, 26.0, 27.0, 53.0]), " | a | z | aa | aaa");
        assert_eq!(row("Alphabetic", &[27.0]), "AA");
    }

    /// Hebrew1 is gematria (15 and 16 as טו and טז) and Hebrew2 counts letters, both to 392.
    #[test]
    fn hebrew_switches_as_word_for_mac() {
        assert_eq!(
            row(
                "Hebrew1",
                &[
                    15.0, 16.0, 100.0, 199.0, 200.0, 299.0, 300.0, 301.0, 350.0, 310.0, 345.0, 349.0, 17.0, 115.0, 116.0, 215.0, 315.0, 316.0, 270.0,
                    296.0, 360.0, 390.0, 391.0, 392.0, 395.0, 399.0, 400.0, 1000.0
                ]
            ),
            format!(
                "טו | טז | ק | קצט | ר | רצט | ש | שא | שנ | שי | שמה | שמט | יז | קטו | קטז | רטו | שטו | שטז | רע | רצו | שס | שצ | שצא | שצב | {E} | {E} | {E} | {E}"
            )
        );
        let h2 = |n: u32, s: &str| assert_eq!(f("Hebrew2", n as f64), format!("\u{200F}{s}"), "{n}");
        h2(1, "א");
        h2(10, "י");
        h2(11, "כ");
        h2(21, "ש");
        h2(22, "ת");
        h2(23, "תא");
        h2(44, "תת");
        h2(45, "תתא");
        h2(66, "תתת");
        h2(67, "תתתא");
        h2(100, "תתתתל");
        h2(300, &("ת".repeat(13) + "נ"));
        h2(374, &"ת".repeat(17));
        h2(375, &("ת".repeat(17) + "א"));
        h2(380, &("ת".repeat(17) + "ו"));
        h2(390, &("ת".repeat(17) + "ע"));
        h2(392, &("ת".repeat(17) + "צ"));
        for n in [0.0, 394.0, 396.0, 400.0, 500.0, 999.0] {
            assert_eq!(f("Hebrew2", n), E, "{n}");
        }
    }

    #[test]
    fn the_switch_is_read_from_the_instruction() {
        assert_eq!(FieldNumFormat::of_instr("PAGE \\* DBNum2"), Switch::Format(FieldNumFormat::DbNum2));
        assert_eq!(FieldNumFormat::of_instr("SEQ 図 \\* MERGEFORMAT \\* Aiueo"), Switch::Format(FieldNumFormat::Aiueo));
        assert_eq!(FieldNumFormat::of_instr("PAGE \\* MERGEFORMAT"), Switch::None);
        assert_eq!(FieldNumFormat::of_instr("PAGE"), Switch::None);
        assert_eq!(FieldNumFormat::of_instr("= 5 \\* Thai1"), Switch::Unknown);
        assert_eq!(FieldNumFormat::of_instr("= 5 \\*"), Switch::None);
        assert_eq!(FieldNumFormat::parse("ROMAN"), Some(FieldNumFormat::Roman { upper: true }));
        assert_eq!(FieldNumFormat::parse("roman"), Some(FieldNumFormat::Roman { upper: false }));
        assert_eq!(FieldNumFormat::Arabic.format_value(f64::NAN), E);
        assert_eq!(FieldNumFormat::Arabic.format_value(f64::INFINITY), E);
        assert_eq!(FieldNumFormat::Aiueo.format_int(u32::MAX), None);
    }
}

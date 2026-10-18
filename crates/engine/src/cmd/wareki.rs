//! Word's Japanese date pictures: 和暦 (the imperial era) and 漢数字 (kanji numerals), as Word
//! for Mac evaluates `DATE \@ "…"` fields (observed 2026-10-18, English interface, any text
//! language — the tokens do not depend on the language of the text):
//!
//! | picture | result | |
//! |---|---|---|
//! | `ggg` | 令和 | the era's name; `gggg` is `ggg` + `g` |
//! | `gg` | 令 | its first character |
//! | `g` | R | its Latin initial |
//! | `GG` / `G` | 令和 / 令 | the capital forms |
//! | `e` / `ee` | 8 / 08 | the year within the era (`eee` is `ee` + `e`) |
//! | `E` | 八 | the era year in kanji (十八, 二十, 三十一 for 18, 20, 31) |
//! | `EE` | 二○二六 | the western year digit by digit, ○ for zero |
//! | `aaa` | 日 | the weekday's kanji; `a` and `aa` are the letter `a` (`aaaa` is 日a) |
//! | `A` | 十八 | the day of the month in kanji |
//! | `O` | 十 | the month in kanji |
//! | `ampm` | 午前 / 午後 | lowercase, or `AMPM`, `amPM`, `AMpm`; `am/pm` stays AM / PM |
//!
//! The era table is Windows' (and so Word's): 明治 from 1868-09-08, 大正 from 1912-07-30, 昭和
//! from 1926-12-25, 平成 from 1989-01-08, 令和 from 2019-05-01. The first year of an era is `1`
//! (Word writes 令和1年, never 元年, unless the picture spells 元 itself). Before 明治 the era
//! tokens give nothing and `e` the western year.

/// One imperial era.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Era {
    /// 令和
    pub name: &'static str,
    /// 令
    pub short: &'static str,
    /// R
    pub latin: &'static str,
    /// First day (year, month, day).
    pub start: (i64, u32, u32),
}

/// Newest last.
pub const ERAS: [Era; 5] = [
    Era { name: "明治", short: "明", latin: "M", start: (1868, 9, 8) },
    Era { name: "大正", short: "大", latin: "T", start: (1912, 7, 30) },
    Era { name: "昭和", short: "昭", latin: "S", start: (1926, 12, 25) },
    Era { name: "平成", short: "平", latin: "H", start: (1989, 1, 8) },
    Era { name: "令和", short: "令", latin: "R", start: (2019, 5, 1) },
];

/// The era a date falls in and its year within the era (1 for the first year), or `None`
/// before 明治.
pub fn era_of(y: i64, m: u32, d: u32) -> Option<(&'static Era, i64)> {
    let era = ERAS.iter().rev().find(|e| (y, m, d) >= e.start)?;
    Some((era, y.saturating_sub(era.start.0).saturating_add(1)))
}

const DIGITS: [&str; 10] = ["○", "一", "二", "三", "四", "五", "六", "七", "八", "九"];

/// A number as Word's `A`, `O` and `E` write it: positional kanji under 100 (八, 十, 十八, 二十,
/// 三十一); larger or negative numbers digit by digit (as `EE` writes a year).
pub fn kanji_number(n: i64) -> String {
    if !(0..100).contains(&n) {
        return kanji_digits(n);
    }
    let (tens, ones) = (n / 10, n % 10);
    let mut out = String::new();
    if tens > 1 {
        out.push_str(DIGITS.get(tens as usize).copied().unwrap_or(""));
    }
    if tens >= 1 {
        out.push('十');
    }
    if ones > 0 || n == 0 {
        out.push_str(DIGITS.get(ones as usize).copied().unwrap_or(""));
    }
    out
}

/// A number digit by digit in kanji, ○ for zero (二○二六 for 2026), as Word's `EE` writes the
/// year.
pub fn kanji_digits(n: i64) -> String {
    let mut out = String::new();
    if n < 0 {
        out.push('-');
    }
    for c in n.unsigned_abs().to_string().chars() {
        let i = c.to_digit(10).unwrap_or(0) as usize;
        out.push_str(DIGITS.get(i).copied().unwrap_or(""));
    }
    out
}

/// The weekday's kanji, Sunday first.
pub const WEEKDAYS: [&str; 7] = ["日", "月", "火", "水", "木", "金", "土"];

/// The weekday's kanji for a day count since 1970-01-01 (a Thursday).
pub fn weekday_kanji(days_since: i64) -> &'static str {
    WEEKDAYS.get((days_since.saturating_add(4)).rem_euclid(7) as usize).copied().unwrap_or("日")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn eras_change_on_windows_dates() {
        let era = |y, m, d| era_of(y, m, d).map(|(e, n)| format!("{}{n}", e.name));
        assert_eq!(era(2026, 10, 18).as_deref(), Some("令和8"));
        assert_eq!(era(2019, 5, 1).as_deref(), Some("令和1"));
        assert_eq!(era(2019, 4, 30).as_deref(), Some("平成31"));
        assert_eq!(era(1989, 1, 8).as_deref(), Some("平成1"));
        assert_eq!(era(1989, 1, 7).as_deref(), Some("昭和64"));
        assert_eq!(era(1926, 12, 25).as_deref(), Some("昭和1"));
        assert_eq!(era(1926, 12, 24).as_deref(), Some("大正15"));
        assert_eq!(era(1912, 7, 30).as_deref(), Some("大正1"));
        assert_eq!(era(1912, 7, 29).as_deref(), Some("明治45"));
        assert_eq!(era(1868, 9, 8).as_deref(), Some("明治1"));
        assert_eq!(era(1868, 9, 7), None);
        assert_eq!(era(i64::MIN, 1, 1), None);
    }

    #[test]
    fn kanji_numbers_are_positional_under_a_hundred() {
        assert_eq!(kanji_number(0), "○");
        assert_eq!(kanji_number(8), "八");
        assert_eq!(kanji_number(10), "十");
        assert_eq!(kanji_number(18), "十八");
        assert_eq!(kanji_number(20), "二十");
        assert_eq!(kanji_number(31), "三十一");
        assert_eq!(kanji_number(64), "六十四");
        assert_eq!(kanji_number(2026), "二○二六");
        assert_eq!(kanji_digits(2026), "二○二六");
        assert_eq!(kanji_digits(1989), "一九八九");
        assert_eq!(kanji_digits(-5), "-五");
        assert_eq!(kanji_number(i64::MIN).chars().count(), 20);
    }

    #[test]
    fn weekdays_start_on_sunday() {
        assert_eq!(weekday_kanji(0), "木"); // 1970-01-01
        assert_eq!(weekday_kanji(3), "日");
        assert_eq!(weekday_kanji(-4), "日");
        assert!(WEEKDAYS.contains(&weekday_kanji(i64::MIN)));
        assert!(WEEKDAYS.contains(&weekday_kanji(i64::MAX)));
    }
}

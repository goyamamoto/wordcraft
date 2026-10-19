//! East Asian list and page number formats (ECMA-376 §17.18.59 ST_NumberFormat): Chinese,
//! Japanese and Korean counting systems (with units 十/百/千 and myriads 万/億), legal (financial)
//! numerals, digit-by-digit ideographs, the heavenly stems and earthly branches, kana and hangul
//! sequences and enclosed numbers — as Word for Mac draws list labels in them, read through its
//! object model (`list string`) with Japanese editing (`nf1`–`nf3` probes, 2026-10-19: every
//! format at 0–30, the sequence lengths, 99–120, 999–1111, 9999–11000, 32767/8, 99999/100000,
//! 999999/1000000, 12345678, 123456789, 10⁹, 2³¹ and 2³²−1).
//!
//! A sequence (kana, hangul, stems, branches, the sixty-year cycle) cycles past its end; an
//! enclosed set and the stems and branches show decimal past theirs; 0 is `0` in all of them.
//! The counting systems with units stop at 999 999 (nothing is drawn beyond), Japanese legal
//! numerals and the digit-by-digit forms go on; Word's Korean native numerals stop at 9 999 999
//! and Japanese ten-thousand digits at 9999.

/// Where a counting system drops the "one" before a unit.
#[derive(Clone, Copy, PartialEq, Eq)]
enum OneRule {
    /// Always written: legal numerals (壹拾, 壱百, 壹拾萬).
    Never,
    /// Dropped before a ten that starts the number and sits in its ones group: 十, 十一, 二十,
    /// but 一百一十, 一千〇一十, 一十万, 一十一万 (Chinese).
    ChineseLeadingTen,
    /// Dropped before 十 and 百 always and before 千 at the start of the number: 十, 百, 千,
    /// 千百, 一万百, 一万一千, 十一万一千, 一万(Japanese).
    Japanese,
    /// As Japanese, and dropped before 만 too — 만, 만일천, 십만 for 110 000 (Korean).
    Korean,
}

/// A myriad-based counting system (groups of four digits, like 1|2345|6789).
struct Counting {
    /// The digits 1 to 9 (index 0 is the lone zero).
    digits: [char; 10],
    /// The units ten, hundred, thousand.
    units: [char; 3],
    /// The myriads: ten thousand, hundred million.
    myriads: [char; 2],
    /// The character written once for a run of zeros between two digits (Chinese 零/〇); `None`
    /// skips zeros (Japanese, Korean).
    zero: Option<char>,
    one: OneRule,
    /// The largest value drawn; nothing is drawn past it.
    limit: u32,
}

impl Counting {
    fn format(&self, n: u32) -> String {
        if n > self.limit {
            return String::new();
        }
        if n == 0 {
            return self.digits.first().map(char::to_string).unwrap_or_default();
        }
        let groups = [n / 100_000_000, (n / 10_000) % 10_000, n % 10_000];
        let mut out = String::new();
        let mut zero_pending = false;
        for (gi, &g) in groups.iter().enumerate() {
            if g == 0 {
                zero_pending |= !out.is_empty();
                continue;
            }
            let ds = [g / 1000, (g / 100) % 10, (g / 10) % 10, g % 10];
            for (pos, &d) in ds.iter().enumerate() {
                if d == 0 {
                    zero_pending |= !out.is_empty();
                    continue;
                }
                if zero_pending && let Some(z) = self.zero {
                    out.push(z);
                }
                zero_pending = false;
                // pos 0 = thousands, 1 = hundreds, 2 = tens, 3 = ones.
                let unit = match pos {
                    0 => self.units.get(2),
                    1 => self.units.get(1),
                    2 => self.units.first(),
                    _ => None,
                };
                let omit = d == 1
                    && match self.one {
                        OneRule::Never => false,
                        OneRule::ChineseLeadingTen => pos == 2 && gi == 2 && out.is_empty(),
                        OneRule::Japanese => match pos {
                            1 | 2 => true,
                            0 => out.is_empty(),
                            _ => false,
                        },
                        OneRule::Korean => match pos {
                            1 | 2 => true,
                            0 => out.is_empty(),
                            _ => gi < 2,
                        },
                    };
                if !omit {
                    out.push(self.digits.get(d as usize).copied().unwrap_or('?'));
                }
                if let Some(&u) = unit {
                    out.push(u);
                }
            }
            match gi {
                0 => out.extend(self.myriads.get(1)),
                1 => out.extend(self.myriads.first()),
                _ => {}
            }
        }
        out
    }
}

const CN_DIGITS: [char; 10] = ['〇', '一', '二', '三', '四', '五', '六', '七', '八', '九'];
/// The digits with Word's ring zero (U+25CB) of `chineseCounting`, `taiwaneseCounting` and
/// `taiwaneseDigital`.
const RING_DIGITS: [char; 10] = ['○', '一', '二', '三', '四', '五', '六', '七', '八', '九'];
/// The digits with 零 of `koreanDigital2`.
const LING_DIGITS: [char; 10] = ['零', '一', '二', '三', '四', '五', '六', '七', '八', '九'];
/// The limit of the counting systems with units: Word draws nothing from a million on.
const UNITS_LIMIT: u32 = 999_999;

const CHINESE: Counting = Counting {
    digits: CN_DIGITS,
    units: ['十', '百', '千'],
    myriads: ['万', '亿'],
    zero: Some('〇'),
    one: OneRule::ChineseLeadingTen,
    limit: UNITS_LIMIT,
};
const TAIWANESE: Counting = Counting {
    digits: LING_DIGITS,
    units: ['十', '百', '千'],
    myriads: ['萬', '億'],
    zero: Some('零'),
    one: OneRule::ChineseLeadingTen,
    limit: UNITS_LIMIT,
};
const CHINESE_LEGAL: Counting = Counting {
    digits: ['零', '壹', '贰', '叁', '肆', '伍', '陆', '柒', '捌', '玖'],
    units: ['拾', '佰', '仟'],
    myriads: ['萬', '億'],
    zero: Some('零'),
    one: OneRule::Never,
    limit: UNITS_LIMIT,
};
const TRADITIONAL_LEGAL: Counting = Counting {
    digits: ['零', '壹', '貳', '參', '肆', '伍', '陸', '柒', '捌', '玖'],
    units: ['拾', '佰', '仟'],
    myriads: ['萬', '億'],
    zero: Some('零'),
    one: OneRule::Never,
    limit: UNITS_LIMIT,
};
const JAPANESE: Counting =
    Counting { digits: CN_DIGITS, units: ['十', '百', '千'], myriads: ['万', '億'], zero: None, one: OneRule::Japanese, limit: UNITS_LIMIT };
const JAPANESE_LEGAL: Counting = Counting {
    digits: ['〇', '壱', '弐', '参', '四', '伍', '六', '七', '八', '九'],
    units: ['拾', '百', '阡'],
    myriads: ['萬', '億'],
    zero: None,
    one: OneRule::Never,
    limit: i32::MAX as u32,
};
const KOREAN: Counting = Counting {
    digits: ['영', '일', '이', '삼', '사', '오', '육', '칠', '팔', '구'],
    units: ['십', '백', '천'],
    myriads: ['만', '억'],
    zero: None,
    one: OneRule::Korean,
    limit: UNITS_LIMIT,
};

/// 一, 二 … 十, 十一 … 九十九 (Chinese counting); from 100 on, digit by digit with Word's ring
/// zero (一○○); 0 is ○.
pub(crate) fn chinese_counting(n: u32) -> String {
    if (1..100).contains(&n) { CHINESE.format(n) } else { digits(n, &RING_DIGITS) }
}

/// Chinese counting with every unit: 一百〇一, 一千, 一万, 一十万; nothing from a million on.
pub(crate) fn chinese_counting_thousand(n: u32) -> String {
    CHINESE.format(n)
}

/// Simplified Chinese legal (financial) numerals: 壹, 贰, 叁 … 壹拾, 壹萬; nothing from a
/// million on.
pub(crate) fn chinese_legal_simplified(n: u32) -> String {
    CHINESE_LEGAL.format(n)
}

/// Traditional Chinese legal numerals: 壹, 貳, 參 … 壹拾, 壹萬; nothing from a million on.
pub(crate) fn ideograph_legal_traditional(n: u32) -> String {
    TRADITIONAL_LEGAL.format(n)
}

/// Taiwanese counting: like Chinese counting below 100, digit by digit with ○ above.
pub(crate) fn taiwanese_counting(n: u32) -> String {
    chinese_counting(n)
}

/// Taiwanese counting with every unit: 一百零一, 一千, 一萬; nothing from a million on.
pub(crate) fn taiwanese_counting_thousand(n: u32) -> String {
    TAIWANESE.format(n)
}

/// One ideograph per decimal digit with Word's ring zero: 一, 一○, 一○一.
pub(crate) fn taiwanese_digital(n: u32) -> String {
    digits(n, &RING_DIGITS)
}

/// Japanese counting: 一 … 十, 十一, 百, 千, 一万, 一万一千; nothing from a million on.
pub(crate) fn japanese_counting(n: u32) -> String {
    JAPANESE.format(n)
}

/// Japanese legal numerals: 壱, 弐, 参 … 壱拾, 壱萬, 壱拾億.
pub(crate) fn japanese_legal(n: u32) -> String {
    JAPANESE_LEGAL.format(n)
}

/// Japanese digits up to 9999 (一二, 九九九九); Word draws nothing from 10 000 on.
pub(crate) fn japanese_digital_ten_thousand(n: u32) -> String {
    if n > 9999 { String::new() } else { digits(n, &CN_DIGITS) }
}

/// Sino-Korean numbers in hangul: 일, 이, 삼 … 십, 십일, 백, 만, 만일천; nothing from a
/// million on.
pub(crate) fn korean_counting(n: u32) -> String {
    KOREAN.format(n)
}

/// Sino-Korean digits in hangul, one per digit: 일, 일영, 일일.
pub(crate) fn korean_digital(n: u32) -> String {
    digits(n, &KOREAN.digits)
}

/// One ideograph per decimal digit with 零: 一, 一零, 一零一; nothing from a million on. A lone
/// zero is Word's Korean compatibility form of the character (U+F9B2).
pub(crate) fn korean_digital2(n: u32) -> String {
    match n {
        0 => "\u{F9B2}".into(),
        1..=UNITS_LIMIT => digits(n, &LING_DIGITS),
        _ => String::new(),
    }
}

const KO_ONES: [&str; 10] = ["", "하나", "둘", "셋", "넷", "다섯", "여섯", "일곱", "여덟", "아홉"];
const KO_TENS: [&str; 10] = ["", "열", "스물", "서른", "마흔", "쉰", "예순", "일흔", "여든", "아흔"];

/// Native Korean numbers: 하나, 둘, 셋 … 열, 열하나 … 아흔아홉; Sino-Korean from 100 on (백,
/// 백일, 만, 백만) up to 9 999 999, nothing beyond; 0 is `0`.
pub(crate) fn korean_legal(n: u32) -> String {
    match n {
        0 => "0".into(),
        1..=99 => {
            let (t, o) = ((n / 10) as usize, (n % 10) as usize);
            format!("{}{}", KO_TENS.get(t).copied().unwrap_or(""), KO_ONES.get(o).copied().unwrap_or(""))
        }
        100..=9_999_999 => Counting { limit: 9_999_999, ..KOREAN }.format(n),
        _ => String::new(),
    }
}

/// One ideograph per decimal digit: 一, 一〇, 一〇一.
pub(crate) fn ideograph_digital(n: u32) -> String {
    digits(n, &CN_DIGITS)
}

fn digits(n: u32, set: &[char; 10]) -> String {
    n.to_string().bytes().map(|b| set.get(b.saturating_sub(b'0') as usize).copied().unwrap_or('?')).collect()
}

const STEMS: [char; 10] = ['甲', '乙', '丙', '丁', '戊', '己', '庚', '辛', '壬', '癸'];
/// The earthly branches as Word writes them: 戍 (U+620D), not 戌, for the eleventh.
const BRANCHES: [char; 12] = ['子', '丑', '寅', '卯', '辰', '巳', '午', '未', '申', '酉', '戍', '亥'];

/// The ten heavenly stems: 甲, 乙, 丙 … 癸; decimal past them.
pub(crate) fn ideograph_traditional(n: u32) -> String {
    nth(&STEMS, n)
}

/// The twelve earthly branches: 子, 丑, 寅 … 亥; decimal past them.
pub(crate) fn ideograph_zodiac(n: u32) -> String {
    nth(&BRANCHES, n)
}

/// The sixty-year cycle: 甲子, 乙丑, 丙寅 … 癸亥, then 甲子 again.
pub(crate) fn ideograph_zodiac_traditional(n: u32) -> String {
    if n == 0 {
        return "0".into();
    }
    let i = ((n - 1) % 60) as usize;
    [STEMS.get(i % 10), BRANCHES.get(i % 12)].into_iter().flatten().collect()
}

/// The `n`th (1-based) item of a sequence, or decimal past its end.
fn nth(set: &[char], n: u32) -> String {
    n.checked_sub(1).and_then(|i| set.get(i as usize)).map(|c| c.to_string()).unwrap_or_else(|| n.to_string())
}

/// Code points `first..` enclosing 1 to `count`, decimal beyond.
pub(crate) fn enclosed(n: u32, first: u32, count: u32) -> String {
    if (1..=count).contains(&n)
        && let Some(c) = char::from_u32(first + n - 1)
    {
        return c.to_string();
    }
    n.to_string()
}

/// ① ② ③ … ⑳.
pub(crate) fn decimal_enclosed_circle(n: u32) -> String {
    enclosed(n, 0x2460, 20)
}

/// ① … ⑩ (the Chinese set stops at ten).
pub(crate) fn decimal_enclosed_circle_chinese(n: u32) -> String {
    enclosed(n, 0x2460, 10)
}

/// ⒈ ⒉ ⒊ … ⒛.
pub(crate) fn decimal_enclosed_fullstop(n: u32) -> String {
    enclosed(n, 0x2488, 20)
}

/// ⑴ ⑵ ⑶ … ⒇.
pub(crate) fn decimal_enclosed_paren(n: u32) -> String {
    enclosed(n, 0x2474, 20)
}

/// ㈠ ㈡ ㈢ … ㈩ (parenthesized ideographs, as Word draws `ideographEnclosedCircle`).
pub(crate) fn ideograph_enclosed_circle(n: u32) -> String {
    enclosed(n, 0x3220, 10)
}

/// Full-width digits: １, ２ … １０ (other characters, the sign of a negative, as they are).
pub(crate) fn decimal_full_width(n: i64) -> String {
    n.to_string().chars().map(|c| c.to_digit(10).and_then(|d| char::from_u32(0xFF10 + d)).unwrap_or(c)).collect()
}

pub(crate) const AIUEO: &str = "アイウエオカキクケコサシスセソタチツテトナニヌネノハヒフヘホマミムメモヤユヨラリルレロワヲン";
const AIUEO_HALF: &str = "ｱｲｳｴｵｶｷｸｹｺｻｼｽｾｿﾀﾁﾂﾃﾄﾅﾆﾇﾈﾉﾊﾋﾌﾍﾎﾏﾐﾑﾒﾓﾔﾕﾖﾗﾘﾙﾚﾛﾜｦﾝ";
/// Word's iroha order: the 47 iroha kana then ン.
pub(crate) const IROHA: &str = "イロハニホヘトチリヌルヲワカヨタレソツネナラムウヰノオクヤマケフコエテアサキユメミシヱヒモセスン";
const IROHA_HALF: &str = "ｲﾛﾊﾆﾎﾍﾄﾁﾘﾇﾙｦﾜｶﾖﾀﾚｿﾂﾈﾅﾗﾑｳヰﾉｵｸﾔﾏｹﾌｺｴﾃｱｻｷﾕﾒﾐｼヱﾋﾓｾｽﾝ";
pub(crate) const GANADA: &str = "가나다라마바사아자차카타파하";
pub(crate) const CHOSUNG: &str = "ㄱㄴㄷㄹㅁㅂㅅㅇㅈㅊㅋㅌㅍㅎ";

/// The `n`th (1-based) character of a sequence, cycling past its end; 0 is `0`.
pub(crate) fn cycle(set: &str, n: u32) -> String {
    if n == 0 {
        return "0".into();
    }
    let len = set.chars().count().max(1);
    set.chars().nth((n as usize - 1) % len).map(|c| c.to_string()).unwrap_or_default()
}

/// Half-width katakana in gojūon order: ｱ, ｲ, ｳ … ﾝ, then ｱ again.
pub(crate) fn aiueo(n: u32) -> String {
    cycle(AIUEO_HALF, n)
}

/// Full-width katakana in gojūon order: ア, イ, ウ … ン, then ア again.
pub(crate) fn aiueo_full_width(n: u32) -> String {
    cycle(AIUEO, n)
}

/// Half-width katakana in iroha order: ｲ, ﾛ, ﾊ … ｽ, ﾝ, then ｲ again.
pub(crate) fn iroha(n: u32) -> String {
    cycle(IROHA_HALF, n)
}

/// Full-width katakana in iroha order: イ, ロ, ハ … ス, ン, then イ again.
pub(crate) fn iroha_full_width(n: u32) -> String {
    cycle(IROHA, n)
}

/// Hangul syllables: 가, 나, 다 … 하, then 가 again.
pub(crate) fn ganada(n: u32) -> String {
    cycle(GANADA, n)
}

/// Hangul initial consonants: ㄱ, ㄴ, ㄷ … ㅎ, then ㄱ again.
pub(crate) fn chosung(n: u32) -> String {
    cycle(CHOSUNG, n)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chinese_counting_systems() {
        let c = |n| chinese_counting(n);
        assert_eq!([c(0), c(1), c(2), c(3), c(10), c(11), c(20), c(21), c(99)], ["○", "一", "二", "三", "十", "十一", "二十", "二十一", "九十九"]);
        assert_eq!([c(100), c(101), c(1000), c(1_000_000_000)], ["一○○", "一○一", "一○○○", "一○○○○○○○○○"]);
        let t = |n| chinese_counting_thousand(n);
        assert_eq!(
            [t(0), t(10), t(11), t(20), t(101), t(110), t(111), t(1000), t(1010)],
            ["〇", "十", "十一", "二十", "一百〇一", "一百一十", "一百一十一", "一千", "一千〇一十"]
        );
        assert_eq!(
            [t(10_000), t(10_001), t(100_000), t(100_001), t(100_100), t(110_000), t(999_999), t(1_000_000), t(u32::MAX)],
            ["一万", "一万〇一", "一十万", "一十万〇一", "一十万〇一百", "一十一万", "九十九万九千九百九十九", "", ""]
        );
        let l = |n| chinese_legal_simplified(n);
        assert_eq!(
            [l(0), l(1), l(3), l(10), l(11), l(101), l(1000), l(10_000), l(100_000), l(1_000_000)],
            ["零", "壹", "叁", "壹拾", "壹拾壹", "壹佰零壹", "壹仟", "壹萬", "壹拾萬", ""]
        );
        assert_eq!(
            [
                taiwanese_counting_thousand(0),
                taiwanese_counting_thousand(10_001),
                taiwanese_counting_thousand(100_000),
                ideograph_legal_traditional(20)
            ],
            ["零", "一萬零一", "一十萬", "貳拾"]
        );
        assert_eq!([ideograph_legal_traditional(999_999), ideograph_legal_traditional(1_000_000)], ["玖拾玖萬玖仟玖佰玖拾玖", ""]);
        assert_eq!([ideograph_digital(10), ideograph_digital(101), taiwanese_digital(10), taiwanese_counting(0)], ["一〇", "一〇一", "一○", "○"]);
    }

    #[test]
    fn japanese_and_korean() {
        let j = |n| japanese_counting(n);
        assert_eq!(
            [j(0), j(10), j(11), j(20), j(101), j(1000), j(1100), j(10_000), j(10_001), j(10_100), j(11_000)],
            ["〇", "十", "十一", "二十", "百一", "千", "千百", "一万", "一万一", "一万百", "一万一千"]
        );
        assert_eq!([j(100_000), j(110_000), j(111_000), j(999_999), j(1_000_000)], ["十万", "十一万", "十一万一千", "九十九万九千九百九十九", ""]);
        assert_eq!(
            [japanese_legal(10), japanese_legal(1000), japanese_legal(100_000), japanese_legal(1_000_000_000)],
            ["壱拾", "壱阡", "壱拾萬", "壱拾億"]
        );
        let d = |n| japanese_digital_ten_thousand(n);
        assert_eq!([d(0), d(12), d(9999), d(10_000), d(12_345)], ["〇", "一二", "九九九九", "", ""]);
        let k = |n| korean_counting(n);
        assert_eq!(
            [k(0), k(11), k(20), k(101), k(1000), k(10_000), k(11_000), k(100_000), k(110_000), k(111_000), k(120_000), k(1_000_000)],
            ["영", "십일", "이십", "백일", "천", "만", "만일천", "십만", "십만", "십만일천", "십이만", ""]
        );
        assert_eq!(
            [korean_digital(10), korean_digital2(0), korean_digital2(10), korean_digital2(999_999), korean_digital2(1_000_000)],
            ["일영", "\u{F9B2}", "一零", "九九九九九九", ""]
        );
        let l = |n| korean_legal(n);
        assert_eq!(
            [l(0), l(1), l(11), l(20), l(70), l(99), l(100), l(101), l(10_000), l(1_000_000), l(9_999_999), l(10_000_000)],
            ["0", "하나", "열하나", "스물", "일흔", "아흔아홉", "백", "백일", "만", "백만", "구백구십구만구천구백구십구", ""]
        );
    }

    #[test]
    fn sequences_and_enclosed_numbers() {
        assert_eq!([ideograph_traditional(1), ideograph_traditional(10), ideograph_traditional(11)], ["甲", "癸", "11"]);
        assert_eq!([ideograph_zodiac(1), ideograph_zodiac(11), ideograph_zodiac(12), ideograph_zodiac(13)], ["子", "戍", "亥", "13"]);
        let z = |n| ideograph_zodiac_traditional(n);
        assert_eq!([z(0), z(1), z(11), z(60), z(61), z(99), z(32_767)], ["0", "甲子", "甲戍", "癸亥", "甲子", "壬寅", "庚午"]);
        assert_eq!(
            [decimal_enclosed_circle(0), decimal_enclosed_circle(1), decimal_enclosed_circle(20), decimal_enclosed_circle(21)],
            ["0", "①", "⑳", "21"]
        );
        assert_eq!([decimal_enclosed_circle_chinese(10), decimal_enclosed_circle_chinese(11)], ["⑩", "11"]);
        assert_eq!(
            [
                decimal_enclosed_fullstop(1),
                decimal_enclosed_paren(20),
                ideograph_enclosed_circle(1),
                ideograph_enclosed_circle(10),
                ideograph_enclosed_circle(11)
            ],
            ["⒈", "⒇", "㈠", "㈩", "11"]
        );
        assert_eq!([decimal_full_width(10), decimal_full_width(-1)], ["１０", "-１"]);
        assert_eq!(
            [aiueo_full_width(0), aiueo_full_width(1), aiueo_full_width(46), aiueo_full_width(47), aiueo(2), aiueo(60)],
            ["0", "ア", "ン", "ア", "ｲ", "ｾ"]
        );
        assert_eq!(
            [iroha_full_width(1), iroha_full_width(47), iroha_full_width(48), iroha_full_width(49), iroha(3), iroha(60)],
            ["イ", "ス", "ン", "イ", "ﾊ", "ｦ"]
        );
        assert_eq!([ganada(1), ganada(14), ganada(15), chosung(14), chosung(29), ganada(0)], ["가", "하", "가", "ㅎ", "ㄱ", "0"]);
        assert_eq!([AIUEO.chars().count(), AIUEO_HALF.chars().count(), IROHA.chars().count(), IROHA_HALF.chars().count()], [46, 46, 48, 48]);
    }
}

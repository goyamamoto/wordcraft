//! Word's Japanese find equivalences (observed in Word for Mac on 2026-10-18 through `find
//! object` with `match byte` and `match fuzzy`), as the classes of characters a Find what
//! character stands for:
//!
//! - **Half/full-width forms** (`match byte` off, Word's default): full-width ASCII is ASCII
//!   (ＡＢＣ = abc, １ = 1, （ = (, ～ = ~), half-width katakana is katakana with ﾞ/ﾟ composed
//!   (ﾜｰﾌﾟﾛ = ワープロ, ｶﾞ = ガ), ｰ = ー, ･ = ・, 　 = space, ‘ ’ = ', “ ” = ", ￥ = \ (¥ stays
//!   itself), and ウ゛ = ヴ. 。 is not `.`, 、 is not `,`.
//! - **あいまい検索** (`match fuzzy`, on by default with Japanese editing; off when `match
//!   byte` is on): hiragana = katakana (か = カ = ｶ), small kana = large (っ = つ, ゃ = や,
//!   ぁ = あ, ヵ = か, ヶ = け), old kana (ゐ = い, ゑ = え, ヰ = イ, ヱ = エ, ヲ = を), the
//!   repeat marks (々 = ゝ = ゞ = ヽ = ヾ), the dashes ー = － = ｰ = ― = ‐ = - (not – — − ‒ or
//!   〜), ヴ = ブ and ヴァ ヴィ ヴェ ヴォ = バ ビ ベ ボ. Voicing is kept: は ≠ ば ≠ ぱ; Latin
//!   letters with diacritics stay themselves.

use std::sync::OnceLock;

/// The dashes あいまい検索 treats alike (ー is the key).
const DASHES: &[char] = &['\u{30FC}', '\u{FF0D}', '\u{FF70}', '\u{2015}', '\u{2010}', '-'];
/// The repeat marks (ゝ is the key).
const REPEATS: &[char] = &['\u{3005}', '\u{309D}', '\u{309E}', '\u{30FD}', '\u{30FE}'];

/// Half-width katakana (U+FF61…U+FF9F) as full-width characters, in code-point order.
const FULL_OF_HALF: &str =
    "。「」、・ヲァィゥェォャュョッーアイウエオカキクケコサシスセソタチツテトナニヌネノハヒフヘホマミムメモヤユヨラリルレロワン゛゜";

const VOICED: &[(char, char)] = &[
    ('カ', 'ガ'),
    ('キ', 'ギ'),
    ('ク', 'グ'),
    ('ケ', 'ゲ'),
    ('コ', 'ゴ'),
    ('サ', 'ザ'),
    ('シ', 'ジ'),
    ('ス', 'ズ'),
    ('セ', 'ゼ'),
    ('ソ', 'ゾ'),
    ('タ', 'ダ'),
    ('チ', 'ヂ'),
    ('ツ', 'ヅ'),
    ('テ', 'デ'),
    ('ト', 'ド'),
    ('ハ', 'バ'),
    ('ヒ', 'ビ'),
    ('フ', 'ブ'),
    ('ヘ', 'ベ'),
    ('ホ', 'ボ'),
    ('ウ', 'ヴ'),
    ('ワ', 'ヷ'),
    ('ヲ', 'ヺ'),
];
const SEMI_VOICED: &[(char, char)] = &[('ハ', 'パ'), ('ヒ', 'ピ'), ('フ', 'プ'), ('ヘ', 'ペ'), ('ホ', 'ポ')];

fn is_halfwidth_kana(c: char) -> bool {
    ('\u{FF61}'..='\u{FF9F}').contains(&c)
}

/// The full-width form of a half-width katakana or punctuation mark.
fn full_of_half(c: char) -> Option<char> {
    FULL_OF_HALF.chars().nth((c as u32).checked_sub(0xFF61)? as usize)
}

/// The voiced (ﾞ) or semi-voiced (ﾟ) form of a katakana.
fn voiced(base: char, semi: bool) -> Option<char> {
    let table = if semi { SEMI_VOICED } else { VOICED };
    table.iter().find(|(b, _)| *b == base).map(|(_, v)| *v)
}

/// Katakana (U+30A1…U+30F6) to hiragana; other characters as they are.
fn hiragana_of(c: char) -> char {
    match c {
        '\u{30A1}'..='\u{30F6}' => char::from_u32(c as u32 - 0x60).unwrap_or(c),
        _ => c,
    }
}

/// Small kana to its large form (hiragana or katakana); other characters as they are.
fn large_of(c: char) -> char {
    match c {
        'ぁ' | 'ぃ' | 'ぅ' | 'ぇ' | 'ぉ' | 'ゃ' | 'ゅ' | 'ょ' | 'っ' | 'ゎ' | 'ァ' | 'ィ' | 'ゥ' | 'ェ' | 'ォ' | 'ャ' | 'ュ' | 'ョ' | 'ッ' | 'ヮ' => {
            char::from_u32(c as u32 + 1).unwrap_or(c)
        }
        'ヵ' => 'カ',
        'ヶ' => 'ケ',
        'ゕ' => 'か',
        'ゖ' => 'け',
        _ => c,
    }
}

/// The character every member of a half/full-width class maps to (`match byte` off).
fn width_key(c: char) -> char {
    match c {
        '\u{FF01}'..='\u{FF5E}' => char::from_u32(c as u32 - 0xFEE0).unwrap_or(c),
        '\u{FFE5}' => '\\',
        '\u{3000}' => ' ',
        '\u{2018}' | '\u{2019}' => '\'',
        '\u{201C}' | '\u{201D}' => '"',
        c if is_halfwidth_kana(c) => full_of_half(c).unwrap_or(c),
        _ => c,
    }
}

/// The character every member of an あいまい検索 class maps to (`match fuzzy` on): the width
/// key, then hiragana, large, new kana, with the dash and repeat-mark classes on one member each.
fn fuzzy_key(c: char) -> char {
    let c = width_key(c);
    if DASHES.contains(&c) {
        return '\u{30FC}';
    }
    if REPEATS.contains(&c) {
        return '\u{309D}';
    }
    let c = hiragana_of(large_of(c));
    match c {
        'ゐ' => 'い',
        'ゑ' => 'え',
        'ゔ' => 'ぶ',
        _ => c,
    }
}

/// A member of a class: one character, or a sequence that stands for one (ｶﾞ for ガ, ウ゛ for
/// ヴ, ヴァ for バ).
#[derive(Clone, Debug, PartialEq, Eq)]
struct Member {
    text: String,
    key: char,
    fuzzy_key: char,
}

/// Every character and sequence the folding knows, with its keys.
fn members() -> &'static [Member] {
    static MEMBERS: OnceLock<Vec<Member>> = OnceLock::new();
    MEMBERS.get_or_init(|| {
        let mut out: Vec<Member> = Vec::new();
        let mut push = |text: String, base: char| {
            out.push(Member { text, key: width_key(base), fuzzy_key: fuzzy_key(base) });
        };
        // Single characters.
        let ranges: [(u32, u32); 9] = [
            (0x20, 0x7E),
            (0xFF01, 0xFF5E),
            (0xFFE5, 0xFFE5),
            (0x3000, 0x3000),
            (0x3001, 0x3007),
            (0x3041, 0x30FF),
            (0xFF61, 0xFF9F),
            (0x2010, 0x2019),
            (0x201C, 0x201D),
        ];
        for (a, b) in ranges {
            for u in a..=b {
                if let Some(c) = char::from_u32(u) {
                    push(c.to_string(), c);
                }
            }
        }
        // Half-width kana with a separate sound mark: ｶﾞ is ガ, ﾊﾟ is パ.
        for u in 0xFF66..=0xFF9D {
            let Some(h) = char::from_u32(u) else { continue };
            let Some(base) = full_of_half(h) else { continue };
            for (mark, semi) in [('\u{FF9E}', false), ('\u{FF9F}', true)] {
                if let Some(v) = voiced(base, semi) {
                    push(format!("{h}{mark}"), v);
                }
            }
        }
        // A kana with a spacing sound mark: ウ゛ is ヴ, は゛ is ば.
        for (base, v) in VOICED.iter().chain(SEMI_VOICED) {
            let semi = SEMI_VOICED.iter().any(|(b, s)| b == base && s == v);
            let mark = if semi { '\u{309C}' } else { '\u{309B}' };
            for b in [*base, hiragana_of(*base)] {
                let v = if b == *base { *v } else { hiragana_of(*v) };
                push(format!("{b}{mark}"), v);
            }
        }
        // ヴ with a small vowel is the バ row (あいまい検索 only): the key of the sequence is
        // バ's fuzzy key; its width key is unique so it never folds without fuzzy.
        for (small, ba) in [('ァ', 'バ'), ('ィ', 'ビ'), ('ェ', 'ベ'), ('ォ', 'ボ')] {
            for vu in ['ヴ', 'ゔ'] {
                for s in [small, hiragana_of(small)] {
                    out.push(Member { text: format!("{vu}{s}"), key: '\u{0}', fuzzy_key: fuzzy_key(ba) });
                }
            }
        }
        out
    })
}

/// The Find what text with the sequences that stand for one character composed (ｶﾞ → ガ, ウ゛
/// → ヴ; with `fuzzy`, ヴァ → バ), so each remaining character can be expanded to its class.
pub fn normalize(query: &str, fuzzy: bool) -> Vec<char> {
    let chars: Vec<char> = query.chars().collect();
    let mut out = Vec::with_capacity(chars.len());
    let mut i = 0;
    while let Some(&c) = chars.get(i) {
        if let Some(&n) = chars.get(i + 1) {
            let pair = format!("{c}{n}");
            let m = members().iter().find(|m| m.text == pair && (fuzzy || m.key != '\u{0}'));
            if let Some(m) = m {
                // The composed character: the member whose text is one char with the same keys.
                let one = members().iter().find(|x| x.text.chars().count() == 1 && x.fuzzy_key == m.fuzzy_key && (fuzzy || x.key == m.key));
                if let Some(one) = one {
                    out.extend(one.text.chars());
                    i += 2;
                    continue;
                }
            }
        }
        out.push(c);
        i += 1;
    }
    out
}

/// The texts a Find what character matches: itself and every member of its class, longest
/// first (so ｶﾞ is taken whole before ｶ). `match_byte` keeps every character to itself (and
/// turns あいまい検索 off, as Word greys it out); otherwise `fuzzy` adds the あいまい検索 classes.
pub fn class(c: char, match_byte: bool, fuzzy: bool) -> Vec<String> {
    let mut out = vec![c.to_string()];
    if match_byte {
        return out;
    }
    let (key, fkey) = (width_key(c), fuzzy_key(c));
    for m in members() {
        let same = if fuzzy { m.fuzzy_key == fkey } else { m.key == key && m.key != '\u{0}' };
        if same && !out.contains(&m.text) {
            out.push(m.text.clone());
        }
    }
    out.sort_by(|a, b| b.chars().count().cmp(&a.chars().count()).then_with(|| a.cmp(b)));
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn has(c: char, byte: bool, fuzzy: bool, t: &str) -> bool {
        class(c, byte, fuzzy).iter().any(|x| x == t)
    }

    /// Word's half/full-width folding (`match byte` off, no あいまい検索).
    #[test]
    fn width_classes_match_word() {
        assert!(has('a', false, false, "ａ"));
        assert!(has('1', false, false, "１"));
        assert!(has('(', false, false, "（"));
        assert!(has('~', false, false, "～"));
        assert!(!has('~', false, false, "〜"));
        assert!(has('\\', false, false, "￥"));
        assert!(!has('¥', false, false, "\\"));
        assert!(has('ー', false, false, "ｰ"));
        assert!(has('・', false, false, "･"));
        assert!(has(' ', false, false, "　"));
        assert!(has('\'', false, false, "’"));
        assert!(has('"', false, false, "“"));
        assert!(has('ガ', false, false, "ｶﾞ"));
        assert!(has('ガ', false, false, "カ゛"));
        assert!(has('ヴ', false, false, "ウ゛"));
        assert!(has('ア', false, false, "ｱ"));
        assert!(!has('ア', false, false, "あ"));
        assert!(!has('.', false, false, "。"));
        assert!(!has(',', false, false, "、"));
        assert!(!has('ツ', false, false, "ッ"));
        assert!(!has('バ', false, false, "ヴァ"));
        // Match byte: every character to itself.
        assert_eq!(class('a', true, true), vec!["a".to_string()]);
        assert_eq!(class('ガ', true, true), vec!["ガ".to_string()]);
    }

    /// あいまい検索's classes, as Word matched them.
    #[test]
    fn fuzzy_classes_match_word() {
        // (the query's character, texts it matches, whether it does).
        let cases: &[(char, &[&str], bool)] = &[
            ('っ', &["ツ", "ッ", "ｯ", "っ", "つ"], true),
            ('は', &["は", "ハ", "ﾊ"], true),
            ('は', &["ば", "ぱ"], false),
            ('ば', &["ば", "バ", "ﾊﾞ", "ヴァ", "ヴぁ"], true),
            ('ブ', &["ヴ"], true),
            ('ヴ', &["ブ"], true),
            ('ゃ', &["ヤ", "ャ", "や", "ゃ"], true),
            ('ぁ', &["あ", "ぁ", "ア", "ァ", "ｱ"], true),
            ('ヱ', &["ヱ", "ゑ", "え", "エ"], true),
            ('い', &["ゐ", "ヰ", "い", "イ"], true),
            ('ｦ', &["ヲ", "を", "ｦ"], true),
            ('ヵ', &["か", "ｶ", "ヵ", "カ"], true),
            ('け', &["ヶ", "け", "ケ"], true),
            ('々', &["々", "ゝ", "ゞ", "ヽ", "ヾ"], true),
            ('-', &["ー", "－", "ｰ", "―", "‐", "-"], true),
            ('-', &["–", "−", "—", "‒", "〜"], false),
        ];
        for &(c, texts, matches) in cases {
            for t in texts {
                assert_eq!(has(c, false, true, t), matches, "{c} {t}");
            }
        }
        assert_eq!(class('〜', false, true), vec!["〜".to_string()]);
        assert_eq!(class('é', false, true), vec!["é".to_string()]);
        assert_eq!(class('ß', false, true), vec!["ß".to_string()]);
        // Longest members first.
        assert_eq!(class('ガ', false, false).first().map(|s| s.chars().count()), Some(2));
    }

    #[test]
    fn queries_compose_their_sequences() {
        assert_eq!(normalize("ｶﾞ", false), vec!['ガ']);
        assert_eq!(normalize("ウ゛", false), vec!['ヴ']);
        assert_eq!(normalize("ヴァ", false), vec!['ヴ', 'ァ']);
        let ba = normalize("ヴァ", true);
        assert_eq!(ba.len(), 1);
        assert!(has(ba[0], false, true, "バ"));
        assert_eq!(normalize("ヴぁ", true), ba);
        assert_eq!(normalize("ﾜｰﾌﾟﾛ", false), vec!['ﾜ', 'ｰ', 'プ', 'ﾛ']);
        assert_eq!(normalize("", true), Vec::<char>::new());
        assert_eq!(normalize("ﾞﾞﾞ", true), vec!['ﾞ', 'ﾞ', 'ﾞ']);
    }
}

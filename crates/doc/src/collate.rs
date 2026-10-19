//! Language-aware sorting of text: a small, pure-Rust collation.
//!
//! Every letter gets a *primary* weight (its place in the alphabet) and a *secondary* one (its
//! diacritic). Strings compare by primary weights first, then by secondary ones, so `é` sorts
//! with `e` (`resume` < `résumé` < `rex`) unless the language makes it a letter of its own:
//! Polish puts `ą` right after `a` and `ż` after `ź` after `z`, Swedish puts `å ä ö` after `z`,
//! Czech puts `ch` after `h`, Spanish `ñ` after `n`, Hungarian `cs`, `gy`, `sz`… after their
//! first letter. The per-language tables follow the published alphabets of those languages.
//! Text without diacritics sorts exactly as a case-insensitive code-point compare.
//!
//! Japanese text (or any text in a Japanese document) sorts as Word for Mac sorts it, observed
//! on a 369-string table (`so1`/`so2` probes, 2026-10-19): symbols in Word's order, then digits,
//! then Latin letters, then kana in 五十音 order, then kanji and everything else by code point —
//! a half-width form, a full-width form, a katakana, a hiragana, a small kana and a large one all
//! being the same letter; voicing (は < ば < ぱ) and Latin diacritics (e < é < è) decide only
//! between strings whose letters are the same, and strings alike in all that keep their order
//! (Word's sort is stable: `ab`, `ａｂ`, `AB` stay as they were). The prolonged sound mark ー is
//! a symbol, as are the spacing ゛ ゜ and the iteration marks; 〆 reads しめ, 〇 is a zero.

use std::cmp::Ordering;

/// The diacritic of a precomposed lowercase Latin letter: `(base letters, mark)`. Marks: 1 acute,
/// 2 grave, 3 circumflex, 4 diaeresis, 5 tilde, 6 ring, 7 caron, 8 cedilla, 9 ogonek, 10 dot,
/// 11 stroke, 12 breve, 13 macron, 14 double acute, 15 comma below, 16 ligature, 17 dotless,
/// 18 eth.
const DECOMP: &[(char, &str, u16)] = &[
    ('à', "a", 2),
    ('á', "a", 1),
    ('â', "a", 3),
    ('ã', "a", 5),
    ('ä', "a", 4),
    ('å', "a", 6),
    ('ā', "a", 13),
    ('ă', "a", 12),
    ('ą', "a", 9),
    ('æ', "ae", 16),
    ('ç', "c", 8),
    ('ć', "c", 1),
    ('ĉ', "c", 3),
    ('ċ', "c", 10),
    ('č', "c", 7),
    ('ď', "d", 7),
    ('đ', "d", 11),
    ('ð', "d", 18),
    ('è', "e", 2),
    ('é', "e", 1),
    ('ê', "e", 3),
    ('ë', "e", 4),
    ('ē', "e", 13),
    ('ĕ', "e", 12),
    ('ė', "e", 10),
    ('ę', "e", 9),
    ('ě', "e", 7),
    ('ĝ', "g", 3),
    ('ğ', "g", 12),
    ('ġ', "g", 10),
    ('ģ', "g", 8),
    ('ĥ', "h", 3),
    ('ħ', "h", 11),
    ('ì', "i", 2),
    ('í', "i", 1),
    ('î', "i", 3),
    ('ï', "i", 4),
    ('ĩ', "i", 5),
    ('ī', "i", 13),
    ('ĭ', "i", 12),
    ('į', "i", 9),
    ('ı', "i", 17),
    ('ĳ', "ij", 16),
    ('ĵ', "j", 3),
    ('ķ', "k", 8),
    ('ĺ', "l", 1),
    ('ļ', "l", 8),
    ('ľ', "l", 7),
    ('ŀ', "l", 10),
    ('ł', "l", 11),
    ('ñ', "n", 5),
    ('ń', "n", 1),
    ('ņ', "n", 8),
    ('ň', "n", 7),
    ('ò', "o", 2),
    ('ó', "o", 1),
    ('ô', "o", 3),
    ('õ', "o", 5),
    ('ö', "o", 4),
    ('ø', "o", 11),
    ('ō', "o", 13),
    ('ŏ', "o", 12),
    ('ő', "o", 14),
    ('œ', "oe", 16),
    ('ŕ', "r", 1),
    ('ŗ', "r", 8),
    ('ř', "r", 7),
    ('ś', "s", 1),
    ('ŝ', "s", 3),
    ('ş', "s", 8),
    ('š', "s", 7),
    ('ș', "s", 15),
    ('ß', "ss", 16),
    ('ţ', "t", 8),
    ('ť', "t", 7),
    ('ŧ', "t", 11),
    ('ț', "t", 15),
    ('ù', "u", 2),
    ('ú', "u", 1),
    ('û', "u", 3),
    ('ü', "u", 4),
    ('ũ', "u", 5),
    ('ū', "u", 13),
    ('ŭ', "u", 12),
    ('ů', "u", 6),
    ('ű', "u", 14),
    ('ų', "u", 9),
    ('ŵ', "w", 3),
    ('ý', "y", 1),
    ('ÿ', "y", 4),
    ('ŷ', "y", 3),
    ('ź', "z", 1),
    ('ż', "z", 10),
    ('ž', "z", 7),
];

/// A letter (or letter group) a language sorts as a letter of its own: right after `after`
/// (`rank` places after it), with secondary weight `sec` among letters of the same place.
struct Tailor {
    seq: &'static str,
    after: char,
    rank: u32,
    sec: u16,
}

const fn t(seq: &'static str, after: char, rank: u32) -> Tailor {
    Tailor { seq, after, rank, sec: 0 }
}

const fn ts(seq: &'static str, after: char, rank: u32, sec: u16) -> Tailor {
    Tailor { seq, after, rank, sec }
}

const PL: &[Tailor] =
    &[t("ą", 'a', 1), t("ć", 'c', 1), t("ę", 'e', 1), t("ł", 'l', 1), t("ń", 'n', 1), t("ó", 'o', 1), t("ś", 's', 1), t("ź", 'z', 1), t("ż", 'z', 2)];
const CS: &[Tailor] = &[t("č", 'c', 1), t("ch", 'h', 1), t("ř", 'r', 1), t("š", 's', 1), t("ž", 'z', 1)];
const SK: &[Tailor] = &[t("ä", 'a', 1), t("č", 'c', 1), t("ch", 'h', 1), t("ô", 'o', 1), t("š", 's', 1), t("ž", 'z', 1)];
const HU: &[Tailor] = &[
    t("cs", 'c', 1),
    t("dz", 'd', 1),
    t("dzs", 'd', 2),
    t("gy", 'g', 1),
    t("ly", 'l', 1),
    t("ny", 'n', 1),
    t("ö", 'o', 1),
    ts("ő", 'o', 1, 14),
    t("sz", 's', 1),
    t("ty", 't', 1),
    t("ü", 'u', 1),
    ts("ű", 'u', 1, 14),
    t("zs", 'z', 1),
];
const SV: &[Tailor] = &[t("å", 'z', 1), t("ä", 'z', 2), ts("æ", 'z', 2, 16), t("ö", 'z', 3), ts("ø", 'z', 3, 11)];
const DA: &[Tailor] = &[t("æ", 'z', 1), ts("ä", 'z', 1, 4), t("ø", 'z', 2), ts("ö", 'z', 2, 4), t("å", 'z', 3)];
const ES: &[Tailor] = &[t("ñ", 'n', 1)];
const TR: &[Tailor] = &[t("ç", 'c', 1), t("ğ", 'g', 1), t("ı", 'h', 1), t("ö", 'o', 1), t("ş", 's', 1), t("ü", 'u', 1)];
const HR: &[Tailor] =
    &[t("č", 'c', 1), t("ć", 'c', 2), t("dž", 'd', 1), t("đ", 'd', 2), t("lj", 'l', 1), t("nj", 'n', 1), t("š", 's', 1), t("ž", 'z', 1)];
const RO: &[Tailor] = &[t("ă", 'a', 1), t("â", 'a', 2), t("î", 'i', 1), t("ș", 's', 1), ts("ş", 's', 1, 8), t("ț", 't', 1), ts("ţ", 't', 1, 8)];
const ET: &[Tailor] = &[t("š", 's', 1), t("z", 's', 2), t("ž", 's', 3), t("õ", 'w', 1), t("ä", 'w', 2), t("ö", 'w', 3), t("ü", 'w', 4)];
const LT: &[Tailor] = &[t("č", 'c', 1), t("š", 's', 1), t("ž", 'z', 1)];
const LV: &[Tailor] = &[t("č", 'c', 1), t("ģ", 'g', 1), t("ķ", 'k', 1), t("ļ", 'l', 1), t("ņ", 'n', 1), t("š", 's', 1), t("ž", 'z', 1)];
const IS: &[Tailor] = &[
    t("á", 'a', 1),
    t("ð", 'd', 1),
    t("é", 'e', 1),
    t("í", 'i', 1),
    t("ó", 'o', 1),
    t("ú", 'u', 1),
    t("ý", 'y', 1),
    t("þ", 'z', 1),
    t("æ", 'z', 2),
    t("ö", 'z', 3),
];

/// The primary language subtag of a BCP 47 tag (`pl-PL` → `pl`), lowercased.
pub fn language(lang: Option<&str>) -> String {
    lang.and_then(|l| l.split(['-', '_']).next()).unwrap_or("").trim().to_ascii_lowercase()
}

fn tailoring(lang: Option<&str>) -> &'static [Tailor] {
    match language(lang).as_str() {
        "pl" => PL,
        "cs" => CS,
        "sk" => SK,
        "hu" => HU,
        "sv" | "fi" => SV,
        "da" | "nb" | "nn" | "no" => DA,
        "es" | "gl" => ES,
        "tr" | "az" => TR,
        "hr" | "bs" | "sr" => HR,
        "ro" => RO,
        "et" => ET,
        "lt" => LT,
        "lv" => LV,
        "is" => IS,
        "sl" => LT,
        _ => &[],
    }
}

/// A lowercase Latin letter followed by a combining mark, and the single letter Unicode
/// composes them to (NFC; generated from the Unicode Character Database). Text pasted from
/// decomposed sources (macOS file names: `z` + U+0307) collates like the composed letter.
const COMPOSE: &[(char, char, char)] = &[
    ('a', '\u{300}', 'à'),
    ('a', '\u{301}', 'á'),
    ('a', '\u{302}', 'â'),
    ('a', '\u{303}', 'ã'),
    ('a', '\u{304}', 'ā'),
    ('a', '\u{306}', 'ă'),
    ('a', '\u{307}', 'ȧ'),
    ('a', '\u{308}', 'ä'),
    ('a', '\u{30a}', 'å'),
    ('a', '\u{30c}', 'ǎ'),
    ('a', '\u{328}', 'ą'),
    ('b', '\u{307}', 'ḃ'),
    ('c', '\u{301}', 'ć'),
    ('c', '\u{302}', 'ĉ'),
    ('c', '\u{307}', 'ċ'),
    ('c', '\u{30c}', 'č'),
    ('c', '\u{327}', 'ç'),
    ('d', '\u{307}', 'ḋ'),
    ('d', '\u{30c}', 'ď'),
    ('d', '\u{327}', 'ḑ'),
    ('e', '\u{300}', 'è'),
    ('e', '\u{301}', 'é'),
    ('e', '\u{302}', 'ê'),
    ('e', '\u{303}', 'ẽ'),
    ('e', '\u{304}', 'ē'),
    ('e', '\u{306}', 'ĕ'),
    ('e', '\u{307}', 'ė'),
    ('e', '\u{308}', 'ë'),
    ('e', '\u{30c}', 'ě'),
    ('e', '\u{327}', 'ȩ'),
    ('e', '\u{328}', 'ę'),
    ('f', '\u{307}', 'ḟ'),
    ('g', '\u{301}', 'ǵ'),
    ('g', '\u{302}', 'ĝ'),
    ('g', '\u{304}', 'ḡ'),
    ('g', '\u{306}', 'ğ'),
    ('g', '\u{307}', 'ġ'),
    ('g', '\u{30c}', 'ǧ'),
    ('g', '\u{327}', 'ģ'),
    ('h', '\u{302}', 'ĥ'),
    ('h', '\u{307}', 'ḣ'),
    ('h', '\u{308}', 'ḧ'),
    ('h', '\u{30c}', 'ȟ'),
    ('h', '\u{327}', 'ḩ'),
    ('i', '\u{300}', 'ì'),
    ('i', '\u{301}', 'í'),
    ('i', '\u{302}', 'î'),
    ('i', '\u{303}', 'ĩ'),
    ('i', '\u{304}', 'ī'),
    ('i', '\u{306}', 'ĭ'),
    ('i', '\u{308}', 'ï'),
    ('i', '\u{30c}', 'ǐ'),
    ('i', '\u{328}', 'į'),
    ('j', '\u{302}', 'ĵ'),
    ('j', '\u{30c}', 'ǰ'),
    ('k', '\u{301}', 'ḱ'),
    ('k', '\u{30c}', 'ǩ'),
    ('k', '\u{327}', 'ķ'),
    ('l', '\u{301}', 'ĺ'),
    ('l', '\u{30c}', 'ľ'),
    ('l', '\u{327}', 'ļ'),
    ('m', '\u{301}', 'ḿ'),
    ('m', '\u{307}', 'ṁ'),
    ('n', '\u{300}', 'ǹ'),
    ('n', '\u{301}', 'ń'),
    ('n', '\u{303}', 'ñ'),
    ('n', '\u{307}', 'ṅ'),
    ('n', '\u{30c}', 'ň'),
    ('n', '\u{327}', 'ņ'),
    ('o', '\u{300}', 'ò'),
    ('o', '\u{301}', 'ó'),
    ('o', '\u{302}', 'ô'),
    ('o', '\u{303}', 'õ'),
    ('o', '\u{304}', 'ō'),
    ('o', '\u{306}', 'ŏ'),
    ('o', '\u{307}', 'ȯ'),
    ('o', '\u{308}', 'ö'),
    ('o', '\u{30b}', 'ő'),
    ('o', '\u{30c}', 'ǒ'),
    ('o', '\u{328}', 'ǫ'),
    ('p', '\u{301}', 'ṕ'),
    ('p', '\u{307}', 'ṗ'),
    ('r', '\u{301}', 'ŕ'),
    ('r', '\u{307}', 'ṙ'),
    ('r', '\u{30c}', 'ř'),
    ('r', '\u{327}', 'ŗ'),
    ('s', '\u{301}', 'ś'),
    ('s', '\u{302}', 'ŝ'),
    ('s', '\u{307}', 'ṡ'),
    ('s', '\u{30c}', 'š'),
    ('s', '\u{326}', 'ș'),
    ('s', '\u{327}', 'ş'),
    ('t', '\u{307}', 'ṫ'),
    ('t', '\u{308}', 'ẗ'),
    ('t', '\u{30c}', 'ť'),
    ('t', '\u{326}', 'ț'),
    ('t', '\u{327}', 'ţ'),
    ('u', '\u{300}', 'ù'),
    ('u', '\u{301}', 'ú'),
    ('u', '\u{302}', 'û'),
    ('u', '\u{303}', 'ũ'),
    ('u', '\u{304}', 'ū'),
    ('u', '\u{306}', 'ŭ'),
    ('u', '\u{308}', 'ü'),
    ('u', '\u{30a}', 'ů'),
    ('u', '\u{30b}', 'ű'),
    ('u', '\u{30c}', 'ǔ'),
    ('u', '\u{328}', 'ų'),
    ('v', '\u{303}', 'ṽ'),
    ('w', '\u{300}', 'ẁ'),
    ('w', '\u{301}', 'ẃ'),
    ('w', '\u{302}', 'ŵ'),
    ('w', '\u{307}', 'ẇ'),
    ('w', '\u{308}', 'ẅ'),
    ('w', '\u{30a}', 'ẘ'),
    ('x', '\u{307}', 'ẋ'),
    ('x', '\u{308}', 'ẍ'),
    ('y', '\u{300}', 'ỳ'),
    ('y', '\u{301}', 'ý'),
    ('y', '\u{302}', 'ŷ'),
    ('y', '\u{303}', 'ỹ'),
    ('y', '\u{304}', 'ȳ'),
    ('y', '\u{307}', 'ẏ'),
    ('y', '\u{308}', 'ÿ'),
    ('y', '\u{30a}', 'ẙ'),
    ('z', '\u{301}', 'ź'),
    ('z', '\u{302}', 'ẑ'),
    ('z', '\u{307}', 'ż'),
    ('z', '\u{30c}', 'ž'),
];

// ---------------------------------------------------------------------------------------------
// Japanese: Word for Mac's order

/// The symbols Word for Mac sorts before digits, in its order (`so2` probe; the index of `ix5`,
/// 2026-10-19, placed 〝〟㈱ before 「, 〔〕§¶ after 』, †〃 after %, °℃←→ after ^, ±÷× after +, ≠
/// after =, ∞○★♪ after ~, 〓〠 after 〒 and ①② after ￥): spaces, then marks, the iteration
/// marks and ー, then currency and circled digits. A half-width and a full-width form are the
/// same symbol (`,` = `，`, `~` = `～`); a symbol not here sorts after these by code point.
const JA_SYMBOLS: &str =
    " _-\u{2010}\u{2013}\u{2014}\u{301C}・,、;:!?.…。'\"()[]{}〝〟㈱「」『』〔〕§¶@*/\\&#%†〃※`^°℃←→+±÷×<=≠>|~∞○★♪〒〓〠々ゝゞーヽヾ$￥①②";

/// Half-width katakana (U+FF66…U+FF9D) as full-width katakana, in code-point order.
const HALF_KATAKANA: &str = "ヲァィゥェォャュョッーアイウエオカキクケコサシスセソタチツテトナニヌネノハヒフヘホマミムメモヤユヨラリルレロワン";

/// Primary weight classes of Word's Japanese order, above any code point's [`weight`].
const JA_SYMBOL: u32 = 1 << 28;
const JA_DIGIT: u32 = 2 << 28;
const JA_LETTER: u32 = 3 << 28;
const JA_KANA: u32 = 4 << 28;
const JA_OTHER: u32 = 5 << 28;

/// Whether `s` has East Asian text (kana, ideographs, their punctuation, full- or half-width
/// forms), which sorts in Word's Japanese order whatever the language.
fn has_east_asian(s: &str) -> bool {
    s.chars().any(|c| matches!(c as u32, 0x3000..=0x30FF | 0x3400..=0x9FFF | 0xF900..=0xFAFF | 0xFF00..=0xFFEF))
}

/// An ideograph (CJK, compatibility, extensions).
fn is_ideograph(c: char) -> bool {
    matches!(c as u32, 0x3400..=0x4DBF | 0x4E00..=0x9FFF | 0xF900..=0xFAFF | 0x20000..=0x3FFFF)
}

/// A full-width ASCII form as ASCII (`Ａ` → `A`, `，` → `,`), an ideographic space as a space, a
/// half-width katakana as full-width, `¥` as `￥`; anything else as it is.
fn fold_width(c: char) -> char {
    match c as u32 {
        0x3000 => ' ',
        0xFF01..=0xFF5E => char::from_u32(c as u32 - 0xFEE0).unwrap_or(c),
        0xFF66..=0xFF9D => HALF_KATAKANA.chars().nth((c as u32 - 0xFF66) as usize).unwrap_or(c),
        0xA5 => '￥',
        _ => c,
    }
}

/// A kana's letter and voicing: katakana as hiragana, a small kana as the large one, a voiced
/// kana as its plain letter with voicing 1 (dakuten) or 2 (handakuten); ゔ is う voiced, ヷ…ヺ
/// わ…を voiced. `None` for anything that isn't kana.
fn kana_base(c: char) -> Option<(char, u16)> {
    let cp = c as u32;
    let h = match cp {
        0x3041..=0x3096 => cp,
        0x30A1..=0x30F6 => cp - 0x60,
        0x30F7..=0x30FA => return Some((char::from_u32(0x308F + (cp - 0x30F7))?, 1)),
        _ => return None,
    };
    let (base, sec) = match h {
        0x3041 | 0x3043 | 0x3045 | 0x3047 | 0x3049 | 0x3063 | 0x3083 | 0x3085 | 0x3087 | 0x308E => (h + 1, 0),
        0x3095 => (0x304B, 0),
        0x3096 => (0x3051, 0),
        0x3094 => (0x3046, 1),
        0x304C | 0x304E | 0x3050 | 0x3052 | 0x3054 | 0x3056 | 0x3058 | 0x305A | 0x305C | 0x305E | 0x3060 | 0x3062 | 0x3065 | 0x3067 | 0x3069
        | 0x3070 | 0x3073 | 0x3076 | 0x3079 | 0x307C => (h - 1, 1),
        0x3071 | 0x3074 | 0x3077 | 0x307A | 0x307D => (h - 2, 2),
        _ => (h, 0),
    };
    Some((char::from_u32(base)?, sec))
}

/// An ideograph's place in Word for Mac's index order: its Shift-JIS code (JIS X 0208 row and
/// cell: 亜 889F, 一 88EA, 京 8B9E, 大 91E5, 東 938C, 龠 EAA4), else — a kanji the code page lacks —
/// after them all by code point. (Word sorts by JIS X 0213, which puts the few kanji outside
/// JIS X 0208 — 𠮷, 﨑, 髙 — among the rest; they come after here.)
fn ideograph_rank(c: char) -> u32 {
    let mut buf = [0u8; 4];
    let (bytes, _, unmappable) = encoding_rs::SHIFT_JIS.encode(c.encode_utf8(&mut buf));
    match (unmappable, bytes.first(), bytes.get(1)) {
        (false, Some(b1), Some(b2)) if bytes.len() == 2 => (u32::from(*b1) << 8) | u32::from(*b2),
        _ => 0x1_0000 + (c as u32),
    }
}

/// A letter of the Latin script (after width folding and decomposition): Word's Japanese index
/// files it among the Latin letters; Greek, Cyrillic, Hangul and other letters go after the kanji.
fn is_latin_letter(c: char) -> bool {
    c.is_alphabetic() && matches!(c as u32, 0..=0x24F | 0x1E00..=0x1EFF | 0x2160..=0x2188)
}

/// The next collation element of `rest` (lowercase) in Word's Japanese order, and its length.
/// `index`: the order of Word for Mac's index (`ix1`–`ix4` probes, 2026-10-19), where kanji
/// follow their JIS code — 塩 横 沖 学 漢 京 銀 札 山 大 東 福 名 和 — and Greek, Cyrillic and Hangul
/// letters come after every kanji (α Ωmega Я 한국 after 𠮷野家), where a table sort takes kanji
/// and other letters by code point (`so1`/`so2`).
fn next_ja_elem(rest: &str, index: bool) -> Option<(Elem<'_>, usize)> {
    let c0 = rest.chars().next()?;
    let n = c0.len_utf8();
    let text = rest.get(..n)?;
    let elem = |primary: Vec<u32>, sec: u16| Elem { primary, sec, text, own: false };
    // Voicing marks (combining, or the half-width ﾞ ﾟ) and combining marks add to the letter
    // before them; the spacing ゛ ゜ are symbols of their own (ハ゛ sorts after バ, before ﾊﾝ).
    match c0 as u32 {
        0x3099 | 0xFF9E => return Some((elem(Vec::new(), 1), n)),
        0x309A | 0xFF9F => return Some((elem(Vec::new(), 2), n)),
        0x300..=0x36F => return Some((elem(Vec::new(), 32 + (c0 as u32 - 0x300) as u16), n)),
        _ => {}
    }
    let c = fold_width(c0);
    if c == '〆' {
        return Some((elem(vec![JA_KANA + weight('し', 0), JA_KANA + weight('め', 0)], 0), n));
    }
    if let Some(i) = JA_SYMBOLS.chars().position(|s| s == c) {
        return Some((elem(vec![JA_SYMBOL + i as u32], 0), n));
    }
    if c == '〇' {
        return Some((elem(vec![JA_DIGIT], 1), n));
    }
    if let Some(d) = c.to_digit(10) {
        return Some((elem(vec![JA_DIGIT + d], 0), n));
    }
    if let Some((base, sec)) = kana_base(c) {
        return Some((elem(vec![JA_KANA + weight(base, 0)], sec), n));
    }
    if let Some((_, base, mark)) = DECOMP.iter().find(|(x, _, _)| *x == c) {
        return Some((elem(base.chars().map(|b| JA_LETTER + weight(b, 0)).collect(), *mark), n));
    }
    if index && is_ideograph(c) {
        return Some((elem(vec![JA_OTHER + ideograph_rank(c)], 0), n));
    }
    if is_ideograph(c) || !c.is_alphabetic() && !c.is_numeric() {
        let class = if is_ideograph(c) || c.is_alphanumeric() { JA_OTHER } else { JA_SYMBOL + 0x1000 };
        return Some((elem(vec![class + weight(c, 0)], 0), n));
    }
    if index && c.is_alphabetic() && !is_latin_letter(c) {
        return Some((elem(vec![JA_OTHER + 0x8_0000 + weight(c, 0)], 0), n));
    }
    Some((elem(vec![if c.is_alphabetic() { JA_LETTER } else { JA_OTHER } + weight(c, 0)], 0), n))
}

/// The heading an entry files under in Word for Mac's Japanese index (`INDEX \h "A"` with
/// Japanese editing; `ix1`–`ix4` probes, 2026-10-19), from its first character: a digit's
/// full-width form (123 and １２３ under １, 7up under ７); a Latin letter's full-width capital
/// (apple, Ａｂｅ under Ａ; éclair under Ｅ); a kana's hiragana, voicing dropped, small kana kept
/// (アメリカ under あ, が and ｶﾀｶﾅ under か, ﾊﾟﾝ under は, ゞ under ゝ, ｧ under ぁ, ャ under ゃ; ゔ
/// and ヴ under ゔ, ヶ under ヶ, ゐ ヲ ん ー 〆 under themselves); a kanji, 々, or any other
/// letter (Greek, Cyrillic, Hangul) under 漢字; a numeral under itself (①, Ⅰ); a symbol under
/// its full-width form (!bang under ！, (paren) under （, #hash under ＃, ・ 〒 〃 」 as they are).
/// `None` for an empty entry.
pub fn ja_heading(s: &str) -> Option<String> {
    let c0 = s.trim_start().chars().next()?;
    let c = fold_width(c0);
    let one = |c: char| Some(c.to_string());
    if let Some(d) = c.to_digit(10) {
        return char::from_u32(0xFF10 + d).and_then(one);
    }
    let latin = |c: char| c.is_ascii_alphabetic().then(|| char::from_u32(0xFF21 + (c.to_ascii_uppercase() as u32 - u32::from(b'A')))).flatten();
    if let Some(h) = latin(c) {
        return one(h);
    }
    if let Some(h) =
        DECOMP.iter().find(|(x, _, _)| *x == c.to_lowercase().next().unwrap_or(c)).and_then(|(_, base, _)| base.chars().next()).and_then(latin)
    {
        return one(h);
    }
    let cp = c as u32;
    let hira = match cp {
        0x3041..=0x3096 | 0x309D..=0x309F | 0x3006 => Some(cp),
        0x30A1..=0x30F4 => Some(cp - 0x60),
        0x30F5..=0x30FA | 0x30FC..=0x30FF => Some(cp),
        _ => None,
    }
    .and_then(char::from_u32);
    if let Some(h) = hira {
        let shown = match h {
            'ゞ' => 'ゝ',
            'ヾ' => 'ヽ',
            'ゔ' | '〆' => h,
            _ => match kana_base(h) {
                Some((base, sec)) if sec > 0 => base,
                _ => h,
            },
        };
        return one(shown);
    }
    if c.is_numeric() {
        return one(c);
    }
    if c.is_alphabetic() {
        return Some("漢字".into());
    }
    match cp {
        0x21..=0x7E => char::from_u32(cp + 0xFEE0).and_then(one),
        _ => one(c),
    }
}

/// `s` lowercased, with each letter + combining mark that has a single-letter form composed.
fn lower_composed(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.to_lowercase().chars() {
        if let Some(prev) = out.chars().next_back()
            && let Some((_, _, composed)) = COMPOSE.iter().find(|(b, m, _)| *b == prev && *m == c)
        {
            out.pop();
            out.push(*composed);
        } else {
            out.push(c);
        }
    }
    out
}

fn weight(c: char, rank: u32) -> u32 {
    ((c as u32) << 4).saturating_add(rank)
}

/// One collation element: primary weight, secondary weight, the text it covers.
struct Elem<'a> {
    primary: Vec<u32>,
    sec: u16,
    text: &'a str,
    /// Set when the text is a letter of its own in the language (shown as is in headings).
    own: bool,
}

/// The next collation element of `rest` (lowercase), and its length in bytes.
fn next_elem<'a>(rest: &'a str, rules: &[Tailor]) -> Option<(Elem<'a>, usize)> {
    if let Some(r) = rules.iter().filter(|r| rest.starts_with(r.seq)).max_by_key(|r| r.seq.len()) {
        let text = rest.get(..r.seq.len())?;
        return Some((Elem { primary: vec![weight(r.after, r.rank)], sec: r.sec, text, own: true }, r.seq.len()));
    }
    let c = rest.chars().next()?;
    let n = c.len_utf8();
    let text = rest.get(..n)?;
    if let Some((_, base, mark)) = DECOMP.iter().find(|(x, _, _)| *x == c) {
        return Some((Elem { primary: base.chars().map(|b| weight(b, 0)).collect(), sec: *mark, text, own: false }, n));
    }
    // A combining mark adds a secondary difference to the letter before it.
    if ('\u{300}'..='\u{36f}').contains(&c) {
        return Some((Elem { primary: Vec::new(), sec: 32 + (c as u32 - 0x300) as u16, text, own: false }, n));
    }
    Some((Elem { primary: vec![weight(c, 0)], sec: 0, text, own: false }, n))
}

/// A sort key: compare keys to sort strings in a language's order. Equal keys mean the strings
/// differ at most in letter case.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub struct SortKey {
    primary: Vec<u32>,
    secondary: Vec<u16>,
    text: String,
}

/// The sort key of `s` for language `lang` (a BCP 47 tag such as `pl-PL`; `None` sorts like
/// English). Japanese (`ja`), or any text with East Asian characters, takes Word's Japanese
/// order (see the module notes); its keys tell strings apart by their letters, voicing and
/// diacritics only, so a stable sort keeps `ab`, `ＡＢ` and `ｱ`, `あ` as they were, as Word does.
pub fn sort_key(s: &str, lang: Option<&str>) -> SortKey {
    sort_key_in(s, lang, false)
}

/// The sort key of an index entry: Word for Mac's index order, which differs from its table
/// sort for Japanese text (see [`next_ja_elem`]).
pub fn index_sort_key(s: &str, lang: Option<&str>) -> SortKey {
    sort_key_in(s, lang, true)
}

fn sort_key_in(s: &str, lang: Option<&str>, index: bool) -> SortKey {
    let text = lower_composed(s);
    let ja = language(lang) == "ja" || has_east_asian(&text);
    let rules = tailoring(lang);
    let mut key = SortKey::default();
    let mut i = 0;
    while let Some(rest) = text.get(i..).filter(|r| !r.is_empty()) {
        let Some((e, n)) = (if ja { next_ja_elem(rest, index) } else { next_elem(rest, rules) }) else { break };
        i += n.max(1);
        if e.primary.is_empty() {
            if let Some(last) = key.secondary.last_mut() {
                *last = last.saturating_add(e.sec);
            }
            continue;
        }
        for (k, w) in e.primary.iter().enumerate() {
            key.primary.push(*w);
            key.secondary.push(if k == 0 { e.sec } else { 0 });
        }
    }
    // An index keeps 123 and １２３, が and ガ as entries of their own, in that order (`ix1`).
    if !ja || index {
        key.text = text;
    }
    key
}

/// Compare two strings in the order of language `lang`, ignoring case.
pub fn collate(a: &str, b: &str, lang: Option<&str>) -> Ordering {
    sort_key(a, lang).cmp(&sort_key(b, lang))
}

/// The index heading letter `s` files under in language `lang`: `Ł` for `Łódź` in Polish, `L`
/// in English; `Ch` for `chata` in Czech; `A` for `Äpfel` in German, `Ä` in Swedish.
pub fn heading_letter(s: &str, lang: Option<&str>) -> Option<String> {
    let low = lower_composed(s.trim_start());
    let (e, _) = next_elem(&low, tailoring(lang))?;
    let shown = if e.own || e.sec == 0 {
        e.text.to_string()
    } else {
        DECOMP.iter().find(|(x, _, _)| e.text.starts_with(*x)).map(|(_, b, _)| b.chars().take(1).collect()).unwrap_or_else(|| e.text.to_string())
    };
    let mut cs = shown.chars();
    let first = cs.next()?;
    Some(first.to_uppercase().chain(cs).collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sorted(words: &[&str], lang: &str) -> Vec<String> {
        let mut v: Vec<String> = words.iter().map(|w| w.to_string()).collect();
        v.sort_by(|a, b| collate(a, b, Some(lang)));
        v
    }

    #[test]
    fn polish_letters_follow_their_base_letter() {
        assert_eq!(sorted(&["Zakopane", "Łódź", "Kraków"], "pl-PL"), ["Kraków", "Łódź", "Zakopane"]);
        assert_eq!(sorted(&["zebra", "ćma", "dom"], "pl-PL"), ["ćma", "dom", "zebra"]);
        assert_eq!(sorted(&["żaba", "źle", "zebra", "ząb", "lato", "łapa", "mama"], "pl"), ["lato", "łapa", "mama", "ząb", "zebra", "źle", "żaba"]);
        assert_eq!(sorted(&["b", "ą", "a"], "pl"), ["a", "ą", "b"]);
        // Every Polish letter is a letter of its own, after its base letter (review of #844).
        assert_eq!(sorted(&["Malbork", "Łódź", "Lublin"], "pl"), ["Lublin", "Łódź", "Malbork"]);
        assert_eq!(sorted(&["ósmy", "owca", "pies"], "pl"), ["owca", "ósmy", "pies"]);
        assert_eq!(sorted(&["ściana", "sum", "tak"], "pl"), ["sum", "ściana", "tak"]);
        assert_eq!(sorted(&["ęsi", "ezop", "fala"], "pl"), ["ezop", "ęsi", "fala"]);
        assert_eq!(sorted(&["ńa", "nz", "oko"], "pl"), ["nz", "ńa", "oko"]);
        assert_eq!(heading_letter("ósmy", Some("pl")).as_deref(), Some("Ó"));
        assert_eq!(heading_letter("ńa", Some("pl")).as_deref(), Some("Ń"));
        // Decomposed letters (`z` + combining dot above, as macOS file names write `ż`) sort like
        // the composed ones.
        assert_eq!(collate("z\u{307}aba", "żaba", Some("pl-PL")), Ordering::Equal);
        assert_eq!(collate("z\u{307}aba", "zebra", Some("pl-PL")), Ordering::Greater);
        assert_eq!(heading_letter("Z\u{307}aba", Some("pl")).as_deref(), Some("Ż"));
        assert_eq!(collate("a\u{328}", "ą", Some("pl")), Ordering::Equal);
    }

    #[test]
    fn swedish_puts_a_ring_and_umlauts_after_z() {
        assert_eq!(sorted(&["ödla", "zebra", "äpple", "åka", "apa"], "sv-SE"), ["apa", "zebra", "åka", "äpple", "ödla"]);
    }

    #[test]
    fn german_sorts_umlauts_with_their_base_letter() {
        assert_eq!(sorted(&["Zug", "Äpfel", "Apfel", "Bach"], "de-DE"), ["Apfel", "Äpfel", "Bach", "Zug"]);
        assert_eq!(sorted(&["Straße", "Strasse", "Strand"], "de"), ["Strand", "Strasse", "Straße"]);
    }

    #[test]
    fn spanish_n_tilde_after_n() {
        assert_eq!(sorted(&["oso", "ñu", "nube"], "es-ES"), ["nube", "ñu", "oso"]);
    }

    #[test]
    fn czech_slovak_hungarian_alphabets() {
        assert_eq!(sorted(&["chata", "hrad", "ihla", "cesta", "čaj", "dům"], "cs-CZ"), ["cesta", "čaj", "dům", "hrad", "chata", "ihla"]);
        assert_eq!(sorted(&["ôsmy", "oko", "pes", "ä", "b"], "sk"), ["ä", "b", "oko", "ôsmy", "pes"]);
        assert_eq!(sorted(&["csak", "cukor", "dob", "szó", "sűrű", "tál"], "hu-HU"), ["cukor", "csak", "dob", "sűrű", "szó", "tál"]);
        // ö and ő are one letter after o, told apart by the accent only.
        assert_eq!(sorted(&["ötös", "őr", "pálya", "olló", "ör"], "hu"), ["olló", "ör", "őr", "ötös", "pálya"]);
    }

    #[test]
    fn english_keeps_case_insensitive_order_and_folds_accents() {
        assert_eq!(sorted(&["rex", "résumé", "resume", "Apple", "banana"], "en-US"), ["Apple", "banana", "resume", "résumé", "rex"]);
        assert_eq!(collate("Łódź", "Lodz", None), Ordering::Greater);
        assert_eq!(collate("Łódź", "Lodzia", None), Ordering::Less);
        assert_eq!(collate("ABC", "abc", None), Ordering::Equal);
    }

    #[test]
    fn heading_letters() {
        assert_eq!(heading_letter("Łódź", Some("pl-PL")).as_deref(), Some("Ł"));
        assert_eq!(heading_letter("Łódź", Some("en-US")).as_deref(), Some("L"));
        assert_eq!(heading_letter("chata", Some("cs")).as_deref(), Some("Ch"));
        assert_eq!(heading_letter("Äpfel", Some("de")).as_deref(), Some("A"));
        assert_eq!(heading_letter("Äpple", Some("sv")).as_deref(), Some("Ä"));
        assert_eq!(heading_letter("", Some("sv")), None);
    }

    /// Japanese text sorts as Word for Mac sorts it: two one-column tables of 81 and 288
    /// strings (kana in every form, punctuation and its full-width forms, digits, Latin with
    /// accents and widths, kanji) sorted by Word (`so1`/`so2` probes, 2026-10-19) come out in
    /// Word's order from a stable sort by `sort_key`, ties included.
    /// Word for Mac's index (`ix1`–`ix4` probes, 2026-10-19): kanji in JIS order, other
    /// scripts' letters after them, and the headings entries file under.
    #[test]
    fn japanese_index_order_and_headings_follow_word_for_mac() {
        let mut v = vec![
            "京都",
            "名古屋",
            "和歌山",
            "塩",
            "大阪",
            "学校",
            "山",
            "札幌",
            "東京",
            "横浜",
            "沖縄",
            "漢字",
            "福岡",
            "銀行",
            "α",
            "Ωmega",
            "Я",
            "한국",
            "apple",
            "あ",
            "123",
            "Ⅰ",
            "𠮷野家",
        ];
        v.sort_by_key(|a| index_sort_key(a, Some("ja")));
        assert_eq!(
            v,
            [
                "123",
                "apple",
                "Ⅰ",
                "あ",
                "塩",
                "横浜",
                "沖縄",
                "学校",
                "漢字",
                "京都",
                "銀行",
                "札幌",
                "山",
                "大阪",
                "東京",
                "福岡",
                "名古屋",
                "和歌山",
                "𠮷野家",
                "α",
                "Ωmega",
                "Я",
                "한국"
            ]
        );
        // A table sort keeps its code-point order.
        assert_eq!(sorted(&["塩", "東京", "京都"], "ja-JP"), ["京都", "塩", "東京"]);
        for (entry, heading) in [
            ("123", "１"),
            ("１２３", "１"),
            ("7up", "７"),
            ("apple", "Ａ"),
            ("Ａｂｅ", "Ａ"),
            ("ｂanana", "Ｂ"),
            ("éclair", "Ｅ"),
            ("アメリカ", "あ"),
            ("が", "か"),
            ("ｶﾀｶﾅ", "か"),
            ("ﾊﾟﾝ", "は"),
            ("ぱ", "は"),
            ("ジ", "し"),
            ("ヂ", "ち"),
            ("づ", "つ"),
            ("ゞ", "ゝ"),
            ("ｧ", "ぁ"),
            ("ャ", "ゃ"),
            ("ゔ", "ゔ"),
            ("ヴぇ", "ゔ"),
            ("ヶ", "ヶ"),
            ("ゐ", "ゐ"),
            ("ヲ", "を"),
            ("ん", "ん"),
            ("ー音", "ー"),
            ("〆", "〆"),
            ("漢字", "漢字"),
            ("々印", "漢字"),
            ("Ωmega", "漢字"),
            ("Я", "漢字"),
            ("한국", "漢字"),
            ("①丸", "①"),
            ("Ⅰ", "Ⅰ"),
            ("!bang", "！"),
            ("(paren)", "（"),
            ("#hash", "＃"),
            ("%pct", "％"),
            ("・中点", "・"),
            ("〒郵便", "〒"),
            ("〃", "〃"),
            ("」閉", "」"),
        ] {
            assert_eq!(ja_heading(entry).as_deref(), Some(heading), "{entry}");
        }
        assert_eq!(ja_heading(""), None);
    }

    #[test]
    fn japanese_text_sorts_as_word_for_mac_does() {
        for (input, word) in [(SO1_IN, SO1_WORD), (SO2_IN, SO2_WORD)] {
            let ours = sorted(input, "ja-JP");
            assert_eq!(ours, word, "\nours: {}\nword: {}", ours.join(" "), word.join(" "));
        }
        // The same order for a document in another language when the text is East Asian.
        assert_eq!(sorted(&["ばら", "はら", "ぱら", "ハラ"], "en-US"), ["はら", "ハラ", "ばら", "ぱら"]);
        assert_eq!(collate("ｶﾞ", "が", Some("ja")), Ordering::Equal);
        assert_eq!(collate("か\u{3099}", "が", Some("ja")), Ordering::Equal);
        assert_eq!(collate("〆", "しめ", Some("ja")), Ordering::Equal);
        assert_eq!(collate("¥", "￥", Some("ja")), Ordering::Equal);
        // Latin-only text in a Japanese document: symbols, digits, letters, case ignored.
        assert_eq!(sorted(&["b", "A", "1", "-", "a", "10", "2"], "ja"), ["-", "1", "10", "2", "A", "a", "b"]);
    }

    const SO1_IN: &[&str] = &[
        "さくら",
        "サクラ",
        "ｻｸﾗ",
        "桜",
        "さくらんぼ",
        "ざくろ",
        "つき",
        "っき",
        "づき",
        "ツキ",
        "ばら",
        "ぱら",
        "はら",
        "バラ",
        "パラ",
        "ハラ",
        "らーめん",
        "ラーメン",
        "らあめん",
        "らいめん",
        "ゆき",
        "ゆうき",
        "ゆぅき",
        "ユキ",
        "apple",
        "Apple",
        "ＡＰＰＬＥ",
        "ａｐｐｌｅ",
        "123",
        "１２３",
        "１２",
        "12",
        "東京",
        "大阪",
        "京都",
        "北海道",
        "一",
        "二",
        "三",
        "亜",
        "あ",
        "ア",
        "ん",
        "ン",
        "ゔ",
        "ヴ",
        "ゐ",
        "ゑ",
        "を",
        "ヲ",
        "ー",
        "ぁ",
        "ァ",
        "　",
        "  ",
        "(かっこ)",
        "（かっこ）",
        "1月",
        "１月",
        "10月",
        "2月",
        "ab",
        "a-b",
        "a b",
        "a_b",
        "abc",
        "ａｂｃ",
        "ABC",
        "Ａ",
        "Ｂ",
        "b",
        "B",
        "ば",
        "ばあ",
        "ばー",
        "びゃ",
        "びや",
        "ぴゃ",
        "キャ",
        "キヤ",
        "ぎゃ",
    ];
    const SO1_WORD: &[&str] = &[
        "　",
        "  ",
        "(かっこ)",
        "（かっこ）",
        "ー",
        "10月",
        "１２",
        "12",
        "123",
        "１２３",
        "1月",
        "１月",
        "2月",
        "Ａ",
        "a b",
        "a_b",
        "a-b",
        "ab",
        "abc",
        "ａｂｃ",
        "ABC",
        "apple",
        "Apple",
        "ＡＰＰＬＥ",
        "ａｐｐｌｅ",
        "Ｂ",
        "b",
        "B",
        "あ",
        "ア",
        "ぁ",
        "ァ",
        "ゔ",
        "ヴ",
        "キャ",
        "キヤ",
        "ぎゃ",
        "さくら",
        "サクラ",
        "ｻｸﾗ",
        "さくらんぼ",
        "ざくろ",
        "つき",
        "っき",
        "ツキ",
        "づき",
        "ば",
        "ばー",
        "ばあ",
        "はら",
        "ハラ",
        "ばら",
        "バラ",
        "ぱら",
        "パラ",
        "びゃ",
        "びや",
        "ぴゃ",
        "ゆうき",
        "ゆぅき",
        "ゆき",
        "ユキ",
        "らーめん",
        "ラーメン",
        "らあめん",
        "らいめん",
        "ゐ",
        "ゑ",
        "を",
        "ヲ",
        "ん",
        "ン",
        "一",
        "三",
        "二",
        "亜",
        "京都",
        "北海道",
        "大阪",
        "東京",
        "桜",
    ];
    const SO2_IN: &[&str] = &[
        "ヂ",
        "AB",
        "ﾊﾟﾝ",
        "＾",
        "ﾊﾞﾝ",
        "２",
        "#",
        "こうし",
        "壱",
        "e",
        "韻",
        "威",
        "旭",
        "%",
        "％",
        "５",
        "Z",
        "1a",
        ",",
        "斡",
        "ヅ",
        "ａｂ",
        "`",
        "ゐ",
        "位",
        "{",
        "Ａ",
        "姶",
        "œ",
        "磯",
        "謂",
        "淫",
        "亜",
        "‐",
        "ñ",
        "・",
        "[",
        "［",
        "＜",
        "3",
        "ブ",
        "a1",
        "＊",
        "9",
        "ｶｷ",
        "委",
        "＃",
        "n",
        "姐",
        "z",
        "郁",
        "以",
        "こおし",
        "ss",
        "、",
        "…",
        "＠",
        "衣",
        "ず",
        "「",
        "ﾊﾝ",
        "悪",
        "唖",
        "圧",
        "袷",
        "（",
        "鯵",
        "違",
        "バン",
        "粟",
        "闇",
        "ö",
        "遺",
        "1",
        "員",
        "！",
        "4",
        "畏",
        "為",
        "*",
        "絢",
        "はん",
        "ばん",
        "-",
        "異",
        "逢",
        "鰯",
        "!",
        "ば",
        "ぶ",
        "茨",
        "10",
        "/",
        "é",
        "］",
        "緯",
        ")",
        "椅",
        "(",
        "ゃま",
        "0",
        "｜",
        ".",
        "やま",
        "＞",
        "惟",
        "＝",
        "ｴ",
        "〆",
        "阿",
        "８",
        "＿",
        "'",
        "３",
        "5",
        "2",
        "娃",
        "。",
        "依",
        "a",
        "ａＢ",
        "慰",
        "）",
        "こーし",
        "￥",
        "：",
        "域",
        "穐",
        "院",
        "\"",
        "ab",
        "」",
        "庵",
        "おう",
        "$",
        "偉",
        "え",
        "愛",
        "．",
        "@",
        "隠",
        "｀",
        "６",
        "０",
        "ヤマ",
        "杏",
        "々",
        "ヴ",
        "允",
        "ー",
        "易",
        "かき",
        "aB",
        "葦",
        "或",
        "Ｚ",
        "９",
        "カキ",
        "暗",
        "＂",
        "扱",
        "＼",
        "A",
        "飴",
        "飲",
        "E",
        "，",
        "9",
        "じ",
        "&",
        "7",
        "７",
        "〜",
        "宛",
        "囲",
        "鮎",
        "吋",
        "〒",
        "尉",
        "意",
        "A1",
        "蔭",
        "づ",
        "』",
        "育",
        "<",
        "鞍",
        "維",
        "夷",
        "〇",
        "8",
        "ＡＢ",
        "按",
        "伊",
        "移",
        "エ",
        "挨",
        "因",
        "aa",
        "+",
        "芦",
        "おお",
        "＇",
        "井",
        "哀",
        "＋",
        "ハ゛",
        "ぱん",
        "稲",
        "=",
        "è",
        "医",
        "溢",
        "zz",
        "6",
        "おー",
        "パン",
        "案",
        "ゞ",
        "?",
        "｝",
        "茜",
        "Ａｂ",
        "胃",
        "一",
        "逸",
        "–",
        "１",
        "ジ",
        "ヾ",
        "ぢ",
        "※",
        "葵",
        "姻",
        "ハン",
        "萎",
        "オー",
        "？",
        "ß",
        ":",
        "ｪ",
        ";",
        "ａ",
        "ぇ",
        "陰",
        "４",
        "胤",
        "o",
        "ゝ",
        "引",
        "_",
        "芋",
        "Ab",
        "梓",
        "ェ",
        "É",
        "｛",
        "～",
        "印",
        "ヽ",
        "『",
        "\\",
        "ゔ",
        "^",
        "渥",
        "~",
        "ヸ",
        "～",
        "}",
        "綾",
        "／",
        "ズ",
        "＄",
        "虻",
        "安",
        "－",
        ">",
        "バ",
        "亥",
        "|",
        "ｚ",
        "；",
        "咽",
        "＆",
        "]",
        "握",
        "—",
        "oe",
    ];
    const SO2_WORD: &[&str] = &[
        "＿",
        "_",
        "-",
        "－",
        "‐",
        "–",
        "—",
        "〜",
        "・",
        ",",
        "，",
        "、",
        ";",
        "；",
        "：",
        ":",
        "！",
        "!",
        "?",
        "？",
        ".",
        "．",
        "…",
        "。",
        "'",
        "＇",
        "\"",
        "＂",
        "（",
        "(",
        ")",
        "）",
        "[",
        "［",
        "］",
        "]",
        "{",
        "｛",
        "｝",
        "}",
        "「",
        "」",
        "『",
        "』",
        "＠",
        "@",
        "＊",
        "*",
        "/",
        "／",
        "＼",
        "\\",
        "&",
        "＆",
        "#",
        "＃",
        "%",
        "％",
        "※",
        "`",
        "｀",
        "＾",
        "^",
        "+",
        "＋",
        "＜",
        "<",
        "＝",
        "=",
        "＞",
        ">",
        "｜",
        "|",
        "～",
        "~",
        "～",
        "〒",
        "々",
        "ゝ",
        "ゞ",
        "ー",
        "ヽ",
        "ヾ",
        "$",
        "＄",
        "￥",
        "0",
        "０",
        "〇",
        "1",
        "１",
        "10",
        "1a",
        "２",
        "2",
        "3",
        "３",
        "4",
        "４",
        "５",
        "5",
        "６",
        "6",
        "7",
        "７",
        "８",
        "8",
        "9",
        "９",
        "9",
        "Ａ",
        "a",
        "A",
        "ａ",
        "a1",
        "A1",
        "aa",
        "AB",
        "ａｂ",
        "ａＢ",
        "ab",
        "aB",
        "ＡＢ",
        "Ａｂ",
        "Ab",
        "e",
        "E",
        "é",
        "É",
        "è",
        "n",
        "ñ",
        "o",
        "ö",
        "oe",
        "œ",
        "ss",
        "ß",
        "Z",
        "z",
        "Ｚ",
        "ｚ",
        "zz",
        "ヴ",
        "ゔ",
        "ｴ",
        "え",
        "エ",
        "ｪ",
        "ぇ",
        "ェ",
        "おー",
        "オー",
        "おう",
        "おお",
        "ｶｷ",
        "かき",
        "カキ",
        "こーし",
        "こうし",
        "こおし",
        "じ",
        "ジ",
        "〆",
        "ず",
        "ズ",
        "ヂ",
        "ぢ",
        "ヅ",
        "づ",
        "ば",
        "バ",
        "ハ゛",
        "ﾊﾝ",
        "はん",
        "ハン",
        "ﾊﾞﾝ",
        "バン",
        "ばん",
        "ﾊﾟﾝ",
        "ぱん",
        "パン",
        "ブ",
        "ぶ",
        "ゃま",
        "やま",
        "ヤマ",
        "ゐ",
        "ヸ",
        "一",
        "井",
        "亜",
        "亥",
        "以",
        "伊",
        "位",
        "依",
        "偉",
        "允",
        "医",
        "印",
        "吋",
        "咽",
        "哀",
        "員",
        "唖",
        "因",
        "囲",
        "圧",
        "域",
        "壱",
        "夷",
        "姐",
        "委",
        "姶",
        "姻",
        "威",
        "娃",
        "安",
        "宛",
        "尉",
        "庵",
        "引",
        "悪",
        "惟",
        "意",
        "愛",
        "慰",
        "或",
        "扱",
        "按",
        "挨",
        "握",
        "斡",
        "旭",
        "易",
        "暗",
        "杏",
        "案",
        "梓",
        "椅",
        "淫",
        "渥",
        "溢",
        "為",
        "畏",
        "異",
        "磯",
        "移",
        "稲",
        "穐",
        "粟",
        "絢",
        "維",
        "綾",
        "緯",
        "育",
        "胃",
        "胤",
        "芋",
        "芦",
        "茜",
        "茨",
        "萎",
        "葦",
        "葵",
        "蔭",
        "虻",
        "衣",
        "袷",
        "謂",
        "逢",
        "逸",
        "違",
        "遺",
        "郁",
        "闇",
        "阿",
        "院",
        "陰",
        "隠",
        "鞍",
        "韻",
        "飲",
        "飴",
        "鮎",
        "鯵",
        "鰯",
    ];

    #[test]
    fn hostile_input_never_panics() {
        for s in [
            "",
            "\u{301}",
            "a\u{301}\u{301}\u{301}",
            "dzs",
            "\u{10FFFF}",
            "ẞ",
            "İstanbul",
            "\u{3099}",
            "ﾞﾟ\u{309B}",
            "ヺ〆〇ー",
            "\u{FF9D}\u{FF66}",
            "\u{2FFFF}",
        ] {
            for l in [None, Some("hu"), Some(""), Some("-"), Some("tr-TR"), Some("ja-JP")] {
                let _ = sort_key(s, l);
                let _ = heading_letter(s, l);
            }
        }
    }
}

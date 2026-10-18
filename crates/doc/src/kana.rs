//! Word's Japanese Change Case kinds (文字種の変換): Half-width, Full-width, Hiragana and
//! Katakana. Observed black-box in Word for Mac with Japanese as the East Asian editing
//! language (2026-10-18; `set case of text object to case half width` etc. and the text read
//! back), including Word's handling of the kana sound marks ゛ ゜ ﾞ ﾟ:
//!
//! - **Half-width**: fullwidth ASCII → ASCII (￥ and ＼ both → `\`), ideographic space → space,
//!   katakana and 。、「」・ー゛゜ → their halfwidth forms, a voiced kana → base + ﾞ/ﾟ (ガ → ｶﾞ,
//!   ヴ → ｳﾞ). Kana without a halfwidth form (ヮヰヱヵヶヷヸヹヺヽヾ), hiragana, ￣ and the
//!   fullwidth cent/pound/not/bar/won signs stay.
//! - **Full-width**: ASCII → fullwidth (`\` → ￥), space → ideographic space, halfwidth kana →
//!   fullwidth; a halfwidth kana followed by ﾞ/ﾟ composes when a voiced kana exists (ｶﾞ → ガ,
//!   ﾊﾟ → パ, ｳﾞ → ヴ), otherwise the mark becomes ゛/゜ (ﾜﾞ → ワ゛, ｲﾟ → イ゜). A fullwidth
//!   mark never composes (ｶ゛ → カ゛). `¥`, Latin-1 and halfwidth Hangul stay.
//! - **Hiragana**: katakana ァ..ヴ → hiragana, ヽヾ → ゝゞ (ヵヶヷ..ヺ stay); halfwidth kana →
//!   hiragana composing ﾞ/ﾟ (ｶﾞ → が). Word's quirks with a mark that can't attach: after an
//!   East Asian character a halfwidth ﾞ/ﾟ is dropped, and the halfwidth kana before it stays
//!   halfwidth (ｲﾟ → ｲ, ﾜﾞ → ﾜ, 漢ﾞ → 漢, カﾞ → か); elsewhere it becomes ゛/゜. A fullwidth
//!   ゛/゜ stays, and still voices a halfwidth kana before it (ｶ゛ → が゛, ｶ゜ → ｶ゜).
//! - **Katakana**: hiragana ぁ..ゔ → katakana, ゝゞ → ヽヾ (ゕゖ stay); う or ウ followed by
//!   ゛ or ﾞ → ヴ (the mark is consumed). Halfwidth text is left alone.
//!
//! Latin letters are untouched by all four (Word's Latin kinds handle them, fullwidth too).

/// One of Word's four Japanese Change Case kinds.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    HalfWidth,
    FullWidth,
    Hiragana,
    Katakana,
}

impl Kind {
    /// The command parameter spelling (`format.changeCase` `mode`).
    pub fn parse(mode: &str) -> Option<Kind> {
        Some(match mode {
            "halfWidth" => Kind::HalfWidth,
            "fullWidth" => Kind::FullWidth,
            "hiragana" => Kind::Hiragana,
            "katakana" => Kind::Katakana,
            _ => return None,
        })
    }
}

/// The fullwidth form of each halfwidth form U+FF61..=U+FF9F, in code point order.
const FULL_OF_HALF: &str =
    "。「」、・ヲァィゥェォャュョッーアイウエオカキクケコサシスセソタチツテトナニヌネノハヒフヘホマミムメモヤユヨラリルレロワン゛゜";

/// Katakana whose voiced form (`+ ﾞ`) is the next code point, and ウ → ヴ.
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
];

/// Katakana with a semi-voiced form (`+ ﾟ`).
const SEMI_VOICED: &[(char, char)] = &[('ハ', 'パ'), ('ヒ', 'ピ'), ('フ', 'プ'), ('ヘ', 'ペ'), ('ホ', 'ポ')];

const HALF_VOICED_MARK: char = 'ﾞ';
const HALF_SEMI_MARK: char = 'ﾟ';
const FULL_VOICED_MARK: char = '゛';
const FULL_SEMI_MARK: char = '゜';

fn is_halfwidth_form(c: char) -> bool {
    ('\u{FF61}'..='\u{FF9F}').contains(&c)
}

fn is_half_mark(c: char) -> bool {
    c == HALF_VOICED_MARK || c == HALF_SEMI_MARK
}

fn is_full_mark(c: char) -> bool {
    c == FULL_VOICED_MARK || c == FULL_SEMI_MARK
}

/// The fullwidth katakana (or punctuation) of a halfwidth form.
fn full_of_half(c: char) -> Option<char> {
    let i = u32::from(c).checked_sub(0xFF61)?;
    FULL_OF_HALF.chars().nth(usize::try_from(i).ok()?)
}

/// The halfwidth form of a fullwidth katakana or punctuation mark.
fn half_of_full(c: char) -> Option<char> {
    let i = FULL_OF_HALF.chars().position(|f| f == c)?;
    char::from_u32(0xFF61u32.checked_add(u32::try_from(i).ok()?)?)
}

/// The voiced (ﾞ) or semi-voiced (ﾟ) katakana of a plain fullwidth katakana.
fn voiced(base: char, semi: bool) -> Option<char> {
    let table = if semi { SEMI_VOICED } else { VOICED };
    table.iter().find(|(b, _)| *b == base).map(|(_, v)| *v)
}

/// A voiced katakana split into its plain katakana and the sound mark that voices it.
fn unvoiced(c: char) -> Option<(char, bool)> {
    if let Some((b, _)) = VOICED.iter().find(|(_, v)| *v == c) {
        return Some((*b, false));
    }
    SEMI_VOICED.iter().find(|(_, v)| *v == c).map(|(b, _)| (*b, true))
}

/// Katakana → hiragana for the characters Word converts (ァ..ヴ, ヽヾ).
fn hiragana_of(c: char) -> char {
    match u32::from(c) {
        0x30A1..=0x30F4 => char::from_u32(u32::from(c) - 0x60).unwrap_or(c),
        0x30FD => 'ゝ',
        0x30FE => 'ゞ',
        _ => c,
    }
}

/// Hiragana → katakana for the characters Word converts (ぁ..ゔ, ゝゞ).
fn katakana_of(c: char) -> char {
    match u32::from(c) {
        0x3041..=0x3094 => char::from_u32(u32::from(c) + 0x60).unwrap_or(c),
        0x309D => 'ヽ',
        0x309E => 'ヾ',
        _ => c,
    }
}

/// Half-width of one character (never composes).
fn half(c: char) -> String {
    match u32::from(c) {
        0x3000 => return " ".into(),
        0xFFE5 => return "\\".into(),
        0xFF01..=0xFF5E => return char::from_u32(u32::from(c) - 0xFEE0).unwrap_or(c).into(),
        _ => {}
    }
    if let Some(h) = half_of_full(c) {
        return h.into();
    }
    if let Some((base, semi)) = unvoiced(c)
        && let Some(h) = half_of_full(base)
    {
        let mut s = String::from(h);
        s.push(if semi { HALF_SEMI_MARK } else { HALF_VOICED_MARK });
        return s;
    }
    c.into()
}

/// Full-width of one character (composition is handled by the caller).
fn full(c: char) -> char {
    match u32::from(c) {
        0x20 => '\u{3000}',
        0x5C => '￥',
        0x21..=0x7E => char::from_u32(u32::from(c) + 0xFEE0).unwrap_or(c),
        _ => full_of_half(c).unwrap_or(c),
    }
}

/// The voiced fullwidth katakana of a halfwidth kana and a sound mark (either width), if the
/// kana can take it.
fn compose(base: char, mark: char) -> Option<char> {
    if !is_halfwidth_form(base) {
        return None;
    }
    let semi = mark == HALF_SEMI_MARK || mark == FULL_SEMI_MARK;
    voiced(full_of_half(base)?, semi)
}

/// The converted form of each character of `src`, in order: one entry per char, so the result
/// can be applied run by run. An empty entry is a character Word removes (a sound mark folded
/// into the kana before it, or one Word drops).
pub fn convert(src: &str, kind: Kind) -> Vec<String> {
    let chars: Vec<char> = src.chars().collect();
    let mut out: Vec<String> = Vec::with_capacity(chars.len());
    let mut i = 0;
    while let Some(&c) = chars.get(i) {
        let next = chars.get(i + 1).copied();
        i += 1;
        match kind {
            Kind::HalfWidth => out.push(half(c)),
            Kind::FullWidth => match next.filter(|n| is_half_mark(*n)).and_then(|n| compose(c, n)) {
                Some(v) => {
                    out.push(v.into());
                    out.push(String::new());
                    i += 1;
                }
                None => out.push(full(c).into()),
            },
            Kind::Hiragana => {
                let convert_one = |c: char| hiragana_of(full_of_half(c).unwrap_or(c));
                match next {
                    // A halfwidth mark after East Asian text: folded in or dropped; a halfwidth
                    // kana it can't voice is left as it is.
                    Some(n) if is_half_mark(n) && crate::east_asian::is_east_asian(c) => {
                        let v = match compose(c, n) {
                            Some(v) => hiragana_of(v),
                            None if is_halfwidth_form(c) => c,
                            None => convert_one(c),
                        };
                        out.push(v.into());
                        out.push(String::new());
                        i += 1;
                    }
                    // A fullwidth mark voices the halfwidth kana before it and stays.
                    Some(n) if is_full_mark(n) && is_halfwidth_form(c) => {
                        out.push(compose(c, n).map(hiragana_of).unwrap_or(c).into());
                    }
                    _ => out.push(convert_one(c).into()),
                }
            }
            Kind::Katakana => {
                if matches!(c, 'う' | 'ウ') && next.is_some_and(|n| n == HALF_VOICED_MARK || n == FULL_VOICED_MARK) {
                    out.push("ヴ".into());
                    out.push(String::new());
                    i += 1;
                } else {
                    out.push(katakana_of(c).into());
                }
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(src: &str, kind: Kind) -> String {
        convert(src, kind).concat()
    }

    #[test]
    fn one_entry_per_source_char() {
        for kind in [Kind::HalfWidth, Kind::FullWidth, Kind::Hiragana, Kind::Katakana] {
            let src = "ｶﾞ ガ う゛ ﾜﾞ abc";
            assert_eq!(convert(src, kind).len(), src.chars().count(), "{kind:?}");
        }
    }

    // Each expectation below is Word for Mac's output for the same text (2026-10-18).

    #[test]
    fn half_width_as_word() {
        assert_eq!(run("Hello ＨＥＬＬＯ ｗｏｒｌｄ １２３", Kind::HalfWidth), "Hello HELLO world 123");
        assert_eq!(run("アイウエオ ガギグ ヴ ッ ー ・ 。、「」！？", Kind::HalfWidth), "ｱｲｳｴｵ ｶﾞｷﾞｸﾞ ｳﾞ ｯ ｰ ･ ｡､｢｣!?");
        assert_eq!(run("あいうえお ぱぴぷ 漢字 ヮ ヰヱ ヵヶ ヷヺ ヽヾ", Kind::HalfWidth), "あいうえお ぱぴぷ 漢字 ヮ ヰヱ ヵヶ ヷヺ ヽヾ");
        assert_eq!(run("！？，．：；＇＂（）［］｛｝＜＞＠＃＄％＾＆＊－＿＝＋／＼｜｀～", Kind::HalfWidth), "!?,.:;'\"()[]{}<>@#$%^&*-_=+/\\|`~");
        assert_eq!(run("￥ ¥ ￣ ‾ ｟｠ ￠￡￢￤￦", Kind::HalfWidth), "\\ ¥ ￣ ‾ ｟｠ ￠￡￢￤￦");
        assert_eq!(run("a\u{3000}b ゛゜ か゛ ガ゛", Kind::HalfWidth), "a b ﾞﾟ かﾞ ｶﾞﾞ");
        assert_eq!(run("ァィゥェォャュョ ｧ ‘’“” ① ㍻ café", Kind::HalfWidth), "ｧｨｩｪｫｬｭｮ ｧ ‘’“” ① ㍻ café");
    }

    #[test]
    fn full_width_as_word() {
        assert_eq!(run("Hello World 123", Kind::FullWidth), "Ｈｅｌｌｏ\u{3000}Ｗｏｒｌｄ\u{3000}１２３");
        assert_eq!(run("!?,.:;'\"()[]{}<>@#$%^&*-_=+/\\|`~", Kind::FullWidth), "！？，．：；＇＂（）［］｛｝＜＞＠＃＄％＾＆＊－＿＝＋／￥｜｀～");
        assert_eq!(
            run("ｱｲｳｴｵ ｶﾞｷﾞｸﾞ ﾊﾟ ｳﾞ ｯ ｰ ･ ｡､｢｣", Kind::FullWidth),
            "アイウエオ\u{3000}ガギグ\u{3000}パ\u{3000}ヴ\u{3000}ッ\u{3000}ー\u{3000}・\u{3000}。、「」"
        );
        assert_eq!(
            run("ﾜﾞ ｲﾟ ｰﾞ ﾞﾟ ｶﾞﾞ ｶ゛ カﾞ ぁﾞ ｶ\u{3099}", Kind::FullWidth),
            "ワ゛\u{3000}イ゜\u{3000}ー゛\u{3000}゛゜\u{3000}ガ゛\u{3000}カ゛\u{3000}カ゛\u{3000}ぁ゛\u{3000}カ\u{3099}"
        );
        assert_eq!(run("¥ ¢£¬¦₩ ﾡﾢ 가 é ‾ あ 漢", Kind::FullWidth), "¥\u{3000}¢£¬¦₩\u{3000}ﾡﾢ\u{3000}가\u{3000}é\u{3000}‾\u{3000}あ\u{3000}漢");
    }

    #[test]
    fn hiragana_as_word() {
        assert_eq!(run("アイウエオ ガギグ ヴ ッ ヽヾ ヵヶ ヷヺ ー ・", Kind::Hiragana), "あいうえお がぎぐ ゔ っ ゝゞ ヵヶ ヷヺ ー ・");
        assert_eq!(run("ｱｲｳｴｵ ｶﾞｷﾞｸﾞ ﾊﾟ ｳﾞ ｯ ｧ ｰ ･ ｡､｢｣ ﾝ", Kind::Hiragana), "あいうえお がぎぐ ぱ ゔ っ ぁ ー ・ 。、「」 ん");
        assert_eq!(run("Hello ＨＥＬＬＯ 123 漢字 あいう ゛゜", Kind::Hiragana), "Hello ＨＥＬＬＯ 123 漢字 あいう ゛゜");
        // Marks that can't attach: dropped after East Asian text, fullwidth elsewhere.
        assert_eq!(run("ﾜﾞ ｲﾟ ｰﾞ ﾞﾟ ｯﾞ ｡ﾞ 漢ﾞ 。ﾞ カﾞ ァﾞ ぁﾞ ヴﾞ ゛ﾞ", Kind::Hiragana), "ﾜ ｲ ｰ ﾞ ｯ ｡ 漢 。 か ぁ ぁ ゔ ゛");
        assert_eq!(run("aﾞ ﾞｶ ｶ ﾞ ｳﾞﾞ ｶﾞﾞ ｶﾞ゜", Kind::Hiragana), "a゛ ゛か か ゛ ゔ゛ が゛ が゜");
        // A fullwidth mark stays and still voices a halfwidth kana before it.
        assert_eq!(run("ｶ゛ ｳ゛ ﾊ゜ ｶ゜ ｰ゛ ｡゛ カ゛ う゛ ヽ゛ ァ゛", Kind::Hiragana), "が゛ ゔ゛ ぱ゜ ｶ゜ ｰ゛ ｡゛ か゛ う゛ ゝ゛ ぁ゛");
        assert_eq!(run("ｱ\u{3099} ｶ\u{309A}", Kind::Hiragana), "あ\u{3099} か\u{309A}");
    }

    #[test]
    fn katakana_as_word() {
        assert_eq!(run("あいうえお ぱぴぷ ゔ っ ゝゞ ゕゖ ゎ ゐゑ", Kind::Katakana), "アイウエオ パピプ ヴ ッ ヽヾ ゕゖ ヮ ヰヱ");
        assert_eq!(run("ｱｲｳｴｵ ｶﾞｷﾞｸﾞ ｯ ｰ ･ ｡､｢｣ ﾞﾟ", Kind::Katakana), "ｱｲｳｴｵ ｶﾞｷﾞｸﾞ ｯ ｰ ･ ｡､｢｣ ﾞﾟ");
        assert_eq!(run("Hello ＨＥＬＬＯ 123 漢字 アイウ ゛゜", Kind::Katakana), "Hello ＨＥＬＬＯ 123 漢字 アイウ ゛゜");
        assert_eq!(run("う゛ ウ゛ うﾞ ウﾞ ゔ゛ ゥ゛ う゜ か゛ かﾞ ぁﾞ ゝ゛", Kind::Katakana), "ヴ ヴ ヴ ヴ ヴ゛ ゥ゛ ウ゜ カ゛ カﾞ ァﾞ ヽ゛");
    }

    #[test]
    fn modes_parse() {
        assert_eq!(Kind::parse("halfWidth"), Some(Kind::HalfWidth));
        assert_eq!(Kind::parse("katakana"), Some(Kind::Katakana));
        assert_eq!(Kind::parse("upper"), None);
    }
}

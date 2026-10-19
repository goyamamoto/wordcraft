//! Asian typography line-breaking rules (the paragraph's `kinsoku` and `word_wrap` flags), applied
//! on top of the Unicode line-break opportunities (UAX #14).
//!
//! - Kinsoku on (the default): no line starts with closing East Asian punctuation (、。，）」…)
//!   and none ends with opening punctuation (（「【…). UAX #14 already forbids most of these
//!   breaks; the lists make sure of it for the standard Japanese/Chinese sets. Word's standard
//!   level lets a line start with a small kana or the prolonged sound mark (ッ, ー: `パカ|ッと`,
//!   `ラメシ|ール`, observed), which UAX #14 forbids, so those breaks are put back.
//! - Kinsoku off: a line may break between any two East Asian characters, punctuation included.
//! - Word wrap off: Latin words may break at any character.
//!
//! Only East Asian characters are in the lists, and the extra breaks need letters on both sides,
//! so text with no East Asian characters breaks exactly as before unless word wrap is off.

use unicode_segmentation::UnicodeSegmentation;

/// East Asian characters a line may not start with: closing brackets and quotes, the
/// ideographic comma and full stop, fullwidth and halfwidth punctuation, the middle dot,
/// iteration and sound marks.
const NO_START: &str = "、。，．・：；？！゛゜ヽヾゝゞ々〻〉》」』】〕〗〙〛〞〟）］｝｠｡｣､･ﾞﾟ％￠";

/// East Asian characters a line may not end with: opening brackets and quotes, currency signs.
const NO_END: &str = "〈《「『【〔〖〘〚〝（［｛｟｢＄￡￥";

/// Small kana and the prolonged sound mark (UAX #14 class CJ): a line may start with one after
/// East Asian text, as in Word's standard kinsoku level.
fn is_conditional_starter(c: char) -> bool {
    matches!(c as u32,
        0x3041 | 0x3043 | 0x3045 | 0x3047 | 0x3049 | 0x3063 | 0x3083 | 0x3085 | 0x3087 | 0x308E | 0x3095 | 0x3096
        | 0x30A1 | 0x30A3 | 0x30A5 | 0x30A7 | 0x30A9 | 0x30C3 | 0x30E3 | 0x30E5 | 0x30E7 | 0x30EE | 0x30F5 | 0x30F6
        | 0x30FC | 0x31F0..=0x31FF | 0xFF67..=0xFF6F | 0xFF70)
}

/// Whether `c` is East Asian text: CJK ideographs, kana, bopomofo, CJK punctuation, and
/// fullwidth/halfwidth forms (not spaces, not combining sound marks).
pub fn is_east_asian(c: char) -> bool {
    matches!(c as u32,
        0x2E80..=0x2FFF          // radicals, ideographic description
        | 0x3001..=0x303F        // CJK symbols and punctuation (not the ideographic space)
        | 0x3040..=0x3098        // hiragana
        | 0x309B..=0x30FF        // sound marks, katakana
        | 0x3100..=0x312F        // bopomofo
        | 0x3190..=0x31FF        // kanbun, CJK strokes, katakana extensions
        | 0x3400..=0x4DBF        // CJK extension A
        | 0x4E00..=0x9FFF        // CJK unified ideographs
        | 0xF900..=0xFAFF        // compatibility ideographs
        | 0xFE30..=0xFE4F        // CJK compatibility forms
        | 0xFF01..=0xFFEF        // fullwidth and halfwidth forms
        | 0x20000..=0x3FFFF) // supplementary ideographs
}

/// The document's kinsoku level (Word's Options › Typography): the standard lists, the high
/// level (`w:strictFirstAndLastChars`), or its own lists.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Level<'a> {
    /// Word's standard level: a line may start with a small kana or ー.
    #[default]
    Standard,
    /// Word's high level: it may not (Word for Mac, measured: いぁ, いー, いッ move on together).
    Strict,
    /// The document's own lists (characters a line may not start with, may not end with:
    /// `w:noLineBreaksBefore`/`After`), which replace the standard ones: a line may then break
    /// between any two East Asian characters not in them, as Word for Mac does (with
    /// `noLineBreaksBefore="あ"` a 。 starts a line and あ no longer does).
    Custom { no_start: &'a str, no_end: &'a str },
}

/// Adjust the break opportunities `opps` (byte offsets into `text`: a line may start there) for
/// the paragraph's kinsoku and word-wrap settings at the document's kinsoku `level`. The result
/// is sorted and has no duplicates.
pub fn apply(text: &str, opps: &mut Vec<usize>, kinsoku: bool, word_wrap: bool, level: Level) {
    let next_char = |i: usize| text.get(i..).and_then(|t| t.chars().next());
    let prev_char = |i: usize| text.get(..i).and_then(|t| t.chars().next_back());
    if let (true, Level::Custom { no_start, no_end }) = (kinsoku, level) {
        for (i, c) in text.char_indices().skip(1) {
            if is_east_asian(c) && prev_char(i).is_some_and(is_east_asian) {
                opps.push(i);
            }
        }
        opps.retain(|&i| !next_char(i).is_some_and(|c| no_start.contains(c)) && !prev_char(i).is_some_and(|c| no_end.contains(c)));
    } else if kinsoku {
        opps.retain(|&i| !next_char(i).is_some_and(|c| NO_START.contains(c)) && !prev_char(i).is_some_and(|c| NO_END.contains(c)));
        // Before an ellipsis after an East Asian letter (not after opening punctuation): Word's
        // lists at either level don't name … or ‥, so a line may start with them (Word for Mac:
        // 44 あ then …いろは wraps as …いろは), where UAX #14 keeps them with the letter.
        for (i, c) in text.char_indices().skip(1) {
            if matches!(c, '…' | '‥') && prev_char(i).is_some_and(|p| is_east_asian(p) && !NO_END.contains(p)) {
                opps.push(i);
            }
        }
        // Before a small kana or ー after an East Asian letter (not after opening punctuation),
        // at the standard level only.
        if level == Level::Standard {
            for (i, c) in text.char_indices().skip(1) {
                if is_conditional_starter(c) && prev_char(i).is_some_and(|p| is_east_asian(p) && !NO_END.contains(p)) {
                    opps.push(i);
                }
            }
        } else {
            opps.retain(|&i| !next_char(i).is_some_and(is_conditional_starter));
        }
    } else {
        // Between any two East Asian characters.
        for (i, c) in text.char_indices().skip(1) {
            if is_east_asian(c) && prev_char(i).is_some_and(is_east_asian) {
                opps.push(i);
            }
        }
    }
    if !word_wrap {
        // Between any two letters or digits that are not East Asian (whole graphemes, so accents
        // stay with their letters).
        let latin = |g: &str| g.chars().next().is_some_and(|c| c.is_alphanumeric() && !is_east_asian(c));
        let mut prev: Option<&str> = None;
        for (i, g) in text.grapheme_indices(true) {
            if prev.is_some_and(latin) && latin(g) {
                opps.push(i);
            }
            prev = Some(g);
        }
    }
    opps.sort_unstable();
    opps.dedup();
}

#[cfg(test)]
mod tests {
    use super::apply;

    /// Where a line may start, as the text from there to the next opportunity.
    fn breaks(text: &str, kinsoku: bool, word_wrap: bool) -> Vec<String> {
        let mut opps: Vec<usize> = unicode_linebreak::linebreaks(text).map(|(i, _)| i).filter(|&i| i < text.len()).collect();
        apply(text, &mut opps, kinsoku, word_wrap, super::Level::Standard);
        opps.iter().map(|&i| text.get(i..).and_then(|t| t.chars().next()).map(String::from).unwrap_or_default()).collect()
    }

    #[test]
    fn kinsoku_and_word_wrap_break_opportunities() {
        let ja = "日本語、「漢字」。テスト";
        let on = breaks(ja, true, true);
        for c in ["、", "」", "。", "漢"] {
            assert!(!on.iter().any(|s| s == c), "kinsoku: no line starts with {c}: {on:?}");
        }
        assert!(on.iter().any(|s| s == "「"), "a line may start at an opening bracket: {on:?}");
        assert!(on.iter().any(|s| s == "テ"), "{on:?}");
        let off = breaks(ja, false, true);
        for c in ["、", "」", "。", "漢"] {
            assert!(off.iter().any(|s| s == c), "kinsoku off: a line may start with {c}: {off:?}");
        }
        // Word's standard level: a line may start with a small kana or ー after a letter, never
        // after an opening bracket.
        let small = breaks("天井がパカッと開いた。ラメシールが「ッ」", true, true);
        assert!(small.iter().any(|s| s == "ッ") && small.iter().any(|s| s == "ー"), "{small:?}");
        assert_eq!(small.iter().filter(|s| *s == "ッ").count(), 1, "not after 「: {small:?}");
        // An ellipsis may start a line after a letter (Word's lists leave it out), not after 「.
        let dots = breaks("ああ…いい「…」‥う", true, true);
        assert_eq!(dots.iter().filter(|s| *s == "…").count(), 1, "{dots:?}");
        assert!(dots.iter().any(|s| s == "‥"), "{dots:?}");
        // Latin text: unchanged by kinsoku, broken anywhere without word wrap.
        let en = "Hello wonderful (world), 50%.";
        assert_eq!(breaks(en, true, true), breaks(en, false, true));
        assert_eq!(breaks(en, true, true), ["w", "(", "5"]);
        let any = breaks("Hello café", true, false);
        assert_eq!(any, ["e", "l", "l", "o", "c", "a", "f", "é"]);
    }

    /// Custom lists replace the standard ones (Word for Mac, measured with
    /// `noLineBreaksBefore="あ"`, `noLineBreaksAfter="「"`): 。 may start a line, あ may not, 「 may
    /// still not end one, and lines break between any other two East Asian characters.
    #[test]
    fn custom_kinsoku_lists_replace_the_standard_ones() {
        let text = "いあい。い「い";
        let mut opps: Vec<usize> = unicode_linebreak::linebreaks(text).map(|(i, _)| i).filter(|&i| i < text.len()).collect();
        apply(text, &mut opps, true, true, super::Level::Custom { no_start: "あ", no_end: "「" });
        let starts: Vec<char> = opps.iter().filter_map(|&i| text.get(i..).and_then(|t| t.chars().next())).collect();
        assert_eq!(starts, vec!['い', '。', 'い', '「'], "{opps:?}");
        // Kinsoku off ignores the lists too.
        let mut all: Vec<usize> = Vec::new();
        apply(text, &mut all, false, true, super::Level::Custom { no_start: "あ", no_end: "「" });
        assert_eq!(all.len(), text.chars().count() - 1);
    }

    /// Word's high level (`w:strictFirstAndLastChars`, measured in Word for Mac): a line may not
    /// start with a small kana or ー either; 々 and ゛ never may.
    #[test]
    fn the_strict_level_keeps_small_kana_off_the_line_start() {
        let text = "いぁいーいッい々い゛";
        let starts = |level: super::Level| -> Vec<char> {
            let mut opps: Vec<usize> = unicode_linebreak::linebreaks(text).map(|(i, _)| i).filter(|&i| i < text.len()).collect();
            apply(text, &mut opps, true, true, level);
            opps.iter().filter_map(|&i| text.get(i..).and_then(|t| t.chars().next())).collect()
        };
        assert_eq!(starts(super::Level::Standard), vec!['ぁ', 'い', 'ー', 'い', 'ッ', 'い', 'い']);
        assert_eq!(starts(super::Level::Strict), vec!['い', 'い', 'い', 'い']);
    }
}

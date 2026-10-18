//! East Asian text: which theme font Chinese, Japanese and Korean characters take, and the
//! document-wide punctuation compression (`w:characterSpacingControl`).
//!
//! A Word theme names one Latin font per slot (`a:latin`) and, for East Asian text, either one
//! font (`a:ea`, usually empty) or one per script (`a:font script="Jpan"`, `Hans`, `Hant`,
//! `Hang`). Measured in Word for Mac with the Japanese editing language: Japanese typed in an
//! Aptos paragraph is drawn in 游明朝 (the minor font's `Jpan` entry) and headings in
//! 游ゴシック Light, chosen by the text's East Asian language (`w:lang/@w:eastAsia`, in the
//! document defaults from `w:themeFontLang/@w:eastAsia`).

use serde::{Deserialize, Serialize};

/// A theme's fonts for one East Asian script (`a:font script="…"`).
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct ScriptFonts {
    /// ISO 15924 script code: `Jpan`, `Hans`, `Hant`, `Hang`.
    pub script: String,
    /// Heading (major) font; empty = none.
    pub major: String,
    /// Body (minor) font; empty = none.
    pub minor: String,
}

/// Word's `w:characterSpacingControl`: which full-width punctuation is compressed to half width
/// when it meets other punctuation (Word's Japanese Normal template chooses `compressPunctuation`,
/// the English one `doNotCompress`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum PunctCompression {
    #[default]
    DoNotCompress,
    CompressPunctuation,
    CompressPunctuationAndJapaneseKana,
}

impl PunctCompression {
    /// The OOXML attribute value.
    pub fn as_str(self) -> &'static str {
        match self {
            PunctCompression::DoNotCompress => "doNotCompress",
            PunctCompression::CompressPunctuation => "compressPunctuation",
            PunctCompression::CompressPunctuationAndJapaneseKana => "compressPunctuationAndJapaneseKana",
        }
    }
    /// The variant an OOXML attribute value names.
    pub fn parse(v: &str) -> Option<PunctCompression> {
        Some(match v {
            "doNotCompress" => PunctCompression::DoNotCompress,
            "compressPunctuation" => PunctCompression::CompressPunctuation,
            "compressPunctuationAndJapaneseKana" => PunctCompression::CompressPunctuationAndJapaneseKana,
            _ => return None,
        })
    }
    pub fn compresses(self) -> bool {
        self != PunctCompression::DoNotCompress
    }
}

/// The theme script (`Jpan`, `Hans`, `Hant`, `Hang`) an East Asian language tag selects:
/// `ja-JP` → `Jpan`, `zh-CN`/`zh-SG`/`zh-Hans` → `Hans`, `zh-TW`/`zh-HK`/`zh-MO`/`zh-Hant` →
/// `Hant`, `ko-KR` → `Hang`. `None` for other tags.
pub fn script_of_lang(tag: &str) -> Option<&'static str> {
    let tag = tag.trim();
    let mut parts = tag.split(['-', '_']);
    let lang = parts.next().unwrap_or_default().to_ascii_lowercase();
    let rest: Vec<String> = parts.map(str::to_ascii_lowercase).collect();
    Some(match lang.as_str() {
        "ja" => "Jpan",
        "ko" => "Hang",
        "zh" => {
            if rest.iter().any(|p| p == "hant" || p == "tw" || p == "hk" || p == "mo") {
                "Hant"
            } else {
                "Hans"
            }
        }
        _ => return None,
    })
}

/// Whether `c` is East Asian text as Word counts and formats it: CJK ideographs, kana, bopomofo,
/// CJK symbols and punctuation, fullwidth and halfwidth forms. Not Hangul (Korean words are
/// separated by spaces, and Word counts them so) and not the ideographic space.
pub fn is_east_asian(c: char) -> bool {
    matches!(u32::from(c),
        0x2E80..=0x2FFF          // radicals, ideographic description
        | 0x3001..=0x303F        // CJK symbols and punctuation
        | 0x3041..=0x30FF        // hiragana, katakana, sound marks
        | 0x3100..=0x312F        // bopomofo
        | 0x3190..=0x31FF        // kanbun, CJK strokes, katakana extensions
        | 0x3400..=0x4DBF        // CJK extension A
        | 0x4E00..=0x9FFF        // CJK unified ideographs
        | 0xF900..=0xFAFF        // compatibility ideographs
        | 0xFE30..=0xFE4F        // CJK compatibility forms
        | 0xFF01..=0xFF60        // fullwidth forms
        | 0xFF61..=0xFFEF        // halfwidth forms
        | 0x20000..=0x3FFFF) // supplementary ideographs
}

/// The script a character alone tells: kana → `Jpan`, Hangul → `Hang`, bopomofo → `Hant`.
/// `None` for Han ideographs and everything else (they belong to several scripts).
pub fn script_of_char(c: char) -> Option<&'static str> {
    Some(match u32::from(c) {
        0x3041..=0x309F | 0x30A0..=0x30FF | 0x31F0..=0x31FF | 0xFF66..=0xFF9F | 0x1B000..=0x1B16F => "Jpan",
        0x1100..=0x11FF | 0x3130..=0x318F | 0xA960..=0xA97F | 0xAC00..=0xD7FF => "Hang",
        0x3100..=0x312F | 0x31A0..=0x31BF => "Hant",
        _ => return None,
    })
}

/// The theme font for East Asian `text` in a run of language `lang_ea` (its
/// `w:lang/@w:eastAsia`, else the document's `theme_font_lang_ea`): the slot's `a:ea` font when
/// the theme names one, else the font of the script the language selects, else — for text whose
/// characters settle it (kana, Hangul, bopomofo) — that script's font. `None` when the theme has
/// nothing to say (Han-only text with no East Asian language stays with the font fallback).
pub fn theme_font<'a>(
    ea_font: &'a str,
    script_fonts: &'a [ScriptFonts],
    major: bool,
    lang_ea: Option<&str>,
    doc_lang_ea: Option<&str>,
    text: &str,
) -> Option<&'a str> {
    if !ea_font.is_empty() {
        return Some(ea_font);
    }
    let script = lang_ea
        .and_then(script_of_lang)
        .or_else(|| doc_lang_ea.and_then(script_of_lang))
        .or_else(|| text.chars().take(4096).find_map(script_of_char))?;
    script_fonts.iter().find(|s| s.script == script).map(|s| if major { s.major.as_str() } else { s.minor.as_str() }).filter(|f| !f.is_empty())
}

/// Word's own Office theme fonts per East Asian script (the names its themes carry; each is
/// drawn with an installed or metric-compatible open substitute when the font isn't installed).
pub fn office_script_fonts() -> Vec<ScriptFonts> {
    [("Jpan", "游ゴシック Light", "游明朝"), ("Hang", "맑은 고딕", "맑은 고딕"), ("Hans", "等线 Light", "等线"), ("Hant", "新細明體", "新細明體")]
        .into_iter()
        .map(|(script, major, minor)| ScriptFonts { script: script.into(), major: major.into(), minor: minor.into() })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn language_and_characters_select_the_script() {
        assert_eq!(script_of_lang("ja-JP"), Some("Jpan"));
        assert_eq!(script_of_lang("ja"), Some("Jpan"));
        assert_eq!(script_of_lang("zh-CN"), Some("Hans"));
        assert_eq!(script_of_lang("zh-Hans-SG"), Some("Hans"));
        assert_eq!(script_of_lang("zh-TW"), Some("Hant"));
        assert_eq!(script_of_lang("zh_HK"), Some("Hant"));
        assert_eq!(script_of_lang("ko-KR"), Some("Hang"));
        assert_eq!(script_of_lang("en-US"), None);
        assert_eq!(script_of_lang(""), None);
        assert_eq!(script_of_char('あ'), Some("Jpan"));
        assert_eq!(script_of_char('ｶ'), Some("Jpan"));
        assert_eq!(script_of_char('한'), Some("Hang"));
        assert_eq!(script_of_char('ㄅ'), Some("Hant"));
        assert_eq!(script_of_char('漢'), None);
        assert_eq!(script_of_char('a'), None);
    }

    #[test]
    fn theme_font_follows_the_language_then_the_characters() {
        let fonts = office_script_fonts();
        // Japanese text in a Japanese document: the minor Jpan font; headings the major one.
        assert_eq!(theme_font("", &fonts, false, Some("ja-JP"), None, "漢字"), Some("游明朝"));
        assert_eq!(theme_font("", &fonts, true, Some("ja-JP"), None, "漢字"), Some("游ゴシック Light"));
        // No run language: the document's theme font language, else the characters.
        assert_eq!(theme_font("", &fonts, false, None, Some("zh-CN"), "漢字"), Some("等线"));
        assert_eq!(theme_font("", &fonts, false, None, None, "かな"), Some("游明朝"));
        assert_eq!(theme_font("", &fonts, false, None, None, "한글"), Some("맑은 고딕"));
        // Han only, nothing else: the theme has nothing to say.
        assert_eq!(theme_font("", &fonts, false, None, None, "漢字"), None);
        assert_eq!(theme_font("", &fonts, false, Some("en-US"), None, "漢字"), None);
        // A theme with one East Asian font names it for every script.
        assert_eq!(theme_font("SimSun", &fonts, false, Some("ja-JP"), None, "かな"), Some("SimSun"));
        // An unknown script in the language, or a script the theme lacks.
        assert_eq!(theme_font("", &fonts[..1], false, Some("ko-KR"), None, "한글"), None);
        assert_eq!(PunctCompression::parse("compressPunctuation"), Some(PunctCompression::CompressPunctuation));
        assert_eq!(PunctCompression::parse("x"), None);
        assert_eq!(PunctCompression::CompressPunctuationAndJapaneseKana.as_str(), "compressPunctuationAndJapaneseKana");
    }
}

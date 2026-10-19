//! Japanese word breaking as Word for Mac breaks words (a double-click's word in Japanese text).
//!
//! Word for Mac segments Japanese with a dictionary: 日本語|の|文章|を|書いて|います|。 — a verb
//! keeps its endings (書いて, 行きました, 飲みながら, 走り回って), an auxiliary verb after て starts a
//! word of its own (います, いる, ください), a noun keeps its suffixes and counters (東京都,
//! 会議室, 十五分, 三年前) and its prefixes (ご意見, お待たせ, 再起動, 第三条), particles stand
//! alone (を, は, ので, らしい). Word's `words` collection was read for 95 sentences of our own
//! with Japanese editing (2026-10-20).
//!
//! The words come from a morphological analysis — [lindera] with the IPADIC dictionary, read at
//! run time from a folder ([`find_dictionary`]; not built in, so without it there are no
//! Japanese words and the caller keeps its own rule) — whose morphemes are merged back into
//! Word's words by [`merge`]. On the 95 sentences the merged words match Word's in 92 (26 of
//! the 30 held out from writing the rules before their last fixes), its word boundaries to
//! 99.7 % (precision) and 99 % (recall); what is left is the dictionaries' difference
//! (電話番号 is one word for Word, two for IPADIC; 道のり the other way round).
//!
//! [lindera]: https://github.com/lindera/lindera
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

/// A morpheme: its text and the IPADIC features the merge rules read.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Morpheme {
    pub surface: String,
    /// 品詞 and its first two subcategories (`名詞`, `接尾`, `助数詞`).
    pub pos: String,
    pub pos1: String,
    pub pos2: String,
    /// 活用形 (`連用形`, `連用タ接続`, `ガル接続`…).
    pub form: String,
    /// 基本形 (`する`, `らしい`…).
    pub base: String,
}

impl Morpheme {
    /// A morpheme from its surface and IPADIC's feature list (品詞, 細分類1–3, 活用型, 活用形,
    /// 基本形, 読み, 発音); missing features read as `*`.
    pub fn from_features(surface: &str, features: &[&str]) -> Morpheme {
        let f = |i: usize| features.get(i).copied().unwrap_or("*").to_string();
        Morpheme { surface: surface.to_string(), pos: f(0), pos1: f(1), pos2: f(2), form: f(5), base: f(6) }
    }
}

/// Dictionary words Word for Mac splits: (IPADIC's word, Word's parts).
const SPLIT: &[(&str, &[&str])] = &[("株式会社", &["株式", "会社"]), ("お客様", &["お客", "様"])];

/// The conjunctive particles that stay with the verb or adjective before them (書いて, 飲みながら,
/// しなければ); ので, から, けれど start words of their own.
const CLINGING_PARTICLES: &[&str] = &["て", "で", "ながら", "ば", "たり", "だり", "つつ"];

fn is_inflecting(m: &Morpheme) -> bool {
    matches!(m.pos.as_str(), "動詞" | "形容詞" | "助動詞")
}

/// Whether morpheme `t` joins the word `group` (non-empty) ends.
fn joins(group: &[Morpheme], t: &Morpheme) -> bool {
    let Some(prev) = group.last() else { return false };
    let inflecting = || group.iter().any(is_inflecting);
    // A prefix always goes with what follows it (ご意見, お待たせ, 再起動, 第三条).
    if prev.pos == "接頭詞" {
        return true;
    }
    match (t.pos.as_str(), t.pos1.as_str()) {
        // Auxiliary verbs stay with the verb (行きました, 遅れた, いなかった), not らしい, nor
        // what follows そう (いたそう|です).
        ("助動詞", _) => t.base != "らしい" && prev.pos2 != "助動詞語幹" && inflecting(),
        ("助詞", "接続助詞") => CLINGING_PARTICLES.contains(&t.surface.as_str()) && is_inflecting(prev),
        // れ, せ, られ go with their verb (書かれて, 聞かせて); a verb right after another's
        // continuative form makes a compound (走り回って, 降り出した), not する (お待たせ|して)
        // and not after て (書いて|います); すぎる after an adjective stem too (甘すぎて).
        ("動詞", "接尾") => matches!(prev.pos.as_str(), "動詞" | "助動詞"),
        ("動詞", _) => {
            (prev.pos == "動詞" && prev.form.starts_with("連用") && !matches!(prev.surface.as_str(), "て" | "で") && t.base != "する")
                || (prev.pos == "形容詞" && prev.form == "ガル接続" && t.base == "すぎる")
        }
        // そう after a past or an adjective (いたそう, よさそう); よう stands alone (できる|よう).
        ("名詞", _) if t.pos2 == "助動詞語幹" => {
            t.surface == "そう" && (prev.pos == "助動詞" || prev.pos == "形容詞" || (prev.pos1 == "接尾" && prev.surface == "さ"))
        }
        ("名詞", "接尾") => match t.pos2.as_str() {
            "特殊" if t.surface == "さ" => matches!(prev.pos.as_str(), "形容詞" | "名詞"),
            "助数詞" => prev.pos1 == "数",
            _ => match t.surface.as_str() {
                // 私たち, 彼ら; 子ども|たち.
                "たち" | "ら" => prev.pos1 == "代名詞",
                "さん" | "様" | "君" | "ちゃん" => false,
                // 発売|日, ご購入|日.
                "日" if prev.pos1 == "サ変接続" => false,
                _ => prev.pos == "名詞",
            },
        },
        // Numbers run together (十五, 二〇二六); 前, 後, 間 after a count (三年前, 一年間).
        ("名詞", "数") => prev.pos1 == "数",
        ("名詞", "副詞可能") => matches!(t.surface.as_str(), "前" | "後" | "間") && group.iter().any(|m| m.pos1 == "数"),
        ("形容詞", "非自立") => matches!(prev.pos.as_str(), "動詞" | "形容詞"),
        _ => false,
    }
}

/// IPADIC's morphemes where Word's words split them: [`SPLIT`]'s words, and an adverb ending in
/// に of three or more characters (すで|に, 本当|に).
fn split_morphemes(morphemes: &[Morpheme]) -> Vec<Morpheme> {
    let mut out = Vec::with_capacity(morphemes.len());
    for m in morphemes {
        if let Some((_, parts)) = SPLIT.iter().find(|(w, _)| *w == m.surface) {
            out.extend(parts.iter().map(|p| Morpheme { surface: (*p).to_string(), pos: "名詞".into(), pos1: "一般".into(), ..Default::default() }));
        } else if m.pos == "副詞" && m.surface.chars().count() >= 3 && m.surface.ends_with('に') {
            let stem = m.surface.trim_end_matches('に');
            out.push(Morpheme { surface: stem.to_string(), pos: "名詞".into(), pos1: "一般".into(), ..Default::default() });
            out.push(Morpheme { surface: "に".into(), pos: "助詞".into(), pos1: "格助詞".into(), ..Default::default() });
        } else {
            out.push(m.clone());
        }
    }
    out
}

/// Word's words from a sentence's morphemes, as their texts.
pub fn merge(morphemes: &[Morpheme]) -> Vec<String> {
    let mut words = Vec::new();
    let mut group: Vec<Morpheme> = Vec::new();
    for m in split_morphemes(morphemes) {
        if !group.is_empty() && joins(&group, &m) {
            group.push(m);
        } else {
            if !group.is_empty() {
                words.push(group.iter().map(|g| g.surface.as_str()).collect());
            }
            group = vec![m];
        }
    }
    if !group.is_empty() {
        words.push(group.iter().map(|g| g.surface.as_str()).collect());
    }
    words
}

/// The byte offsets in `text` where its words end, as Word for Mac breaks them, or `None` without
/// a dictionary (see [`find_dictionary`]) or when the analysis fails. The words cover `text`
/// whole, its spaces included.
pub fn word_ends(text: &str) -> Option<Vec<usize>> {
    #[cfg(not(target_arch = "wasm32"))]
    {
        native::word_ends(text)
    }
    #[cfg(target_arch = "wasm32")]
    {
        let _ = text;
        None
    }
}

/// Where the dictionary is looked for, first found wins: `WORDCRAFT_JA_DICT`; beside the program
/// (`../Resources/dict/lindera-ipadic` in a macOS app bundle, `../share/wordcraft/dict/
/// lindera-ipadic`, `dict/lindera-ipadic`); the user's data folder (`~/Library/Application
/// Support/WordCraft/dict/lindera-ipadic` on macOS, `$XDG_DATA_HOME` or `~/.local/share`
/// `/wordcraft/dict/lindera-ipadic` elsewhere). A folder counts when it holds `dict.da` or
/// `dict.trie` (a compiled lindera dictionary).
pub fn find_dictionary() -> Option<std::path::PathBuf> {
    #[cfg(not(target_arch = "wasm32"))]
    {
        native::find_dictionary()
    }
    #[cfg(target_arch = "wasm32")]
    {
        None
    }
}

#[cfg(not(target_arch = "wasm32"))]
mod native {
    use std::path::{Path, PathBuf};
    use std::sync::OnceLock;

    use lindera::dictionary::load_dictionary;
    use lindera::mode::Mode;
    use lindera::segmenter::Segmenter;

    use super::{Morpheme, merge};

    /// Longest text analysed at once (bytes): a sentence is far shorter; this bounds the work a
    /// hostile paragraph can cause.
    const MAX_TEXT: usize = 64 * 1024;

    fn is_dictionary(dir: &Path) -> bool {
        dir.join("dict.trie").is_file() || dir.join("dict.da").is_file()
    }

    pub(super) fn find_dictionary() -> Option<PathBuf> {
        const SUB: &str = "dict/lindera-ipadic";
        let mut candidates: Vec<PathBuf> = Vec::new();
        if let Some(p) = std::env::var_os("WORDCRAFT_JA_DICT") {
            candidates.push(PathBuf::from(p));
        }
        if let Ok(exe) = std::env::current_exe()
            && let Some(dir) = exe.parent()
        {
            candidates.push(dir.join("../Resources").join(SUB));
            candidates.push(dir.join("../share/wordcraft").join(SUB));
            candidates.push(dir.join(SUB));
        }
        if let Some(home) = std::env::var_os("HOME").map(PathBuf::from) {
            if cfg!(target_os = "macos") {
                candidates.push(home.join("Library/Application Support/WordCraft").join(SUB));
            }
            let data = std::env::var_os("XDG_DATA_HOME").map(PathBuf::from).unwrap_or_else(|| home.join(".local/share"));
            candidates.push(data.join("wordcraft").join(SUB));
        }
        candidates.into_iter().find(|p| is_dictionary(p))
    }

    /// The analyser, loaded the first time it is needed (`None` when no dictionary loads).
    fn segmenter() -> Option<&'static Segmenter> {
        static SEG: OnceLock<Option<Segmenter>> = OnceLock::new();
        SEG.get_or_init(|| {
            let dir = find_dictionary()?;
            let dict = match load_dictionary(&dir.to_string_lossy()) {
                Ok(d) => d,
                Err(e) => {
                    log::warn!("Japanese dictionary {} didn't load: {e}", dir.display());
                    return None;
                }
            };
            Some(Segmenter::new(Mode::Normal, dict, None))
        })
        .as_ref()
    }

    pub(super) fn word_ends(text: &str) -> Option<Vec<usize>> {
        if text.len() > MAX_TEXT {
            return None;
        }
        let seg = segmenter()?;
        let mut tokens = seg.segment(std::borrow::Cow::Borrowed(text)).ok()?;
        let morphemes: Vec<Morpheme> = tokens
            .iter_mut()
            .map(|t| {
                let surface = t.surface.as_ref().to_string();
                let features = t.details();
                Morpheme::from_features(&surface, &features)
            })
            .collect();
        // The analysis leaves out spaces: the words' texts are found in order in `text`, and what
        // lies between two words (spaces) ends with the word before it.
        let mut ends = Vec::new();
        let mut at = 0usize;
        for w in merge(&morphemes) {
            let rest = text.get(at..)?;
            let found = rest.find(w.as_str())?;
            let end = at + found + w.len();
            if found > 0
                && let Some(last) = ends.last_mut()
            {
                *last = at + found;
            }
            ends.push(end);
            at = end;
        }
        if let Some(last) = ends.last_mut() {
            *last = text.len();
        }
        Some(ends)
    }
}

#[cfg(test)]
mod tests;

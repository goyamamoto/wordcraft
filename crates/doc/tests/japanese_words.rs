//! A double-click in Japanese text selects the word the installed breaker finds (Word for Mac's
//! word breaker: `wordcraft-jaseg`). Its own test binary: the breaker is installed process-wide.

use wordcraft_doc::{Paragraph, set_japanese_word_breaker};

/// A stand-in breaker: 日本語|の|文章|を|書いて|います|。 for that sentence, nothing otherwise.
fn breaker(text: &str) -> Option<Vec<usize>> {
    let words = ["日本語", "の", "文章", "を", "書いて", "います", "。"];
    (text == words.concat()).then(|| {
        let mut at = 0;
        words
            .iter()
            .map(|w| {
                at += w.len();
                at
            })
            .collect()
    })
}

#[test]
fn a_double_click_in_japanese_selects_the_breakers_word() {
    let p = Paragraph::with_text("abc 日本語の文章を書いています。", Default::default());
    let at = |s: &str| p.text.find(s).unwrap();
    // Without a breaker: the run of word characters.
    let (a, b) = p.word_at(at("文章"));
    assert_eq!(&p.text[a..b], "日本語の文章を書いています");
    set_japanese_word_breaker(breaker);
    // The sentence is "abc 日本語の文章を書いています。" — the stand-in knows only the Japanese
    // part, so a paragraph of that alone is used below.
    let p = Paragraph::with_text("日本語の文章を書いています。", Default::default());
    let at = |s: &str| p.text.find(s).unwrap();
    let word = |off: usize| {
        let (a, b) = p.word_at(off);
        p.text[a..b].to_string()
    };
    assert_eq!(word(at("文章")), "文章");
    assert_eq!(word(at("章")), "文章");
    assert_eq!(word(at("書")), "書いて");
    assert_eq!(word(at("ます")), "います");
    assert_eq!(word(0), "日本語");
    assert_eq!(word(at("。")), "。");
    // Latin text and text the breaker doesn't know keep the old rule.
    let q = Paragraph::with_text("hello world", Default::default());
    assert_eq!(q.word_at(1), (0, 6));
    let r = Paragraph::with_text("知らない文", Default::default());
    let (a, b) = r.word_at(0);
    assert_eq!(&r.text[a..b], "知らない文");
}

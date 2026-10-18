//! Word's Combine Characters (組み文字) as a file structure: an `EQ` field,
//! `eq \o(\s\up 5(株式),\s\do 2(会社))`, whose two text pieces sit in half-size runs. WordCraft
//! keeps the text with `EastAsianLayout::combine_chars`; the reader folds Word's field into that
//! and the writer puts the field back (observed in Word for Mac, 2026-10-18).

/// The two half lines of `text` the way Word splits it: half the characters above (rounded
/// down: 東京/都千代, 東京都/千代田区), except three, which go two over one (東京/都); two
/// characters are padded with an ideographic space on their outer sides (`平　` over `　成`) so
/// the block stays one character wide.
pub fn split(text: &str) -> (String, String) {
    let chars: Vec<char> = text.chars().collect();
    let n = chars.len();
    let up = if n == 3 { 2 } else { n / 2 };
    let (a, b) = chars.split_at(up.min(n));
    let (mut a, mut b): (String, String) = (a.iter().collect(), b.iter().collect());
    if n == 2 {
        a.push('\u{3000}');
        b.insert(0, '\u{3000}');
    }
    (a, b)
}

/// Word's character scale (`w:w`) for the half runs: 100 for up to two characters per line,
/// 66 for three, 50 for four.
pub fn scale(text: &str) -> Option<f32> {
    match text.chars().count().div_ceil(2) {
        0..=2 => None,
        3 => Some(66.0),
        _ => Some(50.0),
    }
}

/// Word's `\s\up` offset in points for a base font size: half of it, rounded.
pub fn raise(size: f32) -> i32 {
    (size / 2.0).round().clamp(1.0, 999.0) as i32
}

/// The half size of the pieces: half the font size, in whole half-points (10.5 → 5, 12 → 6).
pub fn half_size(size: f32) -> f32 {
    (size.floor() / 2.0).max(0.5)
}

/// The text of a Combine Characters field, if `instr` is one:
/// `eq \o(\s\up N(A),\s\do N(B))` (any spacing, `\up`/`\do` with or without a space before the
/// number). Word's two-character padding (a trailing and a leading ideographic space) is
/// removed. Anything else, including other `EQ` fields, is `None`.
pub fn parse(instr: &str) -> Option<String> {
    let s = instr.trim();
    let rest = s.get(..2).filter(|k| k.eq_ignore_ascii_case("eq")).and_then(|_| s.get(2..))?.trim_start();
    let rest = rest.strip_prefix("\\o")?.trim_start().strip_prefix('(')?;
    let (a, rest) = piece(rest, "up")?;
    let rest = rest.trim_start().strip_prefix(',')?;
    let (b, rest) = piece(rest, "do")?;
    if rest.trim() != ")" {
        return None;
    }
    let (mut a, mut b) = (a.to_string(), b.to_string());
    let visible = |t: &str| t.chars().filter(|c| *c != '\u{3000}').count();
    if visible(&a) == 1 && visible(&b) == 1 && a.ends_with('\u{3000}') && b.starts_with('\u{3000}') {
        a.pop();
        b.remove(0);
    }
    if a.is_empty() && b.is_empty() {
        return None;
    }
    Some(a + &b)
}

/// `\s\up 5(text)` → ("text", what follows the closing bracket).
fn piece<'a>(s: &'a str, dir: &str) -> Option<(&'a str, &'a str)> {
    let s = s.trim_start().strip_prefix("\\s")?.trim_start().strip_prefix('\\')?;
    let s = s.get(..2).filter(|d| d.eq_ignore_ascii_case(dir)).and_then(|_| s.get(2..))?.trim_start();
    let digits = s.chars().take_while(char::is_ascii_digit).count();
    if digits == 0 {
        return None;
    }
    let s = s.get(digits..)?.trim_start().strip_prefix('(')?;
    let close = s.find(')')?;
    let text = s.get(..close)?;
    if text.contains('(') {
        return None;
    }
    Some((text, s.get(close + 1..)?))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn splits_as_word() {
        assert_eq!(split("株式会社"), ("株式".into(), "会社".into()));
        assert_eq!(split("東京都千代田"), ("東京都".into(), "千代田".into()));
        assert_eq!(split("東京都千代"), ("東京".into(), "都千代".into()));
        assert_eq!(split("東京都"), ("東京".into(), "都".into()));
        assert_eq!(split("平成"), ("平\u{3000}".into(), "\u{3000}成".into()));
        assert_eq!(split("東京都千代田区"), ("東京都".into(), "千代田区".into()));
        assert_eq!((scale("株式会社"), scale("東京都千代"), scale("東京都千代田区")), (None, Some(66.0), Some(50.0)));
        assert_eq!((raise(10.5), raise(12.0), half_size(10.5), half_size(12.0)), (5, 6, 5.0, 6.0));
    }

    #[test]
    fn parses_words_field_code() {
        assert_eq!(parse("eq \\o(\\s\\up 5(株式),\\s\\do 2(会社))").as_deref(), Some("株式会社"));
        assert_eq!(parse(" EQ \\o( \\s\\up5(abc) , \\s\\do2(def) ) ").as_deref(), Some("abcdef"));
        assert_eq!(parse("eq \\o(\\s\\up 5(平\u{3000}),\\s\\do 2(\u{3000}成))").as_deref(), Some("平成"));
        assert_eq!(parse("eq \\o(=,/)"), None);
        assert_eq!(parse("eq \\o\\ac(○,字)"), None);
        assert_eq!(parse("eq \\o(\\s\\up 5(a),\\s\\do 2(b)) extra"), None);
        assert_eq!(parse("PAGE"), None);
    }
}

//! Table formulas (Table Layout › Data › Formula): `=SUM(ABOVE)`, cell references and ranges
//! (`=SUM(B2:B4)`, `=A1*2`), arithmetic, and the functions Word offers in tables, worked out as
//! Word for Mac works them out (its window observed with Japanese editing: `fw1`–`fw7` probes of
//! `=` fields in tables and in the body). A formula field is re-evaluated by Update Field (F9),
//! so its result follows the cells it reads; a formula in the body has no cells to read.
//!
//! Cell references count rows from 1 at the top and columns from A at the left, by grid column.
//! ABOVE, LEFT, RIGHT and BELOW take the first run of numbers in that direction: the cells next
//! to the formula's cell are skipped while they hold no number (empty or text), then the numbers
//! are taken up to the next cell that holds none. A cell holds a number for that walk when its
//! text is digits with separators and a percent sign, a sign and the locale's currency sign at
//! most (`1,2`, `5%`, `- 5`, `¥5`, `１２`; `(5)`, `1 2` and `12abc` are text); a cell holding a
//! formula counts by its result. A cell referred to by name (`=A1`) is read loosely, as Word
//! reads it: the leading numbers and operators of its text (`12abc` 12, `1 2` 3, `12-3` 9,
//! `3/4` 0.75, `(5)` −5, `5%` 0.05), anything unreadable 0.
//!
//! What a formula shows follows Word: a result keeps the decimals its operands were written with
//! (`0.1+0.2` 0.3, `1.5*1.5` 2.3, `2^0.500` 1.414), a division two at least (`1/3` 0.33), a negative
//! power eight (`2^-10` 0.00097656), trailing zeros dropped (`10/4` 2.5, a zero `0.0`), fourteen
//! significant digits (`2^62` 4611686018427400000), and the formats of the cells and literals it
//! read: thousands separators (`1,000*2` 2,000), the locale's currency (`¥5+1` ¥6.00, a negative
//! in parentheses) and a parenthesised negative (`(5)*2` (10)). Its errors are Word's texts:
//! `!Zero Divide`, `!Syntax Error, )`, `!Undefined Bookmark, XYZ`, `!Missing Operator`,
//! `!Unexpected End of Formula`, `!The Formula Not In Table` …

use wordcraft_doc::field_format::{FieldNumFormat, Switch, UNKNOWN_SWITCH};
use wordcraft_doc::locale_number::decimal_separator;
use wordcraft_doc::table::{MAX_COLS, MAX_ROWS};
use wordcraft_doc::{Cell, Table};

use crate::CmdError;

/// The longest formula accepted, in bytes.
pub const MAX_FORMULA: usize = 512;
/// The longest numeric picture (`\#` switch) used, in characters.
const MAX_PICTURE: usize = 64;
/// The deepest nesting of parentheses, function calls, unary signs and `^` chains.
const MAX_DEPTH: usize = 32;
/// The most cells one evaluation reads; past it the formula is too complex to work out, so a
/// hostile document full of huge ranges can't make Update Field hang.
pub const MAX_CELL_VISITS: usize = 100_000;
/// The significant digits a result shows (Word for Mac: `2^62` is 4611686018427400000).
const SIGNIFICANT: usize = 14;
/// The most decimals a result shows.
const MAX_DECIMALS: u8 = 30;
/// A result with more digits before the decimal point than this shows `!Number Too Large To
/// Format` (Word for Mac shows `10^60`, 61 digits, and not `10^65`).
const MAX_INT_DIGITS: usize = 64;
/// Two values compare equal when they differ by less than this share of the larger (Word for
/// Mac: `0.1+0.2=0.3` and `1/3=0.333333333333333` hold, `1/3=0.33333333333333` doesn't).
const COMPARE_TOLERANCE: f64 = 2e-15;

/// What Word shows for a formula it can't work out.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FormulaError {
    /// A division by zero (`1/0`, `0^-1`).
    ZeroDivide,
    /// The formula stops short (`1 +`, `(1+2`, `ABS(`).
    UnexpectedEnd,
    /// Two values with nothing between them (`1 2`, `2(3)`).
    MissingOperator,
    /// Something unexpected: the token Word reports.
    Syntax(String),
    /// A name that is neither a function, a cell nor a bookmark.
    UndefinedBookmark(String),
    /// Cells named in a formula outside a table.
    NotInTable,
    /// A cell beyond the table's rows or columns.
    CellNotInTable(String),
    /// ABOVE in the top row, LEFT in the first column.
    TableIndexZero,
    /// A negative number raised to a fractional power.
    ExponentNotInteger,
    /// A result with too many digits to show.
    TooLarge,
    /// `MOD` by zero: Word shows nothing at all.
    Blank,
}

impl FormulaError {
    /// The field result Word shows.
    pub fn text(&self) -> String {
        match self {
            FormulaError::ZeroDivide => "!Zero Divide".into(),
            FormulaError::UnexpectedEnd => "!Unexpected End of Formula".into(),
            FormulaError::MissingOperator => "!Missing Operator".into(),
            FormulaError::Syntax(t) => format!("!Syntax Error, {t}"),
            FormulaError::UndefinedBookmark(n) => format!("!Undefined Bookmark, {n}"),
            FormulaError::NotInTable => "!The Formula Not In Table".into(),
            FormulaError::CellNotInTable(n) => format!("!{n} Is Not In Table"),
            FormulaError::TableIndexZero => "!Table Index Cannot be Zero".into(),
            FormulaError::ExponentNotInteger => "!Exponent Not an Integer".into(),
            FormulaError::TooLarge => "!Number Too Large To Format".into(),
            FormulaError::Blank => String::new(),
        }
    }
}

/// Why an evaluation stopped: a result Word shows, or a limit WordCraft enforces.
#[derive(Debug)]
enum Fail {
    Word(FormulaError),
    Hard(CmdError),
}

impl From<FormulaError> for Fail {
    fn from(e: FormulaError) -> Fail {
        Fail::Word(e)
    }
}

impl From<CmdError> for Fail {
    fn from(e: CmdError) -> Fail {
        Fail::Hard(e)
    }
}

/// How a value is shown: what the cells and literals it came from were written with.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct Fmt {
    /// Thousands separators (`1,000`).
    thousands: bool,
    /// A negative in parentheses (`(5)`).
    parens: bool,
    /// The locale's currency, two decimals, a negative in parentheses (`¥5.00`).
    currency: bool,
}

impl Fmt {
    fn or(self, o: Fmt) -> Fmt {
        Fmt { thousands: self.thousands || o.thousands, parens: self.parens || o.parens, currency: self.currency || o.currency }
    }
}

/// A value with the decimals it shows and the format it inherited.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Num {
    v: f64,
    d: u8,
    fmt: Fmt,
}

impl Num {
    fn new(v: f64) -> Num {
        Num { v, d: 0, fmt: Fmt::default() }
    }

    fn with(v: f64, d: u8, fmt: Fmt) -> Num {
        Num { v, d: d.min(MAX_DECIMALS), fmt }
    }

    fn join(v: f64, d: u8, a: Num, b: Num) -> Num {
        Num::with(v, d, a.fmt.or(b.fmt))
    }

    fn neg(self) -> Num {
        Num { v: -self.v, ..self }
    }

    fn percent(self) -> Num {
        Num::with(self.v / 100.0, self.d.saturating_add(2), self.fmt)
    }

    /// `a op b` with Word's decimals: the most of the two, a division at least two, a negative
    /// power eight.
    fn bin(op: char, a: Num, b: Num) -> Result<Num, FormulaError> {
        let d = a.d.max(b.d);
        Ok(match op {
            '+' => Num::join(a.v + b.v, d, a, b),
            '-' => Num::join(a.v - b.v, d, a, b),
            '*' => Num::join(a.v * b.v, d, a, b),
            '/' => {
                if b.v == 0.0 {
                    return Err(FormulaError::ZeroDivide);
                }
                Num::join(a.v / b.v, d.max(2), a, b)
            }
            _ => {
                if a.v < 0.0 && b.v.fract() != 0.0 {
                    return Err(FormulaError::ExponentNotInteger);
                }
                if a.v == 0.0 && b.v < 0.0 {
                    return Err(FormulaError::ZeroDivide);
                }
                let d = if b.v < 0.0 { 8 } else { d };
                Num::join(a.v.powf(b.v), d, a, b)
            }
        })
    }
}

/// The separators and currency sign the text's language writes numbers with.
#[derive(Clone, Copy, Debug)]
struct Locale {
    dec: char,
    /// The separator a result groups thousands with.
    group: char,
    /// Separators accepted between groups of three digits.
    groups: &'static [char],
    /// The sign that marks a value as currency (`¥` with Japanese text).
    currency: &'static [char],
    /// The separator between a function's arguments.
    list: char,
}

/// Currency signs read and skipped in front of a number.
const CURRENCY_SIGNS: &[char] = &['$', '€', '£', '¥', '￥', '¢', '₽', '₴', '₺', '₹', '₩', '₪', '₫', '฿', '₱', '₸', '₼', '₾'];

fn locale(lang: Option<&str>) -> Locale {
    let tag = lang.unwrap_or("").trim().to_ascii_lowercase().replace('_', "-");
    let language = tag.split('-').next().unwrap_or("");
    let dec = decimal_separator(lang);
    let space_groups =
        matches!(language, "pl" | "fr" | "ru" | "cs" | "sk" | "sv" | "fi" | "nb" | "nn" | "no" | "uk" | "hu" | "bg" | "et" | "lv" | "lt");
    let (group, groups): (char, &'static [char]) =
        if dec == ',' { (if space_groups { ' ' } else { '.' }, &['.', ' ', '\u{a0}', '\u{202f}', '\u{2009}']) } else { (',', &[',']) };
    let currency: &'static [char] = match language {
        "ja" | "zh" => &['¥', '￥'],
        "ko" => &['₩'],
        "en" if tag == "en-gb" || tag == "en-ie" => &['£'],
        "en" | "" => &['$'],
        _ if dec == ',' => &['€'],
        _ => &['$'],
    };
    Locale { dec, group, groups, currency, list: if dec == ',' { ';' } else { ',' } }
}

/// Evaluate `formula` (with or without its leading `=`) for the cell in row `r`, cell index `c`.
/// A result Word shows as an error is an error here too, with Word's text.
pub fn evaluate(t: &Table, r: usize, c: usize, formula: &str) -> Result<f64, CmdError> {
    evaluate_with(t, None, None, r, c, formula)
}

/// [`evaluate`] reading the cells' text in their language (`1,5` is one and a half in Polish).
pub fn evaluate_with(
    t: &Table,
    doc: Option<&wordcraft_doc::Document>,
    default_lang: Option<&str>,
    r: usize,
    c: usize,
    formula: &str,
) -> Result<f64, CmdError> {
    match evaluate_num(t, doc, default_lang, r, c, formula) {
        Ok(n) => Ok(n.v),
        Err(Fail::Word(e)) => Err(CmdError::Failed(e.text())),
        Err(Fail::Hard(e)) => Err(e),
    }
}

/// The formula's value with its decimals and format; `Fail::Word` is a result Word shows.
fn evaluate_num(
    t: &Table,
    doc: Option<&wordcraft_doc::Document>,
    default_lang: Option<&str>,
    r: usize,
    c: usize,
    formula: &str,
) -> Result<Num, Fail> {
    if formula.len() > MAX_FORMULA {
        return Err(CmdError::Failed(format!("syntax error: the formula is longer than {MAX_FORMULA} characters")).into());
    }
    let body = formula.trim();
    let body = body.strip_prefix('=').unwrap_or(body);
    let loc = locale(default_lang);
    let toks = tokenize(body, loc);
    let expr = parse(toks)?;
    let here = Here { t, doc, loc, r, c, g: t.grid_col(r, c), visits: std::cell::Cell::new(0) };
    eval_scalar(&expr, &here)
}

/// The text a formula result shows: a whole number as is, otherwise two decimals.
pub fn format_result(v: f64) -> String {
    wordcraft_doc::locale_number::format_number(v, None)
}

/// The text a formula field shows: its result under its `\#` numeric picture when it has one (see
/// [`format_picture`]) and its `\*` number format, otherwise as Word shows it (see the module
/// note); a formula Word can't work out shows Word's error text.
pub fn evaluate_field(t: &Table, r: usize, c: usize, instr: &str) -> Result<String, CmdError> {
    evaluate_field_with(t, None, None, r, c, instr)
}

pub fn evaluate_field_with(
    t: &Table,
    doc: Option<&wordcraft_doc::Document>,
    default_lang: Option<&str>,
    r: usize,
    c: usize,
    instr: &str,
) -> Result<String, CmdError> {
    let (formula, switch) = split_switch(instr);
    // Word checks the switches before the formula: a picture after a number format, two
    // pictures, a picture with nothing after it and an unknown format each show their error.
    let mut seen_format = false;
    let mut pictures = 0;
    let mut words = switch.unwrap_or("").split_whitespace().peekable();
    while let Some(w) = words.next() {
        match w {
            "\\#" => {
                pictures += 1;
                if seen_format {
                    return Ok("Error! Picture switch must be first formatting switch.".into());
                }
                if words.peek().is_none_or(|n| n.starts_with('\\')) {
                    return Ok("Error! Switch argument not specified.".into());
                }
            }
            "\\*" => seen_format = true,
            _ => {}
        }
    }
    if pictures > 1 {
        return Ok("Error! Too many picture switches defined.".into());
    }
    let number_format = match FieldNumFormat::of_instr(instr) {
        Switch::Format(f) => Some(f),
        Switch::None => None,
        Switch::Unknown => return Ok(UNKNOWN_SWITCH.to_string()),
    };
    let value = match evaluate_num(t, doc, default_lang, r, c, formula) {
        Ok(n) => n,
        Err(Fail::Word(e)) => return Ok(e.text()),
        Err(Fail::Hard(e)) => return Err(e),
    };
    if let Some(f) = number_format {
        return Ok(f.format_value(value.v));
    }
    Ok(match switch.and_then(picture_of) {
        // A picture without a digit place shows as it is (`\# "abc"` abc, `\# ""` nothing).
        Some(picture) if !picture.contains(['0', '#', 'x']) => picture,
        Some(picture) => format_picture(value.v, &picture),
        None => match show(value, locale(default_lang)) {
            Ok(text) => text,
            Err(e) => e.text(),
        },
    })
}

/// Splits a field's switches off its formula: `=SUM(ABOVE) \# "0.00"` gives the formula and the
/// switch text (from the first `\#` or `\*` on).
pub fn split_switch(instr: &str) -> (&str, Option<&str>) {
    match [instr.find("\\#"), instr.find("\\*")].into_iter().flatten().min() {
        Some(at) => (instr.get(..at).unwrap_or("").trim_end(), instr.get(at..)),
        None => (instr, None),
    }
}

/// The picture a `\#` switch names: a quoted one (`"#,##0.00"`, possibly empty) or a single word.
fn picture_of(switch: &str) -> Option<String> {
    let at = switch.find("\\#")?;
    let rest = switch.get(at + 2..)?.trim_start();
    let picture = match rest.strip_prefix('"') {
        Some(quoted) => quoted.split('"').next().unwrap_or_default(),
        None => rest.split_whitespace().next().unwrap_or_default(),
    };
    let picture: String = picture.chars().take(MAX_PICTURE).collect();
    Some(picture)
}

/// The text of `value` under a numeric picture, as Word's `\#` pictures read: `0` is a digit that
/// always shows, `#` one that shows when it has a value, `,` groups thousands, `.` marks the
/// decimals, `%` multiplies by 100, and anything else shows as it is. Sections split by `;` give the
/// positive, negative and zero forms; with one section a negative value takes a `-` in front.
pub fn format_picture(value: f64, picture: &str) -> String {
    let sections: Vec<&str> = picture.split(';').collect();
    let (section, x, minus) = if value < 0.0 && sections.len() >= 2 {
        (sections[1], -value, false)
    } else if value == 0.0 && sections.len() >= 3 {
        (sections[2], 0.0, false)
    } else {
        (sections[0], value.abs(), value < 0.0)
    };
    // The number part runs from the first to the last digit place, separator or point.
    let is_place = |c: char| matches!(c, '0' | '#' | 'x' | ',' | '.');
    let Some(start) = section.find(is_place) else { return section.to_string() };
    let end = section.rfind(is_place).map_or(section.len(), |i| i + 1);
    let core = &section[start..end];
    let suffix = &section[end..];
    let prefix = &section[..start];
    let x = if prefix.contains('%') || suffix.contains('%') { x * 100.0 } else { x };
    let (int_pic, frac_pic) = match core.split_once('.') {
        Some((int_pic, frac_pic)) => (int_pic, Some(frac_pic)),
        None => (core, None),
    };
    let min_int = int_pic.matches('0').count().min(MAX_PICTURE);
    let max_frac = frac_pic.map_or(0, |f| f.matches(['0', '#', 'x']).count()).min(15);
    let min_frac = frac_pic.map_or(0, |f| f.matches('0').count()).min(max_frac);
    let scale = 10f64.powi(max_frac as i32);
    let rounded = (x * scale).round() / scale;
    if !rounded.is_finite() {
        return format_result(value);
    }
    let digits = format!("{:.*}", max_frac, rounded);
    let (whole, decimals) = match digits.split_once('.') {
        Some((whole, decimals)) => (whole.to_string(), decimals.to_string()),
        None => (digits.clone(), String::new()),
    };
    let whole = if whole == "0" && min_int == 0 { String::new() } else { format!("{whole:0>min_int$}") };
    let whole = if int_pic.contains(',') { group_thousands(&whole, ',') } else { whole };
    let mut decimals = decimals;
    while decimals.len() > min_frac && decimals.ends_with('0') {
        decimals.pop();
    }
    let mut body = whole;
    if !decimals.is_empty() {
        body.push('.');
        body.push_str(&decimals);
    }
    let sign = if minus && rounded != 0.0 { "-" } else { "" };
    format!("{sign}{prefix}{body}{suffix}")
}

/// `1234567` → `1,234,567`.
fn group_thousands(digits: &str, sep: char) -> String {
    let mut out = String::new();
    for (i, ch) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i).is_multiple_of(3) {
            out.push(sep);
        }
        out.push(ch);
    }
    out
}

/// The fourteen significant digits of `a` (positive, finite), trailing zeros dropped, and the
/// decimal exponent of the first: 4611686018427387904 → ("46116860184274", 18); 0.0625 → ("625",
/// -2).
fn significant(a: f64) -> (Vec<u8>, i32) {
    let s = format!("{:.*e}", SIGNIFICANT - 1, a);
    let (mantissa, exp) = s.split_once('e').unwrap_or((&s, "0"));
    let exp: i32 = exp.parse().unwrap_or(0);
    let mut digits: Vec<u8> = mantissa.bytes().filter(u8::is_ascii_digit).map(|b| b - b'0').collect();
    while digits.last() == Some(&0) {
        digits.pop();
    }
    (digits, exp)
}

/// Add one to a decimal digit string's last place, carrying; whether it carried out.
fn increment(digits: &mut [u8]) -> bool {
    for x in digits.iter_mut().rev() {
        if *x == 9 {
            *x = 0;
        } else {
            *x += 1;
            return false;
        }
    }
    true
}

/// The text a result shows (see the module note): Word's decimals, fourteen significant digits,
/// trailing zeros dropped, the inherited thousands, currency and parentheses, the language's
/// decimal separator.
fn show(n: Num, loc: Locale) -> Result<String, FormulaError> {
    if n.v.is_nan() {
        return Ok("0.00".into());
    }
    if n.v.is_infinite() {
        return Ok("0".into());
    }
    let d = if n.fmt.currency { 2 } else { n.d.min(MAX_DECIMALS) } as usize;
    let (digits, exp) = if n.v == 0.0 { (Vec::new(), 0) } else { significant(n.v.abs()) };
    // Place the digits: `int` before the point, `frac` after, then round half away from zero
    // to `d` decimals on the decimal digits themselves (no binary surprises).
    let mut int: Vec<u8> = Vec::new();
    let mut frac: Vec<u8> = Vec::new();
    for (i, &dg) in digits.iter().enumerate() {
        let place = exp - i as i32;
        if place >= 0 {
            int.push(dg);
        } else {
            let k = (-place - 1) as usize;
            if k > frac.len() {
                frac.resize(k, 0);
            }
            frac.push(dg);
        }
    }
    if exp >= 0 && int.len() < (exp + 1) as usize {
        int.resize((exp + 1) as usize, 0);
    }
    if frac.len() > d {
        let round_up = frac.get(d).is_some_and(|&x| x >= 5);
        frac.truncate(d);
        if round_up && increment(&mut frac) && increment(&mut int) {
            int.insert(0, 1);
        }
    }
    frac.resize(d, 0);
    while int.len() > 1 && int.first() == Some(&0) {
        int.remove(0);
    }
    if int.len() > MAX_INT_DIGITS {
        return Err(FormulaError::TooLarge);
    }
    let zero = int.iter().all(|&x| x == 0) && frac.iter().all(|&x| x == 0);
    let mut whole: String = if int.is_empty() { "0".into() } else { int.iter().map(|x| char::from(b'0' + x)).collect() };
    if n.fmt.thousands || n.fmt.currency {
        whole = group_thousands(&whole, loc.group);
    }
    let mut decimals: String = frac.iter().map(|x| char::from(b'0' + x)).collect();
    if !n.fmt.currency {
        while decimals.ends_with('0') {
            decimals.pop();
        }
        if zero && d >= 1 {
            decimals = "0".into();
        }
    }
    let body = if decimals.is_empty() { whole } else { format!("{whole}{}{decimals}", loc.dec) };
    let sign = n.v < 0.0 && !zero;
    let currency: String = if n.fmt.currency { loc.currency.first().copied().unwrap_or('$').to_string() } else { String::new() };
    Ok(match (sign, n.fmt.currency || n.fmt.parens) {
        (true, true) => format!("({currency}{body})"),
        (true, false) => format!("-{currency}{body}"),
        (false, _) => format!("{currency}{body}"),
    })
}

/// A cell's number as a sort key reads it: surrounding spaces, currency signs, thousands
/// separators and `%` are ignored, and accounting negatives in parentheses count as negative.
/// Anything else (letters included) makes the cell text, not a number.
pub fn parse_number(text: &str, lang: Option<&str>) -> Option<f64> {
    let t = text.trim();
    let (t, negative) = match t.strip_prefix('(').and_then(|x| x.strip_suffix(')')) {
        Some(inner) => (inner.trim(), true),
        None => (t, false),
    };
    if let Some(x) = wordcraft_doc::locale_number::parse_number(t, lang) {
        let x = if negative { -x.abs() } else { x };
        return x.is_finite().then_some(x);
    }
    let cleaned: String = t.chars().filter(|c| !matches!(c, ',' | '$' | '€' | '£' | '¥' | '৳' | '%') && !c.is_whitespace()).collect();
    if cleaned.is_empty() || !cleaned.chars().all(|c| c.is_ascii_digit() || matches!(c, '.' | '-' | '+' | 'e' | 'E')) {
        return None;
    }
    let x = cleaned.parse::<f64>().ok()?;
    let x = if negative { -x } else { x };
    x.is_finite().then_some(x)
}

/// Whether a cell holds a number for ABOVE, LEFT, RIGHT and BELOW (see the module note), and
/// its value when it does.
pub(crate) fn cell_number_in(c: &Cell, lang: Option<&str>) -> Option<f64> {
    walk_number(c, locale(lang)).map(|n| n.v)
}

/// A decimal digit of any script (`1`, `١`, `１`).
fn is_decimal_digit(c: char) -> bool {
    c.is_ascii_digit() || matches!(c as u32, 0x660..=0x669 | 0x6F0..=0x6F9 | 0x966..=0x96F | 0xFF10..=0xFF19)
}

/// Whether `text` is a number for the walk of ABOVE and its kin: a sign, the locale's currency
/// sign, then digits with separators and `%` and nothing else.
fn is_number_text(text: &str, loc: Locale) -> bool {
    let mut rest = text.trim();
    if let Some(r) = rest.strip_prefix(['-', '+']) {
        rest = r.trim_start();
    }
    if let Some(r) = rest.strip_prefix(loc.currency) {
        rest = r.trim_start();
    }
    !rest.is_empty()
        && rest.chars().any(is_decimal_digit)
        && rest.chars().all(|c| is_decimal_digit(c) || c == loc.dec || loc.groups.contains(&c) || c == '%')
}

/// The number a cell holds for the walk, if it holds one: a text cell as [`is_number_text`]
/// says, a cell holding a formula by its result (a parenthesised negative or a currency counts).
fn walk_number(c: &Cell, loc: Locale) -> Option<Num> {
    let text = super::table::cell_text(c);
    let has_field =
        c.blocks.iter().filter_map(|b| b.as_para()).any(|p| p.objects.iter().any(|o| matches!(o, wordcraft_doc::para::InlineObject::Field { .. })));
    let plain: String = if has_field {
        text.chars().filter(|c| !matches!(c, '(' | ')') && !loc.currency.contains(c) && !loc.groups.contains(c)).collect()
    } else {
        text.clone()
    };
    is_number_text(&plain, loc).then(|| cell_value(&text, loc))
}

/// A cell's text read loosely, as `=A1` reads it (see the module note); 0 for anything Word can't
/// read.
fn cell_value(text: &str, loc: Locale) -> Num {
    loose::value(text, loc).unwrap_or_else(|| Num::new(0.0))
}

/// The loose reading of a cell's text: numbers with the formats they're written in, `+ - * / ^`,
/// parentheses, `%`, words skipped, two numbers side by side added, the rest of the text dropped
/// at the first character outside ASCII; a text Word can't read at all (an unbalanced
/// parenthesis, a leading `+`, a trailing operator) is nothing.
mod loose {
    use super::{Fmt, Locale, Num};

    #[derive(Clone, Debug, PartialEq)]
    enum T {
        Num(Num),
        Op(char),
        Open,
        Close,
        /// A comma, a word or a space between two numbers.
        Sep,
    }

    fn tokens(text: &str, loc: Locale) -> Option<Vec<T>> {
        let chars: Vec<char> = text.chars().collect();
        let mut out: Vec<T> = Vec::new();
        // The token a `%` was already applied to: `5%%` is 0.05, not 0.0005.
        let mut percented: Option<usize> = None;
        let mut i = 0;
        while let Some(&c) = chars.get(i) {
            if c.is_whitespace() {
                i += 1;
                out.push(T::Sep);
            } else if c.is_ascii_digit() || c == loc.dec {
                let (n, next) = number(&chars, i, loc, Fmt::default())?;
                out.push(T::Num(n));
                i = next;
            } else if super::CURRENCY_SIGNS.contains(&c) {
                // A currency sign stands in front of its number only.
                let fmt = Fmt { currency: loc.currency.contains(&c), ..Fmt::default() };
                let mut j = i + 1;
                while chars.get(j).is_some_and(|c| c.is_whitespace()) {
                    j += 1;
                }
                if !chars.get(j).is_some_and(|c| c.is_ascii_digit() || *c == loc.dec) {
                    return None;
                }
                let (n, next) = number(&chars, j, loc, fmt)?;
                out.push(T::Num(n));
                i = next;
            } else if c.is_ascii_alphabetic() {
                // A word is skipped: `1 x 2` is 3, `12abc34` 12.
                while chars.get(i).is_some_and(|c| c.is_ascii_alphanumeric()) {
                    i += 1;
                }
                out.push(T::Sep);
            } else if loc.groups.contains(&c) || c == ',' {
                out.push(T::Sep);
                i += 1;
            } else if matches!(c, '+' | '-' | '*' | '/' | '^') {
                out.push(T::Op(c));
                i += 1;
            } else if c == '(' {
                out.push(T::Open);
                i += 1;
            } else if c == ')' {
                out.push(T::Close);
                i += 1;
            } else if c == '%' {
                // A percent sign after a number (even across spaces: `5 %`) is read with it.
                i += 1;
                if let Some(k) = out.iter().rposition(|t| *t != T::Sep)
                    && percented != Some(k)
                    && let Some(T::Num(n)) = out.get_mut(k)
                {
                    *n = n.percent();
                    percented = Some(k);
                }
            } else if c.is_ascii() {
                i += 1;
            } else {
                break;
            }
        }
        Some(out)
    }

    /// A number at `chars[i]`: digits with the locale's group separators (a group separator
    /// followed by three digits or more), a decimal part, `e+`/`e-` scientific notation. A digit
    /// followed by `e` without a signed exponent is unreadable.
    fn number(chars: &[char], mut i: usize, loc: Locale, fmt: Fmt) -> Option<(Num, usize)> {
        let mut digits = String::new();
        let mut fmt = fmt;
        while let Some(&c) = chars.get(i) {
            if c.is_ascii_digit() {
                digits.push(c);
                i += 1;
            } else if loc.groups.contains(&c) && (1..=3).all(|k| chars.get(i + k).is_some_and(|c| c.is_ascii_digit())) {
                fmt.thousands = true;
                i += 1;
            } else {
                break;
            }
        }
        let mut d = 0u8;
        if chars.get(i) == Some(&loc.dec) {
            i += 1;
            while let Some(&c) = chars.get(i) {
                if !c.is_ascii_digit() {
                    break;
                }
                digits.push(c);
                d = d.saturating_add(1);
                i += 1;
            }
        }
        if digits.is_empty() {
            return None;
        }
        let digits: String = digits.chars().take(400).collect();
        let mut v: f64 = format!("0{digits}").parse().ok()?;
        v /= 10f64.powi(d as i32);
        if chars.get(i).is_some_and(|c| matches!(c, 'e' | 'E')) {
            let sign = chars.get(i + 1).copied();
            if !matches!(sign, Some('+') | Some('-')) || !chars.get(i + 2).is_some_and(|c| c.is_ascii_digit()) {
                return None;
            }
            i += 2;
            let mut exp = String::new();
            while let Some(&c) = chars.get(i) {
                if !c.is_ascii_digit() {
                    break;
                }
                exp.push(c);
                i += 1;
            }
            let exp: i32 = exp.parse().unwrap_or(400);
            let exp = exp.min(400);
            v *= 10f64.powi(if sign == Some('-') { -exp } else { exp });
            d = 0;
        }
        Some((Num::with(v, d, fmt), i))
    }

    struct P {
        toks: Vec<T>,
        pos: usize,
        depth: usize,
    }

    impl P {
        fn peek(&self) -> Option<&T> {
            self.toks.get(self.pos)
        }

        fn skip_seps(&mut self) {
            while self.peek() == Some(&T::Sep) {
                self.pos += 1;
            }
        }

        fn enter(&mut self) -> Option<()> {
            self.depth += 1;
            (self.depth <= super::MAX_DEPTH).then_some(())
        }

        fn leave(&mut self) {
            self.depth = self.depth.saturating_sub(1);
        }

        fn expr(&mut self) -> Option<Num> {
            let mut lhs = self.term()?;
            loop {
                self.skip_seps();
                match self.peek() {
                    Some(T::Op(op @ ('+' | '-'))) => {
                        let op = *op;
                        self.pos += 1;
                        let rhs = self.term()?;
                        lhs = Num::bin(op, lhs, rhs).ok()?;
                    }
                    // Two numbers side by side add up (`1 2`, `1,5`, `5%5`).
                    Some(T::Num(_)) if self.last_was_number() => {
                        let rhs = self.term()?;
                        lhs = Num::bin('+', lhs, rhs).ok()?;
                    }
                    _ => return Some(lhs),
                }
            }
        }

        /// Whether the token before the current separators was a number (not a closing
        /// parenthesis): only numbers sit side by side.
        fn last_was_number(&self) -> bool {
            self.toks.get(..self.pos).and_then(|t| t.iter().rev().find(|t| **t != T::Sep)).is_some_and(|t| matches!(t, T::Num(_)))
        }

        fn term(&mut self) -> Option<Num> {
            let mut lhs = self.power()?;
            loop {
                self.skip_seps();
                match self.peek() {
                    Some(T::Op(op @ ('*' | '/'))) => {
                        let op = *op;
                        self.pos += 1;
                        let rhs = self.power()?;
                        lhs = Num::bin(op, lhs, rhs).ok()?;
                    }
                    _ => return Some(lhs),
                }
            }
        }

        fn power(&mut self) -> Option<Num> {
            let base = self.unary()?;
            self.skip_seps();
            if self.peek() == Some(&T::Op('^')) {
                self.pos += 1;
                self.enter()?;
                let exp = self.power()?;
                self.leave();
                return Num::bin('^', base, exp).ok();
            }
            Some(base)
        }

        fn unary(&mut self) -> Option<Num> {
            self.skip_seps();
            if self.peek() == Some(&T::Op('-')) {
                self.pos += 1;
                self.enter()?;
                let inner = self.unary()?;
                self.leave();
                return Some(inner.neg());
            }
            self.primary()
        }

        fn primary(&mut self) -> Option<Num> {
            self.skip_seps();
            match self.peek()? {
                T::Num(n) => {
                    let n = *n;
                    self.pos += 1;
                    Some(n)
                }
                T::Open => {
                    self.pos += 1;
                    self.enter()?;
                    // `(5)` is the negative five in parentheses; `(1+2)` groups.
                    let literal = match (self.toks.get(self.pos), self.toks.get(self.pos + 1)) {
                        (Some(T::Num(n)), Some(T::Close)) => Some(*n),
                        _ => None,
                    };
                    let inner = match literal {
                        Some(n) => {
                            self.pos += 2;
                            Num::with(-n.v, n.d, Fmt { parens: true, ..n.fmt })
                        }
                        None => {
                            let inner = self.expr()?;
                            self.skip_seps();
                            if self.peek() != Some(&T::Close) {
                                return None;
                            }
                            self.pos += 1;
                            inner
                        }
                    };
                    self.leave();
                    Some(inner)
                }
                _ => None,
            }
        }
    }

    pub(super) fn value(text: &str, loc: Locale) -> Option<Num> {
        let toks = tokens(text, loc)?;
        let mut p = P { toks, pos: 0, depth: 0 };
        p.skip_seps();
        p.peek()?;
        let v = p.expr()?;
        p.skip_seps();
        if p.pos != p.toks.len() {
            return None;
        }
        v.v.is_finite().then_some(v)
    }
}

#[derive(Clone, Debug, PartialEq)]
enum Tok {
    /// A number as written: its value and its text (for an error).
    Num {
        n: Num,
        text: String,
    },
    /// A name: a function, a keyword, a cell reference or a bookmark, upper-cased.
    Name(String),
    Op(char),
    /// A comparison: `=`, `<>`, `<`, `<=`, `>` or `>=`.
    Cmp(&'static str),
    /// Text that can't be part of a formula; the parser reports it where it stands.
    Bad(String),
}

fn tokenize(s: &str, loc: Locale) -> Vec<Tok> {
    let chars: Vec<char> = s.chars().collect();
    let mut out = Vec::new();
    let mut i = 0;
    while let Some(&ch) = chars.get(i) {
        if ch.is_whitespace() {
            i += 1;
        } else if ch.is_ascii_digit() || ch == loc.dec {
            let start = i;
            let mut digits = String::new();
            let mut thousands = false;
            while let Some(&c) = chars.get(i) {
                if c.is_ascii_digit() {
                    digits.push(c);
                    i += 1;
                } else if loc.groups.contains(&c) && (1..=3).all(|k| chars.get(i + k).is_some_and(|c| c.is_ascii_digit())) {
                    thousands = true;
                    i += 1;
                } else {
                    break;
                }
            }
            let mut d = 0u8;
            if chars.get(i) == Some(&loc.dec) {
                i += 1;
                while let Some(&c) = chars.get(i) {
                    if !c.is_ascii_digit() {
                        break;
                    }
                    digits.push(c);
                    d = d.saturating_add(1);
                    i += 1;
                }
            }
            let mut exp = 0i32;
            let mut bad = digits.is_empty();
            if chars.get(i).is_some_and(|c| matches!(c, 'e' | 'E')) {
                let sign = chars.get(i + 1).copied();
                if matches!(sign, Some('+') | Some('-')) && chars.get(i + 2).is_some_and(|c| c.is_ascii_digit()) {
                    i += 2;
                    let mut e = String::new();
                    while let Some(&c) = chars.get(i) {
                        if !c.is_ascii_digit() {
                            break;
                        }
                        e.push(c);
                        i += 1;
                    }
                    exp = e.parse::<i32>().unwrap_or(400).min(400);
                    if sign == Some('-') {
                        exp = -exp;
                    }
                    d = 0;
                } else {
                    // `1e9`: Word reads no scientific notation without a sign.
                    while chars.get(i).is_some_and(|c| c.is_ascii_alphanumeric() || *c == loc.dec) {
                        i += 1;
                    }
                    bad = true;
                }
            }
            let text: String = chars.get(start..i).unwrap_or_default().iter().collect::<String>().to_ascii_uppercase();
            if bad {
                out.push(Tok::Bad(text));
                continue;
            }
            let digits: String = digits.chars().take(400).collect();
            let v = format!("0{digits}").parse::<f64>().unwrap_or(0.0) / 10f64.powi(d as i32) * 10f64.powi(exp);
            out.push(Tok::Num { n: Num::with(v, d, Fmt { thousands, ..Fmt::default() }), text });
        } else if ch.is_ascii_alphabetic() {
            let mut name = String::new();
            while let Some(&c) = chars.get(i) {
                if !c.is_ascii_alphanumeric() {
                    break;
                }
                name.push(c.to_ascii_uppercase());
                i += 1;
            }
            out.push(Tok::Name(name));
        } else if !ch.is_ascii() {
            // Anything outside ASCII is a name up to a space or an ASCII delimiter: `１２`,
            // `（１＋２）＊３`, `−５`.
            let mut name = String::new();
            while let Some(&c) = chars.get(i) {
                if c.is_whitespace() || (c.is_ascii() && !c.is_ascii_alphanumeric()) {
                    break;
                }
                name.push(c.to_ascii_uppercase());
                i += 1;
            }
            out.push(Tok::Name(name));
        } else if matches!(ch, '+' | '-' | '*' | '/' | '^' | '(' | ')' | ':' | '%') {
            out.push(Tok::Op(ch));
            i += 1;
        } else if ch == loc.list {
            out.push(Tok::Op(','));
            i += 1;
        } else if matches!(ch, '<' | '>' | '=') {
            i += 1;
            let next = chars.get(i).copied();
            let (op, longer) = match (ch, next) {
                ('<', Some('=')) => ("<=", true),
                ('<', Some('>')) => ("<>", true),
                ('>', Some('=')) => (">=", true),
                ('<', _) => ("<", false),
                ('>', _) => (">", false),
                _ => ("=", false),
            };
            if longer {
                i += 1;
            }
            out.push(Tok::Cmp(op));
        } else {
            out.push(Tok::Bad(ch.to_string()));
            i += 1;
        }
    }
    out
}

/// The keywords that name the cells next to the formula's cell.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Side {
    Above,
    Left,
    Right,
    Below,
}

#[derive(Clone, Debug)]
enum Expr {
    /// A number as written.
    Lit(Num, String),
    /// `(5)`: the negative in parentheses.
    ParenLit(Num),
    /// A cell: (row, grid column, name).
    Cell(usize, usize, String),
    /// A rectangle of cells: (row0, grid col0, row1, grid col1), inclusive.
    Range(usize, usize, usize, usize),
    Side(Side),
    /// A name that is neither a function nor a cell: a bookmark's number.
    Bookmark(String),
    Neg(Box<Expr>),
    Percent(Box<Expr>),
    Bin(char, Box<Expr>, Box<Expr>),
    /// A comparison: 1 when it holds, 0 when it doesn't.
    Cmp(&'static str, Box<Expr>, Box<Expr>),
    Call(String, Vec<Expr>),
}

const LIST_FUNCTIONS: &[&str] = &["SUM", "AVERAGE", "COUNT", "MAX", "MIN", "PRODUCT"];

/// The arguments a function takes: `None` for a list of any length.
fn arity(name: &str) -> Option<usize> {
    match name {
        "ABS" | "INT" | "SIGN" | "NOT" | "DEFINED" => Some(1),
        "ROUND" | "MOD" | "AND" | "OR" => Some(2),
        "IF" => Some(3),
        _ => None,
    }
}

fn is_function(name: &str) -> bool {
    arity(name).is_some() || LIST_FUNCTIONS.contains(&name)
}

struct Parser {
    toks: Vec<Tok>,
    pos: usize,
    depth: usize,
}

/// The text an error names for a token.
fn tok_text(t: &Tok) -> String {
    match t {
        Tok::Num { text, .. } => text.clone(),
        Tok::Name(n) => n.clone(),
        Tok::Op(c) => c.to_string(),
        Tok::Cmp(op) => op.to_string(),
        Tok::Bad(t) => t.clone(),
    }
}

fn parse(toks: Vec<Tok>) -> Result<Expr, Fail> {
    // Word sees a formula that stops short before anything else.
    let ends_short = match toks.last() {
        None => true,
        Some(Tok::Op(c)) => matches!(c, '+' | '-' | '*' | '/' | '^' | '(' | ',' | ':'),
        Some(Tok::Cmp(_)) => true,
        Some(Tok::Name(n)) => is_function(n),
        _ => false,
    };
    if ends_short {
        return Err(FormulaError::UnexpectedEnd.into());
    }
    let mut p = Parser { toks, pos: 0, depth: 0 };
    let e = p.compare()?;
    if let Some(t) = p.toks.get(p.pos) {
        return Err(p.after_value(t).into());
    }
    Ok(e)
}

impl Parser {
    fn peek_op(&self, c: char) -> bool {
        matches!(self.toks.get(self.pos), Some(Tok::Op(x)) if *x == c)
    }

    fn eat_op(&mut self, c: char) -> bool {
        let hit = self.peek_op(c);
        if hit {
            self.pos += 1;
        }
        hit
    }

    fn enter(&mut self) -> Result<(), Fail> {
        self.depth += 1;
        if self.depth > MAX_DEPTH { Err(CmdError::Failed("syntax error: the formula is nested too deeply".into()).into()) } else { Ok(()) }
    }

    fn leave(&mut self) {
        self.depth = self.depth.saturating_sub(1);
    }

    /// The error for a token where an operator or the end was expected: a number or a
    /// parenthesis after a value is a missing operator (`1 2`, `2(3)`), a number after a name
    /// (`FOO 1`) and anything else is reported as it is.
    fn after_value(&self, t: &Tok) -> FormulaError {
        let after_name = self.pos > 0 && matches!(self.toks.get(self.pos - 1), Some(Tok::Name(_)));
        match t {
            Tok::Num { text, .. } if after_name => FormulaError::Syntax(text.clone()),
            Tok::Num { .. } | Tok::Op('(') => FormulaError::MissingOperator,
            other => FormulaError::Syntax(tok_text(other)),
        }
    }

    /// Comparisons chain from the left: `1<2<3` is `(1<2)<3`.
    fn compare(&mut self) -> Result<Expr, Fail> {
        let mut lhs = self.expr()?;
        while let Some(Tok::Cmp(op)) = self.toks.get(self.pos).cloned() {
            self.pos += 1;
            let rhs = self.expr()?;
            lhs = Expr::Cmp(op, Box::new(lhs), Box::new(rhs));
        }
        Ok(lhs)
    }

    fn expr(&mut self) -> Result<Expr, Fail> {
        let mut lhs = self.term()?;
        loop {
            let op = if self.eat_op('+') {
                '+'
            } else if self.eat_op('-') {
                '-'
            } else {
                return Ok(lhs);
            };
            let rhs = self.term()?;
            lhs = Expr::Bin(op, Box::new(lhs), Box::new(rhs));
        }
    }

    fn term(&mut self) -> Result<Expr, Fail> {
        let mut lhs = self.power()?;
        loop {
            let op = if self.eat_op('*') {
                '*'
            } else if self.eat_op('/') {
                '/'
            } else {
                return Ok(lhs);
            };
            let rhs = self.power()?;
            lhs = Expr::Bin(op, Box::new(lhs), Box::new(rhs));
        }
    }

    fn power(&mut self) -> Result<Expr, Fail> {
        let base = self.unary()?;
        if self.eat_op('^') {
            // `^` is right-associative, so each one in a chain nests one level deeper.
            self.enter()?;
            let exp = self.power()?;
            self.leave();
            return Ok(Expr::Bin('^', Box::new(base), Box::new(exp)));
        }
        Ok(base)
    }

    /// A minus binds tighter than `^` (`-2^2` is 4); a plus in front of a value is an error.
    fn unary(&mut self) -> Result<Expr, Fail> {
        if self.eat_op('-') {
            self.enter()?;
            let inner = self.unary()?;
            self.leave();
            return Ok(Expr::Neg(Box::new(inner)));
        }
        self.postfix()
    }

    /// `5%` is a twentieth; a second `%` is an error.
    fn postfix(&mut self) -> Result<Expr, Fail> {
        let mut e = self.primary()?;
        if self.eat_op('%') {
            e = Expr::Percent(Box::new(e));
            if self.peek_op('%') {
                return Err(FormulaError::Syntax("%".into()).into());
            }
        }
        Ok(e)
    }

    fn primary(&mut self) -> Result<Expr, Fail> {
        let Some(tok) = self.toks.get(self.pos).cloned() else {
            return Err(FormulaError::UnexpectedEnd.into());
        };
        self.pos += 1;
        match tok {
            Tok::Num { n, text } => Ok(Expr::Lit(n, text)),
            Tok::Op('(') => {
                self.enter()?;
                // `(5)` and `(5%)` are negatives in parentheses; anything else groups.
                let literal = match (self.toks.get(self.pos), self.toks.get(self.pos + 1), self.toks.get(self.pos + 2)) {
                    (Some(Tok::Num { n, .. }), Some(Tok::Op(')')), _) => Some((*n, 2)),
                    (Some(Tok::Num { n, .. }), Some(Tok::Op('%')), Some(Tok::Op(')'))) => Some((n.percent(), 3)),
                    _ => None,
                };
                let inner = match literal {
                    Some((n, skip)) => {
                        self.pos += skip;
                        Expr::ParenLit(Num::with(-n.v, n.d, Fmt { parens: true, ..n.fmt }))
                    }
                    None => {
                        let inner = self.compare()?;
                        match self.toks.get(self.pos) {
                            Some(Tok::Op(')')) => self.pos += 1,
                            Some(t) => return Err(self.after_value(t).into()),
                            None => return Err(FormulaError::UnexpectedEnd.into()),
                        }
                        inner
                    }
                };
                self.leave();
                Ok(inner)
            }
            Tok::Name(name) => self.name(name),
            Tok::Op(c) => Err(FormulaError::Syntax(c.to_string()).into()),
            Tok::Cmp(op) => Err(FormulaError::Syntax(op.to_string()).into()),
            Tok::Bad(t) => Err(FormulaError::Syntax(t).into()),
        }
    }

    /// A function's arguments after its `(`.
    fn args(&mut self) -> Result<Vec<Expr>, Fail> {
        self.enter()?;
        let mut args = Vec::new();
        if self.eat_op(')') {
            self.leave();
            return Ok(args);
        }
        loop {
            if self.peek_op(',') {
                return Err(FormulaError::Syntax(",".into()).into());
            }
            args.push(self.compare()?);
            match self.toks.get(self.pos) {
                Some(Tok::Op(')')) => {
                    self.pos += 1;
                    break;
                }
                Some(Tok::Op(',')) => {
                    self.pos += 1;
                    if self.peek_op(')') {
                        return Err(FormulaError::Syntax(")".into()).into());
                    }
                }
                Some(Tok::Bad(t)) => return Err(FormulaError::Syntax(t.clone()).into()),
                Some(_) => return Err(FormulaError::Syntax(")".into()).into()),
                None => return Err(FormulaError::UnexpectedEnd.into()),
            }
        }
        self.leave();
        Ok(args)
    }

    fn name(&mut self, name: String) -> Result<Expr, Fail> {
        let side = match name.as_str() {
            "ABOVE" => Some(Side::Above),
            "LEFT" => Some(Side::Left),
            "RIGHT" => Some(Side::Right),
            "BELOW" => Some(Side::Below),
            _ => None,
        };
        if let Some(side) = side {
            return Ok(Expr::Side(side));
        }
        if matches!(name.as_str(), "TRUE" | "FALSE") && !self.peek_op('(') {
            return Ok(Expr::Call(name, Vec::new()));
        }
        if is_function(&name) {
            if !self.eat_op('(') {
                // A function without its parentheses: Word reports what follows it.
                return Err(match self.toks.get(self.pos) {
                    Some(t) => FormulaError::Syntax(tok_text(t)),
                    None => FormulaError::UnexpectedEnd,
                }
                .into());
            }
            let args = self.args()?;
            check_arity(&name, &args)?;
            return Ok(Expr::Call(name, args));
        }
        if self.eat_op('(') {
            // An unknown function (TRUE and FALSE with parentheses too): Word reports its one
            // literal argument, else the parenthesis.
            let args = self.args()?;
            return Err(match args.as_slice() {
                [Expr::Lit(_, text)] => FormulaError::Syntax(text.clone()),
                _ => FormulaError::Syntax("(".into()),
            }
            .into());
        }
        if let Some((row, col)) = cell_ref(&name) {
            if self.eat_op(':') {
                let Some(Tok::Name(other)) = self.toks.get(self.pos).cloned() else {
                    return Err(FormulaError::Syntax(":".into()).into());
                };
                let Some((row2, col2)) = cell_ref(&other) else {
                    return Err(FormulaError::Syntax(":".into()).into());
                };
                self.pos += 1;
                return Ok(Expr::Range(row.min(row2), col.min(col2), row.max(row2), col.max(col2)));
            }
            return Ok(Expr::Cell(row, col, name));
        }
        Ok(Expr::Bookmark(name))
    }
}

/// Word's checks on a function's arguments: a list function and AND/OR with one argument
/// report it when it is a literal, else `)`; a function with too few arguments reports its one
/// literal argument, else `)`; one with too many reports `,`.
fn check_arity(name: &str, args: &[Expr]) -> Result<(), FormulaError> {
    let one_arg = |a: &Expr| match a {
        Expr::Lit(_, text) => Err(FormulaError::Syntax(text.clone())),
        Expr::Cell(..) | Expr::Range(..) | Expr::Side(_) => Ok(()),
        _ => Err(FormulaError::Syntax(")".into())),
    };
    match arity(name) {
        None => match args {
            [] => Err(FormulaError::Syntax(")".into())),
            [a] => one_arg(a),
            _ => Ok(()),
        },
        Some(n) if args.len() > n => Err(FormulaError::Syntax(",".into())),
        Some(n) if args.len() == n => Ok(()),
        Some(_) => match args {
            [] => Err(FormulaError::Syntax(")".into())),
            [a] if matches!(name, "AND" | "OR") => one_arg(a),
            [Expr::Lit(_, text)] => Err(FormulaError::Syntax(text.clone())),
            _ => Err(FormulaError::Syntax(")".into())),
        },
    }
}

/// `B4` → (row 3, column 1): the row counts from 1 and the column from A.
fn cell_ref(name: &str) -> Option<(usize, usize)> {
    let letters: String = name.chars().take_while(|c| c.is_ascii_alphabetic()).collect();
    let digits = name.get(letters.len()..)?;
    if letters.is_empty() || digits.is_empty() || !digits.chars().all(|c| c.is_ascii_digit()) {
        return None;
    }
    let col =
        letters.bytes().try_fold(0usize, |acc, b| acc.checked_mul(26)?.checked_add((b.to_ascii_uppercase() - b'A') as usize + 1))?.checked_sub(1)?;
    let row = digits.parse::<usize>().ok()?.checked_sub(1)?;
    (col < MAX_COLS && row < MAX_ROWS).then_some((row, col))
}

/// Where the formula's cell sits.
struct Here<'a> {
    t: &'a Table,
    doc: Option<&'a wordcraft_doc::Document>,
    loc: Locale,
    r: usize,
    c: usize,
    g: usize,
    /// Cells read so far in this evaluation (see [`MAX_CELL_VISITS`]).
    visits: std::cell::Cell<usize>,
}

impl Here<'_> {
    /// Whether the formula sits in a table at all.
    fn in_table(&self) -> bool {
        !self.t.rows.is_empty()
    }

    /// Count `n` more cells read, failing once the evaluation has read too many.
    fn visit(&self, n: usize) -> Result<(), Fail> {
        let total = self.visits.get().saturating_add(n);
        self.visits.set(total);
        if total > MAX_CELL_VISITS { Err(CmdError::Failed("Formula too complex".into()).into()) } else { Ok(()) }
    }

    fn cell_locale(&self, c: &Cell) -> Locale {
        let para_lang = self.doc.and_then(|doc| {
            c.blocks.iter().filter_map(|b| b.as_para()).find(|p| !p.plain_text().trim().is_empty()).and_then(|p| super::para_lang(doc, p))
        });
        match para_lang {
            Some(lang) => locale(Some(&lang)),
            None => self.loc,
        }
    }

    fn cell(&self, r: usize, g: usize) -> Result<Option<&Cell>, Fail> {
        self.visit(1)?;
        let Some(ci) = self.t.cell_at_grid(r, g) else { return Ok(None) };
        Ok(self.t.rows.get(r).and_then(|row| row.cells.get(ci)))
    }

    /// The number the cell at grid column `g` of row `r` holds for a walk, counted against the
    /// budget.
    fn walk_at(&self, r: usize, g: usize) -> Result<Option<Num>, Fail> {
        Ok(self.cell(r, g)?.and_then(|c| walk_number(c, self.cell_locale(c))))
    }

    /// The cell's value as `=A1` reads it.
    fn value_at(&self, r: usize, g: usize) -> Result<Num, Fail> {
        Ok(match self.cell(r, g)? {
            Some(c) => cell_value(&super::table::cell_text(c), self.cell_locale(c)),
            None => Num::new(0.0),
        })
    }

    fn bookmark(&self, name: &str) -> Result<Num, Fail> {
        let Some(doc) = self.doc else { return Err(FormulaError::UndefinedBookmark(name.to_string()).into()) };
        let Some((actual, _)) = doc.bookmarks().into_iter().find(|(n, _)| n.eq_ignore_ascii_case(name)) else {
            return Err(FormulaError::UndefinedBookmark(name.to_string()).into());
        };
        let text = super::references::bookmark_text(doc, &actual).unwrap_or_default();
        Ok(cell_value(&text, self.loc))
    }
}

enum Val {
    Num(Num),
    List(Vec<Num>),
}

/// The value of `e` as one number: a keyword outside a table is 0, a list its sum, a range
/// outside a list function an error.
fn eval_scalar(e: &Expr, here: &Here) -> Result<Num, Fail> {
    match e {
        Expr::Side(_) if !here.in_table() => return Ok(Num::new(0.0)),
        Expr::Range(..) => return Err(FormulaError::Syntax(":".into()).into()),
        _ => {}
    }
    match eval(e, here)? {
        Val::Num(n) => Ok(n),
        Val::List(v) => Ok(sum(&v)),
    }
}

fn sum(v: &[Num]) -> Num {
    v.iter().fold(Num::new(0.0), |acc, n| Num::join(acc.v + n.v, acc.d.max(n.d), acc, *n))
}

fn eval(e: &Expr, here: &Here) -> Result<Val, Fail> {
    Ok(match e {
        Expr::Lit(n, _) | Expr::ParenLit(n) => Val::Num(*n),
        Expr::Cell(row, col, name) => {
            if !here.in_table() {
                return Ok(Val::Num(Num::new(0.0)));
            }
            if *row >= here.t.rows.len() || *col >= here.t.cols() {
                return Err(FormulaError::CellNotInTable(name.clone()).into());
            }
            Val::Num(here.value_at(*row, *col)?)
        }
        Expr::Range(r0, g0, r1, g1) => {
            if !here.in_table() {
                return Err(FormulaError::NotInTable.into());
            }
            // Only the part of the range inside the table holds cells.
            let mut v = Vec::new();
            let (r_end, g_end) = (here.t.rows.len().min(r1.saturating_add(1)), here.t.cols().min(g1.saturating_add(1)));
            for row in *r0..r_end {
                for g in *g0..g_end {
                    v.extend(here.walk_at(row, g)?);
                }
            }
            Val::List(v)
        }
        Expr::Side(side) => {
            if !here.in_table() {
                return Err(FormulaError::NotInTable.into());
            }
            Val::List(side_numbers(here, *side)?)
        }
        Expr::Bookmark(name) => Val::Num(here.bookmark(name)?),
        Expr::Neg(inner) => Val::Num(eval_scalar(inner, here)?.neg()),
        Expr::Percent(inner) => Val::Num(eval_scalar(inner, here)?.percent()),
        Expr::Bin(op, a, b) => {
            let (x, y) = (eval_scalar(a, here)?, eval_scalar(b, here)?);
            Val::Num(Num::bin(*op, x, y)?)
        }
        Expr::Cmp(op, a, b) => {
            let (x, y) = (eval_scalar(a, here)?, eval_scalar(b, here)?);
            let tol = COMPARE_TOLERANCE * x.v.abs().max(y.v.abs());
            let (eq, lt, gt) = ((x.v - y.v).abs() <= tol, x.v < y.v - tol, x.v > y.v + tol);
            let holds = match *op {
                "=" => eq,
                "<>" => !eq,
                "<" => lt,
                "<=" => lt || eq,
                ">" => gt,
                _ => gt || eq,
            };
            Val::Num(Num::with(truth(holds), 0, x.fmt.or(y.fmt)))
        }
        Expr::Call(name, args) => Val::Num(call(name, args, here)?),
    })
}

/// A truth value as a formula reads it: 1 for true, 0 for false.
fn truth(holds: bool) -> f64 {
    if holds { 1.0 } else { 0.0 }
}

/// The first run of numbers next to the formula's cell on one side (see the module note): the
/// cells are skipped while they hold no number, then taken while they do. ABOVE in the top row
/// and LEFT in the first column are errors.
fn side_numbers(here: &Here, side: Side) -> Result<Vec<Num>, Fail> {
    let mut out = Vec::new();
    let mut started = false;
    let mut take = |value: Option<Num>| -> bool {
        match value {
            Some(x) => {
                out.push(x);
                started = true;
                true
            }
            None => !started,
        }
    };
    match side {
        Side::Above => {
            if here.r == 0 {
                return Err(FormulaError::TableIndexZero.into());
            }
            for row in (0..here.r).rev() {
                if !take(here.walk_at(row, here.g)?) {
                    break;
                }
            }
        }
        Side::Below => {
            for row in (here.r + 1)..here.t.rows.len() {
                if !take(here.walk_at(row, here.g)?) {
                    break;
                }
            }
        }
        Side::Left | Side::Right => {
            if side == Side::Left && here.c == 0 {
                return Err(FormulaError::TableIndexZero.into());
            }
            let Some(row) = here.t.rows.get(here.r) else { return Ok(out) };
            here.visit(row.cells.len())?;
            let cells: Vec<Option<Num>> = row.cells.iter().map(|c| walk_number(c, here.cell_locale(c))).collect();
            let order: Vec<usize> = if side == Side::Left { (0..here.c).rev().collect() } else { ((here.c + 1)..cells.len()).collect() };
            for ci in order {
                if !take(cells.get(ci).copied().flatten()) {
                    break;
                }
            }
        }
    }
    Ok(out)
}

/// Gather a list function's arguments: numbers and lists (ranges and keywords) become one list.
/// Cells named outside a table are an error here (`=SUM(A1)` in the body).
fn gather(args: &[Expr], here: &Here) -> Result<Vec<Num>, Fail> {
    let mut all = Vec::new();
    for a in args {
        if !here.in_table() && matches!(a, Expr::Cell(..) | Expr::Side(_) | Expr::Range(..)) {
            return Err(FormulaError::NotInTable.into());
        }
        match eval(a, here)? {
            Val::Num(x) => all.push(x),
            Val::List(v) => all.extend(v),
        }
    }
    Ok(all)
}

/// The decimals and format a list's result shows.
fn over(v: &[Num], value: f64, min_d: u8) -> Num {
    let d = v.iter().map(|n| n.d).max().unwrap_or(0).max(min_d);
    let fmt = v.iter().fold(Fmt::default(), |acc, n| acc.or(n.fmt));
    Num::with(value, d, fmt)
}

/// Round half away from zero to `digits` decimals on the decimal value (`1.005` to two decimals
/// is 1.01, as Word rounds, not the 1.00 of the binary value).
fn round_decimal(x: f64, digits: i32) -> f64 {
    let scale = 10f64.powi(digits);
    let scaled = x * scale;
    // Snap to fifteen significant digits first so 100.49999999999999 rounds as 100.5.
    let snapped: f64 = format!("{scaled:.14e}").parse().unwrap_or(scaled);
    snapped.round() / scale
}

fn call(name: &str, args: &[Expr], here: &Here) -> Result<Num, Fail> {
    let one = |i: usize| -> Result<Num, Fail> {
        match args.get(i) {
            Some(a) => eval_scalar(a, here),
            None => Err(FormulaError::Syntax(")".into()).into()),
        }
    };
    let extreme = |pick: fn(f64, f64) -> f64| -> Result<Num, Fail> {
        let v = gather(args, here)?;
        let m = v.iter().map(|n| n.v).reduce(pick).unwrap_or(0.0);
        Ok(over(&v, m, 0))
    };
    match name {
        "TRUE" | "FALSE" => Ok(Num::new(truth(name == "TRUE"))),
        "NOT" => Ok(Num::new(truth(one(0)?.v == 0.0))),
        "AND" | "OR" => {
            let v = gather(args, here)?;
            Ok(over(&v, truth(if name == "AND" { v.iter().all(|x| x.v != 0.0) } else { v.iter().any(|x| x.v != 0.0) }), 0))
        }
        // Both branches are worked out, as Word does: `IF(0,1/0,2)` is a division by zero.
        "IF" => {
            let (cond, yes, no) = (one(0)?, one(1)?, one(2)?);
            Ok(if cond.v != 0.0 { yes } else { no })
        }
        // DEFINED is 1 for a bookmark that exists, a cell inside the table, or anything Word
        // can work out; 0 otherwise.
        "DEFINED" => Ok(Num::new(truth(match args.first() {
            Some(Expr::Bookmark(n)) => here.bookmark(n).is_ok(),
            Some(Expr::Cell(row, col, _)) => !here.in_table() || (*row < here.t.rows.len() && *col < here.t.cols()),
            Some(Expr::Range(..)) => return Err(FormulaError::Syntax(":".into()).into()),
            Some(Expr::Side(_)) if here.in_table() => return Err(FormulaError::Syntax(",".into()).into()),
            Some(Expr::Side(_)) => true,
            Some(a) => match eval_scalar(a, here) {
                Ok(_) => true,
                Err(Fail::Word(_)) => false,
                Err(e @ Fail::Hard(_)) => return Err(e),
            },
            None => false,
        }))),
        "SUM" => {
            let v = gather(args, here)?;
            Ok(sum(&v))
        }
        "COUNT" => {
            let v = gather(args, here)?;
            Ok(over(&v, v.len() as f64, 0))
        }
        "AVERAGE" => {
            let v = gather(args, here)?;
            Ok(over(&v, if v.is_empty() { 0.0 } else { v.iter().map(|n| n.v).sum::<f64>() / v.len() as f64 }, 2))
        }
        "MAX" => extreme(f64::max),
        "MIN" => extreme(f64::min),
        "PRODUCT" => {
            let v = gather(args, here)?;
            Ok(over(&v, if v.is_empty() { 0.0 } else { v.iter().map(|n| n.v).product() }, 0))
        }
        "ABS" => {
            let x = one(0)?;
            Ok(Num { v: x.v.abs(), ..x })
        }
        "INT" => {
            let x = one(0)?;
            Ok(Num::with(x.v.trunc(), 0, x.fmt))
        }
        "SIGN" => {
            let x = one(0)?;
            Ok(Num::with(if x.v == 0.0 { 0.0 } else { x.v.signum() }, 0, x.fmt))
        }
        "ROUND" => {
            let (x, n) = (one(0)?, one(1)?);
            let digits = n.v.clamp(-15.0, 15.0).trunc() as i32;
            Ok(Num::with(round_decimal(x.v, digits), digits.max(0) as u8, x.fmt.or(n.fmt)))
        }
        "MOD" => {
            let (x, y) = (one(0)?, one(1)?);
            if y.v == 0.0 {
                return Err(FormulaError::Blank.into());
            }
            Ok(Num::join(x.v % y.v, x.d.max(y.d).max(1), x, y))
        }
        _ => Err(FormulaError::Syntax("(".into()).into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn table(rows: &[&[&str]]) -> Table {
        let cols = rows.iter().map(|r| r.len()).max().unwrap_or(1);
        let mut t = Table::new(rows.len(), cols, 468.0);
        for (r, row) in rows.iter().enumerate() {
            for (c, text) in row.iter().enumerate() {
                if let Some(cl) = t.rows.get_mut(r).and_then(|x| x.cells.get_mut(c)) {
                    cl.blocks = vec![wordcraft_doc::para_block(wordcraft_doc::Paragraph::with_text(text, Default::default()))];
                }
            }
        }
        t
    }

    fn ev(t: &Table, r: usize, c: usize, f: &str) -> Result<f64, CmdError> {
        evaluate(t, r, c, f)
    }

    /// What a formula field in the body shows.
    fn body(f: &str) -> String {
        evaluate_field(&Table::default(), 0, 0, f).unwrap_or_else(|e| panic!("{f}: {e}"))
    }

    fn shown(t: &Table, r: usize, c: usize, f: &str) -> String {
        evaluate_field(t, r, c, f).unwrap_or_else(|e| panic!("{f}: {e}"))
    }

    fn shown_ja(t: &Table, r: usize, c: usize, f: &str) -> String {
        evaluate_field_with(t, None, Some("ja-JP"), r, c, f).unwrap_or_else(|e| panic!("{f}: {e}"))
    }

    #[test]
    fn cell_references_count_from_one_and_from_a() {
        assert_eq!(cell_ref("A1"), Some((0, 0)));
        assert_eq!(cell_ref("B4"), Some((3, 1)));
        assert_eq!(cell_ref("AA2"), Some((1, 26)));
        assert_eq!(cell_ref("A0"), None);
        assert_eq!(cell_ref("1A"), None);
        assert_eq!(cell_ref("SUM"), None);
    }

    #[test]
    fn above_takes_the_first_run_of_numbers() {
        let t = table(&[&["5"], &["x"], &["2"], &[""]]);
        assert_eq!(ev(&t, 3, 0, "=SUM(ABOVE)"), Ok(2.0), "text stops the run; the 5 above it is not counted");
        assert_eq!(ev(&t, 2, 0, "=SUM(ABOVE)"), Ok(5.0), "text directly above is skipped, so the run is the 5 above it");
        let t = table(&[&["5"], &[""], &[""], &[""]]);
        assert_eq!(ev(&t, 3, 0, "=SUM(ABOVE)"), Ok(5.0), "empty cells are skipped up to the 5");
        let t = table(&[&["5"], &["3"], &[""], &["9"], &[""]]);
        assert_eq!(ev(&t, 4, 0, "=SUM(ABOVE)"), Ok(9.0), "the run is the 9 alone: the blank cuts it off");
        let t = table(&[&["x"], &["5"], &[""]]);
        assert_eq!(ev(&t, 2, 0, "=SUM(ABOVE)"), Ok(5.0));
    }

    /// Word for Mac (`fw4` probe): a cell between 5 and 3 either counts (SUM and COUNT of the
    /// three) or stops the walk at the 3.
    #[test]
    fn which_cells_count_as_numbers_for_the_walk_as_word_for_mac() {
        let walk = |mid: &str| {
            let t = table(&[&["5"], &[mid], &["3"], &[""]]);
            (shown_ja(&t, 3, 0, "=SUM(ABOVE)"), shown_ja(&t, 3, 0, "=COUNT(ABOVE)"))
        };
        for mid in [
            "",
            "x",
            "1e",
            "(5)",
            "$5",
            "abc12",
            "12abc",
            "-",
            "½",
            "0x10",
            "1 2",
            "12-3",
            "5-",
            "(5",
            "5)",
            "--5",
            " ",
            "7 円",
            "1e3",
            "!Zero Divide",
        ] {
            assert_eq!(walk(mid), ("3".to_string(), "1".to_string()), "{mid:?} stops the walk");
        }
        assert_eq!(walk("１２"), ("8".into(), "3".into()), "full-width digits count, but read as 0");
        assert_eq!(walk("5%"), ("8.05".into(), "3".into()));
        assert_eq!(walk("1,2"), ("11".into(), "3".into()));
        assert_eq!(walk("12,34"), ("54".into(), "3".into()));
        assert_eq!(walk("1.5.2"), ("9.7".into(), "3".into()));
        assert_eq!(walk("- 5"), ("3".into(), "3".into()));
        assert_eq!(walk("+5"), ("8".into(), "3".into()), "a leading plus counts but reads as 0");
        assert_eq!(walk("３12"), ("8".into(), "3".into()));
        assert_eq!(walk("1,000"), ("1,008".into(), "3".into()));
        assert_eq!(walk("0"), ("8".into(), "3".into()));
        assert_eq!(walk("¥5"), ("¥13.00".into(), "¥3.00".into()), "the currency format carries into the result");
        // A formula cell counts by its result, parentheses and currency included.
        let mut t = table(&[&["(5)", "5%", "(4.95)", ""]]);
        if let Some(cl) = t.rows.get_mut(0).and_then(|r| r.cells.get_mut(2)) {
            let mut p = wordcraft_doc::Paragraph::new();
            p.insert_object(
                0,
                wordcraft_doc::para::InlineObject::Field {
                    instr: "=A1+B1".into(),
                    result: "(4.95)".into(),
                    locked: false,
                    dirty: false,
                    code: false,
                    form: String::new(),
                },
                &Default::default(),
            )
            .unwrap();
            cl.blocks = vec![wordcraft_doc::para_block(p)];
        }
        assert_eq!(shown(&t, 0, 3, "=SUM(LEFT)"), "(4.9)", "the field's -4.95 and the 5%; the (5) text stops the walk");
        assert_eq!(shown(&t, 0, 3, "=COUNT(LEFT)"), "2");
    }

    /// Word for Mac (`fw3`, `fw5` probes): `=A1` reads the cell's text loosely.
    #[test]
    fn a_cell_named_in_a_formula_is_read_loosely_as_word_for_mac_reads_it() {
        let read = |text: &str| {
            let t = table(&[&[text, ""]]);
            shown_ja(&t, 0, 1, "=A1")
        };
        for (text, value) in [
            ("12abc", "12"),
            ("abc12", "0"),
            ("1,2", "3"),
            ("1 2", "3"),
            ("(5)", "(5)"),
            ("$5", "5"),
            ("5%", "0.05"),
            ("+5", "0"),
            ("1,000", "1,000"),
            ("1,000.5", "1,000.5"),
            ("1.000,5", "6"),
            ("12.", "12"),
            (".5", "0.5"),
            ("5 円", "5"),
            ("½", "0"),
            ("1e3", "0"),
            ("0x10", "0"),
            ("--5", "5"),
            ("５", "0"),
            ("1２", "1"),
            ("-5", "-5"),
            (" 7 ", "7"),
            ("1.5.2", "1.7"),
            ("12-3", "9"),
            ("3/4", "0.75"),
            ("€5", "5"),
            ("1'000", "1"),
            ("1 000", "1"),
            ("12abc34", "12"),
            ("1,5", "6"),
            ("0,5", "5"),
            ("-", "0"),
            ("- 5", "-5"),
            ("5-", "0"),
            ("(5", "0"),
            ("5)", "0"),
            ("((5))", "(5)"),
            ("-(5)", "5"),
            ("1,,000", "1"),
            ("1,00", "1"),
            ("1,0000", "10,000"),
            ("12,345,678", "12,345,678"),
            ("1.2.3,4", "5.5"),
            ("007", "7"),
            ("1e", "0"),
            ("１２３", "0"),
            ("12３", "12"),
            ("３12", "0"),
            ("1,000.50", "1,000.5"),
            ("(1,000)", "(1,000)"),
            ("1,0", "1"),
            ("1,", "1"),
            (",5", "5"),
            ("1,2,3", "6"),
            ("5 %", "0.05"),
            ("5 % 3", "3.05"),
            ("1.5 2", "3.5"),
            ("1,000 5", "1,005"),
            ("12x3", "12"),
            ("1 x 2", "3"),
            ("2*3", "6"),
            ("2^3", "8"),
            ("(5)5", "0"),
            ("5(5)", "0"),
            ("1+2", "3"),
            ("1-", "0"),
            ("-1-", "0"),
            ("1.", "1"),
            ("1..5", "1.5"),
            ("1,00,0", "1"),
            ("1,0000,0", "10,000"),
            ("5%5", "5.05"),
            ("5%%", "0.05"),
            ("$ 5", "5"),
            ("５％", "0"),
            ("12 abc 3", "15"),
            ("12abc 3", "15"),
            ("1e+1", "10"),
            ("1e-1", "0"),
            ("1E3", "0"),
            ("2 e", "2"),
            ("(1.5)", "(1.5)"),
            ("(0)", "0"),
            ("0.125", "0.125"),
            ("1.50", "1.5"),
            ("¥5", "¥5.00"),
            ("-¥5", "(¥5.00)"),
            ("¥-5", "0"),
            ("¥ 5", "¥5.00"),
            ("5¥", "0"),
            ("¥5%", "¥0.05"),
            ("(¥5)", "(¥5.00)"),
            ("¥(5)", "0"),
        ] {
            assert_eq!(read(text), value, "{text:?}");
        }
        // The formats of the cell carry into the result.
        let ja = |text: &str, f: &str| {
            let t = table(&[&[text, ""]]);
            shown_ja(&t, 0, 1, f)
        };
        assert_eq!(ja("¥5", "=A1*2"), "¥10.00");
        assert_eq!(ja("¥5", "=A1+1"), "¥6.00");
        assert_eq!(ja("(5)", "=A1*2"), "(10)");
        assert_eq!(ja("(5)", "=A1+10"), "5");
        assert_eq!(ja("(5)", "=-A1"), "5");
        assert_eq!(ja("(5)", "=ABS(A1)"), "5");
        assert_eq!(ja("5%", "=A1*2"), "0.1");
        assert_eq!(ja("5%", "=A1+1"), "1.05");
        assert_eq!(ja("5%", "=A1*100"), "5");
        assert_eq!(ja("1,000", "=A1*2"), "2,000");
        assert_eq!(ja("1,000", "=A1+1"), "1,001");
        assert_eq!(ja("1.50", "=A1+1"), "2.5");
        assert_eq!(ja("1.50", "=A1/3"), "0.5");
        assert_eq!(ja("0.125", "=A1*2"), "0.25");
        assert_eq!(ja("7", "=A1/8"), "0.88");
        assert_eq!(ja("7", "=A1/4"), "1.75");
        let t =
            table(&[&["(5)", "1,000", "", ""], &["¥5", "5%", "", ""], &["1,000", "(5)", "", ""], &["1,000.5", "(5)", "", ""], &["(5)", "7", "", ""]]);
        let f = |r: usize, f: &str| shown_ja(&t, r, 2, f);
        assert_eq!(f(0, "=A1+B1"), "995");
        assert_eq!(f(0, "=A1*B1"), "(5,000)");
        assert_eq!(f(1, "=A2+B2"), "¥5.05");
        assert_eq!(f(1, "=A1+A2"), "¥0.00");
        assert_eq!(f(2, "=A3+B3"), "995");
        assert_eq!(f(3, "=A4*B4"), "(5,002.5)");
        assert_eq!(f(4, "=A5*B5"), "(35)");
        assert_eq!(f(4, "=A5-B5"), "(12)");
        assert_eq!(f(1, "=A3-A2"), "¥995.00");
        assert_eq!(f(1, "=A2-A3"), "(¥995.00)");
    }

    /// Word for Mac (`fw2`, `fw3`, `fw5`–`fw7` probes): formulas in the body, their errors and
    /// their functions.
    #[test]
    fn formulas_and_their_errors_as_word_for_mac_shows_them() {
        for (f, expected) in [
            ("= 1/0", "!Zero Divide"),
            ("= 0/0", "!Zero Divide"),
            ("= 0^-1", "!Zero Divide"),
            ("= 1 +", "!Unexpected End of Formula"),
            ("= (1+2", "!Unexpected End of Formula"),
            ("= 2^", "!Unexpected End of Formula"),
            ("= -", "!Unexpected End of Formula"),
            ("= 2 3 +", "!Unexpected End of Formula"),
            ("= ABS", "!Unexpected End of Formula"),
            ("= ABS(", "!Unexpected End of Formula"),
            ("= ABS(-5", "!Unexpected End of Formula"),
            ("=", "!Unexpected End of Formula"),
            ("= 1 2", "!Missing Operator"),
            ("= 1 1.5", "!Missing Operator"),
            ("= 1.5.2", "!Missing Operator"),
            ("= 1 000", "!Missing Operator"),
            ("= 5 % 3", "!Missing Operator"),
            ("= 2(3)", "!Missing Operator"),
            ("= (3)2", "!Missing Operator"),
            ("= )", "!Syntax Error, )"),
            ("= 1 + 2 )", "!Syntax Error, )"),
            ("= 1 + * 2", "!Syntax Error, *"),
            ("= 1 << 2", "!Syntax Error, <"),
            ("= +3", "!Syntax Error, +"),
            ("= -+5", "!Syntax Error, +"),
            ("= 1000*+2", "!Syntax Error, +"),
            ("= (+5)", "!Syntax Error, +"),
            ("= 3!", "!Syntax Error, !"),
            ("= 1 @ 2", "!Syntax Error, @"),
            ("= 1 & 2", "!Syntax Error, &"),
            ("= 1 : 2", "!Syntax Error, :"),
            ("= [1]", "!Syntax Error, ["),
            ("= {1}", "!Syntax Error, {"),
            ("= \"abc\"", "!Syntax Error, \""),
            ("= 1,5", "!Syntax Error, ,"),
            ("= 1,00", "!Syntax Error, ,"),
            ("= 12,34", "!Syntax Error, ,"),
            ("= 1,2,3", "!Syntax Error, ,"),
            ("= 5%%", "!Syntax Error, %"),
            ("= 7 MOD 2", "!Syntax Error, MOD"),
            ("= FOO 1", "!Syntax Error, 1"),
            ("= 1 FOO", "!Syntax Error, FOO"),
            ("= 1 e", "!Syntax Error, E"),
            ("= 1e9", "!Syntax Error, 1E9"),
            ("= 1e1", "!Syntax Error, 1E1"),
            ("= 7e2", "!Syntax Error, 7E2"),
            ("= 1.5e1", "!Syntax Error, 1.5E1"),
            ("= 1.5e", "!Syntax Error, 1.5E"),
            ("= 1E", "!Syntax Error, 1E"),
            ("= xyz", "!Undefined Bookmark, XYZ"),
            ("= xyz+1", "!Undefined Bookmark, XYZ"),
            ("= 1+xyz", "!Undefined Bookmark, XYZ"),
            ("= pi", "!Undefined Bookmark, PI"),
            ("= e", "!Undefined Bookmark, E"),
            ("= e1", "0"),
            ("= E1", "0"),
            ("= A1", "0"),
            ("= A99", "0"),
            ("= A1+B1", "0"),
            ("= A1*2", "0"),
            ("= ABOVE", "0"),
            ("= LEFT+1", "1"),
            ("= SUM(ABOVE)", "!The Formula Not In Table"),
            ("= sum(A1)", "!The Formula Not In Table"),
            ("= sum(A1,1)", "!The Formula Not In Table"),
            ("= ROUND(A1)", "!Syntax Error, )"),
            ("= ROUND(1,A1)", "1"),
            ("= IF(1,2,A1)", "2"),
            // Functions and their arguments.
            ("= SUM()", "!Syntax Error, )"),
            ("= count()", "!Syntax Error, )"),
            ("= ABS()", "!Syntax Error, )"),
            ("= and()", "!Syntax Error, )"),
            ("= not()", "!Syntax Error, )"),
            ("= defined()", "!Syntax Error, )"),
            ("= ABS(1,2)", "!Syntax Error, ,"),
            ("= abs(1,2,3)", "!Syntax Error, ,"),
            ("= ROUND(1,2,3)", "!Syntax Error, ,"),
            ("= and(1,1,1)", "!Syntax Error, ,"),
            ("= or(0,0,1)", "!Syntax Error, ,"),
            ("= not(1,2)", "!Syntax Error, ,"),
            ("= mod(5,2,1)", "!Syntax Error, ,"),
            ("= if(1,2,3,4)", "!Syntax Error, ,"),
            ("= ROUND(1)", "!Syntax Error, 1"),
            ("= round(1.5)", "!Syntax Error, 1.5"),
            ("= IF(1)", "!Syntax Error, 1"),
            ("= if(1,2)", "!Syntax Error, )"),
            ("= or(1)", "!Syntax Error, 1"),
            ("= and(1)", "!Syntax Error, 1"),
            ("= sum(5)", "!Syntax Error, 5"),
            ("= sum(1.0)", "!Syntax Error, 1.0"),
            ("= sum(100)", "!Syntax Error, 100"),
            ("= sum(0)", "!Syntax Error, 0"),
            ("= max(0)", "!Syntax Error, 0"),
            ("= min(0)", "!Syntax Error, 0"),
            ("= average(5)", "!Syntax Error, 5"),
            ("= product(0)", "!Syntax Error, 0"),
            ("= count(0)", "!Syntax Error, 0"),
            ("= count(0.5)", "!Syntax Error, 0.5"),
            ("= count(-0)", "!Syntax Error, )"),
            ("= count((0))", "!Syntax Error, )"),
            ("= count(a)", "!Syntax Error, )"),
            ("= sum(a)", "!Syntax Error, )"),
            ("= sum(-5)", "!Syntax Error, )"),
            ("= sum((5))", "!Syntax Error, )"),
            ("= sum(5,)", "!Syntax Error, )"),
            ("= SUM(1,)", "!Syntax Error, )"),
            ("= sum(,5)", "!Syntax Error, ,"),
            ("= sum(1 2)", "!Syntax Error, )"),
            ("= sum(1;2)", "!Syntax Error, ;"),
            ("= sum(1,2;3)", "!Syntax Error, ;"),
            ("= sum(x,1)", "!Undefined Bookmark, X"),
            ("= sum(1,x)", "!Undefined Bookmark, X"),
            ("= FOO(1)", "!Syntax Error, 1"),
            ("= sqrt(2)", "!Syntax Error, 2"),
            ("= sin(0)", "!Syntax Error, 0"),
            ("= FOO()", "!Syntax Error, ("),
            ("= FOO(1,2)", "!Syntax Error, ("),
            ("= BAR(x)", "!Syntax Error, ("),
            ("= TRUE()", "!Syntax Error, ("),
            ("= true(1)", "!Syntax Error, 1"),
            ("= defined(sum)", "!Syntax Error, )"),
            ("= defined(A1:B2)", "!Syntax Error, :"),
            ("= abs(5)", "5"),
            ("= sum(1,2)", "3"),
            ("= sum(5,0)", "5"),
            ("= sum(0,5)", "5"),
            ("= sum(0,0)", "0"),
            ("= sum(0.5,0.5)", "1"),
            ("= sum(1,0.5)", "1.5"),
            ("= sum(1,(0))", "1"),
            ("= sum(1,(5))", "(4)"),
            ("= count(1,2,3)", "3"),
            ("= count(1,0)", "2"),
            ("= average(1,2)", "1.5"),
            ("= average(1,2,4)", "2.33"),
            ("= average(1,2,3,4)", "2.5"),
            ("= average(1.5,2)", "1.75"),
            ("= average(0,0)", "0.0"),
            ("= min(1,2)", "1"),
            ("= max(1,2)", "2"),
            ("= max(1,2,3)", "3"),
            ("= max(0.125,0.1)", "0.125"),
            ("= product(2,3)", "6"),
            ("= product(1,0)", "0"),
            ("= product(0.5,0.5)", "0.3"),
            ("= abs(-2.5)", "2.5"),
            ("= abs(-0.125)", "0.125"),
            ("= abs((5))", "5"),
            ("= abs(-5%)", "0.05"),
            ("= int(2.9)", "2"),
            ("= int(-2.9)", "-2"),
            ("= int(-1.5)", "-1"),
            ("= int(-0.5)", "0"),
            ("= int(2^-1)", "0"),
            ("= sign(-3)", "-1"),
            ("= sign(0)", "0"),
            ("= sign(-0)", "0"),
            ("= round(2.5,0)", "3"),
            ("= round(-2.5,0)", "-3"),
            ("= round(-0.5,0)", "-1"),
            ("= round(1.005,2)", "1.01"),
            ("= round(2.345,2)", "2.35"),
            ("= round(2.675,2)", "2.68"),
            ("= round(8.345,2)", "8.35"),
            ("= round(1.0049999,2)", "1"),
            ("= round(1.00499999999999,2)", "1"),
            ("= round(0.05,1)", "0.1"),
            ("= round(0.15,1)", "0.2"),
            ("= round(1.45,1)", "1.5"),
            ("= round(2.45,1)", "2.5"),
            ("= round(1.2345,3)", "1.235"),
            ("= round(1.50,2)", "1.5"),
            ("= round(1,2)", "1"),
            ("= round(1/3,4)", "0.3333"),
            ("= round(2/3,3)", "0.667"),
            ("= round(2.345,-1)", "0"),
            ("= round(15,-1)", "20"),
            ("= round(25,-1)", "30"),
            ("= round(123.456,-2)", "100"),
            ("= round(150,-2)", "200"),
            ("= round(250,-2)", "300"),
            ("= round(2^-1,0)", "1"),
            ("= round(1/3,2)=0.33", "1"),
            ("= mod(7,3)", "1"),
            ("= mod(-7,3)", "-1"),
            ("= mod(7,-3)", "1"),
            ("= mod(7.5,2)", "1.5"),
            ("= mod(0,1)", "0.0"),
            ("= mod(1,0)", ""),
            ("= MOD(1,0)", ""),
            ("= mod(0,0)", ""),
            ("= not(0)", "1"),
            ("= not(2)", "0"),
            ("= not(-1)", "0"),
            ("= not(0.5)", "0"),
            ("= and(1,0)", "0"),
            ("= and(2,3)", "1"),
            ("= or(1,0)", "1"),
            ("= or(0,0)", "0"),
            ("= true", "1"),
            ("= false", "0"),
            ("= TRUE", "1"),
            ("= true+true", "2"),
            ("= true*2", "2"),
            ("= true=1", "1"),
            ("= TRUE+FALSE", "1"),
            ("= defined(x)", "0"),
            ("= defined(A1)", "1"),
            ("= defined(B9)", "1"),
            ("= defined(x1)", "1"),
            ("= defined(1)", "1"),
            ("= defined((1))", "1"),
            ("= defined(1+1)", "1"),
            ("= defined(sum(1,2))", "1"),
            ("= defined(ABOVE)", "1"),
            ("= if(0.5,1,2)", "1"),
            ("= if(2,1,2)", "1"),
            ("= IF(1,1/0,2)", "!Zero Divide"),
            ("= IF(0,1/0,2)", "!Zero Divide"),
            ("= IF(1/3=0.33,1,0)", "0"),
            // Operators.
            ("= 1 = 1", "1"),
            ("= 2 > 1", "1"),
            ("= 1<2<3", "1"),
            ("= --3", "3"),
            ("= -2^2", "4"),
            ("= 2^3^2", "512"),
            ("= 1-2-3", "-4"),
            ("= 8/2/2", "2"),
            ("= 1+2*3", "7"),
            ("= (1+2)*3", "9"),
            ("= ((1))", "(1)"),
            ("= (-5)", "-5"),
            ("= (--5)", "5"),
            ("= ((-5))", "-5"),
            ("= -(5)", "5"),
            ("= -(1000)", "1000"),
            ("= -5", "-5"),
            ("= -5*2", "-10"),
            ("= 0-5", "-5"),
            ("= 2-7", "-5"),
            ("= -2.5", "-2.5"),
            ("= -1234.5", "-1234.5"),
            ("= -1000", "-1000"),
            ("= 1000*-2", "-2000"),
            ("= -1000*1.5", "-1500"),
            ("= -0", "0"),
            ("= 5 %", "0.05"),
            ("= 50%*2", "1"),
            ("= 10%", "0.1"),
            ("= 12.5%", "0.125"),
            ("= 2.5%", "0.025"),
            ("= 0.5%", "0.005"),
            ("= 100%", "1"),
            ("= 1/3%", "33.33"),
            ("= 7%*100", "7"),
            ("= 10%+1", "1.1"),
            ("= 5%*5%", "0.0"),
            ("= 5%^2", "0.0"),
            ("= 2^5%", "1.04"),
            ("= 1/5%", "20"),
            ("= -5%", "-0.05"),
            ("= (5%)", "(0.05)"),
            ("= (5)%", "(0.05)"),
            ("= 1,000%", "10"),
            ("= 2^0.5", "1.4"),
            ("= 2^0.50", "1.41"),
            ("= 2^0.500", "1.414"),
            ("= 2^(1/2)", "1.41"),
            ("= 2^0.25", "1.19"),
            ("= 2^0.75", "1.68"),
            ("= 2^0.1", "1.1"),
            ("= 3^0.5", "1.7"),
            ("= 9^0.5", "3"),
            ("= 4^0.5", "2"),
            ("= 10^0.5", "3.2"),
            ("= 4^0.5^2", "1.4"),
            ("= 8^(1/3)", "2"),
            ("= 8^0.333", "1.999"),
            ("= 0^0.5", "0.0"),
            ("= 0^2", "0"),
            ("= 2^0", "1"),
            ("= 0^0", "1"),
            ("= 2^10", "1024"),
            ("= 2^1.0", "2"),
            ("= (-8)^1.0", "-8"),
            ("= (-2)^2", "4"),
            ("= (-2)^3", "-8"),
            ("= -1^0.5", "!Exponent Not an Integer"),
            ("= (-1)^0.5", "!Exponent Not an Integer"),
            ("= (-2)^0.5", "!Exponent Not an Integer"),
            ("= (-8)^(1/3)", "!Exponent Not an Integer"),
            ("= 2^-1", "0.5"),
            ("= 2^-2", "0.25"),
            ("= 2^-3", "0.125"),
            ("= 2^-10", "0.00097656"),
            ("= 3^-1", "0.33333333"),
            ("= 5^-1", "0.2"),
            ("= 10^-1", "0.1"),
            ("= 10^-2", "0.01"),
            ("= 10^-3", "0.001"),
            ("= 10^-4", "0.0001"),
            ("= 10^-5", "0.00001"),
            ("= 2^-0.5", "0.70710678"),
            ("= 4^-0.5", "0.5"),
            ("= 1/2^0.5", "0.71"),
            ("= 1/2^1", "0.5"),
            ("= 2^(0-1)", "0.5"),
            ("= 1.5*2^-1", "0.75"),
            ("= 0.5+2^-1", "1"),
            ("= 2^-1=0.5", "1"),
            ("= 7/2^-1", "14"),
            ("= 1/10^2", "0.01"),
            ("= 1/10^5", "0.0"),
            ("= 2^0.5*2^0.5", "2"),
            ("= 1,000^0.5", "31.6"),
            ("= 1,000^2", "1,000,000"),
            ("= (5)^2", "25"),
            ("= (5)*(5)", "25"),
            // Comparisons hold to a relative tolerance.
            ("= 0.1+0.2=0.3", "1"),
            ("= 0.1+0.2>0.3", "0"),
            ("= 0.1+0.2>=0.3", "1"),
            ("= 0.1+0.2<=0.3", "1"),
            ("= 1/3=0.33", "0"),
            ("= 2/3>0.66", "1"),
            ("= 0.5=1/2", "1"),
            ("= 0.57*100=57", "1"),
            ("= 1=1.00000000000001", "0"),
            ("= 1/3=0.333333333333333", "1"),
            ("= 1/3=0.33333333333333", "0"),
            ("= 0.1+0.2-0.3=0", "0"),
            ("= 1.0000000000000002=1", "1"),
            // Switches.
            ("= 1 \\# \"0.00\" \\# \"0\"", "Error! Too many picture switches defined."),
            ("= 1 \\* roman \\# \"0.00\"", "Error! Picture switch must be first formatting switch."),
            ("= 1 \\#", "Error! Switch argument not specified."),
            ("= 5 \\* Thai1", "Error! Unknown switch argument."),
            ("= 1 \\# \"0.00\" \\* roman", "i"),
            ("= 12.5 \\* roman", "xiii"),
            ("= 12 \\# 0.00", "12.00"),
            ("= 1234.5 \\# \"#,##0.00\"", "1,234.50"),
            ("= 1234.5 \\# \"#,##0\"", "1,235"),
            ("= 1234.5 \\# \"＃，＃＃０．００\"", "＃，＃＃０．００"),
            ("= 1 \\# \"\"", ""),
            ("= 1 \\# x", "1"),
            ("= 1 \\# abc", "abc"),
            ("= 1 \\# \"abc\"", "abc"),
            ("= 1 \\# \"0 abc\"", "1 abc"),
            ("= 1 \\# 0abc", "1abc"),
            ("= 1,000 \\# \"0\"", "1000"),
            ("= (5) \\# \"0\"", "-5"),
            ("= (5) \\# \"0;(0)\"", "(5)"),
            ("= (5) \\# \"0;-0\"", "-5"),
            // Full-width input, as Japanese editing produces it.
            ("= １２ + ３", "!Undefined Bookmark, １２"),
            ("= 12 ＋ 3", "!Syntax Error, ＋"),
            ("= １２×３", "!Undefined Bookmark, １２×３"),
            ("=（１＋２）＊３", "!Undefined Bookmark, （１＋２）＊３"),
            ("= ABS(−５)", "!Undefined Bookmark, −５"),
            ("=SUM(１,２)", "!Undefined Bookmark, １"),
            ("= ２ ＞ １", "!Syntax Error, ＞"),
            ("= ＡＢＳ(-3)", "!Syntax Error, ("),
            ("= Ａ1", "!Undefined Bookmark, Ａ1"),
            ("= 12 ／ 5", "!Syntax Error, ／"),
            ("= ⑫ + 1", "!Undefined Bookmark, ⑫"),
        ] {
            assert_eq!(body(f), expected, "{f}");
        }
    }

    /// Word for Mac (`fw4`–`fw7` probes): the decimals, digits and formats a result shows.
    #[test]
    fn results_show_their_operands_decimals_and_formats_as_word_for_mac() {
        for (f, expected) in [
            ("= 1/3", "0.33"),
            ("= 2/3", "0.67"),
            ("= 1/7", "0.14"),
            ("= 100/7", "14.29"),
            ("= 1000000/7", "142857.14"),
            ("= 1.5*1.5", "2.3"),
            ("= 2.5/2", "1.25"),
            ("= 0.5+0.25", "0.75"),
            ("= 1/4", "0.25"),
            ("= 1/8", "0.13"),
            ("= 1/16", "0.06"),
            ("= 3/8", "0.38"),
            ("= 1.0/3", "0.33"),
            ("= 1.00/3", "0.33"),
            ("= 1/3.0", "0.33"),
            ("= 1/3.000", "0.333"),
            ("= 10/4", "2.5"),
            ("= 20/3", "6.67"),
            ("= 1/9", "0.11"),
            ("= 0.1+0.2", "0.3"),
            ("= 0.1+0.2+0.3", "0.6"),
            ("= 1.23+1", "2.23"),
            ("= 1.2345+1", "2.2345"),
            ("= 1.23456+1", "2.23456"),
            ("= 1/3+1.2345", "1.5678"),
            ("= 1.5/7", "0.21"),
            ("= 1.55/7", "0.22"),
            ("= 0.1*0.1", "0.0"),
            ("= 0.5*0.5", "0.3"),
            ("= 0.25*0.25", "0.06"),
            ("= 1.5^2", "2.3"),
            ("= 1.5^3", "3.4"),
            ("= 0.5^10", "0.0"),
            ("= 100*1.1", "110"),
            ("= 1.1*1.1", "1.2"),
            ("= 10/3", "3.33"),
            ("= 1000/8", "125"),
            ("= 1/1024", "0.0"),
            ("= 1/0.5", "2"),
            ("= 1/0.25", "4"),
            ("= 1/0.125", "8"),
            ("= 0.001/1", "0.001"),
            ("= 0.0001", "0.0001"),
            ("= 0.00001", "0.00001"),
            ("= 1/1000", "0.0"),
            ("= 1/10000", "0.0"),
            ("= 2/1000", "0.0"),
            ("= 5/1000", "0.01"),
            ("= 5/10000", "0.0"),
            ("= 1.4999/1", "1.4999"),
            ("= 1.005*1", "1.005"),
            ("= 1.015*1", "1.015"),
            ("= 2.675*1", "2.675"),
            ("= 0.125*1", "0.125"),
            ("= 1/7*7", "1"),
            ("= 1/3*3", "1"),
            ("= sum(0.1,0.2)", "0.3"),
            ("= 1/3*0.1", "0.03"),
            ("= 0.1/3", "0.03"),
            ("= 0.01/3", "0.0"),
            ("= 0.001/3", "0.0"),
            ("= 123.456*1000", "123456"),
            ("= 123.456/1000", "0.123"),
            ("= 1.23456789*1", "1.23456789"),
            ("= 0.1*7", "0.7"),
            ("= 0.7/7", "0.1"),
            ("= 1.1-1", "0.1"),
            ("= 1.10-1", "0.1"),
            ("= 1.100-1", "0.1"),
            ("= 3.0", "3"),
            ("= 3.00", "3"),
            ("= 3.000+0", "3"),
            ("= 2.50*2", "5"),
            ("= 2.5*2", "5"),
            ("= 0.5*2", "1"),
            ("= 7/2", "3.5"),
            ("= 7/2.0", "3.5"),
            ("= 3/4", "0.75"),
            ("= 3/4*4", "3"),
            ("= 3/4+0.25", "1"),
            ("= 0.75+0.5", "1.25"),
            ("= 1-0.50", "0.5"),
            ("= 2-1.50", "0.5"),
            ("= 0.50", "0.5"),
            ("= 2.50", "2.5"),
            ("= 1.5+1.50", "3"),
            ("= 1.5+1.500", "3"),
            ("= 1.5*1.50", "2.25"),
            ("= 1.5-1.50", "0.0"),
            ("= 0.5000+0", "0.5"),
            ("= 1.75000000000000", "1.75"),
            ("= 123.4500", "123.45"),
            ("= 0.005", "0.005"),
            ("= 0.015", "0.015"),
            ("= 2.675", "2.675"),
            ("= 1.005", "1.005"),
            ("= -0.004", "-0.004"),
            ("= 123456.789", "123456.789"),
            ("= 0.1", "0.1"),
            ("= .1", "0.1"),
            ("= 1.", "1"),
            ("= 1.0", "1"),
            ("= 01", "1"),
            ("= 0.0", "0.0"),
            ("= 0.0+0", "0.0"),
            ("= 0*1.5", "0.0"),
            ("= -1/1000", "0.0"),
            ("= 0.1+0.2-0.3", "0.0"),
            ("= 1/3-0.33333333333333", "0.0"),
            ("= 1e-7", "0"),
            ("= 1e+1", "10"),
            ("= 1e-400", "0"),
            ("= 0.6*0.7", "0.4"),
            ("= 0.07*100", "7"),
            ("= 100*0.07", "7"),
            ("= 0.57*100", "57"),
            ("= 1.15*100", "115"),
            ("= 4.35*100", "435"),
            ("= 1.005*100", "100.5"),
            ("= 8.675*100", "867.5"),
            ("= 1.00499999999999*1", "1.005"),
            ("= 1.0000000000000002", "1"),
            // Fourteen significant digits.
            ("= 123456789012", "123456789012"),
            ("= 1000000000*1000000000", "1000000000000000000"),
            ("= 10^15", "1000000000000000"),
            ("= 9999999999999999", "10000000000000000"),
            ("= 9999999999999999+1", "10000000000000000"),
            ("= 12345678901234567890", "12345678901235000000"),
            ("= 2^62", "4611686018427400000"),
            ("= 2^64", "18446744073710000000"),
            ("= 2^100", "1267650600228200000000000000000"),
            ("= 10^22", "10000000000000000000000"),
            ("= 10^60", "1000000000000000000000000000000000000000000000000000000000000"),
            ("= 10^65", "!Number Too Large To Format"),
            ("= 10^99", "!Number Too Large To Format"),
            ("= 10^400", "0"),
            ("= -10^400", "0"),
            ("= 10^308*10", "0"),
            ("= 1/10^400", "0.0"),
            ("= 10^400/10^400", "0.00"),
            ("= 1/3*1000000000", "333333333.33"),
            ("= 1/3*10000000000", "3333333333.33"),
            ("= 0.1*10000000000", "1000000000"),
            ("= 1234567890123/10", "123456789012.3"),
            ("= 1234567890123/100", "12345678901.23"),
            ("= 1/3*1000000000000000", "333333333333330"),
            ("= 1/7*10000000000000", "1428571428571.4"),
            ("= 100000000000000.1", "100000000000000"),
            ("= 12345678.12345678", "12345678.123457"),
            ("= 123456789012.12345", "123456789012.12"),
            ("= 1234567890123456.1", "1234567890123500"),
            // Thousands separators and parentheses carry into the result.
            ("= 1,000.5", "1,000.5"),
            ("= 1,000,000", "1,000,000"),
            ("= 1,0000", "10,000"),
            ("= sum(1,000,2)", "1,002"),
            ("= 1,000+1", "1,001"),
            ("= 1,000*1,000", "1,000,000"),
            ("= 1,000/3", "333.33"),
            ("= 1,000.50*1", "1,000.5"),
            ("= 12,345.678", "12,345.678"),
            ("= 12,345.678*1", "12,345.678"),
            ("= 1,234,567.891", "1,234,567.891"),
            ("= -1,000", "-1,000"),
            ("= (1,000)", "(1,000)"),
            ("= (1,000)*2", "(2,000)"),
            ("= (1,000)+1,000", "0"),
            ("= -1,000+1,000", "0"),
            ("= (5)", "(5)"),
            ("= (1)", "(1)"),
            ("= (1.5)", "(1.5)"),
            ("= (0)", "0"),
            ("= (5)*2", "(10)"),
            ("= 2*(5)", "(10)"),
            ("= ((5))*2", "(10)"),
            ("= -(5)*2", "10"),
            ("= (5)+10", "5"),
            ("= (5)+1", "(4)"),
            ("= 1+(5)", "(4)"),
            ("= (5)+(5)", "(10)"),
            ("= (5)-(5)", "0"),
            ("= (5)/2", "(2.5)"),
            ("= (10)/4", "(2.5)"),
        ] {
            assert_eq!(body(f), expected, "{f}");
        }
    }

    /// Word for Mac (`fw2`, `fw6` probes): cells, keywords and ranges in a table.
    #[test]
    fn table_references_as_word_for_mac() {
        let t = table(&[&["0", "5", "", ""], &["2", "x", "", ""], &["3", "", "", ""], &["", "", "", ""]]);
        assert_eq!(shown(&t, 0, 2, "=B1/A1"), "!Zero Divide");
        assert_eq!(shown(&t, 0, 3, "=SUM(ABOVE)"), "!Table Index Cannot be Zero");
        assert_eq!(shown(&t, 0, 3, "=COUNT(ABOVE)"), "!Table Index Cannot be Zero");
        assert_eq!(shown(&t, 0, 0, "=SUM(LEFT)"), "!Table Index Cannot be Zero");
        assert_eq!(shown(&t, 1, 2, "=A99"), "!A99 Is Not In Table");
        assert_eq!(shown(&t, 1, 2, "=E1"), "!E1 Is Not In Table");
        assert_eq!(shown(&t, 1, 3, "=A1:A2"), "!Syntax Error, :");
        assert_eq!(shown(&t, 2, 2, "=C1"), "0", "an empty cell is 0");
        assert_eq!(shown(&t, 1, 2, "=B2"), "0", "a cell holding text is 0");
        assert_eq!(shown(&t, 2, 2, "=SUM(ABOVE)"), "0", "nothing above holds a number");
        assert_eq!(shown(&t, 3, 1, "=AVERAGE(B1:B3)"), "5", "only the number counts");
        assert_eq!(shown(&t, 3, 0, "=COUNT(ABOVE)"), "3");
        assert_eq!(shown(&t, 0, 2, "=DEFINED(A1)"), "1");
        assert_eq!(shown(&t, 0, 2, "=DEFINED(B2)"), "1", "a text cell is in the table");
        assert_eq!(shown(&t, 0, 2, "=DEFINED(A9)"), "0");
        assert_eq!(shown(&t, 1, 2, "=DEFINED(ABOVE)"), "!Syntax Error, ,");
        assert_eq!(shown(&t, 3, 1, "=SUM(ABOVE,1)"), "6", "the text and the blank are skipped up to the 5");
        assert_eq!(shown(&t, 3, 0, "=SUM(1,ABOVE)"), "6");
        let t = table(&[&["1", "2", "3"], &["4", "5", "6"], &["", "", ""], &["", "", ""], &["", "", ""], &["7", "8", "9"]]);
        assert_eq!(shown(&t, 2, 0, "=SUM(A1:B2)"), "12");
        assert_eq!(shown(&t, 2, 1, "=SUM(B1:C2)"), "16");
        assert_eq!(shown(&t, 2, 2, "=SUM(A1:C1)"), "6");
        assert_eq!(shown(&t, 3, 0, "=SUM(A1:A99)"), "12", "the range stops at the table's edge");
        assert_eq!(shown(&t, 3, 1, "=SUM(A1:Z1)"), "6");
        assert_eq!(shown(&t, 3, 2, "=SUM(C1:A1)"), "6", "either way round");
        assert_eq!(shown(&t, 4, 0, "=SUM(B2:A1)"), "12");
        assert_eq!(shown(&t, 4, 2, "=SUM(A1:B1,C1)"), "6");
        assert_eq!(shown(&t, 4, 2, "=AVERAGE(A1:C2)"), "3.5");
        assert_eq!(shown(&t, 4, 0, "=SUM(LEFT)"), "!Table Index Cannot be Zero");
        assert_eq!(shown(&t, 4, 1, "=SUM(RIGHT)"), "0");
        assert_eq!(shown(&t, 4, 2, "=SUM(BELOW)"), "9");
        assert_eq!(shown(&t, 2, 2, "=A1:B2+1"), "!Syntax Error, :");
        let t = table(&[&["x", "y"], &["", ""], &["", ""]]);
        assert_eq!(shown(&t, 2, 0, "=MAX(ABOVE)"), "0");
        assert_eq!(shown(&t, 2, 1, "=MIN(ABOVE)"), "0");
        assert_eq!(shown(&t, 2, 0, "=AVERAGE(ABOVE)"), "0.0");
        assert_eq!(shown(&t, 2, 0, "=COUNT(ABOVE)"), "0");
        assert_eq!(shown(&t, 2, 0, "=PRODUCT(ABOVE)"), "0");
    }

    #[test]
    fn unknown_functions_are_errors_not_sums() {
        let t = table(&[&["1"], &["4"], &["7"], &[""]]);
        assert_eq!(shown(&t, 3, 0, "=MEDIAN(ABOVE)"), "!Syntax Error, (");
        assert!(ev(&t, 3, 0, "=MEDIAN(ABOVE)").is_err());
    }

    #[test]
    fn ranges_and_references_and_operators() {
        let t = table(&[&["1", "2"], &["3", "4"], &["", ""]]);
        assert_eq!(ev(&t, 2, 0, "=SUM(A1:B2)"), Ok(10.0));
        assert_eq!(ev(&t, 2, 1, "=A1*B2+1"), Ok(5.0));
        assert_eq!(ev(&t, 2, 0, "=(A1+A2)/2"), Ok(2.0));
        assert_eq!(ev(&t, 2, 0, "=2^3-(-1)"), Ok(9.0));
        assert_eq!(ev(&t, 2, 0, "=AVERAGE(A1:B2)"), Ok(2.5));
        assert_eq!(ev(&t, 2, 0, "=MAX(A1:B2)"), Ok(4.0));
        assert_eq!(ev(&t, 2, 0, "=ROUND(2.5, 0)"), Ok(3.0));
        assert_eq!(ev(&t, 2, 0, "=MOD(7, 3)"), Ok(1.0));
    }

    #[test]
    fn division_by_zero_and_bad_syntax_are_errors() {
        let t = table(&[&["1"], &["0"]]);
        assert_eq!(ev(&t, 1, 0, "=A1/A2"), Err(CmdError::Failed("!Zero Divide".into())));
        assert!(ev(&t, 1, 0, "=SUM(A1").is_err());
        assert!(ev(&t, 1, 0, "=A1 A1").is_err());
        assert!(ev(&t, 1, 0, "=A1:").is_err());
        assert!(ev(&t, 1, 0, "=SUM()").is_err());
        assert!(ev(&t, 1, 0, &format!("={}", "(".repeat(64))).is_err(), "deep nesting is refused");
    }

    #[test]
    fn numbers_read_currency_and_separators_but_not_letters() {
        assert_eq!(parse_number("$1,200", None), Some(1200.0));
        assert_eq!(parse_number(" 12% ", None), Some(12.0));
        assert_eq!(parse_number("(5)", None), Some(-5.0));
        assert_eq!(parse_number("x9", None), None);
        assert_eq!(parse_number("1a", None), None);
        assert_eq!(parse_number("inf", None), None);
        assert_eq!(parse_number("", None), None);
        assert_eq!(parse_number("3,50", Some("pl-PL")), Some(3.5));
    }

    #[test]
    fn comparisons_give_one_or_zero_and_chain_from_the_left() {
        let t = table(&[&["5"], &["3"]]);
        assert_eq!(ev(&t, 1, 0, "=A1>A2"), Ok(1.0));
        assert_eq!(ev(&t, 1, 0, "=A1<=A2"), Ok(0.0));
        assert_eq!(ev(&t, 1, 0, "=A1<>A2"), Ok(1.0));
        assert_eq!(ev(&t, 1, 0, "=A2>=3"), Ok(1.0));
        assert_eq!(ev(&t, 1, 0, "=A1=5"), Ok(1.0));
        assert_eq!(ev(&t, 1, 0, "=A1<A2<1"), Ok(1.0), "(5<3)<1 is 0<1");
    }

    #[test]
    fn if_and_or_not_and_the_truth_values() {
        let t = table(&[&["5", "x"], &["3", ""]]);
        assert_eq!(ev(&t, 1, 0, "=IF(A1>A2, 10, 20)"), Ok(10.0));
        assert_eq!(ev(&t, 1, 0, "=IF(A1<A2, 10, 20)"), Ok(20.0));
        assert_eq!(ev(&t, 1, 0, "=IF(A1>A2, 1, B1+1)"), Ok(1.0), "text is 0, so B1+1 is 1 either way");
        assert_eq!(ev(&t, 1, 0, "=AND(A1>1, A2>1)"), Ok(1.0));
        assert_eq!(ev(&t, 1, 0, "=AND(A1>1, A2>4)"), Ok(0.0));
        assert_eq!(ev(&t, 1, 0, "=OR(A1<1, A2>1)"), Ok(1.0));
        assert_eq!(ev(&t, 1, 0, "=OR(A1<1, A2<1)"), Ok(0.0));
        assert_eq!(ev(&t, 1, 0, "=NOT(A1<1)"), Ok(1.0));
        assert_eq!(ev(&t, 1, 0, "=TRUE"), Ok(1.0));
        assert_eq!(ev(&t, 1, 0, "=FALSE"), Ok(0.0));
        assert!(ev(&t, 1, 0, "=TRUE(1)").is_err());
        assert!(ev(&t, 1, 0, "=IF(A1>A2, 1)").is_err(), "IF takes its three arguments");
        assert!(ev(&t, 1, 0, "=AND()").is_err());
    }

    #[test]
    fn defined_says_whether_a_cell_is_in_the_table() {
        let t = table(&[&["5", "x", ""]]);
        assert_eq!(ev(&t, 0, 0, "=DEFINED(A1)"), Ok(1.0));
        assert_eq!(ev(&t, 0, 0, "=DEFINED(B1)"), Ok(1.0), "a text cell is in the table");
        assert_eq!(ev(&t, 0, 0, "=DEFINED(D1)"), Ok(0.0), "a cell past the table isn't");
        assert_eq!(ev(&t, 0, 0, "=IF(DEFINED(D1), D1, 7)"), Err(CmdError::Failed("!D1 Is Not In Table".into())), "both branches are worked out");
    }

    #[test]
    fn a_numeric_picture_formats_the_result() {
        assert_eq!(format_picture(1234.5, "#,##0.00"), "1,234.50");
        assert_eq!(format_picture(0.256, "0%"), "26%");
        assert_eq!(format_picture(2.0, "#.##"), "2");
        assert_eq!(format_picture(0.05, "0.0"), "0.1", "halves round away from zero");
        assert_eq!(format_picture(-5.0, "$#,##0"), "-$5", "one section: a minus in front");
        assert_eq!(format_picture(-5.0, "$#,##0.00;($#,##0.00)"), "($5.00)", "the second section is the negative form");
        assert_eq!(format_picture(0.0, "0.00;-;zero"), "zero");
        assert_eq!(format_picture(7.0, "000"), "007");
        assert_eq!(format_picture(-0.001, "0.00"), "0.00", "a rounded zero has no sign");
    }

    #[test]
    fn a_field_switch_picks_the_picture_and_the_formula_stays_apart_from_it() {
        let t = table(&[&["1234.5"], &[""]]);
        assert_eq!(split_switch("=A1 \\# \"0.00\""), ("=A1", Some("\\# \"0.00\"")));
        assert_eq!(split_switch("=A1"), ("=A1", None));
        assert_eq!(split_switch("=A1 \\* DBNum2"), ("=A1", Some("\\* DBNum2")));
        assert_eq!(split_switch("=A1 \\* MERGEFORMAT \\# 0"), ("=A1", Some("\\* MERGEFORMAT \\# 0")));
        assert_eq!(shown(&t, 1, 0, "=A1 \\# \"#,##0.00\""), "1,234.50");
        assert_eq!(shown(&t, 1, 0, "=A1"), "1234.5");
        assert_eq!(shown(&t, 1, 0, "=A1 \\# 0"), "1235", "a bare picture word");
        assert_eq!(shown(&t, 1, 0, "=A1 \\* MERGEFORMAT \\# 0"), "Error! Picture switch must be first formatting switch.");
        assert_eq!(shown(&t, 1, 0, "=A1 \\# \"\""), "", "an empty picture shows nothing");
        assert_eq!(shown(&t, 1, 0, "=A1 \\* DBNum2"), "千二百三十五");
    }

    #[test]
    fn a_huge_range_over_a_small_table_reads_only_the_table() {
        let t = table(&[&["1", "2", "3"], &["4", "5", "6"], &["", "", ""]]);
        // Forty ranges reaching to the last possible row and column would be about 80 million
        // grid positions; clamped to the table they are 6 numbers each, well inside the budget.
        let f = format!("=SUM({})", ["A1:BK32767"; 40].join(","));
        assert!(f.len() <= MAX_FORMULA);
        assert_eq!(ev(&t, 2, 0, &f).unwrap(), 21.0 * 40.0);
    }

    #[test]
    fn an_evaluation_past_the_cell_budget_is_an_error() {
        let mut t = Table::new(1000, 10, 468.0);
        t.rows[0].cells[0] = Cell::with_text("1");
        // Each range reads 10,000 cells, so eleven of them go past the 100,000-cell budget.
        let ok = format!("=SUM({})", ["A1:J1000"; 10].join(","));
        assert_eq!(ev(&t, 0, 1, &ok).unwrap(), 10.0);
        let too_much = format!("=SUM({})", ["A1:J1000"; 11].join(","));
        let err = ev(&t, 0, 1, &too_much).unwrap_err();
        assert_eq!(err, CmdError::Failed("Formula too complex".into()));
        assert_eq!(evaluate_field(&t, 0, 1, &too_much).unwrap_err().to_string(), "Formula too complex");
    }

    #[test]
    fn deep_power_and_sign_chains_are_rejected() {
        let t = table(&[&["1"]]);
        assert_eq!(ev(&t, 0, 0, "=2^3^2").unwrap(), 512.0);
        assert_eq!(ev(&t, 0, 0, "=--2").unwrap(), 2.0);
        for f in [format!("={}1", "1^".repeat(40)), format!("={}1", "-".repeat(40)), format!("={}1", "(".repeat(40))] {
            let err = ev(&t, 0, 0, &f).unwrap_err();
            assert!(err.to_string().contains("nested too deeply"), "{f}: {err}");
        }
        // A cell's text is read with the same limits: too deep is unreadable, so 0.
        let deep = format!("{}1", "(".repeat(40));
        let t = table(&[&[deep.as_str(), ""]]);
        assert_eq!(shown(&t, 0, 1, "=A1"), "0");
        let deep = format!("{}1", "-".repeat(100));
        let t = table(&[&[deep.as_str(), ""]]);
        assert_eq!(shown(&t, 0, 1, "=A1"), "0");
    }

    /// Polish text: a comma is the decimal separator, a space groups thousands, `;` separates
    /// arguments, and the result is written with a decimal comma.
    #[test]
    fn comma_languages_read_and_write_their_separators() {
        let t = table(&[&["3,50"], &["2,25"], &["1 250,00"], &[""]]);
        let pl = |f: &str| evaluate_field_with(&t, None, Some("pl-PL"), 3, 0, f).unwrap();
        assert_eq!(pl("=SUM(ABOVE)"), "1 255,75");
        assert_eq!(pl("=A1+A2"), "5,75");
        assert_eq!(pl("=ROUND(A1;1)"), "3,5");
        assert_eq!(pl("=1,5*2"), "3");
        assert_eq!(pl("=1,5"), "1,5");
        assert_eq!(pl("=SUM(1,5;2)"), "3,5");
    }
}

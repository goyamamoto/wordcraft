use super::color::{Rgb, TextColor};
use super::para_props::Border;
use super::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub enum Underline {
    #[default]
    None,
    Single,
    Words,
    Double,
    Thick,
    Dotted,
    DottedHeavy,
    Dash,
    DashedHeavy,
    DashLong,
    DashLongHeavy,
    DotDash,
    DashDotHeavy,
    DotDotDash,
    DashDotDotHeavy,
    Wave,
    WavyHeavy,
    /// Named as OOXML names it in JSON (`wavyDouble`); `doubleWave` is read for old callers.
    #[serde(rename = "wavyDouble", alias = "doubleWave")]
    DoubleWave,
}

impl Underline {
    /// Every style, in the order of Word's Font dialog list.
    pub const ALL: [Underline; 18] = [
        Underline::None,
        Underline::Single,
        Underline::Words,
        Underline::Double,
        Underline::Thick,
        Underline::Dotted,
        Underline::DottedHeavy,
        Underline::Dash,
        Underline::DashedHeavy,
        Underline::DashLong,
        Underline::DashLongHeavy,
        Underline::DotDash,
        Underline::DashDotHeavy,
        Underline::DotDotDash,
        Underline::DashDotDotHeavy,
        Underline::Wave,
        Underline::WavyHeavy,
        Underline::DoubleWave,
    ];
    /// Word's heavy styles: the thick line and the heavy patterns, drawn at the thick line's
    /// width.
    pub fn is_heavy(self) -> bool {
        matches!(
            self,
            Underline::Thick
                | Underline::DottedHeavy
                | Underline::DashedHeavy
                | Underline::DashLongHeavy
                | Underline::DashDotHeavy
                | Underline::DashDotDotHeavy
        )
    }
    /// The OOXML `w:u/@w:val` name.
    pub fn ooxml(self) -> &'static str {
        match self {
            Underline::None => "none",
            Underline::Single => "single",
            Underline::Words => "words",
            Underline::Double => "double",
            Underline::Thick => "thick",
            Underline::Dotted => "dotted",
            Underline::DottedHeavy => "dottedHeavy",
            Underline::Dash => "dash",
            Underline::DashedHeavy => "dashedHeavy",
            Underline::DashLong => "dashLong",
            Underline::DashLongHeavy => "dashLongHeavy",
            Underline::DotDash => "dotDash",
            Underline::DashDotHeavy => "dashDotHeavy",
            Underline::DotDotDash => "dotDotDash",
            Underline::DashDotDotHeavy => "dashDotDotHeavy",
            Underline::Wave => "wave",
            Underline::WavyHeavy => "wavyHeavy",
            Underline::DoubleWave => "wavyDouble",
        }
    }
    /// The style of an OOXML `w:u/@w:val` name (an unknown one is a single underline).
    pub fn from_ooxml(s: &str) -> Underline {
        Underline::ALL.iter().copied().find(|u| u.ooxml() == s).unwrap_or(Underline::Single)
    }
}

/// Word's emphasis mark (傍点, `w:em`): a mark drawn above each character (below for
/// `UnderDot`) of East Asian text.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub enum EmphasisMark {
    #[default]
    None,
    /// Filled dot above (Word: "over solid circle").
    Dot,
    /// A 、-shaped mark above (Word: "over comma").
    Comma,
    /// Ring above (Word: "over white circle").
    Circle,
    /// Filled dot below (Word: "under solid circle").
    UnderDot,
}

impl EmphasisMark {
    pub const ALL: [EmphasisMark; 5] = [EmphasisMark::None, EmphasisMark::Dot, EmphasisMark::Comma, EmphasisMark::Circle, EmphasisMark::UnderDot];
    /// The OOXML `w:em/@w:val` name.
    pub fn ooxml(self) -> &'static str {
        match self {
            EmphasisMark::None => "none",
            EmphasisMark::Dot => "dot",
            EmphasisMark::Comma => "comma",
            EmphasisMark::Circle => "circle",
            EmphasisMark::UnderDot => "underDot",
        }
    }
    pub fn from_ooxml(s: &str) -> Option<EmphasisMark> {
        EmphasisMark::ALL.iter().copied().find(|m| m.ooxml() == s)
    }
    /// Whether the mark sits below the text (else above).
    pub fn below(self) -> bool {
        self == EmphasisMark::UnderDot
    }
}

/// Word's 均等割り付け (`w:fitText`): the run is spread (or squeezed) to exactly `width` points.
/// Runs with the same `id` form one block; Word splits a block into several runs when it also
/// writes the spacing it computed.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct FitText {
    /// Width in points.
    pub width: f32,
    /// Block id (`w:id`), 0 when none.
    #[serde(default)]
    pub id: i32,
}

/// Brackets around a two-lines-in-one block (割注, `w:combineBrackets`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub enum CombineBrackets {
    #[default]
    None,
    Round,
    Square,
    Angle,
    Curly,
}

impl CombineBrackets {
    pub const ALL: [CombineBrackets; 5] =
        [CombineBrackets::None, CombineBrackets::Round, CombineBrackets::Square, CombineBrackets::Angle, CombineBrackets::Curly];
    pub fn ooxml(self) -> &'static str {
        match self {
            CombineBrackets::None => "none",
            CombineBrackets::Round => "round",
            CombineBrackets::Square => "square",
            CombineBrackets::Angle => "angle",
            CombineBrackets::Curly => "curly",
        }
    }
    pub fn from_ooxml(s: &str) -> Option<CombineBrackets> {
        CombineBrackets::ALL.iter().copied().find(|b| b.ooxml() == s)
    }
    /// The bracket characters drawn at full size before and after the block (Word draws the
    /// ASCII brackets, as observed in Word for Mac).
    pub fn chars(self) -> Option<(char, char)> {
        match self {
            CombineBrackets::None => None,
            CombineBrackets::Round => Some(('(', ')')),
            CombineBrackets::Square => Some(('[', ']')),
            CombineBrackets::Angle => Some(('<', '>')),
            CombineBrackets::Curly => Some(('{', '}')),
        }
    }
}

/// Word's Asian layout of a run (`w:eastAsianLayout`): 縦中横 (`vert`: the run lies horizontal
/// in vertical text) or 割注 (`combine`: the run is set as two half-size lines in one).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[serde(default, rename_all = "camelCase")]
pub struct EastAsianLayout {
    /// Block id (`w:id`), 0 when none.
    pub id: i32,
    /// Horizontal in vertical (縦中横).
    pub vert: bool,
    /// Compress the horizontal-in-vertical run to the line width (`w:vertCompress`); kept for
    /// the file, Word for Mac draws the run at its natural width either way (observed).
    pub vert_compress: bool,
    /// Two lines in one (割注).
    pub combine: bool,
    pub combine_brackets: CombineBrackets,
    /// Combine characters (組み文字): up to six characters set as two half-size lines in one
    /// character cell. Word keeps this as an `EQ \o(\s\up …(…),\s\do …(…))` field whose code
    /// runs are half size; WordCraft keeps the text and this flag, and writes Word's field.
    pub combine_chars: bool,
}

impl EastAsianLayout {
    /// Nothing set: Word writes nothing.
    pub fn is_none(&self) -> bool {
        !self.vert && !self.combine && !self.combine_chars
    }
    /// Word's limit for Combine Characters.
    pub const COMBINE_CHARS_MAX: usize = 6;
}

/// The OpenType ligatures a run asks for ([MS-DOCX] `w14:ligatures`; Font › Advanced › OpenType
/// Features › Ligatures): standard (`liga`), contextual (`clig`), historical (`hlig`) and
/// discretional (`dlig`). Word for Mac shapes a run that asks for no OpenType feature at all —
/// the default of every document that never set one — by its legacy text path: no ligatures or
/// contextual forms, and Kerning for fonts takes the pairs of the font's `kern` table only. A run
/// that asks for one is shaped by OpenType: GSUB ligatures of the kinds asked for, GPOS kerning
/// (measured 2026-10-19, `kn1`–`kn5` probes; see `layout::para::shaping_features`).
///
/// Serialised as its `w14:val` name (`"standardContextual"`); the flags object reads too.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub struct Ligatures {
    pub standard: bool,
    pub contextual: bool,
    pub historical: bool,
    pub discretional: bool,
}

impl Ligatures {
    /// None of them (`none`).
    pub const NONE: Ligatures = Ligatures { standard: false, contextual: false, historical: false, discretional: false };
    /// Standard and contextual (`standardContextual`): Word's Normal template's choice.
    pub const STANDARD_CONTEXTUAL: Ligatures = Ligatures { standard: true, contextual: true, historical: false, discretional: false };
    /// Every combination of the four flags.
    pub fn all() -> impl Iterator<Item = Ligatures> {
        (0u8..16).map(|b| Ligatures { standard: b & 1 != 0, contextual: b & 2 != 0, historical: b & 4 != 0, discretional: b & 8 != 0 })
    }
    /// The `w14:ligatures/@w14:val` name: `none`, `all`, or the set flags joined in camelCase
    /// (`standardContextual`).
    pub fn ooxml(self) -> String {
        let on: Vec<&str> =
            [(self.standard, "standard"), (self.contextual, "contextual"), (self.historical, "historical"), (self.discretional, "discretional")]
                .into_iter()
                .filter_map(|(on, n)| on.then_some(n))
                .collect();
        match on.len() {
            0 => "none".into(),
            4 => "all".into(),
            _ => on
                .iter()
                .enumerate()
                .flat_map(|(i, n)| n.chars().enumerate().map(move |(j, c)| if i > 0 && j == 0 { c.to_ascii_uppercase() } else { c }))
                .collect(),
        }
    }
    /// `None` for a name [MS-DOCX] doesn't define. Surrounding spaces don't matter, and
    /// `discretionary` reads as `discretional` (earlier WordCraft-ja files spelled it so).
    pub fn from_ooxml(s: &str) -> Option<Ligatures> {
        let s = s.trim().replace("iscretionary", "iscretional");
        Ligatures::all().find(|l| l.ooxml() == s)
    }
    /// Whether no ligature is asked for.
    pub fn is_none(self) -> bool {
        self == Ligatures::NONE
    }
}

impl Serialize for Ligatures {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&self.ooxml())
    }
}

impl<'de> Deserialize<'de> for Ligatures {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        #[serde(untagged)]
        enum Repr {
            Name(String),
            Flags {
                #[serde(default)]
                standard: bool,
                #[serde(default)]
                contextual: bool,
                #[serde(default)]
                historical: bool,
                #[serde(default)]
                discretional: bool,
            },
        }
        match Repr::deserialize(d)? {
            Repr::Name(n) => Ligatures::from_ooxml(&n).ok_or_else(|| serde::de::Error::custom(format!("unknown ligatures {n:?}"))),
            Repr::Flags { standard, contextual, historical, discretional } => Ok(Ligatures { standard, contextual, historical, discretional }),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub enum VertAlign {
    #[default]
    Baseline,
    Superscript,
    Subscript,
}

/// Word's 16 highlighter colours (plus none).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub enum Highlight {
    #[default]
    None,
    Yellow,
    BrightGreen,
    Turquoise,
    Pink,
    Blue,
    Red,
    DarkBlue,
    Teal,
    Green,
    Violet,
    DarkRed,
    DarkYellow,
    Gray50,
    Gray25,
    Black,
    White,
}

impl Highlight {
    pub const ALL: [Highlight; 17] = [
        Highlight::None,
        Highlight::Yellow,
        Highlight::BrightGreen,
        Highlight::Turquoise,
        Highlight::Pink,
        Highlight::Blue,
        Highlight::Red,
        Highlight::DarkBlue,
        Highlight::Teal,
        Highlight::Green,
        Highlight::Violet,
        Highlight::DarkRed,
        Highlight::DarkYellow,
        Highlight::Gray50,
        Highlight::Gray25,
        Highlight::Black,
        Highlight::White,
    ];
    pub fn rgb(self) -> Option<Rgb> {
        Some(match self {
            Highlight::None => return None,
            Highlight::Yellow => Rgb(255, 255, 0),
            Highlight::BrightGreen => Rgb(0, 255, 0),
            Highlight::Turquoise => Rgb(0, 255, 255),
            Highlight::Pink => Rgb(255, 0, 255),
            Highlight::Blue => Rgb(0, 0, 255),
            Highlight::Red => Rgb(255, 0, 0),
            Highlight::DarkBlue => Rgb(0, 0, 128),
            Highlight::Teal => Rgb(0, 128, 128),
            Highlight::Green => Rgb(0, 128, 0),
            Highlight::Violet => Rgb(128, 0, 128),
            Highlight::DarkRed => Rgb(128, 0, 0),
            Highlight::DarkYellow => Rgb(128, 128, 0),
            Highlight::Gray50 => Rgb(128, 128, 128),
            Highlight::Gray25 => Rgb(192, 192, 192),
            Highlight::Black => Rgb(0, 0, 0),
            Highlight::White => Rgb(255, 255, 255),
        })
    }
    /// OOXML `w:highlight/@w:val`.
    pub fn ooxml(self) -> &'static str {
        match self {
            Highlight::None => "none",
            Highlight::Yellow => "yellow",
            Highlight::BrightGreen => "green",
            Highlight::Turquoise => "cyan",
            Highlight::Pink => "magenta",
            Highlight::Blue => "blue",
            Highlight::Red => "red",
            Highlight::DarkBlue => "darkBlue",
            Highlight::Teal => "darkCyan",
            Highlight::Green => "darkGreen",
            Highlight::Violet => "darkMagenta",
            Highlight::DarkRed => "darkRed",
            Highlight::DarkYellow => "darkYellow",
            Highlight::Gray50 => "darkGray",
            Highlight::Gray25 => "lightGray",
            Highlight::Black => "black",
            Highlight::White => "white",
        }
    }
    pub fn from_ooxml(s: &str) -> Highlight {
        Highlight::ALL.iter().copied().find(|h| h.ooxml() == s).unwrap_or(Highlight::None)
    }
    pub fn name(self) -> &'static str {
        match self {
            Highlight::None => "No Color",
            Highlight::Yellow => "Yellow",
            Highlight::BrightGreen => "Bright Green",
            Highlight::Turquoise => "Turquoise",
            Highlight::Pink => "Pink",
            Highlight::Blue => "Blue",
            Highlight::Red => "Red",
            Highlight::DarkBlue => "Dark Blue",
            Highlight::Teal => "Teal",
            Highlight::Green => "Green",
            Highlight::Violet => "Violet",
            Highlight::DarkRed => "Dark Red",
            Highlight::DarkYellow => "Dark Yellow",
            Highlight::Gray50 => "Gray-50%",
            Highlight::Gray25 => "Gray-25%",
            Highlight::Black => "Black",
            Highlight::White => "White",
        }
    }
}

/// The text size, in points, of a run whose size no style or default sets.
pub const DEFAULT_FONT_SIZE: f32 = 11.0;

/// Character formatting (direct or in a style). `None` = inherit.
#[derive(Clone, Debug, PartialEq, Default, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct CharProps {
    /// Character style id.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub style: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub font: Option<String>,
    /// The symbol's own font (OOXML `w:sym/@w:font`, ECMA-376 §17.3.3.30), set on a symbol
    /// character from a `w:sym` or Insert › Symbol: it is shown in this font whatever `font` is,
    /// so changing the text's font keeps the symbol. `None` = the run's font.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sym_font: Option<String>,
    /// Points.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub size: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bold: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub italic: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub underline: Option<Underline>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub underline_color: Option<Rgb>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub strike: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub double_strike: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub color: Option<TextColor>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub highlight: Option<Highlight>,
    /// Character shading (background fill).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub shading: Option<Rgb>,
    /// Character border (`w:bdr`); `style: None` = explicitly no border.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub border: Option<Border>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub vert_align: Option<VertAlign>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub caps: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub small_caps: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hidden: Option<bool>,
    /// Hidden in Web Layout (`w:webHidden`); distinct from `w:vanish` (`hidden`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub web_hidden: Option<bool>,
    /// Snap characters to the document grid (`w:snapToGrid`). Unset inherits; Word's default is on.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub snap_to_grid: Option<bool>,
    /// Character spacing (expanded/condensed), points.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub spacing: Option<f32>,
    /// Horizontal scale, percent (100 = normal).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scale: Option<f32>,
    /// Raised (+) / lowered (−) position, points.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub position: Option<f32>,
    /// Kerning for fonts at or above this size (points); 0 = off.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub kern: Option<f32>,
    /// OpenType ligatures (Font › Advanced › OpenType Features; `w14:ligatures`). Unset = none:
    /// the run is shaped by Word's legacy text path (see [`Ligatures`]).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ligatures: Option<Ligatures>,
    /// OpenType number forms (`w14:numForm`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub num_form: Option<NumForm>,
    /// OpenType number spacing (`w14:numSpacing`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub num_spacing: Option<NumSpacing>,
    /// OpenType stylistic sets 1–20 (`w14:stylisticSets`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stylistic_sets: Option<Vec<u8>>,
    /// Use Contextual Alternates (`w14:cntxtAlts`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cntxt_alts: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub outline: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub shadow: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub emboss: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub engrave: Option<bool>,
    /// Emphasis mark (傍点, `w:em`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub em: Option<EmphasisMark>,
    /// 均等割り付け (`w:fitText`): the run spread to a fixed width.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fit_text: Option<FitText>,
    /// 縦中横 / 割注 (`w:eastAsianLayout`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub east_asian_layout: Option<EastAsianLayout>,
    /// WordArt text effects: fill, outline, shadow, glow, reflection ([MS-DOCX] `w14:` run
    /// properties). Overlaid effect by effect.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text_effects: Option<Box<crate::wordart::TextEffects>>,
    /// BCP 47 language tag (proofing).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub lang: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub no_proof: Option<bool>,
    /// Right-to-left run (OOXML `w:rtl`): its characters are complex script and read right to left.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rtl: Option<bool>,
    /// Format the whole run with the complex-script properties below (OOXML `w:cs`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cs: Option<bool>,
    /// Complex-script font (Arabic, Persian, Hebrew…; OOXML `w:rFonts/@w:cs`). `None` = `font`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub font_cs: Option<String>,
    /// East Asian font (Chinese, Japanese and Korean text; OOXML `w:rFonts/@w:eastAsia`). `None` =
    /// inherit, then `font`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub font_ea: Option<String>,
    /// Complex-script size, points (`w:szCs`). `None` leaves the inherited value unchanged.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub size_cs: Option<f32>,
    /// Complex-script bold / italic (`w:bCs`, `w:iCs`). `None` leaves the inherited value unchanged.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bold_cs: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub italic_cs: Option<bool>,
    /// BCP 47 language of complex-script text (`w:lang/@w:bidi`, e.g. `fa-IR`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub lang_bidi: Option<String>,
    /// BCP 47 language of East Asian text (`w:lang/@w:eastAsia`, e.g. `ja-JP`): which script's
    /// theme font Chinese, Japanese and Korean characters take, and their proofing language.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub lang_ea: Option<String>,
    /// Hyperlink target: a URL, or `#bookmark` for an internal link.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub link: Option<String>,
    /// Tracked insertion: index into `Document::revisions`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ins: Option<u32>,
    /// Tracked deletion: index into `Document::revisions`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub del: Option<u32>,
    /// Tracked formatting change (`w:rPrChange`): the formatting before it.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fmt_change: Option<Box<PropChange<CharProps>>>,
}

/// A tracked formatting change (ECMA-376 §17.13.5: `w:rPrChange`, `w:pPrChange`,
/// `w:tblPrChange`, `w:trPrChange`, `w:tcPrChange`, `w:sectPrChange`): the properties as they
/// were before the change, and the revision (author, date) that made it.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PropChange<T> {
    /// Index into `Document::revisions`.
    pub rev: u32,
    /// The properties before the change (formatting only: no revision or link marks).
    pub old: T,
}

impl<T> PropChange<T> {
    pub fn boxed(rev: u32, old: T) -> Option<Box<PropChange<T>>> {
        Some(Box::new(PropChange { rev, old }))
    }
}

/// A tracked change of a paragraph's list numbering (`w:numberingChange`, kept for round trip):
/// `original` is the number as it was shown before the change.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(default, rename_all = "camelCase")]
pub struct NumChange {
    /// Index into `Document::revisions`.
    pub rev: u32,
    pub original: String,
}

impl CharProps {
    /// The same formatting outside a hyperlink: no link target, and no "Hyperlink" character
    /// style that came with it. What a paragraph mark or text typed next to a link gets.
    pub fn unlinked(&self) -> CharProps {
        let mut c = self.clone();
        if c.link.take().is_some() && c.style.as_deref() == Some("Hyperlink") {
            c.style = None;
        }
        c
    }

    /// The fields of `self` that differ from `base` (equal ones unset): what `self` adds when
    /// overlaid on `base`.
    pub fn minus(mut self, base: &CharProps) -> CharProps {
        self.minus_text_style(base);
        self.minus_typography(base);
        self.minus_run_visibility(base);
        self.minus_script_style(base);
        self.minus_document_marks(base);
        self
    }

    fn minus_text_style(&mut self, base: &CharProps) {
        drop_same_fields!(self, base; style, font, sym_font, size, bold, italic, underline, underline_color, strike, double_strike, color, highlight,
            shading, border, vert_align, caps, small_caps, hidden, spacing, scale, position, kern, outline, shadow, emboss, engrave, lang, no_proof, rtl);
    }

    fn minus_typography(&mut self, base: &CharProps) {
        drop_same_fields!(self, base; ligatures, num_form, num_spacing, stylistic_sets, cntxt_alts, em, fit_text);
    }

    fn minus_run_visibility(&mut self, base: &CharProps) {
        drop_same_fields!(self, base; web_hidden, snap_to_grid);
    }

    fn minus_script_style(&mut self, base: &CharProps) {
        drop_same_fields!(self, base; cs, font_cs, font_ea, size_cs, bold_cs, italic_cs, lang_bidi, lang_ea, east_asian_layout);
    }

    fn minus_document_marks(&mut self, base: &CharProps) {
        drop_same_fields!(self, base; link, ins, del);
    }

    /// Apply every `Some` field of `patch` on top of `self`.
    ///
    /// Plain and complex-script properties inherit independently. Commands that intend to
    /// format both scripts must set both values explicitly.
    pub fn overlay(&mut self, patch: &CharProps) {
        self.overlay_text_style(patch);
        self.overlay_typography(patch);
        self.overlay_run_visibility(patch);
        self.overlay_script_style(patch);
        self.overlay_document_marks(patch);
        if let Some(p) = &patch.text_effects {
            self.text_effects.get_or_insert_with(Default::default).overlay(p);
        }
    }

    fn overlay_text_style(&mut self, patch: &CharProps) {
        overlay_fields!(self, patch; style, font, sym_font, size, bold, italic, underline, underline_color, strike, double_strike, color, highlight,
            shading, border, vert_align, caps, small_caps, hidden, spacing, scale, position, kern, outline, shadow, emboss, engrave, lang, no_proof, rtl);
    }

    fn overlay_typography(&mut self, patch: &CharProps) {
        overlay_fields!(self, patch; ligatures, num_form, num_spacing, stylistic_sets, cntxt_alts, em, fit_text);
    }

    fn overlay_run_visibility(&mut self, patch: &CharProps) {
        overlay_fields!(self, patch; web_hidden, snap_to_grid);
    }

    fn overlay_script_style(&mut self, patch: &CharProps) {
        overlay_fields!(self, patch; cs, font_cs, font_ea, size_cs, bold_cs, italic_cs, lang_bidi, lang_ea, east_asian_layout);
    }

    fn overlay_document_marks(&mut self, patch: &CharProps) {
        overlay_fields!(self, patch; link, ins, del, fmt_change);
    }
    pub fn overlaid(mut self, patch: &CharProps) -> CharProps {
        self.overlay(patch);
        self
    }
    pub fn is_empty(&self) -> bool {
        *self == CharProps::default()
    }
    /// The font a character of this run is shown in: the symbol's own font when it has one
    /// ([`Self::sym_font`]), else the run's font.
    pub fn shown_font(&self) -> Option<&str> {
        self.sym_font.as_deref().filter(|f| !f.trim().is_empty()).or(self.font.as_deref())
    }
    /// The same formatting for text typed or painted next to a symbol: without the symbol's own
    /// font.
    pub fn without_symbol(mut self) -> CharProps {
        self.sym_font = None;
        self
    }
    /// Clear direct formatting but keep what isn't "formatting" (links, revisions, character style
    /// is cleared too, as Word's Clear Formatting does).
    pub fn cleared(&self) -> CharProps {
        CharProps {
            link: self.link.clone(),
            ins: self.ins,
            del: self.del,
            fmt_change: self.fmt_change.clone(),
            // A symbol stays the symbol it is.
            sym_font: self.sym_font.clone(),
            ..Default::default()
        }
    }
    /// Just the formatting: without the link, revision marks and tracked formatting change.
    pub fn formatting(&self) -> CharProps {
        CharProps { link: None, ins: None, del: None, fmt_change: None, ..self.clone() }
    }
}

/// OpenType number forms (`w14:numForm`): lining (`lnum`) or old-style (`onum`) figures.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub enum NumForm {
    #[default]
    Default,
    Lining,
    OldStyle,
}

impl NumForm {
    pub fn ooxml(self) -> &'static str {
        match self {
            NumForm::Default => "default",
            NumForm::Lining => "lining",
            NumForm::OldStyle => "oldStyle",
        }
    }
    pub fn from_ooxml(s: &str) -> Option<NumForm> {
        [NumForm::Default, NumForm::Lining, NumForm::OldStyle].into_iter().find(|f| f.ooxml().eq_ignore_ascii_case(s.trim()))
    }
}

/// OpenType number spacing (`w14:numSpacing`): proportional (`pnum`) or tabular (`tnum`) figures.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub enum NumSpacing {
    #[default]
    Default,
    Proportional,
    Tabular,
}

impl NumSpacing {
    pub fn ooxml(self) -> &'static str {
        match self {
            NumSpacing::Default => "default",
            NumSpacing::Proportional => "proportional",
            NumSpacing::Tabular => "tabular",
        }
    }
    pub fn from_ooxml(s: &str) -> Option<NumSpacing> {
        [NumSpacing::Default, NumSpacing::Proportional, NumSpacing::Tabular].into_iter().find(|f| f.ooxml().eq_ignore_ascii_case(s.trim()))
    }
}

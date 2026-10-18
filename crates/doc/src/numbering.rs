//! Lists: abstract numbering definitions (9 levels each), numbering instances, and the counter
//! state used to compute list labels.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::props::{Align, CharProps};
use crate::section::NumFormat;
use crate::{DocError, free_id};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub enum LevelSuffix {
    #[default]
    Tab,
    Space,
    Nothing,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Level {
    pub start: u32,
    pub format: NumFormat,
    /// Label template: `%1.`, `%1.%2.`, or the bullet character.
    pub text: String,
    pub align: Align,
    /// Left indent of the text, points.
    pub indent: f32,
    /// Hanging indent (label sits this far left of `indent`), points.
    pub hanging: f32,
    pub suffix: LevelSuffix,
    /// Label formatting (bullet font etc.).
    pub chr: CharProps,
    /// Restart after a higher level (true = after any higher level, the default).
    pub restart: bool,
    /// Restart only after this level (1-based, Word's `w:lvlRestart`): a level 3 that restarts
    /// after level 1 keeps counting across level 2 items. `None` restarts after the level just
    /// above. Ignored when `restart` is false.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub restart_after: Option<u8>,
    /// The tab stop that follows the number (Word's "Add tab stop at"), points from the
    /// paragraph's left edge.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tab: Option<f32>,
    /// Show all levels as decimal (legal numbering).
    pub legal: bool,
    /// Paragraph style linked to this level (e.g. Heading 1 for outline numbering).
    pub style: Option<String>,
}

impl Default for Level {
    fn default() -> Self {
        Level {
            start: 1,
            format: NumFormat::Decimal,
            text: "%1.".into(),
            align: Align::Left,
            indent: 36.0,
            hanging: 18.0,
            suffix: LevelSuffix::Tab,
            chr: CharProps::default(),
            restart: true,
            restart_after: None,
            tab: None,
            legal: false,
            style: None,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct AbstractNum {
    pub id: u32,
    pub name: Option<String>,
    pub levels: Vec<Level>,
    /// Word's list identity (`w:nsid`, 8 hex digits), which it uses to match lists across
    /// documents (paste, compare); kept from the file.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub nsid: Option<String>,
    /// The list template the definition came from (`w:tmpl`); kept from the file.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tmpl: Option<String>,
    /// `w:multiLevelType` from the file (`singleLevel`, `multilevel`, `hybridMultilevel`);
    /// `None`: derived from the levels.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub multi_level_type: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Default)]
#[serde(default, rename_all = "camelCase")]
pub struct Num {
    pub id: u32,
    pub abstract_id: u32,
    /// Per-level start overrides (level, start).
    pub start_overrides: Vec<(u8, u32)>,
    /// Levels this list defines itself instead of taking them from its abstract list (level,
    /// definition): Word's `w:lvlOverride/w:lvl`.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub level_overrides: Vec<(u8, Level)>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct Numbering {
    pub abstracts: Vec<AbstractNum>,
    pub nums: Vec<Num>,
}

/// Built-in list kinds for the Bullets / Numbering / Multilevel galleries.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ListKind {
    /// • ○ ▪ repeating.
    Bullet,
    /// A specific bullet character.
    BulletChar(char),
    /// 1. a. i.
    Numbered,
    /// 1) a) i)
    NumberedParen,
    /// I. A. 1.
    Outline,
    /// A. B. C.
    UpperLetter,
    /// a) b) c)
    LowerLetter,
    /// a. b. c.
    LowerLetterDot,
    /// i. ii. iii.
    LowerRoman,
    /// 1. 1.1. 1.1.1.
    Legal,
    /// Word's Japanese Numbering library (an East Asian language of Japanese), read from Word
    /// for Mac's list galleries: each is a single-level list with the number at 0 pt and the
    /// text at 22 pt (`%1.`, `%1)` for letters, `(%1)` for aiueo), except uppercase roman whose
    /// text sits at 33 pt and lowercase roman whose number sits at 22 pt and text at 33 pt.
    /// ● at 0 / 22 pt: Word's default bullet in a Japanese document (Wingdings l), one level.
    JaBullet,
    /// 1. 2. 3.
    JaNumbered,
    /// ① ② ③
    Circled,
    /// I. II. III.
    JaUpperRoman,
    /// A) B) C)
    UpperLetterParen,
    /// (ア) (イ) (ウ)
    Aiueo,
    /// イ) ロ) ハ)
    Iroha,
    /// i. ii. iii.
    JaLowerRoman,
    /// Word's Japanese Multilevel library, in gallery order (positions in points as Word for Mac
    /// reads them back: number position / text position).
    /// 1 1.1 1.1.1 — numbers at 0, 21.25, 42.55…, text at 21.25, 49.6, 70.9, 99.2, 127.55, 163,
    /// 191.35, 219.7, 255.1 (Word's default Multilevel List in a Japanese document).
    JaNumbers,
    /// 1. 1.1. 1.1.1. — every number at 0, text at 21.25, 28.35, 35.45… (7.1 pt a level).
    JaLegal,
    /// １ １-１ １-１-１ — full-width digits, no indent, nothing after the number.
    JaFullWidthDash,
    /// １ (ア) ① (ア) then no number — full-width digit, aiueo, circled, iroha.
    JaMixed,
    /// 第１章 第１節 第１項, then no number.
    JaChapter,
    /// 第一章 第一節 第一項 (kanji digits), then no number.
    JaChapterKanji,
    /// 1. a. i. 1) (a) (i) (1) (a) (i)
    JaOutline,
}

impl ListKind {
    pub fn parse(s: &str) -> Option<ListKind> {
        Some(match s {
            "bullet" => ListKind::Bullet,
            "numbered" | "number" => ListKind::Numbered,
            "numberedParen" => ListKind::NumberedParen,
            "outline" => ListKind::Outline,
            "upperLetter" => ListKind::UpperLetter,
            "lowerLetter" => ListKind::LowerLetter,
            "lowerLetterDot" => ListKind::LowerLetterDot,
            "lowerRoman" => ListKind::LowerRoman,
            "legal" | "multilevel" => ListKind::Legal,
            "jaBullet" => ListKind::JaBullet,
            "jaNumbered" => ListKind::JaNumbered,
            "circled" => ListKind::Circled,
            "jaUpperRoman" => ListKind::JaUpperRoman,
            "upperLetterParen" => ListKind::UpperLetterParen,
            "aiueo" => ListKind::Aiueo,
            "iroha" => ListKind::Iroha,
            "jaLowerRoman" => ListKind::JaLowerRoman,
            "jaNumbers" => ListKind::JaNumbers,
            "jaLegal" => ListKind::JaLegal,
            "jaFullWidthDash" => ListKind::JaFullWidthDash,
            "jaMixed" => ListKind::JaMixed,
            "jaChapter" => ListKind::JaChapter,
            "jaChapterKanji" => ListKind::JaChapterKanji,
            "jaOutline" => ListKind::JaOutline,
            s if s.chars().count() == 1 => ListKind::BulletChar(s.chars().next()?),
            _ => return None,
        })
    }
    pub fn is_bullet(self) -> bool {
        matches!(self, ListKind::Bullet | ListKind::BulletChar(_) | ListKind::JaBullet)
    }
    /// Word's Japanese Numbering library, in gallery order.
    pub const JA_NUMBERING: [ListKind; 7] = [
        ListKind::JaNumbered,
        ListKind::Circled,
        ListKind::JaUpperRoman,
        ListKind::UpperLetterParen,
        ListKind::Aiueo,
        ListKind::Iroha,
        ListKind::JaLowerRoman,
    ];
    /// Word's Japanese Multilevel library, in gallery order.
    pub const JA_MULTILEVEL: [ListKind; 7] = [
        ListKind::JaNumbers,
        ListKind::JaLegal,
        ListKind::JaFullWidthDash,
        ListKind::JaMixed,
        ListKind::JaChapter,
        ListKind::JaChapterKanji,
        ListKind::JaOutline,
    ];
    /// The `kind` parameter that names this kind.
    pub fn id(self) -> String {
        match self {
            ListKind::Bullet => "bullet".into(),
            ListKind::BulletChar(c) => c.to_string(),
            ListKind::Numbered => "numbered".into(),
            ListKind::NumberedParen => "numberedParen".into(),
            ListKind::Outline => "outline".into(),
            ListKind::UpperLetter => "upperLetter".into(),
            ListKind::LowerLetter => "lowerLetter".into(),
            ListKind::LowerLetterDot => "lowerLetterDot".into(),
            ListKind::LowerRoman => "lowerRoman".into(),
            ListKind::Legal => "legal".into(),
            ListKind::JaBullet => "jaBullet".into(),
            ListKind::JaNumbered => "jaNumbered".into(),
            ListKind::Circled => "circled".into(),
            ListKind::JaUpperRoman => "jaUpperRoman".into(),
            ListKind::UpperLetterParen => "upperLetterParen".into(),
            ListKind::Aiueo => "aiueo".into(),
            ListKind::Iroha => "iroha".into(),
            ListKind::JaLowerRoman => "jaLowerRoman".into(),
            ListKind::JaNumbers => "jaNumbers".into(),
            ListKind::JaLegal => "jaLegal".into(),
            ListKind::JaFullWidthDash => "jaFullWidthDash".into(),
            ListKind::JaMixed => "jaMixed".into(),
            ListKind::JaChapter => "jaChapter".into(),
            ListKind::JaChapterKanji => "jaChapterKanji".into(),
            ListKind::JaOutline => "jaOutline".into(),
        }
    }
    /// Whether the kind numbers its levels together (a Multilevel List), as opposed to a
    /// Numbering library entry.
    pub fn is_multilevel(self) -> bool {
        matches!(self, ListKind::Legal | ListKind::Outline) || ListKind::JA_MULTILEVEL.contains(&self)
    }
    /// How a gallery shows this kind: the first three labels (`1. 2. 3.`), or for a multilevel
    /// kind the first label of its first three numbered levels (`1 1.1 1.1.1`).
    pub fn preview(self) -> String {
        let levels = levels_for(self);
        let label = |l: &Level, n: u32| -> String { (1..=9u32).fold(l.text.clone(), |t, k| t.replace(&format!("%{k}"), &l.format.format(n))) };
        if self.is_multilevel() {
            levels.iter().filter(|l| l.format != NumFormat::None).take(3).map(|l| label(l, 1)).collect::<Vec<_>>().join(" ")
        } else {
            levels.first().map(|l| (1..=3).map(|n| label(l, n)).collect::<Vec<_>>().join(" ")).unwrap_or_default()
        }
    }
}

/// A level of Word's Japanese libraries: `number` and `text` positions in points, as Word for
/// Mac reads them back (`w:ind` left = text, hanging = text − number).
fn ja_level(format: NumFormat, text: &str, number: f32, text_pos: f32, suffix: LevelSuffix) -> Level {
    Level { format, text: text.into(), indent: text_pos, hanging: (text_pos - number).max(0.0), suffix, ..Level::default() }
}

/// Number positions of Word's Japanese multilevel templates: 21.25 pt a level.
const JA_STEP: [f32; 9] = [0.0, 21.25, 42.55, 63.8, 85.05, 106.3, 127.55, 148.8, 170.1];

fn bullet_level(i: usize, c: char) -> Level {
    Level {
        format: NumFormat::Bullet,
        text: c.to_string(),
        indent: 36.0 * (i as f32 + 1.0),
        hanging: 18.0,
        chr: CharProps {
            font: match c {
                '•' | '▪' | '○' => Some("Arial".into()),
                // Plain text characters (a dash bullet) use the paragraph's font.
                c if c.is_ascii() || c == '–' => None,
                _ => Some("Segoe UI Symbol".into()),
            },
            ..CharProps::default()
        },
        ..Level::default()
    }
}

/// Nine levels for a list kind (one for Word's Japanese Numbering library).
pub fn levels_for(kind: ListKind) -> Vec<Level> {
    if kind == ListKind::JaBullet || ListKind::JA_NUMBERING.contains(&kind) || ListKind::JA_MULTILEVEL.contains(&kind) {
        return ja_levels(kind);
    }
    (0..9)
        .map(|i| {
            let ind = 36.0 * (i as f32 + 1.0);
            let cyc = |a: NumFormat, b: NumFormat, c: NumFormat| match i % 3 {
                0 => a,
                1 => b,
                _ => c,
            };
            let lvl = |format: NumFormat, text: String| Level { format, text, indent: ind, hanging: 18.0, ..Level::default() };
            let n = i + 1;
            match kind {
                ListKind::Bullet => bullet_level(i, ['•', '○', '▪'][i % 3]),
                ListKind::BulletChar(c) => bullet_level(i, if i == 0 { c } else { ['•', '○', '▪'][i % 3] }),
                ListKind::Numbered => lvl(cyc(NumFormat::Decimal, NumFormat::LowerLetter, NumFormat::LowerRoman), format!("%{n}.")),
                ListKind::NumberedParen => lvl(cyc(NumFormat::Decimal, NumFormat::LowerLetter, NumFormat::LowerRoman), format!("%{n})")),
                ListKind::UpperLetter => lvl(cyc(NumFormat::UpperLetter, NumFormat::LowerLetter, NumFormat::LowerRoman), format!("%{n}.")),
                ListKind::LowerLetter => lvl(cyc(NumFormat::LowerLetter, NumFormat::LowerRoman, NumFormat::Decimal), format!("%{n})")),
                ListKind::LowerLetterDot => lvl(cyc(NumFormat::LowerLetter, NumFormat::LowerRoman, NumFormat::Decimal), format!("%{n}.")),
                ListKind::LowerRoman => lvl(cyc(NumFormat::LowerRoman, NumFormat::LowerLetter, NumFormat::Decimal), format!("%{n}.")),
                ListKind::Outline => {
                    let f = [NumFormat::UpperRoman, NumFormat::UpperLetter, NumFormat::Decimal, NumFormat::LowerLetter, NumFormat::LowerRoman]
                        .get(i)
                        .copied()
                        .unwrap_or(NumFormat::Decimal);
                    lvl(f, format!("%{n}."))
                }
                ListKind::Legal => {
                    let text: String = (1..=n).map(|k| format!("%{k}.")).collect();
                    Level { format: NumFormat::Decimal, text, indent: 18.0 + 25.0 * i as f32, hanging: 18.0 + 7.0 * i as f32, ..Level::default() }
                }
                ListKind::JaBullet
                | ListKind::JaNumbered
                | ListKind::Circled
                | ListKind::JaUpperRoman
                | ListKind::UpperLetterParen
                | ListKind::Aiueo
                | ListKind::Iroha
                | ListKind::JaLowerRoman
                | ListKind::JaNumbers
                | ListKind::JaLegal
                | ListKind::JaFullWidthDash
                | ListKind::JaMixed
                | ListKind::JaChapter
                | ListKind::JaChapterKanji
                | ListKind::JaOutline => Level::default(),
            }
        })
        .collect()
}

/// The levels of Word's Japanese Numbering library (one level each, as Word writes them:
/// `w:multiLevelType singleLevel`) and Multilevel library (nine), as read from Word for Mac.
fn ja_levels(kind: ListKind) -> Vec<Level> {
    use LevelSuffix::{Nothing, Tab};
    use NumFormat::*;
    let step = |i: usize| JA_STEP.get(i).copied().unwrap_or(0.0);
    let next = |i: usize| JA_STEP.get(i + 1).copied().unwrap_or(191.35);
    match kind {
        // Wingdings l in Word's file; ● in the paragraph's font draws the same disc.
        ListKind::JaBullet => vec![ja_level(Bullet, "●", 0.0, 22.0, Tab)],
        ListKind::JaNumbered => vec![ja_level(Decimal, "%1.", 0.0, 22.0, Tab)],
        ListKind::Circled => vec![ja_level(DecimalEnclosedCircle, "%1", 0.0, 22.0, Tab)],
        ListKind::JaUpperRoman => vec![ja_level(UpperRoman, "%1.", 0.0, 33.0, Tab)],
        ListKind::UpperLetterParen => vec![ja_level(UpperLetter, "%1)", 0.0, 22.0, Tab)],
        ListKind::Aiueo => vec![ja_level(AiueoFullWidth, "(%1)", 0.0, 22.0, Tab)],
        ListKind::Iroha => vec![ja_level(IrohaFullWidth, "%1)", 0.0, 22.0, Tab)],
        ListKind::JaLowerRoman => vec![ja_level(LowerRoman, "%1.", 22.0, 33.0, Tab)],
        ListKind::JaNumbers => {
            let text_pos = [21.25, 49.6, 70.9, 99.2, 127.55, 163.0, 191.35, 219.7, 255.1];
            (0..9)
                .map(|i| {
                    let text: String = (1..=i + 1).map(|k| format!("%{k}")).collect::<Vec<_>>().join(".");
                    ja_level(Decimal, &text, step(i), text_pos.get(i).copied().unwrap_or(0.0), Tab)
                })
                .collect()
        }
        ListKind::JaLegal => (0..9)
            .map(|i| {
                let text: String = (1..=i + 1).map(|k| format!("%{k}.")).collect();
                ja_level(Decimal, &text, 0.0, 21.25 + 7.1 * i as f32, Tab)
            })
            .collect(),
        ListKind::JaFullWidthDash => (0..9)
            .map(|i| {
                let text: String = (1..=i + 1).map(|k| format!("%{k}")).collect::<Vec<_>>().join("-");
                ja_level(DecimalFullWidth, &text, 0.0, 0.0, Nothing)
            })
            .collect(),
        ListKind::JaMixed => (0..9)
            .map(|i| match i {
                0 => ja_level(DecimalFullWidth, "%1", step(0), next(0), Tab),
                1 => ja_level(AiueoFullWidth, "(%2)", step(1), next(1), Tab),
                2 => ja_level(DecimalEnclosedCircle, "%3", step(2), next(2), Tab),
                3 => ja_level(IrohaFullWidth, "(%4)", step(3), next(3), Tab),
                _ => ja_level(None, "", step(i), next(i), Nothing),
            })
            .collect(),
        ListKind::JaChapter | ListKind::JaChapterKanji => {
            let digits = if kind == ListKind::JaChapter { DecimalFullWidth } else { JapaneseCounting };
            (0..9)
                .map(|i| match i {
                    0 => ja_level(digits, "第%1章", step(0), next(0), Tab),
                    1 => ja_level(digits, "第%2節", step(1), next(1), Tab),
                    2 => ja_level(digits, "第%3項", step(2), next(2), Tab),
                    _ => ja_level(None, "", step(i), next(i), Nothing),
                })
                .collect()
        }
        ListKind::JaOutline => (0..9)
            .map(|i| {
                let format = [Decimal, LowerLetter, LowerRoman].get(i % 3).copied().unwrap_or(Decimal);
                let n = i + 1;
                let text = match i {
                    0..=2 => format!("%{n}."),
                    3 => format!("%{n})"),
                    _ => format!("(%{n})"),
                };
                ja_level(format, &text, step(i), next(i), Tab)
            })
            .collect(),
        _ => Vec::new(),
    }
}

impl Numbering {
    pub fn num(&self, id: u32) -> Option<&Num> {
        self.nums.iter().find(|n| n.id == id)
    }
    pub fn abstract_of(&self, num: u32) -> Option<&AbstractNum> {
        let n = self.num(num)?;
        self.abstracts.iter().find(|a| a.id == n.abstract_id)
    }
    /// Whether a paragraph in list `num` at `level` gets a label ([`Counters::next_label`] gives
    /// one), without counting it.
    pub fn shows_label(&self, num: u32, level: u8) -> bool {
        self.abstract_of(num).is_some() && self.level(num, level.min(8)).is_some_and(|d| d.format != NumFormat::None)
    }
    pub fn level(&self, num: u32, level: u8) -> Option<&Level> {
        if let Some((_, l)) = self.num(num)?.level_overrides.iter().find(|(i, _)| *i == level) {
            return Some(l);
        }
        self.abstract_of(num)?.levels.get(level as usize)
    }
    /// Create a new list of `kind`; returns the `num` id. List ids run out only past `u32::MAX`
    /// lists, more than memory holds; then nothing is added and the id is 0, "no list".
    pub fn add_list(&mut self, kind: ListKind) -> u32 {
        self.try_add_list(kind).unwrap_or(0)
    }
    /// [`Numbering::add_list`], or an error, adding nothing, when list ids are all taken.
    pub fn try_add_list(&mut self, kind: ListKind) -> crate::Result<u32> {
        self.try_add_levels(None, levels_for(kind))
    }
    /// A new list definition (`name`, `levels`) and a `num` for it; returns the `num` id. Both ids
    /// are [`free_id`]s, so the largest ids a file can hold don't make them collide with its lists;
    /// an error, adding nothing, when they are all taken.
    pub fn try_add_levels(&mut self, name: Option<String>, levels: Vec<Level>) -> crate::Result<u32> {
        let aid = free_id(self.abstracts.iter().map(|a| a.id).max(), self.abstracts.iter().map(|a| a.id), 0);
        let (Some(aid), Some(nid)) = (aid, self.free_num_id()) else { return Err(DocError::Invalid("no free list id".into())) };
        self.abstracts.push(AbstractNum { id: aid, name, levels, ..Default::default() });
        self.nums.push(Num { id: nid, abstract_id: aid, ..Default::default() });
        Ok(nid)
    }
    /// A `num` id no list has (`num` 0 means "no list"), `None` when they are all taken.
    fn free_num_id(&self) -> Option<u32> {
        free_id(self.nums.iter().map(|n| n.id).max(), self.nums.iter().map(|n| n.id), 1)
    }
    /// A new `num` that restarts numbering of an existing list (`None`: no such list, or list ids
    /// are all taken).
    pub fn restart(&mut self, num: u32) -> Option<u32> {
        let source = self.num(num)?;
        let a = source.abstract_id;
        let level_overrides = source.level_overrides.clone();
        let nid = self.free_num_id()?;
        // Every level starts again at its own effective start, so a level this list defines itself keeps its start.
        let levels = self.abstracts.iter().find(|x| x.id == a).map_or(0, |x| x.levels.len());
        let start_overrides = (0..levels).filter_map(|i| self.level(num, i as u8).map(|l| (i as u8, l.start))).collect();
        self.nums.push(Num { id: nid, abstract_id: a, start_overrides, level_overrides });
        Some(nid)
    }
    /// Find an existing list whose first level matches `kind`, so consecutive bullet clicks reuse it.
    pub fn find_kind(&self, kind: ListKind) -> Option<u32> {
        let want = levels_for(kind);
        let first = want.first()?;
        self.nums
            .iter()
            .find(|n| self.abstract_of(n.id).and_then(|a| a.levels.first()).is_some_and(|l| l.format == first.format && l.text == first.text))
            .map(|n| n.id)
    }
}

/// Running list counters while walking a document in order.
#[derive(Default, Debug, Clone)]
pub struct Counters {
    /// Per abstract list: current value per level.
    state: HashMap<u32, [u32; 9]>,
    /// Which nums have started (to apply start overrides once).
    seen: HashMap<u32, bool>,
}

impl Counters {
    /// The current value of each level of `num`'s list (0 for a level not counted yet).
    pub fn values(&self, numbering: &Numbering, num: u32) -> Option<[u32; 9]> {
        let abs = numbering.abstract_of(num)?;
        Some(self.state.get(&abs.id).copied().unwrap_or([0; 9]))
    }

    /// Advance the counter for `num`/`level` and return the label (`"1."`, `"a)"`, `"•"`).
    pub fn next_label(&mut self, numbering: &Numbering, num: u32, level: u8) -> Option<(String, Level)> {
        let n = numbering.num(num)?;
        let abs = numbering.abstract_of(num)?;
        let lv = (level as usize).min(8);
        // This list's own definition of a level wins over its abstract list's.
        let level_def = |k: usize| numbering.level(num, k as u8);
        let def = level_def(lv)?.clone();
        // A "none" level shows no label at all (Word's numFmt none) — not even its text.
        if def.format == NumFormat::None {
            return None;
        }
        let st = self.state.entry(abs.id).or_insert([0; 9]);
        if let std::collections::hash_map::Entry::Vacant(e) = self.seen.entry(num) {
            e.insert(true);
            for (l, start) in &n.start_overrides {
                if let Some(s) = st.get_mut(*l as usize) {
                    *s = start.saturating_sub(1);
                }
            }
        }
        if let Some(v) = st.get_mut(lv) {
            *v = if *v == 0 { def.start.max(if def.start == 0 { 0 } else { 1 }) } else { v.saturating_add(1) };
            if *v == 0 {
                *v = def.start;
            }
        }
        // Deeper levels restart, unless they never restart or only restart after a level above
        // this one (Word's lvlRestart).
        for (d, deeper) in st.iter_mut().enumerate().skip(lv + 1) {
            let restarts = level_def(d).is_none_or(|l| l.restart && l.restart_after.is_none_or(|k| lv < k as usize));
            if restarts {
                *deeper = 0;
            }
        }
        let vals = *st;
        if def.format == NumFormat::Bullet {
            return Some((def.text.clone(), def));
        }
        let mut label = String::new();
        let mut chars = def.text.chars().peekable();
        while let Some(c) = chars.next() {
            if c == '%'
                && let Some(d) = chars.peek().and_then(|d| d.to_digit(10))
            {
                chars.next();
                let k = (d as usize).saturating_sub(1).min(8);
                let val = vals.get(k).copied().unwrap_or(0);
                let val = if val == 0 { level_def(k).map(|l| l.start).unwrap_or(1) } else { val };
                // Legal numbering (w:isLgl) shows every level in Arabic digits, whatever its own
                // format (二 of a Chinese outline becomes 2 in "2.1").
                let fmt = match level_def(k).map(|l| l.format).unwrap_or(NumFormat::Decimal) {
                    NumFormat::DecimalZero => NumFormat::DecimalZero,
                    _ if def.legal => NumFormat::Decimal,
                    f => f,
                };
                label.push_str(&fmt.format(val));
            } else {
                label.push(c);
            }
        }
        Some((label, def))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn words_japanese_numbering_library_is_single_level_at_words_positions() {
        // Word for Mac's Numbering library with an East Asian language of Japanese, read from its
        // list galleries (number position / text position in points) and from the file its
        // default numbering writes (`singleLevel`, `w:ind w:left="440" w:hanging="440"`).
        let one = |kind: ListKind| {
            let l = levels_for(kind);
            assert_eq!(l.len(), 1, "{kind:?}");
            l.into_iter().next().unwrap()
        };
        let l = one(ListKind::JaNumbered);
        assert_eq!((l.format, l.text.as_str(), l.indent, l.hanging, l.suffix), (NumFormat::Decimal, "%1.", 22.0, 22.0, LevelSuffix::Tab));
        let l = one(ListKind::JaBullet);
        assert_eq!((l.format, l.text.as_str(), l.indent, l.hanging), (NumFormat::Bullet, "●", 22.0, 22.0));
        assert!(ListKind::JaBullet.is_bullet() && ListKind::parse("jaBullet") == Some(ListKind::JaBullet));
        assert_eq!(one(ListKind::Circled).format, NumFormat::DecimalEnclosedCircle);
        let l = one(ListKind::JaUpperRoman);
        assert_eq!((l.format, l.text.as_str(), l.indent, l.hanging), (NumFormat::UpperRoman, "%1.", 33.0, 33.0));
        assert_eq!(one(ListKind::UpperLetterParen).text, "%1)");
        let l = one(ListKind::Aiueo);
        assert_eq!((l.format, l.text.as_str()), (NumFormat::AiueoFullWidth, "(%1)"));
        let l = one(ListKind::Iroha);
        assert_eq!((l.format, l.text.as_str()), (NumFormat::IrohaFullWidth, "%1)"));
        let l = one(ListKind::JaLowerRoman);
        assert_eq!((l.format, l.indent, l.hanging), (NumFormat::LowerRoman, 33.0, 11.0));
        // The gallery tiles, with the labels the formats render.
        let previews: Vec<String> = ListKind::JA_NUMBERING.iter().map(|k| k.preview()).collect();
        assert_eq!(previews, ["1. 2. 3.", "① ② ③", "I. II. III.", "A) B) C)", "(ア) (イ) (ウ)", "イ) ロ) ハ)", "i. ii. iii."]);
        for k in ListKind::JA_NUMBERING.iter().chain(&ListKind::JA_MULTILEVEL) {
            assert_eq!(ListKind::parse(&k.id()), Some(*k));
            assert_eq!(k.is_multilevel(), ListKind::JA_MULTILEVEL.contains(k));
        }
    }

    #[test]
    fn words_japanese_multilevel_library_has_words_nine_levels() {
        // Word for Mac's Multilevel library (Japanese), every level read from the galleries.
        let at = |kind: ListKind, i: usize| levels_for(kind).get(i).cloned().unwrap();
        let pos = |l: &Level| (l.indent - l.hanging, l.indent);
        // 1 1.1 1.1.1: Word's default Multilevel List in a Japanese document (left 425, hanging
        // 425; 992/567; 1418/567; 1984/708; 2551/850; 3260/1134; 3827/1276; 4394/1418; 5102/1700).
        let l = at(ListKind::JaNumbers, 0);
        assert_eq!((l.text.as_str(), pos(&l)), ("%1", (0.0, 21.25)));
        let l = at(ListKind::JaNumbers, 1);
        assert_eq!((l.text.as_str(), pos(&l)), ("%1.%2", (21.25, 49.6)));
        let l = at(ListKind::JaNumbers, 8);
        assert_eq!((l.text.as_str(), pos(&l).1), ("%1.%2.%3.%4.%5.%6.%7.%8.%9", 255.1));
        assert!((pos(&l).0 - 170.1).abs() < 0.01);
        // 1. 1.1. 1.1.1.: numbers all at 0, text 7.1 pt further a level.
        let l = at(ListKind::JaLegal, 2);
        assert_eq!(l.text, "%1.%2.%3.");
        assert!((l.indent - 35.45).abs() < 0.01 && (l.hanging - 35.45).abs() < 0.01);
        // １ １-１ １-１-１: full-width digits, no indent, nothing after the number.
        let l = at(ListKind::JaFullWidthDash, 1);
        assert_eq!(
            (l.format, l.text.as_str(), l.indent, l.hanging, l.suffix),
            (NumFormat::DecimalFullWidth, "%1-%2", 0.0, 0.0, LevelSuffix::Nothing)
        );
        // １ (ア) ① (ア), then unnumbered levels.
        let formats: Vec<NumFormat> = levels_for(ListKind::JaMixed).iter().map(|l| l.format).collect();
        assert_eq!(
            formats,
            [
                NumFormat::DecimalFullWidth,
                NumFormat::AiueoFullWidth,
                NumFormat::DecimalEnclosedCircle,
                NumFormat::IrohaFullWidth,
                NumFormat::None,
                NumFormat::None,
                NumFormat::None,
                NumFormat::None,
                NumFormat::None
            ]
        );
        let l = at(ListKind::JaMixed, 3);
        assert_eq!((l.text.as_str(), pos(&l)), ("(%4)", (63.8, 85.05)));
        assert_eq!(at(ListKind::JaMixed, 4).suffix, LevelSuffix::Nothing);
        // 第１章 第１節 第１項 and the kanji-digit one.
        let l = at(ListKind::JaChapter, 1);
        assert_eq!((l.format, l.text.as_str(), pos(&l)), (NumFormat::DecimalFullWidth, "第%2節", (21.25, 42.55)));
        assert_eq!(at(ListKind::JaChapterKanji, 2).format, NumFormat::JapaneseCounting);
        assert_eq!(at(ListKind::JaChapter, 3).format, NumFormat::None);
        // 1. a. i. 1) (a) (i) (1) (a) (i).
        let texts: Vec<String> = levels_for(ListKind::JaOutline).iter().map(|l| l.text.clone()).collect();
        assert_eq!(texts, ["%1.", "%2.", "%3.", "%4)", "(%5)", "(%6)", "(%7)", "(%8)", "(%9)"]);
        assert_eq!(at(ListKind::JaOutline, 5).format, NumFormat::LowerRoman);
        assert_eq!(pos(&at(ListKind::JaOutline, 8)), (170.1, 191.35));
        let previews: Vec<String> = ListKind::JA_MULTILEVEL.iter().map(|k| k.preview()).collect();
        assert_eq!(
            previews,
            ["1 1.1 1.1.1", "1. 1.1. 1.1.1.", "１ １-１ １-１-１", "１ (ア) ①", "第１章 第１節 第１項", "第一章 第一節 第一項", "1. a. i."]
        );
        // The labels a document shows: ⑩ ⑫, コ シ, ヌ ヲ, １０, 十 十二, as Word renders them.
        let mut n = Numbering::default();
        let mut c = Counters::default();
        for (kind, labels) in [
            (ListKind::Circled, ["⑩", "⑫"]),
            (ListKind::Aiueo, ["(コ)", "(シ)"]),
            (ListKind::Iroha, ["ヌ)", "ヲ)"]),
            (ListKind::JaChapter, ["第１０章", "第１２章"]),
            (ListKind::JaChapterKanji, ["第十章", "第十二章"]),
        ] {
            let id = n.add_list(kind);
            let got: Vec<String> = (1..=12).map(|_| c.next_label(&n, id, 0).unwrap().0).collect();
            assert_eq!((got[9].as_str(), got[11].as_str()), (labels[0], labels[1]), "{kind:?}");
        }
    }

    #[test]
    fn a_list_s_own_level_definitions_win() {
        // A specification's article numbers: level 1 counts parts with no label of its own, level 2
        // shows "part.article" with a zero-padded article (Word's lvlOverride/lvl).
        let mut n = Numbering::default();
        let id = n.add_list(ListKind::Numbered);
        let lvl = |format, text: &str| Level { format, text: text.into(), ..Level::default() };
        if let Some(num) = n.nums.iter_mut().find(|x| x.id == id) {
            num.level_overrides = vec![(1, lvl(NumFormat::Decimal, "")), (2, lvl(NumFormat::DecimalZero, "%2.%3"))];
        }
        let mut c = Counters::default();
        assert_eq!(c.next_label(&n, id, 1).unwrap().0, "");
        assert_eq!(c.next_label(&n, id, 2).unwrap().0, "1.01");
        assert_eq!(c.next_label(&n, id, 2).unwrap().0, "1.02");
        assert_eq!(c.next_label(&n, id, 1).unwrap().0, "");
        assert_eq!(c.next_label(&n, id, 2).unwrap().0, "2.01");
        // Levels it doesn't define still come from the abstract list.
        assert_eq!(c.next_label(&n, id, 0).unwrap().0, "1.");
        assert_eq!(n.level(id, 2).map(|l| l.format), Some(NumFormat::DecimalZero));
    }

    #[test]
    fn levels_restart_only_after_their_restart_level() {
        let mut n = Numbering::default();
        let id = n.add_list(ListKind::Legal);
        if let Some(a) = n.abstracts.first_mut() {
            // Level 3 restarts only after level 1; level 2 never restarts.
            a.levels[2].restart_after = Some(1);
            a.levels[2].text = "(%3)".into();
            a.levels[1].restart = false;
        }
        let mut c = Counters::default();
        let mut l = |lv| c.next_label(&n, id, lv).unwrap().0;
        assert_eq!(l(0), "1.");
        assert_eq!(l(1), "1.1.");
        assert_eq!(l(2), "(1)");
        assert_eq!(l(1), "1.2.");
        assert_eq!(l(2), "(2)", "level 2 doesn't restart level 3");
        assert_eq!(l(0), "2.");
        assert_eq!(l(1), "2.3.", "level 2 never restarts");
        assert_eq!(l(2), "(1)", "level 1 restarts level 3");
    }

    #[test]
    fn numbered_labels() {
        let mut n = Numbering::default();
        let id = n.add_list(ListKind::Numbered);
        let mut c = Counters::default();
        assert_eq!(c.next_label(&n, id, 0).unwrap().0, "1.");
        assert_eq!(c.next_label(&n, id, 0).unwrap().0, "2.");
        assert_eq!(c.next_label(&n, id, 1).unwrap().0, "a.");
        assert_eq!(c.next_label(&n, id, 1).unwrap().0, "b.");
        assert_eq!(c.next_label(&n, id, 0).unwrap().0, "3.");
        assert_eq!(c.next_label(&n, id, 1).unwrap().0, "a.");
        assert!(c.next_label(&n, 999, 0).is_none());
    }

    #[test]
    fn east_asian_labels_and_legal_numbering() {
        // A Chinese outline: 第一章, then "一、" items, and a legal level showing 1.1.
        let mut n = Numbering::default();
        let id = n.add_list(ListKind::Numbered);
        if let Some(a) = n.abstracts.first_mut() {
            a.levels[0] = Level { format: NumFormat::ChineseCounting, text: "第%1章".into(), ..Level::default() };
            a.levels[1] = Level { format: NumFormat::ChineseCounting, text: "%2、".into(), ..Level::default() };
            a.levels[2] = Level { format: NumFormat::ChineseCounting, text: "%1.%2.%3".into(), legal: true, ..Level::default() };
            a.levels[3] = Level { format: NumFormat::ChineseCounting, text: "（%4）".into(), ..Level::default() };
        }
        let mut c = Counters::default();
        let mut l = |lv| c.next_label(&n, id, lv).unwrap().0;
        assert_eq!(l(0), "第一章");
        assert_eq!(l(1), "一、");
        assert_eq!(l(1), "二、");
        assert_eq!(l(1), "三、");
        assert_eq!(l(2), "1.3.1", "legal numbering is decimal on every level");
        assert_eq!(l(3), "（一）");
        for _ in 0..9 {
            l(3);
        }
        assert_eq!(l(3), "（十一）");
        assert_eq!(l(0), "第二章");
    }

    #[test]
    fn legal_labels() {
        let mut n = Numbering::default();
        let id = n.add_list(ListKind::Legal);
        let mut c = Counters::default();
        assert_eq!(c.next_label(&n, id, 0).unwrap().0, "1.");
        assert_eq!(c.next_label(&n, id, 1).unwrap().0, "1.1.");
        assert_eq!(c.next_label(&n, id, 2).unwrap().0, "1.1.1.");
        assert_eq!(c.next_label(&n, id, 1).unwrap().0, "1.2.");
    }

    #[test]
    fn restart_and_bullets() {
        let mut n = Numbering::default();
        let id = n.add_list(ListKind::Numbered);
        let mut c = Counters::default();
        c.next_label(&n, id, 0);
        c.next_label(&n, id, 0);
        let r = n.restart(id).unwrap();
        assert_eq!(c.next_label(&n, r, 0).unwrap().0, "1.");
        let b = n.add_list(ListKind::Bullet);
        assert_eq!(c.next_label(&n, b, 0).unwrap().0, "•");
        assert_eq!(n.find_kind(ListKind::Bullet), Some(b));
    }

    #[test]
    fn restart_keeps_the_list_s_own_level_definitions() {
        let mut n = Numbering::default();
        let id = n.add_list(ListKind::Numbered);
        let own = Level { format: NumFormat::UpperRoman, text: "Part %1".into(), indent: 90.0, hanging: 40.0, start: 5, ..Level::default() };
        if let Some(num) = n.nums.iter_mut().find(|x| x.id == id) {
            num.level_overrides.push((0, own.clone()));
        }
        let r = n.restart(id).unwrap();
        assert_eq!(n.num(r).unwrap().level_overrides, vec![(0, own.clone())]);
        assert_eq!(n.level(r, 0), Some(&own));
        let mut c = Counters::default();
        assert_eq!(c.next_label(&n, r, 0).unwrap().0, "Part V");
    }
}

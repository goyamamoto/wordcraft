//! Word-processor font resolution: map a requested family + bold/italic to an available face,
//! with metric-compatible substitutes for common proprietary document fonts, and Word-style line
//! metrics (Windows ascent/descent, which is what Word uses for single line spacing).

use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

use skrifa::raw::TableProvider;

use crate::{FaceRef, FontDb};

/// A requested font resolved to a face.
#[derive(Clone, Copy, Debug)]
pub struct Resolved {
    pub face: FaceRef,
    /// The face isn't bold but bold was asked for: embolden when drawing.
    pub synth_bold: bool,
    /// The face isn't italic but italic was asked for: slant when drawing.
    pub synth_italic: bool,
    /// The requested family wasn't available.
    pub substituted: bool,
}

/// Substitutes to try, in order, for a family that isn't installed. Open fonts that are
/// metric-compatible come first (Carlito ↔ Calibri, Caladea ↔ Cambria, Liberation ↔ Arial /
/// Times New Roman / Courier New), then common system fonts, then our bundled families.
fn substitutes(family: &str) -> &'static [&'static str] {
    const SANS: &[&str] = &["Carlito", "Helvetica Neue", "Arial", "Liberation Sans", "Noto Sans", "DejaVu Sans", "Source Sans 3"];
    const SERIF: &[&str] = &["Caladea", "Times New Roman", "Liberation Serif", "Georgia", "Noto Serif", "DejaVu Serif", "Source Serif 4"];
    const MONO: &[&str] = &["Courier New", "Liberation Mono", "Menlo", "Consolas", "DejaVu Sans Mono", "JetBrains Mono"];
    const ARIAL: &[&str] = &["Liberation Sans", "Helvetica", "Helvetica Neue", "Arimo", "Source Sans 3"];
    const TIMES: &[&str] = &["Liberation Serif", "Times", "Tinos", "Source Serif 4"];
    // Word's symbol fonts (Symbol, Wingdings…) are proprietary: their characters are drawn as their
    // Unicode equivalents (`wordcraft_doc::symbol_fonts`) from these faces or any face that has them.
    const SYMBOL: &[&str] = &["Apple Symbols", "Segoe UI Symbol", "DejaVu Sans", "Noto Sans Symbols", "Noto Sans Symbols 2", "Source Sans 3"];
    // Persian / Arabic document fonts. Naskh (book) faces such as B Nazanin, B Lotus, B Zar and
    // Traditional Arabic, then the sans faces (B Yekan, B Titr, IRANSans) people use for
    // headings and screens. Open fonts with full Persian coverage first.
    const NASKH: &[&str] = &["Noto Naskh Arabic", "XB Zar", "XB Niloofar", "Amiri", "Vazirmatn", "Noto Sans Arabic", "DejaVu Sans", "Source Serif 4"];
    const ARABIC_SANS: &[&str] = &["Vazirmatn", "Sahel", "Shabnam", "Samim", "Noto Sans Arabic", "Noto Naskh Arabic", "DejaVu Sans", "Source Sans 3"];
    // Hebrew document fonts: platform-specific fonts first, then common open source fallbacks.
    // Windows: Microsoft Office fonts (David, Miriam) are common; Segoe UI is standard.
    // macOS: Helvetica has good Hebrew support; Geeza Pro is included by default.
    // Linux: Noto Sans Hebrew is the best option.
    #[cfg(windows)]
    const HEBREW: &[&str] = &["Noto Sans Hebrew", "David", "Miriam", "Segoe UI", "Arial", "Tahoma", "DejaVu Sans", "Source Sans 3"];
    #[cfg(target_os = "macos")]
    const HEBREW: &[&str] = &["Helvetica", "Noto Sans Hebrew", "Arial", "Tahoma", "Geeza Pro", "DejaVu Sans", "Source Sans 3"];
    #[cfg(not(any(windows, target_os = "macos")))]
    const HEBREW: &[&str] = &["Noto Sans Hebrew", "Segoe UI", "Arial", "Tahoma", "DejaVu Sans", "Source Sans 3"];
    // Palatino Linotype / Book Antiqua: macOS ships the same design as "Palatino"; open
    // look-alikes with matching metrics (TeX Gyre Pagella, URW P052 / Palladio) on Linux.
    const PALATINO: &[&str] = &[
        "Palatino Linotype",
        "Palatino",
        "Book Antiqua",
        "TeX Gyre Pagella",
        "P052",
        "URW Palladio L",
        "Palladio",
        "Caladea",
        "Times New Roman",
        "Liberation Serif",
        "Georgia",
        "Noto Serif",
        "DejaVu Serif",
        "Source Serif 4",
    ];
    let f = family.to_ascii_lowercase();
    if let Some(cjk) = cjk_substitutes(&f) {
        return cjk;
    }
    if is_hebrew(&f) {
        return HEBREW;
    }
    if is_persian_naskh(&f) {
        return NASKH;
    }
    if is_persian_sans(&f) {
        return ARABIC_SANS;
    }
    match f.as_str() {
        "palatino linotype" | "book antiqua" | "palatino" => PALATINO,
        "arial" | "helvetica" | "arial nova" => ARIAL,
        "times new roman" | "times" => TIMES,
        "courier new" | "courier" | "consolas" | "lucida console" | "cascadia code" | "cascadia mono" => MONO,
        "symbol" | "wingdings" | "wingdings 2" | "wingdings 3" | "segoe ui symbol" | "webdings" => SYMBOL,
        "cambria"
        | "cambria math"
        | "georgia"
        | "garamond"
        | "constantia"
        | "baskerville old face"
        | "century"
        | "century schoolbook"
        | "bookman old style"
        | "sitka text"
        | "sitka"
        | "aptos serif" => SERIF,
        _ if f.contains("mono") || f.contains("code") => MONO,
        _ if f.contains("serif") && !f.contains("sans") => SERIF,
        _ => SANS,
    }
}

/// Substitutes for the Chinese, Japanese and Korean document fonts Word's themes and templates
/// name (游明朝, ＭＳ 明朝, Meiryo, 等线, 宋体, 新細明體, 맑은 고딕…), by their English or native
/// names: the platform's own fonts of the same style first (Hiragino on macOS, the Windows fonts
/// when running there), then the open pan-CJK families (Noto / Source Han, BIZ UD, IPAex, Nanum).
/// `None` for a name that isn't one of them.
fn cjk_substitutes(lower: &str) -> Option<&'static [&'static str]> {
    let base = fold_cjk_name(lower);
    cjk_chain(&base)
}

/// A CJK font name folded for matching: full-width ASCII (ＭＳ Ｐ明朝) as ASCII, lower case,
/// single spaces.
fn fold_cjk_name(name: &str) -> String {
    let mut base = String::with_capacity(name.len());
    for c in name.trim().chars() {
        let c = match u32::from(c) {
            0xFF01..=0xFF5E => char::from_u32(u32::from(c) - 0xFF01 + 0x21).unwrap_or(c),
            0x3000 => ' ',
            _ => c,
        };
        if c == ' ' && base.ends_with(' ') {
            continue;
        }
        base.extend(c.to_lowercase());
    }
    base
}

/// Word's Chinese, Japanese and Korean theme and template fonts by their English and native
/// names, with the language of the native name: a document names them either way (the Office
/// theme says 游明朝; a Windows document ＭＳ 明朝 or MS Mincho), and Word's font box shows the
/// one in its interface language. `None` for other names.
pub fn cjk_font_names(family: &str) -> Option<(&'static str, &'static str, &'static str)> {
    const NAMES: &[(&str, &str, &str)] = &[
        ("Yu Mincho", "游明朝", "ja"),
        ("Yu Mincho Demibold", "游明朝 Demibold", "ja"),
        ("Yu Mincho Light", "游明朝 Light", "ja"),
        ("Yu Gothic", "游ゴシック", "ja"),
        ("Yu Gothic Light", "游ゴシック Light", "ja"),
        ("Yu Gothic Medium", "游ゴシック Medium", "ja"),
        ("MS Mincho", "ＭＳ 明朝", "ja"),
        ("MS PMincho", "ＭＳ Ｐ明朝", "ja"),
        ("MS Gothic", "ＭＳ ゴシック", "ja"),
        ("MS PGothic", "ＭＳ Ｐゴシック", "ja"),
        ("Meiryo", "メイリオ", "ja"),
        ("Hiragino Mincho ProN", "ヒラギノ明朝 ProN", "ja"),
        ("Hiragino Mincho Pro", "ヒラギノ明朝 Pro", "ja"),
        ("Hiragino Kaku Gothic ProN", "ヒラギノ角ゴ ProN", "ja"),
        ("Hiragino Kaku Gothic Pro", "ヒラギノ角ゴ Pro", "ja"),
        ("Hiragino Sans", "ヒラギノ角ゴシック", "ja"),
        ("DengXian", "等线", "zh-hans"),
        ("DengXian Light", "等线 Light", "zh-hans"),
        ("SimSun", "宋体", "zh-hans"),
        ("SimHei", "黑体", "zh-hans"),
        ("KaiTi", "楷体", "zh-hans"),
        ("FangSong", "仿宋", "zh-hans"),
        ("Microsoft YaHei", "微软雅黑", "zh-hans"),
        ("PMingLiU", "新細明體", "zh-hant"),
        ("MingLiU", "細明體", "zh-hant"),
        ("DFKai-SB", "標楷體", "zh-hant"),
        ("Microsoft JhengHei", "微軟正黑體", "zh-hant"),
        ("Malgun Gothic", "맑은 고딕", "ko"),
        ("Batang", "바탕", "ko"),
        ("Gulim", "굴림", "ko"),
        ("Dotum", "돋움", "ko"),
        ("Gungsuh", "궁서", "ko"),
    ];
    let base = fold_cjk_name(family);
    NAMES.iter().copied().find(|(e, n, _)| fold_cjk_name(e) == base || fold_cjk_name(n) == base)
}

const MINCHO: &[&str] = &[
    "Yu Mincho",
    "YuMincho",
    "Hiragino Mincho ProN",
    "Hiragino Mincho Pro",
    "MS Mincho",
    "MS PMincho",
    "BIZ UDPMincho",
    "BIZ UDMincho",
    "Noto Serif CJK JP",
    "Noto Serif JP",
    "Source Han Serif JP",
    "Source Han Serif",
    "IPAexMincho",
    "IPAMincho",
    "Toppan Bunkyu Mincho",
    "Hiragino Sans",
    "Noto Sans CJK JP",
];
const GOTHIC: &[&str] = &[
    "Yu Gothic",
    "YuGothic",
    "Hiragino Sans",
    "Hiragino Kaku Gothic ProN",
    "Hiragino Kaku Gothic Pro",
    "Meiryo",
    "MS Gothic",
    "MS PGothic",
    "BIZ UDPGothic",
    "BIZ UDGothic",
    "Noto Sans CJK JP",
    "Noto Sans JP",
    "Source Han Sans JP",
    "Source Han Sans",
    "IPAexGothic",
    "IPAGothic",
    "VL Gothic",
];
const SANS_SC: &[&str] = &[
    "Microsoft YaHei",
    "DengXian",
    "PingFang SC",
    "Hiragino Sans GB",
    "Heiti SC",
    "STHeiti",
    "SimHei",
    "Noto Sans CJK SC",
    "Noto Sans SC",
    "Source Han Sans SC",
    "Source Han Sans CN",
    "WenQuanYi Micro Hei",
    "WenQuanYi Zen Hei",
];
const SERIF_SC: &[&str] = &[
    "SimSun",
    "Songti SC",
    "STSong",
    "Noto Serif CJK SC",
    "Noto Serif SC",
    "Source Han Serif SC",
    "Source Han Serif CN",
    "AR PL UMing CN",
    "PingFang SC",
];
const SANS_TC: &[&str] =
    &["Microsoft JhengHei", "PingFang TC", "Heiti TC", "PingFang HK", "Noto Sans CJK TC", "Noto Sans TC", "Source Han Sans TC", "Source Han Sans TW"];
const SERIF_TC: &[&str] = &[
    "PMingLiU",
    "MingLiU",
    "Songti TC",
    "LiSong Pro",
    "Noto Serif CJK TC",
    "Noto Serif TC",
    "Source Han Serif TC",
    "Source Han Serif TW",
    "AR PL UMing TW",
    "PingFang TC",
];
const SANS_KR: &[&str] = &[
    "Malgun Gothic",
    "Apple SD Gothic Neo",
    "AppleGothic",
    "Noto Sans CJK KR",
    "Noto Sans KR",
    "Source Han Sans KR",
    "Source Han Sans K",
    "NanumGothic",
    "Nanum Gothic",
    "Gulim",
    "Dotum",
];
const SERIF_KR: &[&str] = &[
    "Batang",
    "AppleMyungjo",
    "Noto Serif CJK KR",
    "Noto Serif KR",
    "Source Han Serif KR",
    "Source Han Serif K",
    "NanumMyeongjo",
    "Nanum Myeongjo",
    "Gungsuh",
];

/// Is `family` a Chinese, Japanese or Korean serif family — a 明朝, 宋体/明體 or 바탕 face, Word's
/// own (游明朝, ＭＳ 明朝, SimSun, PMingLiU, Batang) or a substitute for one? Word for Mac draws
/// the Hangul such a font lacks in Batang (2026-10-19, see [`hangul_fallback`]).
pub fn is_cjk_serif_family(family: &str) -> bool {
    cjk_substitutes(&family.to_ascii_lowercase()).is_some_and(|c| [MINCHO, SERIF_SC, SERIF_TC, SERIF_KR].iter().any(|s| std::ptr::eq(c, *s)))
}

/// The substitute chain for a folded CJK font name.
fn cjk_chain(base: &str) -> Option<&'static [&'static str]> {
    let chain = match base {
        "游明朝"
        | "yu mincho"
        | "yumincho"
        | "游明朝 demibold"
        | "yu mincho demibold"
        | "ms mincho"
        | "ms 明朝"
        | "ms pmincho"
        | "ms p明朝"
        | "hiragino mincho pron"
        | "hiragino mincho pro"
        | "ヒラギノ明朝 pron"
        | "ヒラギノ明朝 pro"
        | "biz udmincho"
        | "biz udpmincho"
        | "biz udmincho medium"
        | "biz udpmincho medium"
        | "ud デジタル 教科書体 n-r"
        | "ud デジタル 教科書体 nk-r"
        | "ud デジタル 教科書体 np-r"
        | "ud digi kyokasho n-r"
        | "ud digi kyokasho nk-r"
        | "ud digi kyokasho np-r"
        | "hgp明朝e"
        | "hg明朝e"
        | "hgs明朝e"
        | "hgp明朝b"
        | "hg明朝b"
        | "hgs明朝b"
        | "ipaexmincho"
        | "ipamincho"
        | "ipapmincho"
        | "noto serif cjk jp"
        | "noto serif jp"
        | "source han serif jp" => MINCHO,
        "游ゴシック"
        | "yu gothic"
        | "yugothic"
        | "游ゴシック light"
        | "yu gothic light"
        | "游ゴシック medium"
        | "yu gothic medium"
        | "yu gothic ui"
        | "ms gothic"
        | "ms ゴシック"
        | "ms pgothic"
        | "ms pゴシック"
        | "ms ui gothic"
        | "meiryo"
        | "メイリオ"
        | "meiryo ui"
        | "hiragino sans"
        | "hiragino kaku gothic pron"
        | "hiragino kaku gothic pro"
        | "hiragino kaku gothic std"
        | "hiragino kaku gothic stdn"
        | "ヒラギノ角ゴ pron"
        | "ヒラギノ角ゴ pro"
        | "ヒラギノ角ゴシック"
        | "biz udgothic"
        | "biz udpgothic"
        | "hggothice"
        | "hgpgothice"
        | "hgsgothice"
        | "hggothicm"
        | "hgpgothicm"
        | "hgsgothicm"
        | "ipaexgothic"
        | "ipagothic"
        | "ipapgothic"
        | "noto sans cjk jp"
        | "noto sans jp"
        | "source han sans jp" => GOTHIC,
        "等线" | "dengxian" | "等线 light" | "dengxian light" | "微软雅黑" | "microsoft yahei" | "microsoft yahei ui" | "黑体" | "simhei"
        | "pingfang sc" | "苹方-简" | "hiragino sans gb" | "noto sans cjk sc" | "noto sans sc" | "source han sans sc" | "source han sans cn" => {
            SANS_SC
        }
        "宋体"
        | "simsun"
        | "新宋体"
        | "nsimsun"
        | "仿宋"
        | "fangsong"
        | "楷体"
        | "kaiti"
        | "songti sc"
        | "stsong"
        | "华文宋体"
        | "noto serif cjk sc"
        | "noto serif sc"
        | "source han serif sc"
        | "source han serif cn" => SERIF_SC,
        "微軟正黑體"
        | "microsoft jhenghei"
        | "microsoft jhenghei ui"
        | "pingfang tc"
        | "pingfang hk"
        | "苹方-繁"
        | "noto sans cjk tc"
        | "noto sans tc"
        | "source han sans tc"
        | "source han sans tw" => SANS_TC,
        "新細明體"
        | "pmingliu"
        | "細明體"
        | "mingliu"
        | "標楷體"
        | "dfkai-sb"
        | "songti tc"
        | "noto serif cjk tc"
        | "noto serif tc"
        | "source han serif tc"
        | "source han serif tw" => SERIF_TC,
        "맑은 고딕"
        | "malgun gothic"
        | "굴림"
        | "gulim"
        | "돋움"
        | "dotum"
        | "apple sd gothic neo"
        | "applegothic"
        | "noto sans cjk kr"
        | "noto sans kr"
        | "source han sans kr"
        | "source han sans k"
        | "nanumgothic"
        | "nanum gothic" => SANS_KR,
        "바탕"
        | "batang"
        | "궁서"
        | "gungsuh"
        | "applemyungjo"
        | "noto serif cjk kr"
        | "noto serif kr"
        | "source han serif kr"
        | "source han serif k"
        | "nanummyeongjo"
        | "nanum myeongjo" => SERIF_KR,
        _ => return None,
    };
    Some(chain)
}

/// Is `family` a Chinese, Japanese or Korean document font by name (one with CJK substitutes)?
/// A run's East Asian font that isn't installed is then drawn with its substitute instead of the
/// per-character fallback faces.
pub fn is_cjk_family(family: &str) -> bool {
    cjk_substitutes(&family.to_ascii_lowercase()).is_some()
}

/// The font a Windows 3.1-era charset name stands for: `Times New Roman Cyr`, `Arial CE`,
/// `Courier New Baltic`, `Arial (Hebrew)`… are aliases Windows keeps (its FontSubstitutes) for
/// the font itself, and old Russian and Central European documents name them. Other names are
/// returned unchanged.
pub fn charset_base_family(family: &str) -> &str {
    const SUFFIXES: &[&str] = &[" cyr", " ce", " greek", " tur", " baltic", " (arabic)", " (hebrew)", " (vietnamese)"];
    let lower = family.trim_end().to_ascii_lowercase();
    SUFFIXES
        .iter()
        .find(|s| lower.ends_with(*s) && lower.len() > s.len())
        .and_then(|s| family.trim_end().get(..lower.len() - s.len()))
        .map_or(family, str::trim_end)
}

/// Naskh-style Persian/Arabic document fonts (the "B …" family names come from the common
/// Persian font packs; matched by name only).
fn is_persian_naskh(f: &str) -> bool {
    const NAMES: &[&str] = &[
        "b nazanin",
        "b lotus",
        "b zar",
        "b mitra",
        "b badr",
        "b yagut",
        "b roya",
        "b compset",
        "b nazanin bold",
        "b lotus bold",
        "nazanin",
        "lotus",
        "zar",
        "mitra",
        "badr",
        "yagut",
        "roya",
        "ir nazanin",
        "ir lotus",
        "ir zar",
        "ir mitra",
        "ir badr",
        "irnazanin",
        "irlotus",
        "irzar",
        "irmitra",
        "irbadr",
        "traditional arabic",
        "simplified arabic",
        "arabic typesetting",
        "sakkal majalla",
        "adobe arabic",
        "andalus",
        "times new roman (arabic)",
    ];
    NAMES.contains(&f)
}

/// Sans Persian/Arabic fonts for headings and screens.
fn is_persian_sans(f: &str) -> bool {
    const NAMES: &[&str] = &[
        "b yekan",
        "b titr",
        "b homa",
        "b koodak",
        "b traffic",
        "b jadid",
        "b elham",
        "b kamran",
        "b davat",
        "b ferdosi",
        "yekan",
        "titr",
        "homa",
        "koodak",
        "traffic",
        "ir titr",
        "ir homa",
        "irtitr",
        "irhoma",
        "iransans",
        "iransansweb",
        "iran sans",
        "iranyekan",
        "iran yekan",
        "iranyekanweb",
        "dana",
        "estedad",
        "dubai",
        "aldhabi",
        "urdu typesetting",
        "arabic transparent",
    ];
    NAMES.contains(&f) || f.starts_with("iransans") || f.starts_with("iranyekan")
}

/// Hebrew document fonts.
fn is_hebrew(f: &str) -> bool {
    const NAMES: &[&str] = &["david", "miriam", "noto sans hebrew", "arial hebrew", "calibri hebrew"];
    NAMES.contains(&f) || f.contains("hebrew")
}

fn style_name(bold: bool, italic: bool) -> &'static str {
    match (bold, italic) {
        (false, false) => "Regular",
        (true, false) => "Bold",
        (false, true) => "Italic",
        (true, true) => "Bold Italic",
    }
}

/// Resolve `family` with bold/italic to a face (memoised until user fonts are added or removed,
/// [`FontDb::generation`]).
pub fn resolve(family: &str, bold: bool, italic: bool) -> Resolved {
    type Memo = (u64, HashMap<(String, bool, bool), Resolved>);
    static CACHE: OnceLock<Mutex<Memo>> = OnceLock::new();
    let cache = CACHE.get_or_init(|| Mutex::new((0, HashMap::new())));
    let db = FontDb::global();
    let generation = db.generation();
    let key = (family.to_string(), bold, italic);
    {
        let mut c = cache.lock().unwrap_or_else(|e| e.into_inner());
        if c.0 != generation {
            *c = (generation, HashMap::new());
        }
        if let Some(r) = c.1.get(&key) {
            return *r;
        }
    }
    let base = charset_base_family(family);
    let (fam, substituted) = if db.has_family(family) {
        (family.to_string(), false)
    } else if base != family && db.has_family(base) {
        // Windows maps the charset names to the font itself: not a substitution.
        (base.to_string(), false)
    } else {
        let s = substitutes(base).iter().find(|s| db.has_family(s)).copied().unwrap_or(crate::FALLBACK_FAMILY);
        (s.to_string(), true)
    };
    let face = db.face(&fam, style_name(bold, italic));
    let r = Resolved { synth_bold: bold && face.weight < 600.0, synth_italic: italic && !face.italic, substituted, face: FaceRef::of(&face) };
    let mut c = cache.lock().unwrap_or_else(|e| e.into_inner());
    if c.0 == generation {
        c.1.insert(key, r);
    }
    r
}

/// The face for Hangul a run's fonts lack, as Word for Mac picks it (2026-10-19): Batang when
/// the run's East Asian font (`w:eastAsia`, else its font) is an oldstyle serif (OS/2 family
/// class 1: Times New Roman, Hiragino Mincho ProN) or one of Word's serif CJK families (游明朝,
/// [`is_cjk_serif_family`]), Malgun Gothic otherwise (Arial, Aptos, Georgia, Cambria, Courier
/// New, Baskerville, Palatino, Hiragino Sans, 游ゴシック). Both come with Office; the Korean faces
/// installed stand in for them — AppleMyungjo and AppleGothic on a Mac, an em per syllable across
/// a line and along a column like Word's. `None` when no Korean face is installed.
pub fn hangul_fallback(serif: bool, bold: bool, italic: bool) -> Option<FaceRef> {
    const SERIF: &[&str] = &[
        "Batang",
        "AppleMyungjo",
        "Noto Serif CJK KR",
        "Noto Serif KR",
        "Source Han Serif KR",
        "Source Han Serif K",
        "NanumMyeongjo",
        "Nanum Myeongjo",
        "Gungsuh",
    ];
    const SANS: &[&str] = &[
        "Malgun Gothic",
        "AppleGothic",
        "Noto Sans CJK KR",
        "Noto Sans KR",
        "Source Han Sans KR",
        "Source Han Sans K",
        "NanumGothic",
        "Nanum Gothic",
        "Gulim",
        "Dotum",
        "Apple SD Gothic Neo",
    ];
    let list = if serif { SERIF } else { SANS };
    list.iter().map(|f| resolve(f, bold, italic)).find(|r| !r.substituted && r.face.covers('한')).map(|r| r.face)
}

/// The face for the Chinese and Japanese characters a run's fonts lack: Word for Mac draws them
/// in ＭＳ 明朝 whatever the run's language (Times New Roman, Arial and Georgia text tagged ja-JP,
/// zh-CN, zh-TW, ko-KR and en-US alike, 2026-10-19) — here its substitute, when it covers them
/// all.
pub fn han_fallback(chars: &[char], bold: bool, italic: bool) -> Option<FaceRef> {
    let r = resolve("MS Mincho", bold, italic);
    (r.face.is_east_asian() && chars.iter().all(|c| r.face.covers(*c))).then_some(r.face)
}

/// A line's (ascent, descent) from hhea values, both positive: the line gap goes above the ascent.
fn hhea_line(ascender: i16, descender: i16, line_gap: i16) -> (f64, f64) {
    (f64::from(ascender) + f64::from(line_gap.max(0)), f64::from(descender).abs())
}

/// Line metrics for a face in font units: (ascent, descent), both positive, the way Word for
/// Mac measures single line spacing. A Latin face: the hhea values with the line gap above the
/// ascent (Word puts the first baseline that far below the line top; Times New Roman and Arial
/// at 12 and 48 pt on the 18-pt grid, 2026-10-19), falling back to OS/2 usWinAscent/usWinDescent.
/// An East Asian face: 1.3 times its hhea ascent + descent, the line gap ignored, with that
/// box centred in the line (0.15 of it above the ascent and below the descent) — Hiragino
/// Mincho ProN (hhea 0.88/0.12 with a 0.5-em gap), Toppan Bunkyu Mincho, MS Mincho and MS
/// Gothic give 15.6-pt lines at 12 pt and 62.4 at 48 without a grid, Songti SC and Kaiti SC
/// (1.06/0.34) 21.84 and 87.36, Apple SD Gothic Neo (0.9/0.3) 18.72, AppleMyungjo (0.869/0.318)
/// 18.48 and 74.16, Osaka (1.0/0.25 with a gap) 19.44 and 78.0; on the 18-pt grid the baselines
/// sit 1.03 em (Hiragino) and 1.01 em (MS Mincho) below the line top, to 0.2 pt at 8–48 pt
/// (`hg1`, `lm1`–`lm5` probes).
pub fn line_metrics(face: &crate::FontFace) -> (f64, f64) {
    static CACHE: OnceLock<Mutex<HashMap<u32, (f64, f64)>>> = OnceLock::new();
    let cache = CACHE.get_or_init(|| Mutex::new(HashMap::new()));
    if let Some(v) = cache.lock().unwrap_or_else(|e| e.into_inner()).get(&face.id()) {
        return *v;
    }
    let mut v = (face.ascent.max(1.0), face.descent.abs());
    if let Some(f) = face.skrifa() {
        let ok = |m: &(f64, f64)| m.0 > 0.0 && m.0 + m.1 <= face.upem * 2.0;
        let hhea = f.hhea().ok().map(|h| (h.ascender().to_i16(), h.descender().to_i16(), h.line_gap().to_i16()));
        if face.is_east_asian() {
            let (a, d) = hhea.map(|(a, d, _)| (f64::from(a), f64::from(d).abs())).filter(ok).unwrap_or(v);
            let line = EAST_ASIAN_LINE * (a + d);
            let asc = a + (line - (a + d)) / 2.0;
            v = (asc, line - asc);
        } else {
            // hhea ascender + descender + line gap, the gap above the ascent; OS/2 win metrics
            // only when the hhea values are missing, and never more than twice the em.
            let win = f.os2().ok().map(|o| (o.us_win_ascent() as f64, o.us_win_descent() as f64));
            if let Some(m) = hhea.map(|(a, d, g)| hhea_line(a, d, g)).filter(ok) {
                v = m;
            } else if let Some(m) = win.filter(ok) {
                v = m;
            }
        }
    }
    cache.lock().unwrap_or_else(|e| e.into_inner()).insert(face.id(), v);
    v
}

/// The line metrics (ascent, descent) in font units Word lays its own Yu Mincho and Yu Gothic
/// out with, for a document asking for one of them by its English or Japanese name (游明朝,
/// 游ゴシック and their Demibold, Light, Medium and Bold weights): Office bundles its own builds
/// of the Yu fonts, whose line is 1.43 em with the baseline 1.051 em below the line's top and
/// 0.379 em above its bottom — measured in Word for Mac 16.114 on 2026-10-18 at 10.5 and 20 pt
/// with single spacing (pitches 15.0 and 28.6 pt; the first baseline 11.04 pt below the top
/// margin; 12.48 pt below the top of an 18-pt grid line; a 20-pt line 25.0 pt below a 10.5-pt
/// one), the same for every weight. The macOS system 游明朝体/游ゴシック体 that stand in for them
/// have other metrics (hhea 0.88/0.12 with a 0.5 em gap: 1.5 em), which would put every
/// Japanese line 0.07 em further apart than Word. `None` for other families: their faces' own
/// metrics apply ([`line_metrics`]).
pub fn office_line_metrics(family: &str, face: &crate::FontFace) -> Option<(f64, f64)> {
    if is_office_yu(family) {
        return Some((1.051 * face.upem, 0.379 * face.upem));
    }
    // Word's own MS Mincho and MS Gothic (ascent 0.859 em, descent 0.141: a 1-em box) centred
    // in Word for Mac's 1.3-em East Asian line: the baseline 1.009 em below the line's top
    // (`lm1`–`lm3` probes, 12 and 48 pt, with and without the grid), whatever face stands in.
    is_office_ms(family)
        .then_some(((MS_ASCENT + (EAST_ASIAN_LINE - 1.0) / 2.0) * face.upem, (1.0 - MS_ASCENT + (EAST_ASIAN_LINE - 1.0) / 2.0) * face.upem))
}

/// Where a 縦書き column's upright em squares are centred, in font units on the ascent side of
/// the column's baseline (the line turned on its side: the baseline sits the line's ascent from
/// the column's top edge, where lying Latin stands): half the difference of the OS/2 win ascent
/// and descent — measured in Word for Mac at 12, 24 and 36 pt (`vt3`–`vt5` probes, 2026-10-19):
/// Hiragino Mincho ProN (win 0.985/0.182) 0.40 em, Toppan Bunkyu Mincho (1.295/0.326) 0.48,
/// Songti SC (0.86/0.14) 0.36, Word's own Yu Mincho 0.34 and MS Mincho/MS Gothic 0.36 (their
/// line metrics' half difference, whatever face stands in). Without win metrics, half the hhea
/// difference.
pub fn vertical_em_centre(family: &str, face: &crate::FontFace) -> f64 {
    if is_office_yu(family) {
        return (1.051 - 0.379) / 2.0 * face.upem;
    }
    if is_office_ms(family) {
        return (MS_ASCENT - (1.0 - MS_ASCENT)) / 2.0 * face.upem;
    }
    let ok = |a: f64, d: f64| a > 0.0 && a + d <= face.upem * 2.0;
    let win =
        face.skrifa().and_then(|f| f.os2().ok()).map(|o| (f64::from(o.us_win_ascent()), f64::from(o.us_win_descent()))).filter(|&(a, d)| ok(a, d));
    let (a, d) = win.unwrap_or((face.ascent.max(1.0), face.descent.abs()));
    (a - d) / 2.0
}

/// Word for Mac's line for an East Asian face as a multiple of its hhea ascent + descent (1.3
/// em for the usual 1-em box; 1.82 for Songti SC's 1.4; the line gap plays no part).
pub const EAST_ASIAN_LINE: f64 = 1.3;
/// MS Mincho's and MS Gothic's ascent in em (their descent is the rest of the em).
const MS_ASCENT: f64 = 0.859;
/// The top of MS Mincho's and MS Gothic's strikethrough above the baseline, in em.
const MS_STRIKE: f32 = 0.258;

/// Is `family` one of the MS Mincho / MS Gothic families Word bundles (by its English or
/// Japanese name)?
pub fn is_office_ms(family: &str) -> bool {
    const MS: &[&str] =
        &["MS Mincho", "MS PMincho", "MS Gothic", "MS PGothic", "MS UI Gothic", "ＭＳ 明朝", "ＭＳ Ｐ明朝", "ＭＳ ゴシック", "ＭＳ Ｐゴシック"];
    let base = fold_cjk_name(family);
    MS.iter().any(|n| fold_cjk_name(n) == base)
}

/// Is `family` one of the Yu Mincho / Yu Gothic families Office bundles its own builds of (by
/// its English or Japanese name, any weight)?
pub fn is_office_yu(family: &str) -> bool {
    const YU: &[&str] = &[
        "Yu Mincho",
        "Yu Mincho Demibold",
        "Yu Mincho Light",
        "Yu Gothic",
        "Yu Gothic Light",
        "Yu Gothic Medium",
        "Yu Gothic Bold",
        "游明朝",
        "游明朝 Demibold",
        "游明朝 Light",
        "游ゴシック",
        "游ゴシック Light",
        "游ゴシック Medium",
        "游ゴシック Bold",
    ];
    let base = fold_cjk_name(family);
    YU.iter().any(|n| fold_cjk_name(n) == base)
}

/// Where Word draws a run's underline and strikethrough, in points for text of `size` points.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Decorations {
    /// The underline's top edge below the baseline.
    pub underline_top: f32,
    pub underline_thickness: f32,
    /// The strikethrough's top edge above the baseline.
    pub strike_top: f32,
    pub strike_thickness: f32,
}

/// Is `family` Aptos (Office's default Latin font since 2023) or one of its variants?
pub fn is_office_aptos(family: &str) -> bool {
    let f = family.trim();
    f.len() >= 5 && f.get(..5).is_some_and(|h| h.eq_ignore_ascii_case("aptos")) && f.get(5..).is_some_and(|r| r.is_empty() || r.starts_with(' '))
}

/// The underline and strikethrough geometry Word for Mac draws for `size`-point text of
/// `family` drawn with `face`: the face's own `post` and OS/2 suggestions (an underline whose
/// top is the table's offset below the baseline, a strike whose top is OS/2's offset above it),
/// except for the fonts Office bundles, which another face stands in for here — measured in
/// Word for Mac 16.114 on 2026-10-19:
/// - Word's own Yu Mincho / Yu Gothic builds ([`is_office_yu`]), at 8–48 pt: the underline's
///   top edge 0.153 em + 0.53 pt below the baseline (1.68 pt at 8, 2.16 at 10.5, 4.08 at 24,
///   7.92 at 48 — not a fixed share of the em), 0.05 em thick; the strikethrough's top 0.42 em
///   above the baseline, 0.045 em thick (4.32 and 0.48 at 10.5, 20.16 and 2.16 at 48). The
///   macOS 游明朝体 says −0.075 em / 0.05 em and 0.299 em / 0.05 em, which would put the
///   underline a point too close to 10.5-pt text.
/// - Aptos ([`is_office_aptos`]), at 10.5, 16 and 24 pt: the underline's top 0.05 em below the
///   baseline, 0.025 em thick (0.48 and 0.24 at 10.5); the strikethrough's top 0.36 em above,
///   0.05 em thick (3.84 and 0.48 at 10.5).
///
/// Hostile sizes are clamped.
pub fn decoration_metrics(family: &str, face: &crate::FontFace, size: f32) -> Decorations {
    let size = if size.is_finite() { size.clamp(0.0, 1638.0) } else { 0.0 };
    if is_office_yu(family) {
        return Decorations {
            underline_top: 0.153 * size + 0.53,
            underline_thickness: 0.05 * size,
            strike_top: 0.42 * size,
            strike_thickness: 0.045 * size,
        };
    }
    if is_office_aptos(family) {
        return Decorations { underline_top: 0.05 * size, underline_thickness: 0.025 * size, strike_top: 0.36 * size, strike_thickness: 0.05 * size };
    }
    let k = size as f64 / face.upem.max(1.0);
    let (uo, ut) = face.underline;
    let (so, st) = face.strikeout;
    // An East Asian face's underline: its top edge 0.7 of the hhea descent + 0.53 pt below the
    // baseline, the post thickness — Hiragino Mincho ProN (descent 0.12 em) 1.20 at 8 pt, 1.92
    // at 18, 4.56 at 48; Toppan Bunkyu Mincho the same; MS Mincho and MS Gothic (0.141) 1.68 at
    // 12, 5.28 at 48; Word's Yu fonts have their own rule above.
    let hhea_descent = face.skrifa().and_then(|f| f.hhea().ok()).map(|h| f64::from(h.descender().to_i16()).abs() / face.upem.max(1.0));
    let descent = if is_office_ms(family) { 1.0 - MS_ASCENT } else { hhea_descent.unwrap_or(face.descent.abs() / face.upem.max(1.0)) };
    let underline_top = if is_office_ms(family) || face.is_east_asian() { (0.7 * descent * size as f64) as f32 + 0.53 } else { (-uo * k) as f32 };
    let underline_thickness = if is_office_ms(family) { 0.05 * size } else { (ut * k) as f32 };
    // MS Mincho and MS Gothic strike through 0.258 em above the baseline (the top of a 0.05-em
    // line: 3.08 pt at 12, 12.50 at 48, where Times New Roman's and Arial's tables put theirs),
    // whatever stands in for them; Hiragino (0.322), Toppan (0.276), Times (0.259) and Arial are
    // their OS/2 tables (`st1`, `st2` probes).
    let (strike_top, strike_thickness) = if is_office_ms(family) { (MS_STRIKE * size, 0.05 * size) } else { ((so * k) as f32, (st * k) as f32) };
    Decorations { underline_top, underline_thickness, strike_top, strike_thickness }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn words_own_yu_fonts_take_words_line_metrics() {
        let db = FontDb::global();
        let face = resolve("Yu Mincho", false, false).face;
        let em = face.get().upem;
        for name in ["游明朝", "Yu Mincho", "游ゴシック Medium", "Yu Gothic Light", "游明朝 Demibold", "Ｙｕ　Ｇｏｔｈｉｃ"] {
            let (a, d) = office_line_metrics(name, face.get()).unwrap_or_else(|| panic!("{name}"));
            assert!((a / em - 1.051).abs() < 1e-6 && (d / em - 0.379).abs() < 1e-6, "{name}: {a} {d}");
        }
        for name in ["ヒラギノ明朝 ProN", "Aptos", "Yu Gothic UI", ""] {
            assert!(office_line_metrics(name, face.get()).is_none(), "{name}");
        }
        let _ = db;
    }

    #[test]
    fn words_own_yu_fonts_take_words_decoration_geometry() {
        let face = resolve("Yu Mincho", false, false).face;
        let d = decoration_metrics("游明朝", face.get(), 10.5);
        assert!((d.underline_top - 2.1365).abs() < 1e-3 && (d.underline_thickness - 0.525).abs() < 1e-3, "{d:?}");
        assert!((d.strike_top - 4.41).abs() < 1e-3 && (d.strike_thickness - 0.4725).abs() < 1e-3, "{d:?}");
        let d48 = decoration_metrics("Yu Gothic Medium", face.get(), 48.0);
        assert!((d48.underline_top - 7.874).abs() < 1e-3 && (d48.strike_top - 20.16).abs() < 1e-3, "{d48:?}");
        // Aptos: Word's geometry for it, whatever face stands in.
        let face = resolve("Aptos", false, false).face.get();
        let d = decoration_metrics("Aptos", face, 10.5);
        assert!((d.underline_top - 0.525).abs() < 1e-3 && (d.underline_thickness - 0.2625).abs() < 1e-3, "{d:?}");
        assert!((d.strike_top - 3.78).abs() < 1e-3 && (d.strike_thickness - 0.525).abs() < 1e-3, "{d:?}");
        assert!(is_office_aptos("Aptos Display") && is_office_aptos("aptos narrow") && !is_office_aptos("Aptosaurus") && !is_office_aptos(""));
        // Another family: the face's own tables, scaled to the size.
        let face = resolve("Source Sans 3", false, false).face.get();
        let d = decoration_metrics("Source Sans 3", face, 20.0);
        let k = 20.0 / face.upem;
        assert!((d.underline_top as f64 + face.underline.0 * k).abs() < 1e-3, "{d:?} vs {:?}", face.underline);
        assert!((d.underline_thickness as f64 - face.underline.1 * k).abs() < 1e-3);
        assert!((d.strike_top as f64 - face.strikeout.0 * k).abs() < 1e-3 && (d.strike_thickness as f64 - face.strikeout.1 * k).abs() < 1e-3);
        assert!(d.underline_top > 0.0 && d.strike_top > 0.0 && d.underline_thickness > 0.0, "{d:?}");
        // Hostile sizes never produce NaN or infinity.
        for v in [f32::NAN, f32::INFINITY, -5.0, 1e9] {
            let d = decoration_metrics("游明朝", face, v);
            assert!(d.underline_top.is_finite() && d.strike_top.is_finite(), "{v}: {d:?}");
        }
    }

    /// Word for Mac's line for East Asian faces (1.3 times the hhea box, centred) and MS
    /// Mincho's metrics by name, their underlines 0.7 of the descent + 0.53 pt down and MS
    /// Mincho's strikethrough 0.258 em up; Latin faces keep the gap above the ascent and their
    /// `post` underline.
    #[test]
    fn hangul_fallback_is_batang_for_oldstyle_serifs_and_malgun_gothic_otherwise() {
        if let (Some(s), Some(g)) = (hangul_fallback(true, false, false), hangul_fallback(false, false, false)) {
            assert!(s.covers('한') && g.covers('한'));
            if !resolve("AppleMyungjo", false, false).substituted {
                assert_eq!(s.family, "AppleMyungjo");
            }
            if !resolve("AppleGothic", false, false).substituted {
                assert_eq!(g.family, "AppleGothic");
            }
        }
        let times = resolve("Times New Roman", false, false);
        if !times.substituted {
            assert!(times.face.is_oldstyle_serif());
        }
        let hira = resolve("Hiragino Mincho ProN", false, false);
        if !hira.substituted {
            assert!(hira.face.is_oldstyle_serif());
        }
        for f in ["Arial", "Georgia", "Courier New", "Hiragino Sans", "Baskerville"] {
            let r = resolve(f, false, false);
            if !r.substituted {
                assert!(!r.face.is_oldstyle_serif(), "{f}");
            }
        }
        for f in ["Yu Mincho", "游明朝", "MS Mincho", "ＭＳ 明朝", "Batang", "SimSun", "PMingLiU", "Hiragino Mincho ProN"] {
            assert!(is_cjk_serif_family(f), "{f}");
        }
        for f in ["Yu Gothic", "游ゴシック", "Malgun Gothic", "Meiryo", "Arial", "Times New Roman"] {
            assert!(!is_cjk_serif_family(f), "{f}");
        }
        if let Some(f) = han_fallback(&['日', '本'], false, false) {
            assert!(f.is_east_asian() && f.covers('語'));
        }
    }

    #[test]
    fn vertical_origins_and_advances_come_from_the_font_tables() {
        // Word for Mac hangs an upright glyph from its vertical origin — VORG, else yMax + the
        // vmtx top side bearing — and moves the column on by its vmtx advance (vt6/vt7, 2026-10-19).
        for (family, c, vadv, origin) in [
            ("Apple SD Gothic Neo", '글', 1200.0, 900.0),
            ("AppleMyungjo", '글', 1025.0, 887.0),
            ("AppleGothic", '글', 1004.0, 847.0),
            ("Songti SC", '漢', 1000.0, 800.0),
            ("Hiragino Mincho ProN", '縦', 1000.0, 880.0),
        ] {
            let r = resolve(family, false, false);
            if r.substituted || !r.face.covers(c) {
                continue;
            }
            let f = r.face.get();
            let gid = f.glyph_for(c);
            assert_eq!(f.vertical_advance(gid), Some(vadv), "{family} {c}");
            assert!((f.vertical_origin(gid) - origin).abs() < 0.5, "{family} {c}: {}", f.vertical_origin(gid));
        }
    }

    #[test]
    fn east_asian_faces_take_words_for_mac_line_and_underline() {
        let r = resolve("Hiragino Mincho ProN", false, false);
        let f = r.face.get();
        if !r.substituted && f.is_east_asian() {
            use skrifa::raw::TableProvider;
            let (a, d) = line_metrics(f);
            let em = f.upem;
            let h = f.skrifa().and_then(|s| s.hhea().ok()).expect("hhea");
            let (ha, hd) = (f64::from(h.ascender().to_i16()), f64::from(h.descender().to_i16()).abs());
            assert!(((a + d) - 1.3 * (ha + hd)).abs() < 1e-6, "{a} {d} vs {ha} {hd}");
            assert!((a - (ha + 0.15 * (ha + hd))).abs() < 1e-6, "{a} vs {ha} {hd}");
            assert!(((a + d) / em - 1.3).abs() < 1e-6, "Hiragino's box is an em: {a} {d}");
            let dm = decoration_metrics("Hiragino Mincho ProN", f, 48.0);
            let want = (0.7 * hd / em * 48.0) as f32 + 0.53;
            assert!((dm.underline_top - want).abs() < 1e-3 && (dm.underline_thickness - 2.4).abs() < 0.05, "{dm:?}");
        }
        // MS Mincho by either name, whatever face stands in: 1.009 em above the baseline,
        // 0.291 below; the underline 1.68 below at 12 pt (0.72 thick), 5.28 at 48 (2.40).
        for name in ["MS Mincho", "ＭＳ 明朝", "MS PGothic", "ＭＳ Ｐゴシック", "MS UI Gothic"] {
            assert!(is_office_ms(name), "{name}");
            let (a, d) = office_line_metrics(name, f).unwrap_or_else(|| panic!("{name}"));
            assert!((a / f.upem - 1.009).abs() < 1e-6 && (d / f.upem - 0.291).abs() < 1e-6, "{name}: {a} {d}");
        }
        assert!(!is_office_ms("Yu Mincho") && !is_office_ms("Hiragino Mincho ProN") && !is_office_ms(""));
        let dm = decoration_metrics("MS Gothic", f, 48.0);
        assert!((dm.underline_top - 5.268).abs() < 0.01 && (dm.underline_thickness - 2.4).abs() < 1e-3, "{dm:?}");
        assert!((dm.strike_top - 12.384).abs() < 0.01 && (dm.strike_thickness - 2.4).abs() < 1e-3, "{dm:?}");
        let dm = decoration_metrics("ＭＳ 明朝", f, 12.0);
        assert!((dm.underline_top - 1.714).abs() < 0.01 && (dm.underline_thickness - 0.6).abs() < 1e-3, "{dm:?}");
        assert!((dm.strike_top - 3.096).abs() < 0.01 && (dm.strike_thickness - 0.6).abs() < 1e-3, "{dm:?}");
        // A wider hhea box makes a taller line: Songti SC (1.06/0.34) 1.82 em, Apple SD Gothic
        // Neo (0.9/0.3) 1.56, Osaka (1.0/0.25 with a line gap Word ignores) 1.625. A 縦書き
        // column's em centre: half the win ascent − descent (Songti 0.36 em, Toppan Bunkyu
        // Mincho 0.4845, Hiragino 0.4015).
        for (name, want, centre) in [
            ("Songti SC", 1.82, 0.36),
            ("Apple SD Gothic Neo", 1.56, 0.30),
            ("Osaka", 1.625, 0.375),
            ("Toppan Bunkyu Mincho", 1.3, 0.4845),
            ("Hiragino Mincho ProN", 1.3, 0.4015),
        ] {
            let r = resolve(name, false, false);
            let f = r.face.get();
            if !r.substituted && f.is_east_asian() {
                let (a, d) = line_metrics(f);
                assert!(((a + d) / f.upem - want).abs() < 1e-3, "{name}: {a} {d}");
                let c = vertical_em_centre(name, f) / f.upem;
                assert!((c - centre).abs() < 1e-3 || name == "Osaka", "{name}: centre {c}");
            }
        }
        assert!((vertical_em_centre("游明朝", f) / f.upem - 0.336).abs() < 1e-6);
        assert!((vertical_em_centre("MS Gothic", f) / f.upem - 0.359).abs() < 1e-6);
        let t = resolve("Times New Roman", false, false);
        if !t.substituted && !t.face.get().is_east_asian() {
            let tf = t.face.get();
            let (a, d) = line_metrics(tf);
            assert!(((a + d) / tf.upem - 1.15).abs() < 0.01, "{a} {d}");
            let dm = decoration_metrics("Times New Roman", tf, 48.0);
            assert!((dm.underline_top - 5.23).abs() < 0.3, "{dm:?}");
        }
    }

    #[test]
    fn the_line_gap_goes_above_the_ascent() {
        // Times New Roman (hhea 1825 / -443, gap 87), and a font with a large gap: Word puts the
        // first baseline the ascent plus the whole gap below the line top.
        assert_eq!(hhea_line(1825, -443, 87), (1912.0, 443.0));
        assert_eq!(hhea_line(1536, -512, 452), (1988.0, 512.0));
        // A negative gap adds nothing; the most negative descender doesn't overflow.
        assert_eq!(hhea_line(1000, -300, -50), (1000.0, 300.0));
        assert_eq!(hhea_line(1000, i16::MIN, 0), (1000.0, 32768.0));
    }

    #[test]
    fn cjk_document_fonts_have_substitutes_by_either_name() {
        // Word's theme and template fonts by their English or native names, Japanese Mincho and
        // Gothic apart, and the Chinese and Korean families in their own chains.
        let chain = |f: &str| substitutes(f);
        assert_eq!(chain("游明朝"), chain("Yu Mincho"));
        assert_eq!(chain("ＭＳ 明朝"), chain("MS PMincho"));
        assert_ne!(chain("游明朝"), chain("游ゴシック"));
        assert_eq!(chain("メイリオ"), chain("Meiryo UI"));
        assert!(chain("游明朝").contains(&"Hiragino Mincho ProN") && chain("游明朝").contains(&"Noto Serif CJK JP"));
        assert!(chain("Yu Gothic").contains(&"Hiragino Sans") && chain("Yu Gothic").contains(&"BIZ UDPGothic"));
        assert_eq!(chain("等线"), chain("DengXian"));
        assert_eq!(chain("宋体"), chain("SimSun"));
        assert_eq!(chain("新細明體"), chain("PMingLiU"));
        // The names Word shows in its English and native interfaces.
        assert_eq!(cjk_font_names("游明朝"), Some(("Yu Mincho", "游明朝", "ja")));
        assert_eq!(cjk_font_names("ms p明朝"), Some(("MS PMincho", "ＭＳ Ｐ明朝", "ja")));
        assert_eq!(cjk_font_names("맑은 고딕").map(|n| n.0), Some("Malgun Gothic"));
        assert_eq!(cjk_font_names("Aptos"), None);
        assert_eq!(chain("맑은 고딕"), chain("Malgun Gothic"));
        assert!(is_cjk_family("ＭＳ 明朝") && is_cjk_family("yu gothic ui") && !is_cjk_family("Arial") && !is_cjk_family(""));
        // A CJK name resolves to a face (installed, a substitute, or the last resort) without panicking.
        let _ = resolve("游明朝", true, false);
    }

    #[test]
    fn unknown_family_substitutes() {
        let r = resolve("Definitely Not A Font 123", false, false);
        assert!(r.substituted);
        let b = resolve("Source Serif 4", true, false);
        assert!(!b.substituted);
        assert!(!b.synth_bold);
        let (a, d) = line_metrics(&b.face);
        assert!(a > 0.0 && d >= 0.0);
    }

    #[test]
    fn palatino_family_prefers_palatino_look_alikes_over_generic_serif() {
        for name in ["Palatino Linotype", "Book Antiqua", "palatino"] {
            let list = substitutes(name);
            let pos = |n: &str| list.iter().position(|x| *x == n);
            assert!(pos("Palatino").is_some() && pos("TeX Gyre Pagella").is_some() && pos("P052").is_some(), "{name}");
            // All of them come before the generic serif fallbacks (Caladea, Times New Roman).
            assert!(pos("Palatino") < pos("Caladea") && pos("TeX Gyre Pagella") < pos("Times New Roman"), "{name}");
        }
        // Other serif families are unchanged.
        assert_eq!(substitutes("Georgia").first(), Some(&"Caladea"));
        // Charset aliases are the font itself, so they substitute like it.
        assert_eq!(charset_base_family("Times New Roman Cyr"), "Times New Roman");
        assert_eq!(charset_base_family("Arial CYR"), "Arial");
        assert_eq!(charset_base_family("Courier New CE"), "Courier New");
        assert_eq!(charset_base_family("Arial (Hebrew)"), "Arial");
        assert_eq!(charset_base_family("Cyr"), "Cyr");
        assert_eq!(charset_base_family("Calibri"), "Calibri");
        assert_eq!(substitutes(charset_base_family("Times New Roman Cyr")), substitutes("Times New Roman"));
    }

    #[test]
    fn symbol_fonts_substitute_symbol_faces() {
        for f in ["Symbol", "Wingdings", "Wingdings 2", "Wingdings 3", "Webdings"] {
            assert!(substitutes(f).contains(&"Noto Sans Symbols 2"), "{f}");
        }
    }

    #[test]
    fn resolves_weights_named_as_their_own_family() {
        // Word stores a Medium face as the family "Inter Medium" (#219).
        let r = resolve("Inter Medium", false, false);
        assert!(!r.substituted && !r.synth_bold && !r.synth_italic);
        assert_eq!((r.face.family.as_str(), r.face.style.as_str()), ("Inter", "Medium"));
        // Bold and italic within it are drawn from the Medium face.
        let b = resolve("Inter Medium", true, true);
        assert_eq!(b.face.style, "Medium");
        assert!(!b.substituted && b.synth_bold && b.synth_italic);
    }

    #[test]
    fn synthesizes_missing_styles() {
        // JetBrains Mono ships only Regular here.
        let r = resolve("JetBrains Mono", true, true);
        assert!(r.synth_bold && r.synth_italic);
    }

    #[test]
    fn hebrew_fonts_substitute_hebrew_faces() {
        for f in ["David", "Miriam", "Arial Hebrew", "Calibri Hebrew", "Noto Sans Hebrew"] {
            assert!(substitutes(f).contains(&"Noto Sans Hebrew"), "{f}");
            // David is a Windows font; only the Windows list offers it.
            if cfg!(target_os = "windows") {
                assert!(substitutes(f).contains(&"David") || f == "David", "{f}");
            }
        }
        // Generic names with "Hebrew" in them also get Hebrew substitutions
        assert!(substitutes("SomeFont Hebrew").contains(&"Noto Sans Hebrew"));
    }
}

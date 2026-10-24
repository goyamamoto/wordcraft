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
///
/// A font Word for Mac doesn't have either is drawn as Word draws it (observed with Japanese
/// editing, `fj1`–`fj3` probes, 2026-10-19): one with a Chinese, Japanese or Korean name (小塚明朝
/// Pr6N R, HGP創英角ﾎﾟｯﾌﾟ体, 方正书宋简体, 함초롬바탕, 游ゴシック体, ヒラギノ丸ゴ ProN…) in 游ゴシック
/// for every character, Latin included; one with a Latin name (Frutiger, Klee, Noto Sans JP,
/// Kozuka Mincho Pr6N, a nonsense name — whatever the font table says of it) in Cambria, its
/// East Asian characters in ＭＳ 明朝 ([`han_fallback`]); a Helvetica (Helvetica Neue LT Std) in
/// Arial. UD デジタル 教科書体 is Word's one named exception: Hiragino Sans W4.
fn substitutes(family: &str) -> &'static [&'static str] {
    substitutes_by_name(family).0
}

/// Does `family` have substitutes of its own, picked by its name (a metric-compatible clone such
/// as Liberation Serif for Times New Roman, or the same design under another name), rather than
/// the generic lists any unknown font gets?
pub fn has_named_substitutes(family: &str) -> bool {
    substitutes_by_name(charset_base_family(family)).1
}

/// [`substitutes`], and whether they were chosen for this family by name.
fn substitutes_by_name(family: &str) -> (&'static [&'static str], bool) {
    const SANS: &[&str] = &["Carlito", "Helvetica Neue", "Arial", "Liberation Sans", "Noto Sans", "DejaVu Sans", "Source Sans 3"];
    const SERIF: &[&str] = &["Caladea", "Times New Roman", "Liberation Serif", "Georgia", "Noto Serif", "DejaVu Serif", "Source Serif 4"];
    const MONO: &[&str] = &["Courier New", "Liberation Mono", "Menlo", "Consolas", "DejaVu Sans Mono", "JetBrains Mono"];
    const ARIAL: &[&str] = &["Liberation Sans", "Helvetica", "Helvetica Neue", "Arimo", "Source Sans 3"];
    const ARIAL_UNICODE: &[&str] = &["Arial", "Liberation Sans", "Helvetica", "Helvetica Neue", "Arimo", "Source Sans 3"];
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
    // Thai document fonts (TH Sarabun New, Cordia New, the UPC families…): Thai faces, every one
    // with Thai letters; [`substitute`] prefers one with Latin letters and digits too, so a run
    // mixing Thai and Latin keeps one font at one size, as Word substitutes Cordia New for the
    // whole run (#1549). Windows' own Thai fonts first, then the Mac's, then the open Noto and Thai
    // Linux Working Group families. The Mac's Thonburi and Sathu carry Apple's `morx` tables
    // instead of OpenType ones; the shaper applies those, so their marks stack as they should.
    #[cfg(windows)]
    const THAI: &[&str] = &["Cordia New", "Leelawadee UI", "Leelawadee", "Tahoma", "Noto Sans Thai", "Browallia New", "Angsana New"];
    #[cfg(target_os = "macos")]
    const THAI: &[&str] = &["Thonburi", "Ayuthaya", "Sathu", "Krungthep", "Noto Sans Thai", "Tahoma"];
    #[cfg(not(any(windows, target_os = "macos")))]
    const THAI: &[&str] = &["Noto Sans Thai", "Laksaman", "Garuda", "Waree", "Tlwg Typo", "Loma", "Umpush", "Noto Serif Thai"];
    // Serif Thai document fonts (Angsana New, EucrosiaUPC, Norasi…) prefer a serif Thai face.
    #[cfg(windows)]
    const THAI_SERIF: &[&str] = &["Angsana New", "AngsanaUPC", "Noto Serif Thai", "Cordia New", "Leelawadee UI", "Tahoma", "Noto Sans Thai"];
    #[cfg(target_os = "macos")]
    const THAI_SERIF: &[&str] = &["Sathu", "Noto Serif Thai", "Thonburi", "Ayuthaya", "Krungthep", "Noto Sans Thai", "Tahoma"];
    #[cfg(not(any(windows, target_os = "macos")))]
    const THAI_SERIF: &[&str] = &["Noto Serif Thai", "Norasi", "Kinnari", "Noto Sans Thai", "Laksaman", "Garuda", "Waree"];
    // Khmer document fonts (Khmer OS …, Khmer Mool1…): a missing one is drawn in the system's
    // Khmer font, as Word does, not in whatever Khmer font the document happens to load: the Khmer
    // OS fonts are wider, so lines wrap and boxes overflow where Word's don't.
    #[cfg(windows)]
    const KHMER: &[&str] = &["Leelawadee UI", "Khmer UI", "DaunPenh", "MoolBoran", "Noto Sans Khmer"];
    #[cfg(target_os = "macos")]
    const KHMER: &[&str] = &["Khmer Sangam MN", "Khmer MN", "Noto Sans Khmer"];
    #[cfg(not(any(windows, target_os = "macos")))]
    const KHMER: &[&str] = &["Noto Sans Khmer", "Noto Serif Khmer"];
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
        return (cjk, true);
    }
    if is_thai_serif(&f) {
        return (THAI_SERIF, true);
    }
    if is_thai(&f) {
        return (THAI, true);
    }
    if is_hebrew(&f) {
        return (HEBREW, true);
    }
    if is_khmer(&f) {
        return (KHMER, true);
    }
    if is_persian_naskh(&f) {
        return (NASKH, true);
    }
    if is_persian_sans(&f) {
        return (ARABIC_SANS, true);
    }
    // Any other non-Latin name is a font Word lacks: drawn in 游ゴシック (see above).
    if !f.is_ascii() {
        return (GOTHIC, false);
    }
    match f.as_str() {
        "palatino linotype" | "book antiqua" | "palatino" => (PALATINO, true),
        "arial" | "arial nova" => (ARIAL, true),
        "arial unicode ms" => (ARIAL_UNICODE, true),
        _ if f.starts_with("helvetica") => (ARIAL, true),
        "times new roman" | "times" => (TIMES, true),
        "courier new" | "courier" | "consolas" | "lucida console" | "cascadia code" | "cascadia mono" => (MONO, true),
        "symbol" | "wingdings" | "wingdings 2" | "wingdings 3" | "segoe ui symbol" | "webdings" => (SYMBOL, true),
        // Serif faces with no open clone of their own: the generic serif list, as for any font.
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
        | "aptos serif" => (SERIF, false),
        // The sans fonts Office, Windows and macOS ship: Word has them, so they take a sans
        // stand-in; any other Latin name is a font Word lacks too, drawn in Cambria.
        "calibri"
        | "calibri light"
        | "aptos"
        | "aptos display"
        | "aptos narrow"
        | "aptos light"
        | "aptos semibold"
        | "aptos extrabold"
        | "aptos black"
        | "segoe ui"
        | "segoe ui light"
        | "segoe ui semilight"
        | "segoe ui semibold"
        | "segoe ui black"
        | "segoe ui historic"
        | "segoe ui variable"
        | "segoe print"
        | "segoe script"
        | "verdana"
        | "tahoma"
        | "trebuchet ms"
        | "candara"
        | "candara light"
        | "corbel"
        | "corbel light"
        | "franklin gothic book"
        | "franklin gothic medium"
        | "franklin gothic demi"
        | "franklin gothic heavy"
        | "franklin gothic medium cond"
        | "franklin gothic demi cond"
        | "century gothic"
        | "gill sans mt"
        | "gill sans nova"
        | "gill sans"
        | "bahnschrift"
        | "bahnschrift light"
        | "bahnschrift semibold"
        | "arial black"
        | "arial narrow"
        | "arial rounded mt bold"
        | "impact"
        | "lucida sans"
        | "lucida sans unicode"
        | "lucida grande"
        | "microsoft sans serif"
        | "ms sans serif"
        | "helvetica neue"
        | "avenir"
        | "avenir next"
        | "avenir next condensed"
        | "optima"
        | "futura"
        | "geneva"
        | "tw cen mt"
        | "berlin sans fb"
        | "abadi"
        | "abadi extra light"
        | "leelawadee"
        | "leelawadee ui"
        | "nirmala ui"
        | "ebrima"
        | "gadugi"
        | "dejavu sans"
        | "liberation sans"
        | "carlito"
        | "noto sans"
        | "source sans 3" => (SANS, false),
        _ => (SERIF, false),
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
        ("Malgun Gothic Semilight", "맑은 고딕 Semilight", "ko"),
        ("GulimChe", "굴림체", "ko"),
        ("DotumChe", "돋움체", "ko"),
        ("BatangChe", "바탕체", "ko"),
        ("GungsuhChe", "궁서체", "ko"),
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
/// Word for Mac's stand-in for the UD デジタル 教科書体 fonts: Hiragino Sans (W4), then the gothic
/// chain.
const UD_KYOKASHO: &[&str] = &[
    "Hiragino Sans",
    "Yu Gothic",
    "YuGothic",
    "Hiragino Kaku Gothic ProN",
    "Meiryo",
    "BIZ UDPGothic",
    "BIZ UDGothic",
    "Noto Sans CJK JP",
    "Noto Sans JP",
    "Source Han Sans JP",
    "IPAexGothic",
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
        // Word for Mac draws the UD デジタル 教科書体 fonts it lacks in Hiragino Sans W4 (`fj1`).
        "ud デジタル 教科書体 n-r"
        | "ud デジタル 教科書体 nk-r"
        | "ud デジタル 教科書体 np-r"
        | "ud デジタル 教科書体 n-b"
        | "ud デジタル 教科書体 nk-b"
        | "ud デジタル 教科書体 np-b"
        | "ud digi kyokasho n-r"
        | "ud digi kyokasho nk-r"
        | "ud digi kyokasho np-r"
        | "ud digi kyokasho n-b"
        | "ud digi kyokasho nk-b"
        | "ud digi kyokasho np-b" => UD_KYOKASHO,
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
        | "맑은 고딕 semilight"
        | "malgun gothic semilight"
        | "굴림"
        | "gulim"
        | "굴림체"
        | "gulimche"
        | "돋움"
        | "dotum"
        | "돋움체"
        | "dotumche"
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
        | "바탕체"
        | "batangche"
        | "궁서"
        | "gungsuh"
        | "궁서체"
        | "gungsuhche"
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

/// Is `family` a Chinese, Japanese or Korean document font by name (one with CJK substitutes, or
/// any name written in those scripts — Word for Mac draws such a font it lacks in 游ゴシック,
/// `fj1`/`fj2`)? A run's East Asian font that isn't installed is then drawn with its substitute
/// instead of the per-character fallback faces.
pub fn is_cjk_family(family: &str) -> bool {
    cjk_substitutes(&family.to_ascii_lowercase()).is_some() || (!family.is_ascii() && !family.trim().is_empty())
}

/// Ascent and descent (em) of Word's symbol fonts, which aren't open fonts. Their characters are
/// drawn from other faces (`wordcraft_doc::symbol_fonts`), but Word sizes a line by the font
/// itself: a Symbol bullet makes its line taller than the text alone. The values are those Word's
/// PDFs give the fonts (`/Ascent`, `/Descent`).
pub fn symbol_font_metrics(family: &str) -> Option<(f64, f64)> {
    match family.trim().to_ascii_lowercase().as_str() {
        "symbol" | "symbol (pcl6)" => Some((1.005, 0.216)),
        "wingdings" => Some((0.899, 0.205)),
        _ => None,
    }
}

/// The (ascent, descent) in em Word gives a line with a run in a symbol font, when that isn't the
/// resolved face's own: the font isn't installed (`resolved` is a substitute), or the installed
/// face is one em tall, as Apple's Symbol is (hhea 0.701 / -0.299, no gap). Word for Mac spaces
/// such lines like Windows' Symbol: 1.222 em at 11 and 20 pt on synthetic documents (#2681).
pub fn symbol_line(family: &str, resolved: &Resolved) -> Option<(f64, f64)> {
    let m = symbol_font_metrics(family)?;
    (resolved.substituted || one_em(line_metrics(&resolved.face), resolved.face.upem)).then_some(m)
}

/// Is a line's (ascent, descent) one `upem` em tall?
fn one_em((ascent, descent): (f64, f64), upem: f64) -> bool {
    upem > 0.0 && ((ascent + descent) / upem - 1.0).abs() < 0.005
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

/// Thai document fonts: Word's Thai families (Cordia New, Angsana New, Browallia New, the UPC
/// families, Leelawadee), the Thai national fonts (TH Sarabun New and the other "TH …" fonts),
/// the Mac's, the Thai Linux Working Group's and Noto's. Matched by name only.
fn is_thai(f: &str) -> bool {
    const NAMES: &[&str] = &[
        "cordia new",
        "cordiaupc",
        "angsana new",
        "angsanaupc",
        "browallia new",
        "browalliaupc",
        "leelawadee",
        "leelawadee ui",
        "leelawadee ui semilight",
        "th sarabun new",
        "th sarabunpsk",
        "th sarabun psk",
        "th sarabun",
        "thonburi",
        "ayuthaya",
        "sathu",
        "krungthep",
        "silom",
        "sukhumvit set",
        "norasi",
        "kinnari",
        "garuda",
        "laksaman",
        "loma",
        "purisa",
        "sawasdee",
        "umpush",
        "waree",
        "tlwg typo",
        "tlwg typist",
        "tlwg mono",
    ];
    NAMES.contains(&f) || f.ends_with("upc") || f.starts_with("th ") || f.contains("thai")
}

/// Serif Thai document fonts.
fn is_thai_serif(f: &str) -> bool {
    const NAMES: &[&str] = &["angsana new", "angsanaupc", "eucrosiaupc", "jasmineupc", "kodchiangupc", "dilleniaupc", "norasi", "kinnari"];
    NAMES.contains(&f) || (f.contains("thai") && f.contains("serif"))
}

/// Is `family` a Thai document font ([`is_thai`])? A missing one is drawn in a Thai face for the
/// whole run, never in a Latin substitute (#1549).
pub fn is_thai_family(family: &str) -> bool {
    let f = family.to_ascii_lowercase();
    is_thai(&f) || is_thai_serif(&f)
}

/// Khmer document fonts: the Khmer OS family, Khmer Mool1 and the like, Windows' and the Mac's
/// own and Noto's. Matched by name only.
fn is_khmer(f: &str) -> bool {
    f.starts_with("khmer")
        || f.contains("khmer")
        || matches!(f, "daunpenh" | "moolboran" | "limon" | "battambang" | "bokor" | "content" | "dangrek" | "angkor")
}

/// Hebrew document fonts.
fn is_hebrew(f: &str) -> bool {
    const NAMES: &[&str] = &["david", "miriam", "noto sans hebrew", "arial hebrew", "calibri hebrew"];
    NAMES.contains(&f) || f.contains("hebrew")
}

/// The first installed substitute for a missing `family`. For a Thai font, the first with Latin
/// letters and digits as well as Thai ones (a Thai face without Latin would leave a mixed run in
/// two fonts, #1549), else the first with Thai letters, else the first installed.
fn substitute(db: &FontDb, family: &str) -> Option<&'static str> {
    let mut installed = substitutes(family).iter().copied().filter(|s| db.has_family(s));
    if !is_thai_family(family) {
        return installed.next();
    }
    let installed: Vec<&'static str> = installed.collect();
    let covers = |name: &str, chars: &[char]| {
        let face = db.face(name, "Regular");
        face.named(name) && chars.iter().all(|c| face.covers(*c))
    };
    installed
        .iter()
        .copied()
        .find(|s| covers(s, &['\u{0E19}', 'A', 'z', '0', '9']))
        .or_else(|| installed.iter().copied().find(|s| covers(s, &['\u{0E19}'])))
        .or_else(|| installed.first().copied())
}

fn style_name(bold: bool, italic: bool) -> &'static str {
    match (bold, italic) {
        (false, false) => "Regular",
        (true, false) => "Bold",
        (false, true) => "Italic",
        (true, true) => "Bold Italic",
    }
}

type Memo = (u64, HashMap<(String, bool, bool), Resolved>);

fn resolve_cache() -> &'static Mutex<Memo> {
    static CACHE: OnceLock<Mutex<Memo>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new((0, HashMap::new())))
}

const FAMILY_CACHE_MAX: usize = 4096;
/// Family names longer than this (bytes) are resolved but never remembered.
const FAMILY_KEY_MAX: usize = 256;

/// Resolve `family` with bold/italic to a face (memoised until user fonts are added or removed,
/// [`FontDb::generation`]).
pub fn resolve(family: &str, bold: bool, italic: bool) -> Resolved {
    let cache = resolve_cache();
    let db = FontDb::global();
    let generation = db.generation();
    let cacheable = family.len() <= FAMILY_KEY_MAX;
    let key = (if cacheable { family.to_string() } else { String::new() }, bold, italic);
    {
        let mut c = cache.lock().unwrap_or_else(|e| e.into_inner());
        if c.0 != generation {
            *c = (generation, HashMap::new());
        }
        if cacheable && let Some(r) = c.1.get(&key) {
            return *r;
        }
    }
    // Printer-driver Symbol aliases use the same encoding as Symbol.
    let base = if family.trim().eq_ignore_ascii_case("Symbol (PCL6)") { "Symbol" } else { charset_base_family(family) };
    let (fam, substituted) = if db.has_family(family) {
        (family.to_string(), false)
    } else if base != family && db.has_family(base) {
        // Windows maps the charset names to the font itself: not a substitution.
        (base.to_string(), false)
    } else {
        (substitute(db, base).unwrap_or(crate::FALLBACK_FAMILY).to_string(), true)
    };
    // Word draws bold for its own fonts without a bold style by emboldening the regular face
    // (`word_lacks_bold`): the regular face here too, whatever bold the stand-in has.
    let real_bold = bold && !word_lacks_bold(family);
    let face = db.face(&fam, style_name(real_bold, italic));
    let r = Resolved {
        synth_bold: bold && (!real_bold || face.weight < 600.0),
        synth_italic: italic && !face.italic,
        substituted,
        face: FaceRef::of(&face),
    };
    let mut c = cache.lock().unwrap_or_else(|e| e.into_inner());
    if cacheable && c.0 == generation {
        if c.1.len() >= FAMILY_CACHE_MAX {
            c.1.clear();
        }
        c.1.insert(key, r);
    }
    r
}

/// The face for Hangul a run's fonts lack, as Word for Mac picks it (2026-10-19): Batang when
/// the run's East Asian font (`w:eastAsia`, else its font) is an oldstyle serif (OS/2 family
/// class 1: Times New Roman, Hiragino Mincho ProN) or one of Word's serif CJK families (游明朝,
/// [`is_cjk_serif_family`]), Malgun Gothic otherwise (Arial, Aptos, Georgia, Cambria, Courier
/// New, Baskerville, Palatino, Hiragino Sans, 游ゴシック, and the fixed-pitch ＭＳ 明朝 and ＭＳ
/// ゴシック, where ＭＳ Ｐ明朝 takes Batang: `kr4`, `kr5`). Both come with Office and are laid out
/// by name with Word's own metrics whatever Korean face stands in for them here (Apple SD Gothic
/// Neo and AppleMyungjo on a Mac): an em per syllable, Batang's 1.3-em and Malgun Gothic's
/// 1.73-em lines — which count toward the line they fall in: Arial text with a Hangul word runs
/// 41.52-pt lines at 24 pt (`ha1`) — and their 縦書き pitch and origin ([`office_ea_box`],
/// [`named_ea_metrics`]). The face and Word's font name; `None` when no Korean face is
/// installed.
pub fn hangul_fallback(serif: bool, bold: bool, italic: bool) -> Option<(FaceRef, &'static str)> {
    let name = if serif { "Batang" } else { "Malgun Gothic" };
    let r = resolve(name, bold, italic);
    r.face.covers('한').then_some((r.face, name))
}

#[allow(dead_code)]
fn hangul_fallback_lists() {
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
    let _ = (SERIF, SANS);
}

/// The face for the Chinese and Japanese characters a run's fonts lack: Word for Mac draws them
/// in ＭＳ 明朝 whatever the run's language (Times New Roman, Arial and Georgia text tagged ja-JP,
/// zh-CN, zh-TW, ko-KR and en-US alike, 2026-10-19) — in ＭＳ ゴシック when the run's East Asian
/// font is one of Word's sans CJK families (맑은 고딕 and 나눔고딕 text's half-width katakana,
/// `fj2`; 바탕, SimSun and the Latin fonts take ＭＳ 明朝) — here its substitute, when it covers
/// them all.
pub fn han_fallback(chars: &[char], gothic: bool, bold: bool, italic: bool) -> Option<FaceRef> {
    let r = resolve(if gothic { "MS Gothic" } else { "MS Mincho" }, bold, italic);
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
/// of the Yu fonts, whose line is 1.43 em with the baseline 1.045 em below the line's top and
/// 0.385 em above its bottom — measured in Word for Mac 16.114 on 2026-10-18 at 10.5 and 20 pt
/// with single spacing (pitches 15.0 and 28.6 pt; the first baseline 11.04 pt below the top
/// margin; 12.48 pt below the top of an 18-pt grid line; a 20-pt line 25.0 pt below a 10.5-pt
/// one), the same for every weight, and on 2026-10-19 at 24, 48 and 96 pt (`kr4`, `kr6`: the
/// first baseline 24.96, 50.16 and 100.32 pt below the top margin — 1.045 em, which 1.051 would
/// miss by 0.3 pt at 48 and 0.6 at 96). The macOS system 游明朝体/游ゴシック体 that stand in for them
/// have other metrics (hhea 0.88/0.12 with a 0.5 em gap: 1.5 em), which would put every
/// Japanese line 0.07 em further apart than Word. `None` for other families: their faces' own
/// metrics apply ([`line_metrics`]).
pub fn office_line_metrics(family: &str, face: &crate::FontFace) -> Option<(f64, f64)> {
    if is_office_yu(family) {
        return Some((YU_ASCENT * face.upem, YU_DESCENT * face.upem));
    }
    // Word's own East Asian table fonts by name (`office_ea_box`), their box centred in Word for
    // Mac's 1.3-times line: MS Gothic, Gulim, Dotum, Batang and Gungsuh (0.859/0.141, a 1-em
    // box) put the baseline 1.009 em below the line's top, MS Mincho (0.857/0.1406) 1.0067 with
    // a 1.2969-em line (124.56 pt at 96, 62.16 at 48: `lm1`–`lm3`, `kr6` probes), Malgun Gothic
    // (1.0879/0.2429) 1.2875 em with a 1.73-em line (41.52 pt at 24, 123.60 and 166.08 at 96:
    // `kr1`, `kr6`), whatever face stands in.
    let (a, d) = office_ea_box(family)?;
    let line = EAST_ASIAN_LINE * (a + d);
    let asc = a + (line - (a + d)) / 2.0;
    Some((asc * face.upem, (line - asc) * face.upem))
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
        return (YU_ASCENT - YU_DESCENT) / 2.0 * face.upem;
    }
    // Malgun Gothic: half its win ascent − descent (2146 − 526 at 2048 units), 0.40 em on the
    // ascent side (`kr3`: the em squares' centre 21.36 pt from the column's top edge at 24 pt).
    if is_office_malgun(family) {
        return MALGUN_WIN_CENTRE * face.upem;
    }
    if let Some((a, d)) = office_ea_box(family) {
        return (a - d) / 2.0 * face.upem;
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
/// Word's own Yu Mincho/Yu Gothic builds: the baseline below the line's top and above its
/// bottom, in em (a 1.43-em line).
const YU_ASCENT: f64 = 1.045;
const YU_DESCENT: f64 = 0.385;
/// MS Mincho's hhea ascent and descent in em: a 0.9976-em box (1755/288 at 2048 units), whose
/// 1.3-times line runs 124.56 pt at 96 pt and 62.16 at 48 in Word for Mac (`kr6`, `lm3`).
const MS_MINCHO_BOX: (f64, f64) = (0.857, 0.1406);
/// MS Gothic's, and Gulim's, Dotum's, Batang's and Gungsuh's: a 1-em box (1760/288).
const MS_GOTHIC_BOX: (f64, f64) = (0.859, 0.141);
/// Malgun Gothic's (Semilight too): a 1.3308-em box, so a 1.73-em line with the baseline 1.2875
/// em below its top (Word for Mac: 41.52 pt at 24, 83.04 at 48, 166.08 at 96 with the first
/// baseline 30.96, 62.16 and 123.60 below the top margin; `kr1`, `kr6`, 2026-10-19).
const MALGUN_BOX: (f64, f64) = (1.0879, 0.2429);
/// Half Malgun Gothic's win ascent − descent, in em: where its 縦書き em squares are centred.
const MALGUN_WIN_CENTRE: f64 = 0.3955;
/// The top of MS Mincho's, MS Gothic's and the Korean table fonts' strikethrough above the
/// baseline, in em (Gulim, Dotum, Batang, Gungsuh: 6.24 pt at 24, `kr2`).
const MS_STRIKE: f32 = 0.258;

/// The hhea-style box (ascent, descent in em) Word for Mac lays one of its own East Asian table
/// fonts out with, by name, whatever face stands in here: MS Mincho, MS Gothic and the Korean
/// fonts Office bundles (맑은 고딕, 굴림, 돋움, 바탕, 궁서 and their fixed-pitch 체 siblings).
pub fn office_ea_box(family: &str) -> Option<(f64, f64)> {
    if is_office_malgun(family) {
        Some(MALGUN_BOX)
    } else if is_office_ms_mincho(family) {
        Some(MS_MINCHO_BOX)
    } else if is_office_ms_gothic(family) || is_office_gulim(family) {
        Some(MS_GOTHIC_BOX)
    } else {
        None
    }
}

fn named_in(family: &str, names: &[&str]) -> bool {
    let base = fold_cjk_name(family);
    names.iter().any(|n| fold_cjk_name(n) == base)
}

/// Is `family` Malgun Gothic (맑은 고딕) or Malgun Gothic Semilight, Korean fonts Office bundles?
pub fn is_office_malgun(family: &str) -> bool {
    named_in(family, &["Malgun Gothic", "Malgun Gothic Semilight", "맑은 고딕", "맑은 고딕 Semilight"])
}

/// Is `family` one of the Windows Korean table fonts Office bundles — Gulim, Dotum, Batang,
/// Gungsuh (굴림, 돋움, 바탕, 궁서) or their fixed-pitch 체 (Che) siblings?
pub fn is_office_gulim(family: &str) -> bool {
    named_in(
        family,
        &[
            "Gulim",
            "GulimChe",
            "Dotum",
            "DotumChe",
            "Batang",
            "BatangChe",
            "Gungsuh",
            "GungsuhChe",
            "굴림",
            "굴림체",
            "돋움",
            "돋움체",
            "바탕",
            "바탕체",
            "궁서",
            "궁서체",
        ],
    )
}

/// Is `family` a fixed-pitch Office East Asian font — MS Mincho, MS Gothic or a Korean 체 font —
/// whose Latin letters are half an em (`kr4`: A, W, i, a space all 0.50 em at 24 pt in Word for
/// Mac, where MS PMincho, MS PGothic, Gulim and Batang are proportional)? Word for Mac draws the
/// Hangul these lack in Malgun Gothic, MS PMincho's in Batang.
pub fn is_half_width_family(family: &str) -> bool {
    named_in(
        family,
        &[
            "MS Mincho",
            "MS Gothic",
            "ＭＳ 明朝",
            "ＭＳ ゴシック",
            "GulimChe",
            "DotumChe",
            "BatangChe",
            "GungsuhChe",
            "굴림체",
            "돋움체",
            "바탕체",
            "궁서체",
        ],
    )
}

/// Is `c` half an em wide in Word's fixed-pitch East Asian fonts ([`is_half_width_family`])?
/// Measured in Word for Mac (`hw1`, 2026-10-19, 24 pt): in MS Mincho, MS Gothic and GulimChe every
/// ASCII character (letters, digits, punctuation, the space), the Latin-1 letters and symbols
/// (é ñ ü © ¥ £) and the en and em dashes advance 0.50 em, as do the half-width katakana;
/// × and ÷, Greek, Cyrillic, … and the curly quotes a whole em. MS PMincho, MS PGothic, MS UI
/// Gothic, Gulim and Batang are proportional (A 0.664, 0.633 and 0.736 em) and take their
/// faces' widths here.
pub fn is_half_width_char(c: char) -> bool {
    matches!(c, '\u{20}'..='\u{7E}' | '\u{A0}'..='\u{FF}' | '\u{2013}' | '\u{2014}' | '\u{FF61}'..='\u{FF9F}') && !matches!(c, '×' | '÷')
}

/// The face that draws the half-width Latin of a fixed-pitch Office East Asian font
/// ([`is_half_width_family`]) when the face standing in for it (游明朝 or Hiragino for ＭＳ 明朝
/// on a Mac) has proportional Latin: Osaka−等幅, Apple's monospaced Japanese font, whose ASCII,
/// © and ¥ are 0.50 em and centred in their cells like MS Gothic's. `None` when it isn't
/// installed or isn't half-width (the characters are then centred in 0.5-em cells in the
/// stand-in face), or when the stand-in's own A is already half an em (the real MS Gothic).
pub fn half_width_latin_face(primary: &FaceRef, bold: bool, italic: bool) -> Option<FaceRef> {
    let half = |f: &FaceRef| {
        let gid = f.glyph_for('A');
        gid != 0 && (f.advance(gid) / f.upem.max(1.0) - 0.5).abs() < 1e-3
    };
    if half(primary) {
        return None;
    }
    let r = resolve("Osaka Regular-Mono", bold, italic);
    (!r.substituted && half(&r.face)).then_some(r.face)
}

fn is_office_ms_mincho(family: &str) -> bool {
    named_in(family, &["MS Mincho", "MS PMincho", "ＭＳ 明朝", "ＭＳ Ｐ明朝"])
}

fn is_office_ms_gothic(family: &str) -> bool {
    named_in(family, &["MS Gothic", "MS PGothic", "MS UI Gothic", "ＭＳ ゴシック", "ＭＳ Ｐゴシック"])
}

/// How Word for Mac advances and hangs the East Asian characters of one of its own table fonts,
/// by name, whatever face stands in here ([`named_ea_metrics`]), in em.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct NamedEa {
    /// Across a line: every East Asian character's advance (an em), `None` to keep the face's.
    pub advance: Option<f64>,
    /// Along a 縦書き column: the pitch of an upright character.
    pub v_advance: f64,
    /// Along a 縦書き column: an upright character's baseline below its pen.
    pub v_origin: f64,
}

/// Word for Mac's placement of East Asian characters in its own table fonts (measured
/// 2026-10-19 at 24 pt): MS Mincho, MS Gothic, Gulim, Dotum, Batang, Gungsuh and the 체 fonts
/// advance an em across a line and along a column, hanging each glyph its hhea ascent (0.857 or
/// 0.859 em) below the pen (`vt3`, `kr3`, `kr5`); Malgun Gothic's Hangul an em across a line but
/// 1.0625 em along a column (25.5 pt), hung 0.917 em below the pen (`kr3`); Malgun Gothic
/// Semilight an em along the column, hung 0.8975 em (`kr5`; its glyphs vary by a point). The
/// proportional MS PMincho and MS PGothic keep the face's advances across a line (their kana
/// are proportional). `None` for other families.
pub fn named_ea_metrics(family: &str) -> Option<NamedEa> {
    if is_office_malgun(family) {
        let semilight = family.to_ascii_lowercase().contains("semilight");
        return Some(NamedEa {
            advance: Some(1.0),
            v_advance: if semilight { 1.0 } else { 1.0625 },
            v_origin: if semilight { 0.8975 } else { 0.917 },
        });
    }
    let (a, _) = office_ea_box(family)?;
    let proportional = is_office_ms(family) && !is_half_width_family(family);
    Some(NamedEa { advance: (!proportional).then_some(1.0), v_advance: 1.0, v_origin: a })
}

/// Is `family` one of the MS Mincho / MS Gothic families Word bundles (by its English or
/// Japanese name)?
/// Word's own fonts that have no bold style: Word for Mac draws their bold by emboldening the
/// regular glyphs (its PDFs show YuMincho-Regular, MS-Mincho, MS-Gothic and YuGothic-Light for
/// bold runs of 游明朝, ＭＳ 明朝, ＭＳ ゴシック, ＭＳ Ｐ明朝, ＭＳ Ｐゴシック and 游ゴシック Light,
/// 2026-10-20), where 游ゴシック, 游明朝 Demibold, メイリオ and the Hiragino families have a bold
/// face of their own. A face standing in for one of these is emboldened the same way.
pub fn word_lacks_bold(family: &str) -> bool {
    const NAMES: &[&str] = &["游明朝", "Yu Mincho", "YuMincho", "游明朝 Light", "Yu Mincho Light", "游ゴシック Light", "Yu Gothic Light"];
    if is_office_ms(family) {
        return true;
    }
    let base = fold_cjk_name(family);
    NAMES.iter().any(|n| fold_cjk_name(n) == base)
}

/// The points Word for Mac adds to each glyph's advance across a line when it emboldens a face
/// itself: a fiftieth of an em in the font's units, rounded — 0.02 em for Yu Mincho, Hiragino
/// Maru Gothic and Yu Gothic Light (0.48 pt at 24 pt), 5/256 em for the MS fonts and Osaka (0.468
/// pt), on kanji, kana and Latin letters alike, nothing on spaces (its PDFs, 2026-10-20). The
/// MS fonts' 256-unit em counts whatever face stands in for them.
pub fn synthetic_bold_extra(family: &str, upem: f32, size: f32) -> f32 {
    let u = if is_office_ms(family) { 256.0 } else { upem };
    if !(u.is_finite() && u >= 1.0 && size.is_finite()) {
        return 0.0;
    }
    (u / 50.0).round() / u * size.clamp(0.0, 1584.0)
}

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
    // Word's own table fonts by name take the same rule on their descents — MS Mincho 1.68 pt at
    // 12, Gulim, Dotum, Batang and Gungsuh 2.88 at 24, Malgun Gothic 4.56 at 24 (`kr2`) — with
    // a 0.05-em line.
    let office = office_ea_box(family);
    let descent = match office {
        Some((_, d)) => d,
        None => hhea_descent.unwrap_or(face.descent.abs() / face.upem.max(1.0)),
    };
    let underline_top = if office.is_some() || face.is_east_asian() { (0.7 * descent * size as f64) as f32 + 0.53 } else { (-uo * k) as f32 };
    let underline_thickness = if office.is_some() { 0.05 * size } else { (ut * k) as f32 };
    // MS Mincho and MS Gothic strike through 0.258 em above the baseline (the top of a 0.05-em
    // line: 3.08 pt at 12, 12.50 at 48, where Times New Roman's and Arial's tables put theirs),
    // whatever stands in for them; Hiragino (0.322), Toppan (0.276), Times (0.259) and Arial are
    // their OS/2 tables (`st1`, `st2` probes).
    // Malgun Gothic strikes through 0.31 em above the baseline (7.44 pt at 24), its Semilight
    // 0.23 em with a 0.06-em line (`kr2`); the Korean table fonts as MS Mincho.
    let (strike_top, strike_thickness) = if is_office_malgun(family) {
        if family.to_ascii_lowercase().contains("semilight") { (0.23 * size, 0.06 * size) } else { (0.31 * size, 0.05 * size) }
    } else if office.is_some() {
        (MS_STRIKE * size, 0.05 * size)
    } else {
        ((so * k) as f32, (st * k) as f32)
    };
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
            assert!((a / em - 1.045).abs() < 1e-6 && (d / em - 0.385).abs() < 1e-6, "{name}: {a} {d}");
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
    /// Word for Mac (`hw1`, 2026-10-19): MS Mincho, MS Gothic and the 체 fonts set ASCII, Latin-1
    /// and the dashes half an em wide, × ÷ and Greek a whole one; Osaka−等幅 stands in for their
    /// Latin where installed, never for a face whose A is half an em already.
    #[test]
    fn half_width_characters_of_words_fixed_pitch_fonts() {
        for c in ['A', 'a', 'W', 'i', '1', ' ', '.', '@', '~', 'é', 'ñ', '©', '¥', '£', '–', '—', 'ｱ', 'ﾞ'] {
            assert!(is_half_width_char(c), "{c}");
        }
        for c in ['×', '÷', 'α', 'Д', '…', '“', '’', '‐', '〜', '日', 'ア', '　', 'Ａ'] {
            assert!(!is_half_width_char(c), "{c}");
        }
        let yu = resolve("Yu Mincho", false, false);
        if yu.substituted {
            return;
        }
        if let Some(f) = half_width_latin_face(&yu.face, false, false) {
            assert_eq!(f.get().family, "Osaka");
            for c in ['A', 'W', 'i', ' ', '©', '¥'] {
                assert!((f.advance(f.glyph_for(c)) / f.upem - 0.5).abs() < 1e-3, "{c}");
            }
            // A face with half-width Latin of its own needs no stand-in.
            assert!(half_width_latin_face(&f, false, false).is_none());
        }
    }

    #[test]
    fn hangul_fallback_is_batang_for_oldstyle_serifs_and_malgun_gothic_otherwise() {
        if let (Some((s, sn)), Some((g, gn))) = (hangul_fallback(true, false, false), hangul_fallback(false, false, false)) {
            assert!(s.covers('한') && g.covers('한'));
            assert_eq!((sn, gn), ("Batang", "Malgun Gothic"));
            assert_eq!(s.id(), resolve("Batang", false, false).face.id());
            assert_eq!(g.id(), resolve("Malgun Gothic", false, false).face.id());
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
        if let Some(f) = han_fallback(&['日', '本'], false, false, false) {
            assert!(f.is_east_asian() && f.covers('語'));
        }
    }

    #[test]
    fn words_korean_fonts_take_words_metrics_by_name() {
        let face = resolve("Batang", false, false).face;
        let f = face.get();
        let em = f.upem;
        let near = |a: f64, b: f64| (a - b).abs() < 1e-4;
        for name in ["Malgun Gothic", "맑은 고딕", "Malgun Gothic Semilight"] {
            let (a, d) = office_line_metrics(name, f).unwrap();
            assert!(near(a / em, 1.2875) && near((a + d) / em, 1.73), "{name}: {a} {d}");
            assert!(near(vertical_em_centre(name, f) / em, 0.3955), "{name}");
        }
        for name in ["Gulim", "GulimChe", "Dotum", "돋움체", "Batang", "바탕", "Gungsuh", "궁서체", "MS Gothic", "MS PGothic"] {
            let (a, d) = office_line_metrics(name, f).unwrap();
            assert!(near(a / em, 1.009) && near((a + d) / em, 1.3), "{name}: {a} {d}");
            assert!(near(vertical_em_centre(name, f) / em, 0.359), "{name}");
        }
        for name in ["MS Mincho", "ＭＳ Ｐ明朝"] {
            let (a, d) = office_line_metrics(name, f).unwrap();
            assert!(near(a / em, 0.857 + 0.15 * 0.9976) && near((a + d) / em, 1.3 * 0.9976), "{name}: {a} {d}");
        }
        assert!(office_line_metrics("Nanum Gothic", f).is_none() && office_line_metrics("Apple SD Gothic Neo", f).is_none());
        // Decorations at 24 pt (`kr2`).
        let d = decoration_metrics("Malgun Gothic", f, 24.0);
        assert!((d.underline_top - 4.61).abs() < 0.05 && (d.underline_thickness - 1.2).abs() < 1e-3 && (d.strike_top - 7.44).abs() < 1e-3, "{d:?}");
        let d = decoration_metrics("Malgun Gothic Semilight", f, 24.0);
        assert!((d.strike_top - 5.52).abs() < 1e-3 && (d.strike_thickness - 1.44).abs() < 1e-3, "{d:?}");
        let d = decoration_metrics("Batang", f, 24.0);
        assert!((d.underline_top - 2.90).abs() < 0.02 && (d.strike_top - 6.19).abs() < 0.01, "{d:?}");
        // Advances and vertical placement by name.
        assert_eq!(named_ea_metrics("Malgun Gothic"), Some(NamedEa { advance: Some(1.0), v_advance: 1.0625, v_origin: 0.917 }));
        assert_eq!(named_ea_metrics("맑은 고딕 Semilight"), Some(NamedEa { advance: Some(1.0), v_advance: 1.0, v_origin: 0.8975 }));
        assert_eq!(named_ea_metrics("BatangChe"), Some(NamedEa { advance: Some(1.0), v_advance: 1.0, v_origin: 0.859 }));
        assert_eq!(named_ea_metrics("ＭＳ 明朝"), Some(NamedEa { advance: Some(1.0), v_advance: 1.0, v_origin: 0.857 }));
        assert_eq!(named_ea_metrics("MS PGothic"), Some(NamedEa { advance: None, v_advance: 1.0, v_origin: 0.859 }));
        assert_eq!(named_ea_metrics("Arial"), None);
        assert!(
            is_half_width_family("MS Gothic")
                && is_half_width_family("굴림체")
                && !is_half_width_family("MS PMincho")
                && !is_half_width_family("Gulim")
        );
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
        // MS Gothic by either name, whatever face stands in: 1.009 em above the baseline,
        // 0.291 below; MS Mincho's 0.9976-em box 1.0066 and 0.2903; the underline 1.68 below at
        // 12 pt (0.72 thick), 5.28 at 48 (2.40).
        for name in ["MS Gothic", "MS PGothic", "ＭＳ Ｐゴシック", "MS UI Gothic"] {
            assert!(is_office_ms(name), "{name}");
            let (a, d) = office_line_metrics(name, f).unwrap_or_else(|| panic!("{name}"));
            assert!((a / f.upem - 1.009).abs() < 1e-6 && (d / f.upem - 0.291).abs() < 1e-6, "{name}: {a} {d}");
        }
        for name in ["MS Mincho", "ＭＳ 明朝", "MS PMincho"] {
            assert!(is_office_ms(name), "{name}");
            let (a, d) = office_line_metrics(name, f).unwrap_or_else(|| panic!("{name}"));
            assert!((a / f.upem - 1.00664).abs() < 1e-4 && ((a + d) / f.upem - 1.29688).abs() < 1e-4, "{name}: {a} {d}");
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
        assert!((vertical_em_centre("游明朝", f) / f.upem - 0.33).abs() < 1e-6);
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

    /// Word for Mac (`fj1`–`fj3`, 2026-10-19): a font it lacks is drawn in 游ゴシック when its name
    /// is Chinese, Japanese or Korean, in Cambria (East Asian characters in ＭＳ 明朝) when its
    /// name is Latin, in Arial when it is a Helvetica; UD デジタル 教科書体 in Hiragino Sans.
    #[test]
    fn fonts_word_for_mac_lacks_substitute_as_it_draws_them() {
        for name in [
            "小塚明朝 Pr6N R",
            "HGP創英角ﾎﾟｯﾌﾟ体",
            "方正书宋简体",
            "함초롬바탕",
            "游ゴシック体",
            "ヒラギノ丸ゴ ProN",
            "Osaka−等幅",
            "不存在フォント",
            "DFP平成明朝体W3",
        ] {
            assert_eq!(substitutes(name), GOTHIC, "{name}");
            assert!(is_cjk_family(name) && !is_cjk_serif_family(name), "{name}");
        }
        for name in ["Frutiger", "Klee", "Kozuka Mincho Pr6N", "Definitely Not A Font 123", "Foo Mono", "Brand Sans", "Open Sans", "Roboto"] {
            assert_eq!(substitutes(name).first(), Some(&"Caladea"), "{name}: Cambria's metric twin first");
            assert!(!is_cjk_family(name), "{name}");
        }
        assert_eq!(substitutes("Helvetica Neue LT Std").first(), Some(&"Liberation Sans"));
        // Office's own sans fonts keep a sans stand-in (Word has them).
        for name in ["Calibri", "Aptos", "Segoe UI", "Verdana", "Tahoma", "Century Gothic", "Segoe Print"] {
            assert_eq!(substitutes(name).first(), Some(&"Carlito"), "{name}");
        }
        assert_eq!(substitutes("Helvetica").first(), Some(&"Liberation Sans"));
        for name in ["UD デジタル 教科書体 N-R", "UD デジタル 教科書体 NP-B", "UD Digi Kyokasho NK-R"] {
            assert_eq!(substitutes(name).first(), Some(&"Hiragino Sans"), "{name}");
            assert!(is_cjk_family(name) && !is_cjk_serif_family(name), "{name}");
        }
        assert!(!is_cjk_family("") && !is_cjk_family("  "));
        // Office's own fonts keep their substitutes by style.
        assert!(is_cjk_serif_family("ＭＳ Ｐ明朝") && is_cjk_serif_family("游明朝") && !is_cjk_serif_family("メイリオ"));
    }

    #[test]
    fn family_cache_stays_bounded() {
        for i in 0..(FAMILY_CACHE_MAX + 50) {
            let _ = resolve(&format!("Zz Missing Family {i}"), false, false);
        }
        assert!(resolve_cache().lock().unwrap_or_else(|e| e.into_inner()).1.len() <= FAMILY_CACHE_MAX);
        let long = "L".repeat(10_000);
        let _ = resolve(&long, false, false);
        assert!(!resolve_cache().lock().unwrap_or_else(|e| e.into_inner()).1.keys().any(|k| k.0.len() > 1000));
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

    /// Word for Mac emboldens its own fonts that have no bold style (游明朝, the MS fonts, 游ゴシック
    /// Light) and widens each glyph by a fiftieth of an em in the font's units (its PDFs, 2026-10-20).
    #[test]
    fn words_fonts_without_a_bold_are_emboldened_and_widened() {
        for name in ["游明朝", "Yu Mincho", "ＭＳ 明朝", "MS Mincho", "ＭＳ ゴシック", "ＭＳ Ｐ明朝", "MS PGothic", "游ゴシック Light"]
        {
            assert!(word_lacks_bold(name), "{name}");
            let b = resolve(name, true, false);
            assert!(b.synth_bold, "{name}: bold drawn by emboldening");
            assert_eq!(b.face.id(), resolve(name, false, false).face.id(), "{name}: from the regular face");
        }
        for name in ["游ゴシック", "Yu Gothic", "游明朝 Demibold", "メイリオ", "Hiragino Mincho ProN", "Aptos", "Times New Roman"] {
            assert!(!word_lacks_bold(name), "{name}");
        }
        // 0.02 em for a 1000-unit em; 5/256 em for the MS fonts, whatever face stands in.
        assert!((synthetic_bold_extra("游明朝", 1000.0, 24.0) - 0.48).abs() < 1e-4);
        assert!((synthetic_bold_extra("游明朝", 1000.0, 10.5) - 0.21).abs() < 1e-4);
        assert!((synthetic_bold_extra("ＭＳ 明朝", 1000.0, 24.0) - 0.46875).abs() < 1e-4);
        assert!((synthetic_bold_extra("Osaka", 256.0, 24.0) - 0.46875).abs() < 1e-4);
        assert!((synthetic_bold_extra("Hiragino Maru Gothic ProN", 1000.0, 24.0) - 0.48).abs() < 1e-4);
        assert_eq!(synthetic_bold_extra("游明朝", f32::NAN, 24.0), 0.0);
        assert_eq!(synthetic_bold_extra("游明朝", 1000.0, f32::INFINITY), 0.0);
    }

    #[test]
    fn pcl6_symbol_has_symbols_line_metrics() {
        assert_eq!(symbol_font_metrics("Symbol (PCL6)"), symbol_font_metrics("Symbol"));
        assert_eq!(symbol_font_metrics("Arial"), None);
    }

    #[test]
    fn a_one_em_symbol_face_takes_windows_symbols_line() {
        // Apple's Symbol (2048 units): hhea 1436 / -612, no gap, one em; Windows' Symbol line is 1.221 em.
        assert!(one_em(hhea_line(1436, -612, 0), 2048.0));
        assert!(!one_em((1005.0, 216.0), 1000.0));
        assert!(!one_em((899.0, 211.0), 1000.0));
        assert!(!one_em((0.0, 0.0), 0.0));
        let arial = resolve("Arial", false, false);
        assert_eq!(symbol_line("Arial", &arial), None);
        let symbol = resolve("Symbol", false, false);
        if symbol.substituted {
            assert_eq!(symbol_line("Symbol", &symbol), symbol_font_metrics("Symbol"));
        }
    }

    #[test]
    fn pcl6_symbol_resolves_as_symbol() {
        let actual = resolve("Symbol (PCL6)", false, false);
        let db = FontDb::global();
        if db.has_family("Symbol (PCL6)") {
            assert_eq!(actual.face.id(), db.face("Symbol (PCL6)", "Regular").id());
            assert!(!actual.substituted);
            return;
        }
        let expected = resolve("Symbol", false, false);
        assert_eq!(actual.face.id(), expected.face.id());
        assert_eq!(actual.substituted, expected.substituted);
    }

    #[test]
    fn unicode_arial_is_proportional() {
        for name in ["Arial Unicode MS", "ARIAL UNICODE MS"] {
            let candidates = substitutes(name);
            assert_eq!(candidates.first(), Some(&"Arial"));
            assert_eq!(&candidates[1..], substitutes("Arial"));
        }
        assert_eq!(substitutes("Cascadia Code").first(), Some(&"Courier New"));
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

    #[test]
    fn khmer_fonts_substitute_khmer_faces() {
        for f in ["Khmer OS Bokor", "Khmer OS Battambang", "Khmer Mool1", "Khmer OS Muol Light"] {
            assert!(substitutes(f).iter().any(|s| s.contains("Khmer")), "{f}: {:?}", substitutes(f));
        }
    }

    #[test]
    fn thai_fonts_substitute_thai_faces() {
        // Every Thai document family maps to a list of Thai faces, never to the Latin lists.
        for f in [
            "TH Sarabun New",
            "TH SarabunPSK",
            "Angsana New",
            "AngsanaUPC",
            "Cordia New",
            "CordiaUPC",
            "Browallia New",
            "BrowalliaUPC",
            "JasmineUPC",
            "KodchiangUPC",
            "LilyUPC",
            "DilleniaUPC",
            "EucrosiaUPC",
            "FreesiaUPC",
            "IrisUPC",
            "Leelawadee",
            "Leelawadee UI",
            "Noto Sans Thai",
            "TH Niramit AS",
        ] {
            assert!(is_thai_family(f), "{f}");
            let list = substitutes(f);
            assert!(list.iter().any(|s| s.contains("Thai")), "{f}: {list:?}");
            assert!(!list.contains(&"Carlito") && !list.contains(&"Caladea"), "{f}: {list:?}");
        }
        // Serif Thai families prefer a serif Thai face; the sans families a sans one.
        let pos = |family: &str, name: &str| substitutes(family).iter().position(|s| *s == name).unwrap_or(usize::MAX);
        assert!(pos("Angsana New", "Noto Serif Thai") < pos("Angsana New", "Noto Sans Thai"));
        assert!(pos("TH Sarabun New", "Noto Sans Thai") < pos("TH Sarabun New", "Noto Serif Thai"));
        // Latin and other script families are unchanged.
        assert!(!is_thai_family("Arial") && !is_thai_family("Tahoma") && !is_thai_family("Calibri") && !is_thai_family("David"));
        assert_eq!(substitutes("Calibri").first(), Some(&"Carlito"));
    }

    #[test]
    fn a_missing_thai_font_resolves_to_an_installed_thai_face() {
        // The issue's run (#1549): Thai letters and digits, Latin letters, digits and the baht
        // sign in one run of a Thai font that isn't installed.
        const RUN: &str = "[D] น้ำ ค่ำ ปั้น ฝั่ง ปู่ ๑๒๓๔๕๖๗๘๙๐ ฿1,250.50";
        let db = FontDb::global();
        for family in ["TH Sarabun New", "Angsana New", "Cordia New", "BrowalliaUPC"] {
            if db.has_family(family) {
                continue;
            }
            let r = resolve(family, false, false);
            let face = r.face.get();
            if !face.covers('\u{0E19}') {
                eprintln!("skipped {family}: no Thai face from its substitute list is installed");
                continue;
            }
            assert!(r.substituted, "{family}");
            let thai: Vec<char> = RUN.chars().filter(|c| ('\u{0E00}'..='\u{0E7F}').contains(c) && !face.covers(*c)).collect();
            assert!(thai.is_empty(), "{family} → {} lacks {thai:?}", face.family);
            // A listed Thai face with Latin letters and digits wins over one without them.
            let latin = |name: &str| {
                let f = db.face(name, "Regular");
                f.named(name) && f.covers('\u{0E19}') && f.covers('D') && f.covers('5')
            };
            if substitutes(family).iter().any(|s| db.has_family(s) && latin(s)) {
                let missing: Vec<char> = RUN.chars().filter(|c| !c.is_whitespace() && !face.covers(*c)).collect();
                assert!(missing.is_empty(), "{family} → {} lacks {missing:?}", face.family);
            }
        }
    }

    #[test]
    fn thonburi_stacks_thai_marks_through_its_apple_tables() {
        // Thonburi has `morx` but no GSUB: the shaper must still move a tone mark up over an upper
        // vowel (ปั่) instead of drawing it where it sits on a bare consonant (ป่).
        let db = FontDb::global();
        if !db.has_family("Thonburi") {
            eprintln!("skipped: Thonburi not installed");
            return;
        }
        let face = db.face("Thonburi", "Regular");
        let tone = |text: &str| crate::shape(&face, text, &[], |c| c).into_iter().last().map(|g| (g.gid, g.y_offset));
        let (bare, stacked) = (tone("\u{0E1B}\u{0E48}"), tone("\u{0E1B}\u{0E31}\u{0E48}"));
        assert!(bare.is_some() && stacked.is_some());
        assert_ne!(bare, stacked, "the tone mark over ั is raised or swapped");
    }

    #[test]
    fn fonts_with_a_clone_have_named_substitutes() {
        for f in ["Times New Roman", "Times New Roman Cyr", "Arial", "Courier New", "Palatino Linotype", "Symbol"] {
            assert!(has_named_substitutes(f), "{f}");
        }
        for f in ["No Such Brand Font", "Cambria", "Georgia", "Brand Serif"] {
            assert!(!has_named_substitutes(f), "{f}");
        }
    }
}

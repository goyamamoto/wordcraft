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

/// The substitute chain for a folded CJK font name.
fn cjk_chain(base: &str) -> Option<&'static [&'static str]> {
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
    const SANS_TC: &[&str] = &[
        "Microsoft JhengHei",
        "PingFang TC",
        "Heiti TC",
        "PingFang HK",
        "Noto Sans CJK TC",
        "Noto Sans TC",
        "Source Han Sans TC",
        "Source Han Sans TW",
    ];
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

/// A line's (ascent, descent) from hhea values, both positive: the line gap goes above the ascent.
fn hhea_line(ascender: i16, descender: i16, line_gap: i16) -> (f64, f64) {
    (f64::from(ascender) + f64::from(line_gap.max(0)), f64::from(descender).abs())
}

/// Line metrics for a face in font units: (ascent, descent), both positive, the way word
/// processors measure single line spacing: the hhea values with the line gap above the ascent
/// (Word puts the first baseline that far below the line top), falling back to OS/2
/// usWinAscent/usWinDescent.
pub fn line_metrics(face: &crate::FontFace) -> (f64, f64) {
    static CACHE: OnceLock<Mutex<HashMap<u32, (f64, f64)>>> = OnceLock::new();
    let cache = CACHE.get_or_init(|| Mutex::new(HashMap::new()));
    if let Some(v) = cache.lock().unwrap_or_else(|e| e.into_inner()).get(&face.id()) {
        return *v;
    }
    let mut v = (face.ascent.max(1.0), face.descent.abs());
    if let Some(f) = face.skrifa() {
        // hhea ascender + descender + line gap (Word on macOS), the gap above the ascent; OS/2
        // win metrics only when the hhea values are missing, and never more than twice the em.
        let hhea = f.hhea().ok().map(|h| hhea_line(h.ascender().to_i16(), h.descender().to_i16(), h.line_gap().to_i16()));
        let win = f.os2().ok().map(|o| (o.us_win_ascent() as f64, o.us_win_descent() as f64));
        let ok = |m: &(f64, f64)| m.0 > 0.0 && m.0 + m.1 <= face.upem * 2.0;
        if let Some(m) = hhea.filter(ok) {
            v = m;
        } else if let Some(m) = win.filter(ok) {
            v = m;
        }
    }
    cache.lock().unwrap_or_else(|e| e.into_inner()).insert(face.id(), v);
    v
}

#[cfg(test)]
mod tests {
    use super::*;

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

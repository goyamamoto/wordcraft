use super::doc_state::{CellRange, EDIT_MARKS, Selection};
use super::{CharProps, Document, PartKind, Path, Pos, Session, StoryRef, ViewMode, untitled_name};

impl Session {
    /// Remember the caret as the place of an edit, for Go Back. Edits in the same paragraph as
    /// the last one (typing on) update that place rather than adding one.
    pub(super) fn note_edit(&mut self) {
        let at = self.sel.focus.clone();
        match self.edit_marks.last_mut() {
            Some(last) if last.story == at.story && last.path == at.path => *last = at,
            _ => {
                self.edit_marks.push(at);
                if self.edit_marks.len() > EDIT_MARKS {
                    self.edit_marks.remove(0);
                }
            }
        }
        self.go_back_step = 0;
    }

    /// Go Back (Shift+F5): the next of the last edit places, most recent first, cycling; a place
    /// the caret is already at is skipped. `None` before any edit.
    pub fn go_back(&mut self) -> Option<Pos> {
        let n = self.edit_marks.len();
        for _ in 0..n {
            let i = n - 1 - self.go_back_step % n;
            self.go_back_step = self.go_back_step.wrapping_add(1);
            let Some(p) = self.edit_marks.get(i) else { continue };
            let p = self.doc.clamp(p);
            if p != self.sel.focus || n == 1 {
                return Some(p);
            }
        }
        self.edit_marks.last().map(|p| self.doc.clamp(p))
    }

    /// The line pieces of the current column selection, if there is one (the selection hasn't
    /// moved since the block was made).
    pub fn column_segments(&self) -> Option<&[(Pos, Pos)]> {
        // Cells that are one run of the text are an ordinary stream selection.
        self.column.as_ref().filter(|c| c.sel == self.sel && !c.cells.as_ref().is_some_and(|r| r.stream)).map(|c| c.segments.as_slice())
    }

    /// The whole table cells selected, if the selection is that (see [`CellRange`]).
    pub fn cell_range(&self) -> Option<&CellRange> {
        self.column.as_ref().filter(|c| c.sel == self.sel).and_then(|c| c.cells.as_ref())
    }

    /// The selection as it was made: where a drag, Shift+click or Shift+arrows started and where
    /// the caret went, even when Word's table rule widened it to whole cells or rows (`sel` holds
    /// those).
    pub fn sel_ends(&self) -> &Selection {
        self.widened.as_ref().filter(|w| w.sel == self.sel).map_or(&self.sel, |w| &w.ends)
    }

    pub fn selects_whole_story(&self) -> bool {
        let st = self.sel.focus.story;
        let (a, b) = self.sel.ordered();
        a == self.doc.start_of(st) && b == self.doc.end_of(st)
    }

    /// Whether the caret is in a header or footer.
    pub fn in_header_footer(&self) -> bool {
        let StoryRef::Part(id) = self.sel.focus.story else { return false };
        self.doc.parts.get(&id).is_some_and(|p| matches!(p.kind, PartKind::Header | PartKind::Footer))
    }

    /// Document revision (bumped by every change).
    pub fn rev(&self) -> u64 {
        self.rev
    }

    /// Which document is open (see `document_id`): lets a caller tell an edit from a replacement.
    pub fn document_id(&self) -> u64 {
        self.document_id
    }

    /// How many times the body was replaced under this [`Self::document_id`].
    ///
    /// Front ends use it as the body's IME replacement epoch and to tell a body swap from an
    /// ordinary edit; it advances for `document.setText`, a version restore, a compare and
    /// Undo/Redo, never for typing.
    pub fn ime_body_epoch(&self) -> u64 {
        self.ime_body_epoch
    }

    /// Note that a command replaced the document or its whole body under this document id.
    ///
    /// The counter fails closed: if it were ever exhausted (unreachable in practice), the
    /// document owner id advances too, so a cached IME context still cannot be reused.
    pub fn note_body_replaced(&mut self) {
        self.ime_body_epoch = match self.ime_body_epoch.checked_add(1) {
            Some(epoch) => epoch,
            None => {
                self.document_id = self.document_id.saturating_add(1);
                u64::MAX
            }
        };
    }

    pub fn touch(&mut self) {
        self.rev = self.rev.wrapping_add(1);
        self.dirty = true;
    }

    /// Reading Highlight ranges for the current document (searched again after edits); empty when it's off.
    pub fn find_highlights(&mut self) -> &[(Pos, Pos)] {
        if !self.find.highlight {
            return &[];
        }
        if self.find.highlights.as_ref().is_none_or(|(r, _)| *r != self.rev) {
            let found = crate::cmd::edit::search(self, StoryRef::Body).unwrap_or_default();
            self.find.highlights = Some((self.rev, found));
        }
        self.find.highlights.as_ref().map(|(_, v)| v.as_slice()).unwrap_or(&[])
    }

    /// Whether the markup area beside the page can show balloons: markup shown in Print Layout
    /// (not Read Mode or pages side by side), comments not listed in the Comments pane instead,
    /// and Track Changes Options not set to show every revision inline.
    pub fn balloons_shown(&self) -> bool {
        let v = &self.view;
        v.show_markup
            && !v.comments_pane
            && !v.read_mode
            && !v.multi_page
            && v.mode == ViewMode::Print
            && self.prefs.markup.balloons != wordcraft_layout::display::BalloonMode::Inline
    }

    /// Show revisions in balloons, with the balloons on screen: tracked deletions in the body leave
    /// the text for balloons in the markup area. Where no balloons can be shown they stay inline,
    /// as in Word; so do deletions in headers, footers, notes and text boxes, which get no balloons.
    pub fn revisions_in_balloons(&self) -> bool {
        let m = &self.prefs.markup;
        self.balloons_shown() && m.balloons == wordcraft_layout::display::BalloonMode::Revisions && m.insertions_deletions
    }

    /// Initials for new comments: the ones set in File › Options, else the user name's.
    pub fn user_initials(&self) -> String {
        let set = self.initials.trim();
        if set.is_empty() { self.author.split_whitespace().filter_map(|w| w.chars().next()).collect() } else { set.to_string() }
    }

    /// The document's file name and full path for fields (`FILENAME`) and layout context.
    /// Returns `(filename, filepath)`. Unsaved documents with no explicit file name default
    /// consistently to their name, `("Document1", "Document1")` for the first. Document metadata title is never used
    /// as a filename fallback (#3449).
    pub fn resolved_filenames(&self) -> (String, String) {
        if let Some(path) = &self.path {
            let name = path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_else(|| path.to_string_lossy().to_string());
            let full = path.to_string_lossy().to_string();
            (name, full)
        } else if let Some(name) = &self.file_name {
            (name.clone(), name.clone())
        } else {
            let name = untitled_name(self.untitled);
            (name.clone(), name)
        }
    }

    /// The region in effect (#838): the one the user chose ([`Prefs::region`]), else the
    /// system's.
    pub fn region(&self) -> wordcraft_geom::Region {
        match self.prefs.region.as_deref() {
            Some(tag) => wordcraft_geom::Region::from_tag(tag),
            None => self.system_region.clone(),
        }
    }

    /// The English of the region in effect (`en-GB` for a British region), or `None` when the
    /// region's language isn't English, it names no territory, or it is the United States (US
    /// English is the default anyway). An English interface writes new documents in it, as Word
    /// does on a British or Canadian system (#1045).
    pub fn region_english(&self) -> Option<String> {
        let r = self.region();
        (r.language == "en" && !r.territory.is_empty() && r.territory != "US")
            .then(|| format!("en-{}", r.territory))
            .and_then(|t| wordcraft_proof::lang::normalize_tag(&t))
    }

    /// The Spanish of the region in effect (`es-MX` on a Mexican system, `es-UY` on a Uruguayan
    /// one; the ISO 639-2 `spa` counts as Spanish), or Spain's (`es-ES`) when the region's
    /// language isn't Spanish or it names no territory. A Spanish interface writes new documents in it, so they are proofed in the
    /// writer's Spanish from the first word (#3360).
    pub fn region_spanish(&self) -> String {
        let r = self.region();
        (matches!(r.language.as_str(), "es" | "spa") && !r.territory.is_empty())
            .then(|| format!("es-{}", r.territory))
            .and_then(|t| wordcraft_proof::lang::normalize_tag(&t))
            .unwrap_or_else(|| "es-ES".into())
    }

    /// The unit rulers and dialogs show lengths in: the user's choice, else the region's.
    pub fn units(&self) -> wordcraft_geom::Unit {
        self.prefs.units.unwrap_or_else(|| self.region().units())
    }

    /// `pt` in [`Session::units`], written the region's way (`2,54 cm` in Poland).
    pub fn format_length(&self, pt: f32) -> String {
        self.region().format(self.units(), pt)
    }

    /// A new blank document (the Normal template) on the region's paper: Letter in the US and
    /// other Letter regions, A4 elsewhere (#838). Margins stay 2.54 cm (1") all round.
    pub fn blank_document(&self) -> Document {
        blank_document_in(&self.region(), self.east_asian_editing.as_deref())
    }

    /// Whether crop marks are shown at the corners of the text area: the user's choice
    /// (`view.cropMarks`), else on in the Japan region (Word for Mac shows them with Japanese
    /// editing, observed 2026-10-19) and off elsewhere.
    pub fn crop_marks(&self) -> bool {
        self.view.crop_marks.unwrap_or_else(|| self.region().territory == "JP")
    }

    /// Whether the document asks for Microsoft's Aptos family (the theme's body or heading font,
    /// the document defaults' or a style's font) and no Aptos is installed. Word lays it out with
    /// Aptos; no open font has its widths, so a stand-in breaks lines and pages differently. The
    /// interface then points to Microsoft's free download (`ui.getAptos`): installed, it is used.
    pub fn missing_aptos(&self) -> bool {
        let aptos = |f: &str| f.trim().get(..5).is_some_and(|p| p.eq_ignore_ascii_case("aptos"));
        let s = &self.doc.styles;
        let uses = aptos(&self.doc.settings.minor_font)
            || aptos(&self.doc.settings.major_font)
            || s.default_chr.font.as_deref().is_some_and(aptos)
            || s.styles.iter().any(|st| st.chr.font.as_deref().is_some_and(aptos));
        uses && !wordcraft_fonts::FontDb::global().has_family("Aptos")
    }

    /// The paragraph path the caret is in.
    pub fn caret_path(&self) -> &Path {
        &self.sel.focus.path
    }

    pub fn story(&self) -> StoryRef {
        self.sel.focus.story
    }

    /// Formatting typing would use at the caret: the caret's, except run direction. New text
    /// takes its direction from its own letters, as on screen, so Latin typed or inserted into
    /// reopened Hebrew isn't saved as right-to-left; the DOCX writer marks right-to-left pieces
    /// itself (#1543). The bidi language stays (it only applies to complex-script text).
    /// Over a selection, as in Word, typing takes the first selected character's formatting,
    /// whichever way the selection was made (#2879).
    pub fn typing_props(&self) -> CharProps {
        let (a, _) = self.sel.ordered();
        let first = match self.doc.para_at(&a) {
            Some(p) if self.pending.is_none() && !self.sel.is_collapsed() && a.off < p.len() => Some(p.props_of_char(a.off)),
            _ => None,
        };
        let mut c = match first {
            Some(props) => {
                // The selection is replaced, so the new text isn't part of a link or a note number.
                let mut out = if props.link.is_some() { props.unlinked() } else { props.clone() };
                if matches!(out.style.as_deref(), Some("FootnoteReference" | "EndnoteReference")) {
                    out.style = None;
                }
                out
            }
            None => self.caret_props(),
        };
        c.rtl = None;
        c
    }

    /// The caret's formatting, as Reveal Formatting shows it.
    pub fn caret_props(&self) -> CharProps {
        if let Some(p) = &self.pending {
            return p.clone();
        }
        if let Some(p) = self.composition_props() {
            return p.clone();
        }
        let f = &self.sel.focus;
        let Some(p) = self.doc.para_at(f) else { return CharProps::default() };
        let props = p.props_at(f.off);
        // As in Word, text typed at either end of a hyperlink isn't part of it; only typing
        // inside the link extends it.
        let inside_link = props.link.is_some() && f.off > 0 && f.off < p.len() && p.props_of_char(f.off).link == props.link;
        // Text typed next to a symbol is in the text's font, not the symbol's (`w:sym`).
        let mut out = (if props.link.is_some() && !inside_link { props.unlinked() } else { props.clone() }).without_symbol();
        // A footnote or endnote number is raised by its character style; text typed after it is body text.
        if matches!(out.style.as_deref(), Some("FootnoteReference" | "EndnoteReference")) {
            out.style = None;
        }
        out
    }

    /// Words in the document, as the status bar shows them (see [`Prefs::count_notes`]).
    pub fn word_count(&self) -> usize {
        if self.prefs.count_notes { self.doc.word_count_including_notes() } else { self.doc.word_count() }
    }

    /// Selected plain text.
    pub fn selected_text(&self) -> String {
        // A column block or whole cells that aren't one run of the text: each piece's own text,
        // a line each, so the status bar and Word Count count those cells only.
        if let Some(segs) = self.column_segments() {
            return segs.iter().map(|(a, b)| self.doc.copy_range(a.min(b), a.max(b)).plain_text()).collect::<Vec<_>>().join("\n");
        }
        let (a, b) = self.sel.ordered();
        self.doc.copy_range(&a, &b).plain_text()
    }
}

/// A new blank document on `region`'s paper (see [`Session::blank_document`]). A Japanese
/// region gets Word's Japanese Normal template ([`japanese_normal`]); Chinese and Korean ones
/// the theme font language that picks their script's theme font for Han text.
pub fn blank_document(region: &wordcraft_geom::Region) -> Document {
    blank_document_in(region, None)
}

/// [`blank_document`] with `east_asian` as the East Asian editing language
/// ([`Session::east_asian_editing`]), which wins over the region's language: Japanese editing on
/// an English Mac in Japan starts from Word's Japanese Normal template, as Word for Mac does
/// there (observed 2026-10-20, interface English, region `en-JP`, East Asian Languages Japanese).
pub fn blank_document_in(region: &wordcraft_geom::Region, east_asian: Option<&str>) -> Document {
    let mut d = Document::new();
    let (_, w, h) = region.paper();
    d.last_section.page_w = w;
    d.last_section.page_h = h;
    let territory = region.territory.to_ascii_uppercase();
    match east_asian {
        Some(tag) if tag.to_ascii_lowercase().starts_with("ja") => japanese_normal(&mut d),
        Some(tag) => d.settings.theme_font_lang_ea = Some(tag.into()),
        None => match region.language.to_ascii_lowercase().as_str() {
            "ja" => japanese_normal(&mut d),
            "zh" => d.settings.theme_font_lang_ea = Some(if matches!(territory.as_str(), "TW" | "HK" | "MO") { "zh-TW" } else { "zh-CN" }.into()),
            "ko" => d.settings.theme_font_lang_ea = Some("ko-KR".into()),
            _ => {}
        },
    }
    d
}

/// The East Asian editing language among the system's preferred languages (BCP 47 tags, most
/// preferred first): the first Japanese, Chinese or Korean one, as `ja-JP`, `zh-CN` (`zh-TW` for
/// Traditional Chinese, Taiwan, Hong Kong and Macao) or `ko-KR`; `None` when there is none.
pub fn east_asian_editing_language<'a>(tags: impl IntoIterator<Item = &'a str>) -> Option<String> {
    tags.into_iter().find_map(|tag| {
        let tag = tag.to_ascii_lowercase();
        let mut parts = tag.split(['-', '_', '.', '@']);
        match parts.next()? {
            "ja" => Some("ja-JP".into()),
            "ko" => Some("ko-KR".into()),
            "zh" => Some(if parts.any(|p| matches!(p, "hant" | "tw" | "hk" | "mo")) { "zh-TW" } else { "zh-CN" }.into()),
            _ => None,
        }
    })
}

/// Give `doc` the East Asian language `tag` where it names none — in its document defaults and
/// as its theme font language — as Word for Mac does with that East Asian editing language: a
/// file whose defaults say only `w:lang w:val="en-US"` has its Japanese text in Japanese (the
/// object model's `language ID` of the paragraph), and Word saves it back with
/// `w:eastAsia="ja-JP"` in both (`lg1` probe, 2026-10-20). A language the file names stays.
pub fn fill_east_asian_language(doc: &mut Document, tag: &str) {
    if doc.styles.default_chr.lang_ea.is_none() {
        doc.styles.default_chr.lang_ea = Some(tag.into());
    }
    if doc.settings.theme_font_lang_ea.is_none() {
        doc.settings.theme_font_lang_ea = Some(tag.into());
    }
}

/// Word's Normal template with Japanese as the East Asian editing language, measured in Word
/// for Mac 16.113 (a Normal.dotm it created afresh): 10.5 pt body text (Latin and East Asian
/// sizes `w:sz="21"`), single line spacing with no space after and no widow control, a line grid
/// of 18 pt (`w:docGrid w:type="lines" w:linePitch="360"`), 42 pt default tabs, header and
/// footer 851 and 992 twips from the edge, 425 twips between columns, punctuation compression on, the East Asian language
/// `ja-JP` in the document defaults and `w:themeFontLang`, and Word's Japanese compatibility
/// switches (`w:doNotExpandShiftReturn`, `w:ulTrailSpace`, `w:spaceForUL`,
/// `w:balanceSingleByteDoubleByteWidth`, `w:doNotLeaveBackslashAlone`,
/// `w:adjustLineHeightInTable`, `w:useFELayout`). The paper follows the region as before.
/// The styles of Word's Japanese Normal.dotm where they differ from its English one (the files
/// Word for Mac saved after its built-in styles were applied through its object model with
/// Japanese editing, `st1`–`st3` and `fn1` probes, 2026-10-19): the headings are black (the
/// theme's text colour, where the English ones are its dark blue), in the major fonts (Aptos
/// Display and 游ゴシック Light) at every level, Heading 1–3 16, 14 and 12 pt with 14, 8 and 8 pt
/// before and 4 after, Heading 4–9 the text's size with 4 before and 2 after, none italic, all
/// kept with the next paragraph and on one page; Title and Subtitle are centred (Subtitle 8 pt
/// after); Quote has 8 pt after as well as before; Caption is bold and nothing else; Header and
/// Footer keep Normal's spacing, off the line grid, with the 234 and 468 pt tabs; TOC 1 has no
/// indent or spacing and TOC 2–9 are indented one more character a level; TOC Heading is Heading 1
/// at 12 pt in the automatic colour with no space before or after, not kept together; footnote
/// and endnote text are Normal off the grid (10.5 pt, where the English template's are 10 pt with
/// single spacing and no space after), the references superscript in both.
pub fn japanese_normal_styles(styles: &mut wordcraft_doc::styles::StyleSheet) {
    use wordcraft_doc::props::{Align, CharProps, ParaProps, Rgb, TextColor};
    let black = Some(TextColor::Rgb(Rgb(0, 0, 0)));
    for n in 1..=9u32 {
        let Some(style) = styles.get_mut(&format!("Heading{n}")) else { continue };
        let (size, before, after) = match n {
            1 => (Some(16.0), 14.0, 4.0),
            2 => (Some(14.0), 8.0, 4.0),
            3 => (Some(12.0), 8.0, 4.0),
            _ => (None, 4.0, 2.0),
        };
        style.para.space_before = Some(before);
        style.para.space_after = Some(after);
        style.para.keep_next = Some(true);
        style.para.keep_lines = Some(true);
        style.chr.font = Some(wordcraft_doc::styles::HEADING_FONT.into());
        style.chr.size = size;
        style.chr.italic = None;
        style.chr.color = black;
    }
    if let Some(style) = styles.get_mut("Title") {
        style.para.align = Some(Align::Center);
    }
    if let Some(style) = styles.get_mut("Subtitle") {
        style.para.align = Some(Align::Center);
        style.para.space_after = Some(8.0);
        style.chr.font = Some(wordcraft_doc::styles::HEADING_FONT.into());
    }
    if let Some(style) = styles.get_mut("Quote") {
        style.para.space_after = Some(8.0);
    }
    if let Some(style) = styles.get_mut("Caption") {
        style.para = ParaProps::default();
        style.chr = CharProps { bold: Some(true), ..Default::default() };
    }
    for id in ["Header", "Footer"] {
        if let Some(style) = styles.get_mut(id) {
            style.para = ParaProps { tabs: style.para.tabs.take(), snap_to_grid: Some(false), ..Default::default() };
        }
    }
    for n in 1..=9u32 {
        if let Some(style) = styles.get_mut(&format!("TOC{n}")) {
            style.para = ParaProps { indent_left_chars: (n > 1).then_some((n - 1) as f32), ..Default::default() };
        }
    }
    if let Some(style) = styles.get_mut("TOCHeading") {
        style.para.space_before = Some(0.0);
        style.para.space_after = Some(0.0);
        style.para.keep_lines = Some(false);
        style.chr.size = Some(12.0);
        style.chr.color = None;
    }
    for id in ["FootnoteText", "EndnoteText"] {
        if let Some(style) = styles.get_mut(id) {
            style.para = ParaProps { snap_to_grid: Some(false), ..Default::default() };
            style.chr = CharProps::default();
        }
    }
}

pub fn japanese_normal(d: &mut Document) {
    let chr = &mut d.styles.default_chr;
    chr.size = Some(10.5);
    chr.size_cs = Some(12.0);
    // Its Latin language is English (Japan), which Word has no proofing tools for: nothing in a
    // new Japanese document is marked as misspelt until a run is given another language.
    chr.lang = Some("en-JP".into());
    chr.lang_ea = Some("ja-JP".into());
    // Its document defaults: Kerning for fonts from 2 pt and standard and contextual ligatures.
    chr.kern = Some(2.0);
    chr.ligatures = Some(wordcraft_doc::props::Ligatures::STANDARD_CONTEXTUAL);
    let para = &mut d.styles.default_para;
    para.line_spacing = Some(wordcraft_doc::props::LineSpacing::Multiple(1.0));
    para.space_after = Some(0.0);
    para.widow_control = Some(false);
    d.settings.theme_font_lang_ea = Some("ja-JP".into());
    d.settings.punct_compression = wordcraft_doc::east_asian::PunctCompression::CompressPunctuation;
    d.settings.default_tab = 42.0;
    // Word's Japanese Normal.dotm sets these compatibility switches (its English one sets none):
    // a justified line ending with a manual line break is not spread, underlines run under
    // trailing spaces.
    d.settings.do_not_expand_shift_return = true;
    d.settings.ul_trail_space = true;
    d.settings.space_for_ul = true;
    d.settings.balance_single_byte_double_byte_width = true;
    d.settings.do_not_leave_backslash_alone = true;
    d.settings.adjust_line_height_in_table = true;
    d.settings.use_fe_layout = true;
    japanese_normal_styles(&mut d.styles);
    let sect = &mut d.last_section;
    sect.doc_grid = Some(wordcraft_doc::section::DocGrid { kind: wordcraft_doc::section::DocGridType::Lines, line_pitch: 18.0, char_space: 0 });
    sect.header = 851.0 / 20.0;
    sect.footer = 992.0 / 20.0;
    sect.columns.space = 425.0 / 20.0;
}

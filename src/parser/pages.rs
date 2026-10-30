use std::collections::{BTreeMap, HashMap, HashSet};
use std::io::Cursor;

use quick_xml::events::Event;
use quick_xml::{Reader, Writer};

use super::*;

#[derive(Debug, Clone, Copy, Default)]
struct Boundaries {
    explicit: bool,
    rendered: bool,
}

impl Boundaries {
    fn selected(self, mode: PageBoundaryMode) -> Option<PageBoundary> {
        match mode {
            PageBoundaryMode::Combined if self.explicit => Some(PageBoundary::Explicit),
            PageBoundaryMode::Combined if self.rendered => Some(PageBoundary::RenderedHint),
            PageBoundaryMode::ExplicitOnly if self.explicit => Some(PageBoundary::Explicit),
            PageBoundaryMode::RenderedHintsOnly if self.rendered => {
                Some(PageBoundary::RenderedHint)
            }
            _ => None,
        }
    }
}

struct RawBlock {
    xml: Vec<u8>,
    before: Boundaries,
    after: Boundaries,
    listish: bool,
    next_page_parity: Option<PageParity>,
}

struct BlockAnalysis {
    before: Boundaries,
    after: Boundaries,
    style_name: Option<String>,
    listish: bool,
    next_page_parity: Option<PageParity>,
    saw_section_properties: bool,
    continuous_section: bool,
}

#[derive(Clone, Copy)]
enum PageParity {
    Odd,
    Even,
}

struct Scanner {
    reader: Reader<Cursor<Vec<u8>>>,
    part: String,
    format: DocumentFormat,
    in_main: bool,
    finished: bool,
    pending: Option<RawBlock>,
}

impl Scanner {
    fn new(xml: Vec<u8>, part: String, format: DocumentFormat) -> Self {
        let mut reader = Reader::from_reader(Cursor::new(xml));
        reader.config_mut().trim_text(false);
        Self {
            reader,
            part,
            format,
            in_main: false,
            finished: false,
            pending: None,
        }
    }

    fn event(&mut self, buf: &mut Vec<u8>) -> Result<Event<'static>> {
        self.reader
            .read_event_into(buf)
            .map(Event::into_owned)
            .map_err(|source| Error::xml(self.part.clone(), source))
    }

    fn next_block(
        &mut self,
        docx_styles: Option<&HashMap<String, Style>>,
        odt_styles: Option<&HashMap<String, OdtStyle>>,
    ) -> Result<Option<RawBlock>> {
        if let Some(block) = self.pending.take() {
            return Ok(Some(block));
        }
        if self.finished {
            return Ok(None);
        }
        let mut buf = Vec::new();
        loop {
            let event = self.event(&mut buf)?;
            match &event {
                Event::Start(e)
                    if self.format == DocumentFormat::Docx
                        && local(e.name().as_ref()) == b"body" =>
                {
                    self.in_main = true
                }
                Event::Start(e)
                    if self.format == DocumentFormat::Odt
                        && local(e.name().as_ref()) == b"text" =>
                {
                    self.in_main = true
                }
                Event::End(e)
                    if self.in_main
                        && ((self.format == DocumentFormat::Docx
                            && local(e.name().as_ref()) == b"body")
                            || (self.format == DocumentFormat::Odt
                                && local(e.name().as_ref()) == b"text")) =>
                {
                    self.finished = true;
                    return Ok(None);
                }
                Event::Start(e) if self.in_main && self.is_block(local(e.name().as_ref())) => {
                    return self.capture(event, docx_styles, odt_styles).map(Some);
                }
                Event::Empty(e) if self.in_main && self.is_block(local(e.name().as_ref())) => {
                    let mut writer = Writer::new(Vec::new());
                    writer.write_event(event)?;
                    return Ok(Some(RawBlock {
                        xml: writer.into_inner(),
                        before: Boundaries::default(),
                        after: Boundaries::default(),
                        listish: false,
                        next_page_parity: None,
                    }));
                }
                Event::Eof => {
                    self.finished = true;
                    return Ok(None);
                }
                _ => {}
            }
            buf.clear();
        }
    }

    fn is_block(&self, name: &[u8]) -> bool {
        match self.format {
            DocumentFormat::Docx => matches!(name, b"p" | b"tbl" | b"sectPr"),
            DocumentFormat::Odt => matches!(name, b"p" | b"h" | b"list" | b"table"),
        }
    }

    fn capture(
        &mut self,
        first: Event<'static>,
        docx_styles: Option<&HashMap<String, Style>>,
        odt_styles: Option<&HashMap<String, OdtStyle>>,
    ) -> Result<RawBlock> {
        let root_name = match &first {
            Event::Start(e) => local(e.name().as_ref()).to_vec(),
            _ => unreachable!(),
        };
        let mut writer = Writer::new(Vec::new());
        let mut analysis = BlockAnalysis {
            before: Boundaries::default(),
            after: Boundaries::default(),
            style_name: None,
            listish: root_name == b"list",
            next_page_parity: None,
            saw_section_properties: false,
            continuous_section: false,
        };
        analyze_event(&first, self.format, &mut analysis);
        writer.write_event(first)?;
        let mut depth = 1usize;
        let mut buf = Vec::new();
        while depth > 0 {
            let event = self.event(&mut buf)?;
            match &event {
                Event::Start(_) => depth += 1,
                Event::End(_) => depth -= 1,
                Event::Eof => {
                    return Err(Error::Conversion(format!(
                        "unexpected end of {} inside {}",
                        self.part,
                        String::from_utf8_lossy(&root_name)
                    )));
                }
                _ => {}
            }
            analyze_event(&event, self.format, &mut analysis);
            writer.write_event(event)?;
            buf.clear();
        }
        if matches!(root_name.as_slice(), b"tbl" | b"table" | b"list") && analysis.before.explicit {
            analysis.before.explicit = false;
            analysis.after.explicit = true;
        }
        if self.format == DocumentFormat::Docx
            && root_name == b"p"
            && analysis.saw_section_properties
            && !analysis.continuous_section
        {
            analysis.after.explicit = true;
        }
        let has_page_style = match self.format {
            DocumentFormat::Docx => root_name == b"p",
            DocumentFormat::Odt => matches!(root_name.as_slice(), b"p" | b"h" | b"list"),
        };
        match self.format {
            DocumentFormat::Docx => {
                if has_page_style
                    && let (Some(styles), Some(name)) =
                        (docx_styles, analysis.style_name.as_deref())
                {
                    let style = resolved_style(styles, name);
                    analysis.before.explicit |= style.page_break_before.unwrap_or(false);
                    analysis.listish |= style.num_id.is_some();
                }
            }
            DocumentFormat::Odt => {
                if has_page_style
                    && let (Some(styles), Some(name)) = (odt_styles, analysis.style_name.as_deref())
                {
                    let style = resolved_odt_page_style(styles, name);
                    analysis.before.explicit |= style.0;
                    analysis.after.explicit |= style.1;
                }
            }
        }
        Ok(RawBlock {
            xml: writer.into_inner(),
            before: analysis.before,
            after: analysis.after,
            listish: analysis.listish,
            next_page_parity: analysis.next_page_parity,
        })
    }
}

fn analyze_event(event: &Event<'_>, format: DocumentFormat, analysis: &mut BlockAnalysis) {
    let element = match event {
        Event::Start(e) | Event::Empty(e) => e,
        _ => return,
    };
    let qualified_name = element.name();
    let name = local(qualified_name.as_ref());
    match format {
        DocumentFormat::Docx => match name {
            b"sectPr" => analysis.saw_section_properties = true,
            b"pStyle" => analysis.style_name = val(element),
            b"numId" => analysis.listish = true,
            b"pageBreakBefore" => analysis.before.explicit |= bool_val(element, true),
            b"br" if attr(element, b"type").as_deref() == Some("page") => {
                analysis.after.explicit = true
            }
            b"lastRenderedPageBreak" => analysis.after.rendered = true,
            b"type"
                if matches!(
                    val(element).as_deref(),
                    Some("nextPage" | "oddPage" | "evenPage")
                ) =>
            {
                analysis.after.explicit = true;
                analysis.next_page_parity = match val(element).as_deref() {
                    Some("oddPage") => Some(PageParity::Odd),
                    Some("evenPage") => Some(PageParity::Even),
                    _ => None,
                };
            }
            b"type" if val(element).as_deref() == Some("continuous") => {
                analysis.continuous_section = true;
            }
            _ => {}
        },
        DocumentFormat::Odt => {
            if analysis.style_name.is_none() && matches!(name, b"p" | b"h" | b"list" | b"table") {
                analysis.style_name = attr(element, b"style-name");
            }
            if name == b"soft-page-break" {
                analysis.after.rendered = true;
            }
            analysis.before.explicit |= attr(element, b"break-before").as_deref() == Some("page");
            analysis.after.explicit |= attr(element, b"break-after").as_deref() == Some("page");
        }
    }
}

fn resolved_odt_page_style(styles: &HashMap<String, OdtStyle>, name: &str) -> (bool, bool) {
    fn visit(
        styles: &HashMap<String, OdtStyle>,
        name: &str,
        seen: &mut HashSet<String>,
    ) -> (bool, bool) {
        if !seen.insert(name.to_owned()) {
            return (false, false);
        }
        let Some(style) = styles.get(name) else {
            return (false, false);
        };
        let inherited = style
            .parent
            .as_deref()
            .map_or((false, false), |parent| visit(styles, parent, seen));
        (
            inherited.0 || style.break_before_page,
            inherited.1 || style.break_after_page,
        )
    }
    visit(styles, name, &mut HashSet::new())
}

fn resolved_odt_master(styles: &HashMap<String, OdtStyle>, name: &str) -> Option<String> {
    fn visit(
        styles: &HashMap<String, OdtStyle>,
        name: &str,
        seen: &mut HashSet<String>,
    ) -> Option<String> {
        if !seen.insert(name.to_owned()) {
            return None;
        }
        let style = styles.get(name)?;
        style.master_page.clone().or_else(|| {
            style
                .parent
                .as_deref()
                .and_then(|parent| visit(styles, parent, seen))
        })
    }
    visit(styles, name, &mut HashSet::new())
}

#[derive(Default)]
struct PageAssets {
    headers: BTreeMap<String, Story>,
    footers: BTreeMap<String, Story>,
    comments: Vec<Annotation>,
    footnotes: Vec<Annotation>,
    endnotes: Vec<Annotation>,
    image_parts: BTreeMap<String, (String, String)>,
    initial_headers: Vec<String>,
    initial_footers: Vec<String>,
    first_page_stories: bool,
    even_page_stories: bool,
}

#[derive(Default)]
struct DocxHeaderHints {
    kinds: HashMap<String, HeaderFooterKind>,
    headers: Vec<String>,
    footers: Vec<String>,
    title_page: bool,
    even_and_odd: bool,
}

fn scan_docx_header_hints(
    package: &[u8],
    xml: &[u8],
    part: &str,
    indexes: &DocxIndexes,
) -> DocxHeaderHints {
    let mut hints = DocxHeaderHints::default();
    let mut reader = xml_reader(xml);
    let mut in_first_section_properties = false;
    let mut first_section_complete = false;
    while let Ok(event) = reader.read_event() {
        if let Event::End(e) = &event
            && local(e.name().as_ref()) == b"sectPr"
            && in_first_section_properties
        {
            in_first_section_properties = false;
            first_section_complete = true;
            continue;
        }
        let element = match event {
            Event::Start(ref e) | Event::Empty(ref e) => e,
            Event::Eof => break,
            _ => continue,
        };
        let qualified_name = element.name();
        let name = local(qualified_name.as_ref());
        if name == b"sectPr" && !first_section_complete {
            in_first_section_properties = true;
        }
        if name == b"titlePg" && in_first_section_properties {
            hints.title_page = bool_val(element, true);
        }
        if matches!(name, b"headerReference" | b"footerReference")
            && let Some(id) = attr(element, b"id")
            && let Some(rel) = relationship(indexes, part, &id)
            && !rel.external
        {
            let kind = match attr(element, b"type").as_deref() {
                Some("first") => HeaderFooterKind::First,
                Some("even") => HeaderFooterKind::Even,
                _ => HeaderFooterKind::Default,
            };
            hints.kinds.insert(rel.target.clone(), kind);
            if in_first_section_properties {
                let target = if name == b"headerReference" {
                    &mut hints.headers
                } else {
                    &mut hints.footers
                };
                if !target.contains(&rel.target) {
                    target.push(rel.target.clone());
                }
            }
        }
    }
    if let Ok(Some(settings)) = read_part(package, "word/settings.xml") {
        let mut reader = xml_reader(&settings);
        while let Ok(event) = reader.read_event() {
            match event {
                Event::Start(e) | Event::Empty(e)
                    if local(e.name().as_ref()) == b"evenAndOddHeaders" =>
                {
                    hints.even_and_odd = bool_val(&e, true);
                }
                Event::Eof => break,
                _ => {}
            }
        }
    }
    hints
}

enum PageFormatState {
    Docx {
        indexes: DocxIndexes,
        state: Box<ParseState>,
        part: String,
    },
    Odt {
        styles: HashMap<String, OdtStyle>,
        sequence: usize,
        current_master: String,
    },
}

/// Lazy iterator over block-aligned document pages.
pub struct PageIterator<'a> {
    package: &'a [u8],
    config: ParserConfig,
    mode: PageBoundaryMode,
    scanner: Scanner,
    format_state: PageFormatState,
    assets: PageAssets,
    page_number: usize,
    previous_boundary: Option<PageBoundary>,
    pending_blank_page: bool,
    failed: bool,
}

pub(crate) fn page_iterator(
    package: &[u8],
    format: DocumentFormat,
    config: ParserConfig,
    mode: PageBoundaryMode,
) -> Result<PageIterator<'_>> {
    match format {
        DocumentFormat::Docx => docx_iterator(package, config, mode),
        DocumentFormat::Odt => odt_iterator(package, config, mode),
    }
}

fn docx_iterator(
    package: &[u8],
    config: ParserConfig,
    mode: PageBoundaryMode,
) -> Result<PageIterator<'_>> {
    let indexes = build_docx_indexes(package)?;
    let part = docx_document_part(package, &indexes)?;
    let xml = required_part(package, &part)?;
    let header_hints = scan_docx_header_hints(package, &xml, &part, &indexes);
    let mut assets = PageAssets::default();
    let mut asset_state = ParseState::default();
    if config.include_headers_footers {
        for story_part in list_parts(package)? {
            let story_type =
                if story_part.starts_with("word/header") && story_part.ends_with(".xml") {
                    Some(StoryType::Header)
                } else if story_part.starts_with("word/footer") && story_part.ends_with(".xml") {
                    Some(StoryType::Footer)
                } else {
                    None
                };
            let Some(story_type) = story_type else {
                continue;
            };
            let data = required_part(package, &story_part)?;
            let elements = parse_docx_story(
                &data,
                &story_part,
                story_type,
                &indexes,
                &mut asset_state,
                &config,
            )?;
            let story = Story {
                kind: Some(
                    header_hints
                        .kinds
                        .get(&story_part)
                        .copied()
                        .unwrap_or(HeaderFooterKind::Default),
                ),
                used_by_sections: Vec::new(),
                elements,
            };
            if story_type == StoryType::Header {
                assets.headers.insert(story_part, story);
            } else {
                assets.footers.insert(story_part, story);
            }
        }
    }
    if config.include_comments {
        assets.comments = parse_docx_annotations(
            package,
            "word/comments.xml",
            "comment",
            StoryType::Comment,
            &indexes,
            &mut asset_state,
            &config,
        )?;
    }
    if config.include_footnotes {
        assets.footnotes = parse_docx_annotations(
            package,
            "word/footnotes.xml",
            "footnote",
            StoryType::Footnote,
            &indexes,
            &mut asset_state,
            &config,
        )?;
    }
    if config.include_endnotes {
        assets.endnotes = parse_docx_annotations(
            package,
            "word/endnotes.xml",
            "endnote",
            StoryType::Endnote,
            &indexes,
            &mut asset_state,
            &config,
        )?;
    }
    assets.image_parts = asset_state.image_parts;
    assets.initial_headers = header_hints.headers;
    assets.initial_footers = header_hints.footers;
    assets.first_page_stories = header_hints.title_page;
    assets.even_page_stories = header_hints.even_and_odd;
    Ok(PageIterator {
        package,
        config,
        mode,
        scanner: Scanner::new(xml, part.clone(), DocumentFormat::Docx),
        format_state: PageFormatState::Docx {
            indexes,
            state: Box::new(ParseState::default()),
            part,
        },
        assets,
        page_number: 0,
        previous_boundary: None,
        pending_blank_page: false,
        failed: false,
    })
}

fn odt_iterator(
    package: &[u8],
    config: ParserConfig,
    mode: PageBoundaryMode,
) -> Result<PageIterator<'_>> {
    let xml = required_part(package, "content.xml")?;
    let mut styles = HashMap::new();
    if let Some(data) = read_part(package, "styles.xml")? {
        scan_odt_styles(&data, "styles.xml", &mut styles)?;
    }
    let body_offset = xml
        .windows(b"<office:body".len())
        .position(|window| window == b"<office:body")
        .unwrap_or(xml.len());
    scan_odt_styles(&xml[..body_offset], "content.xml", &mut styles)?;
    let stories = parse_odt_master_pages(package, &styles, &config)?;
    Ok(PageIterator {
        package,
        config,
        mode,
        scanner: Scanner::new(xml, "content.xml".into(), DocumentFormat::Odt),
        format_state: PageFormatState::Odt {
            styles,
            sequence: 0,
            current_master: "Standard".into(),
        },
        assets: PageAssets {
            headers: stories.headers,
            footers: stories.footers,
            first_page_stories: true,
            even_page_stories: true,
            ..PageAssets::default()
        },
        page_number: 0,
        previous_boundary: None,
        pending_blank_page: false,
        failed: false,
    })
}

impl Iterator for PageIterator<'_> {
    type Item = Result<DocumentPage>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.failed {
            return None;
        }
        match self.next_page() {
            Ok(Some(page)) => Some(Ok(page)),
            Ok(None) => None,
            Err(error) => {
                self.failed = true;
                Some(Err(error))
            }
        }
    }
}

impl PageIterator<'_> {
    fn next_page(&mut self) -> Result<Option<DocumentPage>> {
        if self.pending_blank_page {
            self.pending_blank_page = false;
            let page = self.build_page(Vec::new(), PageBoundary::Explicit)?;
            self.previous_boundary = Some(PageBoundary::Explicit);
            return Ok(Some(page));
        }
        if self.scanner.finished && self.scanner.pending.is_none() {
            return Ok(None);
        }
        let mut fragments = Vec::new();
        let boundary;
        let mut ending_parity = None;
        let mut deferred_list_boundary = None;
        let mut last_was_listish = false;
        loop {
            let next = match &self.format_state {
                PageFormatState::Docx { indexes, .. } => {
                    self.scanner.next_block(Some(&indexes.styles), None)?
                }
                PageFormatState::Odt { styles, .. } => {
                    self.scanner.next_block(None, Some(styles))?
                }
            };
            let Some(block) = next else {
                if fragments.is_empty() && self.page_number > 0 {
                    return Ok(None);
                }
                boundary = PageBoundary::EndOfDocument;
                break;
            };
            if let Some(pending_boundary) = deferred_list_boundary
                && !block.listish
            {
                self.scanner.pending = Some(block);
                boundary = pending_boundary;
                break;
            }
            if let Some(before) = block.before.selected(self.mode)
                && !fragments.is_empty()
                && deferred_list_boundary.is_none()
            {
                if block.listish && last_was_listish {
                    fragments.push(block.xml);
                    deferred_list_boundary = Some(before);
                    last_was_listish = true;
                    continue;
                } else {
                    self.scanner.pending = Some(block);
                    boundary = before;
                    break;
                }
            }
            let suppress_duplicate_hint = fragments.is_empty()
                && self.previous_boundary == Some(PageBoundary::Explicit)
                && !block.after.explicit
                && block.after.rendered;
            let after = (!suppress_duplicate_hint)
                .then(|| block.after.selected(self.mode))
                .flatten();
            let listish = block.listish;
            let block_parity = block.next_page_parity;
            fragments.push(block.xml);
            last_was_listish = listish;
            if let Some(after) = after {
                if listish {
                    deferred_list_boundary = Some(after);
                } else {
                    boundary = after;
                    ending_parity = block_parity;
                    break;
                }
            }
        }
        let page = self.build_page(fragments, boundary)?;
        if let Some(parity) = ending_parity {
            let next_number = self.page_number + 1;
            self.pending_blank_page = match parity {
                PageParity::Odd => next_number.is_multiple_of(2),
                PageParity::Even => !next_number.is_multiple_of(2),
            };
        }
        self.previous_boundary = Some(boundary);
        Ok(Some(page))
    }

    fn build_page(
        &mut self,
        fragments: Vec<Vec<u8>>,
        boundary: PageBoundary,
    ) -> Result<DocumentPage> {
        let xml = fragments.into_iter().flatten().collect::<Vec<_>>();
        let (elements, local_annotations) = match &mut self.format_state {
            PageFormatState::Docx {
                indexes,
                state,
                part,
            } => {
                let elements = parse_docx_story(
                    &wrap_docx(&xml),
                    part,
                    StoryType::Main,
                    indexes,
                    state,
                    &self.config,
                )?;
                self.assets.image_parts.extend(state.image_parts.clone());
                (elements, None)
            }
            PageFormatState::Odt {
                styles,
                sequence,
                current_master,
            } => {
                let mut parsed = parse_odt_content(&wrap_odt(&xml), styles, &self.config)?;
                if let Some(style_name) = parsed
                    .elements
                    .first()
                    .and_then(|element| element.provenance().paragraph_style.as_deref())
                    && let Some(master) = resolved_odt_master(styles, style_name)
                {
                    *current_master = master;
                }
                offset_sequences(&mut parsed.elements, *sequence);
                *sequence += parsed.elements.len();
                (
                    parsed.elements,
                    Some((parsed.comments, parsed.footnotes, parsed.endnotes)),
                )
            }
        };
        self.page_number += 1;
        let (comment_ids, footnote_ids, endnote_ids) = referenced_annotations(&elements);
        let (comments, footnotes, endnotes) = local_annotations.unwrap_or_else(|| {
            (
                filter_annotations(&self.assets.comments, &comment_ids),
                filter_annotations(&self.assets.footnotes, &footnote_ids),
                filter_annotations(&self.assets.endnotes, &endnote_ids),
            )
        });
        let references = self.current_section_references();
        let usage = self.current_header_footer_usage();
        let header = select_story(
            &self.assets.headers,
            self.page_number,
            &references.0,
            &usage,
            self.assets.first_page_stories,
            self.assets.even_page_stories,
        );
        let footer = select_story(
            &self.assets.footers,
            self.page_number,
            &references.1,
            &usage,
            self.assets.first_page_stories,
            self.assets.even_page_stories,
        );
        let mut image_ids = HashSet::new();
        collect_image_ids(&elements, &mut image_ids);
        for annotation in comments.iter().chain(&footnotes).chain(&endnotes) {
            collect_image_ids(&annotation.elements, &mut image_ids);
        }
        for story in header.iter().chain(footer.iter()) {
            collect_image_ids(&story.elements, &mut image_ids);
        }
        let images = self.load_images(image_ids)?;
        Ok(DocumentPage {
            page_number: self.page_number,
            elements,
            boundary,
            header,
            footer,
            comments,
            footnotes,
            endnotes,
            images,
            config: self.config.clone(),
        })
    }

    fn current_section_references(&self) -> (Vec<String>, Vec<String>) {
        match &self.format_state {
            PageFormatState::Docx { state, .. } => {
                let section = state.sections.last().or(state.pending_section.as_ref());
                section.map_or_else(
                    || {
                        (
                            self.assets.initial_headers.clone(),
                            self.assets.initial_footers.clone(),
                        )
                    },
                    |section| {
                        (
                            section.header_references.clone(),
                            section.footer_references.clone(),
                        )
                    },
                )
            }
            PageFormatState::Odt { current_master, .. } => {
                let prefix = format!("styles.xml#{current_master}/");
                (
                    self.assets
                        .headers
                        .keys()
                        .filter(|id| id.starts_with(&prefix))
                        .cloned()
                        .collect(),
                    self.assets
                        .footers
                        .keys()
                        .filter(|id| id.starts_with(&prefix))
                        .cloned()
                        .collect(),
                )
            }
        }
    }

    fn current_header_footer_usage(&self) -> HashMap<String, (HeaderFooterKind, Vec<usize>)> {
        match &self.format_state {
            PageFormatState::Docx { state, .. } => state.header_footer_usage.clone(),
            PageFormatState::Odt { .. } => HashMap::new(),
        }
    }

    fn load_images(&self, ids: HashSet<String>) -> Result<BTreeMap<String, ImageResource>> {
        let mut images = BTreeMap::new();
        if !self.config.extract_images {
            return Ok(images);
        }
        for id in ids {
            let (part, guessed_type) = self
                .assets
                .image_parts
                .get(&id)
                .cloned()
                .unwrap_or_else(|| (id.clone(), media_type(&id)));
            let data =
                read_part(self.package, &part)?.ok_or_else(|| Error::MissingImage(part.clone()))?;
            let (data, media_type) = prepare_image(data, guessed_type, &self.config)?;
            images.insert(
                id.clone(),
                ImageResource {
                    id,
                    part_path: part,
                    media_type,
                    data,
                },
            );
        }
        Ok(images)
    }
}

fn wrap_docx(body: &[u8]) -> Vec<u8> {
    let mut out = b"<w:document><w:body>".to_vec();
    out.extend_from_slice(body);
    out.extend_from_slice(b"</w:body></w:document>");
    out
}

fn wrap_odt(body: &[u8]) -> Vec<u8> {
    let mut out = b"<office:document-content><office:body><office:text>".to_vec();
    out.extend_from_slice(body);
    out.extend_from_slice(b"</office:text></office:body></office:document-content>");
    out
}

fn offset_sequences(elements: &mut [DocumentElement], offset: usize) {
    for element in elements {
        let provenance = match element {
            DocumentElement::Paragraph(v) => &mut v.provenance,
            DocumentElement::Heading(v) => &mut v.provenance,
            DocumentElement::Title(v) => &mut v.provenance,
            DocumentElement::Subtitle(v) => &mut v.provenance,
            DocumentElement::BlockQuote(v) => &mut v.provenance,
            DocumentElement::List(v) => &mut v.provenance,
            DocumentElement::Table(v) => &mut v.provenance,
            DocumentElement::Image(v) => &mut v.provenance,
            DocumentElement::Unknown(v) => &mut v.provenance,
        };
        provenance.sequence += offset;
    }
}

fn referenced_annotations(
    elements: &[DocumentElement],
) -> (HashSet<String>, HashSet<String>, HashSet<String>) {
    fn inlines(values: &[Inline], ids: &mut (HashSet<String>, HashSet<String>, HashSet<String>)) {
        for value in values {
            match value {
                Inline::CommentReference(id) => {
                    ids.0.insert(id.clone());
                }
                Inline::FootnoteReference(id) => {
                    ids.1.insert(id.clone());
                }
                Inline::EndnoteReference(id) => {
                    ids.2.insert(id.clone());
                }
                Inline::Hyperlink { content, .. } => inlines(content, ids),
                _ => {}
            }
        }
    }
    fn walk(
        elements: &[DocumentElement],
        ids: &mut (HashSet<String>, HashSet<String>, HashSet<String>),
    ) {
        for element in elements {
            match element {
                DocumentElement::Paragraph(v) => inlines(&v.content, ids),
                DocumentElement::Heading(v) => inlines(&v.content, ids),
                DocumentElement::Title(v) => inlines(&v.content, ids),
                DocumentElement::Subtitle(v) => inlines(&v.content, ids),
                DocumentElement::BlockQuote(v) => inlines(&v.content, ids),
                DocumentElement::Unknown(v) => inlines(&v.visible_content, ids),
                DocumentElement::List(v) => {
                    for item in &v.items {
                        walk(&item.blocks, ids);
                        for child in &item.children {
                            walk(&[DocumentElement::List(child.clone())], ids);
                        }
                    }
                }
                DocumentElement::Table(v) => {
                    for cell in v.rows.iter().flatten() {
                        walk(&cell.blocks, ids);
                    }
                }
                DocumentElement::Image(_) => {}
            }
        }
    }
    let mut ids = (HashSet::new(), HashSet::new(), HashSet::new());
    walk(elements, &mut ids);
    ids
}

fn filter_annotations(values: &[Annotation], ids: &HashSet<String>) -> Vec<Annotation> {
    values
        .iter()
        .filter(|annotation| ids.contains(&annotation.id))
        .cloned()
        .collect()
}

fn select_story(
    stories: &BTreeMap<String, Story>,
    page_number: usize,
    references: &[String],
    usage: &HashMap<String, (HeaderFooterKind, Vec<usize>)>,
    allow_first: bool,
    allow_even: bool,
) -> Option<Story> {
    let wanted = if page_number == 1 && allow_first {
        HeaderFooterKind::First
    } else if page_number.is_multiple_of(2) && allow_even {
        HeaderFooterKind::Even
    } else {
        HeaderFooterKind::Default
    };
    for kind in [wanted, HeaderFooterKind::Default] {
        if let Some(story) = references.iter().find_map(|id| {
            let story = stories.get(id)?;
            let actual = usage
                .get(id)
                .map(|value| value.0)
                .or(story.kind)
                .unwrap_or(HeaderFooterKind::Default);
            (actual == kind).then(|| story.clone())
        }) {
            return Some(story);
        }
    }
    stories
        .values()
        .find(|story| story.kind == Some(wanted))
        .cloned()
        .or_else(|| stories.values().next().cloned())
}

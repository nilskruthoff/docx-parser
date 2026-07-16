use std::borrow::Cow;
use std::collections::{BTreeMap, HashMap, HashSet};

use quick_xml::Reader;
use quick_xml::events::{BytesStart, Event};

use crate::model::*;
use crate::package::{list_parts, read_part};
use crate::{DocumentFormat, Error, ParserConfig, Result};

const OFFICE_REL: &str =
    "http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument";
const STRICT_OFFICE_REL: &str =
    "http://purl.oclc.org/ooxml/officeDocument/relationships/officeDocument";

#[derive(Debug, Clone)]
struct Relationship {
    target: String,
    rel_type: String,
    external: bool,
}

type Relationships = HashMap<String, Relationship>;

#[derive(Debug, Clone, Default)]
struct Style {
    based_on: Option<String>,
    name: Option<String>,
    outline: Option<u8>,
    num_id: Option<String>,
    num_level: Option<u8>,
    format: TextFormat,
    bold: Option<bool>,
    italic: Option<bool>,
    underline: Option<bool>,
    strikethrough: Option<bool>,
}

#[derive(Debug, Clone)]
struct NumberLevel {
    marker: ListMarker,
    start: u32,
}

#[derive(Debug, Clone, Default)]
struct OdtStyle {
    parent: Option<String>,
    format: TextFormat,
    markers: HashMap<u8, ListMarker>,
}

#[derive(Default)]
struct OdtParsed {
    elements: Vec<DocumentElement>,
    comments: Vec<Annotation>,
    footnotes: Vec<Annotation>,
    endnotes: Vec<Annotation>,
    sequence: usize,
}

#[derive(Default)]
struct OdtStories {
    sections: Vec<Section>,
    headers: BTreeMap<String, Story>,
    footers: BTreeMap<String, Story>,
}

#[derive(Default)]
struct DocxIndexes {
    relationships: HashMap<String, Relationships>,
    styles: HashMap<String, Style>,
    numbering: HashMap<(String, u8), NumberLevel>,
}

#[derive(Default)]
struct ParseState {
    sequence: usize,
    section: usize,
    image_parts: BTreeMap<String, (String, String)>,
    sections: Vec<Section>,
    pending_section: Option<Section>,
    header_footer_usage: HashMap<String, (HeaderFooterKind, Vec<usize>)>,
}

pub(crate) fn metadata(bytes: &[u8], format: DocumentFormat) -> Result<Metadata> {
    match format {
        DocumentFormat::Docx => parse_docx_metadata(bytes),
        DocumentFormat::Odt => parse_odt_metadata(bytes),
    }
}

pub(crate) fn parse_main(
    bytes: &[u8],
    format: DocumentFormat,
    config: &ParserConfig,
) -> Result<Vec<DocumentElement>> {
    match format {
        DocumentFormat::Docx => {
            let indexes = build_docx_indexes(bytes)?;
            let document_part = docx_document_part(bytes, &indexes)?;
            let xml = required_part(bytes, &document_part)?;
            let mut state = ParseState::default();
            parse_docx_story(
                &xml,
                &document_part,
                StoryType::Main,
                &indexes,
                &mut state,
                config,
            )
        }
        DocumentFormat::Odt => parse_odt_main(bytes, config),
    }
}

pub(crate) fn parse(
    bytes: &[u8],
    format: DocumentFormat,
    config: &ParserConfig,
) -> Result<Document> {
    match format {
        DocumentFormat::Docx => parse_docx(bytes, config),
        DocumentFormat::Odt => parse_odt(bytes, config),
    }
}

/// Parses a DOCX file into a `Document` structure, extracting its main content, metadata, and optional features such as
/// headers, footers, comments, footnotes, endnotes, and images.
///
/// # Arguments
/// - `bytes`: A slice of bytes representing the contents of the DOCX file.
/// - `config`: A reference to a `ParserConfig` object, specifying the parsing configuration options.
///
/// # Returns
/// A `Result` containing a `Document` on success, or an error if parsing fails.
///
/// # Process
/// 1. **Indexes Creation**:
///    - Builds the internal maps of all files present in the DOCX archive `build_docx_indexes`.
///
/// 2. **Main Content Parsing**:
///    - Locates the main document part and extracts the corresponding XML content.
///    - Parses the main story content (e.g., paragraphs, tables) from the XML.
///
/// 3. **Metadata Extraction**:
///    - Retrieves and parses document metadata such as title, author, creation date, keywords, and tags.
///
/// 4. **Headers and Footers**:
///    - Scans for header and footer parts in the DOCX file if `config.include_headers_footers` is `true`.
///    - Extracts and parses headers and footers into associated sections, including inherited header/footer usage info.
///
/// 5. **Annotations**:
///    - If enabled in the configuration, extracts annotations such as:
///      - Comments: Extracted from `word/comments.xml`.
///      - Footnotes: Extracted from `word/footnotes.xml`.
///      - Endnotes: Extracted from `word/endnotes.xml`.
///
/// 6. **Images**:
///    - If `config.extract_images` is `true`, identifies and extracts image resources.
///    - Processes image data for inclusion in the `Document` as `ImageResource` objects.
///
/// 7. **Sections**:
///    - Collects and finalizes sections of the document, as determined during parsing.
///
/// # Errors
///
/// - `Err(MissingPart)`: If missing critical parts of the DOCX file (e.g., `word/document.xml`).
/// - `Err(Xml)`: Malformed XML content in any significant part of the document.
/// - `Err(ImageConversion)`: Issues reading or preparing image data.
/// - `Err(IO)`: General I/O errors or invalid document structure.
fn parse_docx(bytes: &[u8], config: &ParserConfig) -> Result<Document> {
    let indexes = build_docx_indexes(bytes)?;
    let document_part = docx_document_part(bytes, &indexes)?;
    let xml = required_part(bytes, &document_part)?;
    let mut state = ParseState::default();
    let elements = parse_docx_story(
        &xml,
        &document_part,
        StoryType::Main,
        &indexes,
        &mut state,
        config,
    )?;
    let mut document = Document {
        elements,
        metadata: parse_docx_metadata(bytes)?,
        ..Document::default()
    };

    let story_parts: Vec<(String, StoryType, Option<HeaderFooterKind>)> = list_parts(bytes)?
        .into_iter()
        .filter_map(|part| {
            if part.starts_with("word/header") && part.ends_with(".xml") {
                Some((part, StoryType::Header, None))
            } else if part.starts_with("word/footer") && part.ends_with(".xml") {
                Some((part, StoryType::Footer, None))
            } else {
                None
            }
        })
        .collect();
    if config.include_headers_footers {
        for (part, kind, header_kind) in story_parts {
            let data = required_part(bytes, &part)?;
            let elements = parse_docx_story(&data, &part, kind, &indexes, &mut state, config)?;
            let (inherited_kind, used_by_sections) = state
                .header_footer_usage
                .get(&part)
                .cloned()
                .unwrap_or((HeaderFooterKind::Default, Vec::new()));
            let story = Story {
                kind: header_kind.or(Some(inherited_kind)),
                used_by_sections,
                elements,
            };
            if kind == StoryType::Header {
                document.headers.insert(part, story);
            } else {
                document.footers.insert(part, story);
            }
        }
    }
    if config.include_comments {
        document.comments = parse_docx_annotations(
            bytes,
            "word/comments.xml",
            "comment",
            StoryType::Comment,
            &indexes,
            &mut state,
            config,
        )?;
    }
    if config.include_footnotes {
        document.footnotes = parse_docx_annotations(
            bytes,
            "word/footnotes.xml",
            "footnote",
            StoryType::Footnote,
            &indexes,
            &mut state,
            config,
        )?;
    }
    if config.include_endnotes {
        document.endnotes = parse_docx_annotations(
            bytes,
            "word/endnotes.xml",
            "endnote",
            StoryType::Endnote,
            &indexes,
            &mut state,
            config,
        )?;
    }
    if config.extract_images {
        for (id, (part, media_type)) in state.image_parts {
            let data = read_part(bytes, &part)?.ok_or_else(|| Error::MissingImage(part.clone()))?;
            let (data, media_type) = prepare_image(data, media_type, config)?;
            document.images.insert(
                id.clone(),
                ImageResource {
                    id,
                    part_path: part,
                    media_type,
                    data,
                },
            );
        }
    }
    document.sections = std::mem::take(&mut state.sections);
    Ok(document)
}

fn docx_document_part(bytes: &[u8], indexes: &DocxIndexes) -> Result<String> {
    if let Some(root) = indexes.relationships.get("")
        && let Some(rel) = root.values().find(|rel| {
            rel.rel_type == OFFICE_REL
                || rel.rel_type == STRICT_OFFICE_REL
                || rel.rel_type.ends_with("/officeDocument")
        })
    {
        return Ok(rel.target.clone());
    }
    if read_part(bytes, "word/document.xml")?.is_some() {
        Ok("word/document.xml".into())
    } else {
        Err(Error::MissingPart("word/document.xml".into()))
    }
}

/// Builds the `DocxIndexes` structure by parsing a DOCX file's content.
///
/// This function processes the raw byte data of a DOCX file to extract and build
/// the necessary indexes, including relationships, styles, and numbering. It analyses
/// various parts of the document content to populate the `DocxIndexes` structure.
///
/// # Arguments
///
/// * `bytes` - A byte slice representing the raw content of the DOCX file.
///
/// # Returns
///
/// Returns a `Result` containing the populated `DocxIndexes` on success, or an error if
/// any processing step fails.
///
/// # Process
///
/// 1. Iterates through the list of parts in the DOCX file, parsing `.rels` relationships
///    to map relationship data into the `DocxIndexes.relationships`.
/// 2. Looks for the `word/styles.xml` part to parse styles information into `DocxIndexes.styles`.
/// 3. Looks for the `word/numbering.xml` part to parse numbering information into `DocxIndexes.numbering`.
/// # See Also
///
/// `DocxIndexes` - The structure being populated by this function.
fn build_docx_indexes(bytes: &[u8]) -> Result<DocxIndexes> {
    let mut indexes = DocxIndexes::default();
    for part in list_parts(bytes)? {
        if part.ends_with(".rels") {
            let owner = owner_for_rels(&part);
            let data = required_part(bytes, &part)?;
            indexes
                .relationships
                .insert(owner.clone(), parse_relationships(&data, &owner, &part)?);
        }
    }
    if let Some(data) = read_part(bytes, "word/styles.xml")? {
        indexes.styles = parse_styles(&data, "word/styles.xml")?;
    }
    if let Some(data) = read_part(bytes, "word/numbering.xml")? {
        indexes.numbering = parse_numbering(&data, "word/numbering.xml")?;
    }
    Ok(indexes)
}

fn owner_for_rels(path: &str) -> String {
    if path == "_rels/.rels" {
        return String::new();
    }
    let Some((dir, name)) = path.rsplit_once("/_rels/") else {
        return String::new();
    };
    let name = name.strip_suffix(".rels").unwrap_or(name);
    format!("{dir}/{name}")
}

fn parse_relationships(data: &[u8], owner: &str, xml_part: &str) -> Result<Relationships> {
    let mut reader = xml_reader(data);
    let mut buf = Vec::new();
    let mut out = HashMap::new();
    loop {
        match xml_event(&mut reader, &mut buf, xml_part)? {
            Event::Start(e) | Event::Empty(e) if local(e.name().as_ref()) == b"Relationship" => {
                let id = attr(&e, b"Id").unwrap_or_default();
                let target = attr(&e, b"Target").unwrap_or_default();
                let rel_type = attr(&e, b"Type").unwrap_or_default();
                let external =
                    attr(&e, b"TargetMode").is_some_and(|v| v.eq_ignore_ascii_case("external"));
                let target = if external {
                    target
                } else {
                    resolve_target(owner, &target)?
                };
                out.insert(
                    id,
                    Relationship {
                        target,
                        rel_type,
                        external,
                    },
                );
            }
            Event::Eof => break,
            _ => {}
        }
        buf.clear();
    }
    Ok(out)
}

fn resolve_target(owner: &str, target: &str) -> Result<String> {
    let mut components: Vec<&str> = owner
        .rsplit_once('/')
        .map_or(Vec::new(), |(d, _)| d.split('/').collect());
    let normalized = target.replace('\\', "/");
    if normalized.starts_with('/') {
        components.clear();
    }
    for component in normalized.trim_start_matches('/').split('/') {
        match component {
            "" | "." => {}
            ".." => {
                if components.pop().is_none() {
                    return Err(Error::InvalidRelationship {
                        owner: owner.into(),
                        target: target.into(),
                    });
                }
            }
            value => components.push(value),
        }
    }
    Ok(components.join("/"))
}

fn parse_styles(data: &[u8], part: &str) -> Result<HashMap<String, Style>> {
    let mut reader = xml_reader(data);
    let mut buf = Vec::new();
    let mut styles = HashMap::new();
    let mut current_id = None;
    let mut current = Style::default();
    loop {
        match xml_event(&mut reader, &mut buf, part)? {
            Event::Start(e) if matches!(local(e.name().as_ref()), b"style" | b"list-style") => {
                current_id = attr(&e, b"styleId");
                current = Style::default();
            }
            Event::End(e) if local(e.name().as_ref()) == b"style" => {
                if let Some(id) = current_id.take() {
                    styles.insert(id, current.clone());
                }
            }
            Event::Start(e) | Event::Empty(e) if current_id.is_some() => {
                match local(e.name().as_ref()) {
                    b"basedOn" => current.based_on = val(&e),
                    b"name" => current.name = val(&e),
                    b"outlineLvl" => current.outline = val(&e).and_then(|v| v.parse().ok()),
                    b"numId" => current.num_id = val(&e),
                    b"ilvl" => current.num_level = val(&e).and_then(|v| v.parse().ok()),
                    b"b" => current.bold = Some(bool_val(&e, true)),
                    b"i" => current.italic = Some(bool_val(&e, true)),
                    b"u" => current.underline = Some(val(&e).is_none_or(|v| v != "none")),
                    b"strike" => current.strikethrough = Some(bool_val(&e, true)),
                    b"vertAlign" => {
                        current.format.vertical_align = vertical(&val(&e).unwrap_or_default())
                    }
                    b"lang" => current.format.language = val(&e),
                    _ => {}
                }
            }
            Event::Eof => break,
            _ => {}
        }
        buf.clear();
    }
    Ok(styles)
}

fn resolved_style(styles: &HashMap<String, Style>, id: &str) -> Style {
    fn visit(styles: &HashMap<String, Style>, id: &str, seen: &mut HashSet<String>) -> Style {
        if !seen.insert(id.to_owned()) {
            return Style::default();
        }
        let Some(style) = styles.get(id) else {
            return Style::default();
        };
        let mut out = style
            .based_on
            .as_deref()
            .map_or_else(Style::default, |base| visit(styles, base, seen));
        if style.name.is_some() {
            out.name.clone_from(&style.name);
        }
        if style.outline.is_some() {
            out.outline = style.outline;
        }
        if style.num_id.is_some() {
            out.num_id.clone_from(&style.num_id);
        }
        if style.num_level.is_some() {
            out.num_level = style.num_level;
        }
        merge_format(&mut out.format, &style.format);
        if let Some(value) = style.bold {
            out.format.bold = value;
        }
        if let Some(value) = style.italic {
            out.format.italic = value;
        }
        if let Some(value) = style.underline {
            out.format.underline = value;
        }
        if let Some(value) = style.strikethrough {
            out.format.strikethrough = value;
        }
        out
    }
    visit(styles, id, &mut HashSet::new())
}

fn merge_format(target: &mut TextFormat, source: &TextFormat) {
    target.bold |= source.bold;
    target.italic |= source.italic;
    target.underline |= source.underline;
    target.strikethrough |= source.strikethrough;
    if source.vertical_align != VerticalAlign::Baseline {
        target.vertical_align = source.vertical_align;
    }
    if source.language.is_some() {
        target.language.clone_from(&source.language);
    }
}

fn parse_numbering(data: &[u8], part: &str) -> Result<HashMap<(String, u8), NumberLevel>> {
    let mut reader = xml_reader(data);
    let mut buf = Vec::new();
    let mut abstracts: HashMap<(String, u8), NumberLevel> = HashMap::new();
    let mut nums: HashMap<String, String> = HashMap::new();
    let mut overrides: HashMap<(String, u8), (Option<ListMarker>, Option<u32>)> = HashMap::new();
    let mut abstract_id = None;
    let mut num_id = None;
    let mut level = 0;
    let mut marker = ListMarker::Bullet;
    let mut start = 1;
    let mut override_level = None;
    let mut override_marker = None;
    let mut override_start = None;
    loop {
        match xml_event(&mut reader, &mut buf, part)? {
            Event::Start(e) if local(e.name().as_ref()) == b"abstractNum" => {
                abstract_id = attr(&e, b"abstractNumId")
            }
            Event::Start(e) if local(e.name().as_ref()) == b"num" => num_id = attr(&e, b"numId"),
            Event::Start(e) if local(e.name().as_ref()) == b"lvl" && abstract_id.is_some() => {
                level = attr(&e, b"ilvl").and_then(|v| v.parse().ok()).unwrap_or(0);
                marker = ListMarker::Bullet;
                start = 1;
            }
            Event::Start(e) if local(e.name().as_ref()) == b"lvlOverride" => {
                override_level = attr(&e, b"ilvl").and_then(|v| v.parse().ok());
                override_marker = None;
                override_start = None;
            }
            Event::Start(e) | Event::Empty(e) => match local(e.name().as_ref()) {
                b"abstractNumId" if num_id.is_some() => {
                    if let (Some(n), Some(a)) = (num_id.clone(), val(&e)) {
                        nums.insert(n, a);
                    }
                }
                b"numFmt" if abstract_id.is_some() => {
                    marker = list_marker(&val(&e).unwrap_or_default())
                }
                b"numFmt" if override_level.is_some() => {
                    override_marker = Some(list_marker(&val(&e).unwrap_or_default()))
                }
                b"start" if abstract_id.is_some() => {
                    start = val(&e).and_then(|v| v.parse().ok()).unwrap_or(1)
                }
                b"startOverride" if override_level.is_some() => {
                    override_start = val(&e).and_then(|v| v.parse().ok())
                }
                _ => {}
            },
            Event::End(e) if local(e.name().as_ref()) == b"lvl" => {
                if let Some(id) = abstract_id.clone() {
                    abstracts.insert((id, level), NumberLevel { marker, start });
                }
            }
            Event::End(e) if local(e.name().as_ref()) == b"abstractNum" => abstract_id = None,
            Event::End(e) if local(e.name().as_ref()) == b"lvlOverride" => {
                if let (Some(num), Some(level)) = (num_id.clone(), override_level.take()) {
                    overrides.insert(
                        (num, level),
                        (override_marker.take(), override_start.take()),
                    );
                }
            }
            Event::End(e) if local(e.name().as_ref()) == b"num" => num_id = None,
            Event::Eof => break,
            _ => {}
        }
        buf.clear();
    }
    let mut out = HashMap::new();
    for (num, abs) in nums {
        for ((abstract_key, lvl), value) in &abstracts {
            if *abstract_key == abs {
                out.insert((num.clone(), *lvl), value.clone());
            }
        }
    }
    for ((num, level), (marker, start)) in overrides {
        let value = out.entry((num, level)).or_insert(NumberLevel {
            marker: ListMarker::Bullet,
            start: 1,
        });
        if let Some(marker) = marker {
            value.marker = marker;
        }
        if let Some(start) = start {
            value.start = start;
        }
    }
    Ok(out)
}

fn list_marker(value: &str) -> ListMarker {
    match value {
        "decimal" => ListMarker::Decimal,
        "lowerLetter" => ListMarker::LowerLetter,
        "upperLetter" => ListMarker::UpperLetter,
        "lowerRoman" => ListMarker::LowerRoman,
        "upperRoman" => ListMarker::UpperRoman,
        _ => ListMarker::Bullet,
    }
}

#[derive(Default)]
struct ParagraphBuild {
    content: Vec<Inline>,
    style_id: Option<String>,
    num_id: Option<String>,
    num_level: Option<u8>,
    list_marker: Option<ListMarker>,
    list_start: u32,
    provenance: Provenance,
}

/// Parses a DOCX story part (like body, header, footer) and extracts its elements into a structured format.
///
/// This function reads XML data representing a specific part of a DOCX document (e.g., paragraphs, tables,
/// and section properties). It transforms the raw data into high-level `DocumentElement` objects that can
/// be further processed. The parsing is performed sequentially using an XML reader and distinguishes between
/// various elements such as paragraphs, tables, and section properties.
///
/// # Parameters
///
/// - `data`: A slice of bytes containing the XML data for the part of the DOCX document to parse.
/// - `part`: A string specifying the name or type of the part (e.g., "word/document.xml") being parsed.
/// - `story`: The type of story (e.g., body, header, footer) to parse, defined by the `StoryType` enum.
/// - `indexes`: A reference to a `DocxIndexes` structure, which contains indexing information used during parsing.
/// - `state`: A mutable reference to a `ParseState` structure that tracks the state of parsing globally.
/// - `config`: A reference to a `ParserConfig` structure that contains parsing configuration options.
///
/// # Notes
///
/// ### internal mapping of OOXML tags to `DocumentElements`
///
/// | OOXML | Gemeinsames Modell |
/// |---|---|
/// | `w:t` | `Inline::Text` |
/// | `w:br`, `w:cr` | `Inline::LineBreak` |
/// | `w:hyperlink` | `Inline::Hyperlink` |
/// | `w:bookmarkStart` | `Inline::Bookmark` |
/// | `w:commentReference` | `Inline::CommentReference` |
/// | `w:footnoteReference` | `Inline::FootnoteReference` |
/// | `w:endnoteReference` | `Inline::EndnoteReference` |
/// | `a:blip r:embed=...` | `Inline::Image` |
fn parse_docx_story(
    data: &[u8],
    part: &str,
    story: StoryType,
    indexes: &DocxIndexes,
    state: &mut ParseState,
    config: &ParserConfig,
) -> Result<Vec<DocumentElement>> {
    let mut reader = xml_reader(data);
    let mut buf = Vec::new();
    let mut elements = Vec::new();
    loop {
        match xml_event(&mut reader, &mut buf, part)? {
            Event::Start(e) if local(e.name().as_ref()) == b"p" => {
                let paragraph =
                    parse_docx_paragraph(&mut reader, part, story, indexes, state, config)?;
                push_paragraph(&mut elements, paragraph, indexes);
            }
            Event::Start(e) if local(e.name().as_ref()) == b"tbl" => {
                elements.push(parse_docx_table(
                    &mut reader,
                    part,
                    story,
                    indexes,
                    state,
                    config,
                )?);
            }
            Event::Start(e)
                if local(e.name().as_ref()) == b"sectPr" && story == StoryType::Main =>
            {
                parse_section_properties(&mut reader, part, indexes, state)?;
            }
            Event::Eof => break,
            _ => {}
        }
        buf.clear();
    }
    Ok(group_lists(elements))
}

fn parse_section_properties(
    reader: &mut Reader<&[u8]>,
    part: &str,
    indexes: &DocxIndexes,
    state: &mut ParseState,
) -> Result<()> {
    begin_section(state);
    let mut buf = Vec::new();
    loop {
        match xml_event(reader, &mut buf, part)? {
            Event::Start(e) | Event::Empty(e) if local(e.name().as_ref()) == b"headerReference" => {
                record_header_footer(&e, true, part, indexes, state);
            }
            Event::Start(e) | Event::Empty(e) if local(e.name().as_ref()) == b"footerReference" => {
                record_header_footer(&e, false, part, indexes, state);
            }
            Event::End(e) if local(e.name().as_ref()) == b"sectPr" => break,
            Event::Eof => break,
            _ => {}
        }
        buf.clear();
    }
    commit_section(state);
    Ok(())
}

fn parse_docx_paragraph(
    reader: &mut Reader<&[u8]>,
    part: &str,
    story: StoryType,
    indexes: &DocxIndexes,
    state: &mut ParseState,
    config: &ParserConfig,
) -> Result<ParagraphBuild> {
    let mut buf = Vec::new();
    let mut build = ParagraphBuild::default();
    let mut format = TextFormat::default();
    let mut text = String::new();
    let mut hyperlink: Option<(String, Vec<Inline>)> = None;
    let mut in_text = false;
    let mut in_run_properties = false;
    let mut alternate_stack: Vec<bool> = Vec::new();
    let mut skip_depth = 0usize;
    loop {
        match xml_event(reader, &mut buf, part)? {
            Event::Start(_) if skip_depth > 0 => skip_depth += 1,
            Event::Empty(_) if skip_depth > 0 => {}
            Event::End(_) if skip_depth > 0 => skip_depth -= 1,
            Event::Start(e) => match local(e.name().as_ref()) {
                b"AlternateContent" => alternate_stack.push(false),
                b"Choice" | b"Fallback" => {
                    if alternate_stack.last().copied().unwrap_or(false) {
                        skip_depth = 1;
                    } else if let Some(selected) = alternate_stack.last_mut() {
                        *selected = true;
                    }
                }
                b"sectPr" => begin_section(state),
                b"headerReference" => record_header_footer(&e, true, part, indexes, state),
                b"footerReference" => record_header_footer(&e, false, part, indexes, state),
                b"r" => {
                    flush_text(&mut text, &format, &mut build.content, &mut hyperlink);
                    format = TextFormat::default();
                }
                b"rPr" => in_run_properties = true,
                b"t" => in_text = true,
                b"hyperlink" => {
                    let target = attr(&e, b"id")
                        .and_then(|id| relationship(indexes, part, &id))
                        .map(|r| r.target.clone())
                        .or_else(|| attr(&e, b"anchor").map(|v| format!("#{v}")))
                        .unwrap_or_default();
                    hyperlink = Some((target, Vec::new()));
                }
                b"pStyle" => build.style_id = val(&e),
                b"numId" => build.num_id = val(&e),
                b"ilvl" => build.num_level = val(&e).and_then(|v| v.parse().ok()),
                b"rStyle" if in_run_properties => {
                    if let Some(id) = val(&e) {
                        let style = resolved_style(&indexes.styles, &id);
                        merge_format(&mut format, &style.format);
                        format.character_style = Some(id);
                    }
                }
                b"blip" if config.extract_images => {
                    if let Some(id) = attr(&e, b"embed") {
                        add_image_inline(
                            &id,
                            part,
                            indexes,
                            state,
                            &mut build.content,
                            &mut hyperlink,
                        );
                    }
                }
                _ if in_run_properties => apply_run_property(&mut format, &e),
                _ => {}
            },
            Event::Empty(e) => match local(e.name().as_ref()) {
                b"headerReference" => record_header_footer(&e, true, part, indexes, state),
                b"footerReference" => record_header_footer(&e, false, part, indexes, state),
                b"pStyle" => build.style_id = val(&e),
                b"numId" => build.num_id = val(&e),
                b"ilvl" => build.num_level = val(&e).and_then(|v| v.parse().ok()),
                b"br" | b"cr" => {
                    flush_text(&mut text, &format, &mut build.content, &mut hyperlink);
                    push_inline(Inline::LineBreak, &mut build.content, &mut hyperlink);
                }
                b"tab" => text.push('\t'),
                b"bookmarkStart" => {
                    if let Some(name) = attr(&e, b"name") {
                        push_inline(
                            Inline::Bookmark { name },
                            &mut build.content,
                            &mut hyperlink,
                        );
                    }
                }
                b"commentReference" => {
                    if let Some(id) = attr(&e, b"id") {
                        push_inline(
                            Inline::CommentReference(id),
                            &mut build.content,
                            &mut hyperlink,
                        );
                    }
                }
                b"footnoteReference" => {
                    if let Some(id) = attr(&e, b"id") {
                        push_inline(
                            Inline::FootnoteReference(id),
                            &mut build.content,
                            &mut hyperlink,
                        );
                    }
                }
                b"endnoteReference" => {
                    if let Some(id) = attr(&e, b"id") {
                        push_inline(
                            Inline::EndnoteReference(id),
                            &mut build.content,
                            &mut hyperlink,
                        );
                    }
                }
                b"blip" if config.extract_images => {
                    if let Some(id) = attr(&e, b"embed") {
                        add_image_inline(
                            &id,
                            part,
                            indexes,
                            state,
                            &mut build.content,
                            &mut hyperlink,
                        );
                    }
                }
                b"rStyle" if in_run_properties => {
                    if let Some(id) = val(&e) {
                        let style = resolved_style(&indexes.styles, &id);
                        merge_format(&mut format, &style.format);
                        format.character_style = Some(id);
                    }
                }
                _ if in_run_properties => apply_run_property(&mut format, &e),
                _ => {}
            },
            Event::Text(e) if in_text => {
                let decoded = e.decode().map_err(|error| {
                    Error::Conversion(format!("text decoding failed in {part}: {error}"))
                })?;
                let value = quick_xml::escape::unescape(&decoded)
                    .map(Cow::into_owned)
                    .unwrap_or_else(|_| decoded.into_owned());
                text.push_str(&value);
            }
            Event::End(e) => match local(e.name().as_ref()) {
                b"p" => {
                    flush_text(&mut text, &format, &mut build.content, &mut hyperlink);
                    break;
                }
                b"r" => flush_text(&mut text, &format, &mut build.content, &mut hyperlink),
                b"rPr" => in_run_properties = false,
                b"t" => in_text = false,
                b"hyperlink" => {
                    if let Some((target, content)) = hyperlink.take() {
                        build.content.push(Inline::Hyperlink { target, content });
                    }
                }
                b"sectPr" => commit_section(state),
                b"AlternateContent" => {
                    alternate_stack.pop();
                }
                _ => {}
            },
            Event::Eof => {
                return Err(Error::Conversion(format!(
                    "unexpected end of {part} inside paragraph"
                )));
            }
            _ => {}
        }
        buf.clear();
    }
    if let Some(style_id) = build.style_id.clone() {
        let style = resolved_style(&indexes.styles, &style_id);
        if build.num_id.is_none() {
            build.num_id = style.num_id;
        }
        if build.num_level.is_none() {
            build.num_level = style.num_level;
        }
        let inherit_format = !style
            .name
            .as_deref()
            .is_some_and(is_semantic_paragraph_style)
            && !is_semantic_paragraph_style(&style_id);
        for inline in &mut build.content {
            apply_paragraph_style(inline, &style.format, &style_id, inherit_format);
        }
    }
    if let Some(num_id) = &build.num_id {
        let level = build.num_level.unwrap_or(0);
        if let Some(numbering) = indexes.numbering.get(&(num_id.clone(), level)) {
            build.list_marker = Some(numbering.marker);
            build.list_start = numbering.start;
        } else {
            build.list_marker = Some(ListMarker::Bullet);
            build.list_start = 1;
        }
    }
    build.provenance = provenance(part, story, state, build.style_id.clone());
    state.sequence += 1;
    Ok(build)
}

fn apply_run_property(format: &mut TextFormat, e: &BytesStart<'_>) {
    match local(e.name().as_ref()) {
        b"b" => format.bold = bool_val(e, true),
        b"i" => format.italic = bool_val(e, true),
        b"u" => format.underline = val(e).is_none_or(|v| v != "none"),
        b"strike" | b"dstrike" => format.strikethrough = bool_val(e, true),
        b"vertAlign" => format.vertical_align = vertical(&val(e).unwrap_or_default()),
        b"lang" => format.language = val(e),
        _ => {}
    }
}

fn vertical(value: &str) -> VerticalAlign {
    match value {
        "superscript" => VerticalAlign::Superscript,
        "subscript" => VerticalAlign::Subscript,
        _ => VerticalAlign::Baseline,
    }
}

fn apply_paragraph_style(
    inline: &mut Inline,
    style: &TextFormat,
    style_id: &str,
    inherit_format: bool,
) {
    match inline {
        Inline::Text { format, .. } => {
            if inherit_format {
                merge_format(format, style);
            }
            format.paragraph_style = Some(style_id.into());
        }
        Inline::Hyperlink { content, .. } => {
            for child in content {
                apply_paragraph_style(child, style, style_id, inherit_format);
            }
        }
        _ => {}
    }
}

fn add_image_inline(
    relationship_id: &str,
    part: &str,
    indexes: &DocxIndexes,
    state: &mut ParseState,
    content: &mut Vec<Inline>,
    hyperlink: &mut Option<(String, Vec<Inline>)>,
) {
    let Some(rel) = relationship(indexes, part, relationship_id) else {
        return;
    };
    if rel.external {
        return;
    }
    let id = rel.target.clone();
    let media = media_type(&rel.target);
    state
        .image_parts
        .insert(id.clone(), (rel.target.clone(), media));
    push_inline(
        Inline::Image {
            resource_id: id,
            alt: String::new(),
        },
        content,
        hyperlink,
    );
}

fn relationship<'a>(indexes: &'a DocxIndexes, owner: &str, id: &str) -> Option<&'a Relationship> {
    indexes.relationships.get(owner)?.get(id)
}

fn begin_section(state: &mut ParseState) {
    if state.pending_section.is_some() {
        return;
    }
    let mut section = state.sections.last().cloned().unwrap_or(Section {
        index: state.sections.len(),
        header_references: Vec::new(),
        footer_references: Vec::new(),
    });
    section.index = state.sections.len();
    state.pending_section = Some(section);
}

fn record_header_footer(
    element: &BytesStart<'_>,
    header: bool,
    owner: &str,
    indexes: &DocxIndexes,
    state: &mut ParseState,
) {
    begin_section(state);
    let Some(id) = attr(element, b"id") else {
        return;
    };
    let Some(rel) = relationship(indexes, owner, &id) else {
        return;
    };
    if rel.external {
        return;
    }
    let kind = match attr(element, b"type").as_deref() {
        Some("first") => HeaderFooterKind::First,
        Some("even") => HeaderFooterKind::Even,
        _ => HeaderFooterKind::Default,
    };
    let section = state.pending_section.as_mut().expect("section initialized");
    let references = if header {
        &mut section.header_references
    } else {
        &mut section.footer_references
    };
    if !references.contains(&rel.target) {
        references.push(rel.target.clone());
    }
    state
        .header_footer_usage
        .entry(rel.target.clone())
        .or_insert_with(|| (kind, Vec::new()))
        .0 = kind;
}

fn commit_section(state: &mut ParseState) {
    let Some(section) = state.pending_section.take() else {
        return;
    };
    for part in section
        .header_references
        .iter()
        .chain(&section.footer_references)
    {
        let usage = &mut state
            .header_footer_usage
            .entry(part.clone())
            .or_insert_with(|| (HeaderFooterKind::Default, Vec::new()))
            .1;
        if !usage.contains(&section.index) {
            usage.push(section.index);
        }
    }
    state.section = section.index.saturating_add(1);
    state.sections.push(section);
}

fn flush_text(
    text: &mut String,
    format: &TextFormat,
    content: &mut Vec<Inline>,
    hyperlink: &mut Option<(String, Vec<Inline>)>,
) {
    if text.is_empty() {
        return;
    }
    let value = std::mem::take(text);
    let target = hyperlink.as_mut().map_or(content, |(_, values)| values);
    if let Some(Inline::Text {
        text,
        format: previous,
    }) = target.last_mut()
        && previous == format
    {
        text.push_str(&value);
        return;
    }
    target.push(Inline::Text {
        text: value,
        format: format.clone(),
    });
}

fn push_inline(
    value: Inline,
    content: &mut Vec<Inline>,
    hyperlink: &mut Option<(String, Vec<Inline>)>,
) {
    hyperlink
        .as_mut()
        .map_or(content, |(_, values)| values)
        .push(value);
}

fn provenance(
    part: &str,
    story: StoryType,
    state: &ParseState,
    style: Option<String>,
) -> Provenance {
    Provenance {
        source_part: part.into(),
        story,
        sequence: state.sequence,
        section: state.section,
        paragraph_style: style,
        character_styles: Vec::new(),
    }
}

fn push_paragraph(
    elements: &mut Vec<DocumentElement>,
    build: ParagraphBuild,
    indexes: &DocxIndexes,
) {
    let style = build.style_id.clone();
    let resolved = style
        .as_deref()
        .map(|id| resolved_style(&indexes.styles, id));
    let mut heading = resolved.as_ref().and_then(|s| {
        s.outline
            .map(|level| level.saturating_add(1).clamp(1, 6))
            .or_else(|| s.name.as_deref().and_then(heading_from_name))
    });
    if heading.is_none() {
        heading = style.as_deref().and_then(heading_from_name);
    }
    let prov = build.provenance;
    let resolved_name = resolved
        .as_ref()
        .and_then(|resolved| resolved.name.as_deref());
    let element = if resolved_name.is_some_and(is_title_name)
        || style.as_deref().is_some_and(is_title_name)
    {
        DocumentElement::Title(Title {
            content: build.content,
            provenance: prov,
        })
    } else if resolved_name.is_some_and(is_subtitle_name)
        || style.as_deref().is_some_and(is_subtitle_name)
    {
        DocumentElement::Subtitle(Subtitle {
            content: build.content,
            provenance: prov,
        })
    } else if let Some(level) = heading {
        DocumentElement::Heading(Heading {
            level,
            content: build.content,
            provenance: prov,
        })
    } else if resolved
        .as_ref()
        .and_then(|resolved| resolved.name.as_deref())
        .is_some_and(is_quote_name)
        || style.as_deref().is_some_and(is_quote_name)
    {
        DocumentElement::BlockQuote(BlockQuote {
            content: build.content,
            provenance: prov,
        })
    } else {
        let mut paragraph = Paragraph {
            content: build.content,
            provenance: prov,
        };
        if let Some(num_id) = build.num_id {
            paragraph
                .provenance
                .character_styles
                .push(format!("numId:{num_id}"));
            paragraph
                .provenance
                .character_styles
                .push(format!("ilvl:{}", build.num_level.unwrap_or(0)));
            paragraph.provenance.character_styles.push(format!(
                "marker:{:?}",
                build.list_marker.unwrap_or(ListMarker::Bullet)
            ));
            paragraph
                .provenance
                .character_styles
                .push(format!("start:{}", build.list_start.max(1)));
        }
        DocumentElement::Paragraph(paragraph)
    };
    elements.push(element);
}

fn heading_from_name(name: &str) -> Option<u8> {
    let lower = name.to_lowercase();
    ["heading", "überschrift", "titre", "título", "rubrik"]
        .iter()
        .find_map(|prefix| {
            lower
                .strip_prefix(prefix)
                .and_then(|tail| {
                    tail.trim_matches(|c: char| !c.is_ascii_digit())
                        .parse::<u8>()
                        .ok()
                })
                .map(|v| v.clamp(1, 6))
        })
}

fn is_quote_name(name: &str) -> bool {
    let normalized = normalize_style_name(name);
    matches!(
        normalized.as_str(),
        "quote"
            | "intense quote"
            | "quotation"
            | "intense quotation"
            | "zitat"
            | "intensives zitat"
    )
}

fn normalize_style_name(name: &str) -> String {
    name.replace("_20_", " ")
        .replace(['_', '-'], " ")
        .to_lowercase()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

fn is_title_name(name: &str) -> bool {
    matches!(
        normalize_style_name(name).as_str(),
        "title" | "titel" | "titre" | "título" | "titolo"
    )
}

fn is_subtitle_name(name: &str) -> bool {
    matches!(
        normalize_style_name(name).as_str(),
        "subtitle" | "sub title" | "untertitel" | "sous titre" | "subtítulo" | "sottotitolo"
    )
}

fn is_semantic_paragraph_style(name: &str) -> bool {
    is_quote_name(name) || is_title_name(name) || is_subtitle_name(name)
}

fn group_lists(elements: Vec<DocumentElement>) -> Vec<DocumentElement> {
    let mut out = Vec::new();
    for element in elements {
        let DocumentElement::Paragraph(mut paragraph) = element else {
            out.push(element);
            continue;
        };
        let num_id = paragraph
            .provenance
            .character_styles
            .iter()
            .find_map(|v| v.strip_prefix("numId:"))
            .map(str::to_owned);
        let Some(num_id) = num_id else {
            out.push(DocumentElement::Paragraph(paragraph));
            continue;
        };
        let level = paragraph
            .provenance
            .character_styles
            .iter()
            .find_map(|v| v.strip_prefix("ilvl:"))
            .and_then(|v| v.parse().ok())
            .unwrap_or(0);
        let marker = paragraph
            .provenance
            .character_styles
            .iter()
            .find_map(|v| v.strip_prefix("marker:"))
            .map(|v| match v {
                "Decimal" => ListMarker::Decimal,
                "LowerLetter" => ListMarker::LowerLetter,
                "UpperLetter" => ListMarker::UpperLetter,
                "LowerRoman" => ListMarker::LowerRoman,
                "UpperRoman" => ListMarker::UpperRoman,
                _ => ListMarker::Bullet,
            })
            .unwrap_or(ListMarker::Bullet);
        let start = paragraph
            .provenance
            .character_styles
            .iter()
            .find_map(|v| v.strip_prefix("start:"))
            .and_then(|v| v.parse().ok())
            .unwrap_or(1);
        paragraph.provenance.character_styles.retain(|v| {
            !v.starts_with("numId:")
                && !v.starts_with("ilvl:")
                && !v.starts_with("marker:")
                && !v.starts_with("start:")
        });
        let item = ListItem {
            blocks: vec![DocumentElement::Paragraph(paragraph.clone())],
            children: Vec::new(),
        };
        let mut prov = paragraph.provenance;
        prov.character_styles = vec![num_id];
        insert_list_item(&mut out, level, marker, start, item, prov);
    }
    out
}

fn insert_list_item(
    out: &mut Vec<DocumentElement>,
    level: u8,
    marker: ListMarker,
    start: u32,
    item: ListItem,
    provenance: Provenance,
) {
    if let Some(DocumentElement::List(list)) = out.last_mut() {
        let same_id = list.provenance.character_styles == provenance.character_styles;
        if list.level == level && same_id {
            list.items.push(item);
            return;
        }
        if list.level < level
            && same_id
            && let Some(parent) = list.items.last_mut()
        {
            insert_child_list(&mut parent.children, level, marker, start, item, provenance);
            return;
        }
    }
    out.push(DocumentElement::List(List {
        marker,
        level,
        start,
        items: vec![item],
        provenance,
    }));
}

fn insert_child_list(
    children: &mut Vec<List>,
    level: u8,
    marker: ListMarker,
    start: u32,
    item: ListItem,
    provenance: Provenance,
) {
    if let Some(last) = children.last_mut() {
        if last.level == level && last.provenance.character_styles == provenance.character_styles {
            last.items.push(item);
            return;
        }
        if last.level < level
            && last.provenance.character_styles == provenance.character_styles
            && let Some(parent) = last.items.last_mut()
        {
            insert_child_list(&mut parent.children, level, marker, start, item, provenance);
            return;
        }
    }
    children.push(List {
        marker,
        level,
        start,
        items: vec![item],
        provenance,
    });
}

fn parse_docx_table(
    reader: &mut Reader<&[u8]>,
    part: &str,
    story: StoryType,
    indexes: &DocxIndexes,
    state: &mut ParseState,
    config: &ParserConfig,
) -> Result<DocumentElement> {
    let table_prov = provenance(part, story, state, None);
    let mut buf = Vec::new();
    let mut rows = Vec::new();
    let mut row = Vec::new();
    loop {
        match xml_event(reader, &mut buf, part)? {
            Event::Start(e) if local(e.name().as_ref()) == b"tr" => row = Vec::new(),
            Event::End(e) if local(e.name().as_ref()) == b"tr" => {
                rows.push(std::mem::take(&mut row))
            }
            Event::Start(e) if local(e.name().as_ref()) == b"tc" => row.push(parse_docx_cell(
                reader, part, story, indexes, state, config,
            )?),
            Event::End(e) if local(e.name().as_ref()) == b"tbl" => break,
            Event::Eof => {
                return Err(Error::Conversion(format!(
                    "unexpected end of {part} inside table"
                )));
            }
            _ => {}
        }
        buf.clear();
    }
    Ok(DocumentElement::Table(Table {
        rows,
        provenance: table_prov,
    }))
}

fn parse_docx_cell(
    reader: &mut Reader<&[u8]>,
    part: &str,
    story: StoryType,
    indexes: &DocxIndexes,
    state: &mut ParseState,
    config: &ParserConfig,
) -> Result<TableCell> {
    let mut buf = Vec::new();
    let mut blocks = Vec::new();
    let mut column_span = 1;
    let mut row_span = 1;
    loop {
        match xml_event(reader, &mut buf, part)? {
            Event::Start(e) if local(e.name().as_ref()) == b"p" => {
                let p = parse_docx_paragraph(reader, part, story, indexes, state, config)?;
                push_paragraph(&mut blocks, p, indexes);
            }
            Event::Start(e) if local(e.name().as_ref()) == b"tbl" => blocks.push(parse_docx_table(
                reader, part, story, indexes, state, config,
            )?),
            Event::Start(e) | Event::Empty(e) if local(e.name().as_ref()) == b"gridSpan" => {
                column_span = val(&e).and_then(|v| v.parse().ok()).unwrap_or(1)
            }
            Event::Start(e) | Event::Empty(e) if local(e.name().as_ref()) == b"vMerge" => {
                row_span = if val(&e).as_deref() == Some("restart") {
                    1
                } else {
                    0
                }
            }
            Event::End(e) if local(e.name().as_ref()) == b"tc" => break,
            Event::Eof => {
                return Err(Error::Conversion(format!(
                    "unexpected end of {part} inside cell"
                )));
            }
            _ => {}
        }
        buf.clear();
    }
    Ok(TableCell {
        blocks: group_lists(blocks),
        row_span,
        column_span,
    })
}

fn parse_docx_annotations(
    bytes: &[u8],
    part: &str,
    element_name: &str,
    story: StoryType,
    indexes: &DocxIndexes,
    state: &mut ParseState,
    config: &ParserConfig,
) -> Result<Vec<Annotation>> {
    let Some(data) = read_part(bytes, part)? else {
        return Ok(Vec::new());
    };
    let mut reader = xml_reader(&data);
    let mut buf = Vec::new();
    let mut out = Vec::new();
    loop {
        match xml_event(&mut reader, &mut buf, part)? {
            Event::Start(e) if local(e.name().as_ref()) == element_name.as_bytes() => {
                let id = attr(&e, b"id").unwrap_or_default();
                let author = attr(&e, b"author");
                let date = attr(&e, b"date");
                let elements = parse_docx_until(
                    &mut reader,
                    part,
                    story,
                    element_name.as_bytes(),
                    indexes,
                    state,
                    config,
                )?;
                if !matches!(id.as_str(), "-1" | "0") || story == StoryType::Comment {
                    out.push(Annotation {
                        id,
                        author,
                        date,
                        elements,
                    });
                }
            }
            Event::Eof => break,
            _ => {}
        }
        buf.clear();
    }
    Ok(out)
}

fn parse_docx_until(
    reader: &mut Reader<&[u8]>,
    part: &str,
    story: StoryType,
    end: &[u8],
    indexes: &DocxIndexes,
    state: &mut ParseState,
    config: &ParserConfig,
) -> Result<Vec<DocumentElement>> {
    let mut buf = Vec::new();
    let mut elements = Vec::new();
    loop {
        match xml_event(reader, &mut buf, part)? {
            Event::Start(e) if local(e.name().as_ref()) == b"p" => {
                let p = parse_docx_paragraph(reader, part, story, indexes, state, config)?;
                push_paragraph(&mut elements, p, indexes);
            }
            Event::Start(e) if local(e.name().as_ref()) == b"tbl" => elements.push(
                parse_docx_table(reader, part, story, indexes, state, config)?,
            ),
            Event::End(e) if local(e.name().as_ref()) == end => break,
            Event::Eof => break,
            _ => {}
        }
        buf.clear();
    }
    Ok(group_lists(elements))
}

fn parse_docx_metadata(bytes: &[u8]) -> Result<Metadata> {
    let mut metadata = Metadata::default();
    if let Some(data) = read_part(bytes, "docProps/core.xml")? {
        read_metadata_xml(&data, "docProps/core.xml", &mut metadata)?;
    }
    if let Some(data) = read_part(bytes, "docProps/custom.xml")? {
        let values = simple_xml_values(&data, "docProps/custom.xml")?;
        for (key, value) in values {
            metadata.custom.insert(key, value);
        }
    }
    Ok(metadata)
}

fn read_metadata_xml(data: &[u8], part: &str, metadata: &mut Metadata) -> Result<()> {
    for (key, value) in simple_xml_values(data, part)? {
        match key.as_str() {
            "title" => metadata.title = Some(value),
            "subject" => metadata.subject = Some(value),
            "initial-creator" => metadata.author = Some(value),
            "creator" => {
                if metadata.author.is_none() {
                    metadata.author = Some(value.clone());
                }
                metadata.last_modified_by = Some(value);
            }
            "lastModifiedBy" => metadata.last_modified_by = Some(value),
            "description" => metadata.description = Some(value),
            "keywords" | "keyword" => metadata.keywords.extend(
                value
                    .split([',', ';'])
                    .map(str::trim)
                    .filter(|v| !v.is_empty())
                    .map(str::to_owned),
            ),
            "created" | "creation-date" => metadata.created_at = Some(value),
            "modified" | "date" => metadata.modified_at = Some(value),
            _ => {}
        }
    }
    Ok(())
}

fn simple_xml_values(data: &[u8], part: &str) -> Result<Vec<(String, String)>> {
    let mut reader = xml_reader(data);
    let mut buf = Vec::new();
    let mut current = None;
    let mut out = Vec::new();
    loop {
        match xml_event(&mut reader, &mut buf, part)? {
            Event::Start(e) => {
                current = Some(String::from_utf8_lossy(local(e.name().as_ref())).into_owned())
            }
            Event::Text(e) => {
                if let Some(name) = current.take() {
                    let decoded = e.decode().map_err(|error| {
                        Error::Conversion(format!("text decoding failed in {part}: {error}"))
                    })?;
                    let value = quick_xml::escape::unescape(&decoded)
                        .map(Cow::into_owned)
                        .unwrap_or_else(|_| decoded.into_owned());
                    if !value.trim().is_empty() {
                        out.push((name, value));
                    }
                }
            }
            Event::Eof => break,
            _ => {}
        }
        buf.clear();
    }
    Ok(out)
}

fn parse_odt_styles(bytes: &[u8], content: &[u8]) -> Result<HashMap<String, OdtStyle>> {
    let mut styles = HashMap::new();
    if let Some(data) = read_part(bytes, "styles.xml")? {
        scan_odt_styles(&data, "styles.xml", &mut styles)?;
    }
    scan_odt_styles(content, "content.xml", &mut styles)?;
    Ok(styles)
}

fn scan_odt_styles(data: &[u8], part: &str, styles: &mut HashMap<String, OdtStyle>) -> Result<()> {
    let mut reader = xml_reader(data);
    let mut buf = Vec::new();
    let mut name = None;
    let mut style = OdtStyle::default();
    loop {
        match xml_event(&mut reader, &mut buf, part)? {
            Event::Start(e) if matches!(local(e.name().as_ref()), b"style" | b"list-style") => {
                name = attr(&e, b"name");
                style = OdtStyle {
                    parent: attr(&e, b"parent-style-name"),
                    ..OdtStyle::default()
                };
            }
            Event::Start(e) | Event::Empty(e)
                if name.is_some() && local(e.name().as_ref()) == b"text-properties" =>
            {
                style.format.bold = attr(&e, b"font-weight").as_deref() == Some("bold");
                style.format.italic = attr(&e, b"font-style").as_deref() == Some("italic");
                style.format.underline =
                    attr(&e, b"text-underline-style").is_some_and(|value| value != "none");
                style.format.strikethrough =
                    attr(&e, b"text-line-through-style").is_some_and(|value| value != "none");
                style.format.language = attr(&e, b"language");
                if let Some(position) = attr(&e, b"text-position") {
                    style.format.vertical_align = if position.starts_with("super") {
                        VerticalAlign::Superscript
                    } else if position.starts_with("sub") {
                        VerticalAlign::Subscript
                    } else {
                        VerticalAlign::Baseline
                    };
                }
            }
            Event::Start(e) | Event::Empty(e)
                if name.is_some() && local(e.name().as_ref()) == b"list-level-style-number" =>
            {
                let level = attr(&e, b"level")
                    .and_then(|value| value.parse::<u8>().ok())
                    .unwrap_or(1)
                    .saturating_sub(1);
                let marker = match attr(&e, b"num-format").as_deref() {
                    Some("a") => ListMarker::LowerLetter,
                    Some("A") => ListMarker::UpperLetter,
                    Some("i") => ListMarker::LowerRoman,
                    Some("I") => ListMarker::UpperRoman,
                    _ => ListMarker::Decimal,
                };
                style.markers.insert(level, marker);
            }
            Event::Start(e) | Event::Empty(e)
                if name.is_some() && local(e.name().as_ref()) == b"list-level-style-bullet" =>
            {
                let level = attr(&e, b"level")
                    .and_then(|value| value.parse::<u8>().ok())
                    .unwrap_or(1)
                    .saturating_sub(1);
                style.markers.insert(level, ListMarker::Bullet);
            }
            Event::End(e) if matches!(local(e.name().as_ref()), b"style" | b"list-style") => {
                if let Some(name) = name.take() {
                    styles.insert(name, style.clone());
                }
            }
            Event::Eof => break,
            _ => {}
        }
        buf.clear();
    }
    Ok(())
}

fn resolved_odt_style(styles: &HashMap<String, OdtStyle>, name: &str) -> TextFormat {
    fn visit(
        styles: &HashMap<String, OdtStyle>,
        name: &str,
        seen: &mut HashSet<String>,
    ) -> TextFormat {
        if !seen.insert(name.to_owned()) {
            return TextFormat::default();
        }
        let Some(style) = styles.get(name) else {
            return TextFormat::default();
        };
        let mut format = style
            .parent
            .as_deref()
            .map_or_else(TextFormat::default, |parent| visit(styles, parent, seen));
        merge_format(&mut format, &style.format);
        format
    }
    visit(styles, name, &mut HashSet::new())
}

fn is_odt_style(
    styles: &HashMap<String, OdtStyle>,
    name: &str,
    predicate: fn(&str) -> bool,
) -> bool {
    fn visit(
        styles: &HashMap<String, OdtStyle>,
        name: &str,
        predicate: fn(&str) -> bool,
        seen: &mut HashSet<String>,
    ) -> bool {
        if !seen.insert(name.to_owned()) {
            return false;
        }
        if predicate(name) {
            return true;
        }
        styles
            .get(name)
            .and_then(|style| style.parent.as_deref())
            .is_some_and(|parent| visit(styles, parent, predicate, seen))
    }
    visit(styles, name, predicate, &mut HashSet::new())
}

fn is_odt_quote_style(styles: &HashMap<String, OdtStyle>, name: &str) -> bool {
    is_odt_style(styles, name, is_quote_name)
}

fn is_odt_title_style(styles: &HashMap<String, OdtStyle>, name: &str) -> bool {
    is_odt_style(styles, name, is_title_name)
}

fn is_odt_subtitle_style(styles: &HashMap<String, OdtStyle>, name: &str) -> bool {
    is_odt_style(styles, name, is_subtitle_name)
}

/// Parses an OpenDocument Text (ODT) file into a `Document` structure, extracting its main content, metadata, and optional features such as
/// headers, footers, comments, footnotes, endnotes, and images.
///
/// # Arguments
/// - `bytes`: A slice of bytes representing the contents of the DOCX file.
/// - `config`: A reference to a `ParserConfig` object, specifying the parsing configuration options.
///
/// # Returns
/// A `Result` containing a `Document` on success, or an error if parsing fails.
///
/// # Returns
///
/// * `Ok(Document)` - A document object containing parsed elements, metadata, and optionally
///   extracted images if `config.extract_images` is set to `true`.
/// * `Err` - Returns an error if the parsing process fails, such as invalid file format or
///   missing required parts.
///
/// # Process
///
/// 1. Calls `parse_odt_main` to parse the main content and produce elements.
/// 2. Collects any image IDs referenced in the document using `collect_image_ids`.
/// 3. Creates a `Document` object populated with parsed elements and metadata.
/// 4. If `config.extract_images` is enabled:
///     - Extracts the image data for each referenced image part.
///     - Prepares the image data and determines its media type.
///     - Stores the image in the `Document` object wrapped in an `ImageResource` structure.
///
/// # Errors
///
/// - `Err(Xml)`: Failure to parse the main structure of the ODT file via `parse_odt_main`.
/// - `Err()`: Failure to extract metadata using `parse_odt_metadata`.
/// - `Err(MissingImage)`: Missing or inaccessible required parts for referenced images.
/// - `Err(ImageConversion)`: Errors during image preparation (e.g., unsupported media type).
fn parse_odt(bytes: &[u8], config: &ParserConfig) -> Result<Document> {
    let data = required_part(bytes, "content.xml")?;
    let styles = parse_odt_styles(bytes, &data)?;
    let parsed = parse_odt_content(&data, &styles, config)?;
    let mut referenced_images = HashSet::new();
    collect_image_ids(&parsed.elements, &mut referenced_images);
    let stories = parse_odt_master_pages(bytes, &styles, config)?;
    let mut document = Document {
        elements: parsed.elements,
        sections: stories.sections,
        metadata: parse_odt_metadata(bytes)?,
        headers: stories.headers,
        footers: stories.footers,
        comments: parsed.comments,
        footnotes: parsed.footnotes,
        endnotes: parsed.endnotes,
        ..Document::default()
    };
    if config.extract_images {
        for part in referenced_images {
            let data = required_part(bytes, &part)?;
            let (data, media_type) = prepare_image(data, media_type(&part), config)?;
            document.images.insert(
                part.clone(),
                ImageResource {
                    id: part.clone(),
                    part_path: part.clone(),
                    media_type,
                    data,
                },
            );
        }
    }
    Ok(document)
}

fn prepare_image(
    data: Vec<u8>,
    media_type: String,
    config: &ParserConfig,
) -> Result<(Vec<u8>, String)> {
    if !config.compress_images || media_type == "image/svg+xml" {
        return Ok((data, media_type));
    }
    let decoded = image::load_from_memory(&data)
        .map_err(|error| Error::ImageConversion(error.to_string()))?;
    let mut compressed = Vec::new();
    image::codecs::jpeg::JpegEncoder::new_with_quality(&mut compressed, config.quality)
        .encode_image(&decoded)
        .map_err(|error| Error::ImageConversion(error.to_string()))?;
    Ok((compressed, "image/jpeg".into()))
}

fn collect_image_ids(elements: &[DocumentElement], out: &mut HashSet<String>) {
    fn inline(values: &[Inline], out: &mut HashSet<String>) {
        for value in values {
            match value {
                Inline::Image { resource_id, .. } => {
                    out.insert(resource_id.clone());
                }
                Inline::Hyperlink { content, .. } => inline(content, out),
                _ => {}
            }
        }
    }
    for element in elements {
        match element {
            DocumentElement::Paragraph(value) => inline(&value.content, out),
            DocumentElement::Heading(value) => inline(&value.content, out),
            DocumentElement::Title(value) => inline(&value.content, out),
            DocumentElement::Subtitle(value) => inline(&value.content, out),
            DocumentElement::BlockQuote(value) => inline(&value.content, out),
            DocumentElement::Image(value) => {
                out.insert(value.resource_id.clone());
            }
            DocumentElement::Unknown(value) => inline(&value.visible_content, out),
            DocumentElement::List(value) => {
                for item in &value.items {
                    collect_image_ids(&item.blocks, out);
                    for child in &item.children {
                        collect_image_ids(&[DocumentElement::List(child.clone())], out);
                    }
                }
            }
            DocumentElement::Table(value) => {
                for row in &value.rows {
                    for cell in row {
                        collect_image_ids(&cell.blocks, out);
                    }
                }
            }
        }
    }
}

/// Parses the main content of an OpenDocument Text (ODT) file into a structured
/// vector of `DocumentElement`s, such as paragraphs, headings, lists, and tables.
///
/// # Parameters
/// - `bytes`: A byte slice representing the entire ODT file.
/// - `_config`: A reference to the `ParserConfig`, which may allow for
///   customization of the parsing process (unused in this function).
///
/// # Notes
///
/// ### internal mapping of OOXML tags to `DocumentElements`
///
/// | ODT | Gemeinsames Modell |
/// |---|---|
/// | `text:p` | `Paragraph` |
/// | `text:h` | `Heading` |
/// | `text:list` | nested `List` |
/// | `table:table` | `Table` |
/// | `text:a` | `Inline::Hyperlink` |
/// | `text:span` | formatierter Text |
/// | `text:line-break` | `Inline::LineBreak` |
/// | `draw:image` | `Inline::Image` |
fn parse_odt_main(bytes: &[u8], config: &ParserConfig) -> Result<Vec<DocumentElement>> {
    let data = required_part(bytes, "content.xml")?;
    let styles = parse_odt_styles(bytes, &data)?;
    Ok(parse_odt_content(&data, &styles, config)?.elements)
}

fn parse_odt_content(
    data: &[u8],
    styles: &HashMap<String, OdtStyle>,
    config: &ParserConfig,
) -> Result<OdtParsed> {
    let mut reader = xml_reader(data);
    let mut buf = Vec::new();
    let mut parsed = OdtParsed::default();
    loop {
        match xml_event(&mut reader, &mut buf, "content.xml")? {
            Event::Start(e) if matches!(local(e.name().as_ref()), b"p" | b"h") => {
                let tag = local(e.name().as_ref()).to_vec();
                let level = attr(&e, b"outline-level")
                    .and_then(|value| value.parse::<u8>().ok())
                    .unwrap_or(1)
                    .clamp(1, 6);
                let style_name = attr(&e, b"style-name");
                let base_format = odt_base_format(styles, style_name.as_deref());
                let content = parse_odt_inline(
                    &mut reader,
                    &tag,
                    &base_format,
                    styles,
                    config,
                    &mut parsed,
                    "content.xml",
                )?;
                let provenance = odt_provenance(
                    &mut parsed,
                    StoryType::Main,
                    style_name.clone(),
                    "content.xml",
                );
                parsed.elements.push(odt_text_element(
                    &tag,
                    level,
                    style_name.as_deref(),
                    content,
                    provenance,
                    styles,
                ));
            }
            Event::Start(e) if local(e.name().as_ref()) == b"list" => {
                let style_name = attr(&e, b"style-name");
                let list =
                    parse_odt_list(&mut reader, 0, 0, style_name, styles, config, &mut parsed)?;
                parsed.elements.push(DocumentElement::List(list));
            }
            Event::Start(e) if local(e.name().as_ref()) == b"table" => {
                let table = parse_odt_table(&mut reader, styles, config, &mut parsed)?;
                parsed.elements.push(DocumentElement::Table(table));
            }
            Event::Eof => break,
            _ => {}
        }
        buf.clear();
    }
    Ok(parsed)
}

fn odt_base_format(styles: &HashMap<String, OdtStyle>, style_name: Option<&str>) -> TextFormat {
    if style_name.is_some_and(|name| {
        is_odt_quote_style(styles, name)
            || is_odt_title_style(styles, name)
            || is_odt_subtitle_style(styles, name)
    }) {
        TextFormat::default()
    } else {
        style_name
            .map(|name| resolved_odt_style(styles, name))
            .unwrap_or_default()
    }
}

fn odt_provenance(
    parsed: &mut OdtParsed,
    story: StoryType,
    paragraph_style: Option<String>,
    source_part: &str,
) -> Provenance {
    let provenance = Provenance {
        source_part: source_part.into(),
        story,
        sequence: parsed.sequence,
        paragraph_style,
        ..Provenance::default()
    };
    parsed.sequence += 1;
    provenance
}

fn odt_text_element(
    tag: &[u8],
    level: u8,
    style_name: Option<&str>,
    content: Vec<Inline>,
    provenance: Provenance,
    styles: &HashMap<String, OdtStyle>,
) -> DocumentElement {
    if style_name.is_some_and(|name| is_odt_title_style(styles, name)) {
        DocumentElement::Title(Title {
            content,
            provenance,
        })
    } else if style_name.is_some_and(|name| is_odt_subtitle_style(styles, name)) {
        DocumentElement::Subtitle(Subtitle {
            content,
            provenance,
        })
    } else if tag == b"h" {
        DocumentElement::Heading(Heading {
            level,
            content,
            provenance,
        })
    } else if style_name.is_some_and(|name| is_odt_quote_style(styles, name)) {
        DocumentElement::BlockQuote(BlockQuote {
            content,
            provenance,
        })
    } else {
        DocumentElement::Paragraph(Paragraph {
            content,
            provenance,
        })
    }
}

fn odt_list_marker(
    styles: &HashMap<String, OdtStyle>,
    style_name: Option<&str>,
    level: u8,
    fallback: ListMarker,
) -> ListMarker {
    style_name
        .and_then(|name| styles.get(name))
        .and_then(|style| style.markers.get(&level))
        .copied()
        .unwrap_or(fallback)
}

fn parse_odt_list(
    reader: &mut Reader<&[u8]>,
    level: u8,
    style_level: u8,
    style_name: Option<String>,
    styles: &HashMap<String, OdtStyle>,
    config: &ParserConfig,
    parsed: &mut OdtParsed,
) -> Result<List> {
    let list_sequence = parsed.sequence;
    let marker = odt_list_marker(
        styles,
        style_name.as_deref(),
        style_level,
        ListMarker::Bullet,
    );
    let mut buf = Vec::new();
    let mut items = Vec::new();
    let mut blocks = Vec::new();
    let mut children = Vec::new();
    let mut in_item = false;
    loop {
        match xml_event(reader, &mut buf, "content.xml")? {
            Event::Start(e) if local(e.name().as_ref()) == b"list-item" => {
                in_item = true;
                blocks.clear();
                children.clear();
            }
            Event::Start(e) if in_item && matches!(local(e.name().as_ref()), b"p" | b"h") => {
                let tag = local(e.name().as_ref()).to_vec();
                let heading_level = attr(&e, b"outline-level")
                    .and_then(|v| v.parse::<u8>().ok())
                    .unwrap_or(1)
                    .clamp(1, 6);
                let paragraph_style = attr(&e, b"style-name");
                let base_format = odt_base_format(styles, paragraph_style.as_deref());
                let content = parse_odt_inline(
                    reader,
                    &tag,
                    &base_format,
                    styles,
                    config,
                    parsed,
                    "content.xml",
                )?;
                let provenance = odt_provenance(
                    parsed,
                    StoryType::Main,
                    paragraph_style.clone(),
                    "content.xml",
                );
                blocks.push(odt_text_element(
                    &tag,
                    heading_level,
                    paragraph_style.as_deref(),
                    content,
                    provenance,
                    styles,
                ));
            }
            Event::Start(e) if in_item && local(e.name().as_ref()) == b"list" => {
                let explicit_style = attr(&e, b"style-name");
                let child_style_level = if explicit_style.is_some() {
                    0
                } else {
                    style_level.saturating_add(1)
                };
                let child_style = explicit_style.or_else(|| style_name.clone());
                children.push(parse_odt_list(
                    reader,
                    level.saturating_add(1),
                    child_style_level,
                    child_style,
                    styles,
                    config,
                    parsed,
                )?);
            }
            Event::End(e) if local(e.name().as_ref()) == b"list-item" => {
                items.push(ListItem {
                    blocks: std::mem::take(&mut blocks),
                    children: std::mem::take(&mut children),
                });
                in_item = false;
            }
            Event::End(e) if local(e.name().as_ref()) == b"list" => break,
            Event::Eof => break,
            _ => {}
        }
        buf.clear();
    }
    Ok(List {
        marker,
        level,
        start: 1,
        items,
        provenance: Provenance {
            source_part: "content.xml".into(),
            sequence: list_sequence,
            ..Provenance::default()
        },
    })
}

fn parse_odt_table(
    reader: &mut Reader<&[u8]>,
    styles: &HashMap<String, OdtStyle>,
    config: &ParserConfig,
    parsed: &mut OdtParsed,
) -> Result<Table> {
    let table_sequence = parsed.sequence;
    let mut buf = Vec::new();
    let mut rows = Vec::new();
    let mut row = Vec::new();
    loop {
        match xml_event(reader, &mut buf, "content.xml")? {
            Event::Start(e) if local(e.name().as_ref()) == b"table-row" => row.clear(),
            Event::Start(e) if local(e.name().as_ref()) == b"table-cell" => {
                let column_span = attr(&e, b"number-columns-spanned")
                    .and_then(|v| v.parse().ok())
                    .unwrap_or(1);
                let row_span = attr(&e, b"number-rows-spanned")
                    .and_then(|v| v.parse().ok())
                    .unwrap_or(1);
                row.push(parse_odt_cell(
                    reader,
                    row_span,
                    column_span,
                    styles,
                    config,
                    parsed,
                )?);
            }
            Event::End(e) if local(e.name().as_ref()) == b"table-row" => {
                rows.push(std::mem::take(&mut row));
            }
            Event::End(e) if local(e.name().as_ref()) == b"table" => break,
            Event::Eof => break,
            _ => {}
        }
        buf.clear();
    }
    Ok(Table {
        rows,
        provenance: Provenance {
            source_part: "content.xml".into(),
            sequence: table_sequence,
            ..Provenance::default()
        },
    })
}

fn parse_odt_cell(
    reader: &mut Reader<&[u8]>,
    row_span: usize,
    column_span: usize,
    styles: &HashMap<String, OdtStyle>,
    config: &ParserConfig,
    parsed: &mut OdtParsed,
) -> Result<TableCell> {
    let mut buf = Vec::new();
    let mut blocks = Vec::new();
    loop {
        match xml_event(reader, &mut buf, "content.xml")? {
            Event::Start(e) if matches!(local(e.name().as_ref()), b"p" | b"h") => {
                let tag = local(e.name().as_ref()).to_vec();
                let level = attr(&e, b"outline-level")
                    .and_then(|v| v.parse::<u8>().ok())
                    .unwrap_or(1)
                    .clamp(1, 6);
                let paragraph_style = attr(&e, b"style-name");
                let base_format = odt_base_format(styles, paragraph_style.as_deref());
                let content = parse_odt_inline(
                    reader,
                    &tag,
                    &base_format,
                    styles,
                    config,
                    parsed,
                    "content.xml",
                )?;
                let provenance = odt_provenance(
                    parsed,
                    StoryType::Main,
                    paragraph_style.clone(),
                    "content.xml",
                );
                blocks.push(odt_text_element(
                    &tag,
                    level,
                    paragraph_style.as_deref(),
                    content,
                    provenance,
                    styles,
                ));
            }
            Event::Start(e) if local(e.name().as_ref()) == b"list" => {
                let style_name = attr(&e, b"style-name");
                blocks.push(DocumentElement::List(parse_odt_list(
                    reader, 0, 0, style_name, styles, config, parsed,
                )?));
            }
            Event::End(e) if local(e.name().as_ref()) == b"table-cell" => break,
            Event::Eof => break,
            _ => {}
        }
        buf.clear();
    }
    Ok(TableCell {
        blocks,
        row_span,
        column_span,
    })
}

fn parse_odt_inline(
    reader: &mut Reader<&[u8]>,
    end: &[u8],
    base_format: &TextFormat,
    styles: &HashMap<String, OdtStyle>,
    config: &ParserConfig,
    parsed: &mut OdtParsed,
    source_part: &str,
) -> Result<Vec<Inline>> {
    let mut buf = Vec::new();
    let mut out = Vec::new();
    let mut format = base_format.clone();
    let mut format_stack = Vec::new();
    let mut text = String::new();
    let mut hyperlink: Option<(String, Vec<Inline>)> = None;
    loop {
        match xml_event(reader, &mut buf, "content.xml")? {
            Event::Start(e) if local(e.name().as_ref()) == b"a" => {
                flush_odt_text(&mut text, &format, &mut out, &mut hyperlink);
                hyperlink = Some((attr(&e, b"href").unwrap_or_default(), Vec::new()));
            }
            Event::Start(e) if local(e.name().as_ref()) == b"span" => {
                flush_odt_text(&mut text, &format, &mut out, &mut hyperlink);
                format_stack.push(format.clone());
                if let Some(name) = attr(&e, b"style-name") {
                    merge_format(&mut format, &resolved_odt_style(styles, &name));
                    format.character_style = Some(name);
                }
            }
            Event::Start(e) if local(e.name().as_ref()) == b"annotation" => {
                flush_odt_text(&mut text, &format, &mut out, &mut hyperlink);
                if config.include_comments {
                    let id = (parsed.comments.len() + 1).to_string();
                    let comment = parse_odt_annotation(
                        reader,
                        id.clone(),
                        styles,
                        config,
                        parsed,
                        source_part,
                    )?;
                    push_inline(Inline::CommentReference(id), &mut out, &mut hyperlink);
                    parsed.comments.push(comment);
                } else {
                    skip_xml_element(reader, b"annotation", source_part)?;
                }
            }
            Event::Start(e) if local(e.name().as_ref()) == b"note" => {
                flush_odt_text(&mut text, &format, &mut out, &mut hyperlink);
                let class = attr(&e, b"note-class").unwrap_or_else(|| "footnote".into());
                let enabled = if class == "endnote" {
                    config.include_endnotes
                } else {
                    config.include_footnotes
                };
                if enabled {
                    let note = parse_odt_note(reader, &class, styles, config, parsed, source_part)?;
                    let id = note.id.clone();
                    let reference = if class == "endnote" {
                        parsed.endnotes.push(note);
                        Inline::EndnoteReference(id)
                    } else {
                        parsed.footnotes.push(note);
                        Inline::FootnoteReference(id)
                    };
                    push_inline(reference, &mut out, &mut hyperlink);
                } else {
                    skip_xml_element(reader, b"note", source_part)?;
                }
            }
            Event::Text(e) => {
                let decoded = e.decode().map_err(|error| {
                    Error::Conversion(format!("text decoding failed in content.xml: {error}"))
                })?;
                let value = quick_xml::escape::unescape(&decoded)
                    .map(Cow::into_owned)
                    .unwrap_or_else(|_| decoded.into_owned());
                text.push_str(&value);
            }
            Event::Empty(e) if local(e.name().as_ref()) == b"line-break" => {
                flush_odt_text(&mut text, &format, &mut out, &mut hyperlink);
                push_inline(Inline::LineBreak, &mut out, &mut hyperlink);
            }
            Event::Empty(e) if local(e.name().as_ref()) == b"tab" => text.push('\t'),
            Event::Empty(e) if local(e.name().as_ref()) == b"s" => {
                let count = attr(&e, b"c").and_then(|v| v.parse().ok()).unwrap_or(1);
                text.extend(std::iter::repeat_n(' ', count));
            }
            Event::Empty(e) if local(e.name().as_ref()) == b"image" => {
                if let Some(href) = attr(&e, b"href") {
                    flush_odt_text(&mut text, &format, &mut out, &mut hyperlink);
                    push_inline(
                        Inline::Image {
                            resource_id: href.trim_start_matches("./").into(),
                            alt: String::new(),
                        },
                        &mut out,
                        &mut hyperlink,
                    );
                }
            }
            Event::Start(e) if local(e.name().as_ref()) == b"image" => {
                if let Some(href) = attr(&e, b"href") {
                    flush_odt_text(&mut text, &format, &mut out, &mut hyperlink);
                    push_inline(
                        Inline::Image {
                            resource_id: href.trim_start_matches("./").into(),
                            alt: String::new(),
                        },
                        &mut out,
                        &mut hyperlink,
                    );
                }
            }
            Event::End(e) if local(e.name().as_ref()) == b"a" => {
                flush_odt_text(&mut text, &format, &mut out, &mut hyperlink);
                if let Some((target, content)) = hyperlink.take() {
                    out.push(Inline::Hyperlink { target, content });
                }
            }
            Event::End(e) if local(e.name().as_ref()) == b"span" => {
                flush_odt_text(&mut text, &format, &mut out, &mut hyperlink);
                format = format_stack.pop().unwrap_or_else(|| base_format.clone());
            }
            Event::End(e) if local(e.name().as_ref()) == end => break,
            Event::Eof => break,
            _ => {}
        }
        buf.clear();
    }
    flush_odt_text(&mut text, &format, &mut out, &mut hyperlink);
    Ok(out)
}

fn parse_odt_annotation(
    reader: &mut Reader<&[u8]>,
    id: String,
    styles: &HashMap<String, OdtStyle>,
    config: &ParserConfig,
    parsed: &mut OdtParsed,
    source_part: &str,
) -> Result<Annotation> {
    let mut buf = Vec::new();
    let mut author = None;
    let mut date = None;
    let mut capture = None;
    let mut elements = Vec::new();
    loop {
        match xml_event(reader, &mut buf, source_part)? {
            Event::Start(e) if matches!(local(e.name().as_ref()), b"creator" | b"date") => {
                capture = Some(local(e.name().as_ref()).to_vec());
            }
            Event::Start(e) if local(e.name().as_ref()) == b"p" => {
                let style_name = attr(&e, b"style-name");
                let base_format = odt_base_format(styles, style_name.as_deref());
                let content = parse_odt_inline(
                    reader,
                    b"p",
                    &base_format,
                    styles,
                    config,
                    parsed,
                    source_part,
                )?;
                let provenance =
                    odt_provenance(parsed, StoryType::Comment, style_name, source_part);
                elements.push(DocumentElement::Paragraph(Paragraph {
                    content,
                    provenance,
                }));
            }
            Event::Text(e) if capture.is_some() => {
                let value = decode_xml_text(&e, source_part)?;
                match capture.as_deref() {
                    Some(b"creator") => author = Some(value),
                    Some(b"date") => date = Some(value),
                    _ => {}
                }
            }
            Event::End(e) if matches!(local(e.name().as_ref()), b"creator" | b"date") => {
                capture = None
            }
            Event::End(e) if local(e.name().as_ref()) == b"annotation" => break,
            Event::Eof => break,
            _ => {}
        }
        buf.clear();
    }
    Ok(Annotation {
        id,
        author,
        date,
        elements,
    })
}

fn parse_odt_note(
    reader: &mut Reader<&[u8]>,
    class: &str,
    styles: &HashMap<String, OdtStyle>,
    config: &ParserConfig,
    parsed: &mut OdtParsed,
    source_part: &str,
) -> Result<Annotation> {
    let mut buf = Vec::new();
    let mut citation = String::new();
    let mut in_citation = false;
    let mut in_body = false;
    let mut elements = Vec::new();
    loop {
        match xml_event(reader, &mut buf, source_part)? {
            Event::Start(e) if local(e.name().as_ref()) == b"note-citation" => in_citation = true,
            Event::Start(e) if local(e.name().as_ref()) == b"note-body" => in_body = true,
            Event::Start(e) if in_body && local(e.name().as_ref()) == b"p" => {
                let style_name = attr(&e, b"style-name");
                let base_format = odt_base_format(styles, style_name.as_deref());
                let content = parse_odt_inline(
                    reader,
                    b"p",
                    &base_format,
                    styles,
                    config,
                    parsed,
                    source_part,
                )?;
                let story = if class == "endnote" {
                    StoryType::Endnote
                } else {
                    StoryType::Footnote
                };
                let provenance = odt_provenance(parsed, story, style_name, source_part);
                elements.push(DocumentElement::Paragraph(Paragraph {
                    content,
                    provenance,
                }));
            }
            Event::Text(e) if in_citation => citation.push_str(&decode_xml_text(&e, source_part)?),
            Event::End(e) if local(e.name().as_ref()) == b"note-citation" => in_citation = false,
            Event::End(e) if local(e.name().as_ref()) == b"note-body" => in_body = false,
            Event::End(e) if local(e.name().as_ref()) == b"note" => break,
            Event::Eof => break,
            _ => {}
        }
        buf.clear();
    }
    let fallback = if class == "endnote" {
        parsed.endnotes.len() + 1
    } else {
        parsed.footnotes.len() + 1
    };
    let id = if citation.trim().is_empty() {
        fallback.to_string()
    } else {
        citation.trim().to_owned()
    };
    Ok(Annotation {
        id,
        author: None,
        date: None,
        elements,
    })
}

fn skip_xml_element(reader: &mut Reader<&[u8]>, end: &[u8], part: &str) -> Result<()> {
    let mut buf = Vec::new();
    let mut depth = 1usize;
    while depth > 0 {
        match xml_event(reader, &mut buf, part)? {
            Event::Start(_) => depth += 1,
            Event::End(e) => {
                depth -= 1;
                if depth == 0 && local(e.name().as_ref()) == end {
                    break;
                }
            }
            Event::Eof => break,
            _ => {}
        }
        buf.clear();
    }
    Ok(())
}

fn decode_xml_text(event: &quick_xml::events::BytesText<'_>, part: &str) -> Result<String> {
    let decoded = event
        .decode()
        .map_err(|error| Error::Conversion(format!("text decoding failed in {part}: {error}")))?;
    Ok(quick_xml::escape::unescape(&decoded)
        .map(Cow::into_owned)
        .unwrap_or_else(|_| decoded.into_owned()))
}

fn parse_odt_master_pages(
    bytes: &[u8],
    styles: &HashMap<String, OdtStyle>,
    config: &ParserConfig,
) -> Result<OdtStories> {
    if !config.include_headers_footers {
        return Ok(OdtStories::default());
    }
    let Some(data) = read_part(bytes, "styles.xml")? else {
        return Ok(OdtStories::default());
    };
    let mut reader = xml_reader(&data);
    let mut buf = Vec::new();
    let mut current_name = None;
    let mut current_header = Vec::new();
    let mut current_footer = Vec::new();
    let mut pages = Vec::new();
    let mut parsed = OdtParsed::default();
    loop {
        match xml_event(&mut reader, &mut buf, "styles.xml")? {
            Event::Start(e) if local(e.name().as_ref()) == b"master-page" => {
                current_name = attr(&e, b"name");
                current_header.clear();
                current_footer.clear();
            }
            Event::Start(e) if current_name.is_some() && local(e.name().as_ref()) == b"header" => {
                current_header = parse_odt_story_container(
                    &mut reader,
                    b"header",
                    StoryType::Header,
                    styles,
                    config,
                    &mut parsed,
                )?;
            }
            Event::Start(e) if current_name.is_some() && local(e.name().as_ref()) == b"footer" => {
                current_footer = parse_odt_story_container(
                    &mut reader,
                    b"footer",
                    StoryType::Footer,
                    styles,
                    config,
                    &mut parsed,
                )?;
            }
            Event::End(e) if local(e.name().as_ref()) == b"master-page" => {
                if let Some(name) = current_name.take()
                    && (!current_header.is_empty() || !current_footer.is_empty())
                {
                    pages.push((
                        name,
                        std::mem::take(&mut current_header),
                        std::mem::take(&mut current_footer),
                    ));
                }
            }
            Event::Eof => break,
            _ => {}
        }
        buf.clear();
    }
    let selected = pages
        .iter()
        .position(|(name, _, _)| name == "Standard")
        .or_else(|| (!pages.is_empty()).then_some(0));
    let Some(index) = selected else {
        return Ok(OdtStories::default());
    };
    let (name, header_elements, footer_elements) = pages.swap_remove(index);
    let header_id = format!("styles.xml#{name}/header");
    let footer_id = format!("styles.xml#{name}/footer");
    let mut headers = BTreeMap::new();
    let mut footers = BTreeMap::new();
    let mut header_references = Vec::new();
    let mut footer_references = Vec::new();
    if !header_elements.is_empty() {
        header_references.push(header_id.clone());
        headers.insert(
            header_id,
            Story {
                kind: Some(HeaderFooterKind::Default),
                used_by_sections: vec![0],
                elements: header_elements,
            },
        );
    }
    if !footer_elements.is_empty() {
        footer_references.push(footer_id.clone());
        footers.insert(
            footer_id,
            Story {
                kind: Some(HeaderFooterKind::Default),
                used_by_sections: vec![0],
                elements: footer_elements,
            },
        );
    }
    Ok(OdtStories {
        sections: vec![Section {
            index: 0,
            header_references,
            footer_references,
        }],
        headers,
        footers,
    })
}

fn parse_odt_story_container(
    reader: &mut Reader<&[u8]>,
    end: &[u8],
    story: StoryType,
    styles: &HashMap<String, OdtStyle>,
    config: &ParserConfig,
    parsed: &mut OdtParsed,
) -> Result<Vec<DocumentElement>> {
    let mut buf = Vec::new();
    let mut elements = Vec::new();
    loop {
        match xml_event(reader, &mut buf, "styles.xml")? {
            Event::Start(e) if matches!(local(e.name().as_ref()), b"p" | b"h") => {
                let tag = local(e.name().as_ref()).to_vec();
                let level = attr(&e, b"outline-level")
                    .and_then(|value| value.parse::<u8>().ok())
                    .unwrap_or(1)
                    .clamp(1, 6);
                let style_name = attr(&e, b"style-name");
                let base_format = odt_base_format(styles, style_name.as_deref());
                let content = parse_odt_inline(
                    reader,
                    &tag,
                    &base_format,
                    styles,
                    config,
                    parsed,
                    "styles.xml",
                )?;
                let provenance = odt_provenance(parsed, story, style_name.clone(), "styles.xml");
                elements.push(odt_text_element(
                    &tag,
                    level,
                    style_name.as_deref(),
                    content,
                    provenance,
                    styles,
                ));
            }
            Event::End(e) if local(e.name().as_ref()) == end => break,
            Event::Eof => break,
            _ => {}
        }
        buf.clear();
    }
    Ok(elements)
}

fn flush_odt_text(
    text: &mut String,
    format: &TextFormat,
    out: &mut Vec<Inline>,
    hyperlink: &mut Option<(String, Vec<Inline>)>,
) {
    if !text.is_empty() {
        push_inline(
            Inline::Text {
                text: std::mem::take(text),
                format: format.clone(),
            },
            out,
            hyperlink,
        );
    }
}

fn parse_odt_metadata(bytes: &[u8]) -> Result<Metadata> {
    let mut metadata = Metadata::default();
    if let Some(data) = read_part(bytes, "meta.xml")? {
        read_metadata_xml(&data, "meta.xml", &mut metadata)?;
    }
    Ok(metadata)
}

fn required_part(bytes: &[u8], name: &str) -> Result<Vec<u8>> {
    read_part(bytes, name)?.ok_or_else(|| Error::MissingPart(name.into()))
}

fn xml_reader(data: &[u8]) -> Reader<&[u8]> {
    let mut reader = Reader::from_reader(data);
    reader.config_mut().trim_text(false);
    reader
}

fn xml_event<'a>(
    reader: &mut Reader<&'a [u8]>,
    _buf: &mut Vec<u8>,
    part: &str,
) -> Result<Event<'a>> {
    reader.read_event().map_err(|source| Error::Xml {
        part: part.into(),
        source,
    })
}

fn local(name: &[u8]) -> &[u8] {
    name.rsplit(|v| *v == b':').next().unwrap_or(name)
}

fn attr(e: &BytesStart<'_>, wanted: &[u8]) -> Option<String> {
    e.attributes().with_checks(false).flatten().find_map(|a| {
        (local(a.key.as_ref()) == wanted)
            .then(|| String::from_utf8_lossy(a.value.as_ref()).into_owned())
    })
}

fn val(e: &BytesStart<'_>) -> Option<String> {
    attr(e, b"val")
}

fn bool_val(e: &BytesStart<'_>, default: bool) -> bool {
    val(e).map_or(default, |v| {
        !matches!(v.as_str(), "0" | "false" | "off" | "none")
    })
}

fn media_type(path: &str) -> String {
    match path
        .rsplit('.')
        .next()
        .unwrap_or_default()
        .to_ascii_lowercase()
        .as_str()
    {
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "svg" => "image/svg+xml",
        "webp" => "image/webp",
        "bmp" => "image/bmp",
        "tif" | "tiff" => "image/tiff",
        _ => "application/octet-stream",
    }
    .into()
}

#[cfg(test)]
#[path = "../tests/unit/parser.rs"]
mod tests;

use super::*;
use std::fs;
use std::io::{Cursor, Write};
use std::path::PathBuf;

fn fixture_path(group: &str, name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join("unit")
        .join(group)
        .join(name)
}

fn fixture(group: &str, name: &str) -> Vec<u8> {
    fs::read(fixture_path(group, name)).expect("read unit fixture")
}

fn docx_indexes() -> DocxIndexes {
    let relationships = parse_relationships(
        &fixture("xml", "relationships.xml"),
        "word/document.xml",
        "relationships.xml",
    )
    .expect("parse test relationships");
    DocxIndexes {
        relationships: HashMap::from([("word/document.xml".into(), relationships)]),
        styles: parse_styles(&fixture("xml", "styles.xml"), "styles.xml")
            .expect("parse test styles"),
        numbering: parse_numbering(&fixture("xml", "numbering.xml"), "numbering.xml")
            .expect("parse test numbering"),
    }
}

fn parse_docx_fixture(name: &str) -> Vec<DocumentElement> {
    parse_docx_story(
        &fixture("xml", name),
        "word/document.xml",
        StoryType::Main,
        &docx_indexes(),
        &mut ParseState::default(),
        &ParserConfig::default(),
    )
    .expect("parse DOCX XML fixture")
}

fn odt_package() -> Vec<u8> {
    let cursor = Cursor::new(Vec::new());
    let mut zip = zip::ZipWriter::new(cursor);
    let options = zip::write::SimpleFileOptions::default();
    for (name, data) in [
        ("content.xml", fixture("odt", "content.xml")),
        ("styles.xml", fixture("odt", "styles.xml")),
        ("Pictures/example.jpg", fixture("media", "example.jpg")),
    ] {
        zip.start_file(name, options).expect("start ODT part");
        zip.write_all(&data).expect("write ODT part");
    }
    zip.finish().expect("finish ODT package").into_inner()
}

fn inline_text(values: &[Inline]) -> String {
    values
        .iter()
        .map(|value| match value {
            Inline::Text { text, .. } => text.clone(),
            Inline::Hyperlink { content, .. } => inline_text(content),
            Inline::LineBreak => "\n".into(),
            _ => String::new(),
        })
        .collect()
}

#[test]
fn relationships_cannot_escape_package_root() {
    assert!(resolve_target("word/document.xml", "../../secret").is_err());
    assert_eq!(
        resolve_target("word/document.xml", "media/a.png").unwrap(),
        "word/media/a.png"
    );
    assert_eq!(
        resolve_target("word/document.xml", "/word/media/a.png").unwrap(),
        "word/media/a.png"
    );
}

#[test]
fn relationship_fixture_resolves_internal_and_external_targets() {
    let relationships = parse_relationships(
        &fixture("xml", "relationships.xml"),
        "word/document.xml",
        "relationships.xml",
    )
    .unwrap();

    assert_eq!(relationships["rIdImage"].target, "word/media/example.jpg");
    assert!(!relationships["rIdImage"].external);
    assert_eq!(relationships["rIdLink"].target, "https://example.com/a b");
    assert!(relationships["rIdLink"].external);
}

#[test]
fn localized_headings_are_recognized_and_clamped() {
    assert_eq!(heading_from_name("Überschrift 2"), Some(2));
    assert_eq!(heading_from_name("Heading 7"), Some(6));
    assert_eq!(heading_from_name("Body Text"), None);
}

#[test]
fn style_fixture_resolves_inheritance_and_heading_level() {
    let styles = parse_styles(&fixture("xml", "styles.xml"), "styles.xml").unwrap();
    let heading = resolved_style(&styles, "Heading2");

    assert_eq!(heading.name.as_deref(), Some("Heading 2"));
    assert_eq!(heading.outline, Some(1));
    assert!(heading.format.bold);
    assert!(heading.format.italic);
    assert_eq!(heading.format.language.as_deref(), Some("en-GB"));
}

#[test]
fn numbering_fixture_applies_levels_and_start_override() {
    let numbering = parse_numbering(&fixture("xml", "numbering.xml"), "numbering.xml").unwrap();

    assert_eq!(numbering[&("1".into(), 0)].marker, ListMarker::Bullet);
    assert_eq!(numbering[&("1".into(), 1)].marker, ListMarker::Bullet);
    assert_eq!(numbering[&("2".into(), 0)].marker, ListMarker::Decimal);
    assert_eq!(numbering[&("2".into(), 0)].start, 5);
}

#[test]
fn parses_formatted_heading_link_and_line_break_from_xml() {
    let elements = parse_docx_fixture("paragraph.xml");
    let DocumentElement::Heading(heading) = &elements[0] else {
        panic!("expected heading");
    };

    assert_eq!(heading.level, 2);
    assert_eq!(inline_text(&heading.content), "Styled *text*\nExample");
    let Inline::Text { format, .. } = &heading.content[0] else {
        panic!("expected formatted text");
    };
    assert!(format.bold && format.italic && format.underline && format.strikethrough);
    assert_eq!(format.vertical_align, VerticalAlign::Superscript);
    assert_eq!(format.language.as_deref(), Some("en-GB"));
    assert!(matches!(heading.content[1], Inline::LineBreak));
    assert!(matches!(
        &heading.content[2],
        Inline::Hyperlink { target, .. } if target == "https://example.com/a b"
    ));
    assert!(matches!(
        &heading.content[3],
        Inline::Image { resource_id, .. } if resource_id == "word/media/example.jpg"
    ));
}

#[test]
fn parses_and_groups_nested_docx_lists() {
    let elements = parse_docx_fixture("list.xml");

    assert_eq!(elements.len(), 2);
    let DocumentElement::List(bullets) = &elements[0] else {
        panic!("expected bullet list");
    };
    assert_eq!(bullets.marker, ListMarker::Bullet);
    assert_eq!(bullets.items.len(), 2);
    assert_eq!(bullets.items[1].children[0].level, 1);
    assert_eq!(bullets.items[1].children[0].items.len(), 1);
    let DocumentElement::List(numbers) = &elements[1] else {
        panic!("expected numbered list");
    };
    assert_eq!(numbers.marker, ListMarker::Decimal);
    assert_eq!(numbers.start, 5);
}

#[test]
fn parses_docx_table_cells_spans_and_empty_content() {
    let elements = parse_docx_fixture("table.xml");
    let DocumentElement::Table(table) = &elements[0] else {
        panic!("expected table");
    };

    assert_eq!(table.rows.len(), 2);
    assert_eq!(table.rows[0].len(), 3);
    assert_eq!(table.rows[1][0].column_span, 2);
    assert!(table.rows[1][1].blocks.is_empty());
}

#[test]
fn parsed_docx_fragments_match_markdown_fixtures() {
    for (xml, markdown) in [("list.xml", "list.md"), ("table.xml", "table.md")] {
        let document = Document {
            elements: parse_docx_fixture(xml),
            ..Document::default()
        };
        let config = ParserConfig::builder()
            .include_document_metadata(false)
            .build();
        let expected = fs::read_to_string(fixture_path("markdown", markdown)).unwrap();

        assert_eq!(document.to_markdown(&config).unwrap(), expected);
    }
}

#[test]
fn parses_odt_styles_structure_and_image_reference() {
    let bytes = odt_package();
    let elements = parse_odt_main(&bytes, &ParserConfig::default()).unwrap();

    assert!(matches!(
        &elements[0],
        DocumentElement::Heading(heading) if heading.level == 2 && inline_text(&heading.content) == "ODT Heading"
    ));
    let DocumentElement::Paragraph(paragraph) = &elements[1] else {
        panic!("expected ODT paragraph");
    };
    assert_eq!(inline_text(&paragraph.content), "Before strong\nlink");
    let Inline::Text { format, .. } = &paragraph.content[1] else {
        panic!("expected styled ODT span");
    };
    assert!(format.bold && format.italic);
    assert!(
        matches!(&elements[2], DocumentElement::List(list) if !list.items[0].children.is_empty())
    );
    assert!(matches!(&elements[3], DocumentElement::Table(table) if table.rows.len() == 2));
    assert!(matches!(
        &elements[4],
        DocumentElement::Paragraph(paragraph)
            if matches!(&paragraph.content[0], Inline::Image { resource_id, .. } if resource_id == "Pictures/example.jpg")
    ));
}

#[test]
fn odt_parser_loads_referenced_image_bytes() {
    let document = parse_odt(
        &odt_package(),
        &ParserConfig::builder().compress_images(false).build(),
    )
    .unwrap();

    assert_eq!(
        document.images["Pictures/example.jpg"].data,
        fixture("media", "example.jpg")
    );
}

#[test]
fn compresses_media_fixture_to_a_valid_jpeg() {
    let source = fixture("media", "example.jpg");
    let (compressed, media_type) = prepare_image(
        source.clone(),
        "image/jpeg".into(),
        &ParserConfig::builder().quality(50).build(),
    )
    .unwrap();

    assert_eq!(media_type, "image/jpeg");
    assert!(compressed.len() < source.len());
    assert!(image::load_from_memory(&compressed).is_ok());
}

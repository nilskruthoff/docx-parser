use docx_to_md::{
    Document, DocumentContainer, DocumentElement, DocumentFormat, Inline, ListItem, ListMarker,
    ParserConfig, TextFormat, VerticalAlign,
};
use std::path::PathBuf;

fn fixture_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join("integration")
        .join("docx")
        .join("fixture.docx")
}

fn config() -> ParserConfig {
    ParserConfig::builder()
        .compress_images(false)
        .include_comments(true)
        .build()
}

fn parse_fixture() -> (DocumentContainer, Document) {
    let container = DocumentContainer::open_as(fixture_path(), config(), DocumentFormat::Docx)
        .expect("open DOCX fixture");
    let document = container.parse_all().expect("parse DOCX fixture");
    (container, document)
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

fn element_text(element: &DocumentElement) -> String {
    match element {
        DocumentElement::Paragraph(paragraph) => inline_text(&paragraph.content),
        DocumentElement::Heading(heading) => inline_text(&heading.content),
        DocumentElement::List(list) => list
            .items
            .iter()
            .map(list_item_text)
            .collect::<Vec<_>>()
            .join(" "),
        DocumentElement::Table(table) => table
            .rows
            .iter()
            .flatten()
            .flat_map(|cell| cell.blocks.iter())
            .map(element_text)
            .collect::<Vec<_>>()
            .join(" "),
        _ => String::new(),
    }
}

fn list_item_text(item: &ListItem) -> String {
    let mut text = item
        .blocks
        .iter()
        .map(element_text)
        .collect::<Vec<_>>()
        .join(" ");
    for child in &item.children {
        for nested in &child.items {
            text.push(' ');
            text.push_str(&list_item_text(nested));
        }
    }
    text
}

fn text_formats<'a>(document: &'a Document, needle: &str) -> Vec<&'a TextFormat> {
    document
        .elements
        .iter()
        .filter_map(|element| match element {
            DocumentElement::Paragraph(paragraph) => Some(&paragraph.content),
            _ => None,
        })
        .flatten()
        .filter_map(|inline| match inline {
            Inline::Text { text, format } if text.contains(needle) => Some(format),
            _ => None,
        })
        .collect()
}

#[test]
fn detects_docx_and_exposes_metadata() {
    let (container, document) = parse_fixture();

    assert_eq!(container.format(), DocumentFormat::Docx);
    assert_eq!(
        container.metadata().title.as_deref(),
        Some("Feature Fixture")
    );
    assert_eq!(
        container.metadata().subject.as_deref(),
        Some("Integration test document")
    );
    assert_eq!(container.metadata().author.as_deref(), Some("Doe, John"));
    assert_eq!(container.metadata().keywords, ["fixture", "docx", "odt"]);
    assert_eq!(document.metadata, *container.metadata());
}

#[test]
fn preserves_main_story_order_headings_and_provenance() {
    let (container, document) = parse_fixture();
    let iterated = container
        .iter_blocks()
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap();

    assert_eq!(iterated, document.elements);
    let heading_levels = document
        .elements
        .iter()
        .filter_map(|element| match element {
            DocumentElement::Heading(heading) => Some(heading.level),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert!(heading_levels.starts_with(&[1, 1, 2, 3, 4]));
    assert!(
        document
            .elements
            .iter()
            .enumerate()
            .all(|(index, element)| {
                element.provenance().source_part == "word/document.xml"
                    && element.provenance().sequence >= index
            })
    );
}

#[test]
fn parses_inline_formatting_hyperlinks_and_breaks() {
    let (_, document) = parse_fixture();

    assert!(
        text_formats(&document, "bold")
            .iter()
            .any(|format| format.bold)
    );
    assert!(
        text_formats(&document, "italic")
            .iter()
            .any(|format| format.italic)
    );
    assert!(
        text_formats(&document, "underlined")
            .iter()
            .any(|format| format.underline)
    );
    assert!(
        text_formats(&document, "strikethrough")
            .iter()
            .any(|format| format.strikethrough)
    );
    assert!(
        text_formats(&document, "superscript")
            .iter()
            .any(|format| format.vertical_align == VerticalAlign::Superscript)
    );
    assert!(
        text_formats(&document, "subscript")
            .iter()
            .any(|format| format.vertical_align == VerticalAlign::Subscript)
    );
    assert!(document.elements.iter().any(|element| matches!(
        element,
        DocumentElement::Paragraph(paragraph)
            if paragraph.content.iter().any(|inline| matches!(inline, Inline::Hyperlink { target, .. } if target == "https://github.com/nilskruthoff/docx-parser"))
    )));
    assert!(document.elements.iter().any(|element| matches!(
        element,
        DocumentElement::Paragraph(paragraph)
            if paragraph.content.iter().any(|inline| matches!(inline, Inline::LineBreak))
    )));
}

#[test]
fn parses_nested_bullet_and_numbered_lists() {
    let (_, document) = parse_fixture();
    let lists = document
        .elements
        .iter()
        .filter_map(|element| match element {
            DocumentElement::List(list) => Some(list),
            _ => None,
        })
        .collect::<Vec<_>>();

    assert!(lists.iter().any(|list| {
        list.marker == ListMarker::Bullet
            && list.items.len() == 2
            && list_item_text(&list.items[1]).contains("nested item")
    }));
    assert!(lists.iter().any(|list| {
        list.marker == ListMarker::Decimal
            && list.items.len() == 2
            && list.items[1]
                .children
                .first()
                .is_some_and(|child| child.marker == ListMarker::LowerLetter)
            && list_item_text(&list.items[1]).contains("nested nested number")
    }));
}

#[test]
fn parses_table_shape_empty_cell_and_values() {
    let (_, document) = parse_fixture();
    let table = document
        .elements
        .iter()
        .find_map(|element| match element {
            DocumentElement::Table(table) => Some(table),
            _ => None,
        })
        .expect("table in fixture");

    assert_eq!(table.rows.len(), 3);
    assert!(table.rows.iter().all(|row| row.len() == 3));
    assert_eq!(element_text(&table.rows[0][0].blocks[0]), "Heading A");
    assert!(
        table.rows[1][1]
            .blocks
            .iter()
            .all(|block| element_text(block).is_empty())
    );
    assert_eq!(element_text(&table.rows[2][2].blocks[0]), "C2");
}

#[test]
fn parses_docx_stories_sections_and_annotations() {
    let (_, document) = parse_fixture();

    assert!(!document.sections.is_empty());
    assert!(!document.headers.is_empty());
    assert!(!document.footers.is_empty());
    assert!(
        document
            .headers
            .values()
            .any(|story| !story.used_by_sections.is_empty())
    );
    assert!(
        document
            .footers
            .values()
            .any(|story| !story.used_by_sections.is_empty())
    );
    assert!(document.comments.iter().any(|comment| comment.id == "0"));
    assert!(document.footnotes.iter().any(|note| note.id == "1"));
    assert!(document.endnotes.iter().any(|note| note.id == "1"));

    let markdown = document.to_markdown(&config()).unwrap();
    assert!(markdown.contains("## Headers and Footers"));
    assert!(markdown.contains("[^fn-1]"));
    assert!(markdown.contains("[^en-1]"));
    assert!(markdown.contains("> **Comments**"));
}

#[test]
fn renders_representative_docx_content_to_markdown() {
    let (container, _) = parse_fixture();
    let markdown = container.convert_to_md().unwrap();

    assert_eq!(markdown.matches("Document Metadata").count(), 1);
    assert!(markdown.contains("**bold**"));
    assert!(markdown.contains("](https://github.com/nilskruthoff/docx-parser)"));
    assert!(markdown.contains("| Heading A | Heading B | Heading C |"));
    assert!(markdown.contains("- First item"));
    assert!(markdown.contains("1. first number"));
}

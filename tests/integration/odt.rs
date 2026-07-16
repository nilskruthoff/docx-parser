use docx_to_md::{
    Document, DocumentContainer, DocumentElement, DocumentFormat, ImageHandlingMode, Inline,
    ParserConfig, TextFormat, VerticalAlign,
};
use std::fs;
use std::path::PathBuf;

fn fixture_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join("integration")
        .join("odt")
        .join("fixture.odt")
}

fn expected(name: &str) -> String {
    fs::read_to_string(fixture_path().parent().unwrap().join(name)).expect("read ODT golden")
}

fn image_fixture() -> Vec<u8> {
    fs::read(
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("tests")
            .join("fixtures")
            .join("unit")
            .join("media")
            .join("example.jpg"),
    )
    .expect("read image fixture")
}

fn config() -> ParserConfig {
    ParserConfig::builder().compress_images(false).build()
}

fn parse_fixture() -> (DocumentContainer, Document) {
    let container = DocumentContainer::open_as(fixture_path(), config(), DocumentFormat::Odt)
        .expect("open ODT fixture");
    let document = container.parse_all().expect("parse ODT fixture");
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
        DocumentElement::Title(title) => inline_text(&title.content),
        DocumentElement::Subtitle(subtitle) => inline_text(&subtitle.content),
        DocumentElement::BlockQuote(quote) => inline_text(&quote.content),
        _ => String::new(),
    }
}

fn render_section(elements: Vec<DocumentElement>) -> String {
    Document {
        elements,
        ..Document::default()
    }
    .to_markdown(
        &ParserConfig::builder()
            .extract_images(false)
            .include_document_metadata(false)
            .include_headers_footers(false)
            .include_footnotes(false)
            .include_endnotes(false)
            .include_comments(false)
            .build(),
    )
    .unwrap()
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
fn detects_odt_and_preserves_main_block_order() {
    let (container, document) = parse_fixture();
    let iterated = container
        .iter_blocks()
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap();

    assert_eq!(container.format(), DocumentFormat::Odt);
    assert_eq!(iterated, document.elements);
    assert!(matches!(
        &document.elements[0],
        DocumentElement::Heading(heading) if heading.level == 1 && inline_text(&heading.content).contains("Page 1")
    ));
    assert!(
        document
            .elements
            .iter()
            .all(|element| element.provenance().source_part == "content.xml")
    );
    assert!(
        document
            .elements
            .windows(2)
            .all(|window| window[0].provenance().sequence <= window[1].provenance().sequence)
    );
}

#[test]
fn parses_odt_headings_formatting_and_hyperlink() {
    let (_, document) = parse_fixture();
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
}

#[test]
fn parses_odt_title_and_subtitle_as_semantic_blocks() {
    let (_, document) = parse_fixture();
    let elements = document
        .elements
        .iter()
        .filter(|element| {
            matches!(
                element,
                DocumentElement::Title(_) | DocumentElement::Subtitle(_)
            )
        })
        .cloned()
        .collect::<Vec<_>>();

    assert!(
        matches!(&elements[0], DocumentElement::Title(title) if inline_text(&title.content) == "Titel")
    );
    assert!(
        matches!(&elements[1], DocumentElement::Subtitle(subtitle) if inline_text(&subtitle.content) == "Subtitel")
    );
    assert_eq!(render_section(elements), expected("title_subtitle.md"));
}

#[test]
fn parses_nested_odt_lists_and_table_cells() {
    let (_, document) = parse_fixture();
    let lists = document
        .elements
        .iter()
        .filter_map(|element| match element {
            DocumentElement::List(list) => Some(list),
            _ => None,
        })
        .collect::<Vec<_>>();
    let table = document
        .elements
        .iter()
        .find_map(|element| match element {
            DocumentElement::Table(table) => Some(table),
            _ => None,
        })
        .expect("ODT table");

    assert_eq!(lists.len(), 2);
    assert_eq!(lists[0].marker, docx_to_md::ListMarker::Bullet);
    assert_eq!(lists[1].marker, docx_to_md::ListMarker::Decimal);
    assert!(
        lists[0].items[1]
            .children
            .iter()
            .any(|list| !list.items.is_empty())
    );
    assert!(lists[1].items[1].children.iter().any(|list| {
        list.marker == docx_to_md::ListMarker::LowerLetter
            && list.items[0]
                .children
                .iter()
                .any(|child| child.marker == docx_to_md::ListMarker::LowerRoman)
    }));
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
    assert_eq!(
        render_section(
            document
                .elements
                .iter()
                .filter(|element| matches!(element, DocumentElement::List(_)))
                .cloned()
                .collect(),
        ),
        expected("lists.md")
    );
}

#[test]
fn renders_odt_quote_styles_as_exact_blockquotes() {
    let (_, document) = parse_fixture();
    let quotes = document
        .elements
        .iter()
        .filter(|element| matches!(element, DocumentElement::BlockQuote(_)))
        .cloned()
        .collect::<Vec<_>>();

    assert_eq!(quotes.len(), 2);
    assert_eq!(render_section(quotes), expected("quotes.md"));
}

#[test]
fn extracts_odt_image_and_honors_manual_mode() {
    let (_, document) = parse_fixture();
    let resource = document.images.values().next().expect("ODT image");

    assert_eq!(document.images.len(), 1);
    assert_eq!(resource.data, image_fixture());
    assert_eq!(resource.media_type, "image/jpeg");
    assert!(
        document
            .to_markdown(&config())
            .unwrap()
            .contains("data:image/jpeg;base64,")
    );
    let manual = document
        .to_markdown(
            &ParserConfig::builder()
                .compress_images(false)
                .image_handling_mode(ImageHandlingMode::Manually)
                .build(),
        )
        .unwrap();
    assert!(!manual.contains("data:image"));
}

#[test]
fn renders_representative_odt_content_to_markdown() {
    let (container, document) = parse_fixture();
    let markdown = container.convert_to_md().unwrap();

    assert!(markdown.contains("**bold**"));
    assert!(markdown.contains("](https://github.com/nilskruthoff/docx-parser)"));
    assert!(markdown.contains("| Heading A | Heading B | Heading C |"));
    assert!(markdown.contains("- First item"));
    assert_eq!(markdown.matches("data:image/jpeg;base64,").count(), 1);
    assert!(
        document
            .elements
            .iter()
            .any(|element| element_text(element) == "Page 3 – Images")
    );
    assert!(!markdown.contains("Unbekannter Autor2026"));
    let supplementary = &markdown[markdown.find("[^fn-1]:").expect("footnote definition")..];
    assert_eq!(supplementary, expected("supplementary.md"));
}

#[test]
fn extracts_odt_comments_without_leaking_metadata_into_body() {
    let comment_config = ParserConfig::builder()
        .compress_images(false)
        .include_comments(true)
        .build();
    let container =
        DocumentContainer::open_as(fixture_path(), comment_config, DocumentFormat::Odt).unwrap();
    let document = container.parse_all().unwrap();
    let markdown = container.convert_to_md().unwrap();

    assert_eq!(document.comments.len(), 1);
    assert_eq!(document.comments[0].id, "1");
    assert_eq!(
        document.comments[0].author.as_deref(),
        Some("Unbekannter Autor")
    );
    assert_eq!(
        document.comments[0].date.as_deref(),
        Some("2026-07-16T00:33:35.989363400")
    );
    assert!(
        document
            .elements
            .iter()
            .any(|element| element_text(element) == "Page 3 – Images")
    );
    assert!(!markdown.contains("Unbekannter Autor2026"));
    let supplementary = &markdown[markdown.find("[^fn-1]:").expect("footnote definition")..];
    assert_eq!(supplementary, expected("supplementary_with_comments.md"));
}

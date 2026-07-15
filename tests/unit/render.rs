use super::*;
use std::collections::BTreeMap;
use std::fs;
use std::path::PathBuf;

fn fixture_path(group: &str, name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join("unit")
        .join(group)
        .join(name)
}

fn text(value: &str, format: TextFormat) -> Inline {
    Inline::Text {
        text: value.into(),
        format,
    }
}

fn image_document() -> Document {
    let data = fs::read(fixture_path("media", "example.jpg")).expect("read image fixture");
    Document {
        elements: vec![DocumentElement::Image(ImageElement {
            resource_id: "image-1".into(),
            alt: "example image".into(),
            provenance: Provenance::default(),
        })],
        images: BTreeMap::from([(
            "image-1".into(),
            ImageResource {
                id: "image-1".into(),
                part_path: "word/media/example.jpeg".into(),
                media_type: "image/jpeg".into(),
                data,
            },
        )]),
        ..Document::default()
    }
}

#[test]
fn markdown_text_is_escaped_before_formatting() {
    let mut out = String::new();
    render_formatted(
        "a * b | c",
        &TextFormat {
            bold: true,
            ..TextFormat::default()
        },
        &mut out,
        MarkdownContext::TableCell,
    );

    assert_eq!(out, "**a \\* b \\| c**");
}

#[test]
fn formatting_model_matches_markdown_fixture() {
    let paragraph = Paragraph {
        content: vec![
            text(
                "bold",
                TextFormat {
                    bold: true,
                    ..TextFormat::default()
                },
            ),
            text(" ", TextFormat::default()),
            text(
                "italic",
                TextFormat {
                    italic: true,
                    ..TextFormat::default()
                },
            ),
            text(" ", TextFormat::default()),
            text(
                "underline",
                TextFormat {
                    underline: true,
                    ..TextFormat::default()
                },
            ),
            text(" ", TextFormat::default()),
            text(
                "strike",
                TextFormat {
                    strikethrough: true,
                    ..TextFormat::default()
                },
            ),
            text(" ", TextFormat::default()),
            text(
                "up",
                TextFormat {
                    vertical_align: VerticalAlign::Superscript,
                    ..TextFormat::default()
                },
            ),
            text(" ", TextFormat::default()),
            text(
                "down",
                TextFormat {
                    vertical_align: VerticalAlign::Subscript,
                    ..TextFormat::default()
                },
            ),
            text(" ", TextFormat::default()),
            Inline::Hyperlink {
                target: "https://example.com/a b".into(),
                content: vec![text("link", TextFormat::default())],
            },
        ],
        provenance: Provenance::default(),
    };
    let document = Document {
        elements: vec![DocumentElement::Paragraph(paragraph)],
        ..Document::default()
    };
    let config = ParserConfig::builder()
        .include_document_metadata(false)
        .build();
    let expected = fs::read_to_string(fixture_path("markdown", "formatting.md")).unwrap();

    assert_eq!(document.to_markdown(&config).unwrap(), expected);
}

#[test]
fn block_markers_are_only_escaped_at_line_start() {
    assert_eq!(
        escape_text("# heading and # literal", MarkdownContext::Flow),
        "\\# heading and # literal"
    );
    assert_eq!(
        escape_text("left | right", MarkdownContext::TableCell),
        "left \\| right"
    );
    assert_eq!(
        escape_text("left | right", MarkdownContext::Flow),
        "left | right"
    );
}

#[test]
fn metadata_cannot_close_comment_and_renders_once() {
    assert_eq!(sanitize_comment("x--><script>"), "x—&gt;<script&gt;");
    let document = Document {
        metadata: Metadata {
            title: Some("x--> title".into()),
            author: Some("Ada".into()),
            ..Metadata::default()
        },
        ..Document::default()
    };

    let markdown = document.to_markdown(&ParserConfig::default()).unwrap();
    assert_eq!(markdown.matches("Document Metadata").count(), 1);
    assert!(markdown.contains("title: x—&gt; title"));
    assert!(!markdown.contains("--> title"));
}

#[test]
fn annotation_references_and_definitions_follow_config() {
    let annotation = |id: &str, story| Annotation {
        id: id.into(),
        author: Some("Ada".into()),
        date: None,
        elements: vec![DocumentElement::Paragraph(Paragraph {
            content: vec![text("body", TextFormat::default())],
            provenance: Provenance {
                story,
                ..Provenance::default()
            },
        })],
    };
    let document = Document {
        elements: vec![DocumentElement::Paragraph(Paragraph {
            content: vec![
                Inline::CommentReference("7".into()),
                Inline::FootnoteReference("8".into()),
                Inline::EndnoteReference("9".into()),
            ],
            provenance: Provenance::default(),
        })],
        comments: vec![annotation("7", StoryType::Comment)],
        footnotes: vec![annotation("8", StoryType::Footnote)],
        endnotes: vec![annotation("9", StoryType::Endnote)],
        ..Document::default()
    };
    let config = ParserConfig::builder()
        .include_document_metadata(false)
        .include_comments(true)
        .build();

    let markdown = document.to_markdown(&config).unwrap();
    assert!(markdown.contains("<sup>[comment 7]</sup>[^fn-8][^en-9]"));
    assert!(markdown.contains("[^fn-8]: body"));
    assert!(markdown.contains("[^en-9]: body"));
    assert!(markdown.contains("> **7 — Ada:** body"));
}

#[test]
fn embeds_images_as_data_urls() {
    let markdown = image_document()
        .to_markdown(
            &ParserConfig::builder()
                .compress_images(false)
                .include_document_metadata(false)
                .build(),
        )
        .unwrap();

    assert!(markdown.starts_with("![example image](data:image/jpeg;base64,"));
}

#[test]
fn manual_image_mode_keeps_markdown_clean() {
    let markdown = image_document()
        .to_markdown(
            &ParserConfig::builder()
                .image_handling_mode(ImageHandlingMode::Manually)
                .include_document_metadata(false)
                .build(),
        )
        .unwrap();

    assert_eq!(markdown, "\n");
}

#[test]
fn save_mode_writes_image_and_uses_file_url() {
    let directory = tempfile::tempdir().unwrap();
    let config = ParserConfig::builder()
        .image_handling_mode(ImageHandlingMode::Save)
        .image_output_path(directory.path())
        .include_document_metadata(false)
        .build();

    let markdown = image_document().to_markdown(&config).unwrap();

    assert!(directory.path().join("word_media_example.jpg").is_file());
    assert!(markdown.contains("file:///"));
    assert!(markdown.contains("word_media_example.jpg"));
}

#[test]
fn save_mode_requires_an_output_directory() {
    let result = image_document().to_markdown(
        &ParserConfig::builder()
            .image_handling_mode(ImageHandlingMode::Save)
            .build(),
    );

    assert!(matches!(result, Err(Error::InvalidOutputPath(_))));
}

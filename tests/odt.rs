use std::io::{Cursor, Write};

use docx_to_md::{
    DocumentContainer, DocumentElement, DocumentFormat, ImageHandlingMode, ParserConfig,
};
use zip::write::SimpleFileOptions;

fn odt() -> Vec<u8> {
    let cursor = Cursor::new(Vec::new());
    let mut zip = zip::ZipWriter::new(cursor);
    let options = SimpleFileOptions::default();
    zip.start_file("mimetype", options).unwrap();
    zip.write_all(b"application/vnd.oasis.opendocument.text")
        .unwrap();
    zip.start_file("content.xml", options).unwrap();
    zip.write_all(
        br#"<?xml version="1.0" encoding="UTF-8"?>
        <office:document-content xmlns:office="urn:o" xmlns:text="urn:t" xmlns:table="urn:tb" xmlns:draw="urn:d" xmlns:xlink="urn:x" xmlns:style="urn:s" xmlns:fo="urn:f">
          <office:automatic-styles><style:style style:name="Strong"><style:text-properties fo:font-weight="bold"/></style:style></office:automatic-styles>
          <office:body><office:text>
            <text:h text:outline-level="2">Title</text:h>
            <text:p>Before <text:a xlink:href="https://example.com">link</text:a> <text:span text:style-name="Strong">bold</text:span><text:line-break/>after</text:p>
            <text:list><text:list-item><text:p>one</text:p><text:list><text:list-item><text:p>nested</text:p></text:list-item></text:list></text:list-item></text:list>
            <table:table><table:table-row><table:table-cell><text:p>A | B</text:p></table:table-cell></table:table-row></table:table>
            <text:p><draw:image xlink:href="Pictures/a.png"/></text:p>
          </office:text></office:body>
        </office:document-content>"#,
    )
    .unwrap();
    zip.start_file("meta.xml", options).unwrap();
    zip.write_all(br#"<office:document-meta xmlns:office="urn:o" xmlns:dc="urn:d"><office:meta><dc:title>ODT title</dc:title><dc:creator>Ada</dc:creator></office:meta></office:document-meta>"#).unwrap();
    zip.start_file("Pictures/a.png", options).unwrap();
    zip.write_all(b"png bytes").unwrap();
    zip.finish().unwrap().into_inner()
}

#[test]
fn detects_and_parses_odt_structures() {
    let package = DocumentContainer::from_reader(
        Cursor::new(odt()),
        ParserConfig::builder().compress_images(false).build(),
    )
    .unwrap();
    assert_eq!(package.format(), DocumentFormat::Odt);
    assert_eq!(package.metadata().title.as_deref(), Some("ODT title"));
    let document = package.parse_all().unwrap();
    assert!(
        document
            .elements
            .iter()
            .any(|v| matches!(v, DocumentElement::Heading(_)))
    );
    assert!(
        document
            .elements
            .iter()
            .any(|v| matches!(v, DocumentElement::List(_)))
    );
    assert!(
        document
            .elements
            .iter()
            .any(|v| matches!(v, DocumentElement::Table(_)))
    );
    assert_eq!(document.images.len(), 1);
    let markdown = package.convert_to_md().unwrap();
    assert!(markdown.contains("[link](https://example.com)"));
    assert!(markdown.contains("**bold**"));
    assert!(markdown.contains("A \\| B"));
    assert_eq!(markdown.matches("Document Metadata").count(), 1);
}

#[test]
fn manual_mode_keeps_bytes_but_omits_markdown_image() {
    let config = ParserConfig::builder()
        .compress_images(false)
        .image_handling_mode(ImageHandlingMode::Manually)
        .build();
    let package = DocumentContainer::from_reader(Cursor::new(odt()), config).unwrap();
    let document = package.parse_all().unwrap();
    assert_eq!(document.images["Pictures/a.png"].base64(), "cG5nIGJ5dGVz");
    assert!(!package.convert_to_md().unwrap().contains("data:image"));
}

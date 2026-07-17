use super::*;
use std::io::Write;

fn archive(parts: &[(&str, &[u8])]) -> Vec<u8> {
    let cursor = Cursor::new(Vec::new());
    let mut zip = zip::ZipWriter::new(cursor);
    let options = zip::write::SimpleFileOptions::default();
    for (name, data) in parts {
        zip.start_file(*name, options).expect("start ZIP part");
        zip.write_all(data).expect("write ZIP part");
    }
    zip.finish().expect("finish ZIP").into_inner()
}

#[test]
fn detects_docx_from_required_parts() {
    let bytes = archive(&[
        ("[Content_Types].xml", b"<Types/>"),
        ("word/document.xml", b"<w:document/>"),
    ]);

    assert_eq!(detect(&bytes).expect("detect DOCX"), DocumentFormat::Docx);
}

#[test]
fn detects_odt_from_mimetype_or_manifest() {
    let with_mimetype = archive(&[
        ("mimetype", b"application/vnd.oasis.opendocument.text"),
        ("content.xml", b"<office:document-content/>"),
    ]);
    let with_manifest = archive(&[
        ("content.xml", b"<office:document-content/>"),
        ("META-INF/manifest.xml", b"<manifest:manifest/>"),
    ]);

    assert_eq!(detect(&with_mimetype).unwrap(), DocumentFormat::Odt);
    assert_eq!(detect(&with_manifest).unwrap(), DocumentFormat::Odt);
}

#[test]
fn rejects_content_xml_without_odt_identity() {
    let bytes = archive(&[("content.xml", b"<root/>")]);

    assert!(matches!(detect(&bytes), Err(Error::UnknownFormat)));
}

#[test]
fn rejects_archives_that_are_not_supported_documents() {
    let bytes = archive(&[("readme.txt", b"not a document")]);

    assert!(matches!(detect(&bytes), Err(Error::UnknownFormat)));
}

#[test]
fn reads_optional_parts_and_lists_archive_entries() {
    let bytes = archive(&[("a.xml", b"alpha"), ("folder/b.xml", b"beta")]);

    assert_eq!(read_part(&bytes, "a.xml").unwrap(), Some(b"alpha".to_vec()));
    assert_eq!(read_part(&bytes, "missing.xml").unwrap(), None);
    assert_eq!(
        list_parts(&bytes).unwrap(),
        vec!["a.xml".to_string(), "folder/b.xml".to_string()]
    );
}

#[test]
fn open_as_reports_a_format_mismatch() {
    let bytes = archive(&[
        ("mimetype", b"application/vnd.oasis.opendocument.text"),
        ("content.xml", b"<office:document-content/>"),
    ]);
    let result = DocxContainer::from_reader(Cursor::new(bytes), ParserConfig::default());

    assert!(matches!(
        result,
        Err(Error::FormatMismatch {
            expected: DocumentFormat::Docx,
            actual: DocumentFormat::Odt
        })
    ));
}

fn docx_with_document_xml(xml: &str, config: ParserConfig) -> DocumentContainer {
    DocumentContainer::from_bytes(
        archive(&[
            ("[Content_Types].xml", b"<Types/>"),
            ("word/document.xml", xml.as_bytes()),
        ]),
        config,
    )
    .unwrap()
}

fn odt_with_content_xml(xml: &str) -> DocumentContainer {
    DocumentContainer::from_bytes(
        archive(&[
            ("mimetype", b"application/vnd.oasis.opendocument.text"),
            ("content.xml", xml.as_bytes()),
        ]),
        ParserConfig::default(),
    )
    .unwrap()
}

fn page_markdown(container: &DocumentContainer, mode: PageBoundaryMode) -> Vec<String> {
    container
        .iter_pages_with(mode)
        .unwrap()
        .map(|page| page.unwrap().to_markdown().unwrap())
        .collect()
}

#[test]
fn docx_page_modes_distinguish_explicit_and_rendered_markers() {
    let container = docx_with_document_xml(
        r#"<w:document><w:body>
        <w:p><w:r><w:t>One</w:t><w:br w:type="page"/></w:r></w:p>
        <w:p><w:r><w:t>Two</w:t></w:r></w:p>
        <w:p><w:r><w:t>Hint</w:t><w:lastRenderedPageBreak/></w:r></w:p>
        <w:p><w:r><w:t>Three</w:t></w:r></w:p>
        </w:body></w:document>"#,
        ParserConfig::default(),
    );

    let combined = page_markdown(&container, PageBoundaryMode::Combined);
    let explicit = page_markdown(&container, PageBoundaryMode::ExplicitOnly);
    let rendered = page_markdown(&container, PageBoundaryMode::RenderedHintsOnly);
    assert_eq!(combined.len(), 3);
    assert_eq!(explicit.len(), 2);
    assert_eq!(rendered.len(), 2);
    assert!(combined[1].contains("Two") && combined[1].contains("Hint"));
    assert!(rendered[0].contains("One") && rendered[0].contains("Hint"));
}

#[test]
fn odt_page_modes_honor_inherited_style_and_soft_breaks() {
    let container = odt_with_content_xml(
        r#"<office:document-content><office:automatic-styles>
        <style:style style:name="Base"><style:paragraph-properties fo:break-before="page"/></style:style>
        <style:style style:name="Break" style:parent-style-name="Base"/>
        </office:automatic-styles><office:body><office:text>
        <text:p>One</text:p><text:p text:style-name="Break">Two</text:p>
        <text:p>Three<text:soft-page-break/></text:p><text:p>Four</text:p>
        </office:text></office:body></office:document-content>"#,
    );

    assert_eq!(
        page_markdown(&container, PageBoundaryMode::Combined).len(),
        3
    );
    assert_eq!(
        page_markdown(&container, PageBoundaryMode::ExplicitOnly).len(),
        2
    );
    assert_eq!(
        page_markdown(&container, PageBoundaryMode::RenderedHintsOnly).len(),
        2
    );
}

#[test]
fn page_iterator_yields_an_empty_document_once() {
    let container = docx_with_document_xml(
        "<w:document><w:body/></w:document>",
        ParserConfig::default(),
    );
    let pages = container
        .iter_pages()
        .unwrap()
        .collect::<Result<Vec<_>>>()
        .unwrap();

    assert_eq!(pages.len(), 1);
    assert_eq!(pages[0].page_number, 1);
    assert!(pages[0].elements.is_empty());
    assert_eq!(pages[0].boundary, crate::PageBoundary::EndOfDocument);
}

#[test]
fn page_iterator_delays_main_xml_errors_and_then_fuses() {
    let container = docx_with_document_xml(
        r#"<w:document><w:body>
        <w:p><w:r><w:t>Good</w:t><w:br w:type="page"/></w:r></w:p>
        <w:p><w:r><w:t>Broken</w:p></w:body></w:document>"#,
        ParserConfig::default(),
    );
    let mut pages = container.iter_pages().unwrap();

    assert!(
        pages
            .next()
            .unwrap()
            .unwrap()
            .to_markdown()
            .unwrap()
            .contains("Good")
    );
    assert!(pages.next().unwrap().is_err());
    assert!(pages.next().is_none());
}

#[test]
fn odd_section_start_inserts_only_the_required_blank_page() {
    let container = docx_with_document_xml(
        r#"<w:document><w:body>
        <w:p><w:r><w:t>One</w:t></w:r><w:pPr><w:sectPr><w:type w:val="oddPage"/></w:sectPr></w:pPr></w:p>
        <w:p><w:r><w:t>Two</w:t></w:r></w:p>
        </w:body></w:document>"#,
        ParserConfig::default(),
    );
    let pages = container
        .iter_pages()
        .unwrap()
        .collect::<Result<Vec<_>>>()
        .unwrap();

    assert_eq!(pages.len(), 3);
    assert!(pages[1].elements.is_empty());
    assert!(pages[2].to_markdown().unwrap().contains("Two"));
}

#[test]
fn even_section_start_and_continuous_sections_have_correct_boundaries() {
    let container = docx_with_document_xml(
        r#"<w:document><w:body>
        <w:p><w:r><w:t>One</w:t><w:br w:type="page"/></w:r></w:p>
        <w:p><w:r><w:t>Two</w:t></w:r><w:pPr><w:sectPr><w:type w:val="evenPage"/></w:sectPr></w:pPr></w:p>
        <w:p><w:r><w:t>Three</w:t></w:r><w:pPr><w:sectPr><w:type w:val="continuous"/></w:sectPr></w:pPr></w:p>
        <w:p><w:r><w:t>Four</w:t></w:r></w:p>
        </w:body></w:document>"#,
        ParserConfig::default(),
    );
    let pages = container
        .iter_pages()
        .unwrap()
        .collect::<Result<Vec<_>>>()
        .unwrap();

    assert_eq!(pages.len(), 4);
    assert!(pages[2].elements.is_empty());
    let last = pages[3].to_markdown().unwrap();
    assert!(last.contains("Three") && last.contains("Four"));
}

#[test]
fn docx_page_break_before_is_inherited_from_paragraph_styles() {
    let document = br#"<w:document><w:body>
        <w:p><w:r><w:t>One</w:t></w:r></w:p>
        <w:p><w:pPr><w:pStyle w:val="Child"/></w:pPr><w:r><w:t>Two</w:t></w:r></w:p>
        </w:body></w:document>"#;
    let styles = br#"<w:styles>
        <w:style w:styleId="Base"><w:pPr><w:pageBreakBefore/></w:pPr></w:style>
        <w:style w:styleId="Child"><w:basedOn w:val="Base"/></w:style>
        </w:styles>"#;
    let bytes = archive(&[
        ("[Content_Types].xml", b"<Types/>"),
        ("word/document.xml", document),
        ("word/styles.xml", styles),
    ]);
    let container = DocumentContainer::from_bytes(bytes, ParserConfig::default()).unwrap();

    assert_eq!(container.iter_pages().unwrap().count(), 2);
}

#[test]
fn page_number_comment_can_be_disabled_without_affecting_full_markdown() {
    let config = ParserConfig::builder()
        .include_page_number_as_comment(false)
        .build();
    let container = docx_with_document_xml(
        "<w:document><w:body><w:p><w:r><w:t>One</w:t></w:r></w:p></w:body></w:document>",
        config,
    );
    let page = container.iter_pages().unwrap().next().unwrap().unwrap();

    assert!(!page.to_markdown().unwrap().contains("<!-- Page"));
    assert!(!container.convert_to_md().unwrap().contains("<!-- Page"));
}

#[test]
fn docx_pages_select_first_even_and_default_stories() {
    let relationships = br#"<Relationships>
        <Relationship Id="first" Target="header-first.xml" Type="header"/>
        <Relationship Id="even" Target="header-even.xml" Type="header"/>
        <Relationship Id="default" Target="header-default.xml" Type="header"/>
    </Relationships>"#;
    let document = br#"<w:document><w:body>
        <w:p><w:r><w:t>One</w:t><w:br w:type="page"/></w:r></w:p>
        <w:p><w:r><w:t>Two</w:t><w:br w:type="page"/></w:r></w:p>
        <w:p><w:r><w:t>Three</w:t></w:r></w:p>
        <w:sectPr><w:headerReference w:type="first" r:id="first"/>
        <w:headerReference w:type="even" r:id="even"/>
        <w:headerReference w:type="default" r:id="default"/><w:titlePg/></w:sectPr>
        </w:body></w:document>"#;
    let bytes = archive(&[
        ("[Content_Types].xml", b"<Types/>"),
        ("word/document.xml", document),
        ("word/_rels/document.xml.rels", relationships),
        (
            "word/settings.xml",
            b"<w:settings><w:evenAndOddHeaders/></w:settings>",
        ),
        (
            "word/header-first.xml",
            b"<w:hdr><w:p><w:r><w:t>First Header</w:t></w:r></w:p></w:hdr>",
        ),
        (
            "word/header-even.xml",
            b"<w:hdr><w:p><w:r><w:t>Even Header</w:t></w:r></w:p></w:hdr>",
        ),
        (
            "word/header-default.xml",
            b"<w:hdr><w:p><w:r><w:t>Default Header</w:t></w:r></w:p></w:hdr>",
        ),
    ]);
    let container = DocumentContainer::from_bytes(bytes, ParserConfig::default()).unwrap();
    let pages = container
        .iter_pages()
        .unwrap()
        .collect::<Result<Vec<_>>>()
        .unwrap();

    assert_eq!(pages.len(), 3);
    assert!(pages[0].to_markdown().unwrap().contains("First Header"));
    assert!(pages[1].to_markdown().unwrap().contains("Even Header"));
    assert!(pages[2].to_markdown().unwrap().contains("Default Header"));
}

#[test]
fn odt_pages_select_master_page_variants() {
    let content = br#"<office:document-content><office:automatic-styles>
        <style:style style:name="P" style:master-page-name="PageA">
        <style:paragraph-properties fo:break-before="page"/></style:style>
        </office:automatic-styles><office:body><office:text>
        <text:p text:style-name="P">One</text:p>
        <text:p text:style-name="P">Two</text:p>
        <text:p text:style-name="P">Three</text:p>
        </office:text></office:body></office:document-content>"#;
    let styles = br#"<office:document-styles><office:master-styles>
        <style:master-page style:name="PageA">
        <style:header><text:p>Default Header</text:p></style:header>
        <style:header-left><text:p>Even Header</text:p></style:header-left>
        <style:header-first><text:p>First Header</text:p></style:header-first>
        </style:master-page></office:master-styles></office:document-styles>"#;
    let bytes = archive(&[
        ("mimetype", b"application/vnd.oasis.opendocument.text"),
        ("content.xml", content),
        ("styles.xml", styles),
    ]);
    let container = DocumentContainer::from_bytes(bytes, ParserConfig::default()).unwrap();
    let pages = container
        .iter_pages()
        .unwrap()
        .collect::<Result<Vec<_>>>()
        .unwrap();

    assert_eq!(pages.len(), 3);
    assert!(pages[0].to_markdown().unwrap().contains("First Header"));
    assert!(pages[1].to_markdown().unwrap().contains("Even Header"));
    assert!(pages[2].to_markdown().unwrap().contains("Default Header"));
}

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

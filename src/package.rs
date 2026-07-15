use std::fs::File;
use std::io::{Cursor, Read};
use std::path::Path;

use crate::parser;
use crate::{Document, DocumentElement, DocumentFormat, Error, Metadata, ParserConfig, Result};

/// A detected DOCX or ODT package kept in memory for repeatable parsing.
#[derive(Debug, Clone)]
pub struct DocumentContainer {
    bytes: Vec<u8>,
    format: DocumentFormat,
    pub config: ParserConfig,
    metadata: Metadata,
}

impl DocumentContainer {
    /// Opens and parses a file from the given path using the specified parser configuration.
    ///
    /// # Parameters
    /// - `path`: The file path to read from. Accepts any type that implements the `AsRef<Path>` trait.
    /// - `config`: The configuration to use for parsing, provided as a `ParserConfig`.
    ///
    /// # Returns
    /// - `Result<Self>`: `Self` represents the type implementing this method:
    ///     - `Ok(Self)`
    ///     - `Err(std::io::Error)`
    pub fn open(path: impl AsRef<Path>, config: ParserConfig) -> Result<Self> {
        let bytes = std::fs::read(path)?;
        Self::from_bytes(bytes, config)
    }

    pub fn from_reader(mut reader: impl Read, config: ParserConfig) -> Result<Self> {
        let mut bytes = Vec::new();
        reader.read_to_end(&mut bytes)?;
        Self::from_bytes(bytes, config)
    }

    pub fn from_bytes(bytes: Vec<u8>, config: ParserConfig) -> Result<Self> {
        let format = detect(&bytes)?;
        let metadata = parser::metadata(&bytes, format)?;
        Ok(Self {
            bytes,
            format,
            config,
            metadata,
        })
    }

    /// Opens a document located at the specified path with the provided parser configuration
    /// and verifies that the document format matches the expected format.
    ///
    /// # Arguments
    ///
    /// * `path` - A type that can be referenced as a `Path` which points to the document to be opened.
    /// * `config` - A `ParserConfig` object that specifies configuration options for parsing the document.
    /// * `expected` - The `DocumentFormat` that the document is expected to conform to.
    ///
    /// # Returns
    ///
    /// * `Ok(Self)`: Returns an instance of the object if the document is successfully opened
    ///   and its format matches the expected format.
    /// * `Err(Error::FormatMismatch)`: Returns an error if the opened document's format does not
    ///   match the expected format.
    /// * `Err(_)`: Returns any other error that might occur during the opening process.
    ///
    /// # Errors
    ///
    /// - If the document cannot be opened or parsed with the given configuration.
    /// - If the format of the document does not match the expected format.
    pub fn open_as(
        path: impl AsRef<Path>,
        config: ParserConfig,
        expected: DocumentFormat,
    ) -> Result<Self> {
        let this = Self::open(path, config)?;
        if this.format != expected {
            return Err(Error::FormatMismatch {
                expected,
                actual: this.format,
            });
        }
        Ok(this)
    }

    pub fn format(&self) -> DocumentFormat {
        self.format
    }

    pub fn metadata(&self) -> &Metadata {
        &self.metadata
    }

    pub fn parse_all(&self) -> Result<Document> {
        parser::parse(&self.bytes, self.format, &self.config)
    }

    pub fn iter_blocks(&self) -> Result<BlockIterator> {
        let elements = parser::parse_main(&self.bytes, self.format, &self.config)?;
        Ok(BlockIterator(elements.into_iter()))
    }

    pub fn convert_to_md(&self) -> Result<String> {
        self.parse_all()?.to_markdown(&self.config)
    }
}

pub struct BlockIterator(std::vec::IntoIter<DocumentElement>);

impl Iterator for BlockIterator {
    type Item = Result<DocumentElement>;

    fn next(&mut self) -> Option<Self::Item> {
        self.0.next().map(Ok)
    }
}

/// A format-constrained DOCX wrapper.
#[derive(Debug, Clone)]
pub struct DocxContainer(DocumentContainer);

impl DocxContainer {
    pub fn open(path: impl AsRef<Path>, config: ParserConfig) -> Result<Self> {
        Ok(Self(DocumentContainer::open_as(
            path,
            config,
            DocumentFormat::Docx,
        )?))
    }

    pub fn from_reader(reader: impl Read, config: ParserConfig) -> Result<Self> {
        let inner = DocumentContainer::from_reader(reader, config)?;
        if inner.format != DocumentFormat::Docx {
            return Err(Error::FormatMismatch {
                expected: DocumentFormat::Docx,
                actual: inner.format,
            });
        }
        Ok(Self(inner))
    }
}

impl std::ops::Deref for DocxContainer {
    type Target = DocumentContainer;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

/// Detects the document format of a ZIP archive by contextual constraints.
///
/// A document is **DOCX** if it contains both `word/document.xml` and `[Content_Types].xml`.
/// A document is **ODT** is detected if it contains either `content.xml` or `mimetype` with the
/// value `application/vnd.oasis.opendocument.text`.
fn detect(bytes: &[u8]) -> Result<DocumentFormat> {
    let mut archive = zip::ZipArchive::new(Cursor::new(bytes))?;
    let mut names = Vec::with_capacity(archive.len());
    for index in 0..archive.len() {
        names.push(archive.by_index(index)?.name().replace('\\', "/"));
    }
    if names.iter().any(|v| v == "[Content_Types].xml")
        && names.iter().any(|v| v == "word/document.xml")
    {
        return Ok(DocumentFormat::Docx);
    }
    if names.iter().any(|v| v == "content.xml") {
        if let Ok(mut mimetype) = archive.by_name("mimetype") {
            let mut value = String::new();
            mimetype.read_to_string(&mut value)?;
            if value.trim() == "application/vnd.oasis.opendocument.text" {
                return Ok(DocumentFormat::Odt);
            }
        }
        if names.iter().any(|v| v == "META-INF/manifest.xml") {
            return Ok(DocumentFormat::Odt);
        }
    }
    Err(Error::UnknownFormat)
}

/// Reads a specific file (part) by its name from a ZIP archive represented by a byte slice.
///
/// This function attempts to locate and extract a file within a ZIP archive provided as raw bytes.
/// If the requested file is found, its content is returned as a `Vec<u8>`. If the file does
/// not exist, `None` is returned. Any other errors during reading or processing are propagated as `Err`.
///
/// # Arguments
///
/// * `bytes` - A byte slice containing the ZIP archive data.
/// * `name` - The name of the file to read from the ZIP archive.
///
/// # Returns
///
/// * `Ok(Some(Vec<u8>))` - File content is returned as `Vec<u8>`.
/// * `Ok(None)` - File with the specified name does not exist within the archive; an optional part is missing.
/// * `Err` - An Error occurs while processing the ZIP archive or reading the file; a necessary part is missing.
///
/// # Errors
///
/// This function will return an error if:
/// * The provided byte slice does not represent a valid ZIP archive.
/// * The archive cannot be opened or read due to corrupt data or unsupported formats.
/// * Any other IO-related issue occurs while attempting to read the file.
///
/// # Notes
///
/// Paths use `/` internally, independent of the operating system.
/// The package will be held in memory completely.
pub(crate) fn read_part(bytes: &[u8], name: &str) -> Result<Option<Vec<u8>>> {
    let mut archive = zip::ZipArchive::new(Cursor::new(bytes))?;
    match archive.by_name(name) {
        Ok(mut file) => {
            let mut out = Vec::with_capacity(file.size() as usize);
            file.read_to_end(&mut out)?;
            Ok(Some(out))
        }
        Err(zip::result::ZipError::FileNotFound) => Ok(None),
        Err(error) => Err(error.into()),
    }
}

/// Extracts and returns a list of file names from a ZIP archive provided as a byte slice (`bytes`).
///
/// This function reads a ZIP archive from the input byte slice and iterates through its contents
/// to collect the names of all files present in the archive.
///
/// # Arguments
///
/// * `bytes` - A byte slice representing the contents of a ZIP archive.
///
/// # Returns
///
/// * `Result<Vec<String>>`: containing a vector of file names.
/// * `Err`
///
/// # Errors
///
/// * The byte slice does not represent a valid ZIP archive.
/// * There is an issue accessing any file within the ZIP archive.
pub(crate) fn list_parts(bytes: &[u8]) -> Result<Vec<String>> {
    let mut archive = zip::ZipArchive::new(Cursor::new(bytes))?;
    let mut names = Vec::new();
    for i in 0..archive.len() {
        names.push(archive.by_index(i)?.name().to_owned());
    }
    Ok(names)
}

#[allow(dead_code)]
fn _assert_file_send(_: File) {}

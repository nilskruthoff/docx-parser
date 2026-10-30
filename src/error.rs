use std::path::PathBuf;

/// Error raised while reading the ZIP package of a document.
///
/// The error of the underlying ZIP library is available through
/// [`std::error::Error::source`], but its concrete type is not part of the
/// public API, so updating that library does not change this crate's API.
#[derive(Debug)]
pub struct ZipError(zip::result::ZipError);

impl std::fmt::Display for ZipError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(f)
    }
}

impl std::error::Error for ZipError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(&self.0)
    }
}

/// Error raised while parsing an XML part of a document.
///
/// The error of the underlying XML library is available through
/// [`std::error::Error::source`], but its concrete type is not part of the
/// public API, so updating that library does not change this crate's API.
#[derive(Debug)]
pub struct XmlError(quick_xml::Error);

impl std::fmt::Display for XmlError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(f)
    }
}

impl std::error::Error for XmlError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(&self.0)
    }
}

/// Errors returned by package parsing, conversion, and resource extraction.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum Error {
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
    #[error("invalid ZIP package: {0}")]
    Zip(#[from] ZipError),
    #[error("invalid XML in {part}: {source}")]
    Xml {
        part: String,
        #[source]
        source: XmlError,
    },
    #[error("invalid UTF-8 in package part {part}: {source}")]
    Utf8 {
        part: String,
        #[source]
        source: std::str::Utf8Error,
    },
    #[error("the package is neither DOCX nor ODT")]
    UnknownFormat,
    #[error("expected {expected:?}, found {actual:?}")]
    FormatMismatch {
        expected: crate::DocumentFormat,
        actual: crate::DocumentFormat,
    },
    #[error("required package part is missing: {0}")]
    MissingPart(String),
    #[error("invalid relationship target `{target}` owned by `{owner}`")]
    InvalidRelationship { owner: String, target: String },
    #[error("image resource is unavailable: {0}")]
    MissingImage(String),
    #[error("invalid image output path: {0}")]
    InvalidOutputPath(PathBuf),
    #[error("image conversion failed: {0}")]
    ImageConversion(String),
    #[error("conversion failed: {0}")]
    Conversion(String),
}

impl Error {
    /// Wraps an error of the ZIP library without exposing it through a public `From` impl.
    pub(crate) fn zip(error: zip::result::ZipError) -> Self {
        Self::Zip(ZipError(error))
    }

    /// Wraps an error of the XML library for the given package part.
    pub(crate) fn xml(part: impl Into<String>, error: impl Into<quick_xml::Error>) -> Self {
        Self::Xml {
            part: part.into(),
            source: XmlError(error.into()),
        }
    }
}

pub type Result<T> = std::result::Result<T, Error>;

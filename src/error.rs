use std::path::PathBuf;

/// Errors returned by package parsing, conversion, and resource extraction.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum Error {
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
    #[error("invalid ZIP package: {0}")]
    Zip(#[from] zip::result::ZipError),
    #[error("invalid XML in {part}: {source}")]
    Xml {
        part: String,
        #[source]
        source: quick_xml::Error,
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

pub type Result<T> = std::result::Result<T, Error>;

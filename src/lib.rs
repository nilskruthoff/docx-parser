//! Event-driven DOCX and ODT parsing and Markdown rendering.
//!
//! The crate deliberately keeps the package reader, format frontends, common
//! document model and renderer separate. It never builds an XML DOM.

mod config;
mod error;
mod model;
mod package;
mod parser;
mod render;

pub use config::{ImageHandlingMode, ImageMode, ParserConfig, ParserConfigBuilder};
pub use error::{Error, Result};
pub use model::*;
pub use package::{DocumentContainer, DocxContainer};

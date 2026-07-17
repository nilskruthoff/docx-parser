use std::path::PathBuf;

/// Selects how referenced images are represented after parsing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum ImageHandlingMode {
    /// Embed images as base64 `data:` URLs in Markdown.
    InMarkdown,
    /// Keep image bytes in the structured model and omit them from Markdown.
    Manually,
    /// Save images to `image_output_path` and emit absolute `file:` URLs.
    Save,
}

/// Backwards-friendly short name for [`ImageHandlingMode`].
pub type ImageMode = ImageHandlingMode;

/// Parser and renderer options.
#[derive(Debug, Clone)]
pub struct ParserConfig {
    pub extract_images: bool,
    pub compress_images: bool,
    pub quality: u8,
    pub image_handling_mode: ImageHandlingMode,
    pub image_output_path: Option<PathBuf>,
    pub include_document_metadata: bool,
    pub include_headers_footers: bool,
    pub include_footnotes: bool,
    pub include_endnotes: bool,
    pub include_comments: bool,
    /// Prefix page-local Markdown with `<!-- Page n -->`.
    ///
    /// This option is only used by [`crate::DocumentPage::to_markdown`].
    /// Full-document rendering remains unchanged.
    pub include_page_number_as_comment: bool,
}

impl Default for ParserConfig {
    fn default() -> Self {
        Self {
            extract_images: true,
            compress_images: true,
            quality: 80,
            image_handling_mode: ImageHandlingMode::InMarkdown,
            image_output_path: None,
            include_document_metadata: true,
            include_headers_footers: true,
            include_footnotes: true,
            include_endnotes: true,
            include_comments: false,
            include_page_number_as_comment: true,
        }
    }
}

impl ParserConfig {
    pub fn builder() -> ParserConfigBuilder {
        ParserConfigBuilder(Self::default())
    }
}

#[derive(Debug, Clone)]
pub struct ParserConfigBuilder(ParserConfig);

macro_rules! setter {
    ($name:ident, $field:ident, $ty:ty) => {
        pub fn $name(mut self, value: $ty) -> Self {
            self.0.$field = value;
            self
        }
    };
}

impl ParserConfigBuilder {
    setter!(extract_images, extract_images, bool);
    setter!(compress_images, compress_images, bool);
    setter!(quality, quality, u8);
    setter!(image_handling_mode, image_handling_mode, ImageHandlingMode);
    setter!(include_document_metadata, include_document_metadata, bool);
    setter!(include_headers_footers, include_headers_footers, bool);
    setter!(include_footnotes, include_footnotes, bool);
    setter!(include_endnotes, include_endnotes, bool);
    setter!(include_comments, include_comments, bool);
    setter!(
        include_page_number_as_comment,
        include_page_number_as_comment,
        bool
    );

    pub fn image_output_path(mut self, path: impl Into<PathBuf>) -> Self {
        self.0.image_output_path = Some(path.into());
        self
    }

    pub fn build(mut self) -> ParserConfig {
        self.0.quality = self.0.quality.min(100);
        self.0
    }
}

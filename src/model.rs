use std::collections::BTreeMap;
use std::path::Path;

use crate::{ParserConfig, Result};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum DocumentFormat {
    Docx,
    Odt,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DocumentMetadata {
    pub title: Option<String>,
    pub subject: Option<String>,
    pub author: Option<String>,
    pub last_modified_by: Option<String>,
    pub description: Option<String>,
    pub keywords: Vec<String>,
    pub created_at: Option<String>,
    pub modified_at: Option<String>,
    pub custom: BTreeMap<String, String>,
}

pub type Metadata = DocumentMetadata;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[non_exhaustive]
pub enum StoryType {
    #[default]
    Main,
    Header,
    Footer,
    Comment,
    Footnote,
    Endnote,
}

/// Represents the provenance or origin details of a specific piece of content.
///
/// This struct is used to capture metadata associated with a content segment,
/// such as its source, structure, and styles.
///
/// # Fields
/// - `source_part` (`String`):
///   The identifier or name of the content source associated with this piece of content.
///
/// - `story` (`StoryType`):
///   The type or category of story this content belongs to, represented as a `StoryType`.
///
/// - `sequence` (`usize`):
///   The sequence number indicating the order of this piece of content within its context.
///
/// - `section` (`usize`):
///   The section number where this piece of content resides.
///
/// - `paragraph_style` (`Option<String>`):
///   An optional string specifying the paragraph style applied to this content, if any.
///
/// - `character_styles` (`Vec<String>`):
///   A list of character styles applied to this content, represented as strings.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Provenance {
    pub source_part: String,
    pub story: StoryType,
    pub sequence: usize,
    pub section: usize,
    pub paragraph_style: Option<String>,
    pub character_styles: Vec<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TextFormat {
    pub bold: bool,
    pub italic: bool,
    pub underline: bool,
    pub strikethrough: bool,
    pub vertical_align: VerticalAlign,
    pub language: Option<String>,
    pub paragraph_style: Option<String>,
    pub character_style: Option<String>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum VerticalAlign {
    #[default]
    Baseline,
    Superscript,
    Subscript,
}

#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum Inline {
    Text {
        text: String,
        format: TextFormat,
    },
    Hyperlink {
        target: String,
        content: Vec<Inline>,
    },
    Image {
        resource_id: String,
        alt: String,
    },
    LineBreak,
    Bookmark {
        name: String,
    },
    CommentReference(String),
    FootnoteReference(String),
    EndnoteReference(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Paragraph {
    pub content: Vec<Inline>,
    pub provenance: Provenance,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Heading {
    pub level: u8,
    pub content: Vec<Inline>,
    pub provenance: Provenance,
}

/// The document title as identified by its paragraph style.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Title {
    pub content: Vec<Inline>,
    pub provenance: Provenance,
}

/// The document subtitle as identified by its paragraph style.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Subtitle {
    pub content: Vec<Inline>,
    pub provenance: Provenance,
}

/// A semantically quoted paragraph.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BlockQuote {
    pub content: Vec<Inline>,
    pub provenance: Provenance,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ListMarker {
    Bullet,
    Decimal,
    LowerLetter,
    UpperLetter,
    LowerRoman,
    UpperRoman,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ListItem {
    pub blocks: Vec<DocumentElement>,
    pub children: Vec<List>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct List {
    pub marker: ListMarker,
    pub level: u8,
    pub start: u32,
    pub items: Vec<ListItem>,
    pub provenance: Provenance,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TableCell {
    pub blocks: Vec<DocumentElement>,
    pub row_span: usize,
    pub column_span: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Table {
    pub rows: Vec<Vec<TableCell>>,
    pub provenance: Provenance,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImageElement {
    pub resource_id: String,
    pub alt: String,
    pub provenance: Provenance,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnknownElement {
    pub name: String,
    pub visible_content: Vec<Inline>,
    pub provenance: Provenance,
}

#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum DocumentElement {
    Paragraph(Paragraph),
    Heading(Heading),
    Title(Title),
    Subtitle(Subtitle),
    BlockQuote(BlockQuote),
    List(List),
    Table(Table),
    Image(ImageElement),
    Unknown(UnknownElement),
}

impl DocumentElement {
    pub fn provenance(&self) -> &Provenance {
        match self {
            Self::Paragraph(v) => &v.provenance,
            Self::Heading(v) => &v.provenance,
            Self::Title(v) => &v.provenance,
            Self::Subtitle(v) => &v.provenance,
            Self::BlockQuote(v) => &v.provenance,
            Self::List(v) => &v.provenance,
            Self::Table(v) => &v.provenance,
            Self::Image(v) => &v.provenance,
            Self::Unknown(v) => &v.provenance,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HeaderFooterKind {
    First,
    Even,
    Default,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Story {
    pub kind: Option<HeaderFooterKind>,
    pub used_by_sections: Vec<usize>,
    pub elements: Vec<DocumentElement>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Annotation {
    pub id: String,
    pub author: Option<String>,
    pub date: Option<String>,
    pub elements: Vec<DocumentElement>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Section {
    pub index: usize,
    pub header_references: Vec<String>,
    pub footer_references: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImageResource {
    pub id: String,
    pub part_path: String,
    pub media_type: String,
    pub data: Vec<u8>,
}

impl ImageResource {
    pub fn base64(&self) -> String {
        use base64::Engine;
        base64::engine::general_purpose::STANDARD.encode(&self.data)
    }
}

/// Represents a structured document containing various elements, metadata, and resources.
///
/// # Fields
///
/// * `elements` - A vector of `DocumentElement`s, which represents the main content of the document.
/// * `sections` - A vector of `Section`s, which defines logical divisions of the document (e.g., chapters, sections).
/// * `metadata` - Metadata associated with the document, such as title, author, and other descriptive information.
/// * `headers` - A map of header IDs (`String`) to `Story`s, which represent content displayed in the document's headers.
/// * `footers` - A map of footer IDs (`String`) to `Story`s, which represent content displayed in the document's footers.
/// * `comments` - A vector of `Annotation`s providing notes or feedback on the document's content.
/// * `footnotes` - A vector of `Annotation`s that are used for reference or commentary in the document, typically displayed at the bottom of a page.
/// * `endnotes` - A vector of `Annotation`s similar to footnotes, but listed at the end of the document.
/// * `images` - A map of image IDs (`String`) to `ImageResource`s, which represent images included in the document.
#[derive(Debug, Clone, Default)]
pub struct Document {
    pub elements: Vec<DocumentElement>,
    pub sections: Vec<Section>,
    pub metadata: Metadata,
    pub headers: BTreeMap<String, Story>,
    pub footers: BTreeMap<String, Story>,
    pub comments: Vec<Annotation>,
    pub footnotes: Vec<Annotation>,
    pub endnotes: Vec<Annotation>,
    pub images: BTreeMap<String, ImageResource>,
}

impl Document {
    pub fn to_markdown(&self, config: &ParserConfig) -> Result<String> {
        crate::render::render(self, config)
    }

    /// Save all loaded image resources. Existing files are replaced.
    pub fn extract_images(&self, directory: impl AsRef<Path>) -> Result<Vec<std::path::PathBuf>> {
        crate::render::extract_images(self, directory.as_ref())
    }
}

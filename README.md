[![Crates.io](https://img.shields.io/crates/v/docx-to-md.svg)](https://crates.io/crates/docx-to-md)
[![tests](https://github.com/nilskruthoff/docx-parser/actions/workflows/ci.yml/badge.svg)](https://github.com/nilskruthoff/docx-parser/actions/workflows/ci.yml)
[![codecov](https://codecov.io/gh/nilskruthoff/docx-parser/branch/main/graph/badge.svg)](https://codecov.io/gh/nilskruthoff/docx-parser)
![License](https://img.shields.io/crates/l/docx-to-md.svg)
[![dependency status](https://deps.rs/repo/github/nilskruthoff/docx-parser/status.svg)](https://deps.rs/repo/github/nilskruthoff/docx-parser)
[![Documentation](https://docs.rs/docx-to-md/badge.svg)](https://docs.rs/docx-to-md)

# docx-to-md

`docx-to-md` is a Rust 2024 library that reads DOCX and ODT packages with a namespace-tolerant pull parser. It exposes a structured
document model and renders that model to Markdown.

The parser does not evaluate fields, macros, track changes, charts, embedded
objects, content controls, or page layout.

---

## 🚀 Features

- **DOCX and ODT support:** Detects and reads Microsoft Word `.docx` and OpenDocument Text `.odt` packages.
- **Markdown and structured output:** Renders documents to Markdown and exposes a reusable document model for custom processing.
- **Document blocks:** Parses titles, subtitles, paragraphs, headings, block quotes, nested ordered and unordered lists, and tables.
- **Inline content and formatting:** Preserves text, hyperlinks, line breaks, bold, italic, underline, strikethrough, superscript, and subscript where supported by the source format.
- **Images:** Loads referenced image resources and can embed them as base64 `data:` URLs, retain them for custom handling, or save and link them with absolute `file:` URLs.
- **Annotations and stories:** Optionally includes DOCX and ODT headers, footers, comments, footnotes, endnotes, and section associations.
- **Metadata and provenance:** Provides common document metadata as well as source-part, story, section, and style information for parsed elements.
---

## 📌 Quick start

### Minimal example
The `DocumentContainer` struct provides a convenient interface to the
document model.

```rust
use docx_to_md::{DocumentContainer, ParserConfig};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let package = DocumentContainer::open("report.docx", ParserConfig::default())?;
    let markdown = package.convert_to_md()?;
    std::fs::write("report.md", markdown)?;
    Ok(())
}
```

### Metadata handling
The `DocumentContainer` struct also provides access to the document metadata.
Metadata can be read without parsing the main story:

```rust
use docx_to_md::{DocumentContainer, ParserConfig};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let package = DocumentContainer::open("report.docx", ParserConfig::default())?;
    println!("{:?}", package.metadata().title);
    Ok(())
}
```

### Parsing blocks
The `DocumentContainer` struct also provides access to the main story blocks.

`iter_blocks` uses the same event parser as `parse_all` and returns main-story
blocks in source order. `parse_all` additionally resolves headers, footers,
comments, footnotes, endnotes, sections, and image resources.

```rust
use docx_to_md::{DocumentContainer, ParserConfig};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let package = DocumentContainer::open("report.docx", ParserConfig::default())?;
    for block in package.iter_blocks()? {
        println!("{:?}", block?);
    }
    Ok(())
}
```

### Image handling
Images default to embedded data URLs. To save and link them instead:

```rust
use docx_to_md::{DocumentContainer, ImageHandlingMode, ParserConfig};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let config = ParserConfig::builder()
        .image_handling_mode(ImageHandlingMode::Save)
        .image_output_path("output/images")
        .build();
    let package = DocumentContainer::open("report.docx", config)?;
    let markdown = package.convert_to_md()?;
    std::fs::write("output/report.md", markdown)?;
    Ok(())
}
```

For a separate extraction workflow, parse once and call
`document.extract_images("output/images")`; `ImageResource::base64()` is also
available for custom storage.

> for more usage examples, see the [examples](https://github.com/nilskruthoff/docx-parser/tree/master/examples) directory.
---

## ⚙️ Config Parameters

| Parameter                   | Type                | Default      | Description                                                                                                                        |
|-----------------------------|---------------------|--------------|------------------------------------------------------------------------------------------------------------------------------------|
| `extract_images`            | `bool`              | `true`       | Whether referenced image resources are loaded into the document. When `false`, image references and image data are omitted.        |
| `compress_images`           | `bool`              | `true`       | Whether non-SVG images are converted to JPEG while loading; this also affects images saved or accessed from the document model.    |
| `quality`                   | `u8`                | `80`         | JPEG quality used when `compress_images` is enabled. The builder caps values at `100`; higher values favor quality over file size. |
| `image_handling_mode`       | `ImageHandlingMode` | `InMarkdown` | Determines whether loaded images are embedded in Markdown, omitted from it, or saved and linked with `file:` URLs.                 |
| `image_output_path`         | `Option<PathBuf>`   | `None`       | Directory used by `ImageHandlingMode::Save`; required in save mode.                                                                |
| `include_document_metadata` | `bool`              | `true`       | Whether document metadata is rendered as an HTML comment at the beginning of the Markdown.                                         |
| `include_headers_footers`   | `bool`              | `true`       | Whether headers and footers are parsed and rendered in a `Headers and Footers` section.                                            |
| `include_footnotes`         | `bool`              | `true`       | Whether footnotes are parsed and rendered as Markdown footnote references and definitions.                                         |
| `include_endnotes`          | `bool`              | `true`       | Whether endnotes are parsed and rendered as Markdown footnote references and definitions.                                          |
| `include_comments`          | `bool`              | `false`      | Whether comments are parsed, referenced inline, and rendered in a `Comments` blockquote section.                                   |
<br/>

#### Member of `ImageHandlingMode`
| Member       | Description                                                                                                 |
|--------------|-------------------------------------------------------------------------------------------------------------|
| `InMarkdown` | Images are embedded in Markdown image syntax as base64 `data:` URLs.                                        |
| `Manually`   | Image bytes remain in the structured document model but image markup is omitted from Markdown.              |
| `Save`       | Images are saved to `image_output_path` and referenced in Markdown image syntax with absolute `file:` URLs. |

---

## 📦 Installation

Include the following line in your Cargo.toml dependencies section:

```toml
[dependencies]
docx-to-md = "0.1.0"
```

---

## 📜 License
This project is licensed under the [MIT-License](https://github.com/nilskruthoff/pptx-parser/blob/master/LICENCE-MIT)
and [Apache 2.0-Licence](https://github.com/nilskruthoff/pptx-parser/blob/master/LICENSE-APACHE).

Feel free to contribute or suggest improvements! 😊

---

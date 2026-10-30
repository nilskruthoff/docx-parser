# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Changed

- `Error::Zip` now wraps the `ZipError` of `zip` 9 instead of `zip` 8.

### Security

- Upgraded `zip` from 8.6.0 to 9.0.5, which fixes the advisories GHSA-m665-v7pm-7grj, GHSA-q7v9-x93c-xqv3, GHSA-8hxf-qr8p-62x4, GHSA-hjqx-j4rw-g7xv, and GHSA-9hj8-73fh-9963.

## [0.1.0] - 2026-07-17

### Added

- Automatic detection and namespace-tolerant, event-driven parsing of DOCX and ODT packages without building an XML DOM.
- High-level conversion through `DocumentContainer`, format-constrained DOCX access through `DocxContainer`, and full structured parsing through `parse_all()`.
- Markdown rendering through `convert_to_md()` and `Document::to_markdown()` with stable escaping for text, URLs, tables, comments, and Markdown control characters.
- A public, format-independent document model with source provenance and ordered elements for custom processing and retrieval workflows.
- Semantic blocks for titles, subtitles, headings, paragraphs, block quotes, nested ordered and unordered lists, tables, images, and unknown visible content.
- Inline support for bold, italic, underline, strikethrough, superscript, subscript, languages, hyperlinks, bookmarks, line breaks, images, and annotation references.
- Document metadata including title, subject, author, description, keywords, creation and modification timestamps, and custom fields.
- DOCX and ODT sections, headers, footers, comments, footnotes, and endnotes, including configurable Markdown output for supplementary stories.
- DOCX First/Even/Default header and footer selection with section inheritance, `titlePg`, Even/Odd settings, and odd/even section starts.
- ODT master-page resolution with standard, first, and left/even header and footer variants.
- Configurable image handling through embedded base64 `data:` URLs, manual `ImageResource` access, or extracted files referenced by absolute `file:` URLs.
- Optional JPEG compression and quality control for loaded image resources while preserving SVG resources.
- Main-story block iteration through `iter_blocks()`.
- Lazy, block-aligned page streaming through `iter_pages()` and `iter_pages_with(PageBoundaryMode)` for DOCX and ODT.
- Explicit, rendered-hint, and combined page-boundary modes covering DOCX page breaks, inherited `pageBreakBefore`, section changes, `lastRenderedPageBreak`, ODT inherited page breaks, and `soft-page-break`.
- Self-contained `DocumentPage` values with one-based page numbers, selected header/footer stories, referenced annotations, page-local images, Markdown rendering, and image extraction.
- Optional `<!-- Page n -->` comments in page-local Markdown through `include_page_number_as_comment`, enabled by default without changing full-document Markdown.
- Fused page-iterator error behavior: valid pages can be consumed before a later malformed main-story block reports its error once.
- Configurable parsing and rendering through `ParserConfig` and `ParserConfigBuilder`.
- Examples for conversion, metadata access, structured parsing, block and page iteration, and automatic or manual image extraction.

# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.1.0] - 2026-07-15

### Added

- Parsing and automatic format detection for DOCX and ODT packages.
- Markdown rendering and a public structured document model for custom processing.
- Support for paragraphs, headings, nested lists, tables, hyperlinks, inline formatting, and images.
- Access to document metadata, source provenance, sections, headers, footers, comments, footnotes, and endnotes.
- Configurable image handling: embedded base64 `data:` URLs, manual access through the document model, or saving files with absolute `file:` URLs.
- Configurable parsing and rendering through `ParserConfig` and `ParserConfigBuilder`.
- Examples for conversion, metadata access, structured parsing, block iteration, and image extraction.

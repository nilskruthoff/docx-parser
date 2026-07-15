use std::fmt::Write;
use std::path::{Path, PathBuf};

use crate::model::*;
use crate::{Error, ImageHandlingMode, ParserConfig, Result};

#[derive(Clone, Copy, PartialEq, Eq)]
enum MarkdownContext {
    Flow,
    ListItem,
    TableCell,
    Quote,
}

/// Renders a `Document` into a Markdown-formatted string based on the provided `ParserConfig`.
///
/// # Parameters
///
/// - `document`: A reference to the `Document` to be rendered. This contains the main content
///   and associated metadata (headers, footers, footnotes, endnotes, comments, etc.) to be processed.
/// - `config`: A reference to the `ParserConfig` that determines how the rendering should be
///   performed (e.g., what elements to include, how images should be handled).
///
/// # Returns
///
/// - `Ok(String)`: The rendered Markdown content as a `String` if the operation completes successfully.
/// - `Err(Error)`: An error if an issue occurs during rendering, such as missing required configuration data
///   (e.g., `image_output_path` for image extraction).
///
/// # See Also
///
/// - [`Document`] and its associated fields for understanding the structure of the input.
/// - [`ParserConfig`] to configure rendering options.
pub(crate) fn render(document: &Document, config: &ParserConfig) -> Result<String> {
    if config.image_handling_mode == ImageHandlingMode::Save {
        let directory = config.image_output_path.as_deref().ok_or_else(|| {
            Error::InvalidOutputPath(PathBuf::from("image_output_path is required"))
        })?;
        extract_images(document, directory)?;
    }
    let mut out = String::new();
    if config.include_document_metadata {
        render_metadata(&document.metadata, &mut out);
    }
    render_elements(
        &document.elements,
        document,
        config,
        &mut out,
        0,
        MarkdownContext::Flow,
    )?;
    if config.include_headers_footers
        && (!document.headers.is_empty() || !document.footers.is_empty())
    {
        out.push_str("\n## Headers and Footers\n\n");
        for (id, story) in document.headers.iter().chain(&document.footers) {
            let label = if document.headers.contains_key(id) {
                "Header"
            } else {
                "Footer"
            };
            let kind = story.kind.map_or("default", |kind| match kind {
                HeaderFooterKind::First => "first",
                HeaderFooterKind::Even => "even",
                HeaderFooterKind::Default => "default",
            });
            writeln!(out, "### {label} ({kind})").expect("writing String cannot fail");
            if !story.used_by_sections.is_empty() {
                writeln!(
                    out,
                    "\n_Sections: {}_",
                    story
                        .used_by_sections
                        .iter()
                        .map(usize::to_string)
                        .collect::<Vec<_>>()
                        .join(", ")
                )
                .expect("writing String cannot fail");
            }
            out.push('\n');
            render_elements(
                &story.elements,
                document,
                config,
                &mut out,
                0,
                MarkdownContext::Flow,
            )?;
        }
    }
    if config.include_footnotes {
        render_notes(&document.footnotes, "fn", document, config, &mut out)?;
    }
    if config.include_endnotes {
        render_notes(&document.endnotes, "en", document, config, &mut out)?;
    }
    if config.include_comments && !document.comments.is_empty() {
        out.push_str("\n> **Comments**\n>\n");
        for comment in &document.comments {
            let mut body = String::new();
            render_elements(
                &comment.elements,
                document,
                config,
                &mut body,
                0,
                MarkdownContext::Quote,
            )?;
            let author = comment.author.as_deref().unwrap_or("Unknown");
            writeln!(
                out,
                "> **{} — {}:** {}",
                escape_text(&comment.id, MarkdownContext::Quote),
                escape_text(author, MarkdownContext::Quote),
                body.trim().replace('\n', "\n> ")
            )
            .expect("writing String cannot fail");
        }
    }
    Ok(out.trim_end().to_owned() + "\n")
}

fn render_metadata(metadata: &Metadata, out: &mut String) {
    let mut fields = Vec::new();
    macro_rules! field {
        ($name:literal, $value:expr) => {
            if let Some(value) = $value {
                fields.push(format!("{}: {}", $name, sanitize_comment(value)));
            }
        };
    }
    field!("title", metadata.title.as_deref());
    field!("subject", metadata.subject.as_deref());
    field!("author", metadata.author.as_deref());
    field!("last-modified-by", metadata.last_modified_by.as_deref());
    field!("created", metadata.created_at.as_deref());
    field!("modified", metadata.modified_at.as_deref());
    if !metadata.keywords.is_empty() {
        fields.push(format!(
            "keywords={}",
            sanitize_comment(&metadata.keywords.join(", "))
        ));
    }
    if !fields.is_empty() {
        writeln!(out, "<!-- Document Metadata | {} -->\n", fields.join(" | "))
            .expect("writing String cannot fail");
    }
}

fn sanitize_comment(value: &str) -> String {
    value
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .replace("--", "—")
        .replace('>', "&gt;")
}

fn render_elements(
    elements: &[DocumentElement],
    document: &Document,
    config: &ParserConfig,
    out: &mut String,
    indent: usize,
    context: MarkdownContext,
) -> Result<()> {
    for element in elements {
        match element {
            DocumentElement::Paragraph(paragraph) => {
                out.push_str(&" ".repeat(indent));
                render_inline(&paragraph.content, document, config, out, context)?;
                out.push_str("\n\n");
            }
            DocumentElement::Heading(heading) => {
                out.push_str(&"#".repeat(heading.level.clamp(1, 6) as usize));
                out.push(' ');
                render_inline(
                    &heading.content,
                    document,
                    config,
                    out,
                    MarkdownContext::Flow,
                )?;
                out.push_str("\n\n");
            }
            DocumentElement::List(list) => render_list(list, document, config, out, indent)?,
            DocumentElement::Table(table) => render_table(table, document, config, out)?,
            DocumentElement::Image(image) => {
                render_image(&image.resource_id, &image.alt, document, config, out)?;
                out.push_str("\n\n");
            }
            DocumentElement::Unknown(unknown) => {
                render_inline(&unknown.visible_content, document, config, out, context)?;
                out.push_str("\n\n");
            }
        }
    }
    Ok(())
}

fn render_inline(
    values: &[Inline],
    document: &Document,
    config: &ParserConfig,
    out: &mut String,
    context: MarkdownContext,
) -> Result<()> {
    for value in values {
        match value {
            Inline::Text { text, format } => render_formatted(text, format, out, context),
            Inline::Hyperlink { target, content } => {
                out.push('[');
                render_inline(content, document, config, out, context)?;
                write!(out, "]({})", escape_url(target)).expect("writing String cannot fail");
            }
            Inline::Image { resource_id, alt } => {
                render_image(resource_id, alt, document, config, out)?
            }
            Inline::LineBreak => out.push_str("  \n"),
            Inline::Bookmark { .. } => {}
            Inline::CommentReference(id) => {
                if config.include_comments {
                    write!(
                        out,
                        "<sup>[comment {}]</sup>",
                        escape_text(id, MarkdownContext::Flow)
                    )
                    .expect("writing String cannot fail");
                }
            }
            Inline::FootnoteReference(id) => {
                if config.include_footnotes {
                    write!(out, "[^fn-{}]", escape_id(id)).expect("writing String cannot fail");
                }
            }
            Inline::EndnoteReference(id) => {
                if config.include_endnotes {
                    write!(out, "[^en-{}]", escape_id(id)).expect("writing String cannot fail");
                }
            }
        }
    }
    Ok(())
}

fn render_formatted(text: &str, format: &TextFormat, out: &mut String, context: MarkdownContext) {
    let escaped = escape_text(text, context);
    let content_start = escaped
        .find(|character: char| !character.is_whitespace())
        .unwrap_or(escaped.len());
    let content_end = escaped
        .rfind(|character: char| !character.is_whitespace())
        .map(|index| index + escaped[index..].chars().next().map_or(0, char::len_utf8))
        .unwrap_or(content_start);
    let mut value = escaped[content_start..content_end].to_owned();
    if value.is_empty() {
        out.push_str(&escaped);
        return;
    }
    if format.vertical_align == VerticalAlign::Superscript {
        value = format!("<sup>{value}</sup>");
    }
    if format.vertical_align == VerticalAlign::Subscript {
        value = format!("<sub>{value}</sub>");
    }
    if format.strikethrough {
        value = format!("~~{value}~~");
    }
    if format.underline {
        value = format!("<u>{value}</u>");
    }
    if format.italic {
        value = format!("*{value}*");
    }
    if format.bold {
        value = format!("**{value}**");
    }
    out.push_str(&escaped[..content_start]);
    out.push_str(&value);
    out.push_str(&escaped[content_end..]);
}

fn render_list(
    list: &List,
    document: &Document,
    config: &ParserConfig,
    out: &mut String,
    indent: usize,
) -> Result<()> {
    for (offset, item) in list.items.iter().enumerate() {
        out.push_str(&" ".repeat(indent));
        let marker = match list.marker {
            ListMarker::Bullet => "- ".into(),
            _ => format!("{}. ", list.start as usize + offset),
        };
        out.push_str(&marker);
        let mut rendered = String::new();
        render_elements(
            &item.blocks,
            document,
            config,
            &mut rendered,
            0,
            MarkdownContext::ListItem,
        )?;
        let rendered = rendered.trim();
        out.push_str(&rendered.replace('\n', &format!("\n{}  ", " ".repeat(indent))));
        out.push('\n');
        for child in &item.children {
            render_list(child, document, config, out, indent + 2)?;
        }
    }
    out.push('\n');
    Ok(())
}

fn render_table(
    table: &Table,
    document: &Document,
    config: &ParserConfig,
    out: &mut String,
) -> Result<()> {
    if table.rows.is_empty() {
        return Ok(());
    }
    let columns = table.rows.iter().map(Vec::len).max().unwrap_or(0);
    render_table_row(&table.rows[0], columns, document, config, out)?;
    out.push('|');
    for _ in 0..columns {
        out.push_str(" --- |");
    }
    out.push('\n');
    for row in table.rows.iter().skip(1) {
        render_table_row(row, columns, document, config, out)?;
    }
    out.push('\n');
    Ok(())
}

fn render_table_row(
    row: &[TableCell],
    columns: usize,
    document: &Document,
    config: &ParserConfig,
    out: &mut String,
) -> Result<()> {
    out.push('|');
    for index in 0..columns {
        out.push(' ');
        if let Some(cell) = row.get(index) {
            let mut value = String::new();
            render_elements(
                &cell.blocks,
                document,
                config,
                &mut value,
                0,
                MarkdownContext::TableCell,
            )?;
            out.push_str(&value.trim().replace("  \n", "<br>").replace('\n', "<br>"));
        }
        out.push_str(" |");
    }
    out.push('\n');
    Ok(())
}

fn render_image(
    id: &str,
    alt: &str,
    document: &Document,
    config: &ParserConfig,
    out: &mut String,
) -> Result<()> {
    if !config.extract_images || config.image_handling_mode == ImageHandlingMode::Manually {
        return Ok(());
    }
    let resource = document
        .images
        .get(id)
        .ok_or_else(|| Error::MissingImage(id.into()))?;
    let url = match config.image_handling_mode {
        ImageHandlingMode::InMarkdown => {
            format!("data:{};base64,{}", resource.media_type, resource.base64())
        }
        ImageHandlingMode::Save => {
            let directory = config.image_output_path.as_ref().ok_or_else(|| {
                Error::InvalidOutputPath(PathBuf::from("image_output_path is required"))
            })?;
            let directory = directory
                .canonicalize()
                .map_err(|_| Error::InvalidOutputPath(directory.clone()))?;
            let path = image_output_path(&directory, resource)?;
            url::Url::from_file_path(&path)
                .map_err(|()| Error::InvalidOutputPath(path))?
                .to_string()
        }
        ImageHandlingMode::Manually => return Ok(()),
    };
    write!(
        out,
        "![{}]({})",
        escape_text(alt, MarkdownContext::Flow),
        escape_url(&url)
    )
    .expect("writing String cannot fail");
    Ok(())
}

fn render_notes(
    notes: &[Annotation],
    prefix: &str,
    document: &Document,
    config: &ParserConfig,
    out: &mut String,
) -> Result<()> {
    if notes.is_empty() {
        return Ok(());
    }
    out.push('\n');
    for note in notes {
        let mut body = String::new();
        render_elements(
            &note.elements,
            document,
            config,
            &mut body,
            0,
            MarkdownContext::Flow,
        )?;
        let body = body.trim().replace('\n', "\n    ");
        writeln!(out, "[^{prefix}-{}]: {body}", escape_id(&note.id))
            .expect("writing String cannot fail");
    }
    Ok(())
}

pub(crate) fn extract_images(document: &Document, directory: &Path) -> Result<Vec<PathBuf>> {
    std::fs::create_dir_all(directory).map_err(|_| Error::InvalidOutputPath(directory.into()))?;
    let canonical = directory
        .canonicalize()
        .map_err(|_| Error::InvalidOutputPath(directory.into()))?;
    let mut paths = Vec::new();
    for resource in document.images.values() {
        let path = image_output_path(&canonical, resource)?;
        std::fs::write(&path, &resource.data)?;
        paths.push(path);
    }
    Ok(paths)
}

fn image_output_path(directory: &Path, resource: &ImageResource) -> Result<PathBuf> {
    let mut name = resource
        .part_path
        .trim_matches(['/', '\\'])
        .replace(['/', '\\', ':'], "_");
    if name.is_empty() || name == "." || name == ".." {
        return Err(Error::InvalidOutputPath(directory.into()));
    }
    if resource.media_type == "image/jpeg" {
        let stem = name
            .rsplit_once('.')
            .map_or(name.as_str(), |(stem, _)| stem);
        name = format!("{stem}.jpg");
    }
    Ok(directory.join(name))
}

fn escape_text(value: &str, context: MarkdownContext) -> String {
    let mut out = String::with_capacity(value.len());
    for (index, ch) in value.char_indices() {
        let block_marker = index == 0
            && context != MarkdownContext::TableCell
            && (matches!(ch, '#' | '>')
                || ((ch == '-' || ch == '+' || ch == '*')
                    && value[index + ch.len_utf8()..].starts_with(' ')));
        if matches!(ch, '\\' | '`' | '*' | '_' | '[' | ']' | '<' | '~' | '&')
            || (ch == '|' && context == MarkdownContext::TableCell)
            || block_marker
        {
            out.push('\\');
        }
        out.push(ch);
    }
    out
}

fn escape_url(value: &str) -> String {
    value
        .replace(' ', "%20")
        .replace('(', "%28")
        .replace(')', "%29")
}
fn escape_id(value: &str) -> String {
    value
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_'))
        .collect()
}

#[cfg(test)]
#[path = "../tests/unit/render.rs"]
mod tests;

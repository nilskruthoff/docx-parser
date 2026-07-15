use docx_to_md::{DocumentElement, Inline};

fn visit_inline(values: &[Inline], paragraphs: &mut usize, breaks: &mut usize, bold: &mut usize) {
    *paragraphs += 1;
    for value in values {
        match value {
            Inline::LineBreak => *breaks += 1,
            Inline::Text { format, .. } if format.bold => *bold += 1,
            Inline::Hyperlink { content, .. } => visit_inline(content, &mut 0, breaks, bold),
            _ => {}
        }
    }
}

fn visit(
    elements: &[DocumentElement],
    paragraphs: &mut usize,
    breaks: &mut usize,
    bold: &mut usize,
) {
    for element in elements {
        match element {
            DocumentElement::Paragraph(value) => {
                visit_inline(&value.content, paragraphs, breaks, bold)
            }
            DocumentElement::Heading(value) => {
                visit_inline(&value.content, paragraphs, breaks, bold)
            }
            DocumentElement::List(value) => {
                for item in &value.items {
                    visit(&item.blocks, paragraphs, breaks, bold);
                    for child in &item.children {
                        for child_item in &child.items {
                            visit(&child_item.blocks, paragraphs, breaks, bold);
                        }
                    }
                }
            }
            DocumentElement::Table(value) => {
                for row in &value.rows {
                    for cell in row {
                        visit(&cell.blocks, paragraphs, breaks, bold);
                    }
                }
            }
            _ => {}
        }
    }
}

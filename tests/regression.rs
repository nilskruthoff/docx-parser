use docx_to_md::{DocumentContainer, DocumentElement, Inline, ParserConfig};

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

#[test]
fn bachelorarbeit_regression() {
    let container = DocumentContainer::open(
        "tests/fixtures/Bachelorarbeit_Assistant_Plugins_Expose.docx",
        ParserConfig::default(),
    )
    .unwrap();
    let document = container.parse_all().unwrap();
    let mut paragraphs = 0;
    let mut breaks = 0;
    let mut bold = 0;
    visit(&document.elements, &mut paragraphs, &mut breaks, &mut bold);
    assert_eq!(paragraphs, 38);
    assert_eq!(breaks, 36);
    assert!(bold > 0);
}

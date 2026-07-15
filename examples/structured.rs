use docx_to_md::{DocumentContainer, DocumentElement, ParserConfig};

fn main() -> Result<(), docx_to_md::Error> {
    let Some(path) = std::env::args_os().nth(1) else {
        return Ok(());
    };
    let document = DocumentContainer::open(path, ParserConfig::default())?.parse_all()?;
    for element in document.elements {
        match element {
            DocumentElement::Heading(heading) => println!("heading level {}", heading.level),
            DocumentElement::Table(table) => println!("table with {} rows", table.rows.len()),
            other => println!("{other:?}"),
        }
    }
    Ok(())
}

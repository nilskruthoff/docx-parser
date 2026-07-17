use docx_to_md::{DocumentContainer, ParserConfig};

fn main() -> Result<(), docx_to_md::Error> {
    let Some(path) = std::env::args_os().nth(1) else {
        return Ok(());
    };
    let document = DocumentContainer::open(path, ParserConfig::default())?;
    for page in document.iter_pages()? {
        let page = page?;
        println!("{}", page.to_markdown()?);
    }
    Ok(())
}

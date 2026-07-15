use docx_to_md::{DocumentContainer, ParserConfig};

fn main() -> Result<(), docx_to_md::Error> {
    let Some(path) = std::env::args_os().nth(1) else {
        return Ok(());
    };
    let package = DocumentContainer::open(path, ParserConfig::default())?;
    println!("{:#?}", package.metadata());
    Ok(())
}

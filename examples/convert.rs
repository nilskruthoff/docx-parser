use docx_to_md::{DocumentContainer, ParserConfig};
use std::fs;

fn main() -> Result<(), docx_to_md::Error> {
    let Some(path) = std::env::args_os().nth(1) else {
        eprintln!("usage: cargo run --example convert \"<document.docx|document.odt>\"");
        return Ok(());
    };
    let package = DocumentContainer::open(path, ParserConfig::default())?;
    let markdown = package.convert_to_md()?;

    fs::write("output.md", markdown).expect("failed to write output.md");
    print!("{}", package.convert_to_md()?);
    Ok(())
}

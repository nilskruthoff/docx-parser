use docx_to_md::{DocumentContainer, ImageHandlingMode, ParserConfig};

fn main() -> Result<(), docx_to_md::Error> {
    let Some(path) = std::env::args_os().nth(1) else {
        return Ok(());
    };
    let config = ParserConfig::builder()
        .image_handling_mode(ImageHandlingMode::Save)
        .image_output_path("images")
        .build();
    println!(
        "{}",
        DocumentContainer::open(path, config)?.convert_to_md()?
    );
    Ok(())
}

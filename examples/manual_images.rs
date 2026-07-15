use docx_to_md::{DocumentContainer, ImageHandlingMode, ParserConfig};

fn main() -> Result<(), docx_to_md::Error> {
    let Some(path) = std::env::args_os().nth(1) else {
        return Ok(());
    };
    let config = ParserConfig::builder()
        .image_handling_mode(ImageHandlingMode::Manually)
        .build();
    let document = DocumentContainer::open(path, config)?.parse_all()?;
    for image in document.images.values() {
        println!("{}: {}", image.part_path, image.base64());
    }
    Ok(())
}

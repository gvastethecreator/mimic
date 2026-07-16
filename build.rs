#[cfg(windows)]
fn main() {
    println!("cargo:rerun-if-changed=assets/mimic.svg");
    let icon_path = render_windows_icon().expect("failed to render the Mimic Windows icon");
    let icon_path = icon_path
        .to_str()
        .expect("Windows resource icon path must be valid UTF-8");

    let mut resource = winres::WindowsResource::new();
    resource
        .set_icon(icon_path)
        .set("FileDescription", "Mimic Virtual Camera Studio")
        .set("ProductName", "Mimic")
        .set("CompanyName", "Mimic contributors")
        .set("LegalCopyright", "Copyright (c) 2026 Cristian")
        .set("OriginalFilename", "mimic.exe");
    resource
        .compile()
        .expect("failed to compile the Mimic Windows resources");
}

#[cfg(not(windows))]
fn main() {}

#[cfg(windows)]
fn render_windows_icon() -> Result<std::path::PathBuf, Box<dyn std::error::Error>> {
    use std::fs::File;

    let svg = std::fs::read("assets/mimic.svg")?;
    let tree = resvg::usvg::Tree::from_data(&svg, &resvg::usvg::Options::default())?;
    let mut directory = ico::IconDir::new(ico::ResourceType::Icon);

    for size in [16_u32, 24, 32, 48, 64, 128, 256] {
        let mut pixmap = resvg::tiny_skia::Pixmap::new(size, size)
            .ok_or_else(|| format!("could not allocate {size}x{size} icon surface"))?;
        let scale_x = size as f32 / tree.size().width();
        let scale_y = size as f32 / tree.size().height();
        resvg::render(
            &tree,
            resvg::tiny_skia::Transform::from_scale(scale_x, scale_y),
            &mut pixmap.as_mut(),
        );

        let mut rgba = pixmap.take();
        unpremultiply_rgba(&mut rgba);
        let image = ico::IconImage::from_rgba_data(size, size, rgba);
        directory.add_entry(ico::IconDirEntry::encode(&image)?);
    }

    let output = std::path::PathBuf::from(std::env::var_os("OUT_DIR").ok_or("OUT_DIR is unset")?)
        .join("mimic.ico");
    directory.write(File::create(&output)?)?;
    Ok(output)
}

#[cfg(windows)]
fn unpremultiply_rgba(bytes: &mut [u8]) {
    for pixel in bytes.chunks_exact_mut(4) {
        let alpha = u32::from(pixel[3]);
        if alpha == 0 || alpha == 255 {
            continue;
        }
        for channel in &mut pixel[..3] {
            *channel = ((u32::from(*channel) * 255 + alpha / 2) / alpha).min(255) as u8;
        }
    }
}

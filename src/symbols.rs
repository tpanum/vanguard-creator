use image::{imageops::FilterType, RgbaImage};

/// Return the bundled PNG bytes for a mana symbol, or None if unknown.
pub fn bundled_bytes(name: &str) -> Option<&'static [u8]> {
    match name {
        "W" => Some(include_bytes!("../assets/symbols/W.png")),
        "U" => Some(include_bytes!("../assets/symbols/U.png")),
        "B" => Some(include_bytes!("../assets/symbols/B.png")),
        "R" => Some(include_bytes!("../assets/symbols/R.png")),
        "G" => Some(include_bytes!("../assets/symbols/G.png")),
        "1" => Some(include_bytes!("../assets/symbols/1.png")),
        "2" => Some(include_bytes!("../assets/symbols/2.png")),
        "3" => Some(include_bytes!("../assets/symbols/3.png")),
        "4" => Some(include_bytes!("../assets/symbols/4.png")),
        "X" => Some(include_bytes!("../assets/symbols/X.png")),
        "T" => Some(include_bytes!("../assets/symbols/T.png")),
        _ => None,
    }
}

/// Load a mana symbol at the given display size.
/// Returns None if the symbol is unknown.
pub fn load(name: &str, size: u32) -> Option<RgbaImage> {
    let bytes = bundled_bytes(name)?;
    let img = image::load_from_memory(bytes)
        .expect("bundled symbol PNG is valid")
        .into_rgba8();
    Some(image::imageops::resize(&img, size, size, FilterType::Lanczos3))
}

/// Whether a symbol name is known (has a bundled PNG).
pub fn is_known(name: &str) -> bool {
    bundled_bytes(name).is_some()
}

/// Embedded font data bundled into the binary.
pub const NAME_DATA: &[u8] = include_bytes!("../assets/fonts/Fremont-Regular.ttf");
pub const BODY_DATA: &[u8] = include_bytes!("../assets/fonts/Mplantin.ttf");
pub const BODY_BOLD_DATA: &[u8] = include_bytes!("../assets/fonts/Mplantin-Bold.ttf");

/// Embedded card template image.
pub const TEMPLATE_DATA: &[u8] = include_bytes!("../template.png");

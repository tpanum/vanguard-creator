use image::RgbaImage;
use resvg::{tiny_skia, usvg};

use crate::bundle;

/// Convert a user-facing symbol notation to the bundle key used in the SVG archive.
///
/// The notation is the text captured inside `{…}` in ability text, e.g.:
///   "W"      → "W"      (white mana)
///   "W/U"    → "WU"     (hybrid white/blue)
///   "2/W"    → "2W"     (generic hybrid)
///   "W/P"    → "WP"     (Phyrexian white)
///   "W/U/P"  → "WUP"    (Phyrexian hybrid)
///   "HALF"   → "HALF"   (½ mana)
///   "T"      → "T"      (tap)
///
/// The SVG filenames in the bundle match Scryfall's URL slugs, which are the
/// symbol codes with all "/" characters removed.
fn notation_to_key(notation: &str) -> String {
    notation.replace('/', "")
}

/// Rasterize an SVG at the given square pixel size into an RGBA image.
fn rasterize(svg_bytes: &[u8], size: u32) -> Option<RgbaImage> {
    let opts = usvg::Options::default();
    let tree = usvg::Tree::from_data(svg_bytes, &opts).ok()?;
    let mut pixmap = tiny_skia::Pixmap::new(size, size)?;
    let sx = size as f32 / tree.size().width();
    let sy = size as f32 / tree.size().height();
    resvg::render(
        &tree,
        tiny_skia::Transform::from_scale(sx, sy),
        &mut pixmap.as_mut(),
    );
    RgbaImage::from_raw(size, size, pixmap.take())
}

/// Load a mana symbol at the given square display size.
///
/// `notation` is the text between `{` and `}` in the card's ability text.
/// Returns `None` if the symbol is unknown or cannot be rasterized.
pub fn load(notation: &str, size: u32) -> Option<RgbaImage> {
    let key = notation_to_key(notation);
    let svg = bundle::symbol_svg(&key)?;
    rasterize(svg, size)
}

/// Whether a symbol notation is known (has a bundled SVG).
pub fn is_known(notation: &str) -> bool {
    bundle::symbol_known(&notation_to_key(notation))
}

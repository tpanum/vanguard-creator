//! Render one card five times, once per gem colour, and crop the bottom bezel
//! out of each into a single strip:
//!
//!   cargo run --release --example gem_swatches
//!
//! Writes `assets/examples/gem_colors.png`, which README.md shows. The strip
//! is build output like the other sample images — regenerate it whenever the
//! gem transforms or the template change.
//!
//! Order is WUBRG: white, blue, black, red, green.

use image::{imageops, RgbaImage};
use vgc::{card::CardDef, fonts::Fonts, gem::GemColor, layout::DEFAULT, render};

const CARD: &str = "tests/cards/gerrard.yaml";
const OUT: &str = "assets/examples/gem_colors.png";

/// Window around the gem, in template coordinates — wide enough to carry the
/// bezel either side, so each swatch reads as part of a card rather than as a
/// free-floating sphere.
const WINDOW_W: u32 = 150;
const WINDOW_H: u32 = 70;
const ZOOM: u32 = 2;

fn main() {
    let mut card = CardDef::load(std::path::Path::new(CARD)).expect("loading card");
    let template = image::load_from_memory(vgc::bundle::TEMPLATE)
        .expect("template decodes")
        .to_rgba8();
    let artwork = image::open(&card.artwork).ok().map(|i| i.into_rgba8());
    let fonts = Fonts::load().expect("loading fonts");

    let (cw, ch) = (WINDOW_W * ZOOM, WINDOW_H * ZOOM);
    let mut strip = RgbaImage::new(cw * GemColor::ALL.len() as u32, ch);

    for (i, color) in GemColor::ALL.iter().enumerate() {
        card.color = *color;
        let img = render::render_card(&card, artwork.as_ref(), Some(&template), &fonts)
            .expect("rendering card");

        let (cx, cy) = DEFAULT.gem_center;
        let crop = imageops::crop_imm(
            &img,
            cx as u32 - WINDOW_W / 2,
            cy as u32 - WINDOW_H / 2,
            WINDOW_W,
            WINDOW_H,
        )
        .to_image();
        let zoomed = imageops::resize(&crop, cw, ch, imageops::FilterType::Lanczos3);
        imageops::overlay(&mut strip, &zoomed, i as i64 * cw as i64, 0);
        println!("{}", color.name());
    }

    strip.save(OUT).expect("writing strip");
    println!("\nwrote {OUT}");
}

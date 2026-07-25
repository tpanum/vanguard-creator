//! Render one card once per gem colour and crop the bottom bezel out of each,
//! as a visual check on `src/gem.rs`:
//!
//!   cargo run --release --example gem_swatches
//!
//! Writes `target/gem_swatches.png`. Top row is the five single colours in
//! WUBRG order; the two rows below are the ten Magic colour pairs, which is
//! where the left-to-right gradient is worth looking at closely — a pair whose
//! two colours are close in value (white/blue, black/green) is the case that
//! reads as one muddy stone if `BLEND_SPAN` is set too wide.
//!
//! This is a development tool, not build output: nothing in the repo embeds
//! its result.

use image::{imageops, RgbaImage};
use vgc::{
    card::CardDef,
    fonts::Fonts,
    gem::{Gem, GemColor},
    layout::DEFAULT,
    render,
};

const CARD: &str = "tests/cards/gerrard.yaml";
const OUT: &str = "target/gem_swatches.png";

/// The ten Magic colour pairs, in the canonical order the hybrid mana symbols
/// use — allied first, then enemy.
const PAIRS: [(GemColor, GemColor); 10] = [
    (GemColor::White, GemColor::Blue),
    (GemColor::Blue, GemColor::Black),
    (GemColor::Black, GemColor::Red),
    (GemColor::Red, GemColor::Green),
    (GemColor::Green, GemColor::White),
    (GemColor::White, GemColor::Black),
    (GemColor::Blue, GemColor::Red),
    (GemColor::Black, GemColor::Green),
    (GemColor::Red, GemColor::White),
    (GemColor::Green, GemColor::Blue),
];

/// Window around the gem, in template coordinates — wide enough to carry the
/// bezel either side, so each swatch reads as part of a card rather than as a
/// free-floating sphere.
const WINDOW_W: u32 = 150;
const WINDOW_H: u32 = 70;
const ZOOM: u32 = 2;
const COLS: u32 = 5;

fn main() {
    let mut card = CardDef::load(std::path::Path::new(CARD)).expect("loading card");
    let template = image::load_from_memory(vgc::bundle::TEMPLATE)
        .expect("template decodes")
        .to_rgba8();
    let artwork = image::open(&card.artwork).ok().map(|i| i.into_rgba8());
    let fonts = Fonts::load().expect("loading fonts");

    let gems: Vec<Gem> = GemColor::ALL
        .iter()
        .map(|c| Gem::Single(*c))
        .chain(PAIRS.iter().map(|(a, b)| Gem::Dual(*a, *b)))
        .collect();

    let (cw, ch) = (WINDOW_W * ZOOM, WINDOW_H * ZOOM);
    let rows = gems.len().div_ceil(COLS as usize) as u32;
    let mut sheet = RgbaImage::new(cw * COLS, ch * rows);

    for (i, gem) in gems.iter().enumerate() {
        card.color = *gem;
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
        let (col, row) = (i as u32 % COLS, i as u32 / COLS);
        imageops::overlay(&mut sheet, &zoomed, (col * cw) as i64, (row * ch) as i64);
        print!("{gem}  ");
    }

    std::fs::create_dir_all("target").expect("creating target/");
    sheet.save(OUT).expect("writing sheet");
    println!("\n\nwrote {OUT}");
}

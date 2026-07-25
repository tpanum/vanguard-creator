use ab_glyph::{FontRef, PxScale};
use anyhow::{bail, Context, Result};
use image::{imageops, RgbaImage};
use std::path::{Path, PathBuf};

use crate::{
    bundle,
    card::{self, CardDef},
    fonts::Fonts,
    gem,
    layout::{Layout, DEFAULT},
    meta, text,
};

const BLACK: [u8; 3] = [0, 0, 0];

pub fn run(
    paths: &[PathBuf],
    output: Option<&Path>,
    template: Option<&Path>,
    recursive: bool,
) -> Result<()> {
    let yaml_files = card::collect_yaml_files(paths, recursive)?;
    if yaml_files.is_empty() {
        bail!("no YAML card files found in the given paths");
    }

    let template_img = load_template(template)?;
    let fonts = Fonts::load()?;
    let multi = yaml_files.len() > 1;

    // A card the loader rejects is skipped so the rest of a batch still
    // renders, but the run as a whole fails — nothing was written for it.
    let mut refused = 0usize;

    for yaml_path in &yaml_files {
        let card = match CardDef::load(yaml_path) {
            Ok(c) => c,
            Err(e) => {
                // `{e:#}` prints the cause chain — without it a card rejected
                // for a missing or unknown field reports only "parsing YAML in
                // <path>", which names the file but not what is wrong with it.
                eprintln!("error: refusing {}: {e:#}", yaml_path.display());
                refused += 1;
                continue;
            }
        };

        let out_path = resolve_output(output, &card.name, yaml_path, multi);

        // Ensure output parent directory exists
        if let Some(parent) = out_path.parent() {
            if !parent.as_os_str().is_empty() {
                std::fs::create_dir_all(parent)
                    .with_context(|| format!("creating output directory {}", parent.display()))?;
            }
        }

        let artwork = load_artwork(&card);

        match render_card(&card, artwork.as_ref(), Some(&template_img), &fonts) {
            Ok(img) => {
                meta::save_with_version(&img, &out_path)?;
                println!("Saved: {}", out_path.display());
            }
            Err(e) => {
                eprintln!("warning: failed to render {}: {e:#}", card.name);
            }
        }
    }

    if refused > 0 {
        bail!("{refused} card definition(s) refused; see the errors above");
    }

    Ok(())
}

fn load_template(override_path: Option<&Path>) -> Result<RgbaImage> {
    if let Some(p) = override_path {
        return image::open(p)
            .with_context(|| format!("opening template {}", p.display()))
            .map(|i| i.into_rgba8());
    }
    image::load_from_memory(bundle::TEMPLATE)
        .context("loading embedded template")
        .map(|i| i.into_rgba8())
}

/// Load a card's artwork, downgrading any failure to a warning.
fn load_artwork(card: &CardDef) -> Option<RgbaImage> {
    if !card.artwork.exists() {
        eprintln!("warning: artwork not found: {}", card.artwork.display());
        return None;
    }
    match image::open(&card.artwork)
        .with_context(|| format!("opening artwork {}", card.artwork.display()))
    {
        Ok(img) => Some(img.into_rgba8()),
        Err(e) => {
            eprintln!("warning: {e}");
            None
        }
    }
}

fn resolve_output(
    output: Option<&Path>,
    card_name: &str,
    yaml_path: &Path,
    multi: bool,
) -> PathBuf {
    let safe_name = card::sanitize_filename(card_name);
    match output {
        None => {
            // Default: <card-name>.png next to the YAML file
            yaml_path
                .parent()
                .unwrap_or(Path::new("."))
                .join(format!("{safe_name}.png"))
        }
        Some(p) if p.is_dir() || (multi && p.extension().is_none_or(|e| e != "png")) => {
            p.join(format!("{safe_name}.png"))
        }
        Some(p) => p.to_owned(),
    }
}

/// Render a single card into an RGBA image.
///
/// `artwork` and `template` are optional pre-loaded images. Pass `None` for
/// `template` to render on a plain white canvas (useful for tests that compare
/// text placement against a clean mask without template frame noise).
pub fn render_card(
    card: &CardDef,
    artwork: Option<&RgbaImage>,
    template: Option<&RgbaImage>,
    fonts: &Fonts,
) -> Result<RgbaImage> {
    let layout = &DEFAULT;

    let (w, h) = template.map(|t| t.dimensions()).unwrap_or((718, 1024));
    let mut canvas = RgbaImage::new(w, h);

    if let Some(art) = artwork {
        let cropped = scale_to_cover(art.clone(), layout);
        let (ax, ay) = (layout.art_box.left, layout.art_box.top);
        imageops::overlay(&mut canvas, &cropped, ax as i64, ay as i64);
    }

    if let Some(tmpl) = template {
        imageops::overlay(&mut canvas, tmpl, 0, 0);
        // The gem is part of the frame, so it only exists once the template is
        // down. Nothing drawn below reaches it.
        gem::recolor(&mut canvas, card.color, layout);
    }

    draw_name(&mut canvas, &card.name, &fonts.name, layout);
    draw_rules(
        &mut canvas,
        &card.ability,
        card.flavor.as_deref(),
        &fonts.body,
        layout,
    );
    draw_stat(
        &mut canvas,
        &card.hand,
        layout.hand_center,
        &fonts.stats,
        layout,
    );
    draw_stat(
        &mut canvas,
        &card.life,
        layout.life_center,
        &fonts.stats,
        layout,
    );

    Ok(canvas)
}

// ── Per-element rendering ─────────────────────────────────────────────────────
// Each card element is drawn by exactly one function, used both by
// `render_card` and by the F1 tests (which render one element per canvas).
// Keeping these as the single entry points guarantees the tests exercise the
// same code path, with the same parameters, as production rendering.

/// Draw the card name centered in the name banner.
pub fn draw_name(canvas: &mut RgbaImage, name: &str, font: &FontRef, layout: &Layout) {
    let scale = text::fit_name_scale(name, font, layout);
    let (nx, ny) = layout.name_center;
    let pen = text::Pen::new(BLACK, layout.ink_gain);
    text::draw_text_centered_on_baseline(canvas, name, nx, ny, font, scale, pen);
}

/// Fit and draw the ability (and optional flavor) text block.
pub fn draw_rules(
    canvas: &mut RgbaImage,
    ability: &str,
    flavor: Option<&str>,
    font: &FontRef,
    layout: &Layout,
) {
    let fit = text::fit_rules_text(ability, flavor, font, font, layout);
    let pen = text::Pen::new(BLACK, layout.ink_gain);
    text::draw_rules_text(canvas, &fit, font, font, layout, pen);
}

/// Draw a stat modifier (hand or life) centered in its bubble.
pub fn draw_stat(
    canvas: &mut RgbaImage,
    value: &str,
    center: (u32, u32),
    font: &FontRef,
    layout: &Layout,
) {
    let (cx, cy) = center;
    text::draw_text_centered_on_ink(
        canvas,
        value,
        cx,
        cy,
        font,
        text::Run::tracked(
            PxScale::from(layout.stats_size),
            text::stats_tracking(value, layout),
        ),
        text::Pen::new(BLACK, layout.ink_gain),
    );
}

/// Scale artwork to cover the art box (fill both dimensions, then center-crop).
fn scale_to_cover(art: RgbaImage, layout: &Layout) -> RgbaImage {
    let box_w = layout.art_box.width();
    let box_h = layout.art_box.height();

    let art_w = art.width() as f32;
    let art_h = art.height() as f32;
    let art_ratio = art_w / art_h;
    let box_ratio = box_w / box_h;

    let (new_w, new_h) = if art_ratio > box_ratio {
        // Artwork wider — fit height, crop width
        let new_h = box_h as u32;
        let new_w = (art_w * (box_h / art_h)) as u32;
        (new_w, new_h)
    } else {
        // Artwork taller — fit width, crop height
        let new_w = box_w as u32;
        let new_h = (art_h * (box_w / art_w)) as u32;
        (new_w, new_h)
    };

    let resized = imageops::resize(&art, new_w, new_h, imageops::FilterType::Lanczos3);

    // Center-crop to exact art box dimensions
    let crop_x = (new_w as i32 - box_w as i32) / 2;
    let crop_y = (new_h as i32 - box_h as i32) / 2;
    imageops::crop_imm(
        &resized,
        crop_x.max(0) as u32,
        crop_y.max(0) as u32,
        box_w as u32,
        box_h as u32,
    )
    .to_image()
}

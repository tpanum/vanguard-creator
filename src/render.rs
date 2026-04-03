use ab_glyph::{FontRef, PxScale};
use anyhow::{bail, Context, Result};
use image::{imageops, RgbaImage};
use std::path::{Path, PathBuf};

use crate::{
    card::{self, CardDef},
    fonts,
    layout::{Layout, DEFAULT},
    text,
};

pub fn run(paths: &[PathBuf], output: Option<&Path>, template: Option<&Path>) -> Result<()> {
    let yaml_files = card::collect_yaml_files(paths)?;
    if yaml_files.is_empty() {
        bail!("no YAML card files found in the given paths");
    }

    let template_img = load_template(template)?;

    let name_font =
        FontRef::try_from_slice(fonts::name_data()).context("loading embedded name font")?;
    let body_font =
        FontRef::try_from_slice(fonts::body_data()).context("loading embedded body font")?;
    let body_bold_font = FontRef::try_from_slice(fonts::body_bold_data())
        .context("loading embedded body-bold font")?;

    let multi = yaml_files.len() > 1;

    for yaml_path in &yaml_files {
        let card = match CardDef::load(yaml_path) {
            Ok(c) => c,
            Err(e) => {
                eprintln!("warning: skipping {}: {e}", yaml_path.display());
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

        let artwork = if card.artwork.exists() {
            match image::open(&card.artwork)
                .with_context(|| format!("opening artwork {}", card.artwork.display()))
            {
                Ok(img) => Some(img.into_rgba8()),
                Err(e) => {
                    eprintln!("warning: {e}");
                    None
                }
            }
        } else {
            eprintln!("warning: artwork not found: {}", card.artwork.display());
            None
        };

        match render_card(
            &card,
            artwork.as_ref(),
            Some(&template_img),
            &name_font,
            &body_bold_font,
            &body_font,
        ) {
            Ok(img) => {
                img.save(&out_path)
                    .with_context(|| format!("saving {}", out_path.display()))?;
                println!("Saved: {}", out_path.display());
            }
            Err(e) => {
                eprintln!("warning: failed to render {}: {e}", card.name);
            }
        }
    }

    Ok(())
}

fn load_template(override_path: Option<&Path>) -> Result<RgbaImage> {
    if let Some(p) = override_path {
        return image::open(p)
            .with_context(|| format!("opening template {}", p.display()))
            .map(|i| i.into_rgba8());
    }
    image::load_from_memory(fonts::template_data())
        .context("loading embedded template")
        .map(|i| i.into_rgba8())
}

fn resolve_output(
    output: Option<&Path>,
    card_name: &str,
    yaml_path: &Path,
    multi: bool,
) -> PathBuf {
    let safe_name = sanitize_filename(card_name);
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

fn sanitize_filename(name: &str) -> String {
    name.chars()
        .map(|c| {
            if c.is_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect::<String>()
        .trim_matches('_')
        .to_lowercase()
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
    name_font: &FontRef,
    body_bold_font: &FontRef,
    stats_font: &FontRef,
) -> Result<RgbaImage> {
    let layout = &DEFAULT;

    let (w, h) = template.map(|t| t.dimensions()).unwrap_or((718, 1024));

    // ── Create canvas ──────────────────────────────────────────────────────
    let mut canvas = RgbaImage::new(w, h);

    // ── 1. Artwork layer ───────────────────────────────────────────────────
    if let Some(art) = artwork {
        let cropped = scale_to_cover(art.clone(), layout);
        let (ax, ay) = (layout.art_box.0, layout.art_box.1);
        imageops::overlay(&mut canvas, &cropped, ax as i64, ay as i64);
    }

    // ── 2. Template overlay ────────────────────────────────────────────────
    if let Some(tmpl) = template {
        imageops::overlay(&mut canvas, tmpl, 0, 0);
    }

    // ── 3. Card name ───────────────────────────────────────────────────────
    let (nx, ny) = layout.name_center;
    let base_scale = PxScale {
        x: layout.name_scale.0,
        y: layout.name_scale.1,
    };
    let stretch_ratio = base_scale.x / base_scale.y; // default horizontal stretch
                                                     // Measure at uniform y-scale, then apply stretch.
    let uniform_scale = PxScale {
        x: base_scale.y,
        y: base_scale.y,
    };
    let natural_w = text::measure_str(&card.name, name_font, uniform_scale);
    let stretched_w = natural_w * stretch_ratio;
    let name_scale = if stretched_w <= layout.name_max_width {
        // Stretch fits: apply the full default stretch ratio.
        PxScale {
            x: base_scale.y * stretch_ratio,
            y: base_scale.y,
        }
    } else if natural_w <= layout.name_max_width {
        // Stretched exceeds max but natural fits: reduce stretch to exactly fill max.
        PxScale {
            x: base_scale.y * (layout.name_max_width / natural_w),
            y: base_scale.y,
        }
    } else {
        // Even natural width exceeds max: scale both axes down proportionally.
        let f = layout.name_max_width / natural_w;
        PxScale {
            x: base_scale.y * f,
            y: base_scale.y * f,
        }
    };
    text::draw_centered_text(
        &mut canvas,
        &card.name,
        nx,
        ny,
        name_font,
        name_scale,
        [0, 0, 0],
    );

    // ── 4. Ability text ────────────────────────────────────────────────────
    let (tl, tt, tr, _tb) = layout.text_box;
    let text_max_w = (tr - tl) as f32 - layout.text_padding as f32 * 2.0;
    let text_box_h = (layout.text_box.3 - tt) as f32;

    let fit = text::fit_ability_text(
        &card.ability,
        card.flavor.as_deref(),
        body_bold_font,
        stats_font,
        text_max_w,
        text_box_h,
        layout.ability_size_max,
        layout.ability_size_min,
        layout.para_gap,
        layout.line_height_factor,
    );

    text::draw_ability_text(
        &mut canvas,
        &fit,
        layout.text_box,
        body_bold_font,
        stats_font,
        layout.para_gap,
        [0, 0, 0],
    );

    // ── 5. Stat modifiers ──────────────────────────────────────────────────
    let stats_scale = PxScale::from(layout.stats_size);
    let (hx, hy) = layout.hand_center;
    text::draw_centered_text(
        &mut canvas,
        &card.hand,
        hx,
        hy,
        stats_font,
        stats_scale,
        [0, 0, 0],
    );

    let (lx, ly) = layout.life_center;
    text::draw_centered_text(
        &mut canvas,
        &card.life,
        lx,
        ly,
        stats_font,
        stats_scale,
        [0, 0, 0],
    );

    Ok(canvas)
}

/// Scale artwork to cover the art box (fill both dimensions, then center-crop).
fn scale_to_cover(art: RgbaImage, layout: &Layout) -> RgbaImage {
    let (box_l, box_t, box_r, box_b) = layout.art_box;
    let box_w = (box_r - box_l) as f32;
    let box_h = (box_b - box_t) as f32;

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

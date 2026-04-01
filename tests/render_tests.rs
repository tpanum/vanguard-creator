use ab_glyph::FontRef;
use image::{imageops, ImageBuffer, Luma, RgbaImage};
use std::path::Path;

use vgc::{card::CardDef, fonts, render::render_card};

// ── Helpers ───────────────────────────────────────────────────────────────────

/// Render a card onto a plain white canvas (no template, no artwork) so only
/// our text pixels are present. Eliminates template frame noise when comparing
/// against a clean mask.
fn render_text_only(yaml_path: &str) -> RgbaImage {
    let name_font = FontRef::try_from_slice(fonts::NAME_DATA).expect("name font");
    let body_bold_font = FontRef::try_from_slice(fonts::BODY_BOLD_DATA).expect("body-bold font");
    let body_font = FontRef::try_from_slice(fonts::BODY_DATA).expect("body font");

    let mut card = CardDef::load(Path::new(yaml_path)).expect("load yaml");
    card.flavor = None; // mask covers only title, rules text, and stat bubbles

    let mut img = render_card(&card, None, None, &name_font, &body_bold_font, &body_font)
        .expect("render_card");

    // Flatten alpha over white so dark text is visible before binarizing.
    for p in img.pixels_mut() {
        let a = p[3] as f32 / 255.0;
        p[0] = (p[0] as f32 * a + 255.0 * (1.0 - a)) as u8;
        p[1] = (p[1] as f32 * a + 255.0 * (1.0 - a)) as u8;
        p[2] = (p[2] as f32 * a + 255.0 * (1.0 - a)) as u8;
        p[3] = 255;
    }
    img
}

/// Convert RGBA → binary luma mask (0 = text, 255 = background).
/// Composites over white before computing luma so transparent pixels → white.
/// `text_is_dark`: true  → pixels with luma < threshold are text (dark text on light bg)
///                 false → pixels with luma > threshold are text (light text on dark bg)
fn to_binary(img: &RgbaImage, threshold: u8, text_is_dark: bool) -> ImageBuffer<Luma<u8>, Vec<u8>> {
    ImageBuffer::from_fn(img.width(), img.height(), |x, y| {
        let p = img.get_pixel(x, y);
        let a = p[3] as f32 / 255.0;
        let r = p[0] as f32 * a + 255.0 * (1.0 - a);
        let g = p[1] as f32 * a + 255.0 * (1.0 - a);
        let b = p[2] as f32 * a + 255.0 * (1.0 - a);
        let l = (r * 0.299 + g * 0.587 + b * 0.114) as u8;
        let is_text = if text_is_dark { l < threshold } else { l > threshold };
        Luma([if is_text { 0 } else { 255 }])
    })
}

/// Load a ground-truth mask, scale it to match `target` dimensions, and binarize.
/// Polarity (dark-on-light vs light-on-dark) is detected automatically from
/// the mean luma of the mask.
fn load_mask(mask_path: &str, target: &RgbaImage) -> ImageBuffer<Luma<u8>, Vec<u8>> {
    let mask = image::open(mask_path).expect("load mask").into_rgba8();
    let scaled = imageops::resize(&mask, target.width(), target.height(), imageops::FilterType::Lanczos3);

    // Detect polarity: composite over white first so transparent pixels count as
    // background (white), then mean luma > 128 → dark text on light background.
    let lumas_composited: Vec<u8> = scaled.pixels().map(|p| {
        let a = p[3] as f32 / 255.0;
        let r = p[0] as f32 * a + 255.0 * (1.0 - a);
        let g = p[1] as f32 * a + 255.0 * (1.0 - a);
        let b = p[2] as f32 * a + 255.0 * (1.0 - a);
        (r * 0.299 + g * 0.587 + b * 0.114) as u8
    }).collect();
    let mean_luma: f64 = lumas_composited.iter().map(|&l| l as f64).sum::<f64>()
        / lumas_composited.len() as f64;
    let text_is_dark = mean_luma > 128.0;

    // For dark-on-light (e.g. white bg / black text): fixed 128 threshold works.
    // For light-on-dark (e.g. black bg / golden text): use Otsu's method to find
    // the natural valley between background and text luma clusters.
    let threshold = if text_is_dark {
        128u8
    } else {
        let n = lumas_composited.len() as f64;
        let mut hist = [0u64; 256];
        for &l in &lumas_composited { hist[l as usize] += 1; }
        let total_mean: f64 = hist.iter().enumerate()
            .map(|(i, &c)| i as f64 * c as f64).sum::<f64>() / n;
        let (mut best_t, mut best_var) = (0usize, 0.0f64);
        let (mut w0, mut sum0) = (0.0f64, 0.0f64);
        for t in 0..256 {
            w0 += hist[t] as f64 / n;
            sum0 += t as f64 * hist[t] as f64 / n;
            let w1 = 1.0 - w0;
            if w0 == 0.0 || w1 == 0.0 { continue; }
            let mean0 = sum0 / w0;
            let mean1 = (total_mean - sum0) / w1;
            let var = w0 * w1 * (mean0 - mean1).powi(2);
            if var > best_var { best_var = var; best_t = t; }
        }
        best_t as u8
    };

    // Re-binarize after scaling to remove anti-alias gray fringe.
    to_binary(&scaled, threshold, text_is_dark)
}

/// Precision, recall, and F1 of text pixels (0) in `got` vs `reference`.
fn text_f1(
    got: &ImageBuffer<Luma<u8>, Vec<u8>>,
    reference: &ImageBuffer<Luma<u8>, Vec<u8>>,
) -> (f64, f64, f64) {
    assert_eq!(got.dimensions(), reference.dimensions(), "dimension mismatch");
    let (mut tp, mut fp, mut fn_) = (0u64, 0u64, 0u64);
    for (g, r) in got.pixels().zip(reference.pixels()) {
        match (g[0] == 0, r[0] == 0) {
            (true,  true)  => tp  += 1,
            (true,  false) => fp  += 1,
            (false, true)  => fn_ += 1,
            (false, false) => {}
        }
    }
    let precision = if tp + fp  > 0 { tp as f64 / (tp + fp)  as f64 } else { 0.0 };
    let recall    = if tp + fn_ > 0 { tp as f64 / (tp + fn_) as f64 } else { 0.0 };
    let f1 = if precision + recall > 0.0 {
        2.0 * precision * recall / (precision + recall)
    } else { 0.0 };
    (precision, recall, f1)
}

/// Red = missed (in mask, not in render), Green = extra (in render, not in mask), Black = correct.
fn save_diff(got: &ImageBuffer<Luma<u8>, Vec<u8>>, reference: &ImageBuffer<Luma<u8>, Vec<u8>>, path: &str) {
    let (w, h) = got.dimensions();
    let mut diff = RgbaImage::new(w, h);
    for y in 0..h {
        for x in 0..w {
            let color = match (got.get_pixel(x, y)[0] == 0, reference.get_pixel(x, y)[0] == 0) {
                (true,  true)  => [0,   0,   0,   255],
                (false, true)  => [220, 0,   0,   255],
                (true,  false) => [0,   180, 0,   255],
                (false, false) => [255, 255, 255, 255],
            };
            diff.put_pixel(x, y, image::Rgba(color));
        }
    }
    diff.save(path).expect("save diff");
}

fn band_stats(mask: &ImageBuffer<Luma<u8>, Vec<u8>>, y_start: u32, y_end: u32, w: u32) -> (u32, u32, u32, u32, u32) {
    let (mut x_sum, mut x_min, mut x_max, mut px_count) = (0u64, w, 0u32, 0u64);
    for by in y_start..=y_end {
        for bx in 0..w {
            if mask.get_pixel(bx, by)[0] == 0 {
                x_sum += bx as u64;
                x_min = x_min.min(bx);
                x_max = x_max.max(bx);
                px_count += 1;
            }
        }
    }
    let xc = if px_count > 0 { (x_sum / px_count) as u32 } else { w / 2 };
    (y_start, y_end, x_min, x_max, xc)
}

/// Horizontal bands of dark (text) pixels, separated by at least `min_gap` empty rows.
fn text_bands(mask: &ImageBuffer<Luma<u8>, Vec<u8>>, min_gap: u32) -> Vec<(u32, u32, u32, u32, u32)> {
    let (w, h) = mask.dimensions();
    let row_counts: Vec<u32> = (0..h)
        .map(|y| (0..w).filter(|&x| mask.get_pixel(x, y)[0] == 0).count() as u32)
        .collect();

    let mut bands = Vec::new();
    let mut in_band = false;
    let mut band_start = 0u32;
    let mut gap_count = 0u32;

    for (y, &count) in row_counts.iter().enumerate() {
        let y = y as u32;
        if count > 0 {
            if !in_band { band_start = y; in_band = true; }
            gap_count = 0;
        } else if in_band {
            gap_count += 1;
            if gap_count >= min_gap {
                bands.push(band_stats(mask, band_start, y - gap_count, w));
                in_band = false;
                gap_count = 0;
            }
        }
    }
    if in_band { bands.push(band_stats(mask, band_start, h - 1, w)); }
    bands
}

fn run_calibrate(yaml: &str, mask: &str) {
    let rendered = render_text_only(yaml);
    let ref_mask = load_mask(mask, &rendered);
    let got_mask = to_binary(&rendered, 128, true);


    println!("\n── Mask (expected) text bands ──");
    for (ys, ye, xmin, xmax, xc) in text_bands(&ref_mask, 8) {
        println!("  y={ys}..{ye} (center y={})  x={xmin}..{xmax} width={}  x_centroid={xc}",
            (ys + ye) / 2, xmax.saturating_sub(xmin));
    }
    println!("\n── Rendered (ours) text bands ──");
    for (ys, ye, xmin, xmax, xc) in text_bands(&got_mask, 8) {
        println!("  y={ys}..{ye} (center y={})  x={xmin}..{xmax} width={}  x_centroid={xc}",
            (ys + ye) / 2, xmax.saturating_sub(xmin));
    }
}

fn run_test(label: &str, yaml: &str, mask_path: &str, fixture_prefix: &str, threshold: f64) {
    let rendered = render_text_only(yaml);
    let got_mask = to_binary(&rendered, 128, true);
    let ref_mask = load_mask(mask_path, &rendered);

    let (precision, recall, f1) = text_f1(&got_mask, &ref_mask);
    println!("{label} text F1: {:.1}%  (precision {:.1}%, recall {:.1}%)",
        f1 * 100.0, precision * 100.0, recall * 100.0);

    if std::env::var("UPDATE_FIXTURES").is_ok() {
        rendered.save(format!("tests/fixtures/{fixture_prefix}_rendered.png")).unwrap();
        save_diff(&got_mask, &ref_mask, &format!("tests/fixtures/{fixture_prefix}_diff.png"));
        println!("Saved fixtures for {label}");
    }

    assert!(f1 >= threshold,
        "{label} text F1 is {:.1}% — below threshold {:.1}% \
         (precision {:.1}%, recall {:.1}%)",
        f1 * 100.0, threshold * 100.0, precision * 100.0, recall * 100.0);
}

// ── Calibration (run with: cargo test -- --ignored --nocapture) ───────────────

#[test] #[ignore]
fn calibrate_gerrard() {
    run_calibrate("tests/gerrard.yaml", "tests/fixtures/gerrard_mask.png");
}

#[test] #[ignore]
fn calibrate_silverqueen() {
    run_calibrate("tests/silverqueen.yaml", "tests/fixtures/silverqueen_mask.png");
}

// ── Accuracy tests ────────────────────────────────────────────────────────────

#[test]
fn test_gerrard_text_vs_mask() {
    run_test("Gerrard", "tests/gerrard.yaml", "tests/fixtures/gerrard_mask.png", "gerrard", 0.35);
}

#[test]
fn test_silverqueen_text_vs_mask() {
    run_test("Sliver Queen", "tests/silverqueen.yaml", "tests/fixtures/silverqueen_mask.png", "silverqueen", 0.30);
}

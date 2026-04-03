use ab_glyph::{FontRef, PxScale};
use image::{ImageBuffer, Luma, RgbaImage};
use std::path::Path;

use vgc::{card::CardDef, fonts, layout::DEFAULT, render::render_card, text};

// ── Per-element render helpers ────────────────────────────────────────────────
// Each helper draws exactly one text element onto a blank 718×1024 canvas so
// that every rendered pixel belongs to the element under test. This gives
// meaningful precision when compared against per-region reference masks.

fn blank_canvas() -> RgbaImage {
    RgbaImage::from_pixel(718, 1024, image::Rgba([255, 255, 255, 255]))
}

fn flatten_alpha(img: &mut RgbaImage) {
    for p in img.pixels_mut() {
        let a = p[3] as f32 / 255.0;
        p[0] = (p[0] as f32 * a + 255.0 * (1.0 - a)) as u8;
        p[1] = (p[1] as f32 * a + 255.0 * (1.0 - a)) as u8;
        p[2] = (p[2] as f32 * a + 255.0 * (1.0 - a)) as u8;
        p[3] = 255;
    }
}

fn load_card(yaml_path: &str) -> CardDef {
    let mut card = CardDef::load(Path::new(yaml_path)).expect("load yaml");
    card.flavor = None;
    card
}

fn render_title(yaml_path: &str) -> RgbaImage {
    let card = load_card(yaml_path);
    let name_font = FontRef::try_from_slice(fonts::name_data()).expect("name font");
    let layout = &DEFAULT;

    let mut canvas = blank_canvas();
    let base_scale = PxScale {
        x: layout.name_scale.0,
        y: layout.name_scale.1,
    };
    let stretch_ratio = base_scale.x / base_scale.y;
    let uniform_scale = PxScale {
        x: base_scale.y,
        y: base_scale.y,
    };
    let natural_w = text::measure_str(&card.name, &name_font, uniform_scale);
    let stretched_w = natural_w * stretch_ratio;
    let name_scale = if stretched_w <= layout.name_max_width {
        PxScale {
            x: base_scale.y * stretch_ratio,
            y: base_scale.y,
        }
    } else if natural_w <= layout.name_max_width {
        PxScale {
            x: base_scale.y * (layout.name_max_width / natural_w),
            y: base_scale.y,
        }
    } else {
        let f = layout.name_max_width / natural_w;
        PxScale {
            x: base_scale.y * f,
            y: base_scale.y * f,
        }
    };
    let (nx, ny) = layout.name_center;
    text::draw_centered_text(
        &mut canvas,
        &card.name,
        nx,
        ny,
        &name_font,
        name_scale,
        [0, 0, 0],
    );
    flatten_alpha(&mut canvas);
    canvas
}

fn render_rules(yaml_path: &str) -> RgbaImage {
    let card = load_card(yaml_path);
    let body_bold_font = FontRef::try_from_slice(fonts::body_bold_data()).expect("body-bold font");
    let body_font = FontRef::try_from_slice(fonts::body_data()).expect("body font");
    let layout = &DEFAULT;

    let mut canvas = blank_canvas();
    let (tl, tt, tr, _tb) = layout.text_box;
    let text_max_w = (tr - tl) as f32 - layout.text_padding as f32 * 2.0;
    let text_box_h = (layout.text_box.3 - tt) as f32;
    let fit = text::fit_ability_text(
        &card.ability,
        card.flavor.as_deref(),
        &body_bold_font,
        &body_font,
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
        &body_bold_font,
        &body_font,
        layout.para_gap,
        layout.rules_centering_height,
        [0, 0, 0],
    );
    flatten_alpha(&mut canvas);
    canvas
}

fn render_left_bubble(yaml_path: &str) -> RgbaImage {
    let card = load_card(yaml_path);
    let body_font = FontRef::try_from_slice(fonts::body_data()).expect("body font");
    let layout = &DEFAULT;

    let mut canvas = blank_canvas();
    let stats_scale = PxScale::from(layout.stats_size);
    let (hx, hy) = layout.hand_center;
    text::draw_centered_text(
        &mut canvas,
        &card.hand,
        hx,
        hy,
        &body_font,
        stats_scale,
        [0, 0, 0],
    );
    flatten_alpha(&mut canvas);
    canvas
}

fn render_right_bubble(yaml_path: &str) -> RgbaImage {
    let card = load_card(yaml_path);
    let body_font = FontRef::try_from_slice(fonts::body_data()).expect("body font");
    let layout = &DEFAULT;

    let mut canvas = blank_canvas();
    let stats_scale = PxScale::from(layout.stats_size);
    let (lx, ly) = layout.life_center;
    text::draw_centered_text(
        &mut canvas,
        &card.life,
        lx,
        ly,
        &body_font,
        stats_scale,
        [0, 0, 0],
    );
    flatten_alpha(&mut canvas);
    canvas
}

// ── Image analysis helpers ────────────────────────────────────────────────────

/// Convert RGBA → binary luma mask (0 = text, 255 = background).
fn to_binary(img: &RgbaImage, threshold: u8, text_is_dark: bool) -> ImageBuffer<Luma<u8>, Vec<u8>> {
    ImageBuffer::from_fn(img.width(), img.height(), |x, y| {
        let p = img.get_pixel(x, y);
        let a = p[3] as f32 / 255.0;
        let r = p[0] as f32 * a + 255.0 * (1.0 - a);
        let g = p[1] as f32 * a + 255.0 * (1.0 - a);
        let b = p[2] as f32 * a + 255.0 * (1.0 - a);
        let l = (r * 0.299 + g * 0.587 + b * 0.114) as u8;
        let is_text = if text_is_dark {
            l < threshold
        } else {
            l > threshold
        };
        Luma([if is_text { 0 } else { 255 }])
    })
}

/// Load a ground-truth mask, scale it to match `target` dimensions, and binarize.
/// Polarity is auto-detected from the mean luma of the mask.
fn load_mask(mask_path: &str, target: &RgbaImage) -> ImageBuffer<Luma<u8>, Vec<u8>> {
    use image::imageops;

    let mask = image::open(mask_path).expect("load mask").into_rgba8();
    let scaled = imageops::resize(
        &mask,
        target.width(),
        target.height(),
        imageops::FilterType::Lanczos3,
    );

    let lumas: Vec<u8> = scaled
        .pixels()
        .map(|p| {
            let a = p[3] as f32 / 255.0;
            let r = p[0] as f32 * a + 255.0 * (1.0 - a);
            let g = p[1] as f32 * a + 255.0 * (1.0 - a);
            let b = p[2] as f32 * a + 255.0 * (1.0 - a);
            (r * 0.299 + g * 0.587 + b * 0.114) as u8
        })
        .collect();
    let mean_luma: f64 = lumas.iter().map(|&l| l as f64).sum::<f64>() / lumas.len() as f64;
    let text_is_dark = mean_luma > 128.0;

    let threshold = if text_is_dark {
        128u8
    } else {
        let n = lumas.len() as f64;
        let mut hist = [0u64; 256];
        for &l in &lumas {
            hist[l as usize] += 1;
        }
        let total_mean: f64 = hist
            .iter()
            .enumerate()
            .map(|(i, &c)| i as f64 * c as f64)
            .sum::<f64>()
            / n;
        let (mut best_t, mut best_var) = (0usize, 0.0f64);
        let (mut w0, mut sum0) = (0.0f64, 0.0f64);
        for t in 0..256 {
            w0 += hist[t] as f64 / n;
            sum0 += t as f64 * hist[t] as f64 / n;
            let w1 = 1.0 - w0;
            if w0 == 0.0 || w1 == 0.0 {
                continue;
            }
            let mean0 = sum0 / w0;
            let mean1 = (total_mean - sum0) / w1;
            let var = w0 * w1 * (mean0 - mean1).powi(2);
            if var > best_var {
                best_var = var;
                best_t = t;
            }
        }
        best_t as u8
    };

    to_binary(&scaled, threshold, text_is_dark)
}

/// Precision, recall, and F1 of text pixels (0) in `got` vs `reference`.
fn text_f1(
    got: &ImageBuffer<Luma<u8>, Vec<u8>>,
    reference: &ImageBuffer<Luma<u8>, Vec<u8>>,
) -> (f64, f64, f64) {
    assert_eq!(
        got.dimensions(),
        reference.dimensions(),
        "dimension mismatch"
    );
    let (mut tp, mut fp, mut fn_) = (0u64, 0u64, 0u64);
    for (g, r) in got.pixels().zip(reference.pixels()) {
        match (g[0] == 0, r[0] == 0) {
            (true, true) => tp += 1,
            (true, false) => fp += 1,
            (false, true) => fn_ += 1,
            (false, false) => {}
        }
    }
    let precision = if tp + fp > 0 {
        tp as f64 / (tp + fp) as f64
    } else {
        0.0
    };
    let recall = if tp + fn_ > 0 {
        tp as f64 / (tp + fn_) as f64
    } else {
        0.0
    };
    let f1 = if precision + recall > 0.0 {
        2.0 * precision * recall / (precision + recall)
    } else {
        0.0
    };
    (precision, recall, f1)
}

/// Red = missed (in mask, not in render), Green = extra (in render, not in mask), Black = correct.
fn save_diff(
    got: &ImageBuffer<Luma<u8>, Vec<u8>>,
    reference: &ImageBuffer<Luma<u8>, Vec<u8>>,
    path: &str,
) {
    let (w, h) = got.dimensions();
    let mut diff = RgbaImage::new(w, h);
    for y in 0..h {
        for x in 0..w {
            let color = match (
                got.get_pixel(x, y)[0] == 0,
                reference.get_pixel(x, y)[0] == 0,
            ) {
                (true, true) => [0, 0, 0, 255],
                (false, true) => [220, 0, 0, 255],
                (true, false) => [0, 180, 0, 255],
                (false, false) => [255, 255, 255, 255],
            };
            diff.put_pixel(x, y, image::Rgba(color));
        }
    }
    diff.save(path).expect("save diff");
}

fn band_stats(
    mask: &ImageBuffer<Luma<u8>, Vec<u8>>,
    y_start: u32,
    y_end: u32,
    w: u32,
) -> (u32, u32, u32, u32, u32) {
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
    let xc = if px_count > 0 {
        (x_sum / px_count) as u32
    } else {
        w / 2
    };
    (y_start, y_end, x_min, x_max, xc)
}

/// Horizontal bands of dark (text) pixels, separated by at least `min_gap` empty rows.
fn text_bands(
    mask: &ImageBuffer<Luma<u8>, Vec<u8>>,
    min_gap: u32,
) -> Vec<(u32, u32, u32, u32, u32)> {
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
            if !in_band {
                band_start = y;
                in_band = true;
            }
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
    if in_band {
        bands.push(band_stats(mask, band_start, h - 1, w));
    }
    bands
}

fn run_test(
    label: &str,
    rendered: &RgbaImage,
    mask_path: &str,
    fixture_prefix: &str,
    threshold: f64,
) {
    let got_mask = to_binary(rendered, 128, true);
    let ref_mask = load_mask(mask_path, rendered);

    let (precision, recall, f1) = text_f1(&got_mask, &ref_mask);
    println!(
        "{label} text F1: {:.1}%  (precision {:.1}%, recall {:.1}%)",
        f1 * 100.0,
        precision * 100.0,
        recall * 100.0
    );

    if std::env::var("UPDATE_FIXTURES").is_ok() {
        rendered
            .save(format!("tests/fixtures/{fixture_prefix}_rendered.png"))
            .unwrap();
        save_diff(
            &got_mask,
            &ref_mask,
            &format!("tests/fixtures/{fixture_prefix}_diff.png"),
        );
        println!("Saved fixtures for {label}");
    }

    assert!(
        f1 >= threshold,
        "{label} text F1 is {:.1}% — below threshold {:.1}% \
         (precision {:.1}%, recall {:.1}%)",
        f1 * 100.0,
        threshold * 100.0,
        precision * 100.0,
        recall * 100.0
    );
}

fn run_calibrate(rendered: &RgbaImage, mask: &str) {
    let ref_mask = load_mask(mask, rendered);
    let got_mask = to_binary(rendered, 128, true);

    println!("\n── Mask (expected) text bands ──");
    for (ys, ye, xmin, xmax, xc) in text_bands(&ref_mask, 8) {
        println!(
            "  y={ys}..{ye} (center y={})  x={xmin}..{xmax} width={}  x_centroid={xc}",
            (ys + ye) / 2,
            xmax.saturating_sub(xmin)
        );
    }
    println!("\n── Rendered (ours) text bands ──");
    for (ys, ye, xmin, xmax, xc) in text_bands(&got_mask, 8) {
        println!(
            "  y={ys}..{ye} (center y={})  x={xmin}..{xmax} width={}  x_centroid={xc}",
            (ys + ye) / 2,
            xmax.saturating_sub(xmin)
        );
    }
}

// ── Calibration (run with: cargo test -- --ignored --nocapture) ───────────────

#[test]
#[ignore]
fn calibrate_gerrard_title() {
    run_calibrate(
        &render_title("tests/gerrard.yaml"),
        "tests/fixtures/gerrard_title_mask.png",
    );
}

#[test]
#[ignore]
fn calibrate_gerrard_rules() {
    run_calibrate(
        &render_rules("tests/gerrard.yaml"),
        "tests/fixtures/gerrard_rules_mask.png",
    );
}

#[test]
#[ignore]
fn calibrate_gerrard_left_bubble() {
    run_calibrate(
        &render_left_bubble("tests/gerrard.yaml"),
        "tests/fixtures/gerrard_left_bubble_mask.png",
    );
}

#[test]
#[ignore]
fn calibrate_gerrard_right_bubble() {
    run_calibrate(
        &render_right_bubble("tests/gerrard.yaml"),
        "tests/fixtures/gerrard_right_bubble_mask.png",
    );
}

#[test]
#[ignore]
fn calibrate_silverqueen_title() {
    run_calibrate(
        &render_title("tests/silverqueen.yaml"),
        "tests/fixtures/silverqueen_title_mask.png",
    );
}

#[test]
#[ignore]
fn calibrate_silverqueen_rules() {
    run_calibrate(
        &render_rules("tests/silverqueen.yaml"),
        "tests/fixtures/silverqueen_rules_mask.png",
    );
}

#[test]
#[ignore]
fn calibrate_silverqueen_left_bubble() {
    run_calibrate(
        &render_left_bubble("tests/silverqueen.yaml"),
        "tests/fixtures/silverqueen_left_bubble_mask.png",
    );
}

#[test]
#[ignore]
fn calibrate_silverqueen_right_bubble() {
    run_calibrate(
        &render_right_bubble("tests/silverqueen.yaml"),
        "tests/fixtures/silverqueen_right_bubble_mask.png",
    );
}

// ── Accuracy tests ────────────────────────────────────────────────────────────

#[test]
fn test_gerrard_title() {
    run_test(
        "Gerrard title",
        &render_title("tests/gerrard.yaml"),
        "tests/fixtures/gerrard_title_mask.png",
        "gerrard_title",
        0.35,
    );
}

#[test]
fn test_gerrard_rules() {
    run_test(
        "Gerrard rules",
        &render_rules("tests/gerrard.yaml"),
        "tests/fixtures/gerrard_rules_mask.png",
        "gerrard_rules",
        0.18,
    );
}

#[test]
fn test_gerrard_left_bubble() {
    run_test(
        "Gerrard left bubble",
        &render_left_bubble("tests/gerrard.yaml"),
        "tests/fixtures/gerrard_left_bubble_mask.png",
        "gerrard_left_bubble",
        0.20,
    );
}

#[test]
fn test_gerrard_right_bubble() {
    run_test(
        "Gerrard right bubble",
        &render_right_bubble("tests/gerrard.yaml"),
        "tests/fixtures/gerrard_right_bubble_mask.png",
        "gerrard_right_bubble",
        0.30,
    );
}

#[test]
fn test_silverqueen_title() {
    run_test(
        "Sliver Queen title",
        &render_title("tests/silverqueen.yaml"),
        "tests/fixtures/silverqueen_title_mask.png",
        "silverqueen_title",
        0.35,
    );
}

#[test]
fn test_silverqueen_rules() {
    run_test(
        "Sliver Queen rules",
        &render_rules("tests/silverqueen.yaml"),
        "tests/fixtures/silverqueen_rules_mask.png",
        "silverqueen_rules",
        0.20,
    );
}

#[test]
fn test_silverqueen_left_bubble() {
    run_test(
        "Sliver Queen left bubble",
        &render_left_bubble("tests/silverqueen.yaml"),
        "tests/fixtures/silverqueen_left_bubble_mask.png",
        "silverqueen_left_bubble",
        0.20,
    );
}

#[test]
fn test_silverqueen_right_bubble() {
    run_test(
        "Sliver Queen right bubble",
        &render_right_bubble("tests/silverqueen.yaml"),
        "tests/fixtures/silverqueen_right_bubble_mask.png",
        "silverqueen_right_bubble",
        0.20,
    );
}

// ── Sidar Kondo ───────────────────────────────────────────────────────────────

#[test]
#[ignore]
fn calibrate_sidar_title() {
    run_calibrate(
        &render_title("tests/sidar.yaml"),
        "tests/fixtures/sidar_title.png",
    );
}

#[test]
#[ignore]
fn calibrate_sidar_rules() {
    run_calibrate(
        &render_rules("tests/sidar.yaml"),
        "tests/fixtures/sidar_rules_mask.png",
    );
}

#[test]
#[ignore]
fn calibrate_sidar_left_bubble() {
    run_calibrate(
        &render_left_bubble("tests/sidar.yaml"),
        "tests/fixtures/sidar_left_bubble_mask.png",
    );
}

#[test]
#[ignore]
fn calibrate_sidar_right_bubble() {
    run_calibrate(
        &render_right_bubble("tests/sidar.yaml"),
        "tests/fixtures/sidar_right_bubble_mask.png",
    );
}

#[test]
fn test_sidar_title() {
    run_test(
        "Sidar Kondo title",
        &render_title("tests/sidar.yaml"),
        "tests/fixtures/sidar_title.png",
        "sidar_title",
        0.30,
    );
}

#[test]
fn test_sidar_rules() {
    run_test(
        "Sidar Kondo rules",
        &render_rules("tests/sidar.yaml"),
        "tests/fixtures/sidar_rules_mask.png",
        "sidar_rules",
        0.15,
    );
}

#[test]
fn test_sidar_left_bubble() {
    run_test(
        "Sidar Kondo left bubble",
        &render_left_bubble("tests/sidar.yaml"),
        "tests/fixtures/sidar_left_bubble_mask.png",
        "sidar_left_bubble",
        0.20,
    );
}

#[test]
fn test_sidar_right_bubble() {
    run_test(
        "Sidar Kondo right bubble",
        &render_right_bubble("tests/sidar.yaml"),
        "tests/fixtures/sidar_right_bubble_mask.png",
        "sidar_right_bubble",
        0.20,
    );
}

// ── Volrath ───────────────────────────────────────────────────────────────────

#[test]
#[ignore]
fn calibrate_volrath_title() {
    run_calibrate(
        &render_title("tests/volrath.yaml"),
        "tests/fixtures/volrath_title_mask.png",
    );
}

#[test]
#[ignore]
fn calibrate_volrath_rules() {
    run_calibrate(
        &render_rules("tests/volrath.yaml"),
        "tests/fixtures/volrath_rules_mask.png",
    );
}

#[test]
#[ignore]
fn calibrate_volrath_left_bubble() {
    run_calibrate(
        &render_left_bubble("tests/volrath.yaml"),
        "tests/fixtures/volrath_left_bubble_mask.png",
    );
}

#[test]
#[ignore]
fn calibrate_volrath_right_bubble() {
    run_calibrate(
        &render_right_bubble("tests/volrath.yaml"),
        "tests/fixtures/volrath_right_bubble_mask.png",
    );
}

#[test]
fn test_volrath_title() {
    run_test(
        "Volrath title",
        &render_title("tests/volrath.yaml"),
        "tests/fixtures/volrath_title_mask.png",
        "volrath_title",
        0.35,
    );
}

#[test]
fn test_volrath_rules() {
    run_test(
        "Volrath rules",
        &render_rules("tests/volrath.yaml"),
        "tests/fixtures/volrath_rules_mask.png",
        "volrath_rules",
        0.03,
    );
}

#[test]
fn test_volrath_left_bubble() {
    run_test(
        "Volrath left bubble",
        &render_left_bubble("tests/volrath.yaml"),
        "tests/fixtures/volrath_left_bubble_mask.png",
        "volrath_left_bubble",
        0.20,
    );
}

#[test]
fn test_volrath_right_bubble() {
    run_test(
        "Volrath right bubble",
        &render_right_bubble("tests/volrath.yaml"),
        "tests/fixtures/volrath_right_bubble_mask.png",
        "volrath_right_bubble",
        0.20,
    );
}

// ── Smoke test: full card renders without error ───────────────────────────────

#[test]
fn test_full_card_renders() {
    let name_font = FontRef::try_from_slice(fonts::name_data()).expect("name font");
    let body_bold_font = FontRef::try_from_slice(fonts::body_bold_data()).expect("body-bold font");
    let body_font = FontRef::try_from_slice(fonts::body_data()).expect("body font");
    for yaml in &[
        "tests/gerrard.yaml",
        "tests/silverqueen.yaml",
        "tests/sidar.yaml",
        "tests/volrath.yaml",
    ] {
        let card = CardDef::load(Path::new(yaml)).expect("load yaml");
        render_card(&card, None, None, &name_font, &body_bold_font, &body_font)
            .expect("render_card");
    }
}

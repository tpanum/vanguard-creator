//! Measure the bezel credit line — `Illus. <artist>` — against the scans.
//!
//! This is where `Fonts::credit` and the `credit_*` constants in `layout.rs`
//! come from, and it is how to re-derive them. The accuracy suite cannot: it
//! scores four regions of the card and the bezel is not one of them.
//!
//! For each of six scans it segments the credit line out (the same
//! local-background + Otsu recipe as `tests/common/refmask.rs`), renders the
//! same string in each candidate face over a sweep of sizes, and reports the
//! best translation-aligned F1 — shape F1, with placement factored out, which
//! is the metric that chose the stat-bubble face too. The alignment that wins
//! also gives the baseline and centre the line should be drawn on.
//!
//!   cargo run --release --example illus_font

#![allow(clippy::needless_range_loop)]

use ab_glyph::{FontRef, PxScale};
use image::{imageops, GrayImage, ImageBuffer, Luma, RgbaImage};
use vgc::{bundle, text};

const BG_RADIUS: i32 = 26;
const MIN_AREA: usize = 24;

// Window around the credit block, in 718×1024 template coordinates.
const WIN: (u32, u32, u32, u32) = (150, 900, 570, 948);

fn to_gray(img: &RgbaImage) -> GrayImage {
    ImageBuffer::from_fn(img.width(), img.height(), |x, y| {
        let p = img.get_pixel(x, y);
        Luma([(p[0] as f32 * 0.299 + p[1] as f32 * 0.587 + p[2] as f32 * 0.114) as u8])
    })
}

fn box_blur(src: &GrayImage, radius: i32) -> Vec<f32> {
    let (w, h) = (src.width() as i32, src.height() as i32);
    let mut tmp = vec![0f32; (w * h) as usize];
    for y in 0..h {
        let (mut sum, mut n) = (0f32, 0f32);
        for x in -radius..=radius {
            if x >= 0 && x < w {
                sum += src.get_pixel(x as u32, y as u32)[0] as f32;
                n += 1.0;
            }
        }
        for x in 0..w {
            tmp[(y * w + x) as usize] = sum / n;
            let (out, inn) = (x - radius, x + radius + 1);
            if out >= 0 {
                sum -= src.get_pixel(out as u32, y as u32)[0] as f32;
                n -= 1.0;
            }
            if inn < w {
                sum += src.get_pixel(inn as u32, y as u32)[0] as f32;
                n += 1.0;
            }
        }
    }
    let mut out = vec![0f32; (w * h) as usize];
    for x in 0..w {
        let (mut sum, mut n) = (0f32, 0f32);
        for y in -radius..=radius {
            if y >= 0 && y < h {
                sum += tmp[(y * w + x) as usize];
                n += 1.0;
            }
        }
        for y in 0..h {
            out[(y * w + x) as usize] = sum / n;
            let (o, i) = (y - radius, y + radius + 1);
            if o >= 0 {
                sum -= tmp[(o * w + x) as usize];
                n -= 1.0;
            }
            if i < h {
                sum += tmp[(i * w + x) as usize];
                n += 1.0;
            }
        }
    }
    out
}

fn otsu(norm: &[u8]) -> u8 {
    let n = norm.len() as f64;
    let mut hist = [0u64; 256];
    for &v in norm {
        hist[v as usize] += 1;
    }
    let total_mean: f64 = hist
        .iter()
        .enumerate()
        .map(|(i, &c)| i as f64 * c as f64)
        .sum::<f64>()
        / n;
    let (mut best_t, mut best_var) = (0usize, 0.0f64);
    let (mut w0, mut sum0) = (0.0f64, 0.0f64);
    for (t, &count) in hist.iter().enumerate() {
        w0 += count as f64 / n;
        sum0 += t as f64 * count as f64 / n;
        let w1 = 1.0 - w0;
        if w0 <= 0.0 || w1 <= 0.0 {
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
}

fn drop_frame_and_speck(ink: &mut [bool], w: u32, h: u32) {
    let (w, h) = (w as i32, h as i32);
    let idx = |x: i32, y: i32| (y * w + x) as usize;
    let mut seen = vec![false; ink.len()];
    for sy in 0..h {
        for sx in 0..w {
            if !ink[idx(sx, sy)] || seen[idx(sx, sy)] {
                continue;
            }
            let mut comp = Vec::new();
            let mut stack = vec![(sx, sy)];
            seen[idx(sx, sy)] = true;
            let mut edge = false;
            while let Some((x, y)) = stack.pop() {
                comp.push(idx(x, y));
                if x == 0 || y == 0 || x == w - 1 || y == h - 1 {
                    edge = true;
                }
                for (nx, ny) in [(x - 1, y), (x + 1, y), (x, y - 1), (x, y + 1)] {
                    if nx >= 0
                        && ny >= 0
                        && nx < w
                        && ny < h
                        && ink[idx(nx, ny)]
                        && !seen[idx(nx, ny)]
                    {
                        seen[idx(nx, ny)] = true;
                        stack.push((nx, ny));
                    }
                }
            }
            if edge || comp.len() < MIN_AREA {
                for i in comp {
                    ink[i] = false;
                }
            }
        }
    }
}

/// Segment the credit window and keep only its first row band (the illus line).
fn credit_mask(scan_path: &str) -> Vec<Vec<bool>> {
    let scan = image::open(scan_path).unwrap().into_rgba8();
    let sx = scan.width() as f32 / 718.0;
    let sy = scan.height() as f32 / 1024.0;
    let (x0, y0, x1, y1) = WIN;
    let (cx0, cy0) = ((x0 as f32 * sx) as u32, (y0 as f32 * sy) as u32);
    let (cx1, cy1) = ((x1 as f32 * sx) as u32, (y1 as f32 * sy) as u32);
    let crop = imageops::crop_imm(&scan, cx0, cy0, cx1 - cx0, cy1 - cy0).to_image();
    let gray = to_gray(&crop);
    let bg = box_blur(&gray, BG_RADIUS);
    let (cw, ch) = (gray.width(), gray.height());
    let norm: Vec<u8> = gray
        .pixels()
        .zip(bg.iter())
        .map(|(p, &b)| ((p[0] as f32 / b.max(1.0)) * 255.0).clamp(0.0, 255.0) as u8)
        .collect();
    let t = otsu(&norm);
    let mut ink: Vec<bool> = norm.iter().map(|&v| v < t).collect();
    drop_frame_and_speck(&mut ink, cw, ch);

    // Downsample to template pixels.
    let (w, h) = ((x1 - x0) as usize, (y1 - y0) as usize);
    let mut out = vec![vec![false; w]; h];
    for oy in 0..h {
        for ox in 0..w {
            let px0 = (((x0 + ox as u32) as f32 * sx) as i64 - cx0 as i64).max(0) as u32;
            let px1 = ((((x0 + ox as u32 + 1) as f32) * sx).ceil() as i64 - cx0 as i64)
                .clamp(0, cw as i64) as u32;
            let py0 = (((y0 + oy as u32) as f32 * sy) as i64 - cy0 as i64).max(0) as u32;
            let py1 = ((((y0 + oy as u32 + 1) as f32) * sy).ceil() as i64 - cy0 as i64)
                .clamp(0, ch as i64) as u32;
            let (mut hit, mut total) = (0u32, 0u32);
            for y in py0..py1 {
                for x in px0..px1 {
                    total += 1;
                    if ink[(y * cw + x) as usize] {
                        hit += 1;
                    }
                }
            }
            out[oy][ox] = total > 0 && hit * 2 >= total;
        }
    }

    // Keep the first row band only: the illus line, not the copyright below it.
    let rows: Vec<bool> = out.iter().map(|r| r.iter().any(|&b| b)).collect();
    let mut bands = Vec::new();
    let mut gap = usize::MAX;
    for (i, &r) in rows.iter().enumerate() {
        if r {
            if gap >= 2 {
                bands.push(i);
            }
            gap = 0;
        } else {
            gap += 1;
        }
    }
    if bands.len() >= 2 {
        for row in out.iter_mut().skip(bands[1]) {
            row.iter_mut().for_each(|p| *p = false);
        }
    }
    out
}

fn bbox(m: &[Vec<bool>]) -> Option<(usize, usize, usize, usize)> {
    let (mut x0, mut y0, mut x1, mut y1) = (usize::MAX, usize::MAX, 0usize, 0usize);
    for (y, row) in m.iter().enumerate() {
        for (x, &p) in row.iter().enumerate() {
            if p {
                x0 = x0.min(x);
                y0 = y0.min(y);
                x1 = x1.max(x);
                y1 = y1.max(y);
            }
        }
    }
    (x1 >= x0 && y1 >= y0 && x0 != usize::MAX).then_some((x0, y0, x1, y1))
}

fn ink(m: &[Vec<bool>]) -> usize {
    m.iter().flatten().filter(|&&b| b).count()
}

/// Render `text` at `size` into the window, binarize, best-F1 over translation.
fn score(label: &str, font: &FontRef, size: f32, reference: &[Vec<bool>]) -> Fit {
    let (x0, y0, x1, y1) = WIN;
    let (w, h) = ((x1 - x0) as usize, (y1 - y0) as usize);
    let mut canvas = RgbaImage::from_pixel(718, 1024, image::Rgba([255, 255, 255, 255]));
    let run = text::Run::new(PxScale::from(size));
    let pen = text::Pen::new([0, 0, 0], vgc::layout::DEFAULT.ink_gain);
    // Draw ink-centered in the window so placement is not the variable.
    let (bx0, by0, bx1, by1) = text::ink_bounds(label, font, run).unwrap();
    let cx = (x0 + x1) as f32 / 2.0;
    let cy = (y0 + y1) as f32 / 2.0;
    let base_pen_x = cx - (bx0 + bx1) / 2.0;
    let base_line_y = cy - (by0 + by1) / 2.0;
    text::draw_text_at_baseline(&mut canvas, label, base_pen_x, base_line_y, font, run, pen);
    let mut render = vec![vec![false; w]; h];
    for y in 0..h {
        for x in 0..w {
            let p = canvas.get_pixel(x0 + x as u32, y0 + y as u32);
            let luma = p[0] as f32 * 0.299 + p[1] as f32 * 0.587 + p[2] as f32 * 0.114;
            render[y][x] = luma < 128.0;
        }
    }
    let rn = ink(&render);

    let (mut best_f1, mut best_dx, mut best_dy) = (0.0f32, 0, 0);
    for dy in -14i32..=14 {
        for dx in -30i32..=30 {
            let (mut tp, mut fp, mut fn_) = (0usize, 0usize, 0usize);
            for y in 0..h as i32 {
                for x in 0..w as i32 {
                    let r = reference[y as usize][x as usize];
                    let sy = y - dy;
                    let sx = x - dx;
                    let g = sy >= 0
                        && sx >= 0
                        && sy < h as i32
                        && sx < w as i32
                        && render[sy as usize][sx as usize];
                    match (r, g) {
                        (true, true) => tp += 1,
                        (false, true) => fp += 1,
                        (true, false) => fn_ += 1,
                        _ => {}
                    }
                }
            }
            let f1 = if tp == 0 {
                0.0
            } else {
                2.0 * tp as f32 / (2 * tp + fp + fn_) as f32
            };
            if f1 > best_f1 {
                best_f1 = f1;
                best_dx = dx;
                best_dy = dy;
            }
        }
    }
    Fit {
        f1: best_f1,
        size,
        baseline: base_line_y + best_dy as f32,
        center_x: base_pen_x + best_dx as f32 + text::measure_str(label, font, run.scale) / 2.0,
        ink: rn,
    }
}

#[derive(Clone, Copy, Default)]
struct Fit {
    f1: f32,
    size: f32,
    baseline: f32,
    center_x: f32,
    ink: usize,
}

fn main() {
    let cards: Vec<(&str, &str)> = vec![
        ("gerrard", "Illus. Douglas Shuler"),
        ("urza", "Illus. Mark Tedin"),
        ("serra", "Illus. Matthew Wilson"),
        ("volrath", "Illus. Anson Maddocks"),
        ("orim", "Illus. Rebecca Guay"),
        ("hanna", "Illus. Liz Danforth"),
    ];
    let faces: Vec<(&str, &[u8])> = vec![
        ("MPlantin", bundle::font("Mplantin.ttf")),
        ("MPlantin-Bold", bundle::font("Mplantin-Bold.ttf")),
        ("Fremont", bundle::font("Fremont-Regular.ttf")),
    ];

    let mut totals: Vec<(String, f32, Vec<f32>)> = Vec::new();
    for (fname, data) in &faces {
        let font = FontRef::try_from_slice(data).unwrap();
        let mut sizes = Vec::new();
        let mut f1s = Vec::new();
        for (slug, label) in &cards {
            let reference = credit_mask(&format!("tests/assets/{slug}.jpg"));
            let rb = bbox(&reference);
            let ri = ink(&reference);
            let mut best = Fit::default();
            let mut s = 10.0f32;
            while s <= 24.0 {
                let r = score(label, &font, s, &reference);
                if r.f1 > best.f1 {
                    best = r;
                }
                s += 0.25;
            }
            println!(
                "{fname:14} {slug:12} shapeF1 {:.3}  size {:.2}  baseline {:.1}  centre_x {:.1}  ink {}/{} ref-bbox {:?}",
                best.f1, best.size, best.baseline, best.center_x, best.ink, ri, rb
            );
            sizes.push(best.size);
            f1s.push(best.f1);
        }
        let mean = f1s.iter().sum::<f32>() / f1s.len() as f32;
        println!("{fname:14} mean shape F1 {mean:.3}  sizes {sizes:?}\n");
        totals.push((fname.to_string(), mean, sizes));
    }
    totals.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());
    println!("ranking:");
    for (n, m, _) in &totals {
        println!("  {n:14} {m:.3}");
    }

    // One size for every card: the winner above, re-scored over the whole set
    // at each size, so the constant is the best mean rather than an average of
    // per-card optima.
    let font = FontRef::try_from_slice(bundle::font("Mplantin.ttf")).unwrap();
    let refs: Vec<Vec<Vec<bool>>> = cards
        .iter()
        .map(|(slug, _)| credit_mask(&format!("tests/assets/{slug}.jpg")))
        .collect();
    println!("\nMPlantin, mean shape F1 by size:");
    let (mut best_s, mut best_m) = (0.0f32, 0.0f32);
    let mut s = 15.0f32;
    while s <= 17.5 {
        let m = cards
            .iter()
            .zip(&refs)
            .map(|((_, label), r)| score(label, &font, s, r).f1)
            .sum::<f32>()
            / cards.len() as f32;
        println!("  {s:.2}  {m:.4}");
        if m > best_m {
            best_m = m;
            best_s = s;
        }
        s += 0.25;
    }
    println!("  best {best_s:.2} ({best_m:.4})");

    // Where the banner has room for the line: the dark frame either side of the
    // gold shows up as a trough in the luma profile. At the baseline the
    // interior runs 133–578, but at the height the capitals reach it is notched
    // by the scroll ends and narrows to roughly 150–570 — which is what
    // `credit_max_width` is measured against.
    let tmpl = image::load_from_memory(bundle::TEMPLATE)
        .unwrap()
        .to_rgba8();
    for y in [912u32, 918, 925] {
        let row: Vec<u32> = (100..640)
            .step_by(10)
            .map(|x| {
                let p = tmpl.get_pixel(x, y);
                (p[0] as u32 * 30 + p[1] as u32 * 59 + p[2] as u32 * 11) / 100
            })
            .collect();
        println!("banner luma at y={y} (x = 100, 110, … 630): {row:?}");
    }
}

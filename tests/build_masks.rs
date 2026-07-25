//! Reference-mask generator.
//!
//! The rules text and the stat bubbles are dark ink on a light field, so their
//! ground truth can be segmented straight out of a card scan instead of being
//! traced by hand. Doing that matters for two reasons found while auditing the
//! old hand-made masks:
//!
//!   * the hand masks carried roughly 20% more ink than the scans they came
//!     from, which biased the F1 metric toward type that is too heavy — it
//!     ranked MPlantin Bold above MPlantin Regular for the stat bubbles even
//!     though the scans show the bubbles are plainly Regular weight;
//!   * they contained no mana symbols at all, so a correctly drawn `{3}` was
//!     scored as a solid block of false positives.
//!
//! Card titles are gold-on-dark and do not segment cleanly, so those keep their
//! hand-made masks.
//!
//! Regenerate with:
//!
//!   cargo test --release --test build_masks -- --ignored --nocapture
//!
//! Output is written straight to `tests/fixtures/*_ref.png` as black text on
//! white at 718×1024 — the same space the renderer works in, so the accuracy
//! suite compares masks without resampling them, and the polarity takes
//! `load_mask`'s deterministic branch rather than its Otsu fallback.

mod common;

use image::{imageops, GrayImage, ImageBuffer, Luma, RgbaImage};

/// Card scans, all registered to the same frame as the 718×1024 template.
const SCANS: &[(&str, &str)] = &[
    ("gerrard", "tests/assets/gerrard.jpg"),
    ("silverqueen", "tests/assets/silverqueen.jpg"),
    ("sidar", "tests/assets/sidarkondo.jpg"),
    ("volrath", "tests/assets/volrath.jpg"),
];

/// A region to segment, in 718×1024 template coordinates.
struct Region {
    suffix: &'static str,
    x0: u32,
    y0: u32,
    x1: u32,
    y1: u32,
    /// Keep only the leading block of text lines, dropping the flavor text that
    /// follows it. The accuracy suite renders ability text alone.
    ability_only: bool,
}

/// The rules window is deliberately wider and taller than `layout.text_box` so
/// that a render which overflows the box still shows up as a mismatch rather
/// than being silently cropped away. The bubble windows sit inside the metal
/// rim, which is dark enough to segment as ink if included.
const REGIONS: &[Region] = &[
    Region {
        suffix: "rules_ref",
        x0: 96,
        y0: 644,
        x1: 624,
        y1: 800,
        ability_only: true,
    },
    Region {
        suffix: "left_bubble_ref",
        x0: 70,
        y0: 856,
        x1: 131,
        y1: 902,
        ability_only: false,
    },
    Region {
        suffix: "right_bubble_ref",
        x0: 583,
        y0: 856,
        x1: 644,
        y1: 902,
        ability_only: false,
    },
];

/// Expected baseline-to-baseline pitch of ability text, in template pixels.
/// A vertical step larger than `PITCH × 1.5` means the block has ended and the
/// next band is flavor text, which the accuracy suite does not render.
const ABILITY_PITCH: u32 = 30;

/// Ink is `luma < local_background × INK_RATIO`. A local background rather than
/// a global threshold, because the parchment is textured and shades unevenly
/// across the text box, and the bubbles are lighter than the box they sit near.
const INK_RATIO: f32 = 0.74;

/// Radius, in scan pixels, of the box filter that estimates local background.
/// Must be comfortably wider than a glyph stroke so the background estimate is
/// not pulled down by the ink it is meant to separate.
const BG_RADIUS: i32 = 26;

fn to_gray(img: &RgbaImage) -> GrayImage {
    ImageBuffer::from_fn(img.width(), img.height(), |x, y| {
        let p = img.get_pixel(x, y);
        Luma([(p[0] as f32 * 0.299 + p[1] as f32 * 0.587 + p[2] as f32 * 0.114) as u8])
    })
}

/// Separable box blur over a grayscale image, used as the local background estimate.
fn box_blur(src: &GrayImage, radius: i32) -> Vec<f32> {
    let (w, h) = (src.width() as i32, src.height() as i32);
    let mut tmp = vec![0f32; (w * h) as usize];
    for y in 0..h {
        let mut sum = 0f32;
        let mut n = 0f32;
        for x in -radius..=radius {
            if x >= 0 && x < w {
                sum += src.get_pixel(x as u32, y as u32)[0] as f32;
                n += 1.0;
            }
        }
        for x in 0..w {
            tmp[(y * w + x) as usize] = sum / n;
            let out = x - radius;
            let inn = x + radius + 1;
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
        let mut sum = 0f32;
        let mut n = 0f32;
        for y in -radius..=radius {
            if y >= 0 && y < h {
                sum += tmp[(y * w + x) as usize];
                n += 1.0;
            }
        }
        for y in 0..h {
            out[(y * w + x) as usize] = sum / n;
            let o = y - radius;
            let i = y + radius + 1;
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

/// Remove everything that is not a glyph: components running off the edge of
/// the window (the text-box rule lines, the metal bubble rim, the "Starting
/// Hand Size" legend bleeding in from the frame) and isolated scan speckle.
fn drop_frame_and_speck_components(ink: &mut [bool], w: u32, h: u32) {
    const MIN_AREA: usize = 14;

    let (w, h) = (w as i32, h as i32);
    let idx = |x: i32, y: i32| (y * w + x) as usize;
    let mut seen = vec![false; ink.len()];

    for start_y in 0..h {
        for start_x in 0..w {
            if !ink[idx(start_x, start_y)] || seen[idx(start_x, start_y)] {
                continue;
            }
            let mut component = Vec::new();
            let mut stack = vec![(start_x, start_y)];
            seen[idx(start_x, start_y)] = true;
            let mut touches_edge = false;

            while let Some((x, y)) = stack.pop() {
                component.push(idx(x, y));
                if x == 0 || y == 0 || x == w - 1 || y == h - 1 {
                    touches_edge = true;
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

            if touches_edge || component.len() < MIN_AREA {
                for i in component {
                    ink[i] = false;
                }
            }
        }
    }
}

/// Keep only the leading block of text lines. Ability text and flavor text are
/// separated by a vertical step much larger than the ability's own line pitch,
/// so the first oversized step marks the end of the block.
fn keep_ability_block(rows: &mut [((u32, u32), bool)], y0: u32, y1: u32, x_span: u32) {
    let mut inked_row = vec![false; (y1 - y0) as usize];
    for ((_, y), covered) in rows.iter() {
        if *covered {
            inked_row[(y - y0) as usize] = true;
        }
    }

    // Band starts, allowing a 3-row gap inside a line (dots on i, gaps in serifs).
    let mut starts = Vec::new();
    let mut gap = u32::MAX;
    for (i, &inked) in inked_row.iter().enumerate() {
        if inked {
            if gap >= 3 {
                starts.push(y0 + i as u32);
            }
            gap = 0;
        } else {
            gap = gap.saturating_add(1);
        }
    }

    let cut = starts
        .windows(2)
        .find(|p| p[1] - p[0] > ABILITY_PITCH * 3 / 2)
        .map(|p| p[1]);

    if let Some(cut) = cut {
        for ((_, y), covered) in rows.iter_mut() {
            if *y >= cut {
                *covered = false;
            }
        }
    }
    let _ = x_span;
}

/// Segment one region of a scan into an area-correct 718×1024 binary mask.
///
/// Segmentation happens at the scan's native resolution, where the strokes are
/// widest and cleanest; the binary result is then downsampled so each output
/// pixel holds the fraction of the glyph covering it, and thresholded at half
/// coverage. Segmenting after downsampling instead would blur thin strokes into
/// the background and lose exactly the weight information the mask exists to
/// record.
fn segment(scan: &RgbaImage, region: &Region) -> Vec<((u32, u32), bool)> {
    let sx = scan.width() as f32 / 718.0;
    let sy = scan.height() as f32 / 1024.0;

    let (cx0, cy0) = (
        (region.x0 as f32 * sx) as u32,
        (region.y0 as f32 * sy) as u32,
    );
    let (cx1, cy1) = (
        (region.x1 as f32 * sx) as u32,
        (region.y1 as f32 * sy) as u32,
    );
    let crop = imageops::crop_imm(scan, cx0, cy0, cx1 - cx0, cy1 - cy0).to_image();
    let gray = to_gray(&crop);
    let bg = box_blur(&gray, BG_RADIUS);

    let (cw, ch) = (gray.width(), gray.height());
    let mut ink: Vec<bool> = gray
        .pixels()
        .zip(bg.iter())
        .map(|(p, &b)| (p[0] as f32) < b * INK_RATIO)
        .collect();
    drop_frame_and_speck_components(&mut ink, gray.width(), gray.height());

    // Area-average the native-resolution binary down into template pixels.
    let mut out: Vec<((u32, u32), bool)> = Vec::new();
    for oy in region.y0..region.y1 {
        for ox in region.x0..region.x1 {
            let px0 = ((ox as f32 * sx) as i64 - cx0 as i64).max(0) as u32;
            let px1 =
                (((ox + 1) as f32 * sx).ceil() as i64 - cx0 as i64).clamp(0, cw as i64) as u32;
            let py0 = ((oy as f32 * sy) as i64 - cy0 as i64).max(0) as u32;
            let py1 =
                (((oy + 1) as f32 * sy).ceil() as i64 - cy0 as i64).clamp(0, ch as i64) as u32;
            let (mut hit, mut total) = (0u32, 0u32);
            for y in py0..py1 {
                for x in px0..px1 {
                    total += 1;
                    if ink[(y * cw + x) as usize] {
                        hit += 1;
                    }
                }
            }
            let covered = total > 0 && hit * 2 >= total;
            out.push(((ox, oy), covered));
        }
    }

    if region.ability_only {
        keep_ability_block(&mut out, region.y0, region.y1, region.x1 - region.x0);
    }
    out
}

#[test]
#[ignore]
fn build_reference_masks() {
    for (card, path) in SCANS {
        let scan = image::open(path)
            .unwrap_or_else(|e| panic!("opening {path}: {e}"))
            .into_rgba8();

        for region in REGIONS {
            let mut mask = GrayImage::from_pixel(718, 1024, Luma([255]));
            let mut ink = 0u32;
            for ((x, y), covered) in segment(&scan, region) {
                if covered {
                    mask.put_pixel(x, y, Luma([0]));
                    ink += 1;
                }
            }
            let out = format!("tests/fixtures/{card}_{}.png", region.suffix);
            mask.save(&out)
                .unwrap_or_else(|e| panic!("saving {out}: {e}"));
            println!("{out}: {ink} ink px");
        }
    }
    println!("\nReview the generated masks before trusting a score built on them.");
}

/// Overlay each generated mask on its scan so the segmentation can be checked
/// by eye: green where the mask claims ink.
#[test]
#[ignore]
fn preview_reference_masks() {
    for (card, path) in SCANS {
        let scan = image::open(path).expect("scan").into_rgba8();
        let small = imageops::resize(&scan, 718, 1024, imageops::FilterType::Lanczos3);
        let mut preview = small.clone();
        for region in REGIONS {
            for ((x, y), covered) in segment(&scan, region) {
                if covered {
                    preview.put_pixel(x, y, image::Rgba([0, 220, 0, 255]));
                }
            }
        }
        let out = format!("tests/fixtures/{card}_ref_preview.png");
        preview.save(&out).expect("save preview");
        println!("{out}");
    }
}

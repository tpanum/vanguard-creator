//! Reference masks, segmented from card scans at test time.
//!
//! The ground truth for "did we re-generate this card correctly?" is the card
//! itself. Every reference mask is therefore derived from a scan in
//! `tests/assets/` rather than stored as a fixture: the mask is a pure function
//! of two version-controlled inputs, the scan and the code in this file, so a
//! change to either still shows up in a diff, and adding a card to the suite
//! costs a YAML file and a scan rather than four hand-traced PNGs.
//!
//! Card art is dark ink on a light field everywhere the suite scores — dark
//! type on the parchment text box, on the light stat bubbles, and on the gold
//! name banner — so a local-background threshold separates all four regions
//! without any per-card tuning.
//!
//! Inspect what this produces with:
//!
//!   cargo test --release --test build_masks -- --ignored --nocapture
//!
//! which writes an overlay of the segmentation on each scan.

use image::{imageops, GrayImage, ImageBuffer, Luma, RgbaImage};
use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

use super::{BinMask, Element};

/// A region to segment, in 718×1024 template coordinates.
pub struct Region {
    pub x0: u32,
    pub y0: u32,
    pub x1: u32,
    pub y1: u32,
    /// Keep only the leading block of text lines, dropping the flavor text that
    /// follows it. The accuracy suite renders ability text alone.
    pub ability_only: bool,
}

/// Windows are drawn a little wider and taller than the corresponding
/// `layout` boxes, so a render that overflows its box shows up as a mismatch
/// rather than being silently cropped away. They stop short of the frame
/// itself: the banner's scroll ends, the metal bubble rim and the text box's
/// rule lines are all dark enough to segment as ink.
pub fn region(element: Element) -> Region {
    match element {
        Element::Title => Region {
            x0: 112,
            y0: 50,
            x1: 606,
            y1: 112,
            ability_only: false,
        },
        Element::Rules => Region {
            x0: 96,
            y0: 644,
            x1: 624,
            y1: 800,
            ability_only: true,
        },
        Element::LeftBubble => Region {
            x0: 70,
            y0: 856,
            x1: 131,
            y1: 902,
            ability_only: false,
        },
        Element::RightBubble => Region {
            x0: 583,
            y0: 856,
            x1: 644,
            y1: 902,
            ability_only: false,
        },
    }
}

/// Expected baseline-to-baseline pitch of ability text, in template pixels.
/// A vertical step larger than `PITCH × 1.5` means the block has ended and the
/// next band is flavor text, which the accuracy suite does not render.
const ABILITY_PITCH: u32 = 30;

/// Ink is decided on `luma / local_background`, so a textured parchment and a
/// gold banner running from near-white to mid-brown can share one rule. The
/// split point on that ratio is chosen per region by Otsu.
///
/// The threshold must land halfway between paper and ink, because that is where
/// our own renders are binarized — a glyph pixel counts as ink at 50% coverage.
/// An earlier version used a fixed `luma < background × 0.74`, which on the
/// stat bubbles put the boundary at luma 105 when paper is 142 and ink is 9:
/// far up the blurred shoulder of every stroke, so the mask recorded strokes
/// substantially fatter than the card's. That silently corrupted two decisions
/// downstream — it ranked MPlantin Bold above Regular for the bubbles, and it
/// pulled `ink_gain` down to thicken the render toward a phantom weight.
/// Anything that changes where this boundary sits invalidates both.
fn ink_threshold(norm: &[u8]) -> u8 {
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

/// Radius, in scan pixels, of the box filter that estimates local background.
/// Must be comfortably wider than a glyph stroke so the background estimate is
/// not pulled down by the ink it is meant to separate.
const BG_RADIUS: i32 = 26;

/// Smallest component, in scan pixels, kept as real ink. Below this it is
/// scanner speckle or a compression artifact in the frame.
const MIN_AREA: usize = 24;

// ── Cache ─────────────────────────────────────────────────────────────────────

type Key = (&'static str, Element);

fn cache() -> &'static Mutex<HashMap<Key, &'static BinMask>> {
    static CACHE: OnceLock<Mutex<HashMap<Key, &'static BinMask>>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

/// The reference mask for one element of one card, segmented on first use.
///
/// Segmenting a region costs a few milliseconds but the calibrator rescores the
/// whole suite thousands of times, so results are memoised for the life of the
/// test process. Masks are leaked rather than reference-counted: there is one
/// per (card, element), they live until the process exits either way, and a
/// `&'static` keeps the callers free of lifetime plumbing.
pub fn reference(scan_path: &'static str, element: Element) -> &'static BinMask {
    if let Some(mask) = cache().lock().unwrap().get(&(scan_path, element)) {
        return mask;
    }

    let scan = image::open(scan_path)
        .unwrap_or_else(|e| panic!("opening scan {scan_path}: {e}"))
        .into_rgba8();
    let mask: &'static BinMask = Box::leak(Box::new(segment_to_mask(&scan, element)));

    cache()
        .lock()
        .unwrap()
        .insert((scan_path, element), mask)
        .map_or(mask, |existing| existing)
}

fn segment_to_mask(scan: &RgbaImage, element: Element) -> BinMask {
    let mut mask = GrayImage::from_pixel(718, 1024, Luma([255]));
    for ((x, y), covered) in segment(scan, &region(element)) {
        if covered {
            mask.put_pixel(x, y, Luma([0]));
        }
    }
    mask
}

// ── Segmentation ──────────────────────────────────────────────────────────────

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
/// the window (the banner's scroll ends, the metal bubble rim, the text-box
/// rule lines) and isolated scan speckle.
fn drop_frame_and_speck_components(ink: &mut [bool], w: u32, h: u32) {
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
fn keep_ability_block(rows: &mut [((u32, u32), bool)], y0: u32, y1: u32) {
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
}

/// Segment one region of a scan into area-correct 718×1024 coverage flags.
///
/// Segmentation happens at the scan's native resolution, where the strokes are
/// widest and cleanest; the binary result is then downsampled so each output
/// pixel holds the fraction of the glyph covering it, and thresholded at half
/// coverage. Segmenting after downsampling instead would blur thin strokes into
/// the background and lose exactly the weight information the mask exists to
/// record.
pub fn segment(scan: &RgbaImage, region: &Region) -> Vec<((u32, u32), bool)> {
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
    // Flatten the uneven background out of the way, then split paper from ink
    // on the flattened image. 255 is "as light as its surroundings"; ink runs
    // far below that whatever the surroundings happen to be.
    let norm: Vec<u8> = gray
        .pixels()
        .zip(bg.iter())
        .map(|(p, &b)| ((p[0] as f32 / b.max(1.0)) * 255.0).clamp(0.0, 255.0) as u8)
        .collect();
    let threshold = ink_threshold(&norm);
    let mut ink: Vec<bool> = norm.iter().map(|&v| v < threshold).collect();
    drop_frame_and_speck_components(&mut ink, cw, ch);

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
            out.push(((ox, oy), total > 0 && hit * 2 >= total));
        }
    }

    if region.ability_only {
        keep_ability_block(&mut out, region.y0, region.y1);
    }
    out
}

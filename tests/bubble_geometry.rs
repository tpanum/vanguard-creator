//! Glyph-by-glyph geometry of the stat bubbles, reference against render.
//!
//! The pair of F1 scores says how much ink lands in the right place, and the
//! diagnosis in the accuracy suite reports the bounding box of a whole value —
//! but `+12` fitting in the same box as the original can still be reached the
//! wrong way, by squeezing three glyphs where the original set three narrow
//! ones close together. Telling those apart needs the glyphs measured
//! separately: a condensed digit is narrower *and* has thinner vertical stems,
//! where a tightly tracked one keeps its shape and loses the space beside it.
//!
//!   cargo test --release --test bubble_geometry -- --ignored --nocapture
//!
//! Prints, per card, one row per connected component in the bubble — sign and
//! each digit — with its width, height and the gap to the component before it.

mod common;

use common::{refmask, BinMask, Element, CARDS};
use image::GrayImage;

/// One blob of ink: a sign or a digit.
#[derive(Debug, Clone, Copy)]
struct Blob {
    x0: u32,
    y0: u32,
    x1: u32,
    y1: u32,
    ink: u32,
}

impl Blob {
    fn w(&self) -> u32 {
        self.x1 - self.x0 + 1
    }
    fn h(&self) -> u32 {
        self.y1 - self.y0 + 1
    }
}

/// 8-connected components of the ink in a mask, ordered left to right.
/// Blobs of fewer than 8 px are speckle and dropped.
fn blobs(mask: &GrayImage) -> Vec<Blob> {
    let (w, h) = mask.dimensions();
    let mut seen = vec![false; (w * h) as usize];
    let ink = |x: u32, y: u32| mask.get_pixel(x, y).0[0] < 128;
    let mut out = Vec::new();

    for y in 0..h {
        for x in 0..w {
            let idx = (y * w + x) as usize;
            if seen[idx] || !ink(x, y) {
                continue;
            }
            let mut stack = vec![(x, y)];
            seen[idx] = true;
            let mut b = Blob {
                x0: x,
                y0: y,
                x1: x,
                y1: y,
                ink: 0,
            };
            while let Some((cx, cy)) = stack.pop() {
                b.ink += 1;
                b.x0 = b.x0.min(cx);
                b.y0 = b.y0.min(cy);
                b.x1 = b.x1.max(cx);
                b.y1 = b.y1.max(cy);
                for dy in -1i32..=1 {
                    for dx in -1i32..=1 {
                        let nx = cx as i32 + dx;
                        let ny = cy as i32 + dy;
                        if nx < 0 || ny < 0 || nx >= w as i32 || ny >= h as i32 {
                            continue;
                        }
                        let (nx, ny) = (nx as u32, ny as u32);
                        let nidx = (ny * w + nx) as usize;
                        if !seen[nidx] && ink(nx, ny) {
                            seen[nidx] = true;
                            stack.push((nx, ny));
                        }
                    }
                }
            }
            if b.ink >= 8 {
                out.push(b);
            }
        }
    }

    out.sort_by_key(|b| b.x0);
    out
}

fn report(label: &str, mask: &BinMask) {
    let bs = blobs(mask);
    print!("  {label:<26}");
    if bs.is_empty() {
        println!("(no ink)");
        return;
    }
    // Centre-to-centre distance is the advance between two glyphs and is
    // invariant to ink spread, which fattens every glyph by the same amount on
    // every side and so shrinks the measured gap without moving the centres.
    let mut prev_cx: Option<f64> = None;
    for b in &bs {
        let cx = (b.x0 + b.x1) as f64 / 2.0;
        let step = prev_cx.map_or(0.0, |p| cx - p);
        print!(
            "  [w{:>3} h{:>3} ink{:>4} step{:>5.1}]",
            b.w(),
            b.h(),
            b.ink,
            step
        );
        prev_cx = Some(cx);
    }
    let total = bs.last().unwrap().x1 - bs[0].x0 + 1;
    println!("   total w {total}");
}

/// Reference versus render, glyph by glyph, for every bubble in the suite.
#[test]
#[ignore]
fn bubble_geometry() {
    for card in CARDS {
        let yaml = format!("tests/cards/{}.yaml", card.slug);
        let def = common::load_card(&yaml);
        println!("{} — hand {:?}  life {:?}", card.name, def.hand, def.life);
        let scan: &'static str =
            Box::leak(format!("tests/assets/{}.jpg", card.slug).into_boxed_str());
        for (element, value) in [
            (Element::LeftBubble, &def.hand),
            (Element::RightBubble, &def.life),
        ] {
            report(
                &format!("{} ref {}", element.slug(), value),
                refmask::reference(scan, element),
            );
            let gray = image::imageops::grayscale(&element.render(&yaml));
            report(&format!("{} got {}", element.slug(), value), &gray);
        }
    }
}

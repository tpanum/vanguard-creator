//! Visual check on the reference masks the accuracy suite generates.
//!
//! Reference masks are segmented from the card scans at test time by
//! `common::refmask`; nothing is stored on disk. This writes an overlay of what
//! that segmentation found onto each scan — green where the mask claims ink —
//! so a change to the segmentation can be reviewed by eye rather than inferred
//! from a moving F1.
//!
//!   cargo test --release --test build_masks -- --ignored --nocapture
//!
//! Overlays land in `tests/fixtures/*_ref_preview.png`, which is gitignored.

mod common;

use common::{refmask, Element, CARDS};
use image::imageops;

const ELEMENTS: [Element; 4] = [
    Element::Title,
    Element::Rules,
    Element::LeftBubble,
    Element::RightBubble,
];

#[test]
#[ignore]
fn preview_reference_masks() {
    for card in CARDS {
        let path = format!("tests/assets/{}.jpg", card.slug);
        let scan = image::open(&path)
            .unwrap_or_else(|e| panic!("opening {path}: {e}"))
            .into_rgba8();
        let mut preview = imageops::resize(&scan, 718, 1024, imageops::FilterType::Lanczos3);

        let mut ink = 0u32;
        for element in ELEMENTS {
            for ((x, y), covered) in refmask::segment(&scan, &refmask::region(element)) {
                if covered {
                    preview.put_pixel(x, y, image::Rgba([0, 220, 0, 255]));
                    ink += 1;
                }
            }
        }

        let out = format!("tests/fixtures/{}_ref_preview.png", card.slug);
        preview
            .save(&out)
            .unwrap_or_else(|e| panic!("saving {out}: {e}"));
        println!("{out}: {ink} ink px");
    }
    println!("\nGreen is what the suite treats as ground truth. Check every region.");
}

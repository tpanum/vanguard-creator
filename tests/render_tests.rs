//! Accuracy suite: how faithfully can `vgc` re-generate a true original Vanguard?
//!
//! Every (card, element) pair lives in `common::CASES`. Each test renders one
//! element through the same `render::draw_*` entry point that `render_card`
//! uses, scores its text pixels against a reference mask cut from a scan of the
//! original card, and prints a diagnosis explaining the mismatch.
//!
//!   cargo test                              — score every case
//!   cargo test -- --nocapture               — score + per-case diagnosis
//!   cargo test -- --ignored --nocapture     — the summary table (accuracy_report)
//!   UPDATE_FIXTURES=1 cargo test            — also write rendered/diff PNGs
//!
//! Thresholds encode hard-won knowledge about correct output. A failing test
//! means the render regressed; fix the render, not the threshold.

mod common;

use common::{case, run_case, Diagnosis, Element, CASES};

macro_rules! accuracy_tests {
    ($($name:ident => ($card:literal, $element:ident);)*) => {
        $(
            #[test]
            fn $name() {
                run_case(case($card, Element::$element));
            }
        )*
    };
}

accuracy_tests! {
    gerrard_title        => ("Gerrard",      Title);
    gerrard_rules        => ("Gerrard",      Rules);
    gerrard_left_bubble  => ("Gerrard",      LeftBubble);
    gerrard_right_bubble => ("Gerrard",      RightBubble);

    silverqueen_title        => ("Sliver Queen", Title);
    silverqueen_rules        => ("Sliver Queen", Rules);
    silverqueen_left_bubble  => ("Sliver Queen", LeftBubble);
    silverqueen_right_bubble => ("Sliver Queen", RightBubble);

    sidar_title        => ("Sidar Kondo", Title);
    sidar_rules        => ("Sidar Kondo", Rules);
    sidar_left_bubble  => ("Sidar Kondo", LeftBubble);
    sidar_right_bubble => ("Sidar Kondo", RightBubble);

    volrath_title        => ("Volrath", Title);
    volrath_rules        => ("Volrath", Rules);
    volrath_left_bubble  => ("Volrath", LeftBubble);
    volrath_right_bubble => ("Volrath", RightBubble);
}

/// One-glance view of the whole suite: current F1, headroom available from
/// pure repositioning/resizing, and the mean across every case.
///
///   cargo test --test render_tests -- --ignored accuracy_report --nocapture
#[test]
#[ignore]
fn accuracy_report() {
    println!(
        "\n{:<28} {:>7} {:>7} {:>7}  {:>9}  {:>8}  {:>7}",
        "case", "F1", "margin", "aligned", "shift", "scale", "ink"
    );
    println!("{}", "─".repeat(86));

    let mut f1_sum = 0.0;
    let mut aligned_sum = 0.0;
    let mut worst: Option<(f64, String)> = None;

    for c in CASES {
        let rendered = c.element.render(c.yaml);
        let got = common::to_binary(&rendered, 128, true);
        let reference = common::load_mask(c.mask, &rendered);
        let d = Diagnosis::new(&got, &reference);

        let ink_ratio = if d.ink_ref > 0 {
            d.ink_got as f64 / d.ink_ref as f64
        } else {
            0.0
        };
        println!(
            "{:<28} {:>6.1}% {:>+6.1} {:>6.1}%  {:>4} {:>4}  {:>7.2}×  {:>6.2}×",
            c.label(),
            d.raw.f1 * 100.0,
            (d.raw.f1 - c.threshold) * 100.0,
            d.aligned.f1 * 100.0,
            format!("{:+}", d.shift.0),
            format!("{:+}", d.shift.1),
            d.scale,
            ink_ratio,
        );

        f1_sum += d.raw.f1;
        aligned_sum += d.aligned.f1;
        if worst.as_ref().is_none_or(|(w, _)| d.raw.f1 < *w) {
            worst = Some((d.raw.f1, c.label()));
        }
    }

    let n = CASES.len() as f64;
    println!("{}", "─".repeat(86));
    println!(
        "{:<28} {:>6.1}% {:>6} {:>6.1}%",
        "MEAN",
        f1_sum / n * 100.0,
        "",
        aligned_sum / n * 100.0
    );
    if let Some((f1, label)) = worst {
        println!("worst: {label} at {:.1}%", f1 * 100.0);
    }
    println!(
        "\n'aligned' is the F1 reachable by translating and uniformly scaling the render \
         alone.\nThe gap between F1 and aligned is placement error (fixable from \
         layout.rs);\nwhat is left below 100% is glyph shape, weight, and line breaking.\n"
    );
}

// ── Smoke test: full card renders without error ───────────────────────────────

#[test]
fn full_card_renders() {
    use std::path::Path;
    use vgc::{card::CardDef, fonts::Fonts, render};

    let fonts = Fonts::load().expect("fonts");
    for yaml in &[
        "tests/gerrard.yaml",
        "tests/silverqueen.yaml",
        "tests/sidar.yaml",
        "tests/volrath.yaml",
    ] {
        let card = CardDef::load(Path::new(yaml)).expect("load yaml");
        render::render_card(&card, None, None, &fonts).expect("render_card");
    }
}

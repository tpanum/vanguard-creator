//! Accuracy suite: how faithfully can `vgc` re-generate a true original Vanguard?
//!
//! Every card in `tests/assets/` contributes four cases — title, rules, left
//! bubble, right bubble. Each renders one element through the same
//! `render::draw_*` entry point that `render_card` uses, scores its text pixels
//! against a reference segmented from the card's scan, and prints a diagnosis
//! explaining the mismatch.
//!
//!   cargo test                              — score every case
//!   cargo test -- --nocapture               — score + per-case diagnosis
//!   cargo test -- --ignored --nocapture     — the summary table (accuracy_report)
//!   UPDATE_FIXTURES=1 cargo test            — also write rendered/diff PNGs
//!
//! Thresholds encode hard-won knowledge about correct rendering. A failing test
//! means the render regressed; fix the render, not the threshold.

mod common;

use common::{cases, run_case, Diagnosis, CARDS};

/// One test per card, covering all four of its elements, so a failure names the
/// card and the report names the element.
macro_rules! card_tests {
    ($($name:ident => $slug:literal;)*) => {
        $(
            #[test]
            fn $name() {
                for case in cases().iter().filter(|c| c.slug == $slug) {
                    run_case(case);
                }
            }
        )*
    };
}

card_tests! {
    ashnod      => "ashnod";
    crovax      => "crovax";
    eladamri    => "eladamri";
    ertai       => "ertai";
    gerrard     => "gerrard";
    hanna       => "hanna";
    maraxus     => "maraxus";
    mishra      => "mishra";
    multani     => "multani";
    oracle      => "oracle";
    orim        => "orim";
    rofellos    => "rofellos";
    selenia     => "selenia";
    serra       => "serra";
    sidarkondo  => "sidarkondo";
    silverqueen => "silverqueen";
    sisay       => "sisay";
    starke      => "starke";
    tahngarth   => "tahngarth";
    takara      => "takara";
    tawnos      => "tawnos";
    titania     => "titania";
    urza        => "urza";
    volrath     => "volrath";
    xantcha     => "xantcha";
}

/// One-glance view of the whole suite: current F1, headroom available from
/// pure repositioning/resizing, and the mean across every case.
///
///   cargo test --release --test render_tests -- --ignored accuracy_report --nocapture
#[test]
#[ignore]
fn accuracy_report() {
    println!(
        "\n{:<34} {:>9} {:>7} {:>8}  {:>9}  {:>8}  {:>7}",
        "case", "overall", "margin", "shape", "shift", "scale", "ink"
    );
    println!("{}", "─".repeat(92));

    let mut f1_sum = 0.0;
    let mut aligned_sum = 0.0;
    let mut per_element = [(0.0f64, 0.0f64, 0usize); 4];
    let mut worst: Option<(f64, String)> = None;

    for c in cases() {
        let rendered = c.element.render(&c.yaml());
        let got = common::to_binary(&rendered, 128, true);
        let reference = c.reference();
        let d = Diagnosis::new(&got, reference);

        let ink_ratio = if d.ink_ref > 0 {
            d.ink_got as f64 / d.ink_ref as f64
        } else {
            0.0
        };
        println!(
            "{:<34} {:>8.1}% {:>6} {:>7.1}%  {:>4} {:>4}  {:>7.2}×  {:>6.2}×",
            c.label(),
            d.raw.f1 * 100.0,
            c.threshold.map_or_else(
                || "—".to_string(),
                |(overall, _)| format!("{:+.1}", (d.raw.f1 - overall) * 100.0)
            ),
            d.aligned.f1 * 100.0,
            format!("{:+}", d.shift.0),
            format!("{:+}", d.shift.1),
            d.scale,
            ink_ratio,
        );

        f1_sum += d.raw.f1;
        aligned_sum += d.aligned.f1;
        let slot = &mut per_element[c.element as usize];
        slot.0 += d.raw.f1;
        slot.1 += d.aligned.f1;
        slot.2 += 1;
        if worst.as_ref().is_none_or(|(w, _)| d.raw.f1 < *w) {
            worst = Some((d.raw.f1, c.label()));
        }
    }

    let n = cases().len() as f64;
    println!("{}", "─".repeat(92));
    for (element, (overall, shape, count)) in ["title", "rules", "left bubble", "right bubble"]
        .iter()
        .zip(per_element)
    {
        println!(
            "{:<34} {:>8.1}% {:>6} {:>7.1}%",
            format!("mean {element}"),
            overall / count as f64 * 100.0,
            "",
            shape / count as f64 * 100.0
        );
    }
    println!(
        "{:<34} {:>8.1}% {:>6} {:>7.1}%",
        format!("MEAN over {} cards", CARDS.len()),
        f1_sum / n * 100.0,
        "",
        aligned_sum / n * 100.0
    );
    if let Some((f1, label)) = worst {
        println!("worst: {label} at {:.1}%", f1 * 100.0);
    }
    println!(
        "\n'overall' is placement and rendering together — the original single score.\n\
         'shape' is measured after the best translation and uniform scale, so it \
         judges the\nrendering alone: typeface, weight, line breaking. The gap between \
         them is\nplacement error, fixable from layout.rs.\n"
    );
}

// ── Smoke test: a full card renders without error ─────────────────────────────

#[test]
fn full_card_renders() {
    use std::path::Path;
    use vgc::{card::CardDef, fonts::Fonts, render};

    let fonts = Fonts::load().expect("fonts");
    for card in CARDS {
        let path = format!("tests/cards/{}.yaml", card.slug);
        let def = CardDef::load(Path::new(&path)).expect("load yaml");
        render::render_card(&def, None, None, &fonts).expect("render_card");
    }
}

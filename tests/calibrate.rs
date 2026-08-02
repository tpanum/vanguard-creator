//! Layout calibration: search `layout::Layout` constants for the values that
//! maximise text-pixel F1 against the original-card reference masks.
//!
//! The accuracy suite tells you a render is wrong and roughly how. This tells
//! you what to set. It is a coordinate descent: one knob at a time, sweep its
//! range, keep the value with the best mean F1 over the cases that knob can
//! affect, repeat until nothing moves.
//!
//!   cargo test --release --test calibrate -- --ignored --nocapture
//!
//! Nothing here is asserted and nothing is written — it prints a suggested
//! `DEFAULT` block for a human to review and paste into `src/layout.rs`. The
//! reference set is four cards, so treat a change worth less than a few tenths
//! of a percent as noise rather than signal.
//!
//! `ink_gain` is deliberately absent from the descent. F1 scores binarized
//! images and is therefore blind to antialiasing, so it rewards driving the
//! gain to zero — which solidifies every edge pixel and makes the real card
//! look jagged. That knob is set by `tune_ink_gain` against ink volume instead.

mod common;

use vgc::layout::{Layout, DEFAULT};

use common::{cases, text_f1, to_binary, Case, Ctx, Element};

// ── Objective ─────────────────────────────────────────────────────────────────

/// Reference masks are expensive to load (a Lanczos resize of a ~1000×1500
/// scan), and the search re-scores them thousands of times. Load once.
struct Refs {
    ctx: Ctx,
}

impl Refs {
    fn load() -> Refs {
        Refs {
            ctx: Ctx::production(),
        }
    }

    fn f1(&self, case: &Case, layout: &Layout) -> f64 {
        let ctx = Ctx {
            layout: layout.clone(),
            name_font: self.ctx.name_font.clone(),
            body_font: self.ctx.body_font.clone(),
            stats_font: self.ctx.stats_font.clone(),
        };
        let rendered = case.element.render_with(&case.yaml(), &ctx);
        let got = to_binary(&rendered, 128, true);
        text_f1(&got, case.reference()).2
    }

    /// Mean F1 over the cases whose element is in `elements`.
    fn mean_f1(&self, layout: &Layout, elements: &[Element]) -> f64 {
        let scored: Vec<f64> = cases()
            .iter()
            .filter(|c| elements.contains(&c.element))
            .map(|c| self.f1(c, layout))
            .collect();
        scored.iter().sum::<f64>() / scored.len() as f64
    }
}

const ALL: &[Element] = &[
    Element::Title,
    Element::Rules,
    Element::LeftBubble,
    Element::RightBubble,
];

// ── Knobs ─────────────────────────────────────────────────────────────────────

/// One tunable layout constant: how to read it, how to write it, and the range
/// worth searching.
struct Knob {
    name: &'static str,
    /// Cases this knob can possibly change — scoring the rest is wasted work.
    elements: &'static [Element],
    get: fn(&Layout) -> f64,
    set: fn(&mut Layout, f64),
    span: f64,
    step: f64,
}

fn knobs() -> Vec<Knob> {
    const TITLE: &[Element] = &[Element::Title];
    const RULES: &[Element] = &[Element::Rules];
    const LEFT: &[Element] = &[Element::LeftBubble];
    const RIGHT: &[Element] = &[Element::RightBubble];
    const BUBBLES: &[Element] = &[Element::LeftBubble, Element::RightBubble];

    vec![
        Knob {
            name: "name_center.0",
            elements: TITLE,
            get: |l| l.name_center.0 as f64,
            set: |l, v| l.name_center.0 = v.max(0.0) as u32,
            span: 10.0,
            step: 1.0,
        },
        Knob {
            name: "name_center.1",
            elements: TITLE,
            get: |l| l.name_center.1 as f64,
            set: |l, v| l.name_center.1 = v.max(0.0) as u32,
            span: 10.0,
            step: 1.0,
        },
        Knob {
            name: "name_scale.0",
            elements: TITLE,
            get: |l| l.name_scale.0 as f64,
            set: |l, v| l.name_scale.0 = v as f32,
            span: 8.0,
            step: 0.5,
        },
        Knob {
            name: "name_scale.1",
            elements: TITLE,
            get: |l| l.name_scale.1 as f64,
            set: |l, v| l.name_scale.1 = v as f32,
            span: 8.0,
            step: 0.5,
        },
        Knob {
            name: "name_max_width",
            elements: TITLE,
            get: |l| l.name_max_width as f64,
            set: |l, v| l.name_max_width = v as f32,
            span: 40.0,
            step: 5.0,
        },
        Knob {
            name: "text_box.x (both edges)",
            elements: RULES,
            get: |l| l.text_box.left as f64,
            set: |l, v| {
                let d = v - l.text_box.left as f64;
                l.text_box.left = v.max(0.0) as u32;
                l.text_box.right = (l.text_box.right as f64 + d).max(0.0) as u32;
            },
            span: 12.0,
            step: 1.0,
        },
        Knob {
            name: "text_box.width",
            elements: RULES,
            get: |l| l.text_box.right as f64,
            set: |l, v| l.text_box.right = v.max(0.0) as u32,
            span: 20.0,
            step: 2.0,
        },
        Knob {
            name: "text_padding",
            elements: RULES,
            get: |l| l.text_padding as f64,
            set: |l, v| l.text_padding = v.max(0.0) as u32,
            span: 16.0,
            step: 2.0,
        },
        Knob {
            name: "ability_size",
            elements: RULES,
            get: |l| l.ability_size as f64,
            set: |l, v| l.ability_size = v.max(1.0) as u32,
            span: 4.0,
            step: 1.0,
        },
        Knob {
            name: "line_height_factor",
            elements: RULES,
            get: |l| l.line_height_factor as f64,
            set: |l, v| l.line_height_factor = v as f32,
            span: 0.3,
            step: 0.025,
        },
        Knob {
            name: "rules_normal_height",
            elements: RULES,
            get: |l| l.rules_normal_height as f64,
            set: |l, v| l.rules_normal_height = v as f32,
            span: 20.0,
            step: 2.0,
        },
        Knob {
            name: "rules_centering_height",
            elements: RULES,
            get: |l| l.rules_centering_height as f64,
            set: |l, v| l.rules_centering_height = v as f32,
            span: 40.0,
            step: 4.0,
        },
        Knob {
            name: "hand_center.0",
            elements: LEFT,
            get: |l| l.hand_center.0 as f64,
            set: |l, v| l.hand_center.0 = v.max(0.0) as u32,
            span: 8.0,
            step: 1.0,
        },
        Knob {
            name: "hand_center.1",
            elements: LEFT,
            get: |l| l.hand_center.1 as f64,
            set: |l, v| l.hand_center.1 = v.max(0.0) as u32,
            span: 8.0,
            step: 1.0,
        },
        Knob {
            name: "life_center.0",
            elements: RIGHT,
            get: |l| l.life_center.0 as f64,
            set: |l, v| l.life_center.0 = v.max(0.0) as u32,
            span: 8.0,
            step: 1.0,
        },
        Knob {
            name: "life_center.1",
            elements: RIGHT,
            get: |l| l.life_center.1 as f64,
            set: |l, v| l.life_center.1 = v.max(0.0) as u32,
            span: 8.0,
            step: 1.0,
        },
        Knob {
            name: "symbol_scale",
            elements: RULES,
            get: |l| l.symbol_scale as f64,
            set: |l, v| l.symbol_scale = v as f32,
            span: 0.4,
            step: 0.05,
        },
        Knob {
            name: "symbol_y_offset",
            elements: RULES,
            get: |l| l.symbol_y_offset as f64,
            set: |l, v| l.symbol_y_offset = v as f32,
            span: 6.0,
            step: 1.0,
        },
        Knob {
            name: "stats_size",
            elements: BUBBLES,
            get: |l| l.stats_size as f64,
            set: |l, v| l.stats_size = v as f32,
            span: 10.0,
            step: 0.5,
        },
        // Only the four two-digit life totals can move with this one, so its
        // mean is over four cases and worth reading per card before trusting.
        Knob {
            name: "stats_multi_digit_tracking",
            elements: RIGHT,
            get: |l| l.stats_multi_digit_tracking as f64,
            set: |l, v| l.stats_multi_digit_tracking = v as f32,
            span: 6.0,
            step: 0.25,
        },
    ]
}

// ── Search ────────────────────────────────────────────────────────────────────

/// Sweep one knob across its span, returning the best value and its score.
fn sweep(refs: &Refs, layout: &Layout, knob: &Knob) -> (f64, f64) {
    let base = (knob.get)(layout);
    let steps = (knob.span / knob.step).round() as i32;

    let mut best = (base, {
        let mut l = layout.clone();
        (knob.set)(&mut l, base);
        refs.mean_f1(&l, knob.elements)
    });

    for i in -steps..=steps {
        let v = base + i as f64 * knob.step;
        let mut l = layout.clone();
        (knob.set)(&mut l, v);
        let score = refs.mean_f1(&l, knob.elements);
        // Strictly better only, so the incumbent wins ties and the search does
        // not drift on noise.
        if score > best.1 + 1e-9 {
            best = (v, score);
        }
    }
    best
}

#[test]
#[ignore]
fn calibrate_layout() {
    let refs = Refs::load();
    let mut layout = DEFAULT.clone();
    let start = refs.mean_f1(&layout, ALL);

    println!(
        "\nstarting mean F1 over all {} cases: {:.2}%",
        cases().len(),
        start * 100.0
    );
    println!("{}", "─".repeat(72));

    let all_knobs = knobs();
    let mut changed_any = false;

    for pass in 1..=3 {
        let mut moved = false;
        for knob in &all_knobs {
            let before = (knob.get)(&layout);
            let (best, score) = sweep(&refs, &layout, knob);
            if (best - before).abs() > 1e-9 {
                let prev = {
                    let mut l = layout.clone();
                    (knob.set)(&mut l, before);
                    refs.mean_f1(&l, knob.elements)
                };
                println!(
                    "pass {pass}  {:<24} {:>8.3} → {:<8.3}  element F1 {:.2}% → {:.2}% ({:+.2})",
                    knob.name,
                    before,
                    best,
                    prev * 100.0,
                    score * 100.0,
                    (score - prev) * 100.0,
                );
                (knob.set)(&mut layout, best);
                moved = true;
                changed_any = true;
            }
        }
        if !moved {
            println!("pass {pass}: converged");
            break;
        }
    }

    let end = refs.mean_f1(&layout, ALL);
    println!("{}", "─".repeat(72));
    println!(
        "final mean F1 over all {} cases: {:.2}%  ({:+.2} points)",
        cases().len(),
        end * 100.0,
        (end - start) * 100.0
    );

    if !changed_any {
        println!("\nlayout.rs is already at a local optimum for this reference set.");
        return;
    }

    println!("\nsuggested src/layout.rs values (review before pasting):");
    for knob in &all_knobs {
        let before = (knob.get)(&DEFAULT);
        let after = (knob.get)(&layout);
        if (before - after).abs() > 1e-9 {
            println!("    {:<24} {:>8.3}  (was {:.3})", knob.name, after, before);
        }
    }
    println!();
}

/// Choose `ink_gain` against ink *volume*, not F1.
///
/// F1 compares binarized images, so it cannot see antialiasing at all: pushing
/// the gain toward zero turns every partially covered edge pixel into solid
/// ink, which reliably raises F1 while making the actual rendered card look
/// jagged and overinked. The honest target is the gain at which the render
/// lays down the same amount of ink as the original — ratio 1.00 — which is a
/// property of the print, not of the scoring function.
///
///   cargo test --release --test calibrate -- --ignored tune_ink_gain --nocapture
#[test]
#[ignore]
fn tune_ink_gain() {
    let refs = Refs::load();

    println!(
        "\n{:>9}{:>12}{:>14}{:>26}",
        "ink_gain", "mean F1", "ink ratio", ""
    );
    println!("{}", "─".repeat(62));

    for i in 0..=26 {
        let gain = 1.65 - i as f64 * 0.05;
        let mut layout = DEFAULT.clone();
        layout.ink_gain = gain as f32;

        let (mut f1_sum, mut ratio_sum) = (0.0, 0.0);
        for c in cases() {
            let ctx = Ctx {
                layout: layout.clone(),
                name_font: refs.ctx.name_font.clone(),
                body_font: refs.ctx.body_font.clone(),
                stats_font: refs.ctx.stats_font.clone(),
            };
            let rendered = c.element.render_with(&c.yaml(), &ctx);
            let got = to_binary(&rendered, 128, true);
            let reference = c.reference();
            f1_sum += text_f1(&got, reference).2;
            let ink_got = got.pixels().filter(|p| p[0] == 0).count() as f64;
            let ink_ref = reference.pixels().filter(|p| p[0] == 0).count() as f64;
            ratio_sum += ink_got / ink_ref;
        }
        let n = cases().len() as f64;
        let ratio = ratio_sum / n;
        println!(
            "{:>9.2}{:>11.2}%{:>13.3}   {}",
            gain,
            f1_sum / n * 100.0,
            ratio,
            if (ratio - 1.0).abs() < 0.02 {
                "← matches the original's ink volume"
            } else {
                ""
            }
        );
    }
    println!(
        "\nPick the gain nearest ratio 1.00. A lower gain scores higher but only \n\
         because the metric is blind to antialiasing.\n"
    );
}

/// Per-card breakdown of a single knob, so a suspicious global suggestion can
/// be checked for whether the cards actually agree or one outlier is driving it.
///
///   KNOB=stats_size cargo test --release --test calibrate -- --ignored explain_knob --nocapture
#[test]
#[ignore]
fn explain_knob() {
    let Ok(target) = std::env::var("KNOB") else {
        println!("set KNOB=<name> to use this; names:");
        for k in knobs() {
            println!("  {}", k.name);
        }
        return;
    };
    let all_knobs = knobs();
    let Some(knob) = all_knobs.iter().find(|k| k.name == target) else {
        println!("unknown knob {target:?}");
        return;
    };

    let refs = Refs::load();
    let base = (knob.get)(&DEFAULT);
    let steps = (knob.span / knob.step).round() as i32;

    let selected: Vec<&Case> = cases()
        .iter()
        .filter(|c| knob.elements.contains(&c.element))
        .collect();

    print!("\n{:>10}", knob.name);
    for c in &selected {
        print!(" {:>14}", c.card);
    }
    println!(" {:>8}", "mean");

    for i in -steps..=steps {
        let v = base + i as f64 * knob.step;
        let mut l = DEFAULT.clone();
        (knob.set)(&mut l, v);
        print!("{:>10.3}", v);
        let mut sum = 0.0;
        for c in &selected {
            let f1 = refs.f1(c, &l);
            sum += f1;
            print!(" {:>13.1}%", f1 * 100.0);
        }
        let mean = sum / selected.len() as f64;
        println!(
            " {:>7.1}%{}",
            mean * 100.0,
            if (v - base).abs() < 1e-9 {
                "  ← current"
            } else {
                ""
            }
        );
    }
    println!();
}

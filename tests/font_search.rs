//! Font search: score candidate typefaces against the original-card masks.
//!
//! Picking a font by eye is exactly the subjective judgement the F1 metric
//! exists to replace. This scores every candidate the same way the accuracy
//! suite scores the production font, and reports the winner per element group
//! — the rules text and the two stat bubbles are set in the body face, the
//! title in its own face, and they do not have to agree.
//!
//!   cargo test --release --test font_search -- --ignored --nocapture
//!
//! Candidates are the fonts bundled in `assets/fonts/fonts.tar.zst`, plus
//! anything in a directory named by `FONT_DIR` (for trialling a face before
//! committing to bundling it).
//!
//! Each candidate is scored twice: at the production size, and at its own
//! best size. A face with different metrics needs a different point size to
//! set the same physical text, so comparing at a fixed size would reject a
//! better face for the wrong reason.

mod common;

use ab_glyph::FontRef;
use vgc::layout::{Layout, DEFAULT};

use common::{cases, text_f1, to_binary, Case, Ctx, Element};

struct Candidate {
    name: String,
    font: FontRef<'static>,
}

/// Bundled fonts, plus any `.ttf`/`.otf` in `$FONT_DIR`.
fn candidates() -> Vec<Candidate> {
    let mut out = Vec::new();

    for name in ["Mplantin.ttf", "Mplantin-Bold.ttf", "Fremont-Regular.ttf"] {
        match FontRef::try_from_slice(vgc::bundle::font(name)) {
            Ok(font) => out.push(Candidate {
                name: format!("bundled/{name}"),
                font,
            }),
            Err(e) => eprintln!("skipping bundled {name}: {e}"),
        }
    }

    if let Ok(dir) = std::env::var("FONT_DIR") {
        let mut entries: Vec<_> = std::fs::read_dir(&dir)
            .unwrap_or_else(|e| panic!("reading FONT_DIR {dir}: {e}"))
            .filter_map(Result::ok)
            .map(|e| e.path())
            .filter(|p| {
                p.extension()
                    .and_then(|e| e.to_str())
                    .is_some_and(|e| matches!(e.to_ascii_lowercase().as_str(), "ttf" | "otf"))
            })
            .collect();
        entries.sort();
        for path in entries {
            match common::load_font_file(&path) {
                Ok(font) => out.push(Candidate {
                    name: path.file_name().unwrap().to_string_lossy().into_owned(),
                    font,
                }),
                Err(e) => eprintln!("skipping {}: {e}", path.display()),
            }
        }
    }

    out
}

/// Masks are cached inside `common::refmask`, so this exists only to give the
/// scoring helpers a home.
struct Refs;

impl Refs {
    fn load() -> Refs {
        Refs
    }

    fn f1(&self, case: &Case, ctx: &Ctx) -> f64 {
        let rendered = case.element.render_with(&case.yaml(), ctx);
        let got = to_binary(&rendered, 128, true);
        text_f1(&got, case.reference()).2
    }

    fn mean(&self, ctx: &Ctx, elements: &[Element]) -> f64 {
        let v: Vec<f64> = cases()
            .iter()
            .filter(|c| elements.contains(&c.element))
            .map(|c| self.f1(c, ctx))
            .collect();
        v.iter().sum::<f64>() / v.len() as f64
    }
}

/// Build a Ctx that differs from production only in the field under test.
fn ctx_with(
    layout: Layout,
    body: Option<&FontRef<'static>>,
    name: Option<&FontRef<'static>>,
) -> Ctx {
    let base = Ctx::production();
    Ctx {
        layout,
        body_font: body.cloned().unwrap_or(base.body_font),
        name_font: name.cloned().unwrap_or(base.name_font),
    }
}

/// Best mean F1 for `elements` over a sweep of the size knob, so faces with
/// different metrics compete fairly.
fn best_over_sizes(
    refs: &Refs,
    cands: &Candidate,
    elements: &[Element],
    sizes: &[f64],
    set_size: fn(&mut Layout, f64),
    is_title: bool,
) -> (f64, f64) {
    let mut best = (f64::NAN, -1.0);
    for &s in sizes {
        let mut layout = DEFAULT.clone();
        set_size(&mut layout, s);
        let ctx = if is_title {
            ctx_with(layout, None, Some(&cands.font))
        } else {
            ctx_with(layout, Some(&cands.font), None)
        };
        let f1 = refs.mean(&ctx, elements);
        if f1 > best.1 {
            best = (s, f1);
        }
    }
    best
}

#[test]
#[ignore]
fn search_body_font() {
    let refs = Refs::load();
    let cands = candidates();

    const RULES: &[Element] = &[Element::Rules];
    const BUBBLES: &[Element] = &[Element::LeftBubble, Element::RightBubble];

    let ability_sizes: Vec<f64> = (20..=28).map(|v| v as f64).collect();
    let stat_sizes: Vec<f64> = (0..=20).map(|i| 25.0 + i as f64 * 0.5).collect();

    println!(
        "\n{:<28} {:>10} {:>12} {:>12} {:>12} {:>14}",
        "body font candidate",
        "rules F1",
        "@ability_size",
        "best rules",
        "bubbles F1",
        "@stats_size"
    );
    println!("{}", "─".repeat(94));

    for c in &cands {
        let at_default = refs.mean(
            &ctx_with(DEFAULT.clone(), Some(&c.font), None),
            &[Element::Rules],
        );
        let (best_ab, best_rules) = best_over_sizes(
            &refs,
            c,
            RULES,
            &ability_sizes,
            |l, v| l.ability_size = v as u32,
            false,
        );
        let bub_default = refs.mean(&ctx_with(DEFAULT.clone(), Some(&c.font), None), BUBBLES);
        let (best_stat, best_bub) = best_over_sizes(
            &refs,
            c,
            BUBBLES,
            &stat_sizes,
            |l, v| l.stats_size = v as f32,
            false,
        );

        println!(
            "{:<28} {:>9.1}% {:>12} {:>11.1}% {:>11.1}% {:>13}",
            c.name,
            at_default * 100.0,
            format!("{:.0} → {:.0}", DEFAULT.ability_size, best_ab),
            best_rules * 100.0,
            bub_default * 100.0,
            format!(
                "{:.1} → {:.1} = {:.1}%",
                DEFAULT.stats_size,
                best_stat,
                best_bub * 100.0
            ),
        );
    }
    println!(
        "\nproduction body font is Mplantin-Bold. 'best' columns re-fit the size knob \
         per face,\nso a face is not penalised merely for having different metrics.\n"
    );
}

#[test]
#[ignore]
fn search_title_font() {
    let refs = Refs::load();
    let cands = candidates();
    const TITLE: &[Element] = &[Element::Title];

    let sizes: Vec<f64> = (0..=16).map(|i| 49.0 + i as f64).collect();

    println!(
        "\n{:<28} {:>10} {:>16} {:>12}",
        "title font candidate", "title F1", "@name_scale.1", "best title"
    );
    println!("{}", "─".repeat(70));

    for c in &cands {
        let at_default = refs.mean(&ctx_with(DEFAULT.clone(), None, Some(&c.font)), TITLE);
        // Sweep the vertical scale, holding the horizontal stretch ratio fixed.
        let ratio = (DEFAULT.name_scale.0 / DEFAULT.name_scale.1) as f64;
        let mut best = (f64::NAN, -1.0);
        for &s in &sizes {
            let mut layout = DEFAULT.clone();
            layout.name_scale = ((s * ratio) as f32, s as f32);
            let f1 = refs.mean(&ctx_with(layout, None, Some(&c.font)), TITLE);
            if f1 > best.1 {
                best = (s, f1);
            }
        }
        println!(
            "{:<28} {:>9.1}% {:>16} {:>11.1}%",
            c.name,
            at_default * 100.0,
            format!("{:.0} → {:.0}", DEFAULT.name_scale.1, best.0),
            best.1 * 100.0
        );
    }
    println!("\nproduction title font is Fremont-Regular.\n");
}

//! Report short last lines ("widows") in auto-wrapped ability text.
//!
//! A paragraph whose last line carries one short word reads as a mistake on a
//! centered card: the eye sees a full measure, then a stub. This walks a
//! directory of card YAML, wraps each paragraph exactly as `render` does, and
//! prints every last line narrower than a given fraction of the measure.
//!
//! ```sh
//! cargo run --release --example widows -- ../bug-vanguards/vanguards
//! ```
//!
//! No original is affected by anything this measures: all 25 carry their
//! printed line breaks as `\n`, so none of them auto-wraps at all.

use std::path::PathBuf;

use vgc::{
    card::{self, CardDef},
    fonts::Fonts,
    layout::DEFAULT,
    text::{self, Token, WrappedLine},
};

/// Report a last line as a widow below this fraction of the full measure.
const WIDOW_FRACTION: f32 = 0.35;

fn main() {
    let root = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "../bug-vanguards/vanguards".to_string());
    let files = card::collect_yaml_files(&[PathBuf::from(root)], true).expect("collecting yaml");
    let fonts = Fonts::load().expect("fonts");
    let layout = &DEFAULT;

    let mut widows = 0usize;
    let mut cards = 0usize;
    let mut flagged = 0usize;

    for path in &files {
        let Ok(card) = CardDef::load(path) else {
            continue;
        };
        cards += 1;

        let fit = text::fit_rules_text(&card.ability, None, &fonts.body, &fonts.body, layout);
        let spec = fit.spec;
        let mut hits: Vec<(String, f32, f32)> = Vec::new();

        // Walk the wrapped lines paragraph by paragraph; the last Tokens line
        // before a break (or at the end) is the one that can be a widow.
        let all: Vec<&WrappedLine> = fit.lines.iter().chain(fit.narrow_lines.iter()).collect();
        let measure = |i: usize| {
            if i < fit.lines.len() {
                layout.rules_width()
            } else {
                layout.rules_width_narrow()
            }
        };

        let mut run: Vec<(usize, &WrappedLine)> = Vec::new();
        let flush = |run: &mut Vec<(usize, &WrappedLine)>, hits: &mut Vec<_>| {
            if run.len() >= 2 {
                if let Some((i, WrappedLine::Tokens(line))) = run.last() {
                    let w = text::measure_tokens(
                        &line.tokens,
                        &fonts.body,
                        spec.scale,
                        spec.symbol_size,
                    );
                    if w < measure(*i) * WIDOW_FRACTION {
                        let s: String = line
                            .tokens
                            .iter()
                            .map(|t| match t {
                                Token::Text(t) => t.clone(),
                                Token::Symbol(n) => format!("{{{n}}}"),
                            })
                            .collect();
                        hits.push((s, w, measure(*i)));
                    }
                }
            }
            run.clear();
        };

        for (i, line) in all.iter().enumerate() {
            match line {
                WrappedLine::Tokens(_) => run.push((i, line)),
                _ => flush(&mut run, &mut hits),
            }
        }
        flush(&mut run, &mut hits);

        if !hits.is_empty() {
            flagged += 1;
            widows += hits.len();
            println!("{}", card.name);
            for (s, w, m) in hits {
                println!(
                    "  {:5.0}px of {:3.0}  ({:2.0}%)  {:?}",
                    w,
                    m,
                    w / m * 100.0,
                    s
                );
            }
        }
    }

    println!(
        "\n{widows} widow line(s) on {flagged} of {cards} cards \
         (last line under {:.0}% of the measure)",
        WIDOW_FRACTION * 100.0
    );
}

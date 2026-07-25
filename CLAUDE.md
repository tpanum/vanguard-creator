# CLAUDE.md
A command-line tool for creating custom Magic: The Gathering Vanguard cards. Define cards in YAML, provide artwork, and `vgc` composites them onto an authentic card template with proper typography, mana symbols, and print-ready output.

## Asset embedding philosophy

We strive to embed everything required to produce great-looking Vanguard cards directly into the binary — fonts, mana symbols, and the card template are all bundled so that `vgc` works out of the box with zero setup. All bundled assets are packed into a single zstd-compressed tar archive (`bundled.tar.zst`, level 3) at build time via `build.rs` and included with `include_bytes!`. Assets are extracted lazily at runtime via `src/bundle.rs`.

Where customisation makes sense (e.g. swapping the card template for a fan-made variant), we expose optional CLI flags that override the embedded default — but the embedded version is always the fallback. No external files should ever be *required* for a standard render.

## Testing text rendering

Whenever designing or modifying text insertion (placement, sizing, font, layout), you MUST use the text-pixel F1 score as the feedback metric. Raw pixel diff is useless here because the template background dominates the signal.

The suite covers all 25 cards in `tests/assets/`, four elements each. The method (driven by `tests/common/mod.rs`, one test per card):
1. For each text region (title, rules, left bubble, right bubble), render **only that element** onto a blank 718×1024 canvas using the exact same `render::draw_*` functions called by `render::render_card`.
2. Segment the same region out of the card scan, in the same 718×1024 space.
3. Binarize the render at luma < 128 (dark pixels = text).
4. Compute precision, recall, and F1 over the binary text-pixel masks.

### Reference masks

No mask is stored on disk. `tests/common/refmask.rs` segments all four regions out of the card scan at test time — local-background threshold at native resolution, frame and speckle components dropped, area-correct downsample to 718×1024 — and memoises the result. A mask is therefore a pure function of two version-controlled inputs: the scan and that file.

Do not go back to hand-traced masks. The ones this replaced carried ~20% more ink than the scans they came from and contained no mana symbols at all, which biased every measurement toward type that was too heavy and scored a correctly drawn `{3}` as a block of false positives.

Adding a card costs a scan at `tests/assets/<slug>.jpg`, a definition at `tests/cards/<slug>.yaml`, and a row in `CARDS` — no fixtures. After changing anything in `refmask.rs`, run `cargo test --release --test build_masks -- --ignored --nocapture` and look at the overlays before trusting any score built on the result.

### Reading a failure

Every case prints a diagnosis, not just a number: ink volume vs the reference, both bounding boxes, the translation and uniform scale that would maximise F1, the F1 those would reach, and a per-line table. The gap between `F1` and `aligned` is placement error you can fix from `layout.rs`; what remains below 100% is glyph shape, weight, and line breaking. `cargo test -- --ignored accuracy_report --nocapture` prints the whole suite as one table.

### Calibrating

`tests/calibrate.rs` coordinate-descends the `Layout` constants against mean F1 and prints suggested values; `KNOB=<name> … explain_knob` breaks a single knob down per card, which is how you tell a real layout error from one card's scan being off-register. **Always run `explain_knob` before accepting a suggestion.** The descent optimises a mean, so one card can carry it: `symbol_scale` was pushed to 1.40 by Hanna alone, buying a translation through the wrong knob, while the other three symbol cards all peaked near 0.95. `tests/font_search.rs` scores candidate typefaces, re-fitting the size knob per face so a face is not rejected merely for having different metrics.

### Where F1 must not be trusted

F1 compares *binarized* images, so it is blind to antialiasing and will always reward driving `layout.ink_gain` toward zero — which solidifies every partially covered edge pixel and makes the real card look jagged. `ink_gain` is therefore excluded from the calibrator and set by `tune_ink_gain`, which picks the value where the render lays down the same ink volume as the original (ratio ≈ 1.00). Any future knob that can trade antialiasing for coverage needs the same treatment.

**The test helpers MUST always call the same `text::*` functions, with the same arguments, as `render::render_card` does.** If the production render path changes, the corresponding test helper must be updated in the same commit. Tests that call different functions or use different parameters do not validate the actual output.

**Do NOT modify test cases (thresholds, reference fixtures, or test logic) without explicit permission from the user.** The tests encode hard-won knowledge about what correct rendering looks like. Lowering a threshold or updating a fixture to make a failing test pass hides a real regression — it does not fix it. If a test fails, investigate and fix the production code; do not adjust the test to accommodate broken output.

Run with `UPDATE_FIXTURES=1 cargo test -- --nocapture` to save rendered images and diff maps to `tests/fixtures/` for visual inspection.

## Text anchoring

Two anchors, and they are not interchangeable:

- **Stat bubbles** use `text::draw_text_centered_on_ink`. The target is a circle stamped on the template, so what must sit in its middle is the visible `-4`, not the typographic slot around it. Centering on the font's ascent-plus-descent reserves descender space that digits never use and pushes them ~3 px low.
- **Card names** use `text::draw_text_centered_on_baseline`. A banner needs every name on the same baseline, so the vertical position must depend only on the font and its size — never on whether the name happens to contain a descender. Ink-centering a title makes `Volrath` ride higher than `Sliver Queen, Brood Mother`.

## Original line breaks

The 1997 cards were broken by hand, not by a width rule — Sidar Kondo breaks after `+3/+3` with room to spare on the line. Where a card's true breaks are known, encode them as `\n` in its YAML; the auto-wrap is a fallback for new cards, and scoring against it measures the wrap heuristic rather than the rendering. All four reference cards carry their originals' breaks.

**The F1 score is the ground truth for rendering quality. If a change causes F1 to drop, the change made things worse — revert or iterate until F1 recovers or improves. If F1 improves, the change is an improvement. Do not override this metric with subjective impressions.**

**When a test score seems unusually low (e.g. below 10%), always generate and inspect the diff image before drawing conclusions.** The diff (black = correct, red = missed, green = extra) immediately reveals whether the problem is a positional offset, wrong font, wrong line-breaking, or a bad mask. Do not attempt to diagnose low scores from band statistics alone — look at the diff first.

## Scryfall API

Vanguard card metadata can be looked up via the Scryfall search API:

```
https://api.scryfall.com/cards/search?q=t%3Avanguard+name%3A<name>
```

The API reliably provides:
- `hand_modifier` — the hand size modifier shown in the left bubble (e.g. `"-4"`)
- `life_modifier` — the life total modifier shown in the right bubble (e.g. `"+0"`)
- `flavor_text` — lore text on the card

**Do NOT use `oracle_text` for the rules text.** Scryfall only stores modernized oracle text (e.g. "from the battlefield" instead of the original "from play"), which differs from what is printed on the physical cards and shown in the reference masks. Rules text must be sourced from card scans or other references to the original printed wording.

## Title font

The confirmed correct font for Vanguard card titles is **Fremont Regular** (SoftMaker Software GmbH), available at https://fontsgeek.com/fonts/Fremont-Regular. It is embedded as `assets/fonts/Fremont-Regular.ttf`.

## Title font observations

From examining original Vanguard cards:

- **Sliver Queen, Brood Mother**: the title text appears at natural (unstretched) proportions — `x_scale == y_scale`.
- **Gerrard**: the title text is visibly stretched horizontally — `x_scale > y_scale`.

The likely original typesetting rule: names shorter than a minimum banner width are stretched horizontally to fill it; names that already meet or exceed that width are rendered at natural proportions. Implement this as a `name_min_width` constant: if `natural_width < name_min_width`, scale x up to reach `name_min_width` while keeping y fixed; if `natural_width > name_max_width`, scale both x and y down proportionally.

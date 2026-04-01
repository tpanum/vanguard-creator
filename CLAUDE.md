# CLAUDE.md
A command-line tool for creating custom Magic: The Gathering Vanguard cards. Define cards in YAML, provide artwork, and `vgc` composites them onto an authentic card template with proper typography, mana symbols, and print-ready output.

## Testing text rendering

Whenever designing or modifying text insertion (placement, sizing, font, layout), you MUST use the text-pixel F1 score as the feedback metric. Raw pixel diff is useless here because the template background dominates the signal.

The method (implemented in `tests/render_tests.rs`):
1. Render the card and crop the lower region (y ≥ 588).
2. Scale the reference scan to 718×1024 and crop the same region.
3. Convert both to grayscale and binarize at luma < 80 (dark pixels = text).
4. Compute precision, recall, and F1 over the binary text-pixel masks.

Run with `UPDATE_FIXTURES=1 cargo test -- --nocapture` to save crops and masks to `tests/fixtures/` for visual inspection alongside the score. Always inspect the masks to confirm the metric is seeing text and not background noise.

## Title font

The confirmed correct font for Vanguard card titles is **Fremont Regular** (SoftMaker Software GmbH), available at https://fontsgeek.com/fonts/Fremont-Regular. It is embedded as `assets/fonts/Fremont-Regular.ttf`.

## Title font observations

From examining original Vanguard cards:

- **Sliver Queen, Brood Mother**: the title text appears at natural (unstretched) proportions — `x_scale == y_scale`.
- **Gerrard**: the title text is visibly stretched horizontally — `x_scale > y_scale`.

The likely original typesetting rule: names shorter than a minimum banner width are stretched horizontally to fill it; names that already meet or exceed that width are rendered at natural proportions. Implement this as a `name_min_width` constant: if `natural_width < name_min_width`, scale x up to reach `name_min_width` while keeping y fixed; if `natural_width > name_max_width`, scale both x and y down proportionally.

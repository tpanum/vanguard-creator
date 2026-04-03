# CLAUDE.md
A command-line tool for creating custom Magic: The Gathering Vanguard cards. Define cards in YAML, provide artwork, and `vgc` composites them onto an authentic card template with proper typography, mana symbols, and print-ready output.

## Asset embedding philosophy

We strive to embed everything required to produce great-looking Vanguard cards directly into the binary — fonts, mana symbols, and the card template are all bundled so that `vgc` works out of the box with zero setup. All bundled assets are packed into a single zstd-compressed tar archive (`bundled.tar.zst`, level 3) at build time via `build.rs` and included with `include_bytes!`. Assets are extracted lazily at runtime via `src/bundle.rs`.

Where customisation makes sense (e.g. swapping the card template for a fan-made variant), we expose optional CLI flags that override the embedded default — but the embedded version is always the fallback. No external files should ever be *required* for a standard render.

## Testing text rendering

Whenever designing or modifying text insertion (placement, sizing, font, layout), you MUST use the text-pixel F1 score as the feedback metric. Raw pixel diff is useless here because the template background dominates the signal.

The method (implemented in `tests/render_tests.rs`):
1. For each text region (title, rules, left bubble, right bubble), render **only that element** onto a blank 718×1024 canvas using the exact same `text::*` functions called by `render::render_card`.
2. Scale the reference mask to 718×1024.
3. Binarize both at luma < 128 (dark pixels = text).
4. Compute precision, recall, and F1 over the binary text-pixel masks.

**The test helpers MUST always call the same `text::*` functions, with the same arguments, as `render::render_card` does.** If the production render path changes, the corresponding test helper must be updated in the same commit. Tests that call different functions or use different parameters do not validate the actual output.

Run with `UPDATE_FIXTURES=1 cargo test -- --nocapture` to save rendered images and diff maps to `tests/fixtures/` for visual inspection. Always inspect the diffs to confirm the metric is seeing text and not background noise.

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

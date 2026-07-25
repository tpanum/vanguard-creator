# CLAUDE.md
A command-line tool for creating custom Magic: The Gathering Vanguard cards. Define cards in YAML, provide artwork, and `vgc` composites them onto an authentic card template with proper typography, mana symbols, and print-ready output.

## Asset embedding philosophy

We strive to embed everything required to produce great-looking Vanguard cards directly into the binary — fonts, mana symbols, and the card template are all bundled so that `vgc` works out of the box with zero setup. All bundled assets are packed into a single zstd-compressed tar archive (`bundled.tar.zst`, level 3) at build time via `build.rs` and included with `include_bytes!`. Assets are extracted lazily at runtime via `src/bundle.rs`.

Where customisation makes sense (e.g. swapping the card template for a fan-made variant), we expose optional CLI flags that override the embedded default — but the embedded version is always the fallback. No external files should ever be *required* for a standard render.

## Output provenance metadata

Every image `vgc` writes carries the version of the tool that produced it (`vgc <CARGO_PKG_VERSION>`), so a card file found later can be traced back to the exact release that rendered it. This lives in `src/meta.rs`:

- **PNG** — EXIF `Software` (0x0131) in an `eXIf` chunk, plus a `Software` `tEXt` chunk so tools without EXIF support still show it
- **JPEG** — EXIF `Software` in an `APP1` segment
- **Print PDF** — `/Producer` and `/Creator` in the document info dictionary (`print_cmd::save_as_pdf`)

**All raster output must be saved through `meta::save_with_version`, never `img.save()` directly.** A new code path that writes a card image and bypasses this helper silently produces untraceable files. Formats with no metadata container we rely on degrade to a plain save rather than erroring.

The EXIF block is hand-built (a minimal little-endian TIFF header with one IFD0 entry) and spliced into the encoded bytes — after `IHDR` for PNG, after `SOI` for JPEG. Keep the unit tests in `src/meta.rs` passing if you touch that byte layout; one of them round-trips through `image`'s own `PngDecoder::exif_metadata`, which is what catches a malformed block.

## Testing text rendering

Whenever designing or modifying text insertion (placement, sizing, font, layout), you MUST use the text-pixel F1 score as the feedback metric. Raw pixel diff is useless here because the template background dominates the signal.

The method (implemented in `tests/render_tests.rs`):
1. For each text region (title, rules, left bubble, right bubble), render **only that element** onto a blank 718×1024 canvas using the exact same `text::*` functions called by `render::render_card`.
2. Scale the reference mask to 718×1024.
3. Binarize both at luma < 128 (dark pixels = text).
4. Compute precision, recall, and F1 over the binary text-pixel masks.

**The test helpers MUST always call the same `text::*` functions, with the same arguments, as `render::render_card` does.** If the production render path changes, the corresponding test helper must be updated in the same commit. Tests that call different functions or use different parameters do not validate the actual output.

**Do NOT modify test cases (thresholds, reference fixtures, or test logic) without explicit permission from the user.** The tests encode hard-won knowledge about what correct rendering looks like. Lowering a threshold or updating a fixture to make a failing test pass hides a real regression — it does not fix it. If a test fails, investigate and fix the production code; do not adjust the test to accommodate broken output.

Run with `UPDATE_FIXTURES=1 cargo test -- --nocapture` to save rendered images and diff maps to `tests/fixtures/` for visual inspection.

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

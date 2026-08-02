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

## Releasing

A release is two steps, both on `main`, and nothing else:

1. A commit that changes only the `version` field in `Cargo.toml` (and the
   matching line `cargo build` writes into `Cargo.lock`), subject
   `Bump version to <X.Y.Z>`.
2. An annotated tag `v<X.Y.Z>` on that commit, pushed.

Pushing the tag is what triggers `.github/workflows/release.yml`, which builds
Linux musl, macOS aarch64 and Windows msvc, attaches the three archives and
generates the notes.

**Do not open a pull request for a release.** The bump carries no reviewable
content — it is a consequence of what already landed, not a change to it — so a
branch, a PR and a second CI run buy nothing. Commit it straight to `main`.

Because the version is stamped into every image `vgc` writes, a bump is what
makes a card traceable to the build that rendered it; check `vgc --version`
against `Cargo.toml` before tagging.

## Testing text rendering

Whenever designing or modifying text insertion (placement, sizing, font, layout), you MUST use the text-pixel F1 score as the feedback metric. Raw pixel diff is useless here because the template background dominates the signal.

The suite covers all 25 cards in `tests/assets/`, four elements each. The method (driven by `tests/common/mod.rs`, one test per card):
1. For each text region (title, rules, left bubble, right bubble), render **only that element** onto a blank 718×1024 canvas using the exact same `render::draw_*` functions called by `render::render_card`.
2. Segment the same region out of the card scan, in the same 718×1024 space.
3. Binarize the render at luma < 128 (dark pixels = text).
4. Compute precision, recall, and F1 over the binary text-pixel masks.

### Two scores, not one

Every case is gated on a pair:

- **overall F1** — placement and rendering together. The original single score.
- **shape F1** — the same comparison after the best translation and uniform scale have been applied, so placement is factored out and what remains is the rendering itself: typeface, weight, line breaking.

They fail for different reasons, and that is the point. Moving a text box changes overall and leaves shape alone; changing a font moves shape. A drop in overall with shape steady is a layout regression; the reverse is a rendering regression. One number confounds the two, which is how a font error can hide behind a compensating offset — the calibrator will happily buy one with the other if you let it.

Shape F1 is also the right metric for choosing a typeface. At these sizes raw overlap is dominated by how much ink lands where, which barely separates two faces of similar weight: picking the stat-bubble face on overall F1 chose MPlantin Bold, and on shape F1 chose Fremont, which is the one that matches the printed card.

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

## Two-digit stat bubbles

The bubbles are circles stamped on the template, so a two-digit modifier has to
be made to fit. The originals do it by **tightening the spacing, not by shrinking
or condensing the glyphs**: `layout.stats_multi_digit_tracking` subtracts 3.75 px
from the advance after each glyph when the value has more than one digit.
Single-digit values are set at the font's own spacing — a tracking knob applied
to every value swept flat at 0.0, so the originals' `-4` is spaced as ours is.

The distinction is invisible to a bounding box and to F1, and it is the whole
point. Condensing the glyphs horizontally reaches the same 42 px-wide box by
thinning every vertical stem, which the printed cards plainly do not do; it
scored well (right-bubble mean 62.0% overall) and looked wrong. Measuring the
scans glyph by glyph settles it: the digits in `+10`, `+12` and `+15` are the
same width as the digits in `+4` or `-8`, and only the centre-to-centre advance
between them is short. Tracking then beat condensation on the metric too —
right-bubble mean overall 58.5% → 63.2%, shape 77.7% → 80.8%, and the two-digit
cards' shape F1 (77.7–82.4%) joined the single-digit range instead of trailing
it by 10 points.

**Use `tests/bubble_geometry.rs` for anything about bubble typography**, not the
bbox in the accuracy diagnosis:

```sh
cargo test --release --test bubble_geometry -- --ignored --nocapture
```

It splits both the reference and the render into connected components — the sign
and each digit — and prints each one's width, height, ink and the
centre-to-centre step from the glyph before it. Centre-to-centre is the right
measurement because it is invariant to ink spread: the press fattens every glyph
on every side, which shrinks the measured *gap* without moving the *centres*, so
gaps read 3 px tighter on a scan even where the spacing is identical. Ours now
matches Orim's original exactly (13.5, 13.0) and Urza's within half a pixel.

Eladamri's right-bubble reference carries a speckle above the value, so its bbox
reads 30 px tall against everyone else's 21 and its overall F1 swings a few
points on sub-pixel rounding. Judge this knob on the other three, and on shape
rather than overall; the `stats_multi_digit_tracking` column in `explain_knob`
shows all four.

Three digits are refused outright at load (`card::stat_error`) rather than
rendered overflowing — there is no spacing a third digit fits in.

## Rules text that does not fit

Custom cards carry far more rules text than the originals ever did — the longest
of the 168 in `../bug-vanguards` is Ricardi Van Mouse at 366 characters, roughly
three times the median and ten lines at the fixed size. Text that long has to be
made to fit, and the way it is made to fit must not disturb the cards that fit
already.

So the overflow path is **gated on failure, not on length**: the text is set at
`ability_size` in the normal box first, and only if that block overflows does
anything else happen. Every original fits, so none of them ever reaches the code
below and the accuracy suite is unchanged to the digit. Adding a knob that
applies to all cards — a global size reduction, a tighter line height, a wider
box — is the wrong shape for this problem, however much easier it looks.

Overflowing text gets, in order:

1. **The lower parchment.** The panel does not end at `text_box.bottom` (835).
   Measured off the template it runs full width (83–635) to y≈754, narrows to the
   column between the stat-bubble housings (144–574), and continues to y≈905
   where the bottom banner cuts in. No original needs that strip, which is why
   `text_box` stops short of it and every calibrated constant is measured in that
   frame. `rules_overflow_top`/`_bottom` describe the real parchment, and only
   overflow text is allowed down there.
2. **A smaller size**, `ability_size` down to `ability_size_min`, stopping at the
   largest that fits. Ricardi lands at 21, not the 20 it would need without the
   lower strip.

### Where the block sits

`rules_block_top` places every block, overflowing or not, and it is one
expression. Earlier there were three rules — calibrated centring, a centring in
the full text box, and a centring in the parchment for overflow — and the seams
showed: a block 3 px taller could land 35 px lower, and a card just tall enough
to overflow sat 20 px below one that was not.

The measurement behind it: **every original centres its ability on y ≈ 701.** One
line spans 688–714, two 675–728, three 659–741 — same middle, growing both ways.
Every one of the 25 is 30, 60 or 90 px tall (`rules_calibrated_height`), so that
centring is calibrated over exactly that range and nothing else.

Kept up, it walks a taller block off the top: a custom card's 120 px block put
its first line on the panel's top border. So the top pins where a 90 px block
starts — y = 656 — and the block grows downward from there:

```
y = clamp(701 − h/2,  low = 656,  high = 905 − h)
```

Two things fall out of this rather than needing their own rules. Full-width lines
stay clear of the stat-bubble housings, because the wide portion is at most three
lines and 656 + 90 = 746, above the narrowing at 754. And every long card gets
the same top margin, which centring did not give.

Blocks of 90 px and under are untouched by all of it, which is why the accuracy
report is byte-identical: no original is ever tall enough for the anchor to bind.

### Text that cannot be set is refused

Shrinking stops at `ability_size_min`. Past that there is nothing left to try, so
**the card is refused and nothing is written** — `check_rules_fit` runs in
`render_card` before a pixel is drawn, `create` exits non-zero, and `validate`
reports the same thing so it can be caught earlier. Rendering it anyway would
leave a PNG with its last lines across the bottom banner, which in a batch of 168
cards is worse than no file: it sits there looking finished. This is the same
call as `card::stat_error` refusing a three-digit stat.

Two guards, and they catch different things:

- **`ability_chars_max`** (650) is a *readability* limit. Text keeps fitting well
  past the point where it is pleasant to read, because fitting only asks the type
  to get smaller; 650 sets at 17 px and was chosen by rendering a ladder of
  lengths and looking at it. The geometric ceiling is around 920, which is 14 px
  in fourteen lines filling the panel edge to edge.
- **The parchment check** counts pixels rather than characters, and is not
  redundant with the limit. Fourteen short paragraphs is 226 characters — a
  quarter of the limit — and still cannot be set, because the paragraph gaps make
  it a 505 px block against 261 px of parchment.

Size does not fall smoothly with length: 450 and 550 characters both set at 22 px,
then 650 drops to 17, because the wrap crosses out of the full-width box into the
narrow column between the stat-bubble housings. Re-render the ladder before moving
`ability_chars_max`.

Note that F1 cannot see any of this — it scores ability text on a blank canvas
against the original scans, and no original overflows. What guards it is the unit
tests in `src/text.rs` (text that fits never enters the path, the chosen size is
the largest that fits, the block stays inside the parchment, the margins balance,
over-long text is refused, and short-but-untypesettable text is refused too) and
looking at a render. Regenerate one with:

```sh
vgc create <card>.yaml -o target/overflow
```

## Widow control

Greedy wrapping fills each line to the measure and lets the remainder fall where
it lands, which on centered text leaves a full line followed by a stub: `flash.`
alone under a full line is 12% of the measure and reads as a mistake. The survey
found **80 such lines on 73 of 167 custom cards**.

`wrap_paragraph` narrows the measure in 4 px steps and takes the first setting
whose last line clears `widow_min_fraction` (0.35) — **without changing the line
count**. That invariant is what makes it safe to drop into the middle of the
pipeline: block height, the size search, the overflow decision and vertical
placement all key off the number of lines, so only the break points move. If no
narrower measure helps, the greedy wrap stands, so a paragraph can only improve
or stay as it was. `MIN_WIDOW_MEASURE` stops the hunt at 55% of the measure,
past which a paragraph reads as a column.

It took the corpus from 80 widows to 2 — the two the algorithm cannot fix without
adding a line, which it is not allowed to do.

**No original is affected at any threshold**, and this is structural rather than
lucky: all 25 carry their printed line breaks as `\n` or are a single line, so
none of them auto-wraps and `wrap_paragraph` is never on their path. That is also
why balancing is the right target — the 1997 cards were broken by hand, and Sidar
Kondo breaks after `+3/+3` with room to spare on the line.

Survey the corpus before moving the knob:

```sh
cargo run --release --example widows -- ../bug-vanguards/vanguards
```

## Original line breaks

The 1997 cards were broken by hand, not by a width rule — Sidar Kondo breaks after `+3/+3` with room to spare on the line. Where a card's true breaks are known, encode them as `\n` in its YAML; the auto-wrap is a fallback for new cards, and scoring against it measures the wrap heuristic rather than the rendering. All four reference cards carry their originals' breaks.

**The F1 score is the ground truth for rendering quality. If a change causes F1 to drop, the change made things worse — revert or iterate until F1 recovers or improves. If F1 improves, the change is an improvement. Do not override this metric with subjective impressions.**

**When a test score seems unusually low (e.g. below 10%), always generate and inspect the diff image before drawing conclusions.** The diff (black = correct, red = missed, green = extra) immediately reveals whether the problem is a positional offset, wrong font, wrong line-breaking, or a bad mask. Do not attempt to diagnose low scores from band statistics alone — look at the diff first.

## README sample images

`README.md` shows four rendered cards beside scans of the originals, from `assets/examples/`. The `*_org.*` files are the originals and never change; the four rendered ones are build output and go stale the moment anything about rendering changes.

**Regenerate them in the same commit as any change to rendering** — `src/text.rs`, `src/render.rs`, `src/layout.rs`, the bundled fonts or symbols, or a card definition the samples use. A reader compares those images against the originals to judge the tool; a stale sample misrepresents it.

```sh
cargo run --release -- create tests/cards/gerrard.yaml tests/cards/silverqueen.yaml \
    tests/cards/sidarkondo.yaml tests/cards/volrath.yaml -o assets/examples
mv -f assets/examples/sliver_queen__brood_mother.png assets/examples/silverqueen.png
mv -f assets/examples/sidar_kondo.png assets/examples/sidar.png
```

The renames are needed because output filenames come from the card name, while the README links to the shorter slugs. The samples are rendered from `tests/cards/*.yaml` on purpose, so they always show the same card data the accuracy suite scores — those four carry `flavor` text, which the suite drops but the samples need.

Look at the result before committing. The F1 metric never sees the samples: it scores ability text only, on a blank canvas, so nothing in the suite will catch flavor text colliding with the stat bubbles, artwork cropping wrongly, or a symbol rendering at the wrong tone.

## Modal abilities

A line starting with `* ` is one mode of a modal ability (`src/text.rs`). Two
decisions are load-bearing:

- **The em dash is derived, never authored.** `insert_mode_dashes` appends it to
  whatever line introduces a run of modes. That is what collapses the triggered
  and untriggered cases into one construct: `Choose one —` and `When ~ attacks,
  choose one —` differ only in what the author wrote, so there is no second
  syntax for the triggered form and no way to get the dash wrong. The pass is
  idempotent — a line already ending in `—` is left alone — because
  `wrap_text_split` re-wraps its own reconstructed output.
- **A modal block is set flush left as a unit, and the unit is centered.**
  Ordinary Vanguard rules text centers every line individually; doing that to a
  list leaves the bullets in a ragged column. `draw_lines` switches alignment
  when any line carries an indent or a bullet.

Consequences worth keeping: `—` is glued to the word before it during wrapping
(`glue_dashes`), so it can never begin a line; `wrap_text_split` backs its split
up to a mode's bullet so no mode straddles the width change; and the shared left
edge is clamped to the narrow box, or a long header would push the modes into
the stat-bubble housings. `lines_to_text` must not emit a second newline before
a mode it has already broken for — that reads back as a paragraph break and
opens a gap between modes that the list never asked for.

The bullet is a drawn disc, not `•`, so it is the same mark at the same weight
whatever face the rules text is in and a font missing the glyph cannot turn a
list into a row of tofu.

A modal list is also the kind of text that overflows, so the two features meet:
`mode_indent` and `mode_bullet_radius` are measured at `ability_size` and scale
with the type when the overflow path sets a card smaller. They are typographic
indents, not fixed margins — held constant, a list at 16 px inside a 24 px gutter
reads as two loose columns.

None of this is visible to the accuracy suite — no original is modal — so it
rests on the unit tests in `src/text.rs` and on looking at a rendered card.

## The gem

Every original carries a glossy sphere in the bottom bezel, and it is not always the same colour. Sampling the gem disc out of all 25 scans in `tests/assets/` puts the cards into four tight clusters — blue, green, red, white — so this is a real per-card property, recorded as a **required** `color:` field in each card's YAML. `src/gem.rs` recolours it.

`GemColor` is one of the five colours; `Gem` is what a card's field resolves to, either `Single` or `Dual`. Neither implements `Default`, and `CardDef::color` carries no `#[serde(default)]`. A card that omits the field is an error, not a blue card: blue is a real answer that is right for only a fifth of the set, so defaulting it would render the wrong gem silently instead of saying the card is incomplete. `parse-mse` has nothing in an `.mse-set` to derive the colour from, so it writes `blue` with a comment marking it for review — visible, not silent.

The template is a blue card's, so **blue is the identity and is a strict no-op**. Every other colour is a hue rotation, saturation multiplier and value gamma away from it.

Those constants are *fitted*, not read off the cluster means. Hue rotation and a value gamma are both nonlinear over the gem's pixel distribution, so a transform built from means-of-means lands wide of the mean it was built from — building red that way produced a visibly magenta gem whose own mean was nowhere near the target. `cargo run --release --example fit_gem` iterates each triple until the recoloured gem body's mean equals the cluster mean, divides the result through by the fit for blue so the scanner's colour cast drops out, and writes `target/gem_fit.png` — every rendered gem above the original it was fitted to, magnified. **Look at that sheet before accepting new constants**; the fit converging says only that the means agree, not that the gem looks right.

The recolour is per pixel and deliberately narrow: only pixels inside the gem disc, saturated enough not to be frame metal, and in the blue hue band are touched, and hue is *rotated* rather than assigned. That keeps the sphere's internal hue variation and leaves the warm light bouncing up off the bezel — the same warm colour whatever the gem is — alone.

The clusters do **not** follow Magic colour identity — Serra's gem is green, Volrath's is white, Sidar Kondo's is red. Do not "correct" a card's `color:` to match its colour identity; the values in `tests/cards/*.yaml` are what the scans show.

Black has no original in the suite. Its constants are a judgement call sitting alongside the measured four; only its gamma is solved, for a chosen body value of V ≈ 0.28.

### Two-colour gems

`color:` also takes a pair — `wu`, `w/u`, `white/blue` — which grades the two colours into each other from left to right. No original has one; like black, this is an invention for custom cards, in the spirit of hybrid mana. Order is meaningful: `wu` and `uw` are mirror images.

**Interpolate the two results, never the two transforms.** A transform is a hue *rotation*, so averaging white's −173.5° with red's +147.2° passes through 0° — which is blue, a colour neither half of the gem is. `recolor_blended` applies both transforms to each pixel and lerps the two RGB results, which cannot produce a colour that is not on the line between them.

`BLEND_SPAN` (0.7 of the radius) sets how much of the sphere the gradient occupies. It must stay below 1.0 or neither colour ever reaches full strength and every pair reads as one muddy stone; the pairs to check it against are the ones close in value, `white/blue` and `black/green`. `cargo run --release --example gem_swatches` writes `target/gem_swatches.png` with all five singles and all ten pairs.

A pair of the same colour twice must render identically to that colour alone, and a pair containing blue must *not* be a no-op even though blue alone is — both are unit tests.

### What checks this

The accuracy suite does not see any of it: it scores text on a blank canvas with no template. The gem's correctness rests on the unit tests in `src/gem.rs` — blue is byte-identical, nothing outside the disc changes, each colour lands in the right part of colour space, each half of a dual gem shows its own colour and mirroring the pair mirrors the render — and on looking at `target/gem_fit.png` and `target/gem_swatches.png`.

## Scryfall API

Vanguard metadata — `hand_modifier`, `life_modifier`, `flavor_text` — can be looked up card by card at `https://api.scryfall.com`. It is an authoring aid only; `vgc` makes no network calls.

**Do NOT use `oracle_text` for the rules text.** Scryfall only stores modernized oracle text (e.g. "from the battlefield" instead of the original "from play"), which differs from what is printed on the physical cards and shown in the reference masks. Rules text must be sourced from card scans or other references to the original printed wording.

See **[docs/scryfall.md](docs/scryfall.md)** for the queries, the fields worth trusting, and one-liners for pulling a card's image or opening it in a browser.

## The rules of Magic

The Comprehensive Rules are at <https://magic.wizards.com/en/rules>, published as
DOCX, PDF and TXT. **Consult the TXT version** — it is one flat file, so a
question is a `grep` away, where the PDF is not searchable from the shell.

The download URLs carry the release date (`.../MagicCompRules 20260619.txt`) and
change with every rules update, so start from the landing page rather than
hardcoding one:

```sh
curl -sSL -A vgc "https://media.wizards.com/2026/downloads/MagicCompRules%2020260619.txt" -o /tmp/cr.txt
grep -n -i vanguard /tmp/cr.txt
```

What is relevant here lives in rule **902** (Vanguard), rule **313** (the card
type), and rules **211.1** and **212.1**, which define the hand and life
modifiers this tool draws in the bubbles — a signed number or a zero, printed in
the lower left and lower right corners. Reach for it when a card's wording or a
stat value looks wrong; it says nothing about typography or layout.

It is also not a style guide. It defines what a phrase *means* — CR 115.4 says
what `any target` covers — but never says to write that instead of `creature or
player`. Templating lives in the Magic Style Guide, which Wizards does not
publish, so for card wording use **[COMMON_RULE_CHECKLIST.md](COMMON_RULE_CHECKLIST.md)**:
the mistakes that actually show up in custom cards, with the fix for each.

## Which face goes where

- **Card name** — Fremont Regular.
- **Rules and flavor text** — MPlantin Bold. Confirmed against the scans; every alternative tried scored well below it.
- **Hand and life modifiers** — Fremont Regular, *not* the body face. The printed numerals have the title face's tapered, slightly waved minus sign and its high stroke contrast, where MPlantin Bold has a flat rectangular bar and an even stroke. `Fonts::stats` exists to keep this explicit.

`tests/font_search.rs` re-measures all of this; `search_bubble_font` ranks on shape F1.

## Title font

The confirmed correct font for Vanguard card titles is **Fremont Regular** (SoftMaker Software GmbH), available at https://fontsgeek.com/fonts/Fremont-Regular. It is embedded as `assets/fonts/Fremont-Regular.ttf`.

## Title font observations

From examining original Vanguard cards:

- **Sliver Queen, Brood Mother**: the title text appears at natural (unstretched) proportions — `x_scale == y_scale`.
- **Gerrard**: the title text is visibly stretched horizontally — `x_scale > y_scale`.

The likely original typesetting rule: names shorter than a minimum banner width are stretched horizontally to fill it; names that already meet or exceed that width are rendered at natural proportions. Implement this as a `name_min_width` constant: if `natural_width < name_min_width`, scale x up to reach `name_min_width` while keeping y fixed; if `natural_width > name_max_width`, scale both x and y down proportionally.

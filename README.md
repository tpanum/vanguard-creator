# vgc — Vanguard Creator

A command-line tool for creating custom Magic: The Gathering Vanguard cards. Define cards in YAML, provide artwork, and `vgc` composites them onto an authentic card template with proper typography, mana symbols, and print-ready output.

## Installation

```
vgc install
```

Downloads required assets (mana symbol icons) on first run. Fonts and the card template must be provided by the user (see [Assets](#assets)).

## Quick Start

```
# Render a single card
vgc render goblin-king.yaml -o goblin-king.png

# Render all cards in a directory
vgc render vanguards/ -o outputs/

# Import cards from a Magic Set Editor file
vgc import deck.mse-set -o vanguards/

# Arrange rendered cards into a printable PDF
vgc print outputs/*.png -o print.pdf
```

## Commands

### `vgc render`

Render one or more card definitions into card images.

```
vgc render <path>... [flags]
```

`<path>` can be a YAML file or a directory (all `.yaml` files inside will be rendered).

| Flag | Description |
|---|---|
| `-o, --output <path>` | Output file or directory. Defaults to `<card-name>.png` per card. |
| `-t, --template <file>` | Card template image. Defaults to `template.png` in the config directory. |
| `--symbols <dir>` | Directory containing mana symbol PNGs. Defaults to `symbols/` in the config directory. |
| `--dpi <number>` | Output resolution scaling factor. Default: `1` (native template size). |

### `vgc import`

Extract cards and artwork from a Magic Set Editor (`.mse-set`) file into individual YAML card definitions.

```
vgc import <file.mse-set> [flags]
```

| Flag | Description |
|---|---|
| `-o, --output <dir>` | Output directory for YAML files and artwork. Defaults to current directory. |
| `--artwork-dir <name>` | Subdirectory name for extracted artwork. Default: `artwork`. |
| `--overwrite` | Overwrite existing files. Default: skip if YAML already exists. |

### `vgc print`

Arrange card images into a multi-page, print-ready PDF.

```
vgc print <images>... [flags]
```

Accepts card images via arguments or via stdin (one path per line).

| Flag | Description |
|---|---|
| `-o, --output <file>` | Output PDF path. Default: `print.pdf`. |
| `--page-size <size>` | Page format: `a4` or `letter`. Default: `a4`. |
| `--grid <cols>x<rows>` | Cards per page. Default: `3x3`. |
| `--margin <mm>` | Page margin in millimeters. Default: `10`. |
| `--cut-lines` | Draw cut lines between cards. |
| `--stdin` | Read image paths from stdin instead of arguments. |

### `vgc symbols`

Download and cache mana symbol assets.

```
vgc symbols [flags]
```

| Flag | Description |
|---|---|
| `--size <px>` | Rasterized symbol size. Default: `64`. |
| `--source <url>` | Base URL for symbol SVGs. Default: Scryfall CDN. |
| `--force` | Re-download even if symbols already exist. |

### `vgc validate`

Check card definitions for errors without rendering.

```
vgc validate <path>...
```

Reports missing fields, unresolvable artwork paths, and unknown mana symbols.

## Card Definition Format

Each card is a single YAML file:

```yaml
name: "Goblin King"
ability: |-
  Other Goblin creatures get +1/+1.
  {R}: Target Goblin gains haste until end of turn.
hand: "-1"
life: "+3"
artwork: "artwork/goblin-king.png"
```

| Field | Required | Description |
|---|---|---|
| `name` | yes | Card name displayed in the title banner. |
| `ability` | yes | Rules text. Supports `{X}` mana notation and paragraph breaks via newlines. |
| `hand` | yes | Starting hand size modifier (e.g. `+1`, `-2`, `+0`). |
| `life` | yes | Starting life modifier. |
| `artwork` | yes | Path to artwork image, resolved relative to the YAML file. |

### Mana Symbols

Use `{X}` notation in ability text. Supported symbols:

- Colored mana: `{W}` `{U}` `{B}` `{R}` `{G}`
- Generic mana: `{1}` `{2}` `{3}` `{4}`
- Special: `{X}` `{T}` (tap)

Symbols are rendered inline at the correct size and baseline.

### Paragraph Breaks

Separate distinct abilities with a newline in the YAML. Use `|-` (literal block scalar) for multi-paragraph text:

```yaml
ability: |-
  First ability text.
  {2}{G}: Second ability text.
```

This produces a visible gap between the two ability blocks on the rendered card.

## Assets

`vgc` requires the following assets in its config directory (default: `~/.config/vgc/`, overridable per-flag or via `VGC_CONFIG_DIR`):

| Asset | Purpose |
|---|---|
| `template.png` | The base Vanguard card frame with transparent artwork region. |
| `fonts/Name.ttf` | Title font (e.g. Goudy Medieval). |
| `fonts/Body.ttf` | Rules text font (e.g. MPlantin). |
| `fonts/Body-Bold.ttf` | Bold rules text font. |
| `fonts/Stats.ttf` | Stat number font. |
| `symbols/*.png` | Rasterized mana symbols (auto-downloaded via `vgc symbols`). |

## Gallery

Side-by-side comparisons of `vgc`-rendered cards (left) versus original Wizards prints (right).

### Gerrard
| Rendered | Original |
|:---:|:---:|
| ![Gerrard rendered](assets/examples/gerrard.png) | ![Gerrard original](assets/examples/gerrard_org.png) |

### Sliver Queen, Brood Mother
| Rendered | Original |
|:---:|:---:|
| ![Sliver Queen rendered](assets/examples/sliver.png) | ![Sliver Queen original](assets/examples/sliver_org.png) |

### Volrath
| Rendered | Original |
|:---:|:---:|
| ![Volrath rendered](assets/examples/volrath.png) | ![Volrath original](assets/examples/volrath_org.png) |

## Examples

```
# Import an MSE deck, render all cards, and produce a print PDF
vgc import deck.mse-set -o cards/
vgc render cards/ -o renders/
vgc print renders/*.png -o deck-print.pdf --cut-lines

# Re-render a single card after editing its YAML
vgc render cards/goblin-king.yaml -o renders/goblin-king.png

# Validate all cards before rendering
vgc validate cards/

# Pipe changed files into a print run
git diff --name-only HEAD~1 -- '*.png' | vgc print --stdin -o updated.pdf
```

## License

MIT

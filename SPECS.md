# vgc — Design Specification

This document defines the behavior, data formats, and rendering rules for `vgc`. It is intended as a complete reference for implementing the CLI from scratch.

---

## 1. CLI Structure

`vgc` is a single binary with subcommands. Every subcommand writes to stdout on success and stderr on errors. Exit code `0` on success, `1` on user error, `2` on system/IO error.

### 1.1 Global Flags

| Flag | Description |
|---|---|
| `--quiet` | Suppress progress output. Only errors are printed. |
| `--verbose` | Print detailed progress per card. |

### 1.2 Subcommands

| Command | Purpose |
|---|---|
| `create` | Produce card images from YAML definitions. |
| `parse-mse` | Parse an MSE set file into YAML + artwork. |
| `print` | Lay out card images into a paginated PDF. |
| `validate` | Check card definitions without rendering. |

---

## 2. Card Definition Schema

Cards are defined in YAML. One file per card.

### 2.1 Required Fields

```yaml
name: "Card Name"        # string, non-empty
ability: "Rules text."   # string, may contain {X} symbol tokens and newlines
hand: "+1"               # string, must match /^[+-]\d+$/
life: "-2"               # string, must match /^[+-]\d+$/
artwork: "path/to.png"   # string, file path to artwork image
```

### 2.2 Path Resolution

The `artwork` path is resolved relative to the YAML file's parent directory, not the working directory. Absolute paths are used as-is.

### 2.3 Ability Text Conventions

- **Mana symbols**: Encoded as `{X}` where `X` is one of: `W`, `U`, `B`, `R`, `G`, `1`, `2`, `3`, `4`, `X`, `T`. Unknown symbols are rendered as literal text (e.g. `{Z}` renders as the string `{Z}`).
- **Paragraph breaks**: Each `\n` in the parsed ability string starts a new paragraph with a visual gap. YAML block scalars (`|-`) preserve these newlines. YAML flow scalars with folded blank lines may collapse them; the implementation must handle both `\n\n` and single `\n` as paragraph separators.
- **No other markup is supported.** Bold, italic, and reminder text are not part of the card definition format.

---

## 3. Rendering Pipeline

### 3.1 Layer Composition Order

The final card image is built in this exact order:

1. **Canvas**: Transparent RGBA image matching the template dimensions.
2. **Artwork layer**: Artwork is placed behind the template, within the art box region.
3. **Template overlay**: The template PNG (with transparency for the art window) is alpha-composited on top of the artwork layer.
4. **Text layers**: Name, ability, and stat modifiers are drawn on top of the composited result.

### 3.2 Template Layout

The template defines named regions by pixel coordinates. A conforming implementation must support configuring these either by measuring a template image or via a layout descriptor. The reference template (718x1024) uses:

| Region | Coordinates | Purpose |
|---|---|---|
| Art box | Bounding rectangle | Transparent region where artwork shows through. |
| Name banner | Center point | Horizontal center for the card name. |
| Text box | Bounding rectangle | Interior area for ability text. |
| Hand stat | Center point | Center of the hand modifier circle. |
| Life stat | Center point | Center of the life modifier circle. |

### 3.3 Artwork Fitting

Artwork is scaled to **cover** the art box (not fit/contain):

1. Compute scale factor so the artwork fills both dimensions of the art box.
2. Resize artwork using high-quality resampling (Lanczos or equivalent).
3. Center-crop to the exact art box dimensions.
4. Paste at the art box origin.

If the artwork is smaller than the art box in both dimensions, it is still scaled up to cover.

### 3.4 Name Rendering

- Font: Title/medieval font.
- Size: Fixed (does not auto-scale).
- Position: Horizontally and vertically centered on the name banner center point.
- Color: Black.

### 3.5 Ability Text Rendering

This is the most complex rendering step. It involves text auto-scaling, word wrapping with inline symbol images, and consistent baseline alignment.

#### 3.5.1 Text Tokenization

The ability string is parsed into a flat list of typed tokens:

- `text` tokens: Literal string segments.
- `symbol` tokens: Mana symbol references (the content between `{` and `}`).

Tokenization splits on the `{...}` pattern. Adjacent symbols with no intervening text (e.g. `{2}{G}`) produce consecutive symbol tokens with no space between them.

#### 3.5.2 Paragraph Splitting

Before word-wrapping, the ability text is split into paragraphs on `\n` boundaries. Empty paragraphs (blank lines) are discarded. Each non-empty paragraph is wrapped independently, and a fixed-size vertical gap is inserted between paragraph blocks.

#### 3.5.3 Word Wrapping

Each paragraph is wrapped word-by-word to fit within the text box width (minus horizontal padding):

1. Split the paragraph on whitespace into "words."
2. Tokenize each word into text/symbol tokens.
3. Measure each word's pixel width: text tokens use font metrics, symbol tokens use the symbol display size.
4. Accumulate words onto the current line. When adding the next word (plus a space) would exceed the maximum width, start a new line.
5. Spaces between words are represented as `("text", " ")` tokens.

#### 3.5.4 Font Auto-Scaling

The font size for ability text is chosen automatically to maximize readability while fitting the text box:

1. Starting from the maximum font size, try each integer size down to the minimum.
2. For each candidate size, compute the symbol display size (proportional to font size, e.g. 110%), wrap the text, and measure total block height.
3. Total height = sum of `line_height` for each text line + sum of `paragraph_gap` for each paragraph break.
4. The `line_height` is proportional to font size (e.g. 125%).
5. The `paragraph_gap` is a fixed pixel value, not proportional to font size. This prevents the auto-scaler from defeating the visual gap by choosing a smaller font.
6. Accept the first (largest) size where total height fits within the text box height.
7. If no size fits, use the minimum size and allow overflow.

#### 3.5.5 Horizontal Centering

Each line of tokens is horizontally centered within the text box. The total pixel width of all tokens on a line is measured, and the line's starting x-position is offset so the line is centered.

#### 3.5.6 Vertical Centering

The entire text block (all lines + paragraph gaps) is vertically centered within the text box. The block's starting y-position is: `text_box_top + (text_box_height - total_block_height) / 2`.

#### 3.5.7 Baseline Alignment

All text tokens on a line share a single baseline, derived from the font's ascent/descent metrics — not from per-segment bounding boxes. This prevents short glyphs (e.g. "a", "o") from floating above glyphs with descenders (e.g. "g", "y").

The baseline position within a line: `baseline_y = line_top + (line_height - (ascent + descent)) / 2 + ascent`.

Text is drawn using a **baseline anchor** (not a top-left anchor).

#### 3.5.8 Symbol Rendering

Mana symbols are rasterized PNGs pasted inline:

- Symbol display size scales with font size (e.g. 110% of the font point size).
- Symbols are vertically centered relative to the text's visual center (midpoint between ascent line and descent line), not relative to the line height.
- Symbols are alpha-composited (not opaque-pasted) to preserve transparency.
- If a symbol PNG is missing for a token, render the token as literal text (e.g. `{Z}` falls back to drawing the string `{Z}`).

#### 3.5.9 Text Advance

After drawing each token, the x-cursor advances by:

- For text tokens: the font's computed advance width for that string.
- For symbol tokens: the symbol display size.

Use the font's advance width (not bounding box width) for text measurement to ensure correct spacing across segments.

### 3.6 Stat Modifier Rendering

- Font: Body/text font (not bold).
- Size: Fixed.
- Position: Centered on the hand/life stat center points.
- Color: Black.

---

## 4. MSE Import

Magic Set Editor `.mse-set` files are ZIP archives containing a `set` file (custom text format) and image files.

### 4.1 Set File Parsing

The `set` file uses an indentation-based format:

- `card:` at column 0 starts a new card block.
- Single-tab-indented lines are key-value pairs: `\t<key>: <value>` or `\t<key>:` (empty value).
- Double-tab-indented lines are continuation lines appended to the current key's value with a newline separator.
- Lines before the first `card:` block are set-level metadata (ignored).

### 4.2 Markup Conversion

MSE uses XML-like inline markup that must be converted:

| MSE Markup | Output |
|---|---|
| `<sym-auto>2G</sym-auto>` | `{2}{G}` |
| `<sym>T</sym>` | `{T}` |
| `<i-flavor>...</i-flavor>` | (removed entirely) |
| All other tags | Stripped, content preserved. |

Mana symbol expansion: iterate the string character by character. Consecutive digits form a single numeric symbol (e.g. `12` becomes `{12}`). Each letter becomes its own symbol.

### 4.3 Artwork Extraction

Each card's `image` field references a filename inside the ZIP archive. Extract it as a PNG to the artwork subdirectory. The artwork filename is derived from the card name with unsafe filesystem characters removed.

### 4.4 Output

One YAML file per card, using the sanitized card name as the filename. Fields written in schema order: `name`, `ability`, `hand`, `life`, `artwork`. The `artwork` path is relative to the YAML file.

Cards with no `name` field are skipped silently.

---

## 5. Print Layout

### 5.1 Page Geometry

- Page size is specified in a standard paper format (A4: 210x297mm, Letter: 216x279mm).
- Internal calculations use 300 DPI: multiply millimeters by `300 / 25.4` to get pixels.
- Cards are arranged in a grid (default 3 columns x 3 rows = 9 per page).
- A uniform margin surrounds the grid on all four sides.
- A uniform inner gap separates adjacent cards.

### 5.2 Card Scaling

All card images are scaled to a uniform size derived from the first image's aspect ratio:

1. Compute the available grid cell size from page size, margins, grid dimensions, and inner gap.
2. Scale each card image to fill that cell, preserving aspect ratio (fit, not cover — no cropping).

### 5.3 Pagination

Cards are placed left-to-right, top-to-bottom. When a page's grid is full, a new page begins. The last page may be partially filled.

### 5.4 Cut Lines

When enabled, thin lines are drawn between card cells extending to the page margins, as guides for cutting.

### 5.5 Output

A single multi-page PDF at 300 DPI. Pages are RGB (no alpha).

### 5.6 Input Sources

Card images can be provided as:
- Positional arguments (file paths or globs).
- Stdin (one path per line, when `--stdin` is set).

---

## 6. Symbol Management

### 6.1 Required Symbols

The minimum set: `W`, `U`, `B`, `R`, `G`, `1`, `2`, `3`, `4`, `X`, `T`.

Mana symbol PNGs are bundled within the binary and loaded at runtime without requiring any external files or network access.

---

## 7. Validation

`vgc validate` checks card definitions without producing images. It reports:

- Missing required fields (`name`, `ability`, `hand`, `life`, `artwork`).
- Malformed `hand`/`life` values (not matching `+N` / `-N`).
- Artwork file not found at the resolved path.
- Unknown mana symbols (symbol tokens with no corresponding bundled PNG).

Output: one line per issue, prefixed with the file path. Exit code `0` if all cards are valid, `1` if any issues are found.

---

## 8. Error Handling

- **Missing template**: Only relevant when `--template` is passed. Fatal error with the given path if the file is not found. The embedded template is always available as the default.
- **Missing fonts**: Fatal error (fonts are bundled; this would indicate a corrupt binary).
- **Missing artwork**: Warning per card. The card is still rendered with the art box left empty (template background shows through).
- **Missing symbol PNG**: Warning per occurrence. The symbol token falls back to literal text rendering.
- **Malformed YAML**: Fatal error per file with the parse error message.
- **Unwritable output path**: Fatal error.
- **MSE parse failures**: Warning per card. Other cards in the set are still processed.

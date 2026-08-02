use ab_glyph::{point, Font, FontRef, Glyph, GlyphId, PxScale, ScaleFont};
use image::{Rgba, RgbaImage};
use regex::Regex;
use std::sync::OnceLock;

use crate::layout::Layout;
use crate::symbols;

// ── Tokenization ─────────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub enum Token {
    Text(String),
    /// Mana symbol name (the content inside {…})
    Symbol(String),
}

impl Token {
    fn to_text_repr(&self) -> String {
        match self {
            Token::Text(s) => s.clone(),
            Token::Symbol(name) => format!("{{{name}}}"),
        }
    }
}

fn symbol_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"\{([^}]+)\}").unwrap())
}

/// Split a string into Text and Symbol tokens.
pub fn tokenize(text: &str) -> Vec<Token> {
    let re = symbol_re();
    let mut tokens = Vec::new();
    let mut last = 0;
    for m in re.find_iter(text) {
        if m.start() > last {
            tokens.push(Token::Text(text[last..m.start()].to_string()));
        }
        let name = &text[m.start() + 1..m.end() - 1];
        tokens.push(Token::Symbol(name.to_string()));
        last = m.end();
    }
    if last < text.len() {
        tokens.push(Token::Text(text[last..].to_string()));
    }
    tokens
}

// ── Type specification ───────────────────────────────────────────────────────

/// Everything needed to set a run of text at one size: font scale, line
/// height, and the square pixel size of inline mana symbols.
#[derive(Debug, Clone, Copy)]
pub struct TypeSpec {
    pub scale: PxScale,
    pub line_height: f32,
    pub symbol_size: u32,
}

impl TypeSpec {
    /// Ability-text spec.
    fn ability(size: u32, layout: &Layout) -> Self {
        TypeSpec {
            scale: PxScale::from(size as f32),
            line_height: size as f32 * layout.line_height_factor,
            symbol_size: (size as f32 * layout.symbol_scale).round().max(1.0) as u32,
        }
    }

    /// Flavor-text spec: symbols render slightly larger relative to the text.
    fn flavor(size: u32, layout: &Layout) -> Self {
        TypeSpec {
            scale: PxScale::from(size as f32),
            line_height: size as f32 * layout.line_height_factor,
            symbol_size: (size as f32 * layout.symbol_scale * 1.1).round().max(1.0) as u32,
        }
    }

    /// Distance from the top of a line box to the text baseline, centering
    /// the font's ascent+descent within `line_height`.
    fn baseline_from_top(&self, font: &FontRef) -> f32 {
        let scaled = font.as_scaled(self.scale);
        let ascent = scaled.ascent();
        let descent = -scaled.descent();
        (self.line_height - (ascent + descent)) / 2.0 + ascent
    }
}

// ── Font metrics helpers ──────────────────────────────────────────────────────

/// Horizontal advance width of a string using the given scaled font.
/// Includes inter-glyph kerning.
pub fn measure_str(text: &str, font: &FontRef, scale: PxScale) -> f32 {
    let scaled = font.as_scaled(scale);
    let mut advance = 0.0f32;
    let mut prev: Option<GlyphId> = None;
    for c in text.chars() {
        let gid = scaled.glyph_id(c);
        if let Some(p) = prev {
            advance += scaled.kern(p, gid);
        }
        advance += scaled.h_advance(gid);
        prev = Some(gid);
    }
    advance
}

/// Pixel width of a single token.
fn measure_token(token: &Token, font: &FontRef, scale: PxScale, symbol_size: u32) -> f32 {
    match token {
        Token::Text(s) => measure_str(s, font, scale),
        Token::Symbol(name) => {
            if symbols::is_known(name) {
                symbol_size as f32
            } else {
                // Unknown symbol: render as literal text "{name}"
                measure_str(&format!("{{{name}}}"), font, scale)
            }
        }
    }
}

/// Total pixel width of a slice of tokens.
pub fn measure_tokens(tokens: &[Token], font: &FontRef, scale: PxScale, symbol_size: u32) -> f32 {
    tokens
        .iter()
        .map(|t| measure_token(t, font, scale, symbol_size))
        .sum()
}

// ── Word-wrapping ─────────────────────────────────────────────────────────────

/// One laid-out line of rules text.
#[derive(Debug, Clone)]
pub struct Line {
    pub tokens: Vec<Token>,
    /// Left inset from the block's left edge, in px. Non-zero only for the
    /// lines of a mode, so that a mode which wraps hangs under its own text
    /// rather than under its bullet.
    pub indent: f32,
    /// Whether a bullet is stamped in the gutter to the left of this line.
    /// True on the first line of each mode only.
    pub bullet: bool,
}

#[derive(Debug, Clone)]
pub enum WrappedLine {
    Tokens(Line),
    /// Extra inter-paragraph gap (triggered by `\n\n` in input).
    ParagraphBreak,
    /// Forced line break with normal line spacing (triggered by single `\n`).
    HardBreak,
}

/// Split a paragraph into wrappable words, keeping an em dash attached to the
/// word before it.
///
/// A dash that introduces a mode list must never begin a line: on its own it
/// says nothing, and the clause it belongs to has already ended above it. It is
/// the same rule the printed cards follow, and the reason `Choose one —` is one
/// unit to the wrapper even though it is two words to `split_whitespace`.
fn glue_dashes(para: &str) -> Vec<String> {
    let mut words: Vec<String> = Vec::new();
    for word in para.split_whitespace() {
        match words.last_mut() {
            Some(prev) if word == MODE_DASH.to_string() => {
                prev.push(' ');
                prev.push(MODE_DASH);
            }
            _ => words.push(word.to_string()),
        }
    }
    words
}

/// Glue a quoted ability into one unbreakable word when it fits the measure.
///
/// A card that grants an ability quotes it — `Bird creatures you control have
/// “{T}: Draw a card.”` — and the quote is a unit: broken across lines it reads
/// as two fragments, one of them starting mid-clause. Wrapping is greedy and
/// knows nothing about that, so it happily ends a line on `have “{T}:`.
///
/// A quote wider than the measure cannot be kept whole and is left breakable —
/// the long granted abilities on Illobug, Vlademir and Yodog are all in that
/// class. Everything narrower becomes a single word, which the wrapper then
/// cannot split.
///
/// Both curly and straight quotes are recognised; straight ones are paired in
/// order, since `"` gives no clue which end it is.
fn glue_quotes(
    words: Vec<String>,
    font: &FontRef,
    scale: PxScale,
    symbol_size: u32,
    max_width: f32,
) -> Vec<String> {
    let opens = |w: &str| w.starts_with('\u{201c}') || w.starts_with('"');
    let closes = |w: &str| {
        let t = w.trim_end_matches(|c: char| c.is_ascii_punctuation() && c != '"');
        t.ends_with('\u{201d}') || t.ends_with('"')
    };

    let mut out: Vec<String> = Vec::new();
    let mut i = 0;
    while i < words.len() {
        if opens(&words[i]) && !closes(&words[i]) {
            // Find the word carrying the closing quote.
            if let Some(end) = (i + 1..words.len()).find(|&j| closes(&words[j])) {
                let joined = words[i..=end].join(" ");
                let width = measure_tokens(&tokenize(&joined), font, scale, symbol_size);
                if width <= max_width {
                    out.push(joined);
                    i = end + 1;
                    continue;
                }
            }
        }
        out.push(words[i].clone());
        i += 1;
    }
    out
}

fn wrap_greedy(
    para: &str,
    font: &FontRef,
    scale: PxScale,
    max_width: f32,
    symbol_size: u32,
) -> Vec<Vec<Token>> {
    let space_w = measure_str(" ", font, scale);

    let mut lines: Vec<Vec<Token>> = Vec::new();
    let mut current: Vec<Token> = Vec::new();
    let mut current_w = 0.0f32;

    let words = glue_quotes(glue_dashes(para), font, scale, symbol_size, max_width);
    for word in words {
        let word = word.as_str();
        let word_tokens = tokenize(word);
        let word_w: f32 = word_tokens
            .iter()
            .map(|t| measure_token(t, font, scale, symbol_size))
            .sum();

        let gap = if current.is_empty() { 0.0 } else { space_w };

        if !current.is_empty() && current_w + gap + word_w > max_width {
            lines.push(current);
            current = word_tokens;
            current_w = word_w;
        } else {
            if !current.is_empty() {
                current.push(Token::Text(" ".to_string()));
                current_w += space_w;
            }
            current.extend(word_tokens);
            current_w += word_w;
        }
    }

    if !current.is_empty() {
        lines.push(current);
    }
    lines
}

/// Wrap one paragraph, pulling words down off the penultimate line when the
/// greedy wrap would leave a stub on the last one.
///
/// Greedy wrapping fills each line to the measure and lets the remainder fall
/// where it may, which on a centered card leaves a full line followed by
/// `flash.` — 12% of the measure, and it reads as a mistake. The 1997 cards were
/// broken by hand and do not do this: Sidar Kondo breaks after `+3/+3` with room
/// to spare on the line.
///
/// The fix narrows the measure and re-wraps, keeping the widest setting whose
/// last line clears `widow_min_fraction`. **The line count is never allowed to
/// change**, which is what makes this safe to drop into the middle of the
/// pipeline: block height, the size search, the overflow decision and vertical
/// placement all key off the number of lines, so only the break points move. If
/// no narrower measure helps, the greedy wrap stands — this can only improve a
/// paragraph or leave it alone.
///
/// No original reaches this code: all 25 carry their printed breaks as `\n` or
/// are a single line, so none of them auto-wraps at all.
fn wrap_paragraph(
    para: &str,
    font: &FontRef,
    scale: PxScale,
    max_width: f32,
    symbol_size: u32,
    layout: &Layout,
) -> Vec<Vec<Token>> {
    let lines = wrap_greedy(para, font, scale, max_width, symbol_size);

    let last_width = |lines: &[Vec<Token>]| {
        lines
            .last()
            .map(|l| measure_tokens(l, font, scale, symbol_size))
            .unwrap_or(0.0)
    };

    let target = max_width * layout.widow_min_fraction;
    if lines.len() < 2 || last_width(&lines) >= target {
        return lines;
    }

    // Narrow the measure a step at a time and keep the first setting that fixes
    // the stub without costing a line.
    let mut width = max_width;
    while width > max_width * MIN_WIDOW_MEASURE {
        width -= WIDOW_STEP;
        let candidate = wrap_greedy(para, font, scale, width, symbol_size);
        if candidate.len() != lines.len() {
            break;
        }
        if last_width(&candidate) >= target {
            return candidate;
        }
    }

    lines
}

/// Step by which the measure is narrowed while hunting for a wrap without a
/// stub on the last line.
const WIDOW_STEP: f32 = 4.0;

/// Floor on that hunt, as a fraction of the measure: past this a paragraph is
/// so narrow it reads as a column, which is worse than the stub it fixes.
const MIN_WIDOW_MEASURE: f32 = 0.55;

// ── Modal abilities ───────────────────────────────────────────────────────────

/// Line prefix that marks one mode of a modal ability.
///
/// A line beginning with `* ` is a mode: it is set as a bulleted, hanging-
/// indented item. `\*` escapes it back to a literal asterisk.
const MODE_MARKER: &str = "*";

/// The dash the printed cards put after the mode-choosing clause.
const MODE_DASH: char = '—';

/// The mode indent at a given ability-text size.
///
/// Overflowing text is set smaller than `ability_size`, and the gutter has to
/// come down with it: it is a typographic indent, roughly proportional to the
/// type, not a fixed margin. Held constant, a list at 16 px inside a 24 px
/// gutter reads as two loose columns.
fn mode_indent_at(size: u32, layout: &Layout) -> f32 {
    layout.mode_indent * size as f32 / layout.ability_size as f32
}

/// Is this source line one mode of a modal ability?
fn is_mode(line: &str) -> bool {
    let t = line.trim_start();
    t == MODE_MARKER || t.starts_with("* ")
}

/// The text of a mode, with its marker (or the `\` that escapes one) removed.
fn strip_mode_marker(line: &str) -> String {
    let t = line.trim_start();
    match t.strip_prefix(MODE_MARKER) {
        Some(rest) => rest.trim_start().to_string(),
        None => t.strip_prefix('\\').unwrap_or(t).to_string(),
    }
}

/// Append the em dash to any line that introduces a run of modes.
///
/// This is what makes the triggered and untriggered cases one construct rather
/// than two: the difference between `Choose one —` and `When ~ attacks, choose
/// one —` is only what the author wrote on the introducing line, so the dash is
/// derived from the presence of modes below it and never spelled out. A line
/// that already ends in one is left alone, which is what keeps the pass
/// idempotent — `wrap_text_split` re-wraps its own output.
fn insert_mode_dashes(text: &str) -> String {
    let lines: Vec<&str> = text.split('\n').collect();
    let mut out: Vec<String> = Vec::with_capacity(lines.len());

    for (i, line) in lines.iter().enumerate() {
        let next_is_mode = lines[i + 1..]
            .iter()
            .find(|l| !l.trim().is_empty())
            .is_some_and(|l| is_mode(l));

        let trimmed = line.trim_end();
        if next_is_mode && !trimmed.is_empty() && !is_mode(line) && !trimmed.ends_with(MODE_DASH) {
            out.push(format!("{trimmed} {MODE_DASH}"));
        } else {
            out.push((*line).to_string());
        }
    }

    out.join("\n")
}

/// Word-wrap ability text into lines.
///
/// `\n\n` separates paragraphs (inserts a `ParagraphBreak` with extra spacing).
/// Single `\n` is a hard line break (inserts a `HardBreak`, normal line spacing).
/// A line starting with `* ` is one mode of a modal ability — see
/// [`insert_mode_dashes`].
pub fn wrap_text(
    text: &str,
    font: &FontRef,
    scale: PxScale,
    max_width: f32,
    symbol_size: u32,
    layout: &Layout,
) -> Vec<WrappedLine> {
    wrap_text_indented(text, font, scale, max_width, 0.0, symbol_size, layout)
}

/// `wrap_text` with the mode indent supplied. Zero disables the indent (and so
/// the bullets), which is what every caller that has no `Layout` to hand wants.
pub fn wrap_text_indented(
    text: &str,
    font: &FontRef,
    scale: PxScale,
    max_width: f32,
    mode_indent: f32,
    symbol_size: u32,
    layout: &Layout,
) -> Vec<WrappedLine> {
    let text = insert_mode_dashes(text);
    let mut all_lines: Vec<WrappedLine> = Vec::new();
    let mut first_para = true;

    for raw_para in text.split("\n\n") {
        let para = raw_para.trim();
        if para.is_empty() {
            continue;
        }
        if !first_para {
            all_lines.push(WrappedLine::ParagraphBreak);
        }
        first_para = false;

        let mut first_chunk = true;
        for raw_chunk in para.split('\n') {
            let mode = is_mode(raw_chunk);
            let chunk = strip_mode_marker(raw_chunk)
                .split_whitespace()
                .collect::<Vec<_>>()
                .join(" ");
            if chunk.is_empty() {
                continue;
            }
            if !first_chunk {
                all_lines.push(WrappedLine::HardBreak);
            }
            first_chunk = false;

            let indent = if mode { mode_indent } else { 0.0 };
            let wrapped =
                wrap_paragraph(&chunk, font, scale, max_width - indent, symbol_size, layout);
            for (i, tokens) in wrapped.into_iter().enumerate() {
                all_lines.push(WrappedLine::Tokens(Line {
                    tokens,
                    indent,
                    bullet: mode && i == 0,
                }));
            }
        }
    }

    all_lines
}

/// Reconstruct a wrappable text string from a slice of WrappedLines.
/// Consecutive Tokens lines (soft-wrapped) are joined with a space.
/// ParagraphBreak → "\n\n", HardBreak → "\n".
fn lines_to_text(lines: &[WrappedLine]) -> String {
    let mut out = String::new();
    let mut prev_tokens = false;
    for line in lines {
        match line {
            WrappedLine::Tokens(line) => {
                // A mode starts a fresh source line carrying its marker back;
                // its wrapped continuations rejoin it with a space.
                if line.bullet {
                    // A break may already have been emitted for the HardBreak
                    // entry that precedes this mode; a second one would read
                    // back as a paragraph break and open a gap the list has not
                    // asked for.
                    if !out.is_empty() && !out.ends_with('\n') {
                        out.push('\n');
                    }
                    out.push_str("* ");
                } else if prev_tokens {
                    out.push(' ');
                }
                for t in &line.tokens {
                    out.push_str(&t.to_text_repr());
                }
                prev_tokens = true;
            }
            WrappedLine::ParagraphBreak => {
                out.push_str("\n\n");
                prev_tokens = false;
            }
            WrappedLine::HardBreak => {
                out.push('\n');
                prev_tokens = false;
            }
        }
    }
    out
}

/// Wrap `text` with a two-width split: the first `wide_limit` visible lines
/// (Tokens entries) are wrapped at `wide_max_width`; any remaining content is
/// re-wrapped at `narrow_max_width` and returned as the second element.
///
/// If the text fits entirely within `wide_limit` lines, the second Vec is empty.
#[allow(clippy::too_many_arguments)]
pub fn wrap_text_split(
    text: &str,
    font: &FontRef,
    scale: PxScale,
    wide_max_width: f32,
    narrow_max_width: f32,
    wide_limit: usize,
    mode_indent: f32,
    symbol_size: u32,
    layout: &Layout,
) -> (Vec<WrappedLine>, Vec<WrappedLine>) {
    let all = wrap_text_indented(
        text,
        font,
        scale,
        wide_max_width,
        mode_indent,
        symbol_size,
        layout,
    );

    // Count visible content lines (Tokens) plus paragraph separators.
    let total: usize = all
        .iter()
        .filter(|l| matches!(l, WrappedLine::Tokens(_) | WrappedLine::ParagraphBreak))
        .count();
    if total <= wide_limit {
        return (all, Vec::new());
    }

    // Find the index just after the wide_limit-th visible slot (Tokens or ParagraphBreak).
    let mut seen = 0usize;
    let mut split_at = all.len();
    for (i, line) in all.iter().enumerate() {
        match line {
            WrappedLine::Tokens(_) | WrappedLine::ParagraphBreak => {
                seen += 1;
                if seen == wide_limit {
                    split_at = i + 1;
                    break;
                }
            }
            WrappedLine::HardBreak => {}
        }
    }

    // Never break a mode across the width change: the narrow half is re-wrapped
    // from reconstructed text, which can only carry a mode that starts there.
    // Backing the split up to the mode's bullet keeps it whole, and keeps a
    // mode's own lines at one measure, which is what the printed lists do.
    while split_at > 0 && split_at < all.len() {
        match &all[split_at] {
            WrappedLine::Tokens(l) if l.indent > 0.0 && !l.bullet => split_at -= 1,
            _ => break,
        }
    }

    let wide_lines: Vec<WrappedLine> = all[..split_at].to_vec();
    let rest = &all[split_at..];

    // A paragraph break sitting exactly on the split boundary belongs to the
    // narrow block: re-wrapping the remainder as text would trim the leading
    // blank line away and silently run the two paragraphs together.
    let boundary_break = rest
        .first()
        .is_some_and(|l| matches!(l, WrappedLine::ParagraphBreak));

    let overflow_text = lines_to_text(rest);
    let mut narrow_lines = if overflow_text.trim().is_empty() {
        Vec::new()
    } else {
        wrap_text_indented(
            overflow_text.trim(),
            font,
            scale,
            narrow_max_width,
            mode_indent,
            symbol_size,
            layout,
        )
    };
    if boundary_break && !narrow_lines.is_empty() {
        narrow_lines.insert(0, WrappedLine::ParagraphBreak);
    }

    (wide_lines, narrow_lines)
}

// ── Fitting ───────────────────────────────────────────────────────────────────

/// Number of ability lines allowed to use the full text-box width before the
/// remainder falls back to the narrow width between the stat-bubble frames.
const WIDE_LINE_LIMIT: usize = 3;

/// Height consumed by the separator region between ability and flavor text.
const SEPARATOR_H: f32 = 1.0;

/// A fitted flavor-text block.
pub struct FlavorFit {
    pub spec: TypeSpec,
    pub lines: Vec<WrappedLine>,
}

/// A fully fitted rules-text block, ready to draw.
pub struct RulesFit {
    pub spec: TypeSpec,
    /// Ability lines 1-3, wrapped at the full text-box width.
    pub lines: Vec<WrappedLine>,
    /// Ability lines 4+, wrapped at the narrower width (empty when ≤ 3 lines).
    pub narrow_lines: Vec<WrappedLine>,
    pub flavor: Option<FlavorFit>,
    /// The text did not fit the normal box at full size, so it is set in the
    /// extended parchment region and centered there instead.
    pub overflow: bool,
    /// Height of the ability block as fitted, in pixels.
    pub height: f32,
}

fn block_height(lines: &[WrappedLine], line_height: f32, para_gap: f32) -> f32 {
    lines
        .iter()
        .map(|l| match l {
            WrappedLine::Tokens(_) => line_height,
            WrappedLine::ParagraphBreak => para_gap,
            WrappedLine::HardBreak => 0.0,
        })
        .sum()
}

/// Count visible (Tokens) lines in a WrappedLine slice.
fn count_tokens(lines: &[WrappedLine]) -> usize {
    lines
        .iter()
        .filter(|l| matches!(l, WrappedLine::Tokens(_)))
        .count()
}

/// Fit ability and flavor text within the layout's text box.
///
/// Ability text is always rendered at `layout.ability_size` — there is no
/// auto-scaling. Lines 1-3 use the normal margins; if a 4th line would be
/// needed, expanded margins are tried first, and any remaining lines fall back
/// to the narrow width between the stat-bubble frames. Warnings are printed
/// when the text exceeds the line limit or the push-up-free height.
///
/// Flavor text auto-scales downward until it fits the remaining vertical space
/// (bottoming out at `layout.flavor_size_min`).
pub fn fit_rules_text(
    ability: &str,
    flavor: Option<&str>,
    font: &FontRef,
    flavor_font: &FontRef,
    layout: &Layout,
) -> RulesFit {
    // Wrap the ability at one size. Normal margins first; if a 4th line is
    // needed, expanded margins are tried before accepting the narrow split.
    let wrap_at = |size: u32| {
        let spec = TypeSpec::ability(size, layout);
        let mode_indent = mode_indent_at(size, layout);
        let (wide, narrow) = wrap_text_split(
            ability,
            font,
            spec.scale,
            layout.rules_width(),
            layout.rules_width_narrow(),
            WIDE_LINE_LIMIT,
            mode_indent,
            spec.symbol_size,
            layout,
        );
        let (wide, narrow) = if narrow.is_empty() {
            (wide, narrow)
        } else {
            wrap_text_split(
                ability,
                font,
                spec.scale,
                layout.rules_width_expanded(),
                layout.rules_width_narrow(),
                WIDE_LINE_LIMIT,
                mode_indent,
                spec.symbol_size,
                layout,
            )
        };
        let height = block_height(&wide, spec.line_height, layout.para_gap)
            + block_height(&narrow, spec.line_height, layout.para_gap);
        (spec, wide, narrow, height)
    };

    // Full size, normal box. Every original lands here and is untouched by
    // everything below.
    let full = wrap_at(layout.ability_size);
    let overflow = full.3 > layout.rules_normal_height;

    // Overflowing text gets the lower strip of parchment and, if that is still
    // not enough, the largest size that fits it.
    let (spec, lines, narrow_lines, ability_h) = if overflow {
        let budget = layout.overflow_height();
        (layout.ability_size_min..=layout.ability_size)
            .rev()
            .map(wrap_at)
            .find(|(_, _, _, h)| *h <= budget)
            .unwrap_or_else(|| wrap_at(layout.ability_size_min))
    } else {
        full
    };

    let size = spec.scale.x as u32;
    let total_lines = count_tokens(&lines) + count_tokens(&narrow_lines);
    if total_lines > layout.max_ability_lines {
        eprintln!("note: ability text wraps to {total_lines} lines at size {size}");
    }

    // Flavor text auto-scales to fill the remaining vertical space.
    let flavor = flavor.map(|flav| {
        let box_h = if overflow {
            layout.overflow_height()
        } else {
            layout.text_box.height()
        };
        let remaining = box_h - ability_h - layout.para_gap - SEPARATOR_H - layout.para_gap;

        // Flavor text sits at the bottom of the box, where the stat-bubble
        // housings cut into it from both sides, so it wraps to the narrow width
        // even when the ability above it runs the full width of the box.
        let fit_at = |size: u32| {
            let spec = TypeSpec::flavor(size, layout);
            let lines = wrap_text(
                flav,
                flavor_font,
                spec.scale,
                layout.rules_width_narrow(),
                spec.symbol_size,
                layout,
            );
            FlavorFit { spec, lines }
        };

        (layout.flavor_size_min..=size)
            .rev()
            .map(fit_at)
            .find(|f| block_height(&f.lines, f.spec.line_height, layout.para_gap) <= remaining)
            .unwrap_or_else(|| fit_at(layout.flavor_size_min))
    });

    RulesFit {
        spec,
        lines,
        narrow_lines,
        flavor,
        overflow,
        height: ability_h,
    }
}

/// Whether a fitted block still runs past the bottom of the parchment.
///
/// Reaching here means the size search bottomed out at `ability_size_min` and
/// the text is *still* too tall. Nothing further can be done to it: the only
/// remaining moves are to set it smaller than a size anyone can read, or to draw
/// it over the bottom banner. Callers must refuse the card.
impl RulesFit {
    pub fn overflows_parchment(&self, layout: &Layout) -> bool {
        self.height > layout.overflow_height()
    }
}

/// Refuse ability text that cannot be rendered inside the parchment.
///
/// Checked before anything is drawn, so an over-long card fails instead of
/// silently writing a PNG with its last lines across the bottom banner.
pub fn check_rules_fit(
    ability: &str,
    flavor: Option<&str>,
    font: &FontRef,
    flavor_font: &FontRef,
    layout: &Layout,
) -> Result<(), String> {
    let chars = ability.chars().count();
    if chars > layout.ability_chars_max {
        return Err(format!(
            "ability text is too long: {chars} characters, limit {}. Text this long \
             only fits by shrinking the type below what the card can carry legibly.",
            layout.ability_chars_max
        ));
    }

    // The character limit is a readability judgement and counts characters, not
    // ink. Line breaks, long words and mana symbols all set wider than average,
    // so a card under the limit can still overrun the parchment; this is the
    // backstop that catches it.
    let fit = fit_rules_text(ability, flavor, font, flavor_font, layout);
    if !fit.overflows_parchment(layout) {
        return Ok(());
    }
    Err(format!(
        "ability text does not fit: {chars} characters wrap to {} lines at the minimum \
         size {}, a {:.0}px block against {:.0}px of parchment",
        count_tokens(&fit.lines) + count_tokens(&fit.narrow_lines),
        layout.ability_size_min,
        fit.height,
        layout.overflow_height(),
    ))
}

/// Compute the horizontal stretch/shrink for a card name.
///
/// The default scale stretches glyphs horizontally (x > y). If the stretched
/// name exceeds `name_max_width` the stretch is reduced until it exactly fills
/// the maximum; if even the natural (unstretched) width exceeds the maximum,
/// both axes shrink proportionally.
pub fn fit_name_scale(name: &str, font: &FontRef, layout: &Layout) -> PxScale {
    let (x_pts, y_pts) = layout.name_scale;
    let stretch_ratio = x_pts / y_pts;
    let uniform_scale = PxScale { x: y_pts, y: y_pts };
    let natural_w = measure_str(name, font, uniform_scale);
    let stretched_w = natural_w * stretch_ratio;

    if stretched_w <= layout.name_max_width {
        // Stretch fits: apply the full default stretch ratio.
        PxScale {
            x: y_pts * stretch_ratio,
            y: y_pts,
        }
    } else if natural_w <= layout.name_max_width {
        // Stretched exceeds max but natural fits: reduce stretch to exactly fill max.
        PxScale {
            x: y_pts * (layout.name_max_width / natural_w),
            y: y_pts,
        }
    } else {
        // Even natural width exceeds max: scale both axes down proportionally.
        let f = layout.name_max_width / natural_w;
        PxScale {
            x: y_pts * f,
            y: y_pts * f,
        }
    }
}

// ── Rasterization helpers ─────────────────────────────────────────────────────

/// How to lay ink down: what color, and how much the edge of a stroke spreads.
///
/// Carried together because every drawing call needs both, and because keeping
/// `ink_gain` beside the color makes it obvious at each call site that the
/// weight of the type is a rendering parameter, not a property of the font.
#[derive(Debug, Clone, Copy)]
pub struct Pen {
    pub color: [u8; 3],
    /// See `Layout::ink_gain`. 1.0 leaves coverage untouched.
    pub gain: f32,
}

impl Pen {
    pub fn new(color: [u8; 3], gain: f32) -> Pen {
        Pen { color, gain }
    }

    /// Coverage after simulated ink spread.
    fn apply(&self, coverage: f32) -> f32 {
        if self.gain == 1.0 {
            coverage
        } else {
            coverage.clamp(0.0, 1.0).powf(self.gain)
        }
    }
}

/// Blend a foreground color onto a background pixel using porter-duff "over".
fn blend(bg: &Rgba<u8>, fg: [u8; 3], coverage: f32) -> Rgba<u8> {
    let a = coverage.clamp(0.0, 1.0);
    let r = (fg[0] as f32 * a + bg[0] as f32 * (1.0 - a)) as u8;
    let g = (fg[1] as f32 * a + bg[1] as f32 * (1.0 - a)) as u8;
    let b = (fg[2] as f32 * a + bg[2] as f32 * (1.0 - a)) as u8;
    let out_a = ((a + bg[3] as f32 / 255.0 * (1.0 - a)) * 255.0) as u8;
    Rgba([r, g, b, out_a])
}

/// How a run of glyphs is set: the size, and the letter spacing applied between
/// adjacent glyphs. Carried together because a measurement and the draw it
/// belongs to must agree on both, or the ink is measured at one spacing and put
/// down at another.
#[derive(Debug, Clone, Copy)]
pub struct Run {
    pub scale: PxScale,
    /// Pixels added to the advance after every glyph but the last. Negative
    /// pulls the glyphs together without touching their outlines, which is how
    /// the stat bubbles fit two digits — see [`stats_tracking`].
    pub tracking: f32,
}

impl Run {
    /// A run at the font's own spacing.
    pub fn new(scale: PxScale) -> Run {
        Run {
            scale,
            tracking: 0.0,
        }
    }

    pub fn tracked(scale: PxScale, tracking: f32) -> Run {
        Run { scale, tracking }
    }
}

/// Draw a string at a specific baseline position on the canvas.
/// Returns the total advance width consumed.
pub fn draw_text_at_baseline(
    canvas: &mut RgbaImage,
    text: &str,
    pen_x: f32,
    baseline_y: f32,
    font: &FontRef,
    run: Run,
    pen: Pen,
) -> f32 {
    let Run { scale, tracking } = run;
    let scaled = font.as_scaled(scale);
    let mut x = pen_x;
    let mut prev: Option<GlyphId> = None;

    for c in text.chars() {
        let gid = scaled.glyph_id(c);
        if let Some(p) = prev {
            x += scaled.kern(p, gid) + tracking;
        }

        let glyph = Glyph {
            id: gid,
            scale,
            position: point(x, baseline_y),
        };

        if let Some(outlined) = font.outline_glyph(glyph) {
            let bounds = outlined.px_bounds();
            outlined.draw(|px, py, coverage| {
                let cx = bounds.min.x as i32 + px as i32;
                let cy = bounds.min.y as i32 + py as i32;
                if cx >= 0
                    && cy >= 0
                    && (cx as u32) < canvas.width()
                    && (cy as u32) < canvas.height()
                {
                    let existing = *canvas.get_pixel(cx as u32, cy as u32);
                    canvas.put_pixel(
                        cx as u32,
                        cy as u32,
                        blend(&existing, pen.color, pen.apply(coverage)),
                    );
                }
            });
        }

        x += scaled.h_advance(gid);
        prev = Some(gid);
    }

    x - pen_x
}

/// Bounding box of the ink a string actually puts on the page, measured
/// relative to a pen at x = 0 sitting on baseline y = 0.
///
/// This is not the same as the advance box. The advance box is as tall as the
/// font's ascent and descent — space reserved for accents and descenders that a
/// string like `-4` never uses — and as wide as the sum of the advances,
/// including the side bearings that pad the first and last glyph. Centering on
/// the advance box therefore centers the *slot*, not the *marks in it*, which
/// is why the stat bubbles all sat low: digits have no descender, so reserving
/// descender space below them pushed the visible glyphs down.
///
/// Returns `None` for a string that draws nothing (empty, or all whitespace).
pub fn ink_bounds(text: &str, font: &FontRef, run: Run) -> Option<(f32, f32, f32, f32)> {
    let Run { scale, tracking } = run;
    let scaled = font.as_scaled(scale);
    let mut x = 0.0f32;
    let mut prev: Option<GlyphId> = None;
    let mut bounds: Option<(f32, f32, f32, f32)> = None;

    for c in text.chars() {
        let gid = scaled.glyph_id(c);
        if let Some(p) = prev {
            x += scaled.kern(p, gid) + tracking;
        }
        let glyph = Glyph {
            id: gid,
            scale,
            position: point(x, 0.0),
        };
        if let Some(outlined) = font.outline_glyph(glyph) {
            let b = outlined.px_bounds();
            bounds = Some(match bounds {
                None => (b.min.x, b.min.y, b.max.x, b.max.y),
                Some((x0, y0, x1, y1)) => (
                    x0.min(b.min.x),
                    y0.min(b.min.y),
                    x1.max(b.max.x),
                    y1.max(b.max.y),
                ),
            });
        }
        x += scaled.h_advance(gid);
        prev = Some(gid);
    }

    bounds
}

/// Letter spacing for a hand or life modifier, in pixels.
///
/// A two-digit value has to fit the same circle a one-digit value does, and the
/// originals make room by pulling the glyphs together rather than by narrowing
/// or shrinking them — so this is a spacing adjustment, and `stats_size` is the
/// same either way.
pub fn stats_tracking(value: &str, layout: &Layout) -> f32 {
    let digits = value.chars().filter(|c| c.is_ascii_digit()).count();
    if digits > 1 {
        layout.stats_multi_digit_tracking
    } else {
        0.0
    }
}

/// Draw text so that the ink it lays down is centered on `(cx, cy)`.
///
/// This is the right anchor for the hand and life modifiers: the target is a
/// circle stamped on the template, and what has to sit in the middle of it is
/// the visible `-4`, not the typographic slot around it. Every stat value is a
/// sign followed by digits, all of the same cap height and none with a
/// descender, so ink-centering is stable across values.
///
/// It is the wrong anchor for running text or for the card name — see
/// [`draw_text_centered_on_baseline`].
pub fn draw_text_centered_on_ink(
    canvas: &mut RgbaImage,
    text: &str,
    cx: u32,
    cy: u32,
    font: &FontRef,
    run: Run,
    pen: Pen,
) {
    match ink_bounds(text, font, run) {
        Some((x0, y0, x1, y1)) => {
            let pen_x = cx as f32 - (x0 + x1) / 2.0;
            let baseline_y = cy as f32 - (y0 + y1) / 2.0;
            draw_text_at_baseline(canvas, text, pen_x, baseline_y, font, run, pen);
        }
        None => draw_text_centered_on_baseline(canvas, text, cx, cy, font, run.scale, pen),
    }
}

/// Draw text centered horizontally on `cx`, with its baseline placed so the
/// font's ascent-plus-descent slot is centered on `cy`.
///
/// The vertical position depends only on the font and its size, never on which
/// letters the string happens to contain. That is what a banner needs: every
/// card name must sit on the same baseline, so `Volrath` cannot ride higher
/// than `Sliver Queen, Brood Mother` merely because it has no descender.
pub fn draw_text_centered_on_baseline(
    canvas: &mut RgbaImage,
    text: &str,
    cx: u32,
    cy: u32,
    font: &FontRef,
    scale: PxScale,
    pen: Pen,
) {
    let scaled = font.as_scaled(scale);
    let ascent = scaled.ascent();
    let descent = -scaled.descent(); // make positive

    let pen_x = cx as f32 - measure_str(text, font, scale) / 2.0;
    let baseline_y = cy as f32 - (ascent + descent) / 2.0 + ascent;

    draw_text_at_baseline(canvas, text, pen_x, baseline_y, font, Run::new(scale), pen);
}

// ── Rules text block rendering ────────────────────────────────────────────────

/// Top of the ability block, for a fitted block of any height.
///
/// One rule for every block, overflowing or not.
///
/// Every original centres its ability on y ≈ 701: one line spans 688–714, two
/// 675–728, three 659–741 — the same middle, growing both ways.
///
/// Kept up, that centring walks a taller block off the top of the parchment: the
/// tallest original is 90 px, and a custom card's 120 px block put its first line
/// on the panel's top border. So the top pins where a 90 px block starts and the
/// block grows downward from there. That also keeps full-width lines clear of the
/// stat-bubble housings without a second rule — the wide portion is at most three
/// lines, and 656 + 90 = 746, above the narrowing at 754 — and it gives every long
/// card the same top margin, which centring in the parchment did not: a block that
/// happened to be short enough to centre sat 20 px lower than one that did not.
///
/// Only a block too tall to fit below the anchor is pushed back up, and no further
/// than the top of the parchment.
pub fn rules_block_top(fit: &RulesFit, layout: &Layout) -> f32 {
    let ability_h = block_height(&fit.lines, fit.spec.line_height, layout.para_gap)
        + block_height(&fit.narrow_lines, fit.spec.line_height, layout.para_gap);

    let center = layout.text_box.top as f32 + layout.rules_centering_height / 2.0;
    let top_anchor = center - layout.rules_calibrated_height / 2.0;

    (center - ability_h / 2.0)
        .max(top_anchor)
        .min(layout.rules_overflow_bottom - ability_h)
        .max(layout.rules_overflow_top)
}

/// Render the full rules text block onto the canvas.
///
/// Ability lines 1-3 are drawn centered in `layout.text_box`; lines 4+ (if
/// any) are drawn centered in `layout.narrow_text_box` to stay clear of the
/// stat-bubble frames. Flavor text follows after the separator using the same
/// centering as the last ability line.
pub fn draw_rules_text(
    canvas: &mut RgbaImage,
    fit: &RulesFit,
    font: &FontRef,
    flavor_font: &FontRef,
    layout: &Layout,
    pen: Pen,
) {
    let center_x = layout.text_box.center_x();
    let narrow_center_x = layout.narrow_text_box.center_x();

    // The bullet follows the type down with the indent it sits in.
    let bullet_radius = layout.mode_bullet_radius * fit.spec.scale.y / layout.ability_size as f32;

    let mut y = rules_block_top(fit, layout);

    // A modal ability is one list even when it spills into the narrow box, so
    // both halves are set flush against a single left edge: the widest line of
    // either, centered where the narrower half has to live.
    let modal_left = {
        let wide_w = block_width(&fit.lines, font, &fit.spec);
        let narrow_w = block_width(&fit.narrow_lines, font, &fit.spec);
        if fit.narrow_lines.is_empty() {
            center_x - wide_w / 2.0
        } else {
            // Centering on the widest line of either half can push the edge out
            // past the narrow box and into the stat-bubble housings, so it is
            // clamped to what the narrow half can actually occupy.
            let pad = layout.text_padding as f32;
            let lo = layout.narrow_text_box.left as f32 + pad;
            let hi = layout.narrow_text_box.right as f32 - pad - narrow_w;
            (narrow_center_x - wide_w.max(narrow_w) / 2.0).clamp(lo, hi.max(lo))
        }
    };
    let modal_left = Some(modal_left);

    // Ability lines 1-3 (full-width centering)
    draw_lines(
        canvas,
        &fit.lines,
        font,
        &fit.spec,
        center_x,
        layout.para_gap,
        layout.symbol_y_offset,
        bullet_radius,
        modal_left,
        pen,
        &mut y,
    );

    // Ability lines 4+ (narrow centering)
    draw_lines(
        canvas,
        &fit.narrow_lines,
        font,
        &fit.spec,
        narrow_center_x,
        layout.para_gap,
        layout.symbol_y_offset,
        bullet_radius,
        modal_left,
        pen,
        &mut y,
    );

    // Flavor text after the separator, centered like the last ability line.
    if let Some(flavor) = &fit.flavor {
        let flavor_cx = if fit.narrow_lines.is_empty() {
            center_x
        } else {
            narrow_center_x
        };
        y += layout.para_gap + SEPARATOR_H + layout.para_gap;

        draw_lines(
            canvas,
            &flavor.lines,
            flavor_font,
            &flavor.spec,
            flavor_cx,
            layout.para_gap,
            layout.symbol_y_offset,
            layout.mode_bullet_radius,
            None,
            pen,
            &mut y,
        );
    }
}

/// Draw a filled, antialiased disc — the bullet of a modal ability.
///
/// Drawn rather than set as `•` so that a mode's bullet is the same mark at the
/// same weight whatever face the rules text is in, and so that a font missing
/// the glyph cannot turn a mode list into a row of tofu.
fn fill_disc(canvas: &mut RgbaImage, cx: f32, cy: f32, radius: f32, pen: Pen) {
    let r = radius.max(0.0);
    let x0 = (cx - r - 1.0).floor().max(0.0) as u32;
    let y0 = (cy - r - 1.0).floor().max(0.0) as u32;
    let x1 = ((cx + r + 1.0).ceil() as u32).min(canvas.width());
    let y1 = ((cy + r + 1.0).ceil() as u32).min(canvas.height());

    for py in y0..y1 {
        for px in x0..x1 {
            let dx = px as f32 + 0.5 - cx;
            let dy = py as f32 + 0.5 - cy;
            // Coverage falls off over the outermost pixel, matching the way the
            // glyph rasterizer antialiases an edge.
            let coverage = (r + 0.5 - (dx * dx + dy * dy).sqrt()).clamp(0.0, 1.0);
            if coverage > 0.0 {
                let existing = *canvas.get_pixel(px, py);
                canvas.put_pixel(px, py, blend(&existing, pen.color, pen.apply(coverage)));
            }
        }
    }
}

/// Width of the widest line in a block, counting each line's indent.
fn block_width(lines: &[WrappedLine], font: &FontRef, spec: &TypeSpec) -> f32 {
    lines
        .iter()
        .filter_map(|l| match l {
            WrappedLine::Tokens(line) => {
                Some(line.indent + measure_tokens(&line.tokens, font, spec.scale, spec.symbol_size))
            }
            _ => None,
        })
        .fold(0.0f32, f32::max)
}

#[allow(clippy::too_many_arguments)]
fn draw_lines(
    canvas: &mut RgbaImage,
    lines: &[WrappedLine],
    font: &FontRef,
    spec: &TypeSpec,
    center_x: f32,
    para_gap: f32,
    symbol_y_offset: f32,
    bullet_radius: f32,
    // Left edge to set a modal block flush against. `None` derives it from this
    // block alone; the rules text passes one shared value so that lines 4+ line
    // up with the modes above them even though they are centered in a narrower box.
    modal_left: Option<f32>,
    pen: Pen,
    y: &mut f32,
) {
    let baseline_from_top = spec.baseline_from_top(font);

    // A modal block is set flush left as a unit and the unit is centered, so
    // every mode starts at the same x. Centering each line on its own, the way
    // an ordinary Vanguard rules block is set, would leave the bullets in a
    // ragged column and break the list.
    let modal = lines
        .iter()
        .any(|l| matches!(l, WrappedLine::Tokens(line) if line.bullet || line.indent > 0.0));
    let block_left = modal_left.unwrap_or_else(|| center_x - block_width(lines, font, spec) / 2.0);

    for line in lines {
        match line {
            WrappedLine::ParagraphBreak => {
                *y += para_gap;
            }
            WrappedLine::HardBreak => {
                // No extra spacing — next Tokens line advances by line_height as normal.
            }
            WrappedLine::Tokens(Line {
                tokens,
                indent,
                bullet,
            }) => {
                let line_w = measure_tokens(tokens, font, spec.scale, spec.symbol_size);
                let mut x = if modal {
                    block_left + indent
                } else {
                    center_x - line_w / 2.0
                };
                let baseline_y = *y + baseline_from_top;

                if *bullet {
                    // Centered in the gutter, and on the middle of the
                    // lowercase body rather than the baseline.
                    fill_disc(
                        canvas,
                        block_left + indent / 2.0,
                        baseline_y - spec.scale.y * 0.18,
                        bullet_radius,
                        pen,
                    );
                }
                let sym_center_y = *y + spec.line_height / 2.0 + symbol_y_offset;

                for token in tokens {
                    // A known symbol renders as an image; anything else falls
                    // back to its literal text form.
                    if let Token::Symbol(name) = token {
                        if let Some(sym_img) = symbols::load(name, spec.symbol_size) {
                            let sy = (sym_center_y - spec.symbol_size as f32 / 2.0) as i64;
                            image::imageops::overlay(canvas, &sym_img, x as i64, sy);
                            x += spec.symbol_size as f32;
                            continue;
                        }
                    }
                    x += draw_text_at_baseline(
                        canvas,
                        &token.to_text_repr(),
                        x,
                        baseline_y,
                        font,
                        Run::new(spec.scale),
                        pen,
                    );
                }

                *y += spec.line_height;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fonts::Fonts;
    use crate::layout;

    /// Longest ability text in the reference set (Sliver Queen), and the
    /// longest found across 168 custom cards (Ricardi Van Mouse, ~3× the
    /// median). One must be untouched by the overflow path; the other is the
    /// reason it exists.
    const ORIGINAL: &str = "All Slivers get +1/+1 for each other Sliver in play.\n\
                            Sliver tokens you control are 2/2 instead of 1/1.";
    const CUSTOM: &str = "Whenever land enters the battlefield, if a land has already \
        entered the battlefield this turn, that land loses hexproof and becomes a 0/1 \
        green Plant creature with haste. It's still a land.\n\n\
        {1}{G}: You may discard a green card from your hand, if you do, search your \
        library for land card and put it into play tapped. Activate only a sorcery and \
        only once each turn.";

    fn fit(ability: &str) -> RulesFit {
        let fonts = Fonts::load().unwrap();
        fit_rules_text(ability, None, &fonts.body, &fonts.body, &layout::DEFAULT)
    }

    #[test]
    fn text_that_fits_never_enters_the_overflow_path() {
        let fit = fit(ORIGINAL);
        assert!(!fit.overflow);
        assert_eq!(fit.spec.scale.x as u32, layout::DEFAULT.ability_size);
    }

    #[test]
    fn overlong_text_shrinks_only_as_far_as_it_must() {
        let fit = fit(CUSTOM);
        assert!(fit.overflow, "366 chars does not fit at full size");
        let size = fit.spec.scale.x as u32;
        assert!(
            (layout::DEFAULT.ability_size_min..layout::DEFAULT.ability_size).contains(&size),
            "shrank to {size}"
        );

        // One size larger must genuinely not fit, or the search stopped early.
        let l = &layout::DEFAULT;
        let fonts = Fonts::load().unwrap();
        let bigger = TypeSpec::ability(size + 1, l);
        let (wide, narrow) = wrap_text_split(
            CUSTOM,
            &fonts.body,
            bigger.scale,
            l.rules_width_expanded(),
            l.rules_width_narrow(),
            WIDE_LINE_LIMIT,
            l.mode_indent,
            bigger.symbol_size,
            l,
        );
        let h = block_height(&wide, bigger.line_height, l.para_gap)
            + block_height(&narrow, bigger.line_height, l.para_gap);
        assert!(h > l.overflow_height(), "size {} would have fit", size + 1);
    }

    #[test]
    fn overflow_text_fits_the_parchment() {
        let fit = fit(CUSTOM);
        let l = &layout::DEFAULT;
        let h = block_height(&fit.lines, fit.spec.line_height, l.para_gap)
            + block_height(&fit.narrow_lines, fit.spec.line_height, l.para_gap);
        assert!(h <= l.overflow_height(), "block {h} exceeds parchment");

        // And it starts at the same place a long block always starts: the top
        // anchor, which is where the tallest original's block begins. Only a
        // block too deep to fit below it is pushed back up.
        let anchor = l.text_box.top as f32 + l.rules_centering_height / 2.0
            - l.rules_calibrated_height / 2.0;
        let expected = anchor.min(l.rules_overflow_bottom - h);
        let y = rules_block_top(&fit, l);
        assert!(
            (y - expected).abs() < 0.5,
            "block top {y} is not the anchor {expected}"
        );
    }

    fn check(ability: &str) -> Result<(), String> {
        let fonts = Fonts::load().unwrap();
        check_rules_fit(ability, None, &fonts.body, &fonts.body, &layout::DEFAULT)
    }

    #[test]
    fn text_within_the_limits_is_accepted() {
        assert!(check(ORIGINAL).is_ok());
        // The longest real custom card in ../bug-vanguards, 366 characters.
        assert!(check(CUSTOM).is_ok());
    }

    #[test]
    fn text_over_the_character_limit_is_refused() {
        let long = "Draw a card and you gain 2 life. ".repeat(40);
        assert!(long.chars().count() > layout::DEFAULT.ability_chars_max);
        let err = check(&long).unwrap_err();
        assert!(err.contains("too long"), "{err}");
    }

    /// The character limit counts characters, not ink, so it cannot be the only
    /// guard: this card is well under it and still cannot be set.
    #[test]
    fn short_text_that_still_does_not_fit_is_refused() {
        let paragraphs = (0..14)
            .map(|i| format!("Draw a card {i}."))
            .collect::<Vec<_>>()
            .join("\n\n");
        assert!(paragraphs.chars().count() < layout::DEFAULT.ability_chars_max);
        let err = check(&paragraphs).unwrap_err();
        assert!(err.contains("does not fit"), "{err}");
    }

    /// A granted ability is a unit: `have “{T}: Draw a card.”` must not break
    /// after the opening quote, which is what greedy wrapping did.
    #[test]
    fn a_quote_that_fits_is_not_broken() {
        let text = "Bird creatures you control have \u{201c}{T}: Draw a card.\u{201d}";
        let lines = rendered(&wrap(text, layout::DEFAULT.rules_width()));
        assert!(lines.len() > 1, "should wrap at all: {lines:?}");
        let quote_line = lines
            .iter()
            .find(|(_, l)| l.contains('\u{201c}'))
            .expect("a line carries the quote");
        assert!(
            quote_line.1.contains('\u{201d}'),
            "the quote was split across lines: {lines:?}"
        );
    }

    /// A quote wider than the measure cannot be kept whole, and must stay
    /// breakable rather than overflowing the box.
    #[test]
    fn a_quote_too_wide_to_fit_still_breaks() {
        let text = "Create a token with \u{201c}Sacrifice this creature: This creature \
                    deals 1 damage to any target and you gain 1 life.\u{201d}";
        let width = layout::DEFAULT.rules_width();
        let lines = rendered(&wrap(text, width));
        let opens = lines
            .iter()
            .position(|(_, l)| l.contains('\u{201c}'))
            .unwrap();
        let closes = lines
            .iter()
            .position(|(_, l)| l.contains('\u{201d}'))
            .unwrap();
        assert!(
            closes > opens,
            "an over-wide quote must still break: {lines:?}"
        );
        let fonts = Fonts::load().unwrap();
        let spec = TypeSpec::ability(layout::DEFAULT.ability_size, &layout::DEFAULT);
        for (_, line) in &lines {
            let w = measure_str(line, &fonts.body, spec.scale);
            assert!(
                w <= width,
                "line {line:?} is {w}px, over the {width}px measure"
            );
        }
    }

    #[test]
    fn a_paragraph_break_on_the_split_boundary_survives() {
        let fonts = Fonts::load().unwrap();
        let l = &layout::DEFAULT;
        let spec = TypeSpec::ability(l.ability_size, l);

        // Three full lines, then a paragraph break: the break lands exactly on
        // the wide/narrow boundary, where re-wrapping the remainder as text
        // used to trim it away and run the paragraphs together.
        let text = "Whenever land enters the battlefield, if a land has already entered \
                    the battlefield this turn, that land loses hexproof.\n\n\
                    Activate only as a sorcery.";
        let (wide, narrow) = wrap_text_split(
            text,
            &fonts.body,
            spec.scale,
            l.rules_width(),
            l.rules_width_narrow(),
            WIDE_LINE_LIMIT,
            l.mode_indent,
            spec.symbol_size,
            l,
        );
        assert_eq!(count_tokens(&wide), WIDE_LINE_LIMIT);
        assert!(matches!(narrow.first(), Some(WrappedLine::ParagraphBreak)));
        assert!(count_tokens(&narrow) > 0);
    }

    fn wrap(text: &str, width: f32) -> Vec<WrappedLine> {
        let fonts = Fonts::load().unwrap();
        let spec = TypeSpec::ability(layout::DEFAULT.ability_size, &layout::DEFAULT);
        wrap_text_indented(
            text,
            &fonts.body,
            spec.scale,
            width,
            layout::DEFAULT.mode_indent,
            spec.symbol_size,
            &layout::DEFAULT,
        )
    }

    fn rendered(lines: &[WrappedLine]) -> Vec<(bool, String)> {
        lines
            .iter()
            .filter_map(|l| match l {
                WrappedLine::Tokens(line) => Some((
                    line.bullet,
                    line.tokens.iter().map(|t| t.to_text_repr()).collect(),
                )),
                _ => None,
            })
            .collect()
    }

    /// The em dash is derived from the modes below the line, not written by the
    /// author — which is what makes the untriggered form work.
    #[test]
    fn a_mode_list_puts_a_dash_on_the_line_that_introduces_it() {
        let lines = rendered(&wrap("Choose one\n* Draw a card.\n* Gain 2 life.", 460.0));
        assert_eq!(
            lines,
            vec![
                (false, "Choose one —".to_string()),
                (true, "Draw a card.".to_string()),
                (true, "Gain 2 life.".to_string()),
            ]
        );
    }

    /// …and the triggered form is the same construct with a longer introduction.
    #[test]
    fn a_trigger_clause_takes_the_dash_the_same_way() {
        let lines = rendered(&wrap(
            "At the beginning of your upkeep, choose one\n* Draw a card.\n* Gain 2 life.",
            460.0,
        ));
        let header: String = lines
            .iter()
            .take_while(|(bullet, _)| !bullet)
            .map(|(_, s)| s.as_str())
            .collect::<Vec<_>>()
            .join(" ");
        assert_eq!(header, "At the beginning of your upkeep, choose one —");
        let modes: Vec<_> = lines.iter().filter(|(b, _)| *b).collect();
        assert_eq!(modes.len(), 2);
    }

    /// Re-wrapping already-wrapped text (`wrap_text_split` does exactly this to
    /// the overflow half) must not add a second dash.
    #[test]
    fn the_dash_is_not_added_twice() {
        let once = "Choose one —\n* Draw a card.";
        assert_eq!(insert_mode_dashes(once), once);
    }

    /// A mode that wraps hangs under its own text, and only its first line is
    /// bulleted.
    #[test]
    fn a_wrapped_mode_hangs_under_itself() {
        let lines = wrap(
            "Choose one\n* Target creature gets +3/+3 and gains trample until end of turn.",
            240.0,
        );
        let modes: Vec<_> = rendered(&lines);
        assert!(modes.len() > 2, "expected the mode to wrap: {modes:?}");
        assert!(modes[1].0, "first line of the mode carries the bullet");
        assert!(!modes[2].0, "its continuation does not");

        let indents: Vec<f32> = lines
            .iter()
            .filter_map(|l| match l {
                WrappedLine::Tokens(line) => Some(line.indent),
                _ => None,
            })
            .collect();
        assert_eq!(indents[0], 0.0);
        assert!(indents[1] > 0.0 && indents[1] == indents[2]);
    }

    /// Nothing about an ordinary ability changes — no indent, no bullet, no dash.
    #[test]
    fn ordinary_text_is_untouched() {
        let lines = wrap("During your draw phase,\ndraw an additional card.", 460.0);
        for l in &lines {
            if let WrappedLine::Tokens(line) = l {
                assert!(!line.bullet);
                assert_eq!(line.indent, 0.0);
            }
        }
        assert!(!rendered(&lines).iter().any(|(_, s)| s.contains('—')));
    }

    /// A line that really does start with an asterisk escapes with `\*`.
    #[test]
    fn an_escaped_asterisk_is_literal_text() {
        let lines = rendered(&wrap("\\*not a mode", 460.0));
        assert_eq!(lines, vec![(false, "*not a mode".to_string())]);
    }
}

#[cfg(test)]
mod intro_tests {
    use super::*;

    /// The line that introduces a list is never inspected: the dash follows from
    /// the modes below it, so any wording and any capitalization works, and a
    /// count word is just text. Nothing here is a keyword.
    #[test]
    fn any_introduction_takes_the_dash() {
        for intro in [
            "Choose one",
            "choose one",
            "Choose two",
            "choose two",
            "Choose three",
            "Choose one or both",
            "Choose both",
            "CHOOSE ONE",
            "When this creature enters, choose two",
            "At the beginning of your end step, choose one or more",
            "Pick whichever of these you like",
        ] {
            let out = insert_mode_dashes(&format!("{intro}\n* A.\n* B."));
            assert_eq!(
                out.lines().next().unwrap(),
                format!("{intro} —"),
                "introduction {intro:?} did not take the dash"
            );
        }
    }
}

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

#[derive(Debug, Clone)]
pub enum WrappedLine {
    Tokens(Vec<Token>),
    /// Extra inter-paragraph gap (triggered by `\n\n` in input).
    ParagraphBreak,
    /// Forced line break with normal line spacing (triggered by single `\n`).
    HardBreak,
}

fn wrap_paragraph(
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

    for word in para.split_whitespace() {
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

/// Word-wrap ability text into lines.
///
/// `\n\n` separates paragraphs (inserts a `ParagraphBreak` with extra spacing).
/// Single `\n` is a hard line break (inserts a `HardBreak`, normal line spacing).
pub fn wrap_text(
    text: &str,
    font: &FontRef,
    scale: PxScale,
    max_width: f32,
    symbol_size: u32,
) -> Vec<WrappedLine> {
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
        for chunk in para.split('\n') {
            let chunk = chunk.split_whitespace().collect::<Vec<_>>().join(" ");
            if chunk.is_empty() {
                continue;
            }
            if !first_chunk {
                all_lines.push(WrappedLine::HardBreak);
            }
            first_chunk = false;
            let wrapped = wrap_paragraph(&chunk, font, scale, max_width, symbol_size);
            for line in wrapped {
                all_lines.push(WrappedLine::Tokens(line));
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
            WrappedLine::Tokens(tokens) => {
                if prev_tokens {
                    out.push(' ');
                }
                for t in tokens {
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
pub fn wrap_text_split(
    text: &str,
    font: &FontRef,
    scale: PxScale,
    wide_max_width: f32,
    narrow_max_width: f32,
    wide_limit: usize,
    symbol_size: u32,
) -> (Vec<WrappedLine>, Vec<WrappedLine>) {
    let all = wrap_text(text, font, scale, wide_max_width, symbol_size);

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
        wrap_text(
            overflow_text.trim(),
            font,
            scale,
            narrow_max_width,
            symbol_size,
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
        let (wide, narrow) = wrap_text_split(
            ability,
            font,
            spec.scale,
            layout.rules_width(),
            layout.rules_width_narrow(),
            WIDE_LINE_LIMIT,
            spec.symbol_size,
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
                spec.symbol_size,
            )
        };
        let height = block_height(&wide, spec.line_height, layout.para_gap)
            + block_height(&narrow, spec.line_height, layout.para_gap);
        (spec, wide, narrow, height)
    };

    // Full size, normal box. Every original lands here and is untouched by
    // everything below.
    let full = wrap_at(layout.ability_size);
    let overflow = full.3 > layout.pushup_free_height();

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

/// Render the full rules text block onto the canvas.
///
/// Ability lines 1-3 are drawn centered in `layout.text_box`; lines 4+ (if
/// any) are drawn centered in `layout.narrow_text_box` to stay clear of the
/// stat-bubble frames. Flavor text follows after the separator using the same
/// centering as the last ability line.
/// Blank space between the ability block's line boxes and the ink inside them:
/// leading above the first line's tallest glyph, and descent below the last
/// line's lowest. Subtracting these is what makes a margin measured from the
/// parchment edge agree with what the eye sees.
fn ink_padding(fit: &RulesFit, font: &FontRef) -> (f32, f32) {
    let text_of = |line: &WrappedLine| match line {
        WrappedLine::Tokens(tokens) => {
            Some(tokens.iter().map(|t| t.to_text_repr()).collect::<String>())
        }
        _ => None,
    };
    let visible = || {
        fit.lines
            .iter()
            .chain(fit.narrow_lines.iter())
            .filter_map(text_of)
    };

    let baseline = fit.spec.baseline_from_top(font);
    let run = Run::new(fit.spec.scale);
    let bounds = |text: Option<String>| text.and_then(|t| ink_bounds(&t, font, run));

    // A line of symbols only has no outlined glyphs; fall back to no padding.
    let pad_top = bounds(visible().next())
        .map(|(_, y0, _, _)| (baseline + y0).max(0.0))
        .unwrap_or(0.0);
    let pad_bottom = bounds(visible().next_back())
        .map(|(_, _, _, y1)| (fit.spec.line_height - (baseline + y1)).max(0.0))
        .unwrap_or(0.0);

    (pad_top, pad_bottom)
}

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

    let ability_h = block_height(&fit.lines, fit.spec.line_height, layout.para_gap)
        + block_height(&fit.narrow_lines, fit.spec.line_height, layout.para_gap);

    // Short blocks: center within the calibrated centering region.
    // Tall blocks that exceed that region: center within the full text box.
    // y_start is floored at rules_min_y so long blocks never drift above it.
    let mut y = if fit.overflow {
        // Overflow text is placed in the parchment it actually occupies, which
        // reaches below text_box.bottom, and is balanced on its ink rather than
        // on its line boxes — see `rules_overflow_top_share`.
        let (pad_top, pad_bottom) = ink_padding(fit, font);
        let ink_h = (ability_h - pad_top - pad_bottom).max(0.0);
        let free = (layout.overflow_height() - ink_h).max(0.0);
        layout.rules_overflow_top + free * layout.rules_overflow_top_share - pad_top
    } else {
        let offset = if ability_h <= layout.rules_centering_height {
            (layout.rules_centering_height - ability_h) / 2.0
        } else {
            ((layout.text_box.height() - ability_h) / 2.0).max(0.0)
        };
        (layout.text_box.top as f32 + offset).max(layout.rules_min_y)
    };

    // Ability lines 1-3 (full-width centering)
    draw_lines(
        canvas,
        &fit.lines,
        font,
        &fit.spec,
        center_x,
        layout.para_gap,
        layout.symbol_y_offset,
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
            pen,
            &mut y,
        );
    }
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
    pen: Pen,
    y: &mut f32,
) {
    let baseline_from_top = spec.baseline_from_top(font);

    for line in lines {
        match line {
            WrappedLine::ParagraphBreak => {
                *y += para_gap;
            }
            WrappedLine::HardBreak => {
                // No extra spacing — next Tokens line advances by line_height as normal.
            }
            WrappedLine::Tokens(tokens) => {
                let line_w = measure_tokens(tokens, font, spec.scale, spec.symbol_size);
                let mut x = center_x - line_w / 2.0;
                let baseline_y = *y + baseline_from_top;
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
            bigger.symbol_size,
        );
        let h = block_height(&wide, bigger.line_height, l.para_gap)
            + block_height(&narrow, bigger.line_height, l.para_gap);
        assert!(h > l.overflow_height(), "size {} would have fit", size + 1);
    }

    #[test]
    fn overflow_text_fits_the_parchment() {
        let fit = fit(CUSTOM);
        let fonts = Fonts::load().unwrap();
        let l = &layout::DEFAULT;
        let h = block_height(&fit.lines, fit.spec.line_height, l.para_gap)
            + block_height(&fit.narrow_lines, fit.spec.line_height, l.para_gap);
        assert!(h <= l.overflow_height(), "block {h} exceeds parchment");

        // And its ink is balanced within it, rather than its line boxes.
        let (pad_top, pad_bottom) = ink_padding(&fit, &fonts.body);
        assert!(pad_top > 0.0 && pad_bottom > 0.0);
        let ink_h = h - pad_top - pad_bottom;
        let free = l.overflow_height() - ink_h;
        let top = free * l.rules_overflow_top_share;
        assert!(
            (top - (free - top)).abs() <= 1.0,
            "ink margins {top} / {} are not balanced",
            free - top
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
            spec.symbol_size,
        );
        assert_eq!(count_tokens(&wide), WIDE_LINE_LIMIT);
        assert!(matches!(narrow.first(), Some(WrappedLine::ParagraphBreak)));
        assert!(count_tokens(&narrow) > 0);
    }
}

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
    /// Ability-text spec: symbols render at exactly the font size.
    fn ability(size: u32, line_height_factor: f32) -> Self {
        TypeSpec {
            scale: PxScale::from(size as f32),
            line_height: size as f32 * line_height_factor,
            symbol_size: size,
        }
    }

    /// Flavor-text spec: symbols render slightly larger than the font size.
    fn flavor(size: u32, line_height_factor: f32) -> Self {
        TypeSpec {
            scale: PxScale::from(size as f32),
            line_height: size as f32 * line_height_factor,
            symbol_size: (size as f32 * 1.1) as u32,
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
    let overflow_text = lines_to_text(&all[split_at..]);
    let narrow_lines = if overflow_text.trim().is_empty() {
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
    let size = layout.ability_size;
    let spec = TypeSpec::ability(size, layout.line_height_factor);

    // Try normal margins first; if a 4th line is needed, retry with expanded
    // margins before accepting the narrow split.
    let (lines, narrow_lines) = {
        let (wide, narrow) = wrap_text_split(
            ability,
            font,
            spec.scale,
            layout.rules_width(),
            layout.rules_width_narrow(),
            WIDE_LINE_LIMIT,
            spec.symbol_size,
        );
        if narrow.is_empty() {
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
        }
    };

    let total_lines = count_tokens(&lines) + count_tokens(&narrow_lines);
    if total_lines > layout.max_ability_lines {
        eprintln!(
            "warning: ability text wraps to {total_lines} lines at size {size} — \
             exceeds the {}-line limit",
            layout.max_ability_lines
        );
    }

    let ability_h = block_height(&lines, spec.line_height, layout.para_gap)
        + block_height(&narrow_lines, spec.line_height, layout.para_gap);
    if ability_h > layout.pushup_free_height() {
        eprintln!(
            "warning: ability text block ({ability_h:.0}px) exceeds push-up-free height \
             ({:.0}px) — text would rise above the y_start floor",
            layout.pushup_free_height()
        );
    }

    // Flavor text auto-scales to fill the remaining vertical space.
    let flavor = flavor.map(|flav| {
        let remaining =
            layout.text_box.height() - ability_h - layout.para_gap - SEPARATOR_H - layout.para_gap;

        let fit_at = |size: u32| {
            let spec = TypeSpec::flavor(size, layout.line_height_factor);
            let lines = wrap_text(
                flav,
                flavor_font,
                spec.scale,
                layout.rules_width(),
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
    }
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

/// Blend a foreground color onto a background pixel using porter-duff "over".
fn blend(bg: &Rgba<u8>, fg: [u8; 3], coverage: f32) -> Rgba<u8> {
    let a = coverage.clamp(0.0, 1.0);
    let r = (fg[0] as f32 * a + bg[0] as f32 * (1.0 - a)) as u8;
    let g = (fg[1] as f32 * a + bg[1] as f32 * (1.0 - a)) as u8;
    let b = (fg[2] as f32 * a + bg[2] as f32 * (1.0 - a)) as u8;
    let out_a = ((a + bg[3] as f32 / 255.0 * (1.0 - a)) * 255.0) as u8;
    Rgba([r, g, b, out_a])
}

/// Draw a string at a specific baseline position on the canvas.
/// Returns the total advance width consumed.
pub fn draw_text_at_baseline(
    canvas: &mut RgbaImage,
    text: &str,
    pen_x: f32,
    baseline_y: f32,
    font: &FontRef,
    scale: PxScale,
    color: [u8; 3],
) -> f32 {
    let scaled = font.as_scaled(scale);
    let mut x = pen_x;
    let mut prev: Option<GlyphId> = None;

    for c in text.chars() {
        let gid = scaled.glyph_id(c);
        if let Some(p) = prev {
            x += scaled.kern(p, gid);
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
                    canvas.put_pixel(cx as u32, cy as u32, blend(&existing, color, coverage));
                }
            });
        }

        x += scaled.h_advance(gid);
        prev = Some(gid);
    }

    x - pen_x
}

/// Draw text centered horizontally and vertically at a point.
pub fn draw_centered_text(
    canvas: &mut RgbaImage,
    text: &str,
    cx: u32,
    cy: u32,
    font: &FontRef,
    scale: PxScale,
    color: [u8; 3],
) {
    let scaled = font.as_scaled(scale);
    let width = measure_str(text, font, scale);
    let ascent = scaled.ascent();
    let descent = -scaled.descent(); // make positive
    let text_h = ascent + descent;

    let pen_x = cx as f32 - width / 2.0;
    let baseline_y = cy as f32 - text_h / 2.0 + ascent;

    draw_text_at_baseline(canvas, text, pen_x, baseline_y, font, scale, color);
}

// ── Rules text block rendering ────────────────────────────────────────────────

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
    color: [u8; 3],
) {
    let center_x = layout.text_box.center_x();
    let narrow_center_x = layout.narrow_text_box.center_x();

    let ability_h = block_height(&fit.lines, fit.spec.line_height, layout.para_gap)
        + block_height(&fit.narrow_lines, fit.spec.line_height, layout.para_gap);

    // Short blocks: center within the calibrated centering region.
    // Tall blocks that exceed that region: center within the full text box.
    // y_start is floored at rules_min_y so long blocks never drift above it.
    let offset = if ability_h <= layout.rules_centering_height {
        (layout.rules_centering_height - ability_h) / 2.0
    } else {
        ((layout.text_box.height() - ability_h) / 2.0).max(0.0)
    };
    let mut y = (layout.text_box.top as f32 + offset).max(layout.rules_min_y);

    // Ability lines 1-3 (full-width centering)
    draw_lines(
        canvas,
        &fit.lines,
        font,
        &fit.spec,
        center_x,
        layout.para_gap,
        color,
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
        color,
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
            color,
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
    color: [u8; 3],
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
                let sym_center_y = *y + spec.line_height / 2.0;

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
                        spec.scale,
                        color,
                    );
                }

                *y += spec.line_height;
            }
        }
    }
}

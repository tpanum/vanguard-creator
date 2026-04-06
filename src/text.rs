use ab_glyph::{point, Font, FontRef, Glyph, GlyphId, PxScale, ScaleFont};
use image::{Rgba, RgbaImage};
use regex::Regex;
use std::sync::OnceLock;

use crate::symbols;

// ── Tokenization ─────────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub enum Token {
    Text(String),
    /// Mana symbol name (the content inside {…})
    Symbol(String),
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

#[derive(Debug)]
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
    let scaled = font.as_scaled(scale);
    let space_w = measure_str(" ", font, scale);
    let words: Vec<&str> = para.split_whitespace().collect();

    let mut lines: Vec<Vec<Token>> = Vec::new();
    let mut current: Vec<Token> = Vec::new();
    let mut current_w = 0.0f32;

    for word in &words {
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

    let _ = scaled;

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

// ── Auto-scaling ──────────────────────────────────────────────────────────────

pub struct FitResult {
    pub scale: PxScale,
    pub lines: Vec<WrappedLine>,
    pub line_height: f32,
    pub symbol_size: u32,
    pub flavor_lines: Option<Vec<WrappedLine>>,
    pub flavor_scale: Option<PxScale>,
    pub flavor_line_height: Option<f32>,
    pub flavor_symbol_size: Option<u32>,
}

/// Height consumed by the separator region between ability and flavor text.
const SEPARATOR_H: f32 = 1.0;

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

/// Fit ability and flavor text into `box_height`.
///
/// Ability text is always rendered at the largest size where it fits alone.
/// Flavor text then independently auto-scales to fill the remaining space.
#[allow(clippy::too_many_arguments)]
pub fn fit_ability_text(
    ability: &str,
    flavor: Option<&str>,
    font: &FontRef,
    flavor_font: &FontRef,
    max_width: f32,
    box_height: f32,
    size_max: u32,
    size_min: u32,
    para_gap: f32,
    line_height_factor: f32,
) -> FitResult {
    // Pass 1: find largest size where ability text alone fits.
    let (scale, lines, line_height, symbol_size) = (size_min..=size_max)
        .rev()
        .find_map(|size| {
            let scale = PxScale::from(size as f32);
            let symbol_size = (size as f32 * 1.1) as u32;
            let line_height = size as f32 * line_height_factor;
            let lines = wrap_text(ability, font, scale, max_width, symbol_size);
            let h = block_height(&lines, line_height, para_gap);
            if h <= box_height {
                Some((scale, lines, line_height, symbol_size))
            } else {
                None
            }
        })
        .unwrap_or_else(|| {
            // Fallback: min size
            let scale = PxScale::from(size_min as f32);
            let symbol_size = (size_min as f32 * 1.1) as u32;
            let line_height = size_min as f32 * line_height_factor;
            let lines = wrap_text(ability, font, scale, max_width, symbol_size);
            (scale, lines, line_height, symbol_size)
        });

    // Pass 2: fit flavor text into the space below the ability block.
    let (flavor_lines, flavor_scale, flavor_line_height, flavor_symbol_size) = match flavor {
        None => (None, None, None, None),
        Some(flav) => {
            let ability_h = block_height(&lines, line_height, para_gap);
            let remaining = box_height - ability_h - para_gap - SEPARATOR_H - para_gap;

            let result = (size_min..=size_max).rev().find_map(|size| {
                let fscale = PxScale::from(size as f32);
                let fsym = (size as f32 * 1.1) as u32;
                let flh = size as f32 * line_height_factor;
                let fl = wrap_text(flav, flavor_font, fscale, max_width, fsym);
                let fh = block_height(&fl, flh, para_gap);
                if fh <= remaining {
                    Some((fl, fscale, flh, fsym))
                } else {
                    None
                }
            });

            match result {
                Some((fl, fscale, flh, fsym)) => (Some(fl), Some(fscale), Some(flh), Some(fsym)),
                None => {
                    // Fallback: min size
                    let fscale = PxScale::from(size_min as f32);
                    let fsym = (size_min as f32 * 1.1) as u32;
                    let flh = size_min as f32 * line_height_factor;
                    let fl = wrap_text(flav, flavor_font, fscale, max_width, fsym);
                    (Some(fl), Some(fscale), Some(flh), Some(fsym))
                }
            }
        }
    };

    FitResult {
        scale,
        lines,
        line_height,
        symbol_size,
        flavor_lines,
        flavor_scale,
        flavor_line_height,
        flavor_symbol_size,
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

// ── Ability text block rendering ──────────────────────────────────────────────

/// Render the full ability text block onto the canvas.
///
/// Ability text is top-aligned at `box_top + top_padding`. Flavor text follows
/// after the separator. This matches the original card layout where ability text
/// always occupies the top of the text box regardless of flavor presence.
///
/// * `text_box` — (left, top, right, bottom) in pixels
#[allow(clippy::too_many_arguments)]
pub fn draw_ability_text(
    canvas: &mut RgbaImage,
    fit: &FitResult,
    text_box: (u32, u32, u32, u32),
    font: &FontRef,
    flavor_font: &FontRef,
    para_gap: f32,
    centering_height: f32,
    stroke: u32,
    color: [u8; 3],
) {
    let (box_left, box_top, box_right, box_bottom) = text_box;
    let center_x = (box_left + box_right) as f32 / 2.0;

    let baseline_offset = |f: &FontRef, scale: PxScale, lh: f32| {
        let scaled = f.as_scaled(scale);
        let ascent = scaled.ascent();
        let descent = -scaled.descent();
        let text_total_h = ascent + descent;
        (lh - text_total_h) / 2.0 + ascent
    };

    let ability_baseline_from_top = baseline_offset(font, fit.scale, fit.line_height);

    let block_h = block_height(&fit.lines, fit.line_height, para_gap);
    let box_h = (box_bottom - box_top) as f32;
    // Short blocks: center within the calibrated centering region (preserves original
    // card feel for 1-3 lines).  Tall blocks that exceed that region: center within
    // the full text box so they don't snap to the top and leave a gap at the bottom.
    let offset = if block_h <= centering_height {
        (centering_height - block_h) / 2.0
    } else {
        ((box_h - block_h) / 2.0).max(0.0)
    };
    let mut y = box_top as f32 + offset;

    // Draw ability lines
    draw_lines(
        canvas,
        &fit.lines,
        font,
        fit.scale,
        fit.symbol_size,
        fit.line_height,
        ability_baseline_from_top,
        center_x,
        para_gap,
        stroke,
        color,
        &mut y,
    );

    // Draw separator and flavor if present
    if let (Some(fl), Some(fscale), Some(flh), Some(fsym)) = (
        &fit.flavor_lines,
        fit.flavor_scale,
        fit.flavor_line_height,
        fit.flavor_symbol_size,
    ) {
        y += para_gap + SEPARATOR_H + para_gap;

        let flavor_baseline_from_top = baseline_offset(flavor_font, fscale, flh);
        draw_lines(
            canvas,
            fl,
            flavor_font,
            fscale,
            fsym,
            flh,
            flavor_baseline_from_top,
            center_x,
            para_gap,
            0,
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
    scale: PxScale,
    symbol_size: u32,
    line_height: f32,
    baseline_from_top: f32,
    center_x: f32,
    para_gap: f32,
    stroke: u32,
    color: [u8; 3],
    y: &mut f32,
) {
    // Stroke offsets: draw text at each offset before the final on-pixel pass.
    // This thickens strokes uniformly, simulating a weight between regular and bold.
    let offsets: &[(f32, f32)] = match stroke {
        0 => &[],
        1 => &[(-1.0, 0.0), (1.0, 0.0), (0.0, -1.0), (0.0, 1.0)],
        _ => &[
            (-1.0, -1.0),
            (-1.0, 0.0),
            (-1.0, 1.0),
            (0.0, -1.0),
            (0.0, 1.0),
            (1.0, -1.0),
            (1.0, 0.0),
            (1.0, 1.0),
        ],
    };

    for line in lines {
        match line {
            WrappedLine::ParagraphBreak => {
                *y += para_gap;
            }
            WrappedLine::HardBreak => {
                // No extra spacing — next Tokens line advances by line_height as normal.
            }
            WrappedLine::Tokens(tokens) => {
                let line_w = measure_tokens(tokens, font, scale, symbol_size);
                let mut x = center_x - line_w / 2.0;
                let baseline_y = *y + baseline_from_top;
                let sym_center_y = *y + line_height / 2.0;

                for token in tokens {
                    match token {
                        Token::Text(s) => {
                            for &(dx, dy) in offsets {
                                draw_text_at_baseline(
                                    canvas,
                                    s,
                                    x + dx,
                                    baseline_y + dy,
                                    font,
                                    scale,
                                    color,
                                );
                            }
                            let advance =
                                draw_text_at_baseline(canvas, s, x, baseline_y, font, scale, color);
                            x += advance;
                        }
                        Token::Symbol(name) => {
                            if let Some(sym_img) = symbols::load(name, symbol_size) {
                                let sy = (sym_center_y - symbol_size as f32 / 2.0) as i64;
                                image::imageops::overlay(canvas, &sym_img, x as i64, sy);
                                x += symbol_size as f32;
                            } else {
                                let fallback = format!("{{{name}}}");
                                for &(dx, dy) in offsets {
                                    draw_text_at_baseline(
                                        canvas,
                                        &fallback,
                                        x + dx,
                                        baseline_y + dy,
                                        font,
                                        scale,
                                        color,
                                    );
                                }
                                let advance = draw_text_at_baseline(
                                    canvas, &fallback, x, baseline_y, font, scale, color,
                                );
                                x += advance;
                            }
                        }
                    }
                }

                *y += line_height;
            }
        }
    }
}

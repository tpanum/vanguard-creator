/// Axis-aligned pixel rectangle: `left`/`top` inclusive, `right`/`bottom` exclusive.
#[derive(Debug, Clone, Copy)]
pub struct Rect {
    pub left: u32,
    pub top: u32,
    pub right: u32,
    pub bottom: u32,
}

impl Rect {
    pub const fn new(left: u32, top: u32, right: u32, bottom: u32) -> Self {
        Rect {
            left,
            top,
            right,
            bottom,
        }
    }

    pub fn width(&self) -> f32 {
        (self.right - self.left) as f32
    }

    pub fn height(&self) -> f32 {
        (self.bottom - self.top) as f32
    }

    pub fn center_x(&self) -> f32 {
        (self.left + self.right) as f32 / 2.0
    }
}

/// Template layout coordinates for the 718×1024 Vanguard card template.
/// All values are pixel coordinates measured from the reference template.
#[derive(Debug, Clone)]
pub struct Layout {
    /// Artwork transparent region
    pub art_box: Rect,
    /// Name banner center point
    pub name_center: (u32, u32),
    /// Text box interior
    pub text_box: Rect,
    /// Hand stat circle center
    pub hand_center: (u32, u32),
    /// Life stat circle center
    pub life_center: (u32, u32),
    /// Horizontal padding inside the text box
    pub text_padding: u32,
    /// Fixed pixel gap between paragraphs (not scaled with font)
    pub para_gap: f32,
    /// Ability text font size. Every original is set at this size and none of
    /// them needs less; it is only reduced when the text cannot be made to fit
    /// the box at full size (see `ability_size_min`).
    pub ability_size: u32,
    /// Smallest size ability text may shrink to when it does not fit at
    /// `ability_size`. Custom cards can carry three times the rules text of any
    /// original, which no amount of wrapping fits at 24 px. Shrinking engages
    /// only after the block overflows the box, so every card that fits at full
    /// size — which is all 25 originals — renders exactly as before.
    pub ability_size_min: u32,
    /// Longest ability text accepted, in characters. Cards above it are refused
    /// rather than rendered.
    ///
    /// This is a readability limit, not a geometric one, and it is deliberately
    /// stricter: text keeps *fitting* well past the point where it is pleasant
    /// to read, since fitting only requires the type to keep getting smaller.
    /// The geometric ceiling is around 920 characters — 14 px type in fourteen
    /// lines, filling the panel edge to edge — and was rejected as a limit for
    /// exactly that reason.
    ///
    /// 650 was chosen by rendering a ladder of lengths and looking at them: it
    /// sets at 17 px, small against an original's 24 but still comfortable at
    /// print size. For scale, the 168 custom cards in `../bug-vanguards` have a
    /// median of 125 characters and a maximum of 366, so this is roughly 1.8×
    /// the longest card anyone has actually written.
    ///
    /// Size does not fall smoothly with length — 450 and 550 characters both
    /// set at 22 px, then 650 drops to 17 — because the wrap crosses out of the
    /// full-width box into the narrow column between the stat-bubble housings
    /// and loses width. Re-render the ladder before moving this.
    pub ability_chars_max: usize,
    /// Minimum font size flavor text may shrink to when auto-scaling
    pub flavor_size_min: u32,
    /// Name font size: (x_scale, y_scale) in points.
    /// Setting x > y stretches glyphs horizontally to match wider original letterforms.
    pub name_scale: (f32, f32),
    /// Maximum pixel width for the rendered name. If the name is wider at the
    /// default scale it is proportionally scaled down to fit.
    pub name_max_width: f32,
    /// Line-height multiplier for ability text (line_height = font_size × factor).
    /// 1.0 = tight, 1.25 = standard, higher values add more breathing room.
    pub line_height_factor: f32,
    /// Effective height (px) of the centering region for ability text, measured
    /// from box_top. A block no taller than `rules_calibrated_height` is centered
    /// within this region, which puts its middle on y ≈ 701.
    ///
    /// That is where every original sits. Measured off the scans: a one-line
    /// ability spans 688–714, two lines 675–728, three lines 659–741 — same
    /// centre, growing in both directions.
    pub rules_centering_height: f32,
    /// Tallest ability block any original has (px), and so the limit of what the
    /// centering above is calibrated for. Every one of the 25 is exactly 30, 60
    /// or 90 px — one, two or three lines with no paragraph gaps.
    pub rules_calibrated_height: f32,
    /// Stats font size (points)
    pub stats_size: f32,
    /// Letter spacing between the glyphs of a stat value of two or more digits,
    /// in pixels. Negative pulls them together. One-digit values are set at the
    /// font's own spacing: sweeping a tracking knob that applied to every value
    /// peaked flat at 0.0, so the originals' `-4` is spaced as ours is.
    ///
    /// The bubble is a fixed circle and `+12` has to fit in it. What the
    /// originals do is set the glyphs at full size and pull them together:
    /// measured off the scans glyph by glyph (`tests/bubble_geometry.rs`), the
    /// digits in `+10`, `+12` and `+15` are exactly as wide as the digits in
    /// `+4` or `-8`, while the centre-to-centre advance between them is 3–4 px
    /// shorter. Condensing the glyphs instead reaches the same bounding box by
    /// thinning every vertical stem, which the printed cards plainly do not do,
    /// and which F1 cannot see because it scores a binarized mask.
    pub stats_multi_digit_tracking: f32,
    /// Narrower text box used for ability-text lines 4 and beyond, where the
    /// stat-bubble frames on either side reduce the available width.
    /// Derived from template pixel analysis:
    /// left bubble right edge ≈ 143, right bubble left edge ≈ 575.
    pub narrow_text_box: Rect,
    /// Lowest y the ability text may reach when it does not fit the normal box
    /// (px). The parchment panel does not end at `text_box.bottom`: measured off
    /// the template, it runs full width (83–635) to y≈754, narrows to the column
    /// between the stat-bubble housings (144–574), and continues to y≈905 where
    /// the bottom banner cuts in. No original needs that lower strip, so
    /// `text_box` stops short of it and every calibrated constant is measured in
    /// that frame. Overflow text is the only thing allowed down here.
    pub rules_overflow_bottom: f32,
    /// Top of that same region: the first row of parchment below the panel's
    /// inner border.
    pub rules_overflow_top: f32,
    /// Shortest a paragraph's last line may be, as a fraction of the measure.
    ///
    /// Below this the line reads as a stub rather than as the end of a
    /// paragraph — `flash.` alone under a full line is 12% of the measure — and
    /// `wrap_paragraph` narrows the setting until it clears, without changing
    /// the line count. Survey the corpus with `cargo run --release --example
    /// widows` before moving it.
    ///
    /// No original is affected at any value: all 25 carry their printed line
    /// breaks as `\n` or are a single line, so none of them auto-wraps.
    pub widow_min_fraction: f32,
    /// Block height (px) above which ability text counts as overflowing the
    /// normal box, and so may use the lower parchment and a smaller size.
    ///
    /// Slightly taller than `text_box` itself, which is why it is stated rather
    /// than derived: the box was calibrated to where the originals *put* text,
    /// and a block may reach a couple of pixels past it before anything needs to
    /// change. Placement no longer reads this — `rules_block_top` anchors the
    /// top — so it decides only whether a card takes the overflow path.
    pub rules_normal_height: f32,
    /// Maximum number of visible ability-text lines (Tokens entries) accepted.
    pub max_ability_lines: usize,
    /// Exponent applied to glyph coverage before compositing: `a' = a^ink_gain`.
    ///
    /// A clean outline rasterizer lays down noticeably less ink than a printing
    /// press does. Real ink spreads into the paper, so a printed stroke is
    /// fractionally wider than its outline and its edge pixels are darker than
    /// pure area coverage predicts. Values below 1.0 reproduce that spread,
    /// 1.0 disables it. This is not faux-bold: it does not touch the outline,
    /// only how the antialiased edge is weighted, so glyph shapes and advance
    /// widths are untouched.
    pub ink_gain: f32,
    /// Inline mana symbol size, as a fraction of the surrounding font size.
    pub symbol_scale: f32,
    /// Vertical nudge of inline mana symbols, in pixels, positive = down.
    /// Symbols are otherwise centered on the line box.
    pub symbol_y_offset: f32,
    /// Left inset (px) of a mode's text from the left edge of the rules block,
    /// for modal abilities (`Choose one —` followed by bulleted modes). The
    /// bullet sits in this gutter and a mode that wraps hangs under its own
    /// text, not under its bullet.
    ///
    /// Measured at `ability_size`, and scaled with the type when overflowing
    /// text is set smaller — the indent is typographic, not a fixed margin, so
    /// a list at 16 px with a 24 px gutter reads as two loose columns.
    pub mode_indent: f32,
    /// Radius (px) of the disc drawn as a mode bullet, at `ability_size`.
    /// Scaled with the type for the same reason as `mode_indent`.
    pub mode_bullet_radius: f32,
    /// Baseline of the bezel credit line (`Illus. <artist>`), in px.
    ///
    /// A baseline rather than a box centre, for the same reason the card name
    /// has one: the line must sit in the same place whether or not the artist's
    /// name happens to contain a descender, and `Illus. Douglas Shuler` has one
    /// where `Illus. Mark Tedin` does not.
    ///
    /// Fitted against the credit line segmented out of six scans: the best
    /// alignment puts it at 924.5–926.5 with no trend across cards.
    pub credit_baseline: f32,
    /// Horizontal centre of the credit line, in px.
    ///
    /// Not `text_box.center_x()`: the bezel banner is set slightly right of the
    /// panel above it, and the six fitted scans agree on 360.7–362.5 — the same
    /// centre the gem sits on.
    pub credit_center_x: f32,
    /// Credit line font size, in px. Fitted over the six scans as one constant
    /// rather than averaged from per-card optima; mean shape F1 peaks sharply
    /// here (0.61 at 16.0, 0.50 at 15.75, 0.51 at 16.5).
    pub credit_size: f32,
    /// Widest the credit line may be set, in px.
    ///
    /// The banner's interior is 133–578 at the baseline, but it is notched by
    /// the scroll ends at the height the capitals reach, where it narrows to
    /// 150–570. A name too long for that is scaled down rather than run into
    /// the notch, and stops 10 px short of it either side so a long line reads
    /// as set in the banner rather than wedged into it. No original comes close:
    /// the widest, `Illus. Anson Maddocks`, is 161 px.
    pub credit_max_width: f32,
    /// Center of the coloured gem set into the bottom bezel.
    pub gem_center: (f32, f32),
    /// Radius of the gem sphere, in pixels. Recolouring fades out over the
    /// next two pixels, so this is the last fully-affected radius.
    pub gem_radius: f32,
}

impl Layout {
    /// Wrap width for ability lines 1-3 at normal margins.
    pub fn rules_width(&self) -> f32 {
        self.text_box.width() - self.text_padding as f32 * 2.0
    }

    /// Expanded wrap width: halved left/right margins, tried before falling
    /// back to the narrow split when a 4th line would be needed.
    pub fn rules_width_expanded(&self) -> f32 {
        self.text_box.width() - self.text_padding as f32
    }

    /// Wrap width for ability lines 4+ (between the stat-bubble frames).
    pub fn rules_width_narrow(&self) -> f32 {
        self.narrow_text_box.width() - self.text_padding as f32 * 2.0
    }

    /// Height of the parchment available to ability text that has overflowed
    /// the normal box, which reaches below `text_box.bottom`.
    pub fn overflow_height(&self) -> f32 {
        self.rules_overflow_bottom - self.rules_overflow_top
    }
}

/// Default layout calibrated against the 718×1024 reference template.
/// Coordinates for ability text and stats derived from mask measurements.
pub const DEFAULT: Layout = Layout {
    art_box: Rect::new(86, 111, 632, 588),
    name_center: (356, 79),
    text_box: Rect::new(98, 640, 618, 835),
    hand_center: (100, 878),
    life_center: (613, 878),
    text_padding: 4,
    para_gap: 20.0,
    ability_size: 24,
    ability_size_min: 14,
    ability_chars_max: 650,
    flavor_size_min: 14,
    name_scale: (72.5, 56.0),
    name_max_width: 465.0,
    line_height_factor: 1.25,
    rules_centering_height: 122.0,
    rules_calibrated_height: 90.0,
    stats_size: 34.5,
    stats_multi_digit_tracking: -3.75,
    narrow_text_box: Rect::new(144, 640, 574, 835),
    rules_overflow_bottom: 905.0,
    rules_overflow_top: 644.0,
    widow_min_fraction: 0.35,
    rules_normal_height: 197.0,
    max_ability_lines: 8,
    ink_gain: 0.87,
    symbol_scale: 0.95,
    symbol_y_offset: 1.0,
    mode_indent: 20.0,
    mode_bullet_radius: 2.6,
    credit_baseline: 925.5,
    credit_center_x: 361.5,
    credit_size: 16.0,
    credit_max_width: 400.0,
    gem_center: (361.5, 955.5),
    gem_radius: 16.5,
};

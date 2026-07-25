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
    /// Ability text font size (fixed — ability text does not auto-scale)
    pub ability_size: u32,
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
    /// from box_top. The text block is centered within this region:
    ///   offset = max(0, (centering_height - block_h) / 2)
    /// Shorter texts land lower; longer texts land higher — matching original
    /// Vanguard card layouts.
    pub rules_centering_height: f32,
    /// Stats font size (points)
    pub stats_size: f32,
    /// Narrower text box used for ability-text lines 4 and beyond, where the
    /// stat-bubble frames on either side reduce the available width.
    /// Derived from template pixel analysis:
    /// left bubble right edge ≈ 143, right bubble left edge ≈ 575.
    pub narrow_text_box: Rect,
    /// Minimum y coordinate for the top of the ability-text block (px).
    /// Short blocks are never positioned above this line. Must be ≥ text_box top.
    pub rules_min_y: f32,
    /// Maximum number of visible ability-text lines (Tokens entries) accepted.
    pub max_ability_lines: usize,
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

    /// Maximum block height that keeps the block's top at or below `rules_min_y`.
    pub fn pushup_free_height(&self) -> f32 {
        self.text_box.height() - (self.rules_min_y - self.text_box.top as f32)
    }
}

/// Default layout calibrated against the 718×1024 reference template.
/// Coordinates for ability text and stats derived from mask measurements.
pub const DEFAULT: Layout = Layout {
    art_box: Rect::new(86, 111, 632, 588),
    name_center: (359, 79),
    text_box: Rect::new(100, 640, 620, 835),
    hand_center: (100, 879),
    life_center: (613, 879),
    text_padding: 22,
    para_gap: 20.0,
    ability_size: 24,
    flavor_size_min: 14,
    name_scale: (71.0, 57.0),
    name_max_width: 460.0,
    line_height_factor: 1.25,
    rules_centering_height: 126.0,
    stats_size: 30.0,
    narrow_text_box: Rect::new(144, 640, 574, 835),
    rules_min_y: 658.0,
    max_ability_lines: 8,
};

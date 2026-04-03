/// Template layout coordinates for the 718×1024 Vanguard card template.
/// All values are pixel coordinates measured from the reference template.
pub struct Layout {
    /// Artwork transparent region: (left, top, right, bottom)
    pub art_box: (u32, u32, u32, u32),
    /// Name banner center point
    pub name_center: (u32, u32),
    /// Text box interior: (left, top, right, bottom)
    pub text_box: (u32, u32, u32, u32),
    /// Hand stat circle center
    pub hand_center: (u32, u32),
    /// Life stat circle center
    pub life_center: (u32, u32),
    /// Horizontal padding inside the text box
    pub text_padding: u32,
    /// Fixed pixel gap between paragraphs (not scaled with font)
    pub para_gap: f32,
    /// Ability text font size range
    pub ability_size_max: u32,
    pub ability_size_min: u32,
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
}

/// Default layout calibrated against the 718×1024 reference template.
/// Coordinates for ability text and stats derived from mask measurements.
pub const DEFAULT: Layout = Layout {
    art_box: (86, 111, 632, 588),
    name_center: (359, 79),
    text_box: (100, 640, 620, 835),
    hand_center: (100, 879),
    life_center: (613, 879),
    text_padding: 22,
    para_gap: 20.0,
    ability_size_max: 24,
    ability_size_min: 14,
    name_scale: (71.0, 57.0),
    name_max_width: 460.0,
    line_height_factor: 1.25,
    rules_centering_height: 126.0,
    stats_size: 30.0,
};

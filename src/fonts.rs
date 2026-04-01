use crate::bundle;

/// Title font (Fremont Regular).
pub fn name_data() -> &'static [u8] {
    bundle::get("fonts/Fremont-Regular.ttf")
}

/// Body text font (MPlantin).
pub fn body_data() -> &'static [u8] {
    bundle::get("fonts/Mplantin.ttf")
}

/// Bold body text font (MPlantin Bold).
pub fn body_bold_data() -> &'static [u8] {
    bundle::get("fonts/Mplantin-Bold.ttf")
}

/// Embedded card template image.
pub fn template_data() -> &'static [u8] {
    bundle::get("template.png")
}

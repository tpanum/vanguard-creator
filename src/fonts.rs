use ab_glyph::FontRef;
use anyhow::{Context, Result};

use crate::bundle;

/// The embedded card fonts, parsed and ready to render with.
pub struct Fonts {
    /// Title font (Fremont Regular).
    pub name: FontRef<'static>,
    /// Body text font (MPlantin Bold).
    pub body: FontRef<'static>,
}

impl Fonts {
    pub fn load() -> Result<Self> {
        Ok(Fonts {
            name: FontRef::try_from_slice(bundle::font("Fremont-Regular.ttf"))
                .context("parsing embedded title font")?,
            body: FontRef::try_from_slice(bundle::font("Mplantin-Bold.ttf"))
                .context("parsing embedded body font")?,
        })
    }
}

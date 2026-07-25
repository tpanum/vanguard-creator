use ab_glyph::FontRef;
use anyhow::{Context, Result};

use crate::bundle;

/// The embedded card fonts, parsed and ready to render with.
pub struct Fonts {
    /// Title font (Fremont Regular).
    pub name: FontRef<'static>,
    /// Body text font (MPlantin Bold).
    pub body: FontRef<'static>,
    /// Hand/life modifier font.
    ///
    /// The stat bubbles are set in the title face, not the body face. The
    /// printed numerals have the title's tapered, slightly waved minus sign and
    /// its high stroke contrast, where MPlantin Bold has a flat rectangular bar
    /// and an even stroke; scoring letterform shape against the card scans
    /// prefers Fremont too.
    pub stats: FontRef<'static>,
}

impl Fonts {
    pub fn load() -> Result<Self> {
        Ok(Fonts {
            name: FontRef::try_from_slice(bundle::font("Fremont-Regular.ttf"))
                .context("parsing embedded title font")?,
            body: FontRef::try_from_slice(bundle::font("Mplantin-Bold.ttf"))
                .context("parsing embedded body font")?,
            stats: FontRef::try_from_slice(bundle::font("Fremont-Regular.ttf"))
                .context("parsing embedded stats font")?,
        })
    }
}

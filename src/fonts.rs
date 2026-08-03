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
    /// Bezel credit line font (MPlantin Regular).
    ///
    /// The body face, but not the body *weight*: the credit is set lighter than
    /// the rules text above it. Scored against the credit line segmented out of
    /// six scans (`cargo run --release --example illus_font`), MPlantin Regular
    /// reaches 0.636 mean shape F1 where MPlantin Bold reaches 0.605 and
    /// Fremont 0.573 — and Regular at 16 px lays down the same ink volume as
    /// the scans, where Bold lays down a quarter more.
    pub credit: FontRef<'static>,
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
            credit: FontRef::try_from_slice(bundle::font("Mplantin.ttf"))
                .context("parsing embedded credit font")?,
        })
    }
}

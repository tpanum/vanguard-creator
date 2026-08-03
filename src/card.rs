use anyhow::{bail, Context, Result};
use regex::Regex;
use serde::Deserialize;
use std::path::{Path, PathBuf};
use std::str::FromStr;
use std::sync::OnceLock;

use crate::fonts::Fonts;
use crate::gem::Gem;
use crate::layout;
use crate::text;

static STAT_RE: OnceLock<Regex> = OnceLock::new();
fn stat_re() -> &'static Regex {
    // ASCII digits only: `\d` also matches Unicode decimal digits, which the
    // digit count below (and the stats font) would not handle.
    STAT_RE.get_or_init(|| Regex::new(r"^[+-][0-9]+$").unwrap())
}

/// Most digits a hand or life modifier may have.
///
/// The bubbles are circles stamped on the template, and the widest value the
/// originals ever had to fit is two digits — which they only managed by pulling
/// the glyphs together (see `Layout::stats_multi_digit_tracking`). A third digit
/// has nowhere to go: it would either overrun the circle or have to be squeezed
/// past what tightening the spacing can buy. Rather than emit a card that
/// misrepresents what the tool can render, we refuse the value.
const STAT_MAX_DIGITS: usize = 2;

/// Check one stat value, returning a message describing what is wrong with it.
fn stat_error(field: &str, value: &str) -> Option<String> {
    if !stat_re().is_match(value) {
        return Some(format!(
            "invalid '{field}' value: {value:?} (expected +N or -N)"
        ));
    }
    let digits = value.chars().filter(|c| c.is_ascii_digit()).count();
    if digits > STAT_MAX_DIGITS {
        return Some(format!(
            "'{field}' value {value:?} has {digits} digits; the stat bubble holds \
             at most {STAT_MAX_DIGITS}"
        ));
    }
    None
}

#[derive(Debug, Deserialize)]
pub struct CardDef {
    pub name: String,
    pub ability: String,
    pub flavor: Option<String>,
    pub hand: String,
    pub life: String,
    pub artwork: PathBuf,
    /// Colour of the gem in the bottom bezel. Required: it is a property of
    /// the card that nothing else can be derived from, and defaulting it
    /// silently renders the wrong gem rather than saying so.
    pub color: Gem,
    /// The credit line set in the bottom bezel, printed exactly as written —
    /// `Illus. Douglas Shuler`, as the originals do it, is what a card says,
    /// not what the renderer makes of `Douglas Shuler`.
    ///
    /// Optional, where `color` is required, and the difference is what happens
    /// when it is left out. A card with no `color` renders a *wrong* gem —
    /// blue, which is right for a fifth of the set — so silence there is a
    /// silent error. A card with no `artist` renders the bezel the template
    /// already ships with, which is not wrong, only uncredited; and an author
    /// working from a scan or an AI-generated piece may genuinely have nobody
    /// to name. Refusing the card would then block work over a caption.
    pub artist: Option<String>,
}

impl CardDef {
    pub fn load(yaml_path: &Path) -> Result<Self> {
        let text = std::fs::read_to_string(yaml_path)
            .with_context(|| format!("reading {}", yaml_path.display()))?;
        let mut card: Self = serde_yaml::from_str(&text)
            .with_context(|| format!("parsing YAML in {}", yaml_path.display()))?;
        card.artwork = resolve_artwork(yaml_path, &card.artwork);
        card.check_stats()?;
        Ok(card)
    }

    /// Reject a card whose hand or life modifier cannot be set in its bubble.
    /// Rendering is refused outright rather than producing an overflowing card.
    fn check_stats(&self) -> Result<()> {
        for (field, value) in [("hand", &self.hand), ("life", &self.life)] {
            if let Some(msg) = stat_error(field, value) {
                bail!(msg);
            }
        }
        Ok(())
    }
}

/// Resolve an artwork path relative to the YAML file that references it.
/// Absolute paths are returned unchanged.
pub fn resolve_artwork(yaml_path: &Path, artwork: &Path) -> PathBuf {
    if artwork.is_relative() {
        if let Some(parent) = yaml_path.parent() {
            return parent.join(artwork);
        }
    }
    artwork.to_owned()
}

/// Turn a card name into a safe lowercase file stem (e.g. "Sliver Queen,
/// Brood Mother" → "sliver_queen__brood_mother").
pub fn sanitize_filename(name: &str) -> String {
    name.chars()
        .map(|c| {
            if c.is_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect::<String>()
        .trim_matches('_')
        .to_lowercase()
}

#[derive(Debug)]
pub struct ValidationIssue {
    pub path: PathBuf,
    pub message: String,
}

impl std::fmt::Display for ValidationIssue {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.path.display(), self.message)
    }
}

pub fn validate_file(yaml_path: &Path) -> Vec<ValidationIssue> {
    let mut issues = Vec::new();
    let mut issue = |message: String| {
        issues.push(ValidationIssue {
            path: yaml_path.to_owned(),
            message,
        });
    };

    let data = match load_yaml_value(yaml_path) {
        Ok(v) => v,
        Err(e) => {
            issue(format!("{e:#}"));
            return issues;
        }
    };

    for field in &["name", "ability", "hand", "life", "color", "artwork"] {
        if data.get(field).is_none() {
            issue(format!("missing required field '{field}'"));
        }
    }

    for stat in &["hand", "life"] {
        if let Some(val) = data.get(stat).and_then(|v| v.as_str()) {
            if let Some(msg) = stat_error(stat, val) {
                issue(msg);
            }
        }
    }

    if let Some(color) = data.get("color") {
        match color.as_str() {
            Some(s) => {
                if let Err(e) = Gem::from_str(s) {
                    issue(e);
                }
            }
            None => issue("invalid 'color' value: expected a string".to_owned()),
        }
    }

    // `artist` is optional — an uncredited card renders the bezel the template
    // already ships with — but a non-string one is a typo, not an omission.
    if let Some(artist) = data.get("artist") {
        if artist.as_str().is_none() {
            issue("invalid 'artist' value: expected a string".to_owned());
        }
    }

    // Ability text that cannot be set inside the parchment is a defect in the
    // card, so `validate` reports it rather than leaving `create` to be the
    // first thing that says so.
    if let Some(ability) = data.get("ability").and_then(|v| v.as_str()) {
        let flavor = data.get("flavor").and_then(|v| v.as_str());
        match Fonts::load() {
            Ok(fonts) => {
                if let Err(msg) = text::check_rules_fit(
                    ability,
                    flavor,
                    &fonts.body,
                    &fonts.body,
                    &layout::DEFAULT,
                ) {
                    issue(msg);
                }
            }
            Err(e) => issue(format!("cannot check ability text: {e:#}")),
        }
    }

    if let Some(artwork_str) = data.get("artwork").and_then(|v| v.as_str()) {
        let resolved = resolve_artwork(yaml_path, Path::new(artwork_str));
        if !resolved.exists() {
            issue(format!("artwork file not found: {}", resolved.display()));
        }
    }

    issues
}

/// Parse a YAML file into an untyped value (for loose, field-by-field checks).
fn load_yaml_value(yaml_path: &Path) -> Result<serde_yaml::Value> {
    let text = std::fs::read_to_string(yaml_path).context("cannot read file")?;
    serde_yaml::from_str(&text).context("YAML parse error")
}

/// Collect all .yaml files from a list of paths (files and directories).
/// When `recursive` is true, subdirectories are searched depth-first.
pub fn collect_yaml_files(paths: &[PathBuf], recursive: bool) -> Result<Vec<PathBuf>> {
    let mut files = Vec::new();
    for path in paths {
        if path.is_file() {
            files.push(path.clone());
        } else if path.is_dir() {
            collect_yaml_from_dir(path, recursive, &mut files)?;
        } else {
            bail!("path not found: {}", path.display());
        }
    }
    Ok(files)
}

fn collect_yaml_from_dir(dir: &Path, recursive: bool, files: &mut Vec<PathBuf>) -> Result<()> {
    let mut entries: Vec<_> = std::fs::read_dir(dir)
        .with_context(|| format!("reading directory {}", dir.display()))?
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .collect();
    entries.sort();
    for entry in entries {
        if entry.is_dir() && recursive {
            collect_yaml_from_dir(&entry, recursive, files)?;
        } else if entry
            .extension()
            .is_some_and(|ext| ext == "yaml" || ext == "yml")
        {
            files.push(entry);
        }
    }
    Ok(())
}

pub fn list_missing_artwork_cmd(paths: &[PathBuf], recursive: bool) -> Result<()> {
    let files = collect_yaml_files(paths, recursive)?;
    for yaml_path in &files {
        let data = match load_yaml_value(yaml_path) {
            Ok(v) => v,
            Err(e) => {
                eprintln!("warning: skipping {}: {e:#}", yaml_path.display());
                continue;
            }
        };

        let missing = match data.get("artwork").and_then(|v| v.as_str()) {
            None => true,
            Some(artwork_str) => !resolve_artwork(yaml_path, Path::new(artwork_str)).exists(),
        };

        if missing {
            println!("{}", yaml_path.display());
        }
    }
    Ok(())
}

pub fn validate_cmd(paths: &[PathBuf], recursive: bool) -> Result<()> {
    let files = collect_yaml_files(paths, recursive)?;
    let mut any_issues = false;
    for file in &files {
        for issue in validate_file(file) {
            println!("{issue}");
            any_issues = true;
        }
    }
    if any_issues {
        std::process::exit(1);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `color` is required, and `validate` must say so — otherwise the first
    /// sign of it is `create` skipping the card.
    #[test]
    fn validate_reports_a_missing_color() {
        let dir = std::env::temp_dir().join("vgc_validate_color");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("nocolor.yaml");
        std::fs::write(
            &path,
            "name: \"X\"\nability: \"a\"\nhand: \"-1\"\nlife: \"+0\"\nartwork: \"a.png\"\n",
        )
        .unwrap();

        let issues = validate_file(&path);
        assert!(
            issues
                .iter()
                .any(|i| i.message.contains("missing required field 'color'")),
            "got {issues:?}"
        );

        std::fs::remove_file(&path).unwrap();
    }

    #[test]
    fn stat_values_of_one_or_two_digits_are_accepted() {
        for v in ["+0", "-4", "+10", "+15", "-12"] {
            assert_eq!(stat_error("life", v), None, "{v} should be accepted");
        }
    }

    #[test]
    fn stat_values_of_three_or_more_digits_are_rejected() {
        for v in ["+100", "-100", "+123", "-1000"] {
            let msg = stat_error("life", v).unwrap_or_else(|| panic!("{v} should be rejected"));
            assert!(msg.contains("at most 2"), "unexpected message: {msg}");
        }
    }

    #[test]
    fn malformed_stat_values_are_rejected() {
        for v in ["", "4", "++4", "+4x", "+ 4"] {
            assert!(stat_error("hand", v).is_some(), "{v:?} should be rejected");
        }
    }

    #[test]
    fn test_sanitize_filename() {
        assert_eq!(sanitize_filename("Gerrard"), "gerrard");
        assert_eq!(
            sanitize_filename("Sliver Queen, Brood Mother"),
            "sliver_queen__brood_mother"
        );
        assert_eq!(sanitize_filename("Urza's Saga"), "urza_s_saga");
    }
}

use anyhow::{bail, Context, Result};
use regex::Regex;
use serde::Deserialize;
use std::path::{Path, PathBuf};
use std::str::FromStr;
use std::sync::OnceLock;

use crate::gem::GemColor;

static STAT_RE: OnceLock<Regex> = OnceLock::new();
fn stat_re() -> &'static Regex {
    STAT_RE.get_or_init(|| Regex::new(r"^[+-]\d+$").unwrap())
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
    pub color: GemColor,
}

impl CardDef {
    pub fn load(yaml_path: &Path) -> Result<Self> {
        let text = std::fs::read_to_string(yaml_path)
            .with_context(|| format!("reading {}", yaml_path.display()))?;
        let mut card: Self = serde_yaml::from_str(&text)
            .with_context(|| format!("parsing YAML in {}", yaml_path.display()))?;
        card.artwork = resolve_artwork(yaml_path, &card.artwork);
        Ok(card)
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
            if !stat_re().is_match(val) {
                issue(format!(
                    "invalid '{stat}' value: {val:?} (expected +N or -N)"
                ));
            }
        }
    }

    if let Some(color) = data.get("color") {
        match color.as_str() {
            Some(s) => {
                if let Err(e) = GemColor::from_str(s) {
                    issue(e);
                }
            }
            None => issue("invalid 'color' value: expected a string".to_owned()),
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
    fn test_sanitize_filename() {
        assert_eq!(sanitize_filename("Gerrard"), "gerrard");
        assert_eq!(
            sanitize_filename("Sliver Queen, Brood Mother"),
            "sliver_queen__brood_mother"
        );
        assert_eq!(sanitize_filename("Urza's Saga"), "urza_s_saga");
    }
}

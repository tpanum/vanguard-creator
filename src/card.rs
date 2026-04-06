use anyhow::{bail, Context, Result};
use regex::Regex;
use serde::Deserialize;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

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
}

impl CardDef {
    pub fn load(yaml_path: &Path) -> Result<Self> {
        let text = std::fs::read_to_string(yaml_path)
            .with_context(|| format!("reading {}", yaml_path.display()))?;
        let mut card: Self = serde_yaml::from_str(&text)
            .with_context(|| format!("parsing YAML in {}", yaml_path.display()))?;

        // Resolve artwork path relative to YAML file
        if card.artwork.is_relative() {
            if let Some(parent) = yaml_path.parent() {
                card.artwork = parent.join(&card.artwork);
            }
        }
        Ok(card)
    }
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

    let text = match std::fs::read_to_string(yaml_path) {
        Ok(t) => t,
        Err(e) => {
            issues.push(ValidationIssue {
                path: yaml_path.to_owned(),
                message: format!("cannot read file: {e}"),
            });
            return issues;
        }
    };

    let data: serde_yaml::Value = match serde_yaml::from_str(&text) {
        Ok(v) => v,
        Err(e) => {
            issues.push(ValidationIssue {
                path: yaml_path.to_owned(),
                message: format!("YAML parse error: {e}"),
            });
            return issues;
        }
    };

    for field in &["name", "ability", "hand", "life", "artwork"] {
        if data.get(field).is_none() {
            issues.push(ValidationIssue {
                path: yaml_path.to_owned(),
                message: format!("missing required field '{field}'"),
            });
        }
    }

    for stat in &["hand", "life"] {
        if let Some(val) = data.get(stat).and_then(|v| v.as_str()) {
            if !stat_re().is_match(val) {
                issues.push(ValidationIssue {
                    path: yaml_path.to_owned(),
                    message: format!("invalid '{stat}' value: {val:?} (expected +N or -N)"),
                });
            }
        }
    }

    if let Some(artwork_str) = data.get("artwork").and_then(|v| v.as_str()) {
        let artwork = Path::new(artwork_str);
        let resolved = if artwork.is_absolute() {
            artwork.to_owned()
        } else if let Some(parent) = yaml_path.parent() {
            parent.join(artwork)
        } else {
            artwork.to_owned()
        };
        if !resolved.exists() {
            issues.push(ValidationIssue {
                path: yaml_path.to_owned(),
                message: format!("artwork file not found: {}", resolved.display()),
            });
        }
    }

    issues
}

/// Collect all .yaml files from a list of paths (files and directories).
pub fn collect_yaml_files(paths: &[PathBuf]) -> Result<Vec<PathBuf>> {
    let mut files = Vec::new();
    for path in paths {
        if path.is_file() {
            files.push(path.clone());
        } else if path.is_dir() {
            let mut entries: Vec<_> = std::fs::read_dir(path)
                .with_context(|| format!("reading directory {}", path.display()))?
                .filter_map(|e| e.ok())
                .map(|e| e.path())
                .filter(|p| {
                    p.extension()
                        .is_some_and(|ext| ext == "yaml" || ext == "yml")
                })
                .collect();
            entries.sort();
            files.extend(entries);
        } else {
            bail!("path not found: {}", path.display());
        }
    }
    Ok(files)
}

pub fn list_missing_artwork_cmd(paths: &[PathBuf]) -> Result<()> {
    let files = collect_yaml_files(paths)?;
    for yaml_path in &files {
        let text = match std::fs::read_to_string(yaml_path) {
            Ok(t) => t,
            Err(e) => {
                eprintln!("warning: skipping {}: {e}", yaml_path.display());
                continue;
            }
        };
        let data: serde_yaml::Value = match serde_yaml::from_str(&text) {
            Ok(v) => v,
            Err(e) => {
                eprintln!(
                    "warning: skipping {}: YAML parse error: {e}",
                    yaml_path.display()
                );
                continue;
            }
        };

        let missing = match data.get("artwork").and_then(|v| v.as_str()) {
            None => true,
            Some(artwork_str) => {
                let artwork = Path::new(artwork_str);
                let resolved = if artwork.is_absolute() {
                    artwork.to_owned()
                } else if let Some(parent) = yaml_path.parent() {
                    parent.join(artwork)
                } else {
                    artwork.to_owned()
                };
                !resolved.exists()
            }
        };

        if missing {
            println!("{}", yaml_path.display());
        }
    }
    Ok(())
}

pub fn validate_cmd(paths: &[PathBuf]) -> Result<()> {
    let files = collect_yaml_files(paths)?;
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

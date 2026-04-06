use anyhow::{Context, Result};
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use crate::card::collect_yaml_files;

fn sanitize_filename(name: &str) -> String {
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

struct SyncEntry {
    yaml_path: PathBuf,
    new_yaml_path: Option<PathBuf>,
    artwork_abs: Option<PathBuf>,
    new_artwork_abs: Option<PathBuf>,
    /// Raw artwork field value as written in the YAML file.
    artwork_field: Option<String>,
    /// New artwork field value to write into the YAML file.
    new_artwork_field: Option<String>,
}

pub fn run(paths: &[PathBuf], yes: bool) -> Result<()> {
    let yaml_files = collect_yaml_files(paths)?;
    if yaml_files.is_empty() {
        println!("No YAML card files found.");
        return Ok(());
    }

    let mut entries: Vec<SyncEntry> = Vec::new();

    for yaml_path in &yaml_files {
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

        let name = match data.get("name").and_then(|v| v.as_str()) {
            Some(n) => n.to_string(),
            None => {
                eprintln!(
                    "warning: skipping {}: missing 'name' field",
                    yaml_path.display()
                );
                continue;
            }
        };

        let expected_stem = sanitize_filename(&name);
        let current_stem = yaml_path.file_stem().and_then(|s| s.to_str()).unwrap_or("");

        let new_yaml_path = if current_stem != expected_stem {
            Some(yaml_path.with_file_name(format!("{expected_stem}.yaml")))
        } else {
            None
        };

        // The yaml stem after the potential rename determines the expected artwork stem.
        let effective_stem = expected_stem.clone();

        let artwork_field = data
            .get("artwork")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());

        let (artwork_abs, new_artwork_abs, new_artwork_field) = if let Some(ref af) = artwork_field
        {
            let artwork_rel = Path::new(af);
            let resolved = if artwork_rel.is_absolute() {
                artwork_rel.to_owned()
            } else if let Some(parent) = yaml_path.parent() {
                parent.join(artwork_rel)
            } else {
                artwork_rel.to_owned()
            };

            let current_art_stem = resolved.file_stem().and_then(|s| s.to_str()).unwrap_or("");

            if current_art_stem != effective_stem {
                let ext = resolved.extension().and_then(|e| e.to_str()).unwrap_or("");
                let new_art_name = if ext.is_empty() {
                    effective_stem.clone()
                } else {
                    format!("{effective_stem}.{ext}")
                };

                let new_resolved = resolved.with_file_name(&new_art_name);

                // Build the new relative (or absolute) field value, preserving any
                // directory component from the original path.
                let new_field = if artwork_rel.is_relative() {
                    let dir = artwork_rel.parent().filter(|p| !p.as_os_str().is_empty());
                    match dir {
                        Some(d) => d.join(&new_art_name).to_string_lossy().into_owned(),
                        None => new_art_name,
                    }
                } else {
                    new_resolved.to_string_lossy().into_owned()
                };

                (Some(resolved), Some(new_resolved), Some(new_field))
            } else {
                (Some(resolved), None, None)
            }
        } else {
            (None, None, None)
        };

        entries.push(SyncEntry {
            yaml_path: yaml_path.clone(),
            new_yaml_path,
            artwork_abs,
            new_artwork_abs,
            artwork_field,
            new_artwork_field,
        });
    }

    let yaml_rename_count = entries.iter().filter(|e| e.new_yaml_path.is_some()).count();
    let art_rename_count = entries
        .iter()
        .filter(|e| e.new_artwork_abs.is_some())
        .count();

    if yaml_rename_count == 0 && art_rename_count == 0 {
        println!("Everything is already in sync.");
        return Ok(());
    }

    // Print the plan.
    if yaml_rename_count > 0 {
        println!("YAML renames ({yaml_rename_count}):");
        for e in entries.iter().filter(|e| e.new_yaml_path.is_some()) {
            println!(
                "  {} -> {}",
                e.yaml_path.display(),
                e.new_yaml_path.as_ref().unwrap().display()
            );
        }
    }

    if art_rename_count > 0 {
        println!("Artwork renames ({art_rename_count}):");
        for e in entries.iter().filter(|e| e.new_artwork_abs.is_some()) {
            println!(
                "  {} -> {}",
                e.artwork_abs.as_ref().unwrap().display(),
                e.new_artwork_abs.as_ref().unwrap().display()
            );
        }
    }

    println!();

    // Phase 1: YAML renames.
    let mut yaml_renames_done = false;
    if yaml_rename_count > 0 {
        let do_rename = yes || prompt_confirm("Rename YAML files?")?;
        if do_rename {
            for e in entries.iter().filter(|e| e.new_yaml_path.is_some()) {
                let new_yaml = e.new_yaml_path.as_ref().unwrap();
                if !confirm_overwrite(new_yaml)? {
                    println!("Skipping: {}", e.yaml_path.display());
                    continue;
                }
                std::fs::rename(&e.yaml_path, new_yaml).with_context(|| {
                    format!(
                        "renaming {} to {}",
                        e.yaml_path.display(),
                        new_yaml.display()
                    )
                })?;
                println!(
                    "Renamed: {} -> {}",
                    e.yaml_path.display(),
                    new_yaml.display()
                );
            }
            yaml_renames_done = true;
        } else {
            println!("Skipping YAML renames.");
        }
    }

    // Phase 2: Artwork renames + YAML field updates.
    if art_rename_count > 0 {
        let do_rename =
            yes || prompt_confirm("Rename artwork files (and update YAML artwork fields)?")?;
        if do_rename {
            for e in entries.iter().filter(|e| e.new_artwork_abs.is_some()) {
                let old_art = e.artwork_abs.as_ref().unwrap();
                let new_art = e.new_artwork_abs.as_ref().unwrap();

                if !old_art.exists() {
                    eprintln!(
                        "warning: artwork not found, skipping rename: {}",
                        old_art.display()
                    );
                    continue;
                }

                if !confirm_overwrite(new_art)? {
                    println!("Skipping: {}", old_art.display());
                    continue;
                }

                std::fs::rename(old_art, new_art).with_context(|| {
                    format!("renaming {} to {}", old_art.display(), new_art.display())
                })?;
                println!("Renamed: {} -> {}", old_art.display(), new_art.display());

                // Update the artwork field inside the YAML file (which may have been renamed).
                let effective_yaml = if yaml_renames_done {
                    e.new_yaml_path.as_ref().unwrap_or(&e.yaml_path)
                } else {
                    &e.yaml_path
                };

                update_artwork_field(
                    effective_yaml,
                    e.artwork_field.as_deref().unwrap(),
                    e.new_artwork_field.as_deref().unwrap(),
                )?;
            }
        } else {
            println!("Skipping artwork renames.");
        }
    }

    Ok(())
}

fn prompt_confirm(question: &str) -> Result<bool> {
    print!("{question} [y/N] ");
    io::stdout().flush()?;
    let mut input = String::new();
    io::stdin().read_line(&mut input)?;
    Ok(matches!(input.trim().to_lowercase().as_str(), "y" | "yes"))
}

/// If `path` already exists on disk, prompt the user whether to overwrite it.
/// Returns `true` if the rename should proceed (either no conflict, or user confirmed).
/// This prompt is always shown regardless of the `-y` flag.
fn confirm_overwrite(path: &Path) -> Result<bool> {
    if !path.exists() {
        return Ok(true);
    }
    print!("  '{}' already exists — overwrite? [y/N] ", path.display());
    io::stdout().flush()?;
    let mut input = String::new();
    io::stdin().read_line(&mut input)?;
    Ok(matches!(input.trim().to_lowercase().as_str(), "y" | "yes"))
}

fn update_artwork_field(yaml_path: &Path, old_value: &str, new_value: &str) -> Result<()> {
    let text = std::fs::read_to_string(yaml_path)
        .with_context(|| format!("reading {}", yaml_path.display()))?;

    let updated = replace_artwork_value(&text, old_value, new_value);

    std::fs::write(yaml_path, updated)
        .with_context(|| format!("writing {}", yaml_path.display()))?;

    println!("Updated artwork field in {}", yaml_path.display());
    Ok(())
}

/// Replace the artwork field value in raw YAML text, preserving all other content.
fn replace_artwork_value(text: &str, old_value: &str, new_value: &str) -> String {
    let mut result = String::with_capacity(text.len() + new_value.len());
    // split_inclusive preserves the line endings so the file is written back unchanged.
    for line in text.split_inclusive('\n') {
        let line_body = line.trim_end_matches('\n').trim_end_matches('\r');
        let line_ending = &line[line_body.len()..];

        if let Some(rest) = line_body.strip_prefix("artwork:") {
            let value_part = rest.trim();
            let quoted_old = format!("\"{old_value}\"");
            if value_part == quoted_old {
                result.push_str(&format!("artwork: \"{new_value}\"{line_ending}"));
                continue;
            }
            if value_part == old_value {
                result.push_str(&format!("artwork: {new_value}{line_ending}"));
                continue;
            }
        }

        result.push_str(line);
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sanitize_filename() {
        assert_eq!(sanitize_filename("Gerrard"), "gerrard");
        assert_eq!(
            sanitize_filename("Sliver Queen, Brood Mother"),
            "sliver_queen__brood_mother"
        );
        assert_eq!(sanitize_filename("Urza's Saga"), "urza_s_saga");
    }

    #[test]
    fn test_replace_artwork_value_quoted() {
        let yaml = "name: \"Foo\"\nartwork: \"old/path.png\"\nhand: \"+1\"\n";
        let result = replace_artwork_value(yaml, "old/path.png", "new/path.png");
        assert_eq!(
            result,
            "name: \"Foo\"\nartwork: \"new/path.png\"\nhand: \"+1\"\n"
        );
    }

    #[test]
    fn test_replace_artwork_value_unquoted() {
        let yaml = "name: Foo\nartwork: old/path.png\nhand: +1\n";
        let result = replace_artwork_value(yaml, "old/path.png", "new/path.png");
        assert_eq!(result, "name: Foo\nartwork: new/path.png\nhand: +1\n");
    }

    #[test]
    fn test_replace_artwork_value_no_trailing_newline() {
        let yaml = "artwork: old.png";
        let result = replace_artwork_value(yaml, "old.png", "new.png");
        assert_eq!(result, "artwork: new.png");
    }
}

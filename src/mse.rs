use anyhow::{Context, Result};
use regex::Regex;
use std::collections::HashMap;
use std::io::Read;
use std::path::Path;
use std::sync::OnceLock;

fn sym_auto_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"<sym-auto>(.*?)</sym-auto>").unwrap())
}

fn sym_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"<sym>(.*?)</sym>").unwrap())
}

fn flavor_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"<i-flavor>.*?</i-flavor>").unwrap())
}

fn tag_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"<[^>]+>").unwrap())
}

fn whitespace_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"[ \t]+").unwrap())
}

/// Parse an MSE set file text into a list of card attribute maps.
fn parse_set_text(text: &str) -> Vec<HashMap<String, String>> {
    let mut cards: Vec<HashMap<String, String>> = Vec::new();
    let mut current: Option<HashMap<String, String>> = None;
    let mut current_key: Option<String> = None;

    for line in text.lines() {
        if line == "card:" {
            if let Some(card) = current.take() {
                cards.push(card);
            }
            current = Some(HashMap::new());
            current_key = None;
            continue;
        }

        let Some(card) = current.as_mut() else {
            continue;
        };

        if line.starts_with("\t\t") {
            // Continuation line — append to current key value
            if let Some(key) = &current_key {
                let val = card.entry(key.clone()).or_default();
                val.push('\n');
                val.push_str(line.trim());
            }
        } else if line.starts_with('\t') {
            let stripped = line.trim();
            if let Some(colon_pos) = stripped.find(": ") {
                let key = stripped[..colon_pos].to_string();
                let val = stripped[colon_pos + 2..].to_string();
                card.insert(key.clone(), val);
                current_key = Some(key);
            } else if let Some(key) = stripped.strip_suffix(':') {
                card.insert(key.to_string(), String::new());
                current_key = Some(key.to_string());
            }
        }
    }

    if let Some(card) = current {
        cards.push(card);
    }

    cards
}

/// Expand a mana string like "2G" into "{2}{G}".
fn expand_mana(raw: &str) -> String {
    let mut result = String::new();
    let mut i = 0;
    let chars: Vec<char> = raw.chars().collect();
    while i < chars.len() {
        if chars[i].is_ascii_digit() {
            let mut num = String::new();
            while i < chars.len() && chars[i].is_ascii_digit() {
                num.push(chars[i]);
                i += 1;
            }
            result.push('{');
            result.push_str(&num);
            result.push('}');
        } else {
            result.push('{');
            result.push(chars[i]);
            result.push('}');
            i += 1;
        }
    }
    result
}

/// Convert MSE markup to clean rules text.
fn strip_mse_tags(text: &str) -> String {
    // Remove flavor text
    let text = flavor_re().replace_all(text, "");
    // Expand mana symbols
    let text = sym_auto_re().replace_all(&text, |caps: &regex::Captures| {
        expand_mana(&caps[1])
    });
    let text = sym_re().replace_all(&text, |caps: &regex::Captures| {
        format!("{{{}}}", &caps[1])
    });
    // Strip remaining tags
    let text = tag_re().replace_all(&text, "");
    // Normalize whitespace
    let text = whitespace_re().replace_all(&text, " ");
    let cleaned: String = text
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .collect::<Vec<_>>()
        .join("\n");
    cleaned
}

fn sanitize_filename(name: &str) -> String {
    name.chars()
        .filter(|c| c.is_alphanumeric() || *c == ' ' || *c == '-' || *c == '_')
        .collect::<String>()
        .trim()
        .to_string()
}

/// Write a card's YAML file.
fn write_yaml(
    path: &Path,
    name: &str,
    ability: &str,
    hand: &str,
    life: &str,
    artwork_rel: &str,
) -> Result<()> {
    // Manual YAML serialization to maintain field order per SPECS
    let content = format!(
        "name: {}\nability: |-\n{}\nhand: \"{}\"\nlife: \"{}\"\nartwork: {}\n",
        yaml_quote(name),
        ability
            .lines()
            .map(|l| format!("  {l}"))
            .collect::<Vec<_>>()
            .join("\n"),
        hand,
        life,
        yaml_quote(artwork_rel),
    );
    std::fs::write(path, content)
        .with_context(|| format!("writing {}", path.display()))
}

fn yaml_quote(s: &str) -> String {
    if s.contains('"') || s.contains('\n') || s.contains(':') || s.contains('#') {
        format!("\"{}\"", s.replace('"', "\\\""))
    } else {
        s.to_string()
    }
}

pub fn run(mse_path: &Path, out_dir: &Path, artwork_subdir: &str, overwrite: bool) -> Result<()> {
    let artwork_dir = out_dir.join(artwork_subdir);
    std::fs::create_dir_all(&artwork_dir)
        .with_context(|| format!("creating {}", artwork_dir.display()))?;

    let file = std::fs::File::open(mse_path)
        .with_context(|| format!("opening {}", mse_path.display()))?;
    let mut zip = zip::ZipArchive::new(file)
        .with_context(|| format!("reading ZIP archive {}", mse_path.display()))?;

    // Read set file
    let set_text = {
        let mut entry = zip
            .by_name("set")
            .context("'set' file not found in MSE archive")?;
        let mut text = String::new();
        entry.read_to_string(&mut text)?;
        text
    };

    let cards = parse_set_text(&set_text);
    println!("Found {} cards", cards.len());

    let mut written = 0;
    for card in &cards {
        let name = match card.get("name").map(|s| s.trim()) {
            Some(n) if !n.is_empty() => n.to_string(),
            _ => continue,
        };

        let safe_name = sanitize_filename(&name);
        let yaml_path = out_dir.join(format!("{safe_name}.yaml"));

        if yaml_path.exists() && !overwrite {
            eprintln!("skipping {}: already exists (use --overwrite)", yaml_path.display());
            continue;
        }

        // Extract artwork
        let artwork_filename = format!("{safe_name}.png");
        let artwork_path = artwork_dir.join(&artwork_filename);
        let artwork_rel = format!("{artwork_subdir}/{artwork_filename}");

        if let Some(image_ref) = card.get("image") {
            let image_ref = image_ref.trim();
            if zip.by_name(image_ref).is_ok() {
                let mut entry = zip.by_name(image_ref)?;
                let mut buf = Vec::new();
                entry.read_to_end(&mut buf)?;
                std::fs::write(&artwork_path, &buf)
                    .with_context(|| format!("writing artwork {}", artwork_path.display()))?;
            }
        }

        let ability = strip_mse_tags(card.get("rule_text").map(String::as_str).unwrap_or(""));
        let hand = card.get("handmod").map(String::as_str).unwrap_or("+0");
        let life = card.get("lifemod").map(String::as_str).unwrap_or("+0");

        write_yaml(&yaml_path, &name, &ability, hand, life, &artwork_rel)?;
        println!("  {} -> {}", name, yaml_path.display());
        written += 1;
    }

    println!("\nDone. {written} cards written to {}", out_dir.display());
    Ok(())
}

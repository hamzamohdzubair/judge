use anyhow::{bail, Result};
use sha2::{Digest, Sha256};
use std::path::PathBuf;

use crate::models::Question;

pub fn qb_dir() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".judge")
        .join("qb")
}

pub fn topic_path(slug: &str) -> PathBuf {
    qb_dir().join(format!("{}.md", slug))
}

/// Parse a question bank markdown file into a list of Questions.
/// Handles the format:
///   ## [N] Question text
///   - 1: keyword
///   - 3: keyword
pub fn parse_topic_file(slug: &str, content: &str) -> Vec<Question> {
    let mut questions: Vec<Question> = Vec::new();
    let mut cur_level: Option<u8> = None;
    let mut cur_text = String::new();
    let mut cur_keywords: [Vec<String>; 4] = Default::default();
    let mut cur_ai = false;

    for raw_line in content.lines() {
        let line = raw_line.trim();

        if let Some(rest) = line.strip_prefix("## [") {
            // Save previous question if any
            if let Some(level) = cur_level.take() {
                questions.push(make_question(slug, level, &cur_text, cur_keywords, cur_ai));
                cur_keywords = Default::default();
                cur_text.clear();
                cur_ai = false;
            }
            // Parse "N] [AI] question text" or "N] question text"
            if let Some(bracket_end) = rest.find(']') {
                let level_str = &rest[..bracket_end];
                let after = rest[bracket_end + 1..].trim();
                let (ai, text) = if after.starts_with("[AI] ") {
                    (true, after[5..].to_string())
                } else {
                    (false, after.to_string())
                };
                if let Ok(level) = level_str.parse::<u8>() {
                    if (1..=4).contains(&level) && !text.is_empty() {
                        cur_level = Some(level);
                        cur_text = text;
                        cur_ai = ai;
                    }
                }
            }
        } else if line.starts_with("- ") && cur_level.is_some() {
            // Parse "- N: keyword text"
            let rest = &line[2..];
            if let Some(colon_pos) = rest.find(':') {
                let level_str = rest[..colon_pos].trim();
                let keyword = rest[colon_pos + 1..].trim().to_string();
                if let Ok(kw_level) = level_str.parse::<u8>() {
                    if (1..=4).contains(&kw_level) && !keyword.is_empty() {
                        cur_keywords[kw_level as usize - 1].push(keyword);
                    }
                }
            }
        }
    }

    // Flush last question
    if let Some(level) = cur_level {
        questions.push(make_question(slug, level, &cur_text, cur_keywords, cur_ai));
    }

    questions
}

fn make_question(topic: &str, level: u8, text: &str, keywords: [Vec<String>; 4], ai_generated: bool) -> Question {
    let mut hasher = Sha256::new();
    hasher.update(topic.as_bytes());
    hasher.update(b"::");
    hasher.update(text.as_bytes());
    let hash = hasher.finalize();
    let id = hex::encode(&hash[..8]); // 16 hex chars

    Question { id, topic: topic.to_string(), level, text: text.to_string(), keywords, ai_generated }
}

/// Load and parse a question bank file for a topic slug.
/// Returns empty vec (with warning) if file doesn't exist.
pub fn load_topic(slug: &str) -> Vec<Question> {
    let path = topic_path(slug);
    match std::fs::read_to_string(&path) {
        Ok(content) => parse_topic_file(slug, &content),
        Err(_) => {
            eprintln!("  warning: no question bank found for '{}' ({})", slug, path.display());
            vec![]
        }
    }
}

pub fn slugify(s: &str) -> String {
    s.to_lowercase()
        .replace(' ', "-")
        .replace('_', "-")
        .chars()
        .filter(|c| c.is_alphanumeric() || *c == '-')
        .collect()
}

/// Write generated markdown to the question bank file.
/// If the file exists and append=true, strips the # heading line and appends.
/// If the file exists and append=false, prompts the user interactively.
/// Returns Err if user aborts.
pub fn write_qb(slug: &str, content: &str, append: bool) -> Result<()> {
    let path = topic_path(slug);
    std::fs::create_dir_all(path.parent().unwrap())?;

    if path.exists() {
        if !append {
            eprint!(
                "File '{}' already exists. [a]ppend / [A]bort: ",
                path.display()
            );
            let mut input = String::new();
            std::io::stdin().read_line(&mut input)?;
            if input.trim() != "a" {
                bail!("Aborted.");
            }
        }
        // Append: strip the # heading line from the new content
        let body = content
            .lines()
            .skip_while(|l| l.trim().starts_with('#') || l.trim().is_empty())
            .collect::<Vec<_>>()
            .join("\n");
        let mut file = std::fs::OpenOptions::new().append(true).open(&path)?;
        use std::io::Write;
        writeln!(file, "\n{}", body)?;
        println!("Appended to {}", path.display());
    } else {
        std::fs::write(&path, content)?;
        println!("Created {}", path.display());
    }

    Ok(())
}

/// Count questions at each level in generated markdown.
pub fn count_levels(content: &str) -> [usize; 4] {
    let mut counts = [0usize; 4];
    for line in content.lines() {
        if let Some(rest) = line.trim().strip_prefix("## [") {
            if let Some(bracket_end) = rest.find(']') {
                if let Ok(level) = rest[..bracket_end].parse::<u8>() {
                    if (1..=4).contains(&level) {
                        counts[level as usize - 1] += 1;
                    }
                }
            }
        }
    }
    counts
}

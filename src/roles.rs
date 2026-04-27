use anyhow::{Context, Result};
use std::path::PathBuf;

use crate::models::TopicData;
use crate::qb;

pub fn roles_dir() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".judge")
        .join("roles")
}

/// Load topic slugs from ~/.judge/roles/<role-slug>.md
pub fn load_role_topics(role_slug: &str) -> Result<Vec<String>> {
    let path = roles_dir().join(format!("{}.md", role_slug));
    let content = std::fs::read_to_string(&path)
        .with_context(|| format!("Role file not found: {}\n  Create it at: {}", role_slug, path.display()))?;

    let topics: Vec<String> = content
        .lines()
        .map(|l| l.trim())
        .filter(|l| l.starts_with("- "))
        .map(|l| l[2..].trim().to_string())
        .filter(|l| !l.is_empty())
        .collect();

    if topics.is_empty() {
        anyhow::bail!(
            "No topics found in role file: {}\n  Add lines like '- nlp' to define topics",
            path.display()
        );
    }

    Ok(topics)
}

/// Load all topic question banks for a role, returning TopicData for each.
pub fn load_topics_for_role(role_slug: &str) -> Result<Vec<TopicData>> {
    let topic_slugs = load_role_topics(role_slug)?;
    let topics = topic_slugs
        .into_iter()
        .map(|slug| {
            let questions = qb::load_topic(&slug);
            TopicData { name: slug, questions }
        })
        .collect();
    Ok(topics)
}

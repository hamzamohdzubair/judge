use crate::models::Interview;
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;

#[derive(Default, Serialize, Deserialize)]
pub struct Store {
    pub interviews: HashMap<String, Interview>,
}

impl Store {
    pub fn data_path() -> PathBuf {
        dirs::data_local_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("judge")
            .join("interviews.json")
    }

    pub fn load() -> Result<Self> {
        let path = Self::data_path();
        if !path.exists() {
            return Ok(Self::default());
        }
        let data = fs::read_to_string(&path)
            .with_context(|| format!("Failed to read {}", path.display()))?;
        serde_json::from_str(&data).context("Failed to parse interviews data")
    }

    pub fn save(&self) -> Result<()> {
        let path = Self::data_path();
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let data = serde_json::to_string_pretty(self)?;
        fs::write(&path, data)
            .with_context(|| format!("Failed to write {}", path.display()))
    }

    pub fn find_by_prefix(&self, prefix: &str) -> Option<&Interview> {
        if let Some(i) = self.interviews.get(prefix) {
            return Some(i);
        }
        let matches: Vec<&Interview> = self
            .interviews
            .values()
            .filter(|i| i.id.starts_with(prefix))
            .collect();
        if matches.len() == 1 { Some(matches[0]) } else { None }
    }

    pub fn find_id_by_prefix(&self, prefix: &str) -> Option<String> {
        if self.interviews.contains_key(prefix) {
            return Some(prefix.to_string());
        }
        let keys: Vec<String> = self
            .interviews
            .keys()
            .filter(|k| k.starts_with(prefix))
            .cloned()
            .collect();
        if keys.len() == 1 { Some(keys.into_iter().next().unwrap()) } else { None }
    }
}

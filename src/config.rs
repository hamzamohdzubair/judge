use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::io::{IsTerminal, Write};
use std::path::PathBuf;

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct Config {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub interviewer: Option<String>,
}

pub fn config_path() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".judge")
        .join("config.toml")
}

pub fn load() -> Result<Config> {
    let path = config_path();
    if !path.exists() {
        return Ok(Config::default());
    }
    let s = std::fs::read_to_string(&path)
        .with_context(|| format!("Failed to read {}", path.display()))?;
    Ok(toml::from_str(&s).unwrap_or_default())
}

pub fn save(cfg: &Config) -> Result<()> {
    let path = config_path();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let s = toml::to_string_pretty(cfg)?;
    std::fs::write(&path, s)?;
    Ok(())
}

fn nonempty(v: &Option<String>) -> Option<&str> {
    v.as_deref().map(str::trim).filter(|s| !s.is_empty())
}

/// On first run, interactively ask for the interviewer's name and save to
/// config. No-op if already set, env var is set, or stdin is not a TTY.
pub fn ensure_first_run_setup() -> Result<()> {
    // Env var takes precedence and is not persisted.
    if let Ok(v) = std::env::var("JUDGE_INTERVIEWER") {
        if !v.trim().is_empty() {
            return Ok(());
        }
    }

    let mut cfg = load()?;
    if nonempty(&cfg.interviewer).is_some() {
        return Ok(());
    }

    if !std::io::stdin().is_terminal() {
        // Non-interactive: skip silently; pdf will fall back to a default.
        return Ok(());
    }

    eprintln!("Looks like this is your first run of judge — let's set you up.");
    eprint!("Enter your name (the interviewer): ");
    std::io::stderr().flush()?;

    let mut line = String::new();
    std::io::stdin().read_line(&mut line)?;
    let name = line.trim().to_string();
    if name.is_empty() {
        anyhow::bail!("Interviewer name cannot be empty");
    }

    cfg.interviewer = Some(name);
    save(&cfg)?;

    eprintln!("Saved to {}", config_path().display());
    eprintln!("(Override anytime via the JUDGE_INTERVIEWER env var.)");
    Ok(())
}

/// Returns the interviewer name: env var → config file → "Interviewer".
pub fn interviewer_name() -> String {
    if let Ok(v) = std::env::var("JUDGE_INTERVIEWER") {
        let t = v.trim();
        if !t.is_empty() {
            return t.to_string();
        }
    }
    if let Ok(cfg) = load() {
        if let Some(n) = nonempty(&cfg.interviewer) {
            return n.to_string();
        }
    }
    "Interviewer".to_string()
}

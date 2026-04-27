use anyhow::{bail, Result};
use clap::{Parser, Subcommand};
use std::path::PathBuf;

mod config;
mod db;
mod export;
mod llm;
mod models;
mod pdf;
mod qb;
mod roles;
mod tui;

use db::Db;
use export::{ExportData, to_csv, to_html, to_json, write_output, write_output_bytes};

#[derive(Parser)]
#[command(
    name = "judge",
    version,
    about = "TUI interview evaluation tool with question banks and live scoring"
)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Start a new interview or re-open an existing one
    Start {
        /// Candidate ID to re-open (omit to create new)
        id: Option<i64>,

        /// Candidate's full name (required for new)
        #[arg(long)]
        name: Option<String>,

        /// Role slug or name, e.g. "data science" (required for new)
        #[arg(long)]
        role: Option<String>,
    },

    /// List all candidates
    #[command(name = "ls")]
    List,

    /// Export interview results
    Export {
        /// Format: json, csv, html, pdf
        format: String,

        /// Candidate ID
        id: i64,

        /// Write to this file (default: <id>.<ext>)
        #[arg(short, long)]
        output: Option<PathBuf>,

        /// Copy to clipboard instead of writing a file
        #[arg(short = 'c', long)]
        clipboard: bool,
    },

    /// Question bank commands
    Qb {
        #[command(subcommand)]
        command: QbCommands,
    },
}

#[derive(Subcommand)]
enum QbCommands {
    /// Generate questions for a topic using the Groq LLM (requires GROQ_API_KEY)
    Gen {
        /// Topic name, e.g. "nlp" or "machine learning"
        topic: String,

        /// Comma-separated question counts per level: l1,l2,l3,l4
        #[arg(long, default_value = "3,3,2,2")]
        num: String,

        /// Append to existing file without prompting
        #[arg(long = "app")]
        append: bool,
    },
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    config::ensure_first_run_setup()?;

    match cli.command {
        Commands::Start { id, name, role } => cmd_start(id, name, role),
        Commands::List => cmd_list(),
        Commands::Export { format, id, output, clipboard } => {
            cmd_export(&format, id, output.as_ref(), clipboard)
        }
        Commands::Qb { command: QbCommands::Gen { topic, num, append } } => {
            cmd_qb_gen(&topic, &num, append)
        }
    }
}

// ── Commands ──────────────────────────────────────────────────────────────────

fn cmd_start(id: Option<i64>, name: Option<String>, role: Option<String>) -> Result<()> {
    let db = Db::open()?;

    let (candidate, topics, responses) = match id {
        Some(cid) if name.is_none() => {
            // Resume existing
            let c = db.get_candidate(cid)?
                .ok_or_else(|| anyhow::anyhow!("No candidate found with ID {}", cid))?;
            let role_slug = qb::slugify(&c.role);
            let topics = roles::load_topics_for_role(&role_slug)?;
            let responses = db.load_responses(c.id)?;
            println!("Resuming interview for {} ({})", c.name, c.id);
            (c, topics, responses)
        }
        _ => {
            // New candidate
            let cand_name = name.ok_or_else(|| anyhow::anyhow!("--name is required for a new interview"))?;
            let role_raw = role.ok_or_else(|| anyhow::anyhow!("--role is required for a new interview"))?;
            let role_slug = qb::slugify(&role_raw);

            let topics = roles::load_topics_for_role(&role_slug)?;
            let total_q: usize = topics.iter().map(|t| t.questions.len()).sum();

            let candidate = db.create_candidate(cand_name, role_slug.clone())?;

            println!("Started interview: {} ({})", candidate.name, candidate.id);
            println!("Role: {}  ·  {} topics  ·  {} questions", role_slug, topics.len(), total_q);

            (candidate, topics, std::collections::HashMap::new())
        }
    };

    tui::run(db, candidate, topics, responses)
}

fn cmd_list() -> Result<()> {
    let db = Db::open()?;
    let candidates = db.list_candidates()?;

    if candidates.is_empty() {
        println!("No candidates yet. Run: judge start --name 'Name' --role 'role'");
        return Ok(());
    }

    println!("{:<5} {:<22} {:<20} {:<12} {}",
        "ID", "Name", "Role", "Answered", "Date");
    println!("{}", "─".repeat(67));

    for c in &candidates {
        let role_slug = qb::slugify(&c.role);
        let topics = roles::load_topics_for_role(&role_slug).unwrap_or_default();
        let responses = db.load_responses(c.id).unwrap_or_default();

        let total_q: usize = topics.iter().map(|t| t.questions.len()).sum();
        let answered: usize = topics.iter().map(|t| t.answered(&responses)).sum();
        let score: u32 = topics.iter().map(|t| t.score(&responses)).sum();
        let max_score: u32 = topics.iter().map(|t| t.max_score()).sum();
        let date = c.created_at.format("%Y-%m-%d").to_string();

        println!("{:<5} {:<22} {:<20} {:<12} {}",
            c.id,
            truncate_str(&c.name, 21),
            truncate_str(&c.role, 19),
            format!("{}/{} ({}/{})", answered, total_q, score, max_score),
            date,
        );
    }

    Ok(())
}

fn cmd_export(format: &str, id: i64, output: Option<&PathBuf>, clipboard: bool) -> Result<()> {
    let db = Db::open()?;
    let candidate = db.get_candidate(id)?
        .ok_or_else(|| anyhow::anyhow!("No candidate found with ID {}", id))?;

    let role_slug = qb::slugify(&candidate.role);
    let topics = roles::load_topics_for_role(&role_slug)?;
    let responses = db.load_responses(candidate.id)?;

    let data = ExportData { candidate: &candidate, topics: &topics, responses: &responses };
    let slug = qb::slugify(&candidate.name);
    let base = if slug.is_empty() { candidate.id.to_string() } else { slug };

    if format == "pdf" {
        if clipboard {
            bail!("--clipboard is not supported for PDF output");
        }
        let bytes = pdf::to_pdf(&data)?;
        let default_name = format!("{}.pdf", base);
        return write_output_bytes(&bytes, output, &default_name);
    }

    let (content, ext) = match format {
        "json" => (to_json(&data)?, "json"),
        "csv"  => (to_csv(&data)?, "csv"),
        "html" => (to_html(&data)?, "html"),
        other  => bail!("Unknown format '{}'. Supported: json, csv, html, pdf", other),
    };

    let default_name = format!("{}.{}", base, ext);
    write_output(&content, output, clipboard, &default_name)
}

fn cmd_qb_gen(topic: &str, num_str: &str, append: bool) -> Result<()> {
    let counts = parse_num(num_str)?;
    let slug = qb::slugify(topic);

    println!(
        "Generating questions for '{}': {} level-1, {} level-2, {} level-3, {} level-4...",
        slug, counts[0], counts[1], counts[2], counts[3]
    );

    let content = llm::generate_questions(topic, counts)?;
    let levels = qb::count_levels(&content);

    qb::write_qb(&slug, &content, append)?;

    println!(
        "Generated: L1={} L2={} L3={} L4={}",
        levels[0], levels[1], levels[2], levels[3]
    );
    Ok(())
}

// ── Helpers ───────────────────────────────────────────────────────────────────

fn parse_num(s: &str) -> Result<[u8; 4]> {
    let parts: Vec<&str> = s.split(',').collect();
    if parts.len() != 4 {
        bail!("--num must be 4 comma-separated integers, e.g. 3,3,2,2");
    }
    Ok([
        parts[0].trim().parse().map_err(|_| anyhow::anyhow!("Invalid number: {}", parts[0]))?,
        parts[1].trim().parse().map_err(|_| anyhow::anyhow!("Invalid number: {}", parts[1]))?,
        parts[2].trim().parse().map_err(|_| anyhow::anyhow!("Invalid number: {}", parts[2]))?,
        parts[3].trim().parse().map_err(|_| anyhow::anyhow!("Invalid number: {}", parts[3]))?,
    ])
}

fn truncate_str(s: &str, max: usize) -> String {
    if s.len() <= max {
        s.to_string()
    } else {
        format!("{}…", &s[..max.saturating_sub(1)])
    }
}

use anyhow::{bail, Result};
use chrono::Local;
use clap::{Parser, Subcommand};
use colored::Colorize;

mod models;
mod store;

use models::{Interview, Note, Status, Verdict};
use store::Store;

#[derive(Parser)]
#[command(
    name = "judge",
    version,
    about = "Interview panel tool for technical interviewers",
    long_about = "Quickly capture timestamped notes, rate candidates across key dimensions, \
                  and generate shareable feedback reports — all from your terminal."
)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Start a new interview session
    New {
        /// Candidate's full name
        candidate: String,
        /// Job role being interviewed for
        #[arg(short, long)]
        role: Option<String>,
        /// Your name as the interviewer
        #[arg(short, long)]
        interviewer: Option<String>,
    },
    /// List all interview sessions
    List {
        /// Show only active interviews
        #[arg(short, long)]
        active: bool,
    },
    /// Show details of an interview
    Show {
        /// Interview ID (or unique prefix)
        id: String,
    },
    /// Add a timestamped note to an interview
    Note {
        /// Interview ID (or unique prefix)
        id: String,
        /// Note text
        text: String,
        /// Optional tag (e.g. strength, concern, red-flag)
        #[arg(short, long)]
        tag: Option<String>,
    },
    /// Rate a candidate on a dimension (score 1–10)
    Rate {
        /// Interview ID (or unique prefix)
        id: String,
        /// Dimension: technical, problem-solving, communication, culture
        category: String,
        /// Score from 1 (poor) to 10 (exceptional)
        score: u8,
    },
    /// Close an interview with a hiring verdict
    Close {
        /// Interview ID (or unique prefix)
        id: String,
        /// Verdict: hire, no-hire, strong-hire, strong-no-hire
        verdict: String,
        /// Optional closing summary
        #[arg(short, long)]
        summary: Option<String>,
    },
    /// Print a formatted report for an interview
    Report {
        /// Interview ID (or unique prefix)
        id: String,
    },
    /// Delete an interview permanently
    Delete {
        /// Interview ID (or unique prefix)
        id: String,
        /// Skip confirmation prompt
        #[arg(short, long)]
        yes: bool,
    },
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    let mut store = Store::load()?;

    match cli.command {
        Commands::New { candidate, role, interviewer } => {
            let interview = Interview::new(candidate.clone(), role.clone(), interviewer);
            let id = interview.id.clone();
            store.interviews.insert(id.clone(), interview);
            store.save()?;

            println!("{} Interview started for {}", "✓".green().bold(), candidate.bold());
            println!("  ID:   {}", id.cyan().bold());
            if let Some(r) = &role {
                println!("  Role: {}", r);
            }
            println!();
            println!("  {} judge note {} \"your note\"", "Add note:".dimmed(), id);
            println!("  {} judge rate {} technical 8", "Rate:    ".dimmed(), id);
            println!("  {} judge close {} hire", "Close:   ".dimmed(), id);
        }

        Commands::List { active } => {
            let mut interviews: Vec<&Interview> = store.interviews.values().collect();
            if active {
                interviews.retain(|i| i.status == Status::Active);
            }
            interviews.sort_by(|a, b| b.started_at.cmp(&a.started_at));

            if interviews.is_empty() {
                println!("{}", "No interviews found.".dimmed());
                return Ok(());
            }

            for i in &interviews {
                let status = match i.status {
                    Status::Active => "active".green().to_string(),
                    Status::Closed => "closed".dimmed().to_string(),
                };
                let role = i.role.as_deref().unwrap_or("—");
                let avg = i
                    .average_rating()
                    .map(|r| format!("{:.1}/10", r))
                    .unwrap_or_else(|| "—".to_string());
                let verdict = i
                    .verdict
                    .as_ref()
                    .map(|v| format!(" → {}", v))
                    .unwrap_or_default();
                let date = i
                    .started_at
                    .with_timezone(&Local)
                    .format("%Y-%m-%d")
                    .to_string();

                println!(
                    "  {} {} {}  {} {}{}",
                    i.id.cyan(),
                    i.candidate.bold(),
                    format!("({})", role).dimmed(),
                    status,
                    avg.dimmed(),
                    verdict,
                );
                println!("     {}", date.dimmed());
            }
        }

        Commands::Show { id } => {
            let interview = store
                .find_by_prefix(&id)
                .ok_or_else(|| anyhow::anyhow!("Interview '{}' not found", id))?;
            print_interview(interview);
        }

        Commands::Note { id, text, tag } => {
            let iid = store
                .find_id_by_prefix(&id)
                .ok_or_else(|| anyhow::anyhow!("Interview '{}' not found", id))?;
            {
                let interview = store.interviews.get_mut(&iid).unwrap();
                if interview.status == Status::Closed {
                    bail!("Cannot add notes to a closed interview");
                }
                interview.notes.push(Note {
                    timestamp: chrono::Utc::now(),
                    text: text.clone(),
                    tag: tag.clone(),
                });
            }
            store.save()?;
            let tag_str = tag
                .as_deref()
                .map(|t| format!(" [{}]", t).yellow().to_string())
                .unwrap_or_default();
            println!("{} Note added{}", "✓".green(), tag_str);
        }

        Commands::Rate { id, category, score } => {
            if score < 1 || score > 10 {
                bail!("Score must be between 1 and 10, got {}", score);
            }
            let iid = store
                .find_id_by_prefix(&id)
                .ok_or_else(|| anyhow::anyhow!("Interview '{}' not found", id))?;
            let prev = {
                let interview = store.interviews.get_mut(&iid).unwrap();
                if interview.status == Status::Closed {
                    bail!("Cannot rate a closed interview");
                }
                interview.ratings.insert(category.clone(), score)
            };
            store.save()?;
            if let Some(p) = prev {
                println!(
                    "{} {} updated: {} → {}/10  {}",
                    "✓".green(),
                    category,
                    p,
                    score,
                    rating_bar(score)
                );
            } else {
                println!("{} {} rated: {}/10  {}", "✓".green(), category, score, rating_bar(score));
            }
        }

        Commands::Close { id, verdict, summary } => {
            let verdict_parsed: Verdict = verdict
                .parse()
                .map_err(|e: String| anyhow::anyhow!(e))?;
            let iid = store
                .find_id_by_prefix(&id)
                .ok_or_else(|| anyhow::anyhow!("Interview '{}' not found", id))?;
            {
                let interview = store.interviews.get_mut(&iid).unwrap();
                if interview.status == Status::Closed {
                    bail!("Interview is already closed");
                }
                interview.status = Status::Closed;
                interview.closed_at = Some(chrono::Utc::now());
                interview.verdict = Some(verdict_parsed.clone());
                interview.summary = summary;
            }
            store.save()?;
            println!(
                "{} Interview closed — Verdict: {}",
                "✓".green(),
                verdict_parsed.to_string().bold()
            );
            println!(
                "  Run {} for the full report.",
                format!("judge report {}", iid).cyan()
            );
        }

        Commands::Report { id } => {
            let interview = store
                .find_by_prefix(&id)
                .ok_or_else(|| anyhow::anyhow!("Interview '{}' not found", id))?;
            print_report(interview);
        }

        Commands::Delete { id, yes } => {
            let iid = store
                .find_id_by_prefix(&id)
                .ok_or_else(|| anyhow::anyhow!("Interview '{}' not found", id))?;
            let candidate = store.interviews[&iid].candidate.clone();

            if !yes {
                eprint!("Delete interview for {}? [y/N] ", candidate.bold());
                let mut input = String::new();
                std::io::stdin().read_line(&mut input)?;
                if input.trim().to_lowercase() != "y" {
                    println!("Aborted.");
                    return Ok(());
                }
            }
            store.interviews.remove(&iid);
            store.save()?;
            println!("{} Deleted interview for {}", "✓".green(), candidate);
        }
    }

    Ok(())
}

fn rating_bar(score: u8) -> String {
    let n = score.min(10) as usize;
    let filled = "█".repeat(n);
    let empty = "░".repeat(10 - n);
    if n >= 8 {
        format!("{}{}", filled.green(), empty.dimmed())
    } else if n >= 5 {
        format!("{}{}", filled.yellow(), empty.dimmed())
    } else {
        format!("{}{}", filled.red(), empty.dimmed())
    }
}

fn print_interview(i: &Interview) {
    println!("{}", "─".repeat(52).dimmed());
    println!("{} — {}", i.candidate.bold(), i.id.cyan());
    if let Some(r) = &i.role {
        println!("Role:        {}", r);
    }
    if let Some(iv) = &i.interviewer {
        println!("Interviewer: {}", iv);
    }
    println!(
        "Started:     {}",
        i.started_at.with_timezone(&Local).format("%Y-%m-%d %H:%M")
    );
    let status = match i.status {
        Status::Active => "Active".green().to_string(),
        Status::Closed => "Closed".dimmed().to_string(),
    };
    println!("Status:      {}", status);

    if !i.ratings.is_empty() {
        println!("\n{}", "Ratings".bold().underline());
        let mut ratings: Vec<(&String, &u8)> = i.ratings.iter().collect();
        ratings.sort_by_key(|(k, _)| k.as_str());
        for (cat, score) in &ratings {
            println!("  {:<22} {:>2}/10  {}", cat, score, rating_bar(**score));
        }
        if let Some(avg) = i.average_rating() {
            println!("  {:<22} {:.1}/10", "average", avg);
        }
    }

    if !i.notes.is_empty() {
        println!("\n{}", "Notes".bold().underline());
        for note in &i.notes {
            let time = note.timestamp.with_timezone(&Local).format("%H:%M");
            let tag = note
                .tag
                .as_deref()
                .map(|t| format!(" [{}]", t).yellow().to_string())
                .unwrap_or_default();
            println!("  [{}]{} {}", time, tag, note.text);
        }
    }

    if let Some(v) = &i.verdict {
        println!("\nVerdict: {}", v.to_string().bold());
    }
    if let Some(s) = &i.summary {
        println!("Summary: {}", s);
    }
    println!("{}", "─".repeat(52).dimmed());
}

fn print_report(i: &Interview) {
    let local_start = i.started_at.with_timezone(&Local);

    println!();
    println!("{}", "═".repeat(60));
    println!("{}", "  INTERVIEW REPORT".bold());
    println!("{}", "═".repeat(60));
    println!("  Candidate:   {}", i.candidate.bold());
    if let Some(r) = &i.role {
        println!("  Role:        {}", r);
    }
    if let Some(iv) = &i.interviewer {
        println!("  Interviewer: {}", iv);
    }
    println!("  Date:        {}", local_start.format("%B %d, %Y"));
    println!("  Started:     {}", local_start.format("%H:%M"));
    if let Some(closed) = &i.closed_at {
        let local_end = closed.with_timezone(&Local);
        let mins = closed.signed_duration_since(i.started_at).num_minutes();
        println!("  Ended:       {} ({} min)", local_end.format("%H:%M"), mins);
    }
    println!();

    if !i.ratings.is_empty() {
        println!("{}", "  RATINGS".bold());
        println!("{}", "  ".to_string() + &"─".repeat(48).dimmed().to_string());
        let mut ratings: Vec<(&String, &u8)> = i.ratings.iter().collect();
        ratings.sort_by_key(|(k, _)| k.as_str());
        for (cat, score) in &ratings {
            println!("  {:<22} {:>2}/10  {}", cat, score, rating_bar(**score));
        }
        if let Some(avg) = i.average_rating() {
            println!();
            println!("  {:<22} {:.1}/10", "Average Score", avg);
        }
        println!();
    }

    if !i.notes.is_empty() {
        println!("{}", "  NOTES".bold());
        println!("{}", "  ".to_string() + &"─".repeat(48).dimmed().to_string());
        for note in &i.notes {
            let time = note.timestamp.with_timezone(&Local).format("%H:%M");
            if let Some(tag) = &note.tag {
                println!(
                    "  [{}] [{}] {}",
                    time,
                    tag.to_uppercase().yellow(),
                    note.text
                );
            } else {
                println!("  [{}] {}", time, note.text);
            }
        }
        println!();
    }

    println!("{}", "  VERDICT".bold());
    println!("{}", "  ".to_string() + &"─".repeat(48).dimmed().to_string());
    match &i.verdict {
        Some(v) => {
            let display = match v {
                Verdict::StrongHire | Verdict::Hire => v.to_string().green().bold().to_string(),
                Verdict::NoHire | Verdict::StrongNoHire => v.to_string().red().bold().to_string(),
            };
            println!("  {}", display);
        }
        None => println!("  {}", "Pending".yellow()),
    }
    if let Some(s) = &i.summary {
        println!();
        println!("  {}", s);
    }
    println!("{}", "═".repeat(60));
    println!();
}

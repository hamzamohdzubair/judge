use anyhow::Result;
use std::collections::HashMap;
use std::path::PathBuf;

use crate::models::{Candidate, TopicData};

pub struct ExportData<'a> {
    pub candidate: &'a Candidate,
    pub topics: &'a [TopicData],
    pub responses: &'a HashMap<String, u8>,
}

// ── JSON ──────────────────────────────────────────────────────────────────────

pub fn to_json(data: &ExportData) -> Result<String> {
    let topics: Vec<serde_json::Value> = data.topics.iter().map(|t| {
        let questions: Vec<serde_json::Value> = t.questions.iter().map(|q| {
            let score = data.responses.get(&q.id).copied().unwrap_or(0);
            let question_score = score as u32 * q.level as u32;
            let max_score = 4 * q.level as u32;
            let keywords: serde_json::Value = serde_json::json!({
                "1": q.keywords[0],
                "2": q.keywords[1],
                "3": q.keywords[2],
                "4": q.keywords[3],
            });
            serde_json::json!({
                "id": q.id,
                "level": q.level,
                "text": q.text,
                "score": score,
                "question_score": question_score,
                "max_score": max_score,
                "keywords": keywords,
            })
        }).collect();

        let topic_score = t.score(data.responses);
        let topic_max = t.max_score_asked(data.responses);
        let answered = t.answered(data.responses);
        serde_json::json!({
            "name": t.name,
            "answered": answered,
            "total_questions": t.questions.len(),
            "score": topic_score,
            "max_score": topic_max,
            "questions": questions,
        })
    }).collect();

    let total_score: u32 = data.topics.iter().map(|t| t.score(data.responses)).sum();
    let total_max: u32 = data.topics.iter().map(|t| t.max_score_asked(data.responses)).sum();
    let total_answered: usize = data.topics.iter().map(|t| t.answered(data.responses)).sum();
    let total_questions: usize = data.topics.iter().map(|t| t.questions.len()).sum();

    let out = serde_json::json!({
        "candidate": {
            "id": data.candidate.id,
            "name": data.candidate.name,
            "role": data.candidate.role,
            "created_at": data.candidate.created_at.to_rfc3339(),
        },
        "summary": {
            "total_score": total_score,
            "total_max": total_max,
            "total_answered": total_answered,
            "total_questions": total_questions,
        },
        "topics": topics,
    });

    Ok(serde_json::to_string_pretty(&out)?)
}

// ── CSV ───────────────────────────────────────────────────────────────────────

pub fn to_csv(data: &ExportData) -> Result<String> {
    let mut wtr = csv::Writer::from_writer(vec![]);
    wtr.write_record([
        "candidate_id", "name", "role", "topic", "q_level",
        "question", "score", "question_score", "max_score",
    ])?;

    let cid = data.candidate.id.to_string();
    for t in data.topics {
        for q in &t.questions {
            let score = data.responses.get(&q.id).copied().unwrap_or(0);
            let question_score = score as u32 * q.level as u32;
            let max_score = 4 * q.level as u32;
            wtr.write_record([
                cid.as_str(),
                data.candidate.name.as_str(),
                data.candidate.role.as_str(),
                t.name.as_str(),
                &q.level.to_string(),
                q.text.as_str(),
                &score.to_string(),
                &question_score.to_string(),
                &max_score.to_string(),
            ])?;
        }
    }

    Ok(String::from_utf8(wtr.into_inner()?)?)
}

// ── HTML ──────────────────────────────────────────────────────────────────────

pub fn to_html(data: &ExportData) -> Result<String> {
    let total_score: u32 = data.topics.iter().map(|t| t.score(data.responses)).sum();
    let total_max: u32 = data.topics.iter().map(|t| t.max_score_asked(data.responses)).sum();
    let total_answered: usize = data.topics.iter().map(|t| t.answered(data.responses)).sum();
    let total_questions: usize = data.topics.iter().map(|t| t.questions.len()).sum();

    let pct = if total_max > 0 { total_score * 100 / total_max } else { 0 };
    let date = data.candidate.created_at.format("%B %d, %Y").to_string();

    let mut topic_rows = String::new();
    for t in data.topics {
        let sc = t.score(data.responses);
        let mx = t.max_score_asked(data.responses);
        let ans = t.answered(data.responses);
        let tot = t.questions.len();
        let tp = if mx > 0 { sc * 100 / mx } else { 0 };
        topic_rows.push_str(&format!(
            "<tr><td>{}</td><td>{}/{}</td><td>{}/{}</td><td>{}%</td></tr>\n",
            esc(&t.name), ans, tot, sc, mx, tp
        ));
    }

    let mut topic_sections = String::new();
    for t in data.topics {
        let mut q_rows = String::new();
        for q in &t.questions {
            let score = data.responses.get(&q.id).copied().unwrap_or(0);
            let qs = score as u32 * q.level as u32;
            let ms = 4 * q.level as u32;
            let kw: Vec<String> = (0..4)
                .filter(|&i| !q.keywords[i].is_empty())
                .map(|i| format!("<b>[{}]</b> {}", i + 1, esc(&q.keywords[i].join(", "))))
                .collect();
            q_rows.push_str(&format!(
                "<tr><td>[L{}]</td><td>{}</td><td>{}</td><td>{}/{}</td><td>{}</td></tr>\n",
                q.level, esc(&q.text), score, qs, ms, kw.join("  ")
            ));
        }
        topic_sections.push_str(&format!(
            "<h2>{}</h2><table><thead><tr><th>Lvl</th><th>Question</th><th>Score</th><th>Pts</th><th>Keywords</th></tr></thead><tbody>{}</tbody></table>",
            esc(&t.name), q_rows
        ));
    }

    let html = format!(
        r#"<!DOCTYPE html>
<html lang="en">
<head>
<meta charset="UTF-8">
<title>Interview Report — {name}</title>
<style>
  body {{ font-family: system-ui, sans-serif; max-width: 960px; margin: 2rem auto; padding: 0 1rem; color: #1a1a1a; }}
  h1 {{ font-size: 1.6rem; margin-bottom: 0.25rem; }}
  .meta {{ color: #666; margin-bottom: 2rem; }}
  .summary {{ display: flex; gap: 2rem; margin-bottom: 2rem; }}
  .stat {{ background: #f5f5f5; border-radius: 8px; padding: 1rem 1.5rem; }}
  .stat .label {{ font-size: 0.8rem; color: #666; text-transform: uppercase; letter-spacing: 0.05em; }}
  .stat .value {{ font-size: 1.8rem; font-weight: 700; color: #0070f3; }}
  table {{ width: 100%; border-collapse: collapse; margin-bottom: 2rem; }}
  th {{ background: #f5f5f5; text-align: left; padding: 0.5rem 0.75rem; font-size: 0.85rem; }}
  td {{ padding: 0.5rem 0.75rem; border-bottom: 1px solid #eee; font-size: 0.9rem; }}
  h2 {{ font-size: 1.2rem; margin-top: 2.5rem; border-bottom: 2px solid #eee; padding-bottom: 0.25rem; }}
</style>
</head>
<body>
<h1>Interview Report</h1>
<p class="meta"><strong>{name}</strong> · {role} · {date}</p>
<div class="summary">
  <div class="stat"><div class="label">Total Score</div><div class="value">{total_score}/{total_max}</div></div>
  <div class="stat"><div class="label">Percentage</div><div class="value">{pct}%</div></div>
  <div class="stat"><div class="label">Answered</div><div class="value">{total_answered}/{total_questions}</div></div>
</div>
<h2>Topic Summary</h2>
<table>
<thead><tr><th>Topic</th><th>Answered</th><th>Score</th><th>%</th></tr></thead>
<tbody>{topic_rows}</tbody>
</table>
{topic_sections}
</body>
</html>"#,
        name = esc(&data.candidate.name),
        role = esc(&data.candidate.role),
        date = date,
        total_score = total_score,
        total_max = total_max,
        pct = pct,
        total_answered = total_answered,
        total_questions = total_questions,
        topic_rows = topic_rows,
        topic_sections = topic_sections,
    );

    Ok(html)
}

fn esc(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

// ── Output routing ────────────────────────────────────────────────────────────

pub fn write_output(
    content: &str,
    output: Option<&PathBuf>,
    clipboard: bool,
    default_name: &str,
) -> Result<()> {
    if clipboard {
        match arboard::Clipboard::new() {
            Ok(mut cb) => {
                cb.set_text(content)?;
                println!("Copied to clipboard ({} bytes)", content.len());
            }
            Err(e) => {
                eprintln!("Clipboard unavailable ({}). Printing to stdout instead:", e);
                println!("{}", content);
            }
        }
        return Ok(());
    }

    let path = match output {
        Some(p) => p.clone(),
        None => PathBuf::from(default_name),
    };

    std::fs::write(&path, content)?;
    println!("Written to {}", path.display());
    Ok(())
}

pub fn write_output_bytes(
    content: &[u8],
    output: Option<&PathBuf>,
    default_name: &str,
) -> Result<()> {
    let path = match output {
        Some(p) => p.clone(),
        None => PathBuf::from(default_name),
    };
    std::fs::write(&path, content)?;
    println!("Written to {} ({} bytes)", path.display(), content.len());
    Ok(())
}

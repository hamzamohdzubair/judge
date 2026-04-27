use anyhow::{bail, Context, Result};
use serde_json::json;

pub fn generate_questions(topic: &str, counts: [u8; 4]) -> Result<String> {
    let api_key = std::env::var("GROQ_API_KEY")
        .context("GROQ_API_KEY is not set — required for question generation")?;

    let model = std::env::var("JUDGE_LLM_MODEL")
        .unwrap_or_else(|_| "llama-3.3-70b-versatile".to_string());

    let [n1, n2, n3, n4] = counts;
    let topic_slug = crate::qb::slugify(topic);

    let system = "You are an expert technical interviewer generating structured question banks. \
                  Output ONLY valid markdown in the exact format specified. \
                  No preamble, no explanation, no markdown code fences.";

    // Build the prompt in parts to avoid raw-string delimiter conflicts
    let format_example = format!(
        "# {slug}\n\n\
         ## [1] Question text?\n\n\
         - 1: basic keyword or short phrase\n\
         - 2: intermediate keyword\n\n\
         ## [2] Question text?\n\n\
         - 1: basic keyword\n\
         - 2: intermediate keyword\n\
         - 3: advanced keyword",
        slug = topic_slug
    );

    let rules = "Rules:\n\
         - Each heading '## [N] text?' defines a question at level N (1=basic, 4=expert)\n\
         - Each bullet '- N: text' is an answer keyword for depth N\n\
         - Only include keyword levels that are relevant to the question\n\
         - Keywords are SHORT (under 10 words each); comma-separate multiple concepts\n\
         - Questions must be specific and testable, not vague\n\
         - Output ONLY the markdown. No extra text before or after.";

    let user = format!(
        "Generate interview questions for the topic: {topic}\n\n\
         Generate exactly:\n\
         - {n1} level-1 questions (basic, foundational knowledge)\n\
         - {n2} level-2 questions (intermediate, applied understanding)\n\
         - {n3} level-3 questions (advanced, deep understanding)\n\
         - {n4} level-4 questions (expert, research/architecture level)\n\n\
         Output ONLY in this exact format (no code fences, no preamble):\n\n\
         {fmt}\n\n\
         {rules}",
        topic = topic,
        n1 = n1,
        n2 = n2,
        n3 = n3,
        n4 = n4,
        fmt = format_example,
        rules = rules,
    );

    let body = json!({
        "model": model,
        "messages": [
            {"role": "system", "content": system},
            {"role": "user",   "content": user}
        ],
        "temperature": 0.7,
        "max_tokens": 4096
    });

    let response = ureq::post("https://api.groq.com/openai/v1/chat/completions")
        .set("Authorization", &format!("Bearer {}", api_key))
        .set("Content-Type", "application/json")
        .send_json(body)
        .context("Failed to call Groq API")?;

    let json: serde_json::Value =
        response.into_json().context("Failed to parse Groq API response")?;

    let content = json["choices"][0]["message"]["content"]
        .as_str()
        .context("Unexpected Groq API response structure")?
        .trim()
        .to_string();

    // Basic validation: must contain at least one question heading
    if !content.contains("## [") {
        bail!(
            "LLM response did not contain questions in the expected format.\nResponse preview:\n{}",
            &content[..content.len().min(500)]
        );
    }

    // Strip markdown code fences if the model ignored instructions
    let content = if content.starts_with("```") {
        content
            .lines()
            .skip(1)
            .collect::<Vec<_>>()
            .join("\n")
            .trim_end_matches("```")
            .trim()
            .to_string()
    } else {
        content
    };

    // Inject [AI] marker into every question heading
    let content = content
        .lines()
        .map(|line| {
            if let Some(rest) = line.trim_start().strip_prefix("## [") {
                if let Some(end) = rest.find(']') {
                    let after = rest[end + 1..].trim();
                    if !after.starts_with("[AI]") {
                        return format!("## [{}] [AI] {}", &rest[..end], after);
                    }
                }
            }
            line.to_string()
        })
        .collect::<Vec<_>>()
        .join("\n");

    Ok(content)
}

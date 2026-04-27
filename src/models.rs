use chrono::{DateTime, Utc};
use std::collections::HashMap;

#[derive(Debug, Clone)]
pub struct Candidate {
    pub id: i64,
    pub name: String,
    pub role: String,
    pub created_at: DateTime<Utc>,
}

/// A single interview question loaded from a .md question bank file.
/// Never stored in the DB — loaded fresh from ~/.judge/qb/ each run.
#[derive(Debug, Clone)]
pub struct Question {
    /// hex(sha256(topic + "::" + text))[..16] — stable content hash
    pub id: String,
    #[allow(dead_code)]
    pub topic: String,
    pub level: u8,
    pub text: String,
    /// Index 0 = level-1 keywords, index 3 = level-4 keywords
    pub keywords: [Vec<String>; 4],
    /// True when the question heading has the [AI] marker
    pub ai_generated: bool,
}

#[derive(Debug, Clone)]
pub struct TopicData {
    pub name: String,
    pub questions: Vec<Question>,
}

impl TopicData {
    pub fn max_score(&self) -> u32 {
        self.questions.iter().map(|q| 4 * q.level as u32).sum()
    }

    pub fn max_score_asked(&self, responses: &HashMap<String, u8>) -> u32 {
        self.questions
            .iter()
            .filter(|q| responses.contains_key(&q.id))
            .map(|q| 4 * q.level as u32)
            .sum()
    }

    pub fn score(&self, responses: &HashMap<String, u8>) -> u32 {
        self.questions
            .iter()
            .map(|q| responses.get(&q.id).copied().unwrap_or(0) as u32 * q.level as u32)
            .sum()
    }

    pub fn answered(&self, responses: &HashMap<String, u8>) -> usize {
        self.questions.iter().filter(|q| responses.contains_key(&q.id)).count()
    }
}

pub struct Response {
    pub candidate_id: i64,
    pub question_id: String,
    pub score: u8,
}

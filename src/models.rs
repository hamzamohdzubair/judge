use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Interview {
    pub id: String,
    pub candidate: String,
    pub role: Option<String>,
    pub interviewer: Option<String>,
    pub started_at: DateTime<Utc>,
    pub closed_at: Option<DateTime<Utc>>,
    pub status: Status,
    pub notes: Vec<Note>,
    pub ratings: HashMap<String, u8>,
    pub verdict: Option<Verdict>,
    pub summary: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum Status {
    Active,
    Closed,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Note {
    pub timestamp: DateTime<Utc>,
    pub text: String,
    pub tag: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Verdict {
    StrongHire,
    Hire,
    NoHire,
    StrongNoHire,
}

impl std::fmt::Display for Verdict {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Verdict::StrongHire => write!(f, "Strong Hire"),
            Verdict::Hire => write!(f, "Hire"),
            Verdict::NoHire => write!(f, "No Hire"),
            Verdict::StrongNoHire => write!(f, "Strong No Hire"),
        }
    }
}

impl std::str::FromStr for Verdict {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let normalized = s.to_lowercase().replace(['-', '_', ' '], "");
        match normalized.as_str() {
            "stronghire" => Ok(Verdict::StrongHire),
            "hire" => Ok(Verdict::Hire),
            "nohire" => Ok(Verdict::NoHire),
            "strongnohire" => Ok(Verdict::StrongNoHire),
            _ => Err(format!(
                "Unknown verdict '{}'. Use: hire, no-hire, strong-hire, strong-no-hire",
                s
            )),
        }
    }
}

impl Interview {
    pub fn new(
        candidate: String,
        role: Option<String>,
        interviewer: Option<String>,
    ) -> Self {
        let id = Uuid::new_v4().to_string()[..8].to_string();
        Self {
            id,
            candidate,
            role,
            interviewer,
            started_at: Utc::now(),
            closed_at: None,
            status: Status::Active,
            notes: Vec::new(),
            ratings: HashMap::new(),
            verdict: None,
            summary: None,
        }
    }

    pub fn average_rating(&self) -> Option<f32> {
        if self.ratings.is_empty() {
            return None;
        }
        let sum: u32 = self.ratings.values().map(|&v| v as u32).sum();
        Some(sum as f32 / self.ratings.len() as f32)
    }
}

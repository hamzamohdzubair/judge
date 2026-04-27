use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use rusqlite::{params, Connection};
use std::collections::HashMap;
use std::path::PathBuf;

use crate::models::{Candidate, Response};

pub struct Db {
    conn: Connection,
}

impl Db {
    pub fn db_path() -> PathBuf {
        dirs::home_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join(".judge")
            .join("judge.db")
    }

    pub fn open() -> Result<Self> {
        let path = Self::db_path();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let conn = Connection::open(&path)
            .with_context(|| format!("Failed to open database at {}", path.display()))?;
        let db = Db { conn };
        db.migrate()?;
        Ok(db)
    }

    fn migrate(&self) -> Result<()> {
        // Detect old schema where candidates.id was TEXT PRIMARY KEY.
        // CREATE TABLE IF NOT EXISTS is a no-op on an existing table, which
        // would leave inserts with empty ids and silently drop rows on read.
        let legacy: i64 = self.conn.query_row(
            "SELECT COUNT(*) FROM pragma_table_info('candidates')
             WHERE name = 'id' AND UPPER(type) = 'TEXT'",
            [],
            |row| row.get(0),
        ).unwrap_or(0);

        if legacy > 0 {
            self.conn.execute_batch(
                "DROP TABLE IF EXISTS responses;
                 DROP TABLE IF EXISTS candidates;",
            )?;
        }

        self.conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS candidates (
                id         INTEGER PRIMARY KEY AUTOINCREMENT,
                name       TEXT NOT NULL,
                role       TEXT NOT NULL,
                created_at TEXT NOT NULL
            );
            CREATE TABLE IF NOT EXISTS responses (
                candidate_id INTEGER NOT NULL,
                question_id  TEXT NOT NULL,
                score        INTEGER NOT NULL DEFAULT 0,
                updated_at   TEXT NOT NULL,
                PRIMARY KEY (candidate_id, question_id)
            );",
        )?;
        Ok(())
    }

    pub fn create_candidate(&self, name: String, role: String) -> Result<Candidate> {
        let created_at = Utc::now();
        self.conn.execute(
            "INSERT INTO candidates (name, role, created_at) VALUES (?1, ?2, ?3)",
            params![name, role, created_at.to_rfc3339()],
        )?;
        let id = self.conn.last_insert_rowid();
        Ok(Candidate { id, name, role, created_at })
    }

    pub fn get_candidate(&self, id: i64) -> Result<Option<Candidate>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, name, role, created_at FROM candidates WHERE id = ?1",
        )?;
        let result = stmt
            .query_map(params![id], parse_candidate)?
            .filter_map(|r| r.ok())
            .next();
        Ok(result)
    }

    pub fn list_candidates(&self) -> Result<Vec<Candidate>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, name, role, created_at FROM candidates ORDER BY created_at DESC",
        )?;
        let candidates = stmt
            .query_map([], parse_candidate)?
            .filter_map(|r| r.ok())
            .collect();
        Ok(candidates)
    }

    pub fn upsert_response(&self, r: &Response) -> Result<()> {
        self.conn.execute(
            "INSERT OR REPLACE INTO responses (candidate_id, question_id, score, updated_at)
             VALUES (?1, ?2, ?3, ?4)",
            params![r.candidate_id, r.question_id, r.score, Utc::now().to_rfc3339()],
        )?;
        Ok(())
    }

    pub fn delete_response(&self, candidate_id: i64, question_id: &str) -> Result<()> {
        self.conn.execute(
            "DELETE FROM responses WHERE candidate_id = ?1 AND question_id = ?2",
            params![candidate_id, question_id],
        )?;
        Ok(())
    }

    pub fn load_responses(&self, candidate_id: i64) -> Result<HashMap<String, u8>> {
        let mut stmt = self.conn.prepare(
            "SELECT question_id, score FROM responses WHERE candidate_id = ?1",
        )?;
        let map = stmt
            .query_map(params![candidate_id], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, u8>(1)?))
            })?
            .filter_map(|r| r.ok())
            .collect();
        Ok(map)
    }
}

fn parse_candidate(row: &rusqlite::Row) -> rusqlite::Result<Candidate> {
    let created_at_str: String = row.get(3)?;
    let created_at: DateTime<Utc> = DateTime::parse_from_rfc3339(&created_at_str)
        .map(|dt| dt.with_timezone(&Utc))
        .unwrap_or_else(|_| Utc::now());
    Ok(Candidate {
        id: row.get(0)?,
        name: row.get(1)?,
        role: row.get(2)?,
        created_at,
    })
}

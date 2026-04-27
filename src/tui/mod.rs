use anyhow::Result;
use crossterm::event::{self, Event};
use std::collections::HashMap;
use std::time::Duration;

mod app;
mod ui;

pub use app::AppState;

use crate::db::Db;
use crate::models::{Candidate, TopicData};

pub fn run(
    db: Db,
    candidate: Candidate,
    topics: Vec<TopicData>,
    responses: HashMap<String, u8>,
) -> Result<()> {
    let mut terminal = ratatui::init();
    let mut app = AppState::new(candidate, topics, responses, db);

    loop {
        terminal.draw(|f| ui::render(f, &app))?;

        if event::poll(Duration::from_millis(100))? {
            if let Event::Key(key) = event::read()? {
                app.handle_key(key);
            }
        }

        if app.should_quit {
            break;
        }
    }

    ratatui::restore();
    Ok(())
}

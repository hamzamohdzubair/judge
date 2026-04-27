use std::cell::Cell;
use std::collections::HashMap;

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use rand::seq::SliceRandom;

use crate::db::Db;
use crate::models::{Candidate, Question, Response, TopicData};

// ── Filter ────────────────────────────────────────────────────────────────────

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum FilterSource {
    User,
    Ai,
}

#[derive(Clone)]
pub struct Filter {
    pub level:  Option<u8>,
    pub source: Option<FilterSource>,
}

impl Filter {
    fn none() -> Self {
        Filter { level: None, source: None }
    }

    pub fn is_active(&self) -> bool {
        self.level.is_some() || self.source.is_some()
    }

    pub fn matches(&self, q: &Question) -> bool {
        if let Some(lvl) = self.level {
            if q.level != lvl {
                return false;
            }
        }
        if let Some(src) = &self.source {
            match src {
                FilterSource::User => {
                    if q.ai_generated {
                        return false;
                    }
                }
                FilterSource::Ai => {
                    if !q.ai_generated {
                        return false;
                    }
                }
            }
        }
        true
    }

    pub fn label(&self) -> String {
        let lvl = self.level.map(|l| format!("L{}", l));
        let src = self.source.as_ref().map(|s| match s {
            FilterSource::User => "USER",
            FilterSource::Ai   => "AI",
        });
        match (lvl, src) {
            (Some(l), Some(s)) => format!("{}·{}", l, s),
            (Some(l), None)    => l,
            (None, Some(s))    => s.to_string(),
            (None, None)       => String::new(),
        }
    }
}

pub enum FilterMode {
    Normal,
    PendingLetter,
    PendingLevel(FilterSource),
}

// ── Search ────────────────────────────────────────────────────────────────────

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum SearchScope {
    InTopic,
    AllTopics,
    TopicName,
}

pub struct SearchState {
    pub scope:   SearchScope,
    pub query:   String,
    /// (topic_idx, question_idx in raw list). For TopicName: (topic_idx, 0).
    pub matches: Vec<(usize, usize)>,
    pub cursor:  usize,
}

impl SearchState {
    fn new(scope: SearchScope) -> Self {
        SearchState { scope, query: String::new(), matches: vec![], cursor: 0 }
    }

    fn recompute(&mut self, topics: &[TopicData], current_topic: usize) {
        self.matches.clear();
        let q = self.query.to_lowercase();

        match self.scope {
            SearchScope::TopicName => {
                for (ti, topic) in topics.iter().enumerate() {
                    if topic.name.to_lowercase().contains(&q) {
                        self.matches.push((ti, 0));
                    }
                }
            }
            SearchScope::InTopic => {
                if let Some(topic) = topics.get(current_topic) {
                    for (qi, question) in topic.questions.iter().enumerate() {
                        if question_matches_query(question, &q) {
                            self.matches.push((current_topic, qi));
                        }
                    }
                }
            }
            SearchScope::AllTopics => {
                for (ti, topic) in topics.iter().enumerate() {
                    for (qi, question) in topic.questions.iter().enumerate() {
                        if question_matches_query(question, &q) {
                            self.matches.push((ti, qi));
                        }
                    }
                }
            }
        }

        self.cursor = self.cursor.min(self.matches.len().saturating_sub(1));
    }

    pub fn prefix_char(&self) -> char {
        match self.scope {
            SearchScope::InTopic   => '/',
            SearchScope::AllTopics => '?',
            SearchScope::TopicName => 't',
        }
    }
}

fn question_matches_query(q: &Question, query_lower: &str) -> bool {
    if query_lower.is_empty() {
        return true;
    }
    if q.text.to_lowercase().contains(query_lower) {
        return true;
    }
    q.keywords.iter().flat_map(|kws| kws.iter()).any(|kw| kw.to_lowercase().contains(query_lower))
}

// ── AppState ──────────────────────────────────────────────────────────────────

pub struct AppState {
    pub candidate: Candidate,
    pub topics: Vec<TopicData>,
    pub responses: HashMap<String, u8>,
    pub current_topic: usize,
    pub current_question: usize,
    pub scroll_offset: usize,
    pub topic_table_offset: usize,
    pub should_quit: bool,
    /// Updated by ui::render each frame so scroll logic in handle_key is accurate
    pub visible_card_count: Cell<usize>,
    /// Updated by ui::render each frame — number of topic rows visible in the right panel
    pub visible_topic_count: Cell<usize>,
    pub filter: Filter,
    pub filter_mode: FilterMode,
    pub search: Option<SearchState>,
    /// First key of a two-key sequence ('R' or 'r')
    pub key_seq: Option<char>,
    db: Db,
}

impl AppState {
    pub fn new(
        candidate: Candidate,
        mut topics: Vec<TopicData>,
        responses: HashMap<String, u8>,
        db: Db,
    ) -> Self {
        let mut rng = rand::thread_rng();
        for topic in topics.iter_mut() {
            reorder_questions(&mut topic.questions, &mut rng);
        }

        Self {
            candidate,
            topics,
            responses,
            current_topic: 0,
            current_question: 0,
            scroll_offset: 0,
            topic_table_offset: 0,
            should_quit: false,
            visible_card_count: Cell::new(5),
            visible_topic_count: Cell::new(5),
            filter: Filter::none(),
            filter_mode: FilterMode::Normal,
            search: None,
            key_seq: None,
            db,
        }
    }

    // Questions visible in the left panel for the current topic.
    // Search overrides filter; without search, filter applies.
    pub fn topic_questions(&self) -> Vec<&Question> {
        let topic = &self.topics[self.current_topic];
        match &self.search {
            Some(s) if s.scope != SearchScope::TopicName => {
                let ql = s.query.to_lowercase();
                topic.questions.iter().filter(|q| question_matches_query(q, &ql)).collect()
            }
            _ => {
                topic.questions.iter().filter(|q| self.filter.matches(q)).collect()
            }
        }
    }

    // Whether a raw question index in a topic is a search match (for border colour).
    pub fn is_search_match(&self, topic_idx: usize, raw_q_idx: usize) -> bool {
        if let Some(s) = &self.search {
            if s.scope == SearchScope::TopicName {
                return false;
            }
            return s.matches.iter().any(|&(ti, qi)| ti == topic_idx && qi == raw_q_idx);
        }
        false
    }

    pub fn is_current_search_match(&self, topic_idx: usize, raw_q_idx: usize) -> bool {
        if let Some(s) = &self.search {
            if s.matches.is_empty() {
                return false;
            }
            let (ti, qi) = s.matches[s.cursor];
            return ti == topic_idx && qi == raw_q_idx;
        }
        false
    }

    pub fn topic_search_is_match(&self, topic_idx: usize) -> bool {
        if let Some(s) = &self.search {
            match s.scope {
                SearchScope::TopicName | SearchScope::AllTopics => {
                    return s.matches.iter().any(|&(ti, _)| ti == topic_idx);
                }
                SearchScope::InTopic => {}
            }
        }
        false
    }

    pub fn topic_search_is_cursor(&self, topic_idx: usize) -> bool {
        if let Some(s) = &self.search {
            if !s.matches.is_empty() {
                match s.scope {
                    SearchScope::TopicName | SearchScope::AllTopics => {
                        return s.matches[s.cursor].0 == topic_idx;
                    }
                    SearchScope::InTopic => {}
                }
            }
        }
        false
    }

    pub fn handle_key(&mut self, key: KeyEvent) {
        // ── Search input mode ─────────────────────────────────────────────────
        if self.search.is_some() {
            match key.code {
                KeyCode::Esc => {
                    self.search = None;
                    return;
                }
                KeyCode::Enter => {
                    let is_topic = self.search.as_ref()
                        .map(|s| s.scope == SearchScope::TopicName)
                        .unwrap_or(false);
                    if is_topic {
                        if let Some(s) = &self.search {
                            if !s.matches.is_empty() {
                                self.current_topic = s.matches[s.cursor].0;
                                self.current_question = 0;
                                self.scroll_offset = 0;
                                self.adjust_topic_scroll();
                            }
                        }
                        self.search = None;
                    }
                    return;
                }
                KeyCode::Tab => {
                    self.search_step(1);
                    self.navigate_topic_search_cursor();
                    return;
                }
                KeyCode::BackTab => {
                    self.search_step(-1);
                    self.navigate_topic_search_cursor();
                    return;
                }
                KeyCode::Backspace => {
                    self.search.as_mut().unwrap().query.pop();
                    let ct = self.current_topic;
                    self.search.as_mut().unwrap().recompute(&self.topics, ct);
                    if !self.auto_open_if_single_topic_match() {
                        self.jump_to_search_cursor();
                    }
                    return;
                }
                KeyCode::Char(c) => {
                    self.search.as_mut().unwrap().query.push(c);
                    let ct = self.current_topic;
                    self.search.as_mut().unwrap().recompute(&self.topics, ct);
                    if !self.auto_open_if_single_topic_match() {
                        self.jump_to_search_cursor();
                    }
                    return;
                }
                // Arrow keys fall through to normal navigation
                KeyCode::Up | KeyCode::Down | KeyCode::Left | KeyCode::Right => {}
                _ => return,
            }
        }

        // ── Filter key sequences ──────────────────────────────────────────────
        match &self.filter_mode {
            FilterMode::PendingLetter => {
                match key.code {
                    KeyCode::Char('1') => self.apply_filter(Filter { level: Some(1), source: None }),
                    KeyCode::Char('2') => self.apply_filter(Filter { level: Some(2), source: None }),
                    KeyCode::Char('3') => self.apply_filter(Filter { level: Some(3), source: None }),
                    KeyCode::Char('4') => self.apply_filter(Filter { level: Some(4), source: None }),
                    KeyCode::Char('a') => {
                        self.filter_mode = FilterMode::PendingLevel(FilterSource::Ai);
                    }
                    KeyCode::Char('u') => {
                        self.filter_mode = FilterMode::PendingLevel(FilterSource::User);
                    }
                    _ => { self.filter_mode = FilterMode::Normal; }
                }
                return;
            }
            FilterMode::PendingLevel(src) => {
                let src = *src;
                match key.code {
                    KeyCode::Char('1') => self.apply_filter(Filter { level: Some(1), source: Some(src) }),
                    KeyCode::Char('2') => self.apply_filter(Filter { level: Some(2), source: Some(src) }),
                    KeyCode::Char('3') => self.apply_filter(Filter { level: Some(3), source: Some(src) }),
                    KeyCode::Char('4') => self.apply_filter(Filter { level: Some(4), source: Some(src) }),
                    KeyCode::Char('a') if src == FilterSource::Ai => {
                        self.apply_filter(Filter { level: None, source: Some(FilterSource::Ai) });
                    }
                    KeyCode::Char('u') if src == FilterSource::User => {
                        self.apply_filter(Filter { level: None, source: Some(FilterSource::User) });
                    }
                    _ => { self.filter_mode = FilterMode::Normal; }
                }
                return;
            }
            FilterMode::Normal => {}
        }

        // ── Normal keys ───────────────────────────────────────────────────────

        // Handle pending two-key sequences (R_ / r_)
        if let Some(seq) = self.key_seq.take() {
            match (seq, key.code) {
                ('R', KeyCode::Char('R')) => { self.jump_random(true, None); return; }
                ('R', KeyCode::Char('U')) => { self.jump_random(true, Some(false)); return; }
                ('R', KeyCode::Char('A')) => { self.jump_random(true, Some(true)); return; }
                ('r', KeyCode::Char('r')) => { self.jump_random(false, None); return; }
                ('r', KeyCode::Char('u')) => { self.jump_random(false, Some(false)); return; }
                ('r', KeyCode::Char('a')) => { self.jump_random(false, Some(true)); return; }
                _ => {} // Unrecognized sequence — fall through and process the key normally
            }
        }

        match key.code {
            KeyCode::Char('q') => self.should_quit = true,
            KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.should_quit = true;
            }

            KeyCode::Char('f') => {
                self.filter_mode = FilterMode::PendingLetter;
            }
            KeyCode::Char('F') => {
                self.apply_filter(Filter::none());
            }

            KeyCode::Char('/') => {
                let mut s = SearchState::new(SearchScope::InTopic);
                s.recompute(&self.topics, self.current_topic);
                self.search = Some(s);
            }
            KeyCode::Char('?') => {
                let mut s = SearchState::new(SearchScope::AllTopics);
                s.recompute(&self.topics, self.current_topic);
                self.search = Some(s);
            }
            KeyCode::Char('t') => {
                let mut s = SearchState::new(SearchScope::TopicName);
                s.recompute(&self.topics, self.current_topic);
                self.search = Some(s);
            }

            // Topic navigation
            KeyCode::Right | KeyCode::Char('l') => {
                if self.current_topic + 1 < self.topics.len() {
                    self.current_topic += 1;
                    let max = self.topic_questions().len().saturating_sub(1);
                    self.current_question = self.current_question.min(max);
                    self.scroll_offset = 0;
                    self.adjust_topic_scroll();
                }
            }
            KeyCode::Left | KeyCode::Char('h') => {
                if self.current_topic > 0 {
                    self.current_topic -= 1;
                    let max = self.topic_questions().len().saturating_sub(1);
                    self.current_question = self.current_question.min(max);
                    self.scroll_offset = 0;
                    self.adjust_topic_scroll();
                }
            }

            // Question navigation
            KeyCode::Down | KeyCode::Char('j') => {
                let n = self.topic_questions().len();
                if n > 0 && self.current_question + 1 < n {
                    self.current_question += 1;
                    self.adjust_scroll_down();
                }
            }
            KeyCode::Up | KeyCode::Char('k') => {
                if self.current_question > 0 {
                    self.current_question -= 1;
                    self.adjust_scroll_up();
                }
            }

            KeyCode::Tab => self.jump_next_section(),
            KeyCode::BackTab => self.jump_prev_section(),

            KeyCode::Char(c) if ('0'..='4').contains(&c) => {
                let score = c as u8 - b'0';
                self.set_score(score);
            }
            KeyCode::Char('-') => {
                self.clear_score();
            }

            KeyCode::Char('R') => { self.key_seq = Some('R'); }
            KeyCode::Char('r') => { self.key_seq = Some('r'); }

            _ => {}
        }
    }

    fn apply_filter(&mut self, filter: Filter) {
        self.filter = filter;
        self.filter_mode = FilterMode::Normal;
        let max = self.topic_questions().len().saturating_sub(1);
        self.current_question = self.current_question.min(max);
        self.scroll_offset = 0;
    }

    fn search_step(&mut self, dir: i32) {
        let Some(s) = &self.search else { return };
        if s.matches.is_empty() {
            return;
        }
        let len = s.matches.len();
        let next = ((s.cursor as i32 + dir).rem_euclid(len as i32)) as usize;
        self.search.as_mut().unwrap().cursor = next;
        self.jump_to_search_cursor();
    }

    fn jump_to_search_cursor(&mut self) {
        let Some(s) = &self.search else { return };
        if s.matches.is_empty() {
            return;
        }
        let (ti, raw_qi) = s.matches[s.cursor];
        let scope = s.scope;

        match scope {
            SearchScope::TopicName => {}
            SearchScope::InTopic | SearchScope::AllTopics => {
                if scope == SearchScope::AllTopics {
                    self.current_topic = ti;
                    self.adjust_topic_scroll();
                }
                // Find the position of the raw question in the visible list
                let raw_ptr = &self.topics[ti].questions[raw_qi] as *const _;
                let visible = self.topic_questions();
                if let Some(pos) = visible.iter().position(|q| *q as *const _ == raw_ptr) {
                    self.current_question = pos;
                    self.adjust_scroll_to(pos);
                }
            }
        }
    }

    fn adjust_scroll_to(&mut self, pos: usize) {
        let visible = self.visible_card_count.get().max(1);
        if pos < self.scroll_offset {
            self.scroll_offset = pos;
        } else if pos >= self.scroll_offset + visible {
            self.scroll_offset = pos + 1 - visible;
        }
    }

    fn adjust_scroll_down(&mut self) {
        let visible = self.visible_card_count.get().max(1);
        if self.current_question >= self.scroll_offset + visible {
            self.scroll_offset = self.current_question + 1 - visible;
        }
    }

    fn adjust_scroll_up(&mut self) {
        if self.current_question < self.scroll_offset {
            self.scroll_offset = self.current_question;
        }
    }

    pub fn adjust_topic_scroll(&mut self) {
        let visible = self.visible_topic_count.get().max(1);
        if self.current_topic < self.topic_table_offset {
            self.topic_table_offset = self.current_topic;
        } else if self.current_topic >= self.topic_table_offset + visible {
            self.topic_table_offset = self.current_topic + 1 - visible;
        }
    }

    fn section_key(q: &Question) -> (bool, u8) {
        (q.ai_generated, q.level)
    }

    fn jump_next_section(&mut self) {
        let visible = self.topic_questions();
        if visible.is_empty() {
            return;
        }
        let cur_key = Self::section_key(visible[self.current_question]);
        let next_idx = visible
            .iter()
            .enumerate()
            .skip(self.current_question + 1)
            .find(|(_, q)| Self::section_key(q) != cur_key)
            .map(|(i, _)| i)
            .unwrap_or(0);
        self.current_question = next_idx;
        self.scroll_offset = next_idx;
    }

    fn jump_prev_section(&mut self) {
        let visible = self.topic_questions();
        if visible.is_empty() {
            return;
        }
        let cur_key = Self::section_key(visible[self.current_question]);
        let section_start = visible
            .iter()
            .enumerate()
            .rev()
            .skip(visible.len() - self.current_question)
            .find(|(_, q)| Self::section_key(q) != cur_key)
            .map(|(i, _)| i + 1)
            .unwrap_or(0);

        if section_start == 0 {
            let last_key = Self::section_key(visible.last().unwrap());
            let last_section_start = visible
                .iter()
                .enumerate()
                .rev()
                .find(|(_, q)| Self::section_key(q) != last_key)
                .map(|(i, _)| i + 1)
                .unwrap_or(0);
            self.current_question = last_section_start;
            self.scroll_offset = last_section_start;
        } else {
            let prev_key = Self::section_key(visible[section_start - 1]);
            let prev_section_start = visible
                .iter()
                .enumerate()
                .rev()
                .skip(visible.len() - section_start)
                .find(|(_, q)| Self::section_key(q) != prev_key)
                .map(|(i, _)| i + 1)
                .unwrap_or(0);
            self.current_question = prev_section_start;
            self.scroll_offset = prev_section_start;
        }
    }

    fn navigate_topic_search_cursor(&mut self) {
        if let Some(s) = &self.search {
            if s.scope == SearchScope::TopicName && !s.matches.is_empty() {
                self.current_topic = s.matches[s.cursor].0;
                self.current_question = 0;
                self.scroll_offset = 0;
                self.adjust_topic_scroll();
            }
        }
    }

    fn auto_open_if_single_topic_match(&mut self) -> bool {
        let should_open = self.search.as_ref().map_or(false, |s| {
            s.scope == SearchScope::TopicName && !s.query.is_empty() && s.matches.len() == 1
        });
        if should_open {
            let ti = self.search.as_ref().unwrap().matches[0].0;
            self.current_topic = ti;
            self.current_question = 0;
            self.scroll_offset = 0;
            self.search = None;
            self.adjust_topic_scroll();
        }
        should_open
    }

    fn jump_random(&mut self, any_topic: bool, ai_filter: Option<bool>) {
        let mut rng = rand::thread_rng();

        if any_topic {
            let responses = &self.responses;
            let pool: Vec<(usize, usize)> = self.topics.iter()
                .enumerate()
                .flat_map(|(ti, topic)| {
                    topic.questions.iter()
                        .enumerate()
                        .filter(move |(_, q)| {
                            !responses.contains_key(&q.id)
                                && ai_filter.map_or(true, |ai| q.ai_generated == ai)
                        })
                        .map(move |(qi, _)| (ti, qi))
                })
                .collect();

            if let Some(&(ti, qi)) = pool.choose(&mut rng) {
                self.current_topic = ti;
                self.scroll_offset = 0;
                self.search = None;
                self.filter = Filter::none();
                self.adjust_topic_scroll();
                let q_id = self.topics[ti].questions[qi].id.clone();
                let visible = self.topic_questions();
                if let Some(pos) = visible.iter().position(|q| q.id == q_id) {
                    self.current_question = pos;
                    self.adjust_scroll_to(pos);
                }
            }
        } else {
            let ti = self.current_topic;
            let responses = &self.responses;
            let pool: Vec<usize> = self.topics[ti].questions.iter()
                .enumerate()
                .filter(|(_, q)| {
                    !responses.contains_key(&q.id)
                        && ai_filter.map_or(true, |ai| q.ai_generated == ai)
                })
                .map(|(qi, _)| qi)
                .collect();

            if let Some(&qi) = pool.choose(&mut rng) {
                let q_id = self.topics[ti].questions[qi].id.clone();
                self.search = None;
                self.filter = Filter::none();
                let visible = self.topic_questions();
                if let Some(pos) = visible.iter().position(|q| q.id == q_id) {
                    self.current_question = pos;
                    self.adjust_scroll_to(pos);
                }
            }
        }
    }

    fn set_score(&mut self, score: u8) {
        let visible = self.topic_questions();
        if visible.is_empty() {
            return;
        }
        let question = visible[self.current_question];
        let qid = question.id.clone();
        let cid = self.candidate.id;

        self.responses.insert(qid.clone(), score);

        let r = Response { candidate_id: cid, question_id: qid, score };
        if let Err(e) = self.db.upsert_response(&r) {
            eprintln!("DB write error: {}", e);
        }
    }

    fn clear_score(&mut self) {
        let visible = self.topic_questions();
        if visible.is_empty() {
            return;
        }
        let qid = visible[self.current_question].id.clone();
        let cid = self.candidate.id;

        self.responses.remove(&qid);

        if let Err(e) = self.db.delete_response(cid, &qid) {
            eprintln!("DB write error: {}", e);
        }
    }

    pub fn total_score(&self) -> u32 {
        self.topics.iter().map(|t| t.score(&self.responses)).sum()
    }

    pub fn total_max(&self) -> u32 {
        self.topics.iter().map(|t| t.max_score_asked(&self.responses)).sum()
    }

    pub fn total_answered(&self) -> usize {
        self.topics.iter().map(|t| t.answered(&self.responses)).sum()
    }

    pub fn total_questions(&self) -> usize {
        self.topics.iter().map(|t| t.questions.len()).sum()
    }
}

fn reorder_questions(qs: &mut Vec<Question>, rng: &mut impl rand::Rng) {
    // User questions (ai_generated=false) first, AI last; level ascending within each group.
    qs.sort_by_key(|q| (q.ai_generated, q.level));
    // Shuffle within each (ai_generated, level) bucket.
    let mut i = 0;
    while i < qs.len() {
        let mut j = i + 1;
        while j < qs.len()
            && qs[j].ai_generated == qs[i].ai_generated
            && qs[j].level == qs[i].level
        {
            j += 1;
        }
        qs[i..j].shuffle(rng);
        i = j;
    }
}

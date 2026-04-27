use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Cell, Paragraph, Row, Table},
    Frame,
};

use super::app::{AppState, FilterMode, SearchScope};

const CARD_HEIGHT: u16 = 7;

pub fn render(frame: &mut Frame, app: &AppState) {
    let area = frame.area();

    let header_height: u16 = if app.search.is_some() { 4 } else { 3 };

    let outer = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(header_height), Constraint::Min(0)])
        .split(area);

    render_header(frame, outer[0], app);

    let body = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(65), Constraint::Percentage(35)])
        .split(outer[1]);

    render_card_list(frame, body[0], app);
    render_score_table(frame, body[1], app);
}

fn render_header(frame: &mut Frame, area: Rect, app: &AppState) {
    let visible = app.topic_questions().len();
    let total   = app.topics.get(app.current_topic).map(|t| t.questions.len()).unwrap_or(0);

    // Right-side status: filter badge + count
    let filter_badge = match &app.filter_mode {
        FilterMode::PendingLetter         => "[f_]".to_string(),
        FilterMode::PendingLevel(src) => {
            let c = match src {
                super::app::FilterSource::Ai   => "a",
                super::app::FilterSource::User => "u",
            };
            format!("[f{}_]", c)
        }
        FilterMode::Normal => {
            if app.filter.is_active() {
                format!("[{}]", app.filter.label())
            } else {
                String::new()
            }
        }
    };

    let count_str = if app.filter.is_active() || app.search.is_some() {
        format!("{}/{}", visible, total)
    } else {
        format!("{}", total)
    };

    let status_str = if filter_badge.is_empty() {
        count_str.clone()
    } else {
        format!("{} {}", filter_badge, count_str)
    };

    let status_style = if app.filter.is_active() {
        Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)
    } else if matches!(app.filter_mode, FilterMode::PendingLetter | FilterMode::PendingLevel(_)) {
        Style::default().fg(Color::Yellow)
    } else {
        Style::default().fg(Color::DarkGray)
    };

    // Line 1: title + right-aligned status
    let title_spans = vec![
        Span::styled("judge", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
        Span::raw("  "),
        Span::styled(&app.candidate.name, Style::default().add_modifier(Modifier::BOLD)),
        Span::raw("  ·  "),
        Span::styled(&app.candidate.role, Style::default().fg(Color::Yellow)),
    ];
    let title_width: usize = "judge".len() + 2 + app.candidate.name.len() + 5 + app.candidate.role.len();
    let status_width = status_str.chars().count();
    let pad = (area.width as usize).saturating_sub(title_width + status_width);
    let mut line1_spans = title_spans;
    line1_spans.push(Span::raw(" ".repeat(pad)));
    line1_spans.push(Span::styled(status_str, status_style));

    // Line 2: keybindings
    let line2 = Line::from(Span::styled(
        "h l ← → topics  j k ↑ ↓ questions  Tab section  0-4 score  f filter  F clear  / ? t search  RR/RU/RA rr/ru/ra random  q quit",
        Style::default().fg(Color::DarkGray),
    ));

    let mut lines = vec![Line::from(line1_spans), line2];

    // Line 3 (search bar, when active)
    if let Some(s) = &app.search {
        let prefix_color = match s.scope {
            SearchScope::InTopic   => Color::Cyan,
            SearchScope::AllTopics => Color::Yellow,
            SearchScope::TopicName => Color::Green,
        };
        let prefix_label = s.prefix_char().to_string();
        let match_str = if s.query.is_empty() {
            String::new()
        } else if s.matches.is_empty() {
            "  no matches".to_string()
        } else {
            format!("  {}/{} matches  Tab/S-Tab navigate  Esc clear", s.cursor + 1, s.matches.len())
        };
        let (match_color, match_style_mod) = if s.matches.is_empty() && !s.query.is_empty() {
            (Color::Red, Modifier::BOLD)
        } else {
            (Color::DarkGray, Modifier::empty())
        };
        let search_line = Line::from(vec![
            Span::styled(prefix_label.clone(), Style::default().fg(prefix_color).add_modifier(Modifier::BOLD)),
            Span::styled(format!(" {}_", s.query), Style::default().fg(Color::White)),
            Span::styled(match_str, Style::default().fg(match_color).add_modifier(match_style_mod)),
        ]);
        lines.push(search_line);
    }

    // Separator line (always last)
    lines.push(Line::from(Span::styled(
        "─".repeat(area.width as usize),
        Style::default().fg(Color::DarkGray),
    )));

    let text = ratatui::text::Text::from(lines);
    frame.render_widget(Paragraph::new(text), area);
}

fn render_card_list(frame: &mut Frame, area: Rect, app: &AppState) {
    let topic_idx = app.current_topic;
    let topic = match app.topics.get(topic_idx) {
        Some(t) => t,
        None => return,
    };

    let visible: Vec<&crate::models::Question> = app.topic_questions();

    if visible.is_empty() {
        let msg = if app.filter.is_active() || app.search.is_some() {
            Paragraph::new("  No questions match. Press F to clear filter or Esc to clear search.")
                .style(Style::default().fg(Color::DarkGray))
        } else {
            Paragraph::new("  No questions loaded for this topic.\n  Run: judge qb gen <topic>")
                .style(Style::default().fg(Color::DarkGray))
        };
        frame.render_widget(msg, area);
        return;
    }

    // Map each visible question to its raw index for search-highlight lookups.
    let raw_indices: Vec<usize> = visible
        .iter()
        .map(|vq| topic.questions.iter().position(|rq| rq.id == vq.id).unwrap_or(0))
        .collect();

    let scroll = app.scroll_offset;
    let total_visible = visible.len();
    let area_bottom = area.y + area.height;
    let mut y = area.y;
    let mut cards_rendered = 0usize;
    let mut last_section: Option<(bool, u8)> = None;

    // Always pin a section separator for the first visible card at y=0, so the
    // user always knows what section they're in even when scrolled.
    if scroll < total_visible {
        let first_q = visible[scroll];
        if y + 1 <= area_bottom {
            render_section_sep(frame, Rect { x: area.x, y, width: area.width, height: 1 }, first_q);
            y += 1;
        }
        last_section = Some((first_q.ai_generated, first_q.level));
    }

    for (v_idx, question) in visible.iter().enumerate().skip(scroll) {
        let sec = (question.ai_generated, question.level);

        if last_section != Some(sec) {
            if y + 1 > area_bottom {
                break;
            }
            render_section_sep(frame, Rect { x: area.x, y, width: area.width, height: 1 }, question);
            y += 1;
            last_section = Some(sec);
        }

        if y + CARD_HEIGHT > area_bottom {
            break;
        }

        let card_area = Rect { x: area.x, y, width: area.width, height: CARD_HEIGHT };
        let is_selected = v_idx == app.current_question;
        let raw_qi = raw_indices[v_idx];
        let is_match = app.is_search_match(topic_idx, raw_qi);
        let is_cursor_match = app.is_current_search_match(topic_idx, raw_qi);
        let score = app.responses.get(&question.id).copied();

        render_card(
            frame,
            card_area,
            question,
            v_idx,
            total_visible,
            score,
            is_selected,
            is_match,
            is_cursor_match,
        );
        y += CARD_HEIGHT;
        cards_rendered += 1;
    }

    app.visible_card_count.set(cards_rendered.max(1));
}

// Single separator line for a question's section. AI questions get a ✦ prefix.
fn render_section_sep(frame: &mut Frame, area: Rect, q: &crate::models::Question) {
    if q.ai_generated {
        render_ai_level_separator(frame, area, q.level);
    } else {
        render_level_separator(frame, area, q.level);
    }
}

fn render_ai_level_separator(frame: &mut Frame, area: Rect, level: u8) {
    let (name, _) = match level {
        1 => ("BASIC", Color::Yellow),
        2 => ("INTERMEDIATE", Color::Cyan),
        3 => ("ADVANCED", Color::Green),
        4 => ("EXPERT", Color::Magenta),
        _ => ("", Color::Gray),
    };
    let label = format!(" ✦ AI {} · L{} ", name, level);
    let width = area.width as usize;
    let lead = "── ";
    let pad = width.saturating_sub(lead.chars().count() + label.chars().count());
    let trail = "─".repeat(pad);
    let style = Style::default().fg(Color::Magenta).add_modifier(Modifier::BOLD);
    let para = Paragraph::new(Line::from(vec![
        Span::styled(lead, Style::default().fg(Color::DarkGray)),
        Span::styled(label, style),
        Span::styled(trail, Style::default().fg(Color::DarkGray)),
    ]));
    frame.render_widget(para, area);
}


fn render_level_separator(frame: &mut Frame, area: Rect, level: u8) {
    let (name, color) = match level {
        1 => ("BASIC", Color::Yellow),
        2 => ("INTERMEDIATE", Color::Cyan),
        3 => ("ADVANCED", Color::Green),
        4 => ("EXPERT", Color::Magenta),
        _ => ("", Color::Gray),
    };
    let label = format!(" {} · L{} ", name, level);
    let width = area.width as usize;
    let lead = "── ";
    let pad = width.saturating_sub(lead.chars().count() + label.chars().count());
    let trail = "─".repeat(pad);
    let style = Style::default().fg(color).add_modifier(Modifier::BOLD);
    let para = Paragraph::new(Line::from(vec![
        Span::styled(lead, Style::default().fg(Color::DarkGray)),
        Span::styled(label, style),
        Span::styled(trail, Style::default().fg(Color::DarkGray)),
    ]));
    frame.render_widget(para, area);
}

#[allow(clippy::too_many_arguments)]
fn render_card(
    frame: &mut Frame,
    area: Rect,
    question: &crate::models::Question,
    v_idx: usize,
    total_visible: usize,
    score: Option<u8>,
    is_selected: bool,
    is_match: bool,
    is_cursor_match: bool,
) {
    let border_style = if is_selected || is_cursor_match {
        Style::default().fg(Color::Cyan)
    } else if is_match {
        Style::default().fg(Color::Yellow)
    } else {
        Style::default().fg(Color::DarkGray)
    };

    let title_style = if is_selected {
        Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(Color::DarkGray)
    };

    let (mark_sym, mark_style) = match score {
        None     => ("  ".to_string(), Style::default()),
        Some(0)  => ("0 ".to_string(), Style::default().fg(Color::Red).add_modifier(Modifier::BOLD)),
        Some(n)  => (format!("{} ", n), Style::default().fg(Color::Green).add_modifier(Modifier::BOLD)),
    };
    let title_text = format!("[L{}] Q{}/{} ", question.level, v_idx + 1, total_visible);
    let mut title_spans = vec![
        Span::raw(" "),
        Span::styled(mark_sym, mark_style),
        Span::styled(title_text, title_style),
    ];
    if question.ai_generated {
        title_spans.push(Span::styled(
            "AI ",
            Style::default().fg(Color::Magenta).add_modifier(Modifier::BOLD),
        ));
    }
    let title_line = Line::from(title_spans);
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(border_style)
        .title(title_line);

    let inner = block.inner(area);
    frame.render_widget(block, area);

    if inner.height < 3 {
        return;
    }

    let inner_w = inner.width as usize;

    let q_text = truncate(&question.text, inner_w);
    let text_style = if is_selected {
        Style::default().add_modifier(Modifier::BOLD)
    } else {
        Style::default()
    };
    let q_line = Paragraph::new(Span::styled(q_text, text_style));
    frame.render_widget(q_line, Rect { x: inner.x, y: inner.y, width: inner.width, height: 1 });

    for i in 0..4 {
        if inner.y + 1 + i as u16 >= inner.y + inner.height {
            break;
        }
        let line = build_keyword_line(i, &question.keywords[i], inner_w);
        frame.render_widget(
            Paragraph::new(line),
            Rect { x: inner.x, y: inner.y + 1 + i as u16, width: inner.width, height: 1 },
        );
    }
}

fn build_keyword_line(level_idx: usize, kws: &[String], width: usize) -> Line<'static> {
    let level_n = (level_idx + 1) as u8;
    let label = format!("[{}]", level_n);
    let style = Style::default().fg(Color::Gray);

    let kw_str = if kws.is_empty() {
        "—".to_string()
    } else {
        kws.join(", ")
    };
    let kw_truncated = truncate(&kw_str, width.saturating_sub(5));

    Line::from(vec![
        Span::styled(label, style),
        Span::styled(format!(" {}", kw_truncated), style),
    ])
}

fn render_score_table(frame: &mut Frame, area: Rect, app: &AppState) {
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::DarkGray))
        .title(Span::styled(" Progress ", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)));

    let inner = block.inner(area);
    frame.render_widget(block, area);

    // 4 columns: topic | User(4) | diff " 0 0 0 0"(8) | "pct% (resp)"(10)
    // spacing: 3 gaps × 1 = 3  →  total fixed = 4 + 8 + 10 + 3 = 25
    let col_w = inner.width.saturating_sub(25).max(4);
    let max_name_len = col_w as usize;
    let col_constraints = [
        Constraint::Min(col_w),
        Constraint::Length(4),
        Constraint::Length(8),
        Constraint::Length(10),
    ];

    // Fixed section: TOTAL row (1) + separator row (1)
    let fixed_height = 2u16;
    let sections = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(fixed_height), Constraint::Min(0)])
        .split(inner);
    let fixed_area = sections[0];
    let scrollable_area = sections[1];

    let ta = app.total_answered();
    let ts = app.total_score();
    let tm = app.total_max();
    let total_user: usize = app.topics.iter()
        .flat_map(|t| t.questions.iter())
        .filter(|q| !q.ai_generated)
        .count();
    let total_diff = diff_str_topics(&app.topics, &app.responses);
    let fixed_rows: Vec<Row> = vec![
        Row::new([
            Cell::from("TOTAL"),
            Cell::from(total_user.to_string()),
            Cell::from(total_diff),
            Cell::from(pct_resp_str(ts, tm, ta)),
        ])
        .style(Style::default().add_modifier(Modifier::BOLD)),
        Row::new(["─────────────", "───", "────────", "─────────"])
            .style(Style::default().fg(Color::DarkGray)),
    ];
    let fixed_table = Table::new(fixed_rows, col_constraints).column_spacing(1);
    frame.render_widget(fixed_table, fixed_area);

    // Render scrollable topic rows
    let visible_height = scrollable_area.height as usize;
    app.visible_topic_count.set(visible_height.max(1));

    let offset = app.topic_table_offset;
    let topic_rows: Vec<Row> = app
        .topics
        .iter()
        .enumerate()
        .skip(offset)
        .take(visible_height)
        .map(|(i, topic)| {
            let answered = topic.answered(&app.responses);
            let user_q   = topic.questions.iter().filter(|q| !q.ai_generated).count();
            let score    = topic.score(&app.responses);
            let max      = topic.max_score_asked(&app.responses);
            let diff     = diff_str_topic(topic, &app.responses);
            let style = if app.topic_search_is_cursor(i) {
                Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)
            } else if app.topic_search_is_match(i) {
                Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)
            } else if i == app.current_topic {
                Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::Gray)
            };
            let name = truncate(&topic.name, max_name_len.max(4));
            Row::new([
                Cell::from(name),
                Cell::from(user_q.to_string()),
                Cell::from(diff),
                Cell::from(pct_resp_str(score, max, answered)),
            ])
            .style(style)
        })
        .collect();

    let scrollable_table = Table::new(topic_rows, col_constraints).column_spacing(1);
    frame.render_widget(scrollable_table, scrollable_area);
}


fn pct_resp_str(score: u32, max: u32, resp: usize) -> String {
    let pct = if max == 0 {
        "  —".to_string()
    } else {
        format!("{:>3}", (score * 100) / max)
    };
    format!("{}% ({})", pct, resp)
}

fn diff_str_topic(topic: &crate::models::TopicData, responses: &std::collections::HashMap<String, u8>) -> String {
    let c: [usize; 4] = std::array::from_fn(|i| {
        topic.questions.iter()
            .filter(|q| q.level == (i + 1) as u8 && responses.contains_key(&q.id))
            .count()
    });
    format!("{:>2}{:>2}{:>2}{:>2}", c[0], c[1], c[2], c[3])
}

fn diff_str_topics(topics: &[crate::models::TopicData], responses: &std::collections::HashMap<String, u8>) -> String {
    let c: [usize; 4] = std::array::from_fn(|i| {
        topics.iter()
            .flat_map(|t| t.questions.iter())
            .filter(|q| q.level == (i + 1) as u8 && responses.contains_key(&q.id))
            .count()
    });
    format!("{:>2}{:>2}{:>2}{:>2}", c[0], c[1], c[2], c[3])
}

fn truncate(s: &str, max: usize) -> String {
    if max == 0 {
        return String::new();
    }
    let chars: Vec<char> = s.chars().collect();
    if chars.len() <= max {
        s.to_string()
    } else {
        let cut: String = chars[..max.saturating_sub(1)].iter().collect();
        format!("{}…", cut)
    }
}

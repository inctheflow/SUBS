use crate::decay::{self, DecayScore};
use crate::ingest::{self, MatchState};
use crate::matcher::{self, Recommendation};
use anyhow::Result;
use crossterm::{
    event::{self, Event, KeyCode},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{
    backend::{Backend, CrosstermBackend},
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, Paragraph},
    Frame, Terminal,
};
use std::{
    collections::HashSet,
    io,
    path::PathBuf,
};

pub struct App {
    pub data_dir: PathBuf,
    pub match_id: String,
    pub team: String,
    pub minute: u32,
    pub scroll: usize,
    pub verbose: bool,
    state: MatchState,
}

impl App {
    pub fn new(
        data_dir: PathBuf,
        match_id: String,
        team: String,
        minute: u32,
        verbose: bool,
    ) -> Result<Self> {
        let state = ingest::load_match(&data_dir, &match_id)?;
        Ok(Self {
            data_dir,
            match_id,
            team,
            minute,
            scroll: 0,
            verbose,
            state,
        })
    }

    fn reload(&mut self) -> Result<()> {
        self.state = ingest::load_match(&self.data_dir, &self.match_id)?;
        Ok(())
    }

    fn opponent(&self) -> &str {
        if self.state.home_lineup.team_name.eq_ignore_ascii_case(&self.team) {
            &self.state.away_lineup.team_name
        } else {
            &self.state.home_lineup.team_name
        }
    }

    fn scores(&self) -> Vec<DecayScore> {
        decay::compute_decay(&self.state, &self.team, self.minute)
    }

    fn subbed_off(&self) -> HashSet<String> {
        self.state
            .events
            .iter()
            .filter(|e| {
                e.event_type == "Substitution"
                    && e.team_name.eq_ignore_ascii_case(&self.team)
                    && e.minute <= self.minute
            })
            .filter_map(|e| e.player_name.clone())
            .collect()
    }

    fn recommendation(&self, scores: &[DecayScore]) -> Option<Recommendation> {
        matcher::recommend(&self.state, &self.team, scores, self.minute, self.verbose)
    }
}

pub fn run(app: App) -> Result<()> {
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let result = run_loop(&mut terminal, app);

    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    terminal.show_cursor()?;

    result
}

fn run_loop<B: Backend>(terminal: &mut Terminal<B>, mut app: App) -> Result<()> {
    loop {
        let scores = app.scores();
        let subbed_off = app.subbed_off();
        let rec = app.recommendation(&scores);

        terminal.draw(|f| ui(f, &app, &scores, &subbed_off, &rec))?;

        if event::poll(std::time::Duration::from_millis(200))? {
            if let Event::Key(key) = event::read()? {
                match key.code {
                    KeyCode::Char('q') | KeyCode::Char('Q') => break,
                    KeyCode::Char('+') | KeyCode::Char('=') => {
                        if app.minute < 120 {
                            app.minute += 1;
                        }
                    }
                    KeyCode::Char('-') => {
                        if app.minute > 1 {
                            app.minute -= 1;
                        }
                    }
                    KeyCode::Char('r') | KeyCode::Char('R') => {
                        let _ = app.reload();
                    }
                    KeyCode::Up => {
                        if app.scroll > 0 {
                            app.scroll -= 1;
                        }
                    }
                    KeyCode::Down => {
                        app.scroll += 1;
                    }
                    _ => {}
                }
            }
        }
    }
    Ok(())
}

fn ui(
    f: &mut Frame<'_>,
    app: &App,
    scores: &[DecayScore],
    subbed_off: &HashSet<String>,
    rec: &Option<Recommendation>,
) {
    let size = f.size();

    let title_area = Rect::new(0, 0, size.width, 2);
    let footer_area = Rect::new(0, size.height.saturating_sub(2), size.width, 2);

    let opponent = app.opponent();
    let title = format!(
        " SUBWATCH • {} vs {} • Minute: {} ",
        app.team, opponent, app.minute
    );
    let title_block = Paragraph::new(title)
        .style(Style::default().fg(Color::White).add_modifier(Modifier::BOLD))
        .block(Block::default().borders(Borders::BOTTOM));
    f.render_widget(title_block, title_area);

    let footer_text = " [q] quit  [+] minute+1  [-] minute-1  [r] reload  [↑↓] scroll ";
    let footer = Paragraph::new(footer_text)
        .style(Style::default().fg(Color::DarkGray))
        .block(Block::default().borders(Borders::TOP));
    f.render_widget(footer, footer_area);

    let content_top = 2u16;
    let content_bottom = size.height.saturating_sub(2);
    if content_bottom <= content_top {
        return;
    }
    let content_area = Rect::new(0, content_top, size.width, content_bottom - content_top);

    let columns = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(55), Constraint::Percentage(45)])
        .split(content_area);

    render_decay(f, app, scores, subbed_off, columns[0]);
    render_recommendation(f, rec, columns[1]);
}

fn bar_for_score(score: f64) -> String {
    let filled = (score * 4.0).round() as usize;
    let filled = filled.min(4);
    let empty = 4 - filled;
    format!("{}{}", "█".repeat(filled), "░".repeat(empty))
}

fn score_color(score: f64) -> Color {
    if score > 0.60 {
        Color::Red
    } else if score >= 0.25 {
        Color::Yellow
    } else {
        Color::Green
    }
}

fn render_decay(
    f: &mut Frame<'_>,
    app: &App,
    scores: &[DecayScore],
    subbed_off: &HashSet<String>,
    area: Rect,
) {
    let block = Block::default()
        .title(" Decay Scores (fatigue) ")
        .borders(Borders::ALL);
    let inner = block.inner(area);
    f.render_widget(block, area);

    // fixed cols: pos(5) + space(1) + score(4) + space(1) + bar(4) + space(1) = 16
    // plus 2 for marker " *" on subbed players — handled in name truncation
    let fixed_cols: u16 = 16;
    let name_width = (inner.width as i32 - fixed_cols as i32).max(8) as usize;

    let header = Line::from(vec![
        Span::styled(
            format!("{:<width$} ", "PLAYER", width = name_width),
            Style::default().add_modifier(Modifier::BOLD),
        ),
        Span::styled("POS   ", Style::default().add_modifier(Modifier::BOLD)),
        Span::styled(" DEC ", Style::default().add_modifier(Modifier::BOLD)),
        Span::styled("BAR ", Style::default().add_modifier(Modifier::BOLD)),
    ]);

    let visible_height = inner.height.saturating_sub(1) as usize;
    let scroll = app.scroll.min(scores.len().saturating_sub(1));

    let mut items: Vec<ListItem> = vec![ListItem::new(header)];

    for score in scores.iter().skip(scroll).take(visible_height) {
        let is_subbed = subbed_off.contains(&score.player_name);
        let marker_len: usize = if is_subbed { 2 } else { 0 };
        let char_limit = name_width.saturating_sub(marker_len);
        let ellipsis_at = char_limit.saturating_sub(1);

        let base: String = if score.player_name.chars().count() > char_limit {
            let cut: String = score.player_name.chars().take(ellipsis_at).collect();
            format!("{}…", cut)
        } else {
            score.player_name.clone()
        };

        let display_name = if is_subbed {
            format!("{} *", base)
        } else {
            base
        };

        let bar = bar_for_score(score.score);
        let text_color = if is_subbed { Color::DarkGray } else { Color::White };
        let score_color = if is_subbed { Color::DarkGray } else { score_color(score.score) };

        let row = Line::from(vec![
            Span::styled(
                format!("{:<width$} ", display_name, width = name_width),
                Style::default().fg(text_color),
            ),
            Span::styled(
                format!("{:<5} ", score.position),
                Style::default().fg(Color::Gray),
            ),
            Span::styled(
                format!("{:>4.2} ", score.score),
                Style::default().fg(score_color),
            ),
            Span::styled(bar, Style::default().fg(score_color)),
        ]);

        items.push(ListItem::new(row));
    }

    let list = List::new(items);
    f.render_widget(list, inner);
}

fn render_recommendation(
    f: &mut Frame<'_>,
    rec: &Option<Recommendation>,
    area: Rect,
) {
    let block = Block::default()
        .title(" Recommendation ")
        .borders(Borders::ALL);
    let inner = block.inner(area);
    f.render_widget(block, area);

    let mut lines: Vec<Line> = vec![];

    match rec {
        None => {
            lines.push(Line::from(Span::styled(
                "No substitution recommended.",
                Style::default().fg(Color::Green),
            )));
            lines.push(Line::from(Span::raw(
                "All players performing at baseline.",
            )));
        }
        Some(r) => {
            lines.push(Line::from(vec![
                Span::styled("TACTICAL PROBLEM: ", Style::default().add_modifier(Modifier::BOLD)),
                Span::styled(
                    r.tactical_problem.as_str(),
                    Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD),
                ),
            ]));

            lines.push(Line::from(Span::raw("")));
            lines.push(Line::from(Span::styled(
                "TAKE OFF:",
                Style::default().add_modifier(Modifier::BOLD).fg(Color::Red),
            )));

            for (i, (name, pos, decay)) in r.take_off.iter().enumerate() {
                lines.push(Line::from(vec![
                    Span::raw(format!("  {}. ", i + 1)),
                    Span::styled(name.as_str(), Style::default().fg(Color::Red)),
                    Span::raw(format!(" ({}) — decay {:.2}", pos, decay)),
                ]));
            }

            lines.push(Line::from(Span::raw("")));
            lines.push(Line::from(Span::styled(
                "BRING ON:",
                Style::default().add_modifier(Modifier::BOLD).fg(Color::Cyan),
            )));

            let candidates: Vec<_> = r.bring_on.iter().take(3).collect();
            for (i, c) in candidates.iter().enumerate() {
                let is_best = i == 0;
                let suffix = if is_best {
                    "  [BEST MATCH]"
                } else if i == 2 {
                    "  (if available)"
                } else {
                    ""
                };

                let name_style = if is_best {
                    Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(Color::Gray)
                };

                lines.push(Line::from(vec![
                    Span::raw(format!("  {}. ", i + 1)),
                    Span::styled(c.player_name.as_str(), name_style),
                    Span::styled(
                        format!(" ({}) — fit {:.1}{}", c.position, c.fit_score, suffix),
                        Style::default().fg(if is_best { Color::Cyan } else { Color::Gray }),
                    ),
                ]));
            }

            if r.bring_on.is_empty() {
                lines.push(Line::from(Span::styled(
                    "  No bench players available.",
                    Style::default().fg(Color::DarkGray),
                )));
            }
        }
    }

    let paragraph = Paragraph::new(lines);
    f.render_widget(paragraph, inner);
}

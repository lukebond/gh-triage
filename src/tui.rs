use std::collections::HashSet;
use std::io;
use std::path::Path;

use chrono::Utc;
use crossterm::event::{self, Event, KeyCode, KeyEventKind};
use crossterm::terminal::{
    disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen,
};
use crossterm::ExecutableCommand;
use ratatui::prelude::*;
use ratatui::widgets::{Block, Borders, List, ListItem, ListState, Paragraph};

use crate::config::Config;
use crate::db::Db;
use crate::types::ItemStatus;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum View {
    Latest,
    ByRepo,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ShowStatus {
    Active,
    Archived,
}

pub fn run_tui(config: &Config, db_path: &Path) -> Result<(), crate::AppError> {
    enable_raw_mode().map_err(|e| crate::AppError::Tui(e.to_string()))?;
    let mut stdout = io::stdout();
    stdout
        .execute(EnterAlternateScreen)
        .map_err(|e| crate::AppError::Tui(e.to_string()))?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend).map_err(|e| crate::AppError::Tui(e.to_string()))?;

    let result = run_tui_loop(config, db_path, &mut terminal);

    disable_raw_mode().ok();
    io::stdout().execute(LeaveAlternateScreen).ok();

    result
}

fn run_tui_loop(
    config: &Config,
    db_path: &Path,
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
) -> Result<(), crate::AppError> {
    let mut view = View::Latest;
    let mut show_status = ShowStatus::Active;
    let mut list_state = ListState::default();
    list_state.select(Some(0));
    let mut seen: HashSet<String> = HashSet::new();

    loop {
        let db = Db::open(db_path)?;
        let status = match show_status {
            ShowStatus::Active => ItemStatus::Active,
            ShowStatus::Archived => ItemStatus::Archived,
        };
        let mut items = db.get_items(status)?;

        // Sort by repo if in ByRepo view
        if view == View::ByRepo {
            items.sort_by(|a, b| a.repo.cmp(&b.repo).then(b.updated_at.cmp(&a.updated_at)));
        }

        // Mark currently selected as seen
        if let Some(idx) = list_state.selected() {
            if let Some(item) = items.get(idx) {
                seen.insert(item.id.clone());
            }
        }

        let org = &config.github_org;
        let item_count = items.len();
        let status_label = match show_status {
            ShowStatus::Active => "active",
            ShowStatus::Archived => "archived",
        };

        terminal
            .draw(|frame| {
                let area = frame.area();

                let chunks = Layout::default()
                    .direction(Direction::Vertical)
                    .constraints([
                        Constraint::Length(1), // header
                        Constraint::Length(1), // view tabs
                        Constraint::Min(1),    // list
                    ])
                    .split(area);

                // Header
                let header = Paragraph::new(Line::from(vec![
                    Span::styled("gh-triage", Style::default().fg(Color::Green).bold()),
                    Span::raw(format!(
                        "          [{status_label}: {item_count}]    org: {org}    q quit"
                    )),
                ]));
                frame.render_widget(header, chunks[0]);

                // View tabs
                let tabs = Paragraph::new(Line::from(vec![
                    if view == View::Latest {
                        Span::styled("[latest]", Style::default().fg(Color::Yellow).bold())
                    } else {
                        Span::raw("[latest]")
                    },
                    Span::raw(" "),
                    if view == View::ByRepo {
                        Span::styled("[by repo]", Style::default().fg(Color::Yellow).bold())
                    } else {
                        Span::raw("[by repo]")
                    },
                    Span::raw("                          "),
                    if show_status == ShowStatus::Archived {
                        Span::styled("[A archived]", Style::default().fg(Color::Yellow).bold())
                    } else {
                        Span::raw("[a archived]")
                    },
                ]));
                frame.render_widget(tabs, chunks[1]);

                // Item list
                let list_items: Vec<ListItem> = items
                    .iter()
                    .map(|item| {
                        let is_seen = seen.contains(&item.id);
                        let indicator = if is_seen { " " } else { "●" };
                        let type_label = item.item_type.short_label();
                        let repo_short = item.repo.split('/').nth(1).unwrap_or(&item.repo);
                        let reason_display = match item.reason.as_str() {
                            "review_requested" => "Review requested",
                            "assigned" => "Assigned",
                            "mentioned" => "Mentioned",
                            "all" => "All",
                            other => other,
                        };

                        let reason_color = match item.reason.as_str() {
                            "review_requested" => Color::Yellow,
                            "assigned" => Color::Cyan,
                            _ => Color::White,
                        };

                        let indicator_style = if is_seen {
                            Style::default()
                        } else {
                            Style::default().fg(Color::Red).bold()
                        };

                        let title_truncated: String = item.title.chars().take(50).collect();

                        let line1 = Line::from(vec![
                            Span::styled(indicator, indicator_style),
                            Span::raw(" "),
                            Span::styled(type_label, Style::default().fg(Color::Blue)),
                            Span::raw("  "),
                            Span::styled(
                                format!("{:<16}", repo_short),
                                Style::default().fg(Color::Magenta),
                            ),
                            Span::styled(
                                format!("{:<18}", reason_display),
                                Style::default().fg(reason_color),
                            ),
                            Span::raw(title_truncated),
                        ]);

                        let summary_text = item.summary.as_deref().unwrap_or("Fetching summary...");
                        let relative_time = format_relative_time(item.updated_at);

                        let meta_line = Line::from(vec![
                            Span::raw("  "),
                            Span::styled(&item.author, Style::default().fg(Color::DarkGray)),
                            Span::raw(" • "),
                            Span::styled(relative_time, Style::default().fg(Color::DarkGray)),
                        ]);

                        // Wrap summary across as many lines as needed
                        let summary_style = Style::default().fg(Color::DarkGray);
                        let summary_lines: Vec<Line> =
                            wrap_text(summary_text, area.width.saturating_sub(4) as usize)
                                .into_iter()
                                .map(|s| {
                                    Line::from(vec![
                                        Span::raw("  "),
                                        Span::styled(s, summary_style),
                                    ])
                                })
                                .collect();

                        let mut lines = vec![line1, meta_line];
                        lines.extend(summary_lines);
                        lines.push(Line::raw(""));

                        ListItem::new(lines)
                    })
                    .collect();

                let list = List::new(list_items)
                    .block(Block::default().borders(Borders::TOP))
                    .highlight_style(
                        Style::default()
                            .bg(Color::Indexed(236))
                            .add_modifier(Modifier::BOLD),
                    );

                frame.render_stateful_widget(list, chunks[2], &mut list_state);
            })
            .map_err(|e| crate::AppError::Tui(e.to_string()))?;

        // Handle input
        if event::poll(std::time::Duration::from_millis(250))
            .map_err(|e| crate::AppError::Tui(e.to_string()))?
        {
            if let Event::Key(key) =
                event::read().map_err(|e| crate::AppError::Tui(e.to_string()))?
            {
                if key.kind != KeyEventKind::Press {
                    continue;
                }
                match key.code {
                    KeyCode::Char('q') => break,
                    KeyCode::Char('j') | KeyCode::Down => {
                        let i = list_state.selected().unwrap_or(0);
                        if i + 1 < item_count {
                            list_state.select(Some(i + 1));
                        }
                    }
                    KeyCode::Char('k') | KeyCode::Up => {
                        let i = list_state.selected().unwrap_or(0);
                        if i > 0 {
                            list_state.select(Some(i - 1));
                        }
                    }
                    KeyCode::Enter => {
                        // Both "browser" and "preview" open in browser for v1
                        let _ = &config.enter_action;
                        if let Some(idx) = list_state.selected() {
                            let db = Db::open(db_path)?;
                            let items = db.get_items(status)?;
                            if let Some(item) = items.get(idx) {
                                let _ = open::that(&item.url);
                            }
                        }
                    }
                    KeyCode::Char('a') => {
                        if show_status == ShowStatus::Active {
                            if let Some(idx) = list_state.selected() {
                                let db = Db::open(db_path)?;
                                let items_list = db.get_items(ItemStatus::Active)?;
                                if let Some(item) = items_list.get(idx) {
                                    let _ = db.archive_item(&item.id);
                                }
                                // Adjust selection if needed
                                if idx > 0 && idx >= item_count.saturating_sub(1) {
                                    list_state.select(Some(idx.saturating_sub(1)));
                                }
                            }
                        }
                    }
                    KeyCode::Char('A') => {
                        show_status = match show_status {
                            ShowStatus::Active => ShowStatus::Archived,
                            ShowStatus::Archived => ShowStatus::Active,
                        };
                        list_state.select(Some(0));
                    }
                    KeyCode::Tab => {
                        view = match view {
                            View::Latest => View::ByRepo,
                            View::ByRepo => View::Latest,
                        };
                    }
                    KeyCode::Char('R') => {
                        // Refresh — just re-render on next loop iteration
                        // In a full implementation this would trigger a poll
                    }
                    _ => {}
                }
            }
        }
    }

    Ok(())
}

fn wrap_text(text: &str, width: usize) -> Vec<String> {
    if width == 0 {
        return vec![text.to_string()];
    }
    let mut lines = Vec::new();
    let mut remaining = text;
    while !remaining.is_empty() {
        if remaining.len() <= width {
            lines.push(remaining.to_string());
            break;
        }
        // Try to break at a space
        let break_at = remaining[..width].rfind(' ').unwrap_or(width);
        lines.push(remaining[..break_at].to_string());
        remaining = remaining[break_at..].trim_start();
    }
    if lines.is_empty() {
        lines.push(String::new());
    }
    lines
}

fn format_relative_time(dt: chrono::DateTime<Utc>) -> String {
    let now = Utc::now();
    let diff = now.signed_duration_since(dt);

    if diff.num_days() > 0 {
        format!("{}d ago", diff.num_days())
    } else if diff.num_hours() > 0 {
        format!("{}h ago", diff.num_hours())
    } else if diff.num_minutes() > 0 {
        format!("{}m ago", diff.num_minutes())
    } else {
        "just now".to_string()
    }
}

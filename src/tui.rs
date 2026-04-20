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
use ratatui::widgets::{Block, Borders, Clear, List, ListItem, ListState, Paragraph};

use crate::config::Config;
use crate::db::Db;
use crate::types::{Item, ItemStatus};

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

/// Each row in the rendered list is either a repo group header or an item.
#[derive(Debug, Clone)]
enum Row {
    Header(String),
    Item(usize), // index into the items vec
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
    _config: &Config,
    db_path: &Path,
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
) -> Result<(), crate::AppError> {
    let mut view = View::Latest;
    let mut show_status = ShowStatus::Active;
    let mut list_state = ListState::default();
    list_state.select(Some(0));
    let mut seen: HashSet<String> = HashSet::new();
    let mut show_help = false;
    let db = Db::open(db_path)?;

    loop {
        let status = match show_status {
            ShowStatus::Active => ItemStatus::Active,
            ShowStatus::Archived => ItemStatus::Archived,
        };
        let mut items = db.get_items(status)?;

        // Sort by repo if in ByRepo view
        if view == View::ByRepo {
            items.sort_by(|a, b| a.repo.cmp(&b.repo).then(b.updated_at.cmp(&a.updated_at)));
        }

        // Build row mapping
        let rows = build_rows(&items, view);
        let row_count = rows.len();

        // Mark currently selected item as seen
        if let Some(idx) = list_state.selected() {
            if let Some(Row::Item(item_idx)) = rows.get(idx) {
                if let Some(item) = items.get(*item_idx) {
                    seen.insert(item.id.clone());
                }
            }
        }

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
                        "          [{status_label}: {item_count}]    ? help    q quit"
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

                // Build list items from rows
                let list_items: Vec<ListItem> = rows
                    .iter()
                    .map(|row| match row {
                        Row::Header(repo) => {
                            let repo_short = repo.split('/').nth(1).unwrap_or(repo);
                            ListItem::new(vec![
                                Line::raw(""),
                                Line::from(Span::styled(
                                    format!("── {repo_short} ──"),
                                    Style::default()
                                        .fg(Color::Magenta)
                                        .add_modifier(Modifier::BOLD),
                                )),
                            ])
                        }
                        Row::Item(item_idx) => render_item(&items[*item_idx], &seen, area.width),
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

                // Help modal
                if show_help {
                    let help_text = vec![
                        Line::from(vec![
                            Span::styled("j", Style::default().fg(Color::Yellow).bold()),
                            Span::raw(" / "),
                            Span::styled("↓", Style::default().fg(Color::Yellow).bold()),
                            Span::raw("          Move down"),
                        ]),
                        Line::from(vec![
                            Span::styled("k", Style::default().fg(Color::Yellow).bold()),
                            Span::raw(" / "),
                            Span::styled("↑", Style::default().fg(Color::Yellow).bold()),
                            Span::raw("          Move up"),
                        ]),
                        Line::from(vec![
                            Span::styled("Enter", Style::default().fg(Color::Yellow).bold()),
                            Span::raw("          Open in browser"),
                        ]),
                        Line::from(vec![
                            Span::styled("a", Style::default().fg(Color::Yellow).bold()),
                            Span::raw("              Archive item"),
                        ]),
                        Line::from(vec![
                            Span::styled("A", Style::default().fg(Color::Yellow).bold()),
                            Span::raw("              Toggle archived view"),
                        ]),
                        Line::from(vec![
                            Span::styled("Tab", Style::default().fg(Color::Yellow).bold()),
                            Span::raw("            Toggle latest / by repo"),
                        ]),
                        Line::from(vec![
                            Span::styled("g", Style::default().fg(Color::Yellow).bold()),
                            Span::raw("              Go to first item"),
                        ]),
                        Line::from(vec![
                            Span::styled("G", Style::default().fg(Color::Yellow).bold()),
                            Span::raw("              Go to last item"),
                        ]),
                        Line::from(vec![
                            Span::styled("z", Style::default().fg(Color::Yellow).bold()),
                            Span::raw("              Centre view on selection"),
                        ]),
                        Line::from(vec![
                            Span::styled("R", Style::default().fg(Color::Yellow).bold()),
                            Span::raw("              Refresh"),
                        ]),
                        Line::from(vec![
                            Span::styled("?", Style::default().fg(Color::Yellow).bold()),
                            Span::raw("              Toggle this help"),
                        ]),
                        Line::from(vec![
                            Span::styled("q", Style::default().fg(Color::Yellow).bold()),
                            Span::raw("              Quit"),
                        ]),
                    ];

                    let help_width = 42u16;
                    let help_height = (help_text.len() as u16) + 2; // +2 for borders
                    let x = area.width.saturating_sub(help_width) / 2;
                    let y = area.height.saturating_sub(help_height) / 2;
                    let help_area = Rect::new(x, y, help_width, help_height);

                    frame.render_widget(Clear, help_area);
                    let help_block = Paragraph::new(help_text).block(
                        Block::default()
                            .borders(Borders::ALL)
                            .title(" Keybindings ")
                            .title_alignment(Alignment::Center)
                            .border_style(Style::default().fg(Color::Green)),
                    );
                    frame.render_widget(help_block, help_area);
                }
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
                if show_help {
                    show_help = false;
                    continue;
                }
                // TODO: define keybindings as a shared struct so the match arms
                // and the help modal are driven from the same source.
                match key.code {
                    KeyCode::Char('q') => break,
                    KeyCode::Char('j') | KeyCode::Down => {
                        let i = list_state.selected().unwrap_or(0);
                        // Skip forward past any headers
                        let next = next_item_row(&rows, i, 1);
                        list_state.select(Some(next));
                    }
                    KeyCode::Char('k') | KeyCode::Up => {
                        let i = list_state.selected().unwrap_or(0);
                        let prev = next_item_row(&rows, i, -1);
                        list_state.select(Some(prev));
                    }
                    KeyCode::Enter => {
                        if let Some(idx) = list_state.selected() {
                            if let Some(Row::Item(item_idx)) = rows.get(idx) {
                                if let Some(item) = items.get(*item_idx) {
                                    let _ = open::that(&item.url);
                                }
                            }
                        }
                    }
                    KeyCode::Char('a') => {
                        if show_status == ShowStatus::Active {
                            if let Some(idx) = list_state.selected() {
                                if let Some(Row::Item(item_idx)) = rows.get(idx) {
                                    if let Some(item) = items.get(*item_idx) {
                                        let _ = db.archive_item(&item.id);
                                    }
                                }
                                // Stay at same index; the list will be one shorter
                                // on next render so clamp to avoid going past the end.
                                // row_count - 1 because we just removed one item.
                                let max = row_count.saturating_sub(2);
                                list_state.select(Some(idx.min(max)));
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
                        list_state.select(Some(0));
                    }
                    KeyCode::Char('g') => {
                        // Jump to first item
                        let first = next_item_row(&rows, 0, 0);
                        list_state.select(Some(first));
                    }
                    KeyCode::Char('G') => {
                        // Jump to last item
                        if row_count > 0 {
                            let last = next_item_row(&rows, row_count - 1, 0);
                            list_state.select(Some(last));
                        }
                    }
                    KeyCode::Char('z') => {
                        // Centre the view on the current selection
                        if let Some(idx) = list_state.selected() {
                            // Each item takes multiple lines; estimate visible rows
                            // by using the list area height (chunks[2] from layout).
                            // We approximate with terminal height minus header (2 lines + border).
                            let visible = terminal
                                .size()
                                .map(|s| s.height.saturating_sub(4) as usize)
                                .unwrap_or(20);
                            let half = visible / 2;
                            let new_offset = idx.saturating_sub(half);
                            *list_state.offset_mut() = new_offset;
                        }
                    }
                    KeyCode::Char('R') => {
                        // Refresh — just re-render on next loop iteration
                    }
                    KeyCode::Char('?') => {
                        show_help = true;
                    }
                    _ => {}
                }
            }
        }
    }

    Ok(())
}

/// Build the flat list of rows. In Latest view, it's just items.
/// In ByRepo view, insert a header before each new repo group.
fn build_rows(items: &[Item], view: View) -> Vec<Row> {
    let mut rows = Vec::new();
    match view {
        View::Latest => {
            rows.extend((0..items.len()).map(Row::Item));
        }
        View::ByRepo => {
            let mut last_repo: Option<&str> = None;
            for (i, item) in items.iter().enumerate() {
                if last_repo != Some(&item.repo) {
                    rows.push(Row::Header(item.repo.clone()));
                    last_repo = Some(&item.repo);
                }
                rows.push(Row::Item(i));
            }
        }
    }
    rows
}

/// Find the next Item row in direction (1 = down, -1 = up, 0 = at or after current), skipping headers.
fn next_item_row(rows: &[Row], current: usize, direction: i32) -> usize {
    // direction 0: find the nearest item at current position or forward
    if direction == 0 {
        if current < rows.len() && matches!(rows[current], Row::Item(_)) {
            return current;
        }
        // Try forward then backward
        return next_item_row(rows, current, 1);
    }
    let mut i = current as i32 + direction;
    while i >= 0 && (i as usize) < rows.len() {
        if matches!(rows[i as usize], Row::Item(_)) {
            return i as usize;
        }
        i += direction;
    }
    current // stay put if nothing found
}

fn render_item<'a>(item: &'a Item, seen: &HashSet<String>, term_width: u16) -> ListItem<'a> {
    let is_seen = seen.contains(&item.id);
    let indicator = if is_seen { " " } else { "●" };
    let type_label = item.item_type.short_label();
    let repo_short = item.repo_short();
    let reason_display = item.reason_label();

    let reason_color = match item.reason.as_str() {
        "review_requested" => Color::Yellow,
        "assigned" => Color::Cyan,
        "authored" => Color::Green,
        _ => Color::White,
    };

    let indicator_style = if is_seen {
        Style::default()
    } else {
        Style::default().fg(Color::Red).bold()
    };

    let number = item.url.rsplit('/').next().unwrap_or("");
    let type_and_num = format!("{} #{:<5}", type_label, number);
    let title_truncated: String = item.title.chars().take(50).collect();

    let line1 = Line::from(vec![
        Span::styled(indicator, indicator_style),
        Span::raw(" "),
        Span::styled(type_and_num, Style::default().fg(Color::Blue)),
        Span::raw(" "),
        Span::styled(
            format!("{:<17}", repo_short),
            Style::default().fg(Color::Magenta),
        ),
        Span::styled(
            format!("{:<19}", reason_display),
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
    let summary_lines: Vec<Line> = wrap_text(summary_text, term_width.saturating_sub(4) as usize)
        .into_iter()
        .map(|s| Line::from(vec![Span::raw("  "), Span::styled(s, summary_style)]))
        .collect();

    let mut lines = vec![line1, meta_line];
    lines.extend(summary_lines);
    lines.push(Line::raw(""));

    ListItem::new(lines)
}

fn wrap_text(text: &str, width: usize) -> Vec<String> {
    if width == 0 {
        return vec![text.to_string()];
    }
    let mut lines = Vec::new();
    let mut remaining = text;
    while !remaining.is_empty() {
        let char_count: usize = remaining.chars().count();
        if char_count <= width {
            lines.push(remaining.to_string());
            break;
        }
        // Find the byte offset of the char at position `width`
        let byte_limit = remaining
            .char_indices()
            .nth(width)
            .map(|(i, _)| i)
            .unwrap_or(remaining.len());
        // Try to break at a space within that range
        let break_at = remaining[..byte_limit].rfind(' ').unwrap_or(byte_limit);
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wrap_text_ascii() {
        let lines = wrap_text("hello world foo bar", 12);
        assert_eq!(lines, vec!["hello world", "foo bar"]);
    }

    #[test]
    fn wrap_text_short_enough() {
        let lines = wrap_text("short", 20);
        assert_eq!(lines, vec!["short"]);
    }

    #[test]
    fn wrap_text_multibyte_emdash() {
        // "—" is 3 bytes in UTF-8; wrapping must not split it
        let text = "no urgent action needed beyond standard code review — this is fine";
        let lines = wrap_text(text, 55);
        assert_eq!(lines.len(), 2);
        assert!(!lines[0].is_empty());
        assert!(!lines[1].is_empty());
    }

    #[test]
    fn wrap_text_empty() {
        let lines = wrap_text("", 40);
        assert_eq!(lines, vec![""]);
    }
}

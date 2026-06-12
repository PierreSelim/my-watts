use crossterm::{
    cursor,
    event::{self, Event, KeyCode},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{
    backend::CrosstermBackend,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    widgets::{Block, Borders, Paragraph, Row, Table, TableState},
    Frame, Terminal,
};
use std::io::Stdout;

use crate::{fmt_hhmmss, index::RideEntry, GpsAnalyzerError};

/// What the user asked for when leaving the list screen.
pub enum ListOutcome {
    Quit,
    /// Re-open the plot for the ride at this index.
    Replay(usize),
}

const HEADERS: [&str; 8] = [
    "Date", "Ride", "Dist km", "Elapsed", "Moving", "km/h", "Watts", "Elev m",
];

const COLUMN_WIDTHS: [u16; 8] = [16, 26, 8, 9, 9, 7, 7, 7];

fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else {
        let kept: String = s.chars().take(max.saturating_sub(1)).collect();
        format!("{kept}…")
    }
}

/// The eight display cells for one ride, in column order.
pub fn format_row(entry: &RideEntry) -> [String; 8] {
    [
        entry.start_timestamp.format("%Y-%m-%d %H:%M").to_string(),
        truncate(&entry.stem, 25),
        format!("{:.1}", entry.distance_km),
        fmt_hhmmss(entry.elapsed_secs),
        fmt_hhmmss(entry.moving_secs),
        format!("{:.1}", entry.moving_avg_speed_kmh),
        format!("{:.0}", entry.avg_power_watts),
        format!("{:.0}", entry.total_elevation_gain_m),
    ]
}

fn select_next(state: &mut TableState, len: usize) {
    if len == 0 {
        return;
    }
    let next = match state.selected() {
        Some(i) if i + 1 < len => i + 1,
        Some(i) => i,
        None => 0,
    };
    state.select(Some(next));
}

fn select_prev(state: &mut TableState, len: usize) {
    if len == 0 {
        return;
    }
    let prev = match state.selected() {
        Some(i) => i.saturating_sub(1),
        None => 0,
    };
    state.select(Some(prev));
}

struct TerminalGuard;

impl Drop for TerminalGuard {
    fn drop(&mut self) {
        let _ = disable_raw_mode();
        let _ = execute!(std::io::stdout(), LeaveAlternateScreen, cursor::Show);
    }
}

/// Run the interactive ride list, starting with `initial_selected` highlighted and an optional
/// `status` line (used to surface a replay error). Returns the user's choice on exit; the terminal
/// is always restored, including on I/O errors.
pub fn run_list_tui(
    entries: &[RideEntry],
    initial_selected: usize,
    status: Option<&str>,
) -> Result<ListOutcome, GpsAnalyzerError> {
    enable_raw_mode()?;
    let mut stdout = std::io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let _guard = TerminalGuard;

    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    event_loop(&mut terminal, entries, initial_selected, status)
}

fn event_loop(
    terminal: &mut Terminal<CrosstermBackend<Stdout>>,
    entries: &[RideEntry],
    initial_selected: usize,
    status: Option<&str>,
) -> Result<ListOutcome, GpsAnalyzerError> {
    let mut state = TableState::default();
    if !entries.is_empty() {
        state.select(Some(initial_selected.min(entries.len() - 1)));
    }

    loop {
        terminal.draw(|frame| draw(frame, entries, status, &mut state))?;

        if event::poll(std::time::Duration::from_millis(200))? {
            if let Event::Key(key) = event::read()? {
                match key.code {
                    KeyCode::Char('q') | KeyCode::Esc => return Ok(ListOutcome::Quit),
                    KeyCode::Down | KeyCode::Char('j') => select_next(&mut state, entries.len()),
                    KeyCode::Up | KeyCode::Char('k') => select_prev(&mut state, entries.len()),
                    KeyCode::Enter => {
                        if let Some(i) = state.selected() {
                            return Ok(ListOutcome::Replay(i));
                        }
                    }
                    _ => {}
                }
            }
        }
    }
}

fn draw(frame: &mut Frame, entries: &[RideEntry], status: Option<&str>, state: &mut TableState) {
    let constraints = match status {
        Some(_) => vec![Constraint::Min(0), Constraint::Length(1)],
        None => vec![Constraint::Min(0)],
    };
    let areas = Layout::default()
        .direction(Direction::Vertical)
        .constraints(constraints)
        .split(frame.area());

    draw_table(frame, areas[0], entries, state);

    if let Some(msg) = status {
        let paragraph = Paragraph::new(msg).style(Style::default().fg(Color::Red));
        frame.render_widget(paragraph, areas[1]);
    }
}

fn draw_table(frame: &mut Frame, area: Rect, entries: &[RideEntry], state: &mut TableState) {
    let header = Row::new(HEADERS).style(Style::default().add_modifier(Modifier::BOLD));
    let rows = entries.iter().map(|e| Row::new(format_row(e)));
    let widths = COLUMN_WIDTHS.map(Constraint::Length);

    let title = format!("Rides ({}) — ↑↓ navigate · ⏎ open · q quit", entries.len());
    let table = Table::new(rows, widths)
        .header(header)
        .block(Block::default().borders(Borders::ALL).title(title))
        .row_highlight_style(Style::default().add_modifier(Modifier::REVERSED))
        .highlight_symbol("▌ ");

    frame.render_stateful_widget(table, area, state);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::index::ReplayParams;
    use chrono::{TimeZone, Utc};
    use std::path::PathBuf;

    fn entry(stem: &str) -> RideEntry {
        RideEntry {
            stem: stem.to_string(),
            source_gpx_path: PathBuf::from(format!("{stem}.gpx")),
            analyze_csv_path: PathBuf::from(format!("{stem}.analyze.csv")),
            intervals_csv_path: PathBuf::from(format!("{stem}.intervals.csv")),
            start_timestamp: Utc.with_ymd_and_hms(2024, 6, 1, 8, 30, 0).unwrap(),
            indexed_at: Utc.with_ymd_and_hms(2026, 6, 12, 0, 0, 0).unwrap(),
            distance_km: 42.34,
            elapsed_secs: 7510.0,
            moving_secs: 7110.0,
            moving_avg_speed_kmh: 21.4,
            avg_power_watts: 184.6,
            total_calories_kcal: 1240.0,
            total_elevation_gain_m: 820.0,
            replay: ReplayParams {
                rider_weight_kg: 75.0,
                bike_weight_kg: 10.0,
                bike_name: "road".to_string(),
                config_path: None,
                window_size: 5,
                degree: 2,
                smooth_window: 5,
                stop_buffer_secs: 10.0,
            },
        }
    }

    #[test]
    fn test_format_row_columns() {
        let row = format_row(&entry("my_ride"));
        assert_eq!(row[0], "2024-06-01 08:30");
        assert_eq!(row[1], "my_ride");
        assert_eq!(row[2], "42.3");
        assert_eq!(row[3], "02:05:10");
        assert_eq!(row[4], "01:58:30");
        assert_eq!(row[5], "21.4");
        assert_eq!(row[6], "185");
        assert_eq!(row[7], "820");
    }

    #[test]
    fn test_truncate_short_unchanged() {
        assert_eq!(truncate("ride", 25), "ride");
    }

    #[test]
    fn test_truncate_long_gets_ellipsis() {
        let long = "a-really-long-ride-name-that-overflows";
        let out = truncate(long, 10);
        assert_eq!(out.chars().count(), 10);
        assert!(out.ends_with('…'));
    }

    #[test]
    fn test_format_row_truncates_long_stem() {
        let row = format_row(&entry("a-really-long-ride-name-that-overflows-badly"));
        assert!(row[1].chars().count() <= 25);
    }

    #[test]
    fn test_select_next_advances_and_clamps() {
        let mut state = TableState::default();
        state.select(Some(0));
        select_next(&mut state, 3);
        assert_eq!(state.selected(), Some(1));
        select_next(&mut state, 3);
        select_next(&mut state, 3);
        assert_eq!(state.selected(), Some(2)); // clamped at last
    }

    #[test]
    fn test_select_prev_decrements_and_clamps() {
        let mut state = TableState::default();
        state.select(Some(1));
        select_prev(&mut state, 3);
        assert_eq!(state.selected(), Some(0));
        select_prev(&mut state, 3);
        assert_eq!(state.selected(), Some(0)); // clamped at first
    }

    #[test]
    fn test_select_next_on_empty_is_noop() {
        let mut state = TableState::default();
        select_next(&mut state, 0);
        assert_eq!(state.selected(), None);
    }

    #[test]
    fn test_draw_does_not_panic() {
        use ratatui::{backend::TestBackend, Terminal};
        let entries = vec![entry("ride-a"), entry("ride-b")];
        let mut state = TableState::default();
        state.select(Some(0));
        let backend = TestBackend::new(120, 20);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|f| draw(f, &entries, None, &mut state))
            .unwrap();
    }

    #[test]
    fn test_draw_with_status_does_not_panic() {
        use ratatui::{backend::TestBackend, Terminal};
        let entries = vec![entry("ride-a")];
        let mut state = TableState::default();
        state.select(Some(0));
        let backend = TestBackend::new(120, 20);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|f| draw(f, &entries, Some("Cannot open ride"), &mut state))
            .unwrap();
    }

    #[test]
    fn test_draw_empty_does_not_panic() {
        use ratatui::{backend::TestBackend, Terminal};
        let mut state = TableState::default();
        let backend = TestBackend::new(120, 20);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal.draw(|f| draw(f, &[], None, &mut state)).unwrap();
    }
}

use crossterm::{
    cursor,
    event::{self, Event, KeyCode},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{
    backend::CrosstermBackend,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Style},
    symbols::Marker,
    text::Span,
    widgets::{Axis, Block, Borders, Chart, Dataset, Paragraph},
    Frame, Terminal,
};
use std::io::Stdout;

use crate::{AnalyzePoint, GpsAnalyzerError};

#[derive(Debug, Clone)]
pub struct RideSummary {
    pub total_distance_km: f64,
    pub elapsed_secs: f64,
    pub moving_secs: f64,
    pub avg_speed_kmh: f64,
    pub avg_power_watts: Option<f64>,
    pub total_elevation_gain_m: Option<f64>,
}

#[derive(Debug, Clone)]
pub struct PlotData {
    pub power_series: Vec<(f64, f64)>,
    pub average_power_series: Vec<(f64, f64)>,
    pub instant_speed_series: Vec<(f64, f64)>,
    pub average_speed_series: Vec<(f64, f64)>,
    pub altitude_series: Vec<(f64, f64)>,
    pub time_bounds: [f64; 2],
    pub power_bounds: [f64; 2],
    pub speed_bounds: [f64; 2],
    pub altitude_bounds: [f64; 2],
    pub summary: RideSummary,
}

pub fn compute_y_bounds(series: &[(f64, f64)], step: f64) -> [f64; 2] {
    let max_y = series.iter().map(|(_, y)| *y).fold(0.0_f64, f64::max);
    let rounded_max = (max_y / step).ceil() * step;
    [0.0, rounded_max.max(step)]
}

pub fn compute_altitude_bounds(series: &[(f64, f64)], step: f64) -> [f64; 2] {
    if series.is_empty() {
        return [0.0, step];
    }
    let min_y = series.iter().map(|(_, y)| *y).fold(f64::INFINITY, f64::min);
    let max_y = series
        .iter()
        .map(|(_, y)| *y)
        .fold(f64::NEG_INFINITY, f64::max);
    let lo = (min_y / step).floor() * step;
    let hi = (max_y / step).ceil() * step;
    [lo, hi.max(lo + step)]
}

pub fn build_plot_data(points: &[AnalyzePoint]) -> PlotData {
    let power_series: Vec<(f64, f64)> = points
        .iter()
        .map(|p| (p.seconds_from_start, p.power_smooth_watts.unwrap_or(0.0)))
        .collect();

    let average_power_series: Vec<(f64, f64)> = points
        .iter()
        .map(|p| {
            let avg = match p.cumulative_energy_kj {
                Some(kj) if p.moving_seconds_from_start > 0.0 => {
                    kj * 1000.0 / p.moving_seconds_from_start
                }
                _ => 0.0,
            };
            (p.seconds_from_start, avg)
        })
        .collect();

    let instant_speed_series: Vec<(f64, f64)> = points
        .iter()
        .map(|p| (p.seconds_from_start, p.instant_speed_kmh))
        .collect();

    let average_speed_series: Vec<(f64, f64)> = points
        .iter()
        .map(|p| (p.seconds_from_start, p.average_speed_kmh))
        .collect();

    let altitude_series: Vec<(f64, f64)> = points
        .iter()
        .filter_map(|p| p.smoothed_alt.map(|a| (p.seconds_from_start, a)))
        .collect();

    let max_time = points.last().map(|p| p.seconds_from_start).unwrap_or(0.0);
    let time_bounds = [0.0, max_time];
    let power_bounds = compute_y_bounds(&power_series, 50.0);
    let speed_bounds = compute_y_bounds(&instant_speed_series, 5.0);
    let altitude_bounds = compute_altitude_bounds(&altitude_series, 50.0);

    let total_distance_km = points.last().map(|p| p.distance_km).unwrap_or(0.0);
    let elapsed_secs = max_time;
    let moving_secs = points
        .last()
        .map(|p| p.moving_seconds_from_start)
        .unwrap_or(0.0);
    let avg_speed_kmh = points.last().map(|p| p.average_speed_kmh).unwrap_or(0.0);

    let avg_power_watts = points.last().and_then(|last| {
        let kj = last.cumulative_energy_kj?;
        if last.moving_seconds_from_start > 0.0 {
            Some(kj * 1000.0 / last.moving_seconds_from_start)
        } else {
            None
        }
    });

    let total_elevation_gain_m = if altitude_series.is_empty() {
        None
    } else {
        let gain = altitude_series
            .windows(2)
            .map(|w| (w[1].1 - w[0].1).max(0.0))
            .sum::<f64>();
        Some(gain)
    };

    PlotData {
        power_series,
        average_power_series,
        instant_speed_series,
        average_speed_series,
        altitude_series,
        time_bounds,
        power_bounds,
        speed_bounds,
        altitude_bounds,
        summary: RideSummary {
            total_distance_km,
            elapsed_secs,
            moving_secs,
            avg_speed_kmh,
            avg_power_watts,
            total_elevation_gain_m,
        },
    }
}

struct TerminalGuard;

impl Drop for TerminalGuard {
    fn drop(&mut self) {
        let _ = disable_raw_mode();
        let _ = execute!(std::io::stdout(), LeaveAlternateScreen, cursor::Show);
    }
}

pub fn run_tui(data: &PlotData) -> Result<(), GpsAnalyzerError> {
    enable_raw_mode()?;
    let mut stdout = std::io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let _guard = TerminalGuard;

    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    event_loop(&mut terminal, data)
}

fn event_loop(
    terminal: &mut Terminal<CrosstermBackend<Stdout>>,
    data: &PlotData,
) -> Result<(), GpsAnalyzerError> {
    loop {
        terminal.draw(|frame| draw(frame, data))?;

        if event::poll(std::time::Duration::from_millis(200))? {
            if let Event::Key(key) = event::read()? {
                match key.code {
                    KeyCode::Char('q') | KeyCode::Esc => return Ok(()),
                    _ => {}
                }
            }
        }
    }
}

fn draw(frame: &mut Frame, data: &PlotData) {
    let areas = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage(50),
            Constraint::Percentage(47),
            Constraint::Length(3),
        ])
        .split(frame.area());

    draw_speed_altitude_panel(frame, areas[0], data);
    draw_power_panel(frame, areas[1], data);
    draw_status_bar(frame, areas[2], data);
}

fn time_axis_labels(bounds: &[f64; 2]) -> Vec<Span<'static>> {
    let mid = (bounds[0] + bounds[1]) / 2.0;
    vec![
        Span::raw(crate::fmt_hhmmss(bounds[0])),
        Span::raw(crate::fmt_hhmmss(mid)),
        Span::raw(crate::fmt_hhmmss(bounds[1])),
    ]
}

fn draw_power_panel(frame: &mut Frame, area: Rect, data: &PlotData) {
    let instant_dataset = Dataset::default()
        .name("Power (W)")
        .marker(Marker::Braille)
        .style(Style::default().fg(Color::Yellow))
        .data(&data.power_series);

    let average_dataset = Dataset::default()
        .name("Avg power (W)")
        .marker(Marker::Braille)
        .style(Style::default().fg(Color::Green))
        .data(&data.average_power_series);

    let power_max = data.power_bounds[1];
    let power_labels = vec![
        Span::raw("0"),
        Span::raw(format!("{:.0}", power_max / 2.0)),
        Span::raw(format!("{:.0}", power_max)),
    ];

    let chart = Chart::new(vec![instant_dataset, average_dataset])
        .block(Block::default().borders(Borders::ALL).title("Power (W)"))
        .x_axis(
            Axis::default()
                .bounds(data.time_bounds)
                .labels(time_axis_labels(&data.time_bounds)),
        )
        .y_axis(
            Axis::default()
                .bounds(data.power_bounds)
                .labels(power_labels),
        );

    frame.render_widget(chart, area);
}

fn draw_speed_altitude_panel(frame: &mut Frame, area: Rect, data: &PlotData) {
    let speed_max = data.speed_bounds[1];

    // Normalize altitude into the speed y-range so both fit on the same axis.
    // The shape of the altitude profile is preserved; the actual range is shown in the title.
    let [alt_min, alt_max] = data.altitude_bounds;
    let alt_range = (alt_max - alt_min).max(1.0);
    let normalized_altitude: Vec<(f64, f64)> = data
        .altitude_series
        .iter()
        .map(|(t, alt)| (*t, (alt - alt_min) / alt_range * speed_max))
        .collect();

    let title = if data.altitude_series.is_empty() {
        "Speed (km/h)".to_string()
    } else {
        format!("Speed (km/h)  ·  Altitude: {:.0}–{:.0} m", alt_min, alt_max)
    };

    let instant_dataset = Dataset::default()
        .name("Instant (km/h)")
        .marker(Marker::Braille)
        .style(Style::default().fg(Color::Cyan))
        .data(&data.instant_speed_series);

    let average_dataset = Dataset::default()
        .name("Average (km/h)")
        .marker(Marker::Braille)
        .style(Style::default().fg(Color::Green))
        .data(&data.average_speed_series);

    let altitude_dataset = Dataset::default()
        .name("Altitude (m)")
        .marker(Marker::Braille)
        .style(Style::default().fg(Color::LightBlue))
        .data(&normalized_altitude);

    let speed_labels = vec![
        Span::raw("0"),
        Span::raw(format!("{:.0}", speed_max / 2.0)),
        Span::raw(format!("{:.0}", speed_max)),
    ];

    let mut datasets = vec![instant_dataset, average_dataset];
    if !normalized_altitude.is_empty() {
        datasets.push(altitude_dataset);
    }

    let chart = Chart::new(datasets)
        .block(Block::default().borders(Borders::ALL).title(title))
        .x_axis(
            Axis::default()
                .bounds(data.time_bounds)
                .labels(time_axis_labels(&data.time_bounds)),
        )
        .y_axis(
            Axis::default()
                .bounds(data.speed_bounds)
                .labels(speed_labels),
        );

    frame.render_widget(chart, area);
}

fn draw_status_bar(frame: &mut Frame, area: Rect, data: &PlotData) {
    let s = &data.summary;
    let power_str = s
        .avg_power_watts
        .map(|w| format!("{:.0} W", w))
        .unwrap_or_else(|| "N/A".to_string());
    let elevation_str = s
        .total_elevation_gain_m
        .map(|m| format!("  |  Elevation: {:.0} m", m))
        .unwrap_or_default();

    let text = format!(
        " Dist: {:.2} km  |  Elapsed: {}  |  Moving: {}  |  Avg speed: {:.1} km/h  |  Avg power: {}{}  |  [q] quit",
        s.total_distance_km,
        crate::fmt_hhmmss(s.elapsed_secs),
        crate::fmt_hhmmss(s.moving_secs),
        s.avg_speed_kmh,
        power_str,
        elevation_str,
    );

    let paragraph = Paragraph::new(text).block(Block::default().borders(Borders::ALL));
    frame.render_widget(paragraph, area);
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;

    fn make_point(
        seconds: f64,
        instant_speed: f64,
        avg_speed: f64,
        power: Option<f64>,
        distance: f64,
    ) -> AnalyzePoint {
        AnalyzePoint {
            timestamp: Utc::now(),
            seconds_from_start: seconds,
            moving_seconds_from_start: seconds * 0.9,
            raw_lat: 0.0,
            raw_lon: 0.0,
            smoothed_lat: 0.0,
            smoothed_lon: 0.0,
            smoothed_alt: None,
            instant_speed_kmh: instant_speed,
            average_speed_kmh: avg_speed,
            distance_km: distance,
            power_smooth_watts: power,
            cumulative_energy_kj: power.map(|w| w * seconds / 1000.0),
        }
    }

    #[test]
    fn test_build_plot_data_length_matches_points() {
        let points = vec![
            make_point(0.0, 0.0, 0.0, None, 0.0),
            make_point(60.0, 20.0, 15.0, Some(200.0), 0.3),
            make_point(120.0, 25.0, 18.0, Some(250.0), 0.7),
        ];
        let data = build_plot_data(&points);
        assert_eq!(data.power_series.len(), 3);
        assert_eq!(data.average_power_series.len(), 3);
        assert_eq!(data.instant_speed_series.len(), 3);
        assert_eq!(data.average_speed_series.len(), 3);
    }

    #[test]
    fn test_build_plot_data_x_values_are_seconds_from_start() {
        let points = vec![
            make_point(0.0, 10.0, 10.0, Some(100.0), 0.0),
            make_point(30.0, 20.0, 15.0, Some(200.0), 0.1),
        ];
        let data = build_plot_data(&points);
        assert_eq!(data.power_series[0].0, 0.0);
        assert_eq!(data.power_series[1].0, 30.0);
        assert_eq!(data.instant_speed_series[1].0, 30.0);
        assert_eq!(data.average_speed_series[1].0, 30.0);
    }

    #[test]
    fn test_build_plot_data_none_power_becomes_zero() {
        let points = vec![make_point(0.0, 0.0, 0.0, None, 0.0)];
        let data = build_plot_data(&points);
        assert_eq!(data.power_series[0].1, 0.0);
    }

    #[test]
    fn test_build_plot_data_some_power_is_preserved() {
        let points = vec![make_point(0.0, 0.0, 0.0, Some(200.0), 0.0)];
        let data = build_plot_data(&points);
        assert_eq!(data.power_series[0].1, 200.0);
    }

    #[test]
    fn test_build_plot_data_time_bounds_span_full_ride() {
        let points = vec![
            make_point(0.0, 0.0, 0.0, None, 0.0),
            make_point(3600.0, 30.0, 25.0, Some(300.0), 25.0),
        ];
        let data = build_plot_data(&points);
        assert_eq!(data.time_bounds, [0.0, 3600.0]);
    }

    #[test]
    fn test_build_plot_data_summary_distance_matches_last_point() {
        let points = vec![
            make_point(0.0, 0.0, 0.0, None, 0.0),
            make_point(60.0, 20.0, 15.0, Some(200.0), 12.5),
        ];
        let data = build_plot_data(&points);
        assert_eq!(data.summary.total_distance_km, 12.5);
    }

    #[test]
    fn test_build_plot_data_avg_power_none_when_all_none() {
        let points = vec![
            make_point(0.0, 0.0, 0.0, None, 0.0),
            make_point(60.0, 10.0, 8.0, None, 0.1),
        ];
        let data = build_plot_data(&points);
        assert!(data.summary.avg_power_watts.is_none());
    }

    #[test]
    fn test_build_plot_data_avg_power_computed_when_present() {
        let points = vec![
            make_point(0.0, 0.0, 0.0, Some(200.0), 0.0),
            make_point(60.0, 20.0, 15.0, Some(200.0), 0.3),
        ];
        let data = build_plot_data(&points);
        // energy-based: cumulative_energy_kj * 1000 / moving_secs = 12000 / 54 ≈ 222.2 W
        let expected = 200.0 * 60.0 / (60.0 * 0.9);
        assert!((data.summary.avg_power_watts.unwrap() - expected).abs() < 1e-9);
    }

    #[test]
    fn test_build_plot_data_empty_returns_zero_bounds() {
        let data = build_plot_data(&[]);
        assert_eq!(data.time_bounds, [0.0, 0.0]);
        assert_eq!(data.power_bounds, [0.0, 50.0]);
        assert_eq!(data.speed_bounds, [0.0, 5.0]);
        assert_eq!(data.altitude_bounds, [0.0, 50.0]);
        assert!(data.altitude_series.is_empty());
        assert!(data.summary.total_elevation_gain_m.is_none());
    }

    #[test]
    fn test_compute_y_bounds_rounds_up_to_next_step() {
        let series = vec![(0.0, 175.0), (1.0, 100.0)];
        assert_eq!(compute_y_bounds(&series, 50.0), [0.0, 200.0]);
    }

    #[test]
    fn test_compute_y_bounds_exact_multiple_stays_same() {
        let series = vec![(0.0, 200.0)];
        assert_eq!(compute_y_bounds(&series, 50.0), [0.0, 200.0]);
    }

    #[test]
    fn test_compute_y_bounds_zero_series_returns_one_step() {
        let series = vec![(0.0, 0.0), (1.0, 0.0)];
        assert_eq!(compute_y_bounds(&series, 50.0), [0.0, 50.0]);
    }

    #[test]
    fn test_compute_y_bounds_lower_bound_always_zero() {
        let series = vec![(0.0, 1000.0)];
        let bounds = compute_y_bounds(&series, 100.0);
        assert_eq!(bounds[0], 0.0);
    }

    #[test]
    fn test_average_power_series_is_zero_without_power_data() {
        let points = vec![
            make_point(0.0, 0.0, 0.0, None, 0.0),
            make_point(60.0, 20.0, 15.0, None, 0.3),
        ];
        let data = build_plot_data(&points);
        assert!(data.average_power_series.iter().all(|(_, w)| *w == 0.0));
    }

    #[test]
    fn test_average_power_series_equals_energy_over_moving_time() {
        // 200 W constant for 60 s → cumulative_energy_kj = 200 * 60 / 1000 = 12 kJ
        // moving_seconds = 60 * 0.9 = 54 s (from make_point formula)
        // average_power = 12_000 / 54 ≈ 222.2 W
        let points = vec![
            make_point(0.0, 0.0, 0.0, None, 0.0),
            make_point(60.0, 20.0, 15.0, Some(200.0), 0.3),
        ];
        let data = build_plot_data(&points);
        let expected = 200.0 * 60.0 / (60.0 * 0.9);
        assert!((data.average_power_series[1].1 - expected).abs() < 1e-9);
    }
}

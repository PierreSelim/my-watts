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

use crate::{
    stats::{self, Quartiles},
    AnalyzePoint, GpsAnalyzerError,
};

#[derive(Debug, Clone)]
pub struct RideSummary {
    pub total_distance_km: f64,
    pub elapsed_secs: f64,
    pub moving_secs: f64,
    pub avg_speed_kmh: f64,
    pub avg_power_watts: Option<f64>,
    pub total_elevation_gain_m: Option<f64>,
    pub moving_speed_quartiles_kmh: Option<Quartiles>,
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

pub fn build_plot_data(points: &[AnalyzePoint], moving_speed_threshold_kmh: f64) -> PlotData {
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

    let moving_speed_quartiles_kmh = moving_speed_quartiles(points, moving_speed_threshold_kmh);

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
            moving_speed_quartiles_kmh,
        },
    }
}

/// Quartiles of the instant-speed distribution, restricted to samples at or above
/// `threshold_kmh`. Returns `None` if no sample meets the threshold.
pub fn moving_speed_quartiles(points: &[AnalyzePoint], threshold_kmh: f64) -> Option<Quartiles> {
    let moving_speeds: Vec<f64> = points
        .iter()
        .map(|p| p.instant_speed_kmh)
        .filter(|s| *s >= threshold_kmh)
        .collect();
    stats::quartiles(&moving_speeds)
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
            Constraint::Length(4),
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

pub fn format_status_text(s: &RideSummary) -> String {
    let power_str = s
        .avg_power_watts
        .map(|w| format!("{:.0} W", w))
        .unwrap_or_else(|| "N/A".to_string());
    let elevation_str = s
        .total_elevation_gain_m
        .map(|m| format!("  |  Elevation: {:.0} m", m))
        .unwrap_or_default();

    let (p25_str, p50_str, p75_str) = match s.moving_speed_quartiles_kmh {
        Some(q) => (
            format!("{:.1} km/h", q.p25),
            format!("{:.1} km/h", q.p50),
            format!("{:.1} km/h", q.p75),
        ),
        None => ("N/A".to_string(), "N/A".to_string(), "N/A".to_string()),
    };

    let dist_cell = format!("Dist: {:.2} km", s.total_distance_km);
    let elapsed_cell = format!("Elapsed: {}", crate::fmt_hhmmss(s.elapsed_secs));
    let moving_cell = format!("Moving: {}", crate::fmt_hhmmss(s.moving_secs));
    let avg_speed_cell = format!("Avg speed: {:.1} km/h", s.avg_speed_kmh);

    let speed_label_cell = "Speed (moving)";
    let p25_cell = format!("P25: {}", p25_str);
    let median_cell = format!("Median: {}", p50_str);
    let p75_cell = format!("P75: {}", p75_str);

    let w1 = dist_cell.len().max(speed_label_cell.len());
    let w2 = elapsed_cell.len().max(p25_cell.len());
    let w3 = moving_cell.len().max(median_cell.len());
    let w4 = avg_speed_cell.len().max(p75_cell.len());

    format!(
        " {dist:<w1$}  |  {elapsed:<w2$}  |  {moving:<w3$}  |  {avg_speed:<w4$}  |  Avg power: {power}{elevation}  |  [q] quit\n {speed_label:<w1$}  |  {p25:<w2$}  |  {median:<w3$}  |  {p75:<w4$}",
        dist = dist_cell,
        elapsed = elapsed_cell,
        moving = moving_cell,
        avg_speed = avg_speed_cell,
        power = power_str,
        elevation = elevation_str,
        speed_label = speed_label_cell,
        p25 = p25_cell,
        median = median_cell,
        p75 = p75_cell,
        w1 = w1,
        w2 = w2,
        w3 = w3,
        w4 = w4,
    )
}

fn draw_status_bar(frame: &mut Frame, area: Rect, data: &PlotData) {
    let text = format_status_text(&data.summary);
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
        let data = build_plot_data(&points, 0.0);
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
        let data = build_plot_data(&points, 0.0);
        assert_eq!(data.power_series[0].0, 0.0);
        assert_eq!(data.power_series[1].0, 30.0);
        assert_eq!(data.instant_speed_series[1].0, 30.0);
        assert_eq!(data.average_speed_series[1].0, 30.0);
    }

    #[test]
    fn test_build_plot_data_none_power_becomes_zero() {
        let points = vec![make_point(0.0, 0.0, 0.0, None, 0.0)];
        let data = build_plot_data(&points, 0.0);
        assert_eq!(data.power_series[0].1, 0.0);
    }

    #[test]
    fn test_build_plot_data_some_power_is_preserved() {
        let points = vec![make_point(0.0, 0.0, 0.0, Some(200.0), 0.0)];
        let data = build_plot_data(&points, 0.0);
        assert_eq!(data.power_series[0].1, 200.0);
    }

    #[test]
    fn test_build_plot_data_time_bounds_span_full_ride() {
        let points = vec![
            make_point(0.0, 0.0, 0.0, None, 0.0),
            make_point(3600.0, 30.0, 25.0, Some(300.0), 25.0),
        ];
        let data = build_plot_data(&points, 0.0);
        assert_eq!(data.time_bounds, [0.0, 3600.0]);
    }

    #[test]
    fn test_build_plot_data_summary_distance_matches_last_point() {
        let points = vec![
            make_point(0.0, 0.0, 0.0, None, 0.0),
            make_point(60.0, 20.0, 15.0, Some(200.0), 12.5),
        ];
        let data = build_plot_data(&points, 0.0);
        assert_eq!(data.summary.total_distance_km, 12.5);
    }

    #[test]
    fn test_build_plot_data_avg_power_none_when_all_none() {
        let points = vec![
            make_point(0.0, 0.0, 0.0, None, 0.0),
            make_point(60.0, 10.0, 8.0, None, 0.1),
        ];
        let data = build_plot_data(&points, 0.0);
        assert!(data.summary.avg_power_watts.is_none());
    }

    #[test]
    fn test_build_plot_data_avg_power_computed_when_present() {
        let points = vec![
            make_point(0.0, 0.0, 0.0, Some(200.0), 0.0),
            make_point(60.0, 20.0, 15.0, Some(200.0), 0.3),
        ];
        let data = build_plot_data(&points, 0.0);
        // energy-based: cumulative_energy_kj * 1000 / moving_secs = 12000 / 54 ≈ 222.2 W
        let expected = 200.0 * 60.0 / (60.0 * 0.9);
        assert!((data.summary.avg_power_watts.unwrap() - expected).abs() < 1e-9);
    }

    #[test]
    fn test_build_plot_data_empty_returns_zero_bounds() {
        let data = build_plot_data(&[], 0.0);
        assert_eq!(data.time_bounds, [0.0, 0.0]);
        assert_eq!(data.power_bounds, [0.0, 50.0]);
        assert_eq!(data.speed_bounds, [0.0, 5.0]);
        assert_eq!(data.altitude_bounds, [0.0, 50.0]);
        assert!(data.altitude_series.is_empty());
        assert!(data.summary.total_elevation_gain_m.is_none());
        assert!(data.summary.moving_speed_quartiles_kmh.is_none());
    }

    #[test]
    fn test_build_plot_data_speed_percentiles_use_threshold_filter() {
        // instant speeds: [0, 10, 20, 30, 40]; with threshold 5.0, kept = [10,20,30,40].
        // rank = q*(n-1) = q*3 with linear interp on sorted samples:
        // P25 → 10 + 0.75*10 = 17.5; Median → 20 + 0.5*10 = 25.0; P75 → 30 + 0.25*10 = 32.5.
        let points: Vec<AnalyzePoint> = (0..5)
            .map(|i| make_point(i as f64 * 10.0, i as f64 * 10.0, 0.0, None, 0.0))
            .collect();
        let data = build_plot_data(&points, 5.0);
        let q = data.summary.moving_speed_quartiles_kmh.unwrap();
        assert!((q.p25 - 17.5).abs() < 1e-9, "p25={}", q.p25);
        assert!((q.p50 - 25.0).abs() < 1e-9, "p50={}", q.p50);
        assert!((q.p75 - 32.5).abs() < 1e-9, "p75={}", q.p75);
    }

    #[test]
    fn test_build_plot_data_speed_percentiles_none_when_all_below_threshold() {
        let points = vec![
            make_point(0.0, 1.0, 0.0, None, 0.0),
            make_point(1.0, 2.0, 0.0, None, 0.0),
        ];
        let data = build_plot_data(&points, 10.0);
        assert!(data.summary.moving_speed_quartiles_kmh.is_none());
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
        let data = build_plot_data(&points, 0.0);
        assert!(data.average_power_series.iter().all(|(_, w)| *w == 0.0));
    }

    fn make_summary(
        total_distance_km: f64,
        elapsed_secs: f64,
        moving_secs: f64,
        avg_speed_kmh: f64,
        avg_power_watts: Option<f64>,
        total_elevation_gain_m: Option<f64>,
        moving_speed_quartiles_kmh: Option<Quartiles>,
    ) -> RideSummary {
        RideSummary {
            total_distance_km,
            elapsed_secs,
            moving_secs,
            avg_speed_kmh,
            avg_power_watts,
            total_elevation_gain_m,
            moving_speed_quartiles_kmh,
        }
    }

    /// Byte offsets of `|` in a line.
    fn pipe_positions(line: &str) -> Vec<usize> {
        line.match_indices('|').map(|(i, _)| i).collect()
    }

    #[test]
    fn test_format_status_text_pipes_align_full_data() {
        let summary = make_summary(
            42.34,
            3725.0,
            3500.0,
            22.5,
            Some(180.0),
            Some(320.0),
            Some(Quartiles {
                p25: 18.2,
                p50: 22.0,
                p75: 26.4,
            }),
        );
        let text = format_status_text(&summary);
        let lines: Vec<&str> = text.split('\n').collect();
        assert_eq!(lines.len(), 2);

        let p1 = pipe_positions(lines[0]);
        let p2 = pipe_positions(lines[1]);
        // Line 2 has 3 pipes (4 cells); line 1 has more. First 3 must align.
        assert_eq!(p2.len(), 3);
        assert!(p1.len() >= 3);
        assert_eq!(&p1[..3], &p2[..]);
    }

    #[test]
    fn test_format_status_text_pipes_align_without_elevation() {
        let summary = make_summary(
            10.0,
            1800.0,
            1750.0,
            21.0,
            Some(150.0),
            None,
            Some(Quartiles {
                p25: 15.0,
                p50: 20.0,
                p75: 25.0,
            }),
        );
        let text = format_status_text(&summary);
        let lines: Vec<&str> = text.split('\n').collect();
        let p1 = pipe_positions(lines[0]);
        let p2 = pipe_positions(lines[1]);
        assert_eq!(&p1[..3], &p2[..]);
        // No elevation cell: line 1 has exactly 5 pipes (Dist|Elapsed|Moving|AvgSpeed|AvgPower|quit).
        assert_eq!(p1.len(), 5);
        assert!(!lines[0].contains("Elevation"));
    }

    #[test]
    fn test_format_status_text_pipes_align_without_power() {
        let summary = make_summary(
            5.0,
            600.0,
            580.0,
            18.0,
            None,
            Some(100.0),
            Some(Quartiles {
                p25: 10.0,
                p50: 15.0,
                p75: 20.0,
            }),
        );
        let text = format_status_text(&summary);
        let lines: Vec<&str> = text.split('\n').collect();
        let p1 = pipe_positions(lines[0]);
        let p2 = pipe_positions(lines[1]);
        assert_eq!(&p1[..3], &p2[..]);
        assert!(lines[0].contains("Avg power: N/A"));
    }

    #[test]
    fn test_format_status_text_pipes_align_without_quartiles() {
        let summary = make_summary(2.0, 120.0, 100.0, 9.0, None, None, None);
        let text = format_status_text(&summary);
        let lines: Vec<&str> = text.split('\n').collect();
        let p1 = pipe_positions(lines[0]);
        let p2 = pipe_positions(lines[1]);
        assert_eq!(&p1[..3], &p2[..]);
        assert!(lines[1].contains("P25: N/A"));
        assert!(lines[1].contains("Median: N/A"));
        assert!(lines[1].contains("P75: N/A"));
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
        let data = build_plot_data(&points, 0.0);
        let expected = 200.0 * 60.0 / (60.0 * 0.9);
        assert!((data.average_power_series[1].1 - expected).abs() < 1e-9);
    }
}

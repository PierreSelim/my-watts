mod cli;

use chrono::Utc;
use clap::Parser;
use my_watts::{
    analyze_pipeline, fmt_hhmmss,
    index::{RideEntry, RideIndex},
    list_tui::{self, ListOutcome},
    load_power_config,
    power::PowerConfig,
    power_pipeline, smooth_pipeline, tui, AnalyzeSummary, GpsAnalyzerError, SavitzkyGolayConfig,
};

fn main() {
    let cli = cli::Cli::parse();

    let result = match cli.command {
        cli::Commands::Smooth(cmd) => run_smooth(&cmd),
        cli::Commands::Power(cmd) => run_power(&cmd),
        cli::Commands::Analyze(cmd) => run_analyze(&cmd),
        cli::Commands::List(_) => run_list(),
    };

    if let Err(e) = result {
        eprintln!("Error: {}", e);
        std::process::exit(1);
    }
}

fn run_smooth(cmd: &cli::SmoothCommand) -> Result<(), GpsAnalyzerError> {
    let sg_config = SavitzkyGolayConfig::new(cmd.window_size, cmd.degree)?;
    let output_path = cmd.output_path();
    if cmd.verbose {
        eprintln!(
            "Smoothing {:?} (window={}, degree={}) → {:?}",
            cmd.input, cmd.window_size, cmd.degree, output_path
        );
    }
    let summary = smooth_pipeline(&cmd.input, &output_path, sg_config)?;
    eprintln!(
        "Smoothed {} points → {:?}",
        summary.point_count, output_path
    );
    Ok(())
}

fn run_power(cmd: &cli::PowerCommand) -> Result<(), GpsAnalyzerError> {
    let power_config = load_power_config(
        cmd.config.as_deref(),
        Some(cmd.rider_weight),
        cmd.bike_weight,
        Some(&cmd.bike),
    )?;
    let output_path = cmd.output_path();
    if cmd.verbose {
        eprintln!(
            "Power estimation for {:?} — bike '{}' (Crr={}, CdA={}) → {:?}",
            cmd.input,
            power_config.bike.name,
            power_config.bike.crr,
            power_config.bike.cda,
            output_path,
        );
    }
    let point_count = power_pipeline(&cmd.input, &output_path, &power_config)?;
    eprintln!("Computed {} power points → {:?}", point_count, output_path);
    Ok(())
}

fn run_analyze(cmd: &cli::AnalyzeCommand) -> Result<(), GpsAnalyzerError> {
    let sg_config = SavitzkyGolayConfig::new(cmd.window_size, cmd.degree)?;
    let power_config = load_power_config(
        cmd.config.as_deref(),
        cmd.rider_weight,
        cmd.bike_weight,
        cmd.bike.as_deref(),
    )?;
    if cmd.verbose {
        eprintln!(
            "Analyzing {:?} — rider {:.1} kg, bike '{}' (Crr={}, CdA={})",
            cmd.input,
            power_config.rider_weight_kg,
            power_config.bike.name,
            power_config.bike.crr,
            power_config.bike.cda,
        );
    }

    let summary = analyze_pipeline(
        &cmd.input,
        &cmd.analyze_output_path(),
        &cmd.intervals_output_path(),
        sg_config,
        &power_config,
        cmd.smooth_window,
        cmd.stop_buffer_secs,
    )?;

    eprintln!(
        "Elapsed: {} | Moving: {} | Distance: {:.2} km | Elapsed avg: {:.1} km/h | Moving avg: {:.1} km/h | Training: {:.1} km/h | Avg power: {:.0} W | Calories: {:.0} kcal | Elevation: {:.0} m",
        fmt_hhmmss(summary.elapsed_secs),
        fmt_hhmmss(summary.moving_secs),
        summary.total_distance_km,
        summary.elapsed_avg_speed_kmh,
        summary.moving_avg_speed_kmh,
        summary.training_speed_kmh,
        summary.avg_power_watts,
        summary.total_calories_kcal,
        summary.total_elevation_gain_m,
    );
    match &summary.moving_speed_quartiles {
        Some(q) => eprintln!(
            "Speed (moving) | P25: {:.1} km/h | Median: {:.1} km/h | P75: {:.1} km/h",
            q.p25, q.p50, q.p75,
        ),
        None => eprintln!("Speed (moving) | P25: N/A | Median: N/A | P75: N/A"),
    }
    eprintln!(
        "{} points → {:?}\n{} interval rows → {:?}",
        summary.point_count,
        cmd.analyze_output_path(),
        summary.interval_count,
        cmd.intervals_output_path(),
    );

    update_ride_index(cmd, &power_config, &summary);

    if !cmd.no_plot {
        tui::run_tui(&summary.plot_data)?;
    }

    Ok(())
}

/// Upsert this ride into the persistent index. A failure here is never fatal: the analysis
/// already succeeded, so we only warn and continue.
fn update_ride_index(
    cmd: &cli::AnalyzeCommand,
    power_config: &PowerConfig,
    summary: &AnalyzeSummary,
) {
    let entry = build_ride_entry(cmd, power_config, summary);
    let mut index = RideIndex::load_default().unwrap_or_else(|e| {
        eprintln!("Warning: could not read ride index ({e}); starting a new one.");
        RideIndex::default()
    });
    index.upsert(entry);
    if let Err(e) = index.save_default() {
        eprintln!("Warning: could not update ride index: {e}");
    }
}

fn build_ride_entry(
    cmd: &cli::AnalyzeCommand,
    power_config: &PowerConfig,
    summary: &AnalyzeSummary,
) -> RideEntry {
    let stem = cmd
        .input
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();
    RideEntry {
        stem,
        source_gpx_path: cmd.input.clone(),
        analyze_csv_path: cmd.analyze_output_path(),
        intervals_csv_path: cmd.intervals_output_path(),
        start_timestamp: summary.start_timestamp,
        indexed_at: Utc::now(),
        distance_km: summary.total_distance_km,
        elapsed_secs: summary.elapsed_secs,
        moving_secs: summary.moving_secs,
        moving_avg_speed_kmh: summary.moving_avg_speed_kmh,
        avg_power_watts: summary.avg_power_watts,
        total_calories_kcal: summary.total_calories_kcal,
        total_elevation_gain_m: summary.total_elevation_gain_m,
        replay: my_watts::index::ReplayParams {
            rider_weight_kg: power_config.rider_weight_kg,
            bike_weight_kg: power_config.bike_weight_kg,
            bike_name: power_config.bike.name.clone(),
            config_path: cmd.config.clone(),
            window_size: cmd.window_size,
            degree: cmd.degree,
            smooth_window: cmd.smooth_window,
            stop_buffer_secs: cmd.stop_buffer_secs,
        },
    }
}

fn run_list() -> Result<(), GpsAnalyzerError> {
    let index = RideIndex::load_default()?;
    if index.rides.is_empty() {
        eprintln!("No rides indexed yet. Run `my-watts analyze <file.gpx>` to add one.");
        return Ok(());
    }

    let mut selected = 0usize;
    let mut status: Option<String> = None;
    loop {
        match list_tui::run_list_tui(&index.rides, selected, status.as_deref())? {
            ListOutcome::Quit => break,
            ListOutcome::Replay(i) => {
                selected = i;
                status = match replay_entry(&index.rides[i]) {
                    Ok(()) => None,
                    Err(e) => Some(format!("Cannot open '{}': {e}", index.rides[i].stem)),
                };
            }
        }
    }
    Ok(())
}

/// Re-run the analyze pipeline for an indexed ride using its stored parameters, then open the plot.
fn replay_entry(entry: &RideEntry) -> Result<(), GpsAnalyzerError> {
    let p = &entry.replay;
    let sg_config = SavitzkyGolayConfig::new(p.window_size, p.degree)?;
    let power_config = load_power_config(
        p.config_path.as_deref(),
        Some(p.rider_weight_kg),
        p.bike_weight_kg,
        Some(&p.bike_name),
    )?;
    let summary = analyze_pipeline(
        &entry.source_gpx_path,
        &entry.analyze_csv_path,
        &entry.intervals_csv_path,
        sg_config,
        &power_config,
        p.smooth_window,
        p.stop_buffer_secs,
    )?;
    tui::run_tui(&summary.plot_data)
}

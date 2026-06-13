mod cli;

use clap::Parser;
use my_watts::{
    analyze_pipeline, build_ride_entry, config, fmt_hhmmss,
    index::{ReplayParams, RideEntry, RideIndex},
    list_tui::{self, ListOutcome},
    load_power_config,
    power::PowerConfig,
    power_pipeline, reindex_pipeline, smooth_pipeline, storage, tui, AnalyzeSummary,
    GpsAnalyzerError, SavitzkyGolayConfig,
};

fn main() {
    let cli = cli::Cli::parse();

    let result = match cli.command {
        cli::Commands::Smooth(cmd) => run_smooth(&cmd),
        cli::Commands::Power(cmd) => run_power(&cmd),
        cli::Commands::Analyze(cmd) => run_analyze(&cmd),
        cli::Commands::List(_) => run_list(),
        cli::Commands::Reindex(cmd) => run_reindex(&cmd),
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

/// Copy the analyzed GPX into the store and upsert the ride into the persistent index. Neither
/// step is fatal: the analysis already succeeded, so failures only warn. When the copy fails we
/// fall back to indexing the original path so the entry is still usable in this session.
fn update_ride_index(
    cmd: &cli::AnalyzeCommand,
    power_config: &PowerConfig,
    summary: &AnalyzeSummary,
) {
    let source_gpx_path = storage::store_gpx(&cmd.input).unwrap_or_else(|e| {
        eprintln!("Warning: could not copy GPX into the store ({e}); indexing original path.");
        cmd.input.clone()
    });
    let entry = build_ride_entry(
        source_gpx_path,
        cmd.analyze_output_path(),
        cmd.intervals_output_path(),
        replay_params_from_analyze(cmd, power_config),
        summary,
    );
    let mut index = RideIndex::load_default().unwrap_or_else(|e| {
        eprintln!("Warning: could not read ride index ({e}); starting a new one.");
        RideIndex::default()
    });
    index.upsert(entry);
    if let Err(e) = index.save_default() {
        eprintln!("Warning: could not update ride index: {e}");
    }
}

fn replay_params_from_analyze(
    cmd: &cli::AnalyzeCommand,
    power_config: &PowerConfig,
) -> ReplayParams {
    ReplayParams {
        rider_weight_kg: power_config.rider_weight_kg,
        bike_weight_kg: power_config.bike_weight_kg,
        bike_name: power_config.bike.name.clone(),
        config_path: cmd.config.clone(),
        window_size: cmd.window_size,
        degree: cmd.degree,
        smooth_window: cmd.smooth_window,
        stop_buffer_secs: cmd.stop_buffer_secs,
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

/// Rebuild the index from scratch out of every GPX in the store, applying the given parameters
/// uniformly. The freshly built index replaces the existing one.
fn run_reindex(cmd: &cli::ReindexCommand) -> Result<(), GpsAnalyzerError> {
    let sg_config = SavitzkyGolayConfig::new(cmd.window_size, cmd.degree)?;
    let power_config = load_power_config(
        cmd.config.as_deref(),
        cmd.rider_weight,
        cmd.bike_weight,
        cmd.bike.as_deref(),
    )?;
    let gpx_files = storage::stored_gpx_files()?;
    if gpx_files.is_empty() {
        eprintln!(
            "No GPX files found in {:?}. Run `my-watts analyze <file.gpx>` to add some.",
            config::gpx_dir()
        );
        return Ok(());
    }

    let replay = ReplayParams {
        rider_weight_kg: power_config.rider_weight_kg,
        bike_weight_kg: power_config.bike_weight_kg,
        bike_name: power_config.bike.name.clone(),
        config_path: cmd.config.clone(),
        window_size: cmd.window_size,
        degree: cmd.degree,
        smooth_window: cmd.smooth_window,
        stop_buffer_secs: cmd.stop_buffer_secs,
    };
    let result = reindex_pipeline(
        &gpx_files,
        &config::analysis_dir(),
        sg_config,
        &power_config,
        cmd.smooth_window,
        cmd.stop_buffer_secs,
        &replay,
    );

    for (path, err) in &result.skipped {
        eprintln!("Warning: skipped {path:?}: {err}");
    }
    result.index.save_default()?;
    eprintln!(
        "Reindexed {} rides ({} skipped) → {:?}",
        result.index.rides.len(),
        result.skipped.len(),
        config::index_path(),
    );
    Ok(())
}

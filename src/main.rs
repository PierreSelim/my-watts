mod cli;

use clap::Parser;
use my_watts::{
    analyze, config::AppConfig, csv, fmt_hhmmss, gpx, kj_to_kcal, power, smoothing, tui,
    GpsAnalyzerError, SavitzkyGolayConfig,
};
use std::path::Path;

fn load_power_config(
    config_path: Option<&Path>,
    rider_weight: Option<f64>,
    bike_weight: f64,
    bike_name: Option<&str>,
) -> Result<power::PowerConfig, GpsAnalyzerError> {
    let app_config = AppConfig::load_or_default(config_path)?;
    let rider_weight = rider_weight.unwrap_or(app_config.default_rider_weight_kg);
    let bike_name = bike_name.unwrap_or(&app_config.default_bike);
    let bike = app_config
        .find_bike(bike_name)
        .ok_or_else(|| GpsAnalyzerError::BikeNotFound(bike_name.to_string()))?
        .clone();
    Ok(power::PowerConfig {
        rider_weight_kg: rider_weight,
        bike_weight_kg: bike_weight,
        bike,
    })
}

fn main() {
    let cli = cli::Cli::parse();

    let result = match cli.command {
        cli::Commands::Smooth(cmd) => run_smooth(&cmd),
        cli::Commands::Power(cmd) => run_power(&cmd),
        cli::Commands::Analyze(cmd) => run_analyze(&cmd),
    };

    match result {
        Ok(_) => {}
        Err(e) => {
            eprintln!("Error: {}", e);
            std::process::exit(1);
        }
    }
}

fn run_power(cmd: &cli::PowerCommand) -> Result<(), GpsAnalyzerError> {
    if cmd.verbose {
        eprintln!("Loading GPX file: {:?}", cmd.input);
    }

    let track = gpx::load_gpx(&cmd.input)?;

    if cmd.verbose {
        eprintln!("Loaded {} points", track.len());
    }

    let app_config = AppConfig::load_or_default(cmd.config.as_deref())?;

    let bike = app_config
        .find_bike(&cmd.bike)
        .ok_or_else(|| GpsAnalyzerError::BikeNotFound(cmd.bike.clone()))?
        .clone();

    if cmd.verbose {
        eprintln!(
            "Using bike '{}' (Crr={}, CdA={})",
            bike.name, bike.crr, bike.cda
        );
    }

    let power_config = power::PowerConfig {
        rider_weight_kg: cmd.rider_weight,
        bike_weight_kg: cmd.bike_weight,
        bike,
    };

    let points = power::compute_power(&track, &power_config)?;

    let output_path = cmd.output_path();

    if cmd.verbose {
        eprintln!("Writing power CSV to: {:?}", output_path);
    }

    csv::write_power_csv(&points, &output_path)?;

    eprintln!("Computed {} power points → {:?}", points.len(), output_path);

    Ok(())
}

fn run_analyze(cmd: &cli::AnalyzeCommand) -> Result<(), GpsAnalyzerError> {
    if cmd.verbose {
        eprintln!("Loading GPX file: {:?}", cmd.input);
    }

    let raw_track = gpx::load_gpx(&cmd.input)?;

    if cmd.verbose {
        eprintln!("Loaded {} points", raw_track.len());
    }

    let sg_config = SavitzkyGolayConfig::new(cmd.window_size, cmd.degree)?;
    let smoothed_track = smoothing::smooth_track(&raw_track, sg_config)?;

    let power_config = load_power_config(
        cmd.config.as_deref(),
        cmd.rider_weight,
        cmd.bike_weight,
        cmd.bike.as_deref(),
    )?;

    if cmd.verbose {
        eprintln!(
            "Using rider weight {:.1} kg, bike '{}' (Crr={}, CdA={})",
            power_config.rider_weight_kg,
            power_config.bike.name,
            power_config.bike.crr,
            power_config.bike.cda,
        );
    }

    let moving_speed_threshold_kmh = power_config.bike.moving_speed_threshold_kmh;
    let power_points = power::compute_power(&smoothed_track, &power_config)?;
    let (analyze_points, intervals) = analyze::analyze_track(
        &raw_track,
        &smoothed_track,
        Some(&power_points),
        moving_speed_threshold_kmh,
        cmd.smooth_window,
    );

    let analyze_path = cmd.analyze_output_path();
    let intervals_path = cmd.intervals_output_path();

    for path in [&analyze_path, &intervals_path] {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(GpsAnalyzerError::Io)?;
        }
    }
    csv::write_analyze_csv(&analyze_points, &analyze_path)?;
    csv::write_intervals_csv(&intervals, &intervals_path)?;

    let total_distance_km = analyze_points.last().map(|p| p.distance_km).unwrap_or(0.0);
    let elapsed_secs = analyze_points
        .last()
        .map(|p| p.seconds_from_start)
        .unwrap_or(0.0);
    let moving_secs = analyze_points
        .last()
        .map(|p| p.moving_seconds_from_start)
        .unwrap_or(0.0);
    let avg_speed_kmh = analyze_points
        .last()
        .map(|p| p.average_speed_kmh)
        .unwrap_or(0.0);
    let training_speed_kmh =
        analyze::compute_training_speed_kmh(&analyze_points, moving_speed_threshold_kmh, cmd.stop_buffer_secs);
    let moving_power: Vec<f64> = power_points
        .iter()
        .filter(|p| p.speed_ms * 3.6 >= moving_speed_threshold_kmh)
        .map(|p| p.power_watts)
        .collect();
    let avg_power_watts = if moving_power.is_empty() {
        0.0
    } else {
        moving_power.iter().sum::<f64>() / moving_power.len() as f64
    };
    let total_calories_kcal = analyze_points
        .last()
        .and_then(|p| p.cumulative_energy_kj)
        .map(kj_to_kcal)
        .unwrap_or(0.0);
    let total_elevation_gain_m: f64 = analyze_points
        .windows(2)
        .filter_map(|w| {
            let curr = w[1].smoothed_alt?;
            let prev = w[0].smoothed_alt?;
            Some((curr - prev).max(0.0))
        })
        .sum();

    eprintln!(
        "Elapsed: {} | Moving: {} | Distance: {:.2} km | Avg speed: {:.1} km/h | Training speed: {:.1} km/h | Avg power: {:.0} W | Calories: {:.0} kcal | Elevation: {:.0} m",
        fmt_hhmmss(elapsed_secs),
        fmt_hhmmss(moving_secs),
        total_distance_km,
        avg_speed_kmh,
        training_speed_kmh,
        avg_power_watts,
        total_calories_kcal,
        total_elevation_gain_m,
    );

    match tui::moving_speed_quartiles(&analyze_points, moving_speed_threshold_kmh) {
        Some(q) => eprintln!(
            "Speed (moving) | P25: {:.1} km/h | Median: {:.1} km/h | P75: {:.1} km/h",
            q.p25, q.p50, q.p75,
        ),
        None => eprintln!("Speed (moving) | P25: N/A | Median: N/A | P75: N/A"),
    }

    eprintln!(
        "{} points → {:?}\n{} interval rows → {:?}",
        analyze_points.len(),
        analyze_path,
        intervals.len(),
        intervals_path
    );

    if !cmd.no_plot {
        let plot_data = tui::build_plot_data(
            &analyze_points,
            moving_speed_threshold_kmh,
            training_speed_kmh,
        );
        tui::run_tui(&plot_data)?;
    }

    Ok(())
}

fn run_smooth(cmd: &cli::SmoothCommand) -> Result<(), GpsAnalyzerError> {
    if cmd.verbose {
        eprintln!("Loading GPX file: {:?}", cmd.input);
    }

    let track = gpx::load_gpx(&cmd.input)?;

    if cmd.verbose {
        eprintln!("Loaded {} points", track.len());
    }

    let config = SavitzkyGolayConfig::new(cmd.window_size, cmd.degree)?;

    if cmd.verbose {
        eprintln!(
            "Smoothing with window_size={}, degree={}",
            cmd.window_size, cmd.degree
        );
    }

    let smoothed_track = smoothing::smooth_track(&track, config)?;

    let output_path = cmd.output_path();

    if cmd.verbose {
        eprintln!("Writing CSV to: {:?}", output_path);
    }

    csv::write_csv(&smoothed_track, &output_path)?;

    if cmd.verbose {
        eprintln!("Done!");
    }

    Ok(())
}

mod cli;

use clap::Parser;
use my_watts::{
    analyze, config::AppConfig, csv, gpx, power, smoothing, GpsAnalyzerError, SavitzkyGolayConfig,
};

fn fmt_hhmmss(total_secs: f64) -> String {
    let secs = total_secs as u64;
    format!(
        "{:02}:{:02}:{:02}",
        secs / 3600,
        (secs % 3600) / 60,
        secs % 60
    )
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

    let app_config = AppConfig::load_or_default(cmd.config.as_deref())?;
    let rider_weight = cmd
        .rider_weight
        .unwrap_or(app_config.default_rider_weight_kg);
    let bike_name = cmd.bike.as_deref().unwrap_or(&app_config.default_bike);

    let bike = app_config
        .find_bike(bike_name)
        .ok_or_else(|| GpsAnalyzerError::BikeNotFound(bike_name.to_string()))?
        .clone();

    if cmd.verbose {
        eprintln!(
            "Using rider weight {rider_weight:.1} kg, bike '{}' (Crr={}, CdA={})",
            bike.name, bike.crr, bike.cda
        );
    }

    let power_config = power::PowerConfig {
        rider_weight_kg: rider_weight,
        bike_weight_kg: cmd.bike_weight,
        bike,
    };

    let moving_speed_threshold_kmh = power_config.bike.moving_speed_threshold_kmh;
    let power_points = power::compute_power(&smoothed_track, &power_config)?;
    let (analyze_points, intervals) = analyze::analyze_track(
        &raw_track,
        &smoothed_track,
        Some(&power_points),
        moving_speed_threshold_kmh,
    );

    let analyze_path = cmd.analyze_output_path();
    let intervals_path = cmd.intervals_output_path();

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
        .unwrap_or(0.0);

    eprintln!(
        "Elapsed: {} | Moving: {} | Distance: {:.2} km | Avg speed: {:.1} km/h | Avg power: {:.0} W | Calories: {:.0} kcal",
        fmt_hhmmss(elapsed_secs),
        fmt_hhmmss(moving_secs),
        total_distance_km,
        avg_speed_kmh,
        avg_power_watts,
        total_calories_kcal
    );
    eprintln!(
        "{} points → {:?}\n{} interval rows → {:?}",
        analyze_points.len(),
        analyze_path,
        intervals.len(),
        intervals_path
    );

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

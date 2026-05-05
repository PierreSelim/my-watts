mod cli;

use clap::Parser;
use my_watts::{
    config::AppConfig, csv, gpx, power, smoothing, GpsAnalyzerError, SavitzkyGolayConfig,
};

fn main() {
    let cli = cli::Cli::parse();

    let result = match cli.command {
        cli::Commands::Smooth(cmd) => run_smooth(&cmd),
        cli::Commands::Power(cmd) => run_power(&cmd),
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

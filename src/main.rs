mod cli;

use clap::Parser;
use my_watts::{csv, gpx, smoothing, GpsAnalyzerError, SavitzkyGolayConfig};

fn main() {
    let cli = cli::Cli::parse();

    let result = match cli.command {
        cli::Commands::Smooth(cmd) => run_smooth(&cmd),
    };

    match result {
        Ok(_) => {}
        Err(e) => {
            eprintln!("Error: {}", e);
            std::process::exit(1);
        }
    }
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

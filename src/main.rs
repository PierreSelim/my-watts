mod cli;

use clap::Parser;
use my_watts::{
    analyze_pipeline, fmt_hhmmss, load_power_config, power_pipeline, smooth_pipeline, tui,
    GpsAnalyzerError, SavitzkyGolayConfig,
};

fn main() {
    let cli = cli::Cli::parse();

    let result = match cli.command {
        cli::Commands::Smooth(cmd) => run_smooth(&cmd),
        cli::Commands::Power(cmd) => run_power(&cmd),
        cli::Commands::Analyze(cmd) => run_analyze(&cmd),
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

    if !cmd.no_plot {
        tui::run_tui(&summary.plot_data)?;
    }

    Ok(())
}

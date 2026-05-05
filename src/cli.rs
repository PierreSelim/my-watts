use clap::{Parser, Subcommand};
use std::path::PathBuf;

#[derive(Parser)]
#[command(name = "my-watts")]
#[command(about = "GPS analyzer for bike rides", long_about = None)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Commands,
}

#[derive(Subcommand)]
pub enum Commands {
    /// Smooth GPS drift using Savitzky-Golay filter
    Smooth(SmoothCommand),
    /// Estimate power output from a GPX file
    Power(PowerCommand),
}

#[derive(Parser)]
pub struct SmoothCommand {
    /// Input GPX file
    pub input: PathBuf,

    /// Output CSV file
    #[arg(short, long)]
    pub output: Option<PathBuf>,

    /// Savitzky-Golay window size (must be odd, default: 5)
    #[arg(long, default_value = "5")]
    pub window_size: u32,

    /// Polynomial degree for smoothing (default: 2)
    #[arg(long, default_value = "2")]
    pub degree: u32,

    /// Enable verbose output
    #[arg(short, long)]
    pub verbose: bool,
}

impl SmoothCommand {
    pub fn output_path(&self) -> PathBuf {
        self.output.clone().unwrap_or_else(|| {
            let input_stem = self.input.file_stem().unwrap().to_string_lossy();
            PathBuf::from(format!("{}.smoothed.csv", input_stem))
        })
    }
}

#[derive(Parser)]
pub struct PowerCommand {
    /// Input GPX file
    pub input: PathBuf,

    /// Rider weight in kg
    #[arg(long)]
    pub rider_weight: f64,

    /// Bike weight in kg
    #[arg(long, default_value = "10.0")]
    pub bike_weight: f64,

    /// Bike name from config (e.g. road, gravel, mountain, hybrid)
    #[arg(long)]
    pub bike: String,

    /// Path to config file (default: platform config dir)
    #[arg(long)]
    pub config: Option<PathBuf>,

    /// Output CSV file
    #[arg(short, long)]
    pub output: Option<PathBuf>,

    /// Enable verbose output
    #[arg(short, long)]
    pub verbose: bool,
}

impl PowerCommand {
    pub fn output_path(&self) -> PathBuf {
        self.output.clone().unwrap_or_else(|| {
            let input_stem = self.input.file_stem().unwrap().to_string_lossy();
            PathBuf::from(format!("{}.power.csv", input_stem))
        })
    }
}

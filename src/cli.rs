use clap::{Parser, Subcommand};
use my_watts::config;
use std::path::{Path, PathBuf};

fn input_stem(input: &Path) -> String {
    input
        .file_stem()
        .unwrap_or(input.as_os_str())
        .to_string_lossy()
        .into_owned()
}

fn default_output_path(input: &Path, suffix: &str) -> PathBuf {
    PathBuf::from(format!("{}.{}", input_stem(input), suffix))
}

fn analysis_output_path(input: &Path, suffix: &str) -> PathBuf {
    config::analysis_dir().join(format!("{}.{}", input_stem(input), suffix))
}

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
    /// Produce enriched per-point CSV and interval summary CSV from a GPX file
    Analyze(AnalyzeCommand),
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
        self.output
            .clone()
            .unwrap_or_else(|| default_output_path(&self.input, "smoothed.csv"))
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
        self.output
            .clone()
            .unwrap_or_else(|| default_output_path(&self.input, "power.csv"))
    }
}

#[derive(Parser)]
pub struct AnalyzeCommand {
    /// Input GPX file
    pub input: PathBuf,

    /// Output CSV file for per-point data
    #[arg(short, long)]
    pub output: Option<PathBuf>,

    /// Savitzky-Golay window size (must be odd, default: 5)
    #[arg(long, default_value = "5")]
    pub window_size: u32,

    /// Polynomial degree for smoothing (default: 2)
    #[arg(long, default_value = "2")]
    pub degree: u32,

    /// Rider weight in kg (overrides config default of 75 kg)
    #[arg(long)]
    pub rider_weight: Option<f64>,

    /// Bike weight in kg (default: 10.0)
    #[arg(long, default_value = "10.0")]
    pub bike_weight: f64,

    /// Bike name from config (overrides config default of "road")
    #[arg(long)]
    pub bike: Option<String>,

    /// Path to config file (default: platform config dir)
    #[arg(long)]
    pub config: Option<PathBuf>,

    /// Half-window for instant speed and power smoothing (default: 5).
    /// Both are computed over [i-n, i+n] seconds; use 0 for consecutive-point speed.
    #[arg(long, default_value = "5")]
    pub smooth_window: usize,

    /// Seconds to exclude before and after each stop when computing Training speed (default: 10)
    #[arg(long, default_value = "10.0")]
    pub stop_buffer_secs: f64,

    /// Skip the interactive terminal plot after writing CSVs
    #[arg(long)]
    pub no_plot: bool,

    /// Enable verbose output
    #[arg(short, long)]
    pub verbose: bool,
}

impl AnalyzeCommand {
    pub fn analyze_output_path(&self) -> PathBuf {
        self.output
            .clone()
            .unwrap_or_else(|| analysis_output_path(&self.input, "analyze.csv"))
    }

    pub fn intervals_output_path(&self) -> PathBuf {
        analysis_output_path(&self.input, "intervals.csv")
    }
}

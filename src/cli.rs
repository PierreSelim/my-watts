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
            let output_name = format!("{}.smoothed.csv", input_stem);
            PathBuf::from(output_name)
        })
    }
}

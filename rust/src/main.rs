mod app;
mod config;
mod investigation;
mod slurm;
mod views;

use anyhow::Result;
use clap::Parser;

/// A TUI dashboard for Slurm clusters.
#[derive(Debug, Parser)]
#[command(name = "sqtop", version, about)]
struct Cli {
    /// Path to an alternate config file.
    #[arg(long, env = "SQTOP_CONFIG")]
    config: Option<std::path::PathBuf>,
}

fn main() -> Result<()> {
    let _cli = Cli::parse();
    Ok(())
}

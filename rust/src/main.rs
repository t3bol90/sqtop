mod app;
mod columns;
mod config;
mod investigation;
mod responsive;
mod slurm;
mod views;

use anyhow::Result;
use clap::Parser;

/// A TUI dashboard for Slurm clusters.
#[derive(Debug, Parser)]
#[command(name = "sqtop", version, about)]
struct Cli {
    /// Path to an alternate config file.
    ///
    /// Precedence is resolved by `config::resolve_config_path`:
    /// this flag, then `$SQTOP_CONFIG`, then `~/.config/sqtop/config.toml`.
    #[arg(long)]
    config: Option<std::path::PathBuf>,
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    let config_path = config::resolve_config_path(cli.config);
    let _settings = config::load(&config_path);
    Ok(())
}

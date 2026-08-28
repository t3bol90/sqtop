mod app;
mod chrome;
mod clipboard;
mod columns;
mod config;
mod investigation;
mod responsive;
mod slurm;
mod views;

use anyhow::Result;
use clap::Parser;
use crossterm::{
    cursor, execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{backend::CrosstermBackend, Terminal};
use std::io;
use std::panic;

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

    /// Remote Slurm cluster via SSH host/alias from `~/.ssh/config`.
    ///
    /// Overrides `[remote].host` in the config file.
    #[arg(long, value_name = "HOST_OR_ALIAS")]
    remote: Option<String>,

    /// SSH identity file used with `--remote`.
    #[arg(long, value_name = "PATH")]
    ssh_key: Option<String>,
}

fn main() -> Result<()> {
    // Install panic hook before entering alternate screen so panics restore terminal
    let original_hook = panic::take_hook();
    panic::set_hook(Box::new(move |panic_info| {
        let _ = restore_terminal();
        original_hook(panic_info);
    }));

    let cli = Cli::parse();
    let config_path = config::resolve_config_path(cli.config);
    let mut settings = config::load(&config_path);

    // `--remote` overrides `[remote].host`; `--ssh-key` has no config equivalent.
    if let Some(host) = cli.remote.as_ref() {
        settings.remote.host = host.trim().to_string();
    }
    let ssh_key = cli.ssh_key.unwrap_or_default().trim().to_string();

    // Enable raw mode, then run TUI in a closure so we can restore on ANY error
    enable_raw_mode()?;
    let result = (|| -> Result<()> {
        let mut stdout = io::stdout();
        execute!(stdout, EnterAlternateScreen)?;
        execute!(stdout, crossterm::event::EnableMouseCapture)?;
        let backend = CrosstermBackend::new(stdout);
        let mut terminal = Terminal::new(backend)?;
        app::run(&mut terminal, settings, config_path, ssh_key)
    })();

    // Always restore terminal, even on error - best effort, never masks the real error
    let _ = restore_terminal();
    result
}

/// Restore terminal state - called both on normal exit and in panic hook.
fn restore_terminal() -> Result<()> {
    let mut stdout = io::stdout();
    execute!(stdout, crossterm::event::DisableMouseCapture)?;
    execute!(stdout, cursor::Show)?;
    execute!(stdout, LeaveAlternateScreen)?;
    disable_raw_mode()?;
    Ok(())
}

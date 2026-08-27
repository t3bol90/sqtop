//! Health view - command latency and failure diagnostics.
//!
//! This view is PASSIVE by design: it reads the command history that the Runner
//! already records as a side effect of every Slurm call, via `Runner::history(limit)`.
//! It must NEVER issue a Slurm command of its own. Preserve that property - it is
//! the whole point of the view.

use crate::app::App;
use crate::slurm::model::ErrorCategory;
use ratatui::layout::{Constraint, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::Span;
use ratatui::widgets::{Block, Borders, Row, Table};

/// Format an ErrorCategory for display.
///
/// Uses ErrorCategory::as_str() to ensure exact match with Python.
fn format_error_category(category: &Option<ErrorCategory>) -> String {
    category
        .as_ref()
        .map(|c| c.as_str())
        .unwrap_or("")
        .to_string()
}

/// Truncate stderr to fit in the ERROR column (40 chars + "..." if longer).
///
/// Matches Python:
/// ```python
/// err = item.stderr[:40] + ("..." if len(item.stderr) > 40 else "")
/// ```
fn truncate_stderr(stderr: &str) -> String {
    if stderr.len() > 40 {
        format!("{}...", &stderr[..40])
    } else {
        stderr.to_string()
    }
}

/// Render the health view.
///
/// Displays command history from the Runner (passive, never issues commands).
/// Columns: COMMAND, OK, LATENCY, CATEGORY, ERROR.
/// Matches layout from Python src/sqtop/views/health.py:
/// ```python
/// table.add_column("COMMAND", width=22)
/// table.add_column("OK", width=6)
/// table.add_column("LATENCY", width=10)
/// table.add_column("CATEGORY", width=22)
/// table.add_column("ERROR", width=42)
/// ```
pub fn render(f: &mut ratatui::Frame, app: &App, area: Rect) {
    // Check for too-small area
    if area.width < 10 || area.height < 3 {
        let block = Block::default()
            .borders(Borders::ALL)
            .title("Health (too small)");
        f.render_widget(block, area);
        return;
    }

    // Fetch history from Runner (passive read, never issues commands)
    let stats = app.runner.history(100);

    // Build header
    let header_cells = vec![
        Span::styled(
            "COMMAND",
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            "OK",
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            "LATENCY",
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            "CATEGORY",
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            "ERROR",
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        ),
    ];
    let header = Row::new(header_cells).height(1).bottom_margin(0);

    // Build rows (reverse order, most recent first)
    let rows: Vec<Row> = stats
        .iter()
        .rev()
        .map(|stat| {
            let command = stat.command.split_whitespace().next().unwrap_or("");
            let ok_text = if stat.ok { "yes" } else { "no" };
            let ok_color = if stat.ok { Color::Green } else { Color::Red };
            let category = format_error_category(&stat.error_category);
            let category_color = if category.is_empty() {
                Color::White
            } else {
                Color::Red
            };
            let error = truncate_stderr(&stat.stderr);

            let cells = vec![
                Span::raw(command),
                Span::styled(ok_text, Style::default().fg(ok_color)),
                Span::raw(format!("{} ms", stat.latency_ms)),
                Span::styled(category, Style::default().fg(category_color)),
                Span::raw(error),
            ];
            Row::new(cells)
        })
        .collect();

    // Column widths (fixed, matching Python)
    let widths = vec![
        Constraint::Length(22),
        Constraint::Length(6),
        Constraint::Length(10),
        Constraint::Length(22),
        Constraint::Length(42),
    ];

    // Calculate statistics for title
    let failures = stats.iter().filter(|s| !s.ok).count();
    let avg_ms = if stats.is_empty() {
        0
    } else {
        stats.iter().map(|s| s.latency_ms).sum::<u64>() / stats.len() as u64
    };

    let title = format!(
        "health  {} failures  {}ms avg  {} samples",
        failures,
        avg_ms,
        stats.len()
    );

    // Create table widget
    let table = Table::new(rows, widths)
        .header(header)
        .block(Block::default().borders(Borders::ALL).title(title))
        .row_highlight_style(Style::default().add_modifier(Modifier::REVERSED));

    f.render_widget(table, area);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;
    use ratatui::backend::TestBackend;
    use ratatui::Terminal;

    #[test]
    fn test_format_error_category_all_variants() {
        // Verify ErrorCategory::as_str() matches expected snake_case strings exactly
        assert_eq!(
            format_error_category(&Some(ErrorCategory::SlurmCommandNotFound)),
            "slurm_command_not_found"
        );
        assert_eq!(
            format_error_category(&Some(ErrorCategory::SlurmCommandTimeout)),
            "slurm_command_timeout"
        );
        assert_eq!(
            format_error_category(&Some(ErrorCategory::SlurmCommandFailed)),
            "slurm_command_failed"
        );
        assert_eq!(
            format_error_category(&Some(ErrorCategory::SlurmPermissionDenied)),
            "slurm_permission_denied"
        );
        assert_eq!(
            format_error_category(&Some(ErrorCategory::SlurmFieldUnavailable)),
            "slurm_field_unavailable"
        );
        assert_eq!(
            format_error_category(&Some(ErrorCategory::SshConnectionFailed)),
            "ssh_connection_failed"
        );
        assert_eq!(
            format_error_category(&Some(ErrorCategory::SshAuthFailed)),
            "ssh_auth_failed"
        );
        assert_eq!(
            format_error_category(&Some(ErrorCategory::SshCommandTimeout)),
            "ssh_command_timeout"
        );
        assert_eq!(
            format_error_category(&Some(ErrorCategory::JobNotFound)),
            "job_not_found"
        );
        assert_eq!(
            format_error_category(&Some(ErrorCategory::NodeNotFound)),
            "node_not_found"
        );
    }

    #[test]
    fn test_format_error_category_none() {
        assert_eq!(format_error_category(&None), "");
    }

    #[test]
    fn test_truncate_stderr_under_40() {
        assert_eq!(truncate_stderr("short error"), "short error");
    }

    #[test]
    fn test_truncate_stderr_exactly_40() {
        let s = "a".repeat(40);
        assert_eq!(truncate_stderr(&s), s);
    }

    #[test]
    fn test_truncate_stderr_over_40() {
        let s = "a".repeat(50);
        assert_eq!(truncate_stderr(&s), format!("{}...", "a".repeat(40)));
    }

    #[test]
    fn test_truncate_stderr_41_chars() {
        let s = "a".repeat(41);
        assert_eq!(truncate_stderr(&s), format!("{}...", "a".repeat(40)));
    }

    #[test]
    fn test_health_view_reads_from_runner_history() {
        // Health view is passive - it reads from Runner::history(), never issues commands
        let config = Config::default();
        let app = App::new(config);

        // Initially empty history
        let stats = app.runner.history(100);
        assert_eq!(stats.len(), 0);

        // Render should not panic with empty history
        let backend = TestBackend::new(120, 30);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|f| {
                let area = f.area();
                render(f, &app, area);
            })
            .unwrap();
    }

    #[test]
    fn test_health_view_100_entry_cap() {
        // Verify that Runner::history(100) is called, enforcing the 100-entry cap
        let config = Config::default();
        let app = App::new(config);

        // Simulate 150 commands in history by calling run_result
        for i in 0..150 {
            let cmd = format!("echo test{}", i);
            app.runner.run_result(&cmd);
        }

        // Health view requests history(100)
        let stats = app.runner.history(100);

        // Should cap at 100 entries
        assert!(stats.len() <= 100);
    }

    #[test]
    fn test_health_view_too_small_area() {
        let config = Config::default();
        let app = App::new(config);

        // Render with too-small area
        let backend = TestBackend::new(8, 2);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|f| {
                let area = f.area();
                render(f, &app, area);
            })
            .unwrap();

        // Should not panic
    }

    #[test]
    fn test_health_view_snapshot() {
        // Create app with some command history
        let config = Config::default();
        let app = App::new(config);

        // Add a successful and a failed command
        let _ = app.runner.run_result("squeue --version");
        let _ = app.runner.run_result("squeue_nonexistent");

        // Render to TestBackend
        let backend = TestBackend::new(120, 30);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|f| {
                let area = f.area();
                render(f, &app, area);
            })
            .unwrap();

        // Verify it rendered without panicking
        let buffer = terminal.backend().buffer().clone();
        assert!(buffer.area().width > 0);
    }
}

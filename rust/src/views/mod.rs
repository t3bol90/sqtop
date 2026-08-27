//! UI layer: one module per tab, plus modal overlays.
//!
//! View workers implement render functions for each tab:
//! - `render_jobs` - Jobs table
//! - `render_nodes` - Nodes table
//! - `render_partitions` - Partitions table
//!
//! Each render function receives:
//! - `f: &mut Frame` - ratatui frame
//! - `app: &App` - application state
//! - `area: Rect` - rendering area
//!
//! Placeholder implementations render simple text until view workers
//! implement the full tables.

pub mod health;
pub mod partitions;
pub mod table_state;

use crate::app::App;
use ratatui::layout::Rect;
use ratatui::style::{Color, Style};
use ratatui::text::Line;
use ratatui::widgets::{Block, Borders, Paragraph};

/// Render the Jobs tab.
pub fn render_jobs(f: &mut ratatui::Frame, app: &App, area: Rect) {
    let text = if app.jobs.is_empty() {
        vec![
            Line::from("No jobs found"),
            Line::from(""),
            Line::from("This is a placeholder. View workers will implement the full table."),
        ]
    } else {
        vec![
            Line::from(format!("{} jobs loaded", app.jobs.len())),
            Line::from(""),
            Line::from("View workers will render the jobs table here."),
        ]
    };

    let block = Block::default()
        .borders(Borders::ALL)
        .title("Jobs")
        .style(Style::default().fg(Color::White));

    let paragraph = Paragraph::new(text).block(block);
    f.render_widget(paragraph, area);
}

/// Render the Nodes tab.
pub fn render_nodes(f: &mut ratatui::Frame, app: &App, area: Rect) {
    let text = if app.nodes.is_empty() {
        vec![
            Line::from("No nodes found"),
            Line::from(""),
            Line::from("This is a placeholder. View workers will implement the full table."),
        ]
    } else {
        vec![
            Line::from(format!("{} nodes loaded", app.nodes.len())),
            Line::from(""),
            Line::from("View workers will render the nodes table here."),
        ]
    };

    let block = Block::default()
        .borders(Borders::ALL)
        .title("Nodes")
        .style(Style::default().fg(Color::White));

    let paragraph = Paragraph::new(text).block(block);
    f.render_widget(paragraph, area);
}

/// Render the Partitions tab.
pub fn render_partitions(f: &mut ratatui::Frame, app: &App, area: Rect) {
    partitions::render(f, app, area);
}

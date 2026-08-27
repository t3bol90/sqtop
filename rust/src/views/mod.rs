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
pub mod jobs;
pub mod nodes;
pub mod partitions;
pub mod table_state;

use crate::app::App;
use ratatui::layout::Rect;
use ratatui::style::{Color, Style};
use ratatui::text::Line;
use ratatui::widgets::{Block, Borders, Paragraph};

/// Render the Jobs tab.
pub fn render_jobs(f: &mut ratatui::Frame, app: &mut App, area: Rect) {
    jobs::render(
        f,
        area,
        &mut app.jobs_view,
        &app.jobs,
        &app.config,
        &std::env::var("USER").unwrap_or_default(),
    );
}

/// Render the Nodes tab.
pub fn render_nodes(f: &mut ratatui::Frame, app: &App, area: Rect) {
    use nodes::NodesView;
    let mut view = NodesView::new(&app.config);
    view.render(f, &app.nodes, area);
}

/// Render the Partitions tab.
pub fn render_partitions(f: &mut ratatui::Frame, app: &App, area: Rect) {
    partitions::render(f, app, area);
}

//! UI layer: one module per tab, plus modal overlays.
//!
//! View workers implement render functions for each tab:
//! - `render_jobs` - Jobs table
//! - `render_nodes` - Nodes table
//! - `render_partitions` - Partitions table
//!
//! Each render function receives:
//! - `f: &mut Frame` - ratatui frame
//! - `app: &App` (or `&mut App` where the view owns cursor state)
//! - `area: Rect` - rendering area
//!

pub mod detail;
pub mod health;
pub mod history;
pub mod investigate;
pub mod jobs;
pub mod modals;
pub mod nodes;
pub mod partitions;
pub mod table_state;
pub mod visual;

use crate::app::App;
use ratatui::layout::Rect;

/// Render the Jobs tab.
pub fn render_jobs(f: &mut ratatui::Frame, app: &mut App, area: Rect) {
    // Store the actual rendered area for mouse hit testing
    app.last_jobs_table_area = Some(area);

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
pub fn render_nodes(f: &mut ratatui::Frame, app: &mut App, area: Rect) {
    // Store the actual rendered area for mouse hit testing
    app.last_nodes_table_area = Some(area);

    // Split the borrow so the view can be mutated while reading node data.
    let App {
        nodes, nodes_view, ..
    } = app;
    nodes_view.render(f, nodes, area);
}

/// Render the Partitions tab.
pub fn render_partitions(f: &mut ratatui::Frame, app: &mut App, area: Rect) {
    partitions::render(f, app, area);
}

/// Render the History tab.
pub fn render_history(f: &mut ratatui::Frame, area: Rect, view: &mut history::HistoryView) {
    history::render(f, area, view);
}

/// Render the Health tab.
pub fn render_health(f: &mut ratatui::Frame, app: &App, area: Rect) {
    health::render(f, app, area);
}

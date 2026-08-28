//! History view: completed/failed job history via sacct.
//!
//! This module implements the history tab, including:
//! - Filter pipeline (mine only)
//! - Column layout with responsive allocation
//! - Cyclic cursor state
//! - Rendering with ratatui

use crate::responsive::{
    allocate_columns, tier_for, ColumnSpec, Tier, TOO_SMALL_HEIGHT, TOO_SMALL_WIDTH,
};
use crate::slurm::fetch::SacctJob;
use crate::views::table_state::CyclicTableState;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::Line;
use ratatui::widgets::{Block, Borders, Cell, Paragraph, Row, Table};
use std::collections::HashMap;
use std::sync::LazyLock;

/// Color scheme for job states.
static STATE_COLORS: LazyLock<HashMap<&str, Color>> = LazyLock::new(|| {
    let mut m = HashMap::new();
    m.insert("COMPLETED", Color::DarkGray);
    m.insert("FAILED", Color::Red);
    m.insert("CANCELLED", Color::Yellow);
    m.insert("TIMEOUT", Color::Magenta);
    m
});

/// Column specifications for history view.
/// (name, min_width, content_max, priority, min_tier)
fn history_columns() -> Vec<ColumnSpec> {
    vec![
        ColumnSpec::new("JOBID", 8, 12, 100, Tier::Xs),
        ColumnSpec::new("STATE", 12, 16, 95, Tier::Xs),
        ColumnSpec::new("ELAPSED", 10, 12, 90, Tier::Xs),
        ColumnSpec::new("NAME", 12, 24, 80, Tier::Sm),
        ColumnSpec::new("USER", 8, 12, 75, Tier::Sm),
        ColumnSpec::new("EXIT", 6, 8, 70, Tier::Sm),
        ColumnSpec::new("PARTITION", 10, 14, 60, Tier::Md),
    ]
}

/// History view state.
#[derive(Debug, Clone)]
pub struct HistoryView {
    /// Filter: show only jobs owned by $USER
    pub filter_mine: bool,
    /// Current column specs and widths (name, width)
    pub current_cols: Vec<(String, u16)>,
    /// Rebuild cache: width when columns were last built
    pub rebuild_cache_width: u16,
    /// Rebuild cache: column names when last built
    pub rebuild_cache_names: Vec<String>,
    /// Table cursor state
    pub table_state: CyclicTableState,
    /// Last filtered jobs (for row lookups)
    pub last_jobs: Vec<SacctJob>,
    /// Last unfiltered jobs (raw from app)
    pub last_jobs_raw: Vec<SacctJob>,
    /// Hours to query (default 24)
    pub hours: u32,
}

impl Default for HistoryView {
    fn default() -> Self {
        Self::new()
    }
}

impl HistoryView {
    /// Create a new history view with default state.
    pub fn new() -> Self {
        Self {
            filter_mine: false,
            current_cols: Vec::new(),
            rebuild_cache_width: 0,
            rebuild_cache_names: Vec::new(),
            table_state: CyclicTableState::new(),
            last_jobs: Vec::new(),
            last_jobs_raw: Vec::new(),
            hours: 24,
        }
    }

    /// Toggle mine filter on/off.
    pub fn toggle_mine(&mut self, current_user: &str) {
        self.filter_mine = !self.filter_mine;
        self.update(self.last_jobs_raw.clone(), current_user);
    }

    /// Update the view with new data and apply filters.
    /// Returns the old cursor state (selected row index and anchor key).
    pub fn update(
        &mut self,
        jobs: Vec<SacctJob>,
        current_user: &str,
    ) -> (Option<usize>, Option<String>) {
        let old_selected = self.table_state.selected();
        let anchor = old_selected.and_then(|idx| self.last_jobs.get(idx).map(|j| j.job_id.clone()));

        self.last_jobs_raw = jobs;

        // Apply filter
        let filtered = if self.filter_mine {
            self.last_jobs_raw
                .iter()
                .filter(|j| j.user == current_user)
                .cloned()
                .collect()
        } else {
            self.last_jobs_raw.clone()
        };

        self.last_jobs = filtered;
        (old_selected, anchor)
    }

    /// Restore cursor state after update.
    pub fn restore_state(&mut self, old_selected: Option<usize>, anchor: Option<String>) {
        if self.last_jobs.is_empty() {
            self.table_state.select(None);
            self.table_state.set_row_count(0);
            return;
        }

        self.table_state.set_row_count(self.last_jobs.len());

        // Try to find anchor
        if let Some(ref anchor_id) = anchor {
            for (idx, job) in self.last_jobs.iter().enumerate() {
                if &job.job_id == anchor_id {
                    self.table_state.select(Some(idx));
                    return;
                }
            }
        }

        // Fall back to old position or 0
        let new_idx = old_selected
            .unwrap_or(0)
            .min(self.last_jobs.len().saturating_sub(1));
        self.table_state.select(Some(new_idx));
    }

    /// Rebuild column layout if width changed.
    pub fn rebuild_columns(&mut self, width: u16, _force: bool) -> bool {
        let budget = width.saturating_sub(3); // CHROME_OVERHEAD
        let tier = tier_for(width);
        let specs = history_columns();

        let new_cols = allocate_columns(budget, &specs, tier);
        let visible_names: Vec<String> = new_cols.iter().map(|(n, _)| n.clone()).collect();

        // Check if changed
        if width == self.rebuild_cache_width && visible_names == self.rebuild_cache_names {
            return false;
        }

        self.rebuild_cache_width = width;
        self.rebuild_cache_names = visible_names;
        self.current_cols = new_cols;
        true
    }

    /// Get the state color for a job state.
    fn state_color(state: &str) -> Color {
        let upper = state.to_uppercase();
        for (key, color) in STATE_COLORS.iter() {
            if upper.starts_with(key) {
                return *color;
            }
        }
        Color::White
    }

    /// Get the exit code color.
    fn exit_color(exit_code: &str) -> Color {
        if exit_code == "0:0" {
            Color::Green
        } else {
            Color::Red
        }
    }

    /// Get cell value for a job and column.
    fn cell_value(job: &SacctJob, col_name: &str) -> String {
        match col_name {
            "JOBID" => job.job_id.clone(),
            "NAME" => job.name.clone(),
            "USER" => job.user.clone(),
            "STATE" => job.state.clone(),
            "ELAPSED" => job.elapsed.clone(),
            "EXIT" => job.exit_code.clone(),
            "PARTITION" => job.partition.clone(),
            _ => String::new(),
        }
    }

    /// Truncate a cell value to fit the column width.
    fn truncate_cell(value: &str, width: u16) -> String {
        let w = width as usize;
        if value.len() <= w {
            value.to_string()
        } else if w == 0 {
            String::new()
        } else if w == 1 {
            value.chars().next().unwrap_or(' ').to_string()
        } else {
            format!("{}…", value.chars().take(w - 1).collect::<String>())
        }
    }

    /// Handle key input for the history view.
    pub fn handle_key(&mut self, key: crossterm::event::KeyEvent, current_user: &str) -> bool {
        use crossterm::event::{KeyCode, KeyModifiers};

        match (key.code, key.modifiers) {
            (KeyCode::Down | KeyCode::Char('j'), KeyModifiers::NONE) => {
                self.table_state.next();
                true
            }
            (KeyCode::Up | KeyCode::Char('k'), KeyModifiers::NONE) => {
                self.table_state.prev();
                true
            }
            (KeyCode::Char('m'), KeyModifiers::NONE) => {
                self.toggle_mine(current_user);
                true
            }
            _ => false,
        }
    }
}

/// Render the history view.
pub fn render(f: &mut ratatui::Frame, area: Rect, view: &mut HistoryView) {
    // Check too-small floor
    if area.width < TOO_SMALL_WIDTH || area.height < TOO_SMALL_HEIGHT {
        let text = vec![
            Line::from("Terminal too small"),
            Line::from(""),
            Line::from(format!(
                "Need at least {}×{}",
                TOO_SMALL_WIDTH, TOO_SMALL_HEIGHT
            )),
            Line::from(format!("Current: {}×{}", area.width, area.height)),
        ];
        let block = Block::default()
            .borders(Borders::ALL)
            .title("History")
            .style(Style::default().fg(Color::White));
        let paragraph = Paragraph::new(text).block(block);
        f.render_widget(paragraph, area);
        return;
    }

    // Rebuild columns if needed
    view.rebuild_columns(area.width, false);

    // Build header
    let header_title = if view.filter_mine {
        format!("History (last {}h) • mine", view.hours)
    } else {
        format!("History (last {}h)", view.hours)
    };

    let failed_count = view
        .last_jobs
        .iter()
        .filter(|j| j.state.to_uppercase().starts_with("FAILED"))
        .count();

    let total_str = if view.filter_mine {
        format!("{}/{} jobs", view.last_jobs.len(), view.last_jobs_raw.len())
    } else {
        format!("{} jobs", view.last_jobs.len())
    };

    // Build column headers
    let mut header_cells = Vec::new();
    for (name, _) in &view.current_cols {
        header_cells.push(
            Cell::from(name.as_str()).style(
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            ),
        );
    }
    let header = Row::new(header_cells).height(1);

    // Build rows
    let mut rows = Vec::new();
    for job in &view.last_jobs {
        let mut cells = Vec::new();
        for (name, width) in &view.current_cols {
            let value = HistoryView::cell_value(job, name);
            let truncated = HistoryView::truncate_cell(&value, *width);
            let cell = match name.as_str() {
                "STATE" => {
                    let color = HistoryView::state_color(&job.state);
                    Cell::from(truncated).style(Style::default().fg(color))
                }
                "EXIT" => {
                    let color = HistoryView::exit_color(&job.exit_code);
                    Cell::from(truncated).style(Style::default().fg(color))
                }
                _ => Cell::from(truncated),
            };
            cells.push(cell);
        }
        rows.push(Row::new(cells));
    }

    let widths: Vec<ratatui::layout::Constraint> = view
        .current_cols
        .iter()
        .map(|(_, w)| ratatui::layout::Constraint::Length(*w))
        .collect();

    let table = Table::new(rows, widths)
        .header(header)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(format!(
                    "{} | {} failed | {}",
                    header_title, failed_count, total_str
                ))
                .style(Style::default().fg(Color::White)),
        )
        .row_highlight_style(
            Style::default()
                .bg(Color::DarkGray)
                .add_modifier(Modifier::BOLD),
        );

    let mut table_state = ratatui::widgets::TableState::default();
    table_state.select(view.table_state.selected());

    f.render_stateful_widget(table, area, &mut table_state);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_job(job_id: &str, state: &str, user: &str) -> SacctJob {
        SacctJob {
            job_id: job_id.to_string(),
            name: "test-job".to_string(),
            user: user.to_string(),
            state: state.to_string(),
            num_cpus: "4".to_string(),
            elapsed: "01:00:00".to_string(),
            exit_code: "0:0".to_string(),
            partition: "gpu".to_string(),
        }
    }

    #[test]
    fn test_history_view_creation() {
        let view = HistoryView::new();
        assert!(!view.filter_mine);
        assert_eq!(view.hours, 24);
        assert!(view.last_jobs.is_empty());
    }

    #[test]
    fn test_toggle_mine_filter() {
        let mut view = HistoryView::new();
        let jobs = vec![
            make_job("1", "COMPLETED", "alice"),
            make_job("2", "FAILED", "bob"),
            make_job("3", "COMPLETED", "alice"),
        ];

        view.update(jobs, "alice");
        assert_eq!(view.last_jobs.len(), 3);

        view.toggle_mine("alice");
        assert!(view.filter_mine);
        assert_eq!(view.last_jobs.len(), 2);
        assert_eq!(view.last_jobs[0].job_id, "1");
        assert_eq!(view.last_jobs[1].job_id, "3");

        view.toggle_mine("alice");
        assert!(!view.filter_mine);
        assert_eq!(view.last_jobs.len(), 3);
    }

    #[test]
    fn test_history_preserves_cursor_on_update() {
        let mut view = HistoryView::new();
        let jobs = vec![
            make_job("100", "COMPLETED", "alice"),
            make_job("101", "COMPLETED", "alice"),
            make_job("102", "COMPLETED", "bob"),
        ];

        // First update
        let (old_selected, anchor) = view.update(jobs.clone(), "alice");
        view.restore_state(old_selected, anchor);
        view.table_state.select(Some(1));

        // Second update with same data - cursor should stay on job 101
        let (old_selected, anchor) = view.update(jobs, "alice");
        view.restore_state(old_selected, anchor);

        assert_eq!(view.table_state.selected(), Some(1));
    }

    #[test]
    fn test_state_colors() {
        assert_eq!(HistoryView::state_color("COMPLETED"), Color::DarkGray);
        assert_eq!(HistoryView::state_color("FAILED"), Color::Red);
        assert_eq!(HistoryView::state_color("CANCELLED"), Color::Yellow);
        assert_eq!(HistoryView::state_color("TIMEOUT"), Color::Magenta);
        assert_eq!(HistoryView::state_color("UNKNOWN"), Color::White);
    }

    #[test]
    fn test_exit_colors() {
        assert_eq!(HistoryView::exit_color("0:0"), Color::Green);
        assert_eq!(HistoryView::exit_color("1:0"), Color::Red);
        assert_eq!(HistoryView::exit_color("0:1"), Color::Red);
    }

    #[test]
    fn test_cell_value() {
        let job = make_job("123", "COMPLETED", "alice");
        assert_eq!(HistoryView::cell_value(&job, "JOBID"), "123");
        assert_eq!(HistoryView::cell_value(&job, "STATE"), "COMPLETED");
        assert_eq!(HistoryView::cell_value(&job, "USER"), "alice");
        assert_eq!(HistoryView::cell_value(&job, "UNKNOWN"), "");
    }

    #[test]
    fn test_truncate_cell() {
        assert_eq!(HistoryView::truncate_cell("short", 10), "short");
        assert_eq!(HistoryView::truncate_cell("exactly10c", 10), "exactly10c");
        assert_eq!(HistoryView::truncate_cell("toolongvalue", 8), "toolong…");
        assert_eq!(HistoryView::truncate_cell("abc", 2), "a…");
    }

    #[test]
    fn test_restore_state_with_anchor() {
        let mut view = HistoryView::new();
        let jobs = vec![
            make_job("1", "COMPLETED", "alice"),
            make_job("2", "FAILED", "bob"),
            make_job("3", "COMPLETED", "alice"),
        ];

        view.update(jobs.clone(), "alice");
        view.table_state.select(Some(1));
        view.table_state.set_row_count(3);

        // Simulate refresh with same data but reordered
        let new_jobs = vec![
            make_job("3", "COMPLETED", "alice"),
            make_job("2", "FAILED", "bob"),
            make_job("1", "COMPLETED", "alice"),
        ];
        view.last_jobs = new_jobs;

        view.restore_state(Some(1), Some("2".to_string()));
        assert_eq!(view.table_state.selected(), Some(1));
    }

    #[test]
    fn test_restore_state_empty() {
        let mut view = HistoryView::new();
        view.restore_state(Some(0), None);
        assert_eq!(view.table_state.selected(), None);
    }

    #[test]
    fn test_column_rebuild() {
        let mut view = HistoryView::new();
        assert!(view.rebuild_columns(80, true));
        assert!(!view.current_cols.is_empty());

        // No change on same width
        assert!(!view.rebuild_columns(80, false));

        // Change on different width
        assert!(view.rebuild_columns(120, false));
    }
}

//! Jobs view: filter, sort, and render the jobs table.
//!
//! This module implements the jobs tab, including:
//! - Filter pipeline (mine, search, sort)
//! - Column layout and auto-sizing
//! - Cyclic cursor state
//! - Rendering with ratatui

use crate::columns::{jobs_columns, reconcile_order};
use crate::config::Config;
use crate::responsive::{
    allocate_columns, tier_for, ColumnSpec, TOO_SMALL_HEIGHT, TOO_SMALL_WIDTH,
};
use crate::slurm::model::Job;
use crate::views::table_state::CyclicTableState;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::Line;
use ratatui::widgets::{Block, Borders, Cell, Paragraph, Row, Table};
use std::collections::HashMap;
use std::sync::LazyLock;
use toml;

/// State priority order for default sorting.
/// Lower number = higher priority.
static STATE_ORDER: LazyLock<HashMap<&str, u8>> = LazyLock::new(|| {
    let mut m = HashMap::new();
    m.insert("COMPLETING", 0);
    m.insert("RUNNING", 1);
    m.insert("PENDING", 2);
    m
});

/// Color scheme for job states.
static STATE_COLORS: LazyLock<HashMap<&str, Color>> = LazyLock::new(|| {
    let mut m = HashMap::new();
    m.insert("RUNNING", Color::Green);
    m.insert("PENDING", Color::Yellow);
    m.insert("FAILED", Color::Red);
    m.insert("CANCELLED", Color::Red);
    m.insert("COMPLETED", Color::DarkGray);
    m.insert("TIMEOUT", Color::Magenta);
    m.insert("NODE_FAIL", Color::Red);
    m.insert("PREEMPTED", Color::Yellow);
    m
});

/// Jobs view state.
#[derive(Debug, Clone)]
pub struct JobsView {
    /// Filter: show only jobs owned by $USER
    pub filter_mine: bool,
    /// Search query (case-insensitive substring match)
    pub search_query: String,
    /// Whether search input is active (user is typing)
    pub search_input_active: bool,
    /// Sort column name, or None for default state-priority sort
    pub sort_col: Option<String>,
    /// Reverse sort order
    pub sort_reversed: bool,
    /// Column order (saved user preference)
    pub column_order: Vec<String>,
    /// Current column widths
    pub column_widths: HashMap<String, u16>,
    /// Reorder target column index (in visible-column space)
    pub reorder_target_idx: usize,
    /// Table cursor state
    pub table_state: CyclicTableState,
    /// Visual selection state
    pub visual_selection: crate::views::visual::VisualSelection,
    /// Last filtered/sorted jobs (for row lookups)
    pub last_jobs: Vec<Job>,
    /// Last unfiltered jobs (raw from app)
    pub last_jobs_raw: Vec<Job>,
    /// Pending config update to persist (set by view actions, consumed by app)
    pending_config_update: Option<HashMap<String, toml::Value>>,
}

impl Default for JobsView {
    fn default() -> Self {
        Self::new()
    }
}

impl JobsView {
    /// Create a new jobs view with default state.
    pub fn new() -> Self {
        Self {
            filter_mine: false,
            search_query: String::new(),
            search_input_active: false,
            sort_col: None,
            sort_reversed: false,
            column_order: Vec::new(),
            column_widths: HashMap::new(),
            reorder_target_idx: 0,
            table_state: CyclicTableState::new(),
            visual_selection: crate::views::visual::VisualSelection::new(),
            last_jobs: Vec::new(),
            last_jobs_raw: Vec::new(),
            pending_config_update: None,
        }
    }

    /// Load saved state from config.
    pub fn from_config(config: &Config) -> Self {
        let mut view = Self::new();
        if !config.view_state.jobs_sort_col.is_empty() {
            view.sort_col = Some(config.view_state.jobs_sort_col.clone());
        }
        view.sort_reversed = config.view_state.jobs_sort_reversed;

        // Load column order from config
        if !config.columns.jobs_order.is_empty() {
            view.column_order = config.columns.jobs_order.clone();
        }

        view
    }

    /// Take pending config update (returns and clears it).
    pub fn take_pending_config_update(&mut self) -> Option<HashMap<String, toml::Value>> {
        self.pending_config_update.take()
    }

    /// Toggle the "mine" filter.
    pub fn toggle_filter_mine(&mut self) {
        self.filter_mine = !self.filter_mine;
    }

    /// Set the search query.
    #[cfg(test)]
    pub fn set_search_query(&mut self, query: String) {
        self.search_query = query;
    }

    /// Clear the search query.
    pub fn clear_search(&mut self) {
        self.search_query.clear();
    }

    /// Toggle sort by column.
    pub fn toggle_sort(&mut self, column: &str) {
        if self.sort_col.as_deref() == Some(column) {
            self.sort_reversed = !self.sort_reversed;
        } else {
            self.sort_col = Some(column.to_string());
            self.sort_reversed = false;
        }
        // Persist sort state
        let mut view_state = toml::Table::new();
        view_state.insert(
            "jobs_sort_col".to_string(),
            toml::Value::String(self.sort_col.clone().unwrap_or_default()),
        );
        view_state.insert(
            "jobs_sort_reversed".to_string(),
            toml::Value::Boolean(self.sort_reversed),
        );
        let mut update = HashMap::new();
        update.insert("view_state".to_string(), toml::Value::Table(view_state));
        self.pending_config_update = Some(update);
    }

    /// Clear sort (revert to default state-priority sort).
    pub fn clear_sort(&mut self) {
        self.sort_col = None;
        self.sort_reversed = false;
        // Persist cleared sort state
        let mut view_state = toml::Table::new();
        view_state.insert(
            "jobs_sort_col".to_string(),
            toml::Value::String(String::new()),
        );
        view_state.insert(
            "jobs_sort_reversed".to_string(),
            toml::Value::Boolean(false),
        );
        let mut update = HashMap::new();
        update.insert("view_state".to_string(), toml::Value::Table(view_state));
        self.pending_config_update = Some(update);
    }
    /// Cycle the reorder target to the next visible column (wraps).
    pub fn cycle_reorder_target(&mut self) {
        let visible_count = self.column_widths.len();
        if visible_count > 0 {
            self.reorder_target_idx = (self.reorder_target_idx + 1) % visible_count;
        }
    }

    /// Get the list of currently visible column names (in order).
    fn visible_column_names(&self) -> Vec<String> {
        // Return columns in the order they appear in column_order, filtered by visibility
        self.column_order
            .iter()
            .filter(|name| self.column_widths.contains_key(*name))
            .cloned()
            .collect()
    }

    /// Shift the targeted column left in the absolute column_order.
    pub fn shift_column_left(&mut self) {
        use crate::columns::move_in_order;

        let visible = self.visible_column_names();
        if visible.is_empty() || self.reorder_target_idx >= visible.len() {
            return;
        }

        let target_name = &visible[self.reorder_target_idx];

        // Find position in absolute column_order
        let abs_idx = self.column_order.iter().position(|n| n == target_name);
        if let Some(idx) = abs_idx {
            if idx > 0 {
                // Move before the predecessor
                let before = self.column_order.get(idx - 1).map(|s| s.as_str());
                self.column_order = move_in_order(&self.column_order, target_name, before);
                // Clamp target index
                if self.reorder_target_idx > 0 {
                    self.reorder_target_idx -= 1;
                }
                // Persist column order
                let mut columns = toml::Table::new();
                let order_array: Vec<toml::Value> = self
                    .column_order
                    .iter()
                    .map(|s| toml::Value::String(s.clone()))
                    .collect();
                columns.insert("jobs_order".to_string(), toml::Value::Array(order_array));
                let mut update = HashMap::new();
                update.insert("columns".to_string(), toml::Value::Table(columns));
                self.pending_config_update = Some(update);
            }
        }
    }

    /// Shift the targeted column right in the absolute column_order.
    pub fn shift_column_right(&mut self) {
        use crate::columns::move_in_order;

        let visible = self.visible_column_names();
        if visible.is_empty() || self.reorder_target_idx >= visible.len() {
            return;
        }

        let target_name = &visible[self.reorder_target_idx];

        // Find position in absolute column_order
        let abs_idx = self.column_order.iter().position(|n| n == target_name);
        if let Some(idx) = abs_idx {
            if idx < self.column_order.len() - 1 {
                // Move before the item two positions ahead (or None for end)
                let before = self.column_order.get(idx + 2).map(|s| s.as_str());
                self.column_order = move_in_order(&self.column_order, target_name, before);
                // Clamp target index
                self.reorder_target_idx = (self.reorder_target_idx + 1).min(visible.len() - 1);
                // Persist column order
                let mut columns = toml::Table::new();
                let order_array: Vec<toml::Value> = self
                    .column_order
                    .iter()
                    .map(|s| toml::Value::String(s.clone()))
                    .collect();
                columns.insert("jobs_order".to_string(), toml::Value::Array(order_array));
                let mut update = HashMap::new();
                update.insert("columns".to_string(), toml::Value::Table(columns));
                self.pending_config_update = Some(update);
            }
        }
    }

    /// Apply the filter pipeline: mine -> search -> sort.
    ///
    /// Returns the filtered and sorted jobs.
    pub fn apply_filters(&self, jobs: &[Job], current_user: &str) -> Vec<Job> {
        let mut filtered: Vec<Job> = jobs.to_vec();

        // Step 1: filter_mine
        if self.filter_mine {
            filtered.retain(|j| j.user == current_user);
        }

        // Step 2: search_query
        if !self.search_query.is_empty() {
            let query = self.search_query.to_lowercase();
            filtered.retain(|j| job_matches_search(j, &query));
        }

        // Step 3: sort
        if let Some(ref col) = self.sort_col {
            filtered.sort_by(|a, b| {
                let ord = match col.as_str() {
                    "STATE" => a.state.cmp(&b.state).then_with(|| job_id_cmp(a, b)),
                    "TIME" => a.time_used.cmp(&b.time_used),
                    "CPUS" => safe_int_cmp(&a.num_cpus, &b.num_cpus),
                    "QOS" => a
                        .qos
                        .to_lowercase()
                        .cmp(&b.qos.to_lowercase())
                        .then_with(|| job_sort_key(a).cmp(&job_sort_key(b))),
                    _ => std::cmp::Ordering::Equal,
                };
                if self.sort_reversed {
                    ord.reverse()
                } else {
                    ord
                }
            });
        } else {
            // Default state-priority sort
            filtered.sort_by_key(job_sort_key);
        }

        filtered
    }

    /// Update the view with new jobs and apply filters.
    pub fn update(&mut self, jobs: Vec<Job>, current_user: &str) -> CapturedState {
        let state = self.capture_state();
        self.last_jobs_raw = jobs;
        self.last_jobs = self.apply_filters(&self.last_jobs_raw, current_user);
        self.table_state.set_row_count(self.last_jobs.len());
        state
    }

    /// Restore cursor state after update.
    pub fn restore_state(&mut self, state: CapturedState) {
        if let Some(anchor) = state.anchor {
            if let Some(idx) = self.last_jobs.iter().position(|j| j.job_id == anchor) {
                self.table_state.select(Some(idx));
            }
        }
    }

    /// Capture current cursor state for restoration.
    pub fn capture_state(&self) -> CapturedState {
        let anchor = self.selected_job().map(|j| j.job_id.clone());
        CapturedState { anchor }
    }

    /// Rebuild column widths based on terminal width and content.
    pub fn rebuild_columns(&mut self, terminal_width: u16, config: &Config) {
        // Get default columns
        let mut cols = jobs_columns();

        // Filter out hidden columns
        if !config.columns.jobs_hidden.is_empty() {
            let hidden_set: std::collections::HashSet<String> =
                config.columns.jobs_hidden.iter().cloned().collect();
            cols.retain(|c| !hidden_set.contains(&c.name));
        }

        // Apply content_max bounds from config
        for col in &mut cols {
            match col.name.as_str() {
                "NAME" => col.content_max = config.jobs.name_max as u16,
                "USER" => col.content_max = config.jobs.user_max as u16,
                "PARTITION" => col.content_max = config.jobs.partition_max as u16,
                "NODELIST(REASON)" => col.content_max = config.jobs.nodelist_reason_max as u16,
                "QOS" => col.content_max = config.jobs.qos_max as u16,
                _ => {}
            }
        }

        // Apply user column order
        if !self.column_order.is_empty() {
            let default_names: Vec<String> = cols.iter().map(|c| c.name.clone()).collect();
            let ordered_names = reconcile_order(&self.column_order, &default_names);

            // Reorder cols to match
            let col_map: HashMap<String, ColumnSpec> =
                cols.into_iter().map(|c| (c.name.clone(), c)).collect();
            cols = ordered_names
                .iter()
                .filter_map(|name| col_map.get(name).cloned())
                .collect();
        }

        // Auto-size to content
        let mut content_widths: HashMap<String, u16> = HashMap::new();
        for col_spec in &cols {
            let header_len = col_spec.name.len() as u16;
            let mut max_content = header_len;

            for job in &self.last_jobs {
                let cell_len = match col_spec.name.as_str() {
                    "JOBID" => job.job_id.len(),
                    "STATE" => job.state.len(),
                    "NAME" => job.name.len(),
                    "USER" => job.user.len(),
                    "TIME" => job.time_used.len(),
                    "TIME_LEFT" => estimate_time_left_width(job),
                    "PARTITION" => job.partition.len(),
                    "NODES" => job.num_nodes.len(),
                    "CPUS" => job.num_cpus.len(),
                    "QOS" => job.qos.len(),
                    "TIME_LIMIT" => job.time_limit.len(),
                    "NODELIST(REASON)" => {
                        if job.nodelist.is_empty() || job.nodelist == "None" {
                            job.reason.len()
                        } else {
                            job.nodelist.len()
                        }
                    }
                    _ => 0,
                } as u16;
                max_content = max_content.max(cell_len);
            }

            // Bound by content_max
            let bounded = max_content.min(col_spec.content_max);
            content_widths.insert(col_spec.name.clone(), bounded);
        }

        // Allocate budget
        let tier = tier_for(terminal_width);
        let allocated = allocate_columns(
            terminal_width.saturating_sub(crate::responsive::CHROME_OVERHEAD),
            &cols,
            tier,
        );

        // Build final widths: min(allocated, content_width)
        self.column_widths.clear();
        for (name, alloc_width) in allocated {
            let content_width = content_widths.get(&name).copied().unwrap_or(8);
            self.column_widths
                .insert(name, alloc_width.min(content_width));
        }
    }

    /// Get the currently selected job, if any.
    pub fn selected_job(&self) -> Option<&Job> {
        self.table_state
            .selected()
            .and_then(|idx| self.last_jobs.get(idx))
    }

    /// Move cursor to next row.
    pub fn cursor_next(&mut self) {
        self.table_state.next();
    }

    /// Move cursor to previous row.
    pub fn cursor_prev(&mut self) {
        self.table_state.prev();
    }

    /// Get the filtered jobs list (post-filter, pre-render).
    pub fn filtered_jobs<'a>(&self, jobs: &'a [Job]) -> Vec<&'a Job> {
        // Return references to jobs in last_jobs order
        self.last_jobs
            .iter()
            .filter_map(|j| jobs.iter().find(|jj| jj.job_id == j.job_id))
            .collect()
    }

    /// Get the cursor row index.
    pub fn cursor_row(&self) -> usize {
        self.table_state.selected().unwrap_or(0)
    }

    /// Get selected jobs (visual mode selection).
    pub fn selected_jobs(&self) -> Vec<&Job> {
        let rows = self.visual_selection.rows();
        rows.iter()
            .filter_map(|&idx| self.last_jobs.get(idx))
            .collect()
    }

    /// Get selection count.
    pub fn selection_count(&self) -> usize {
        self.visual_selection.rows().len()
    }

    /// Handle key input for the jobs view.
    pub fn handle_key(&mut self, key: crossterm::event::KeyEvent) -> bool {
        use crossterm::event::{KeyCode, KeyModifiers};

        // If search input is active, handle search keys first
        if self.search_input_active {
            match (key.code, key.modifiers) {
                (KeyCode::Char(c), KeyModifiers::NONE | KeyModifiers::SHIFT) => {
                    self.search_query.push(c);
                    return true;
                }
                (KeyCode::Backspace, KeyModifiers::NONE) => {
                    self.search_query.pop();
                    return true;
                }
                (KeyCode::Enter, KeyModifiers::NONE) => {
                    // Accept search and stay in input mode
                    return true;
                }
                (KeyCode::Esc, KeyModifiers::NONE) => {
                    // Exit search input mode and clear query
                    self.search_input_active = false;
                    self.clear_search();
                    return true;
                }
                _ => return false,
            }
        }

        // Visual mode keys
        if self.visual_selection.is_active() {
            match (key.code, key.modifiers) {
                (KeyCode::Esc, KeyModifiers::NONE) => {
                    self.visual_selection.exit();
                    return true;
                }
                (KeyCode::Char('y'), KeyModifiers::NONE) => {
                    // Yank handled at app level via status message
                    return true;
                }
                (KeyCode::Down | KeyCode::Char('j'), KeyModifiers::NONE) => {
                    let cursor_row = self.table_state.selected().unwrap_or(0);
                    self.visual_selection
                        .move_cursor(1, self.last_jobs.len(), cursor_row);
                    // Also move table cursor to match visual cursor
                    if let Some(vc) = self.visual_selection.cursor() {
                        self.table_state.select(Some(vc));
                    }
                    return true;
                }
                (KeyCode::Up | KeyCode::Char('k'), KeyModifiers::NONE) => {
                    let cursor_row = self.table_state.selected().unwrap_or(0);
                    self.visual_selection
                        .move_cursor(-1, self.last_jobs.len(), cursor_row);
                    // Also move table cursor to match visual cursor
                    if let Some(vc) = self.visual_selection.cursor() {
                        self.table_state.select(Some(vc));
                    }
                    return true;
                }
                _ => {}
            }
        }

        // Normal mode keys
        match (key.code, key.modifiers) {
            (KeyCode::Down | KeyCode::Char('j'), KeyModifiers::NONE) => {
                self.cursor_next();
                true
            }
            (KeyCode::Up | KeyCode::Char('k'), KeyModifiers::NONE) => {
                self.cursor_prev();
                true
            }
            (KeyCode::Char('m'), KeyModifiers::NONE) => {
                self.toggle_filter_mine();
                true
            }
            (KeyCode::Char('/'), KeyModifiers::NONE) => {
                // Enter search input mode
                self.search_input_active = true;
                true
            }
            (KeyCode::Char('v') | KeyCode::Char('V'), KeyModifiers::NONE) => {
                // Enter visual mode at current cursor
                if let Some(cursor_row) = self.table_state.selected() {
                    self.visual_selection.enter(cursor_row);
                }
                true
            }
            (KeyCode::Char('s'), KeyModifiers::NONE) => {
                self.toggle_sort("state");
                true
            }
            (KeyCode::Char('S'), KeyModifiers::SHIFT) => {
                self.clear_sort();
                true
            }
            (KeyCode::Char('.'), KeyModifiers::NONE) => {
                self.cycle_reorder_target();
                true
            }
            (KeyCode::Char('['), KeyModifiers::NONE) => {
                self.shift_column_left();
                true
            }
            (KeyCode::Char(']'), KeyModifiers::NONE) => {
                self.shift_column_right();
                true
            }
            _ => false,
        }
    }
}

/// Captured state for restore after update.
#[derive(Debug, Clone)]
pub struct CapturedState {
    pub anchor: Option<String>,
}

/// Check if a job matches the search query.
///
/// Searchable fields: name, user, state, partition, qos, reason, nodelist, job_id.
/// Match is case-insensitive. Empty query matches all jobs.
fn job_matches_search(job: &Job, query: &str) -> bool {
    if query.is_empty() {
        return true;
    }
    let q = query.to_lowercase();
    job.name.to_lowercase().contains(&q)
        || job.state.to_lowercase().contains(&q)
        || job.partition.to_lowercase().contains(&q)
        || job.job_id.contains(&q)
        || job.user.to_lowercase().contains(&q)
        || job.qos.to_lowercase().contains(&q)
        || job.reason.to_lowercase().contains(&q)
        || job.nodelist.to_lowercase().contains(&q)
}

/// Default sort key for jobs: (state_priority, job_id).
fn job_sort_key(job: &Job) -> (u8, u64) {
    let priority = STATE_ORDER.get(job.state.as_str()).copied().unwrap_or(3);
    let job_id = job.job_id.parse::<u64>().unwrap_or(0);
    (priority, job_id)
}

/// Compare job IDs as integers.
fn job_id_cmp(a: &Job, b: &Job) -> std::cmp::Ordering {
    let a_id = a.job_id.parse::<u64>().unwrap_or(0);
    let b_id = b.job_id.parse::<u64>().unwrap_or(0);
    a_id.cmp(&b_id)
}

/// Compare numeric strings.
fn safe_int_cmp(a: &str, b: &str) -> std::cmp::Ordering {
    let a_val = a.parse::<i64>().unwrap_or(0);
    let b_val = b.parse::<i64>().unwrap_or(0);
    a_val.cmp(&b_val)
}

/// Estimate display width for TIME_LEFT column.
fn estimate_time_left_width(job: &Job) -> usize {
    // TIME_LEFT is computed from time_limit - time_used
    // For now, just use time_limit width as an estimate
    job.time_limit.len()
}

/// Format seconds into D-HH:MM:SS or HH:MM:SS.
///
/// Returns "—" (em dash) for negative input, matching Python behavior where
/// parse_slurm_duration returns -1 for invalid/unparseable values.
fn format_duration(total_seconds: i64) -> String {
    if total_seconds < 0 {
        return "—".to_string();
    }

    let days = total_seconds / 86400;
    let remainder = total_seconds % 86400;
    let hours = remainder / 3600;
    let remainder = remainder % 3600;
    let minutes = remainder / 60;
    let seconds = remainder % 60;

    if days > 0 {
        format!("{}-{:02}:{:02}:{:02}", days, hours, minutes, seconds)
    } else {
        format!("{:02}:{:02}:{:02}", hours, minutes, seconds)
    }
}

/// Return (display_str, color) for remaining wall-clock time.
///
/// Python's parse_slurm_duration returns -1 for invalid/UNLIMITED/unparseable values.
/// Rust's parse_slurm_duration returns None for the same cases. We map None to -1
/// semantics for parity.
fn time_left(job: &Job) -> (String, Color) {
    use crate::slurm::parse::parse_slurm_duration;

    // Map Option<u64> to signed int (-1 for None)
    let limit_secs = parse_slurm_duration(&job.time_limit)
        .map(|n| n as i64)
        .unwrap_or(-1);

    if limit_secs < 0 {
        return ("UNLIMITED".to_string(), Color::DarkGray);
    }

    let used_secs = parse_slurm_duration(&job.time_used)
        .map(|n| n as i64)
        .unwrap_or(-1);

    if used_secs < 0 {
        return ("—".to_string(), Color::DarkGray);
    }

    let mut remaining = limit_secs - used_secs;
    if remaining < 0 {
        remaining = 0;
    }

    let display = format_duration(remaining);

    let color = if limit_secs == 0 {
        Color::DarkGray
    } else {
        let pct = remaining as f64 / limit_secs as f64;
        if pct > 0.50 {
            Color::Green
        } else if pct >= 0.10 {
            Color::Yellow
        } else {
            Color::Red
        }
    };

    (display, color)
}

/// Render the jobs table.
pub fn render(
    f: &mut ratatui::Frame,
    area: Rect,
    view: &mut JobsView,
    jobs: &[Job],
    config: &Config,
    current_user: &str,
) {
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
            .title("Jobs")
            .style(Style::default().fg(Color::White));
        let paragraph = Paragraph::new(text).block(block);
        f.render_widget(paragraph, area);
        return;
    }

    // Update view with new data
    let state = view.update(jobs.to_vec(), current_user);
    view.rebuild_columns(area.width, config);
    view.restore_state(state);

    // Build header
    let mut header_cells = Vec::new();
    let cols_to_render: Vec<String> = view.column_widths.keys().cloned().collect();

    for (idx, col_name) in cols_to_render.iter().enumerate() {
        let style = if idx == view.reorder_target_idx {
            // Highlight the reorder target column
            Style::default().add_modifier(Modifier::REVERSED | Modifier::BOLD)
        } else {
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD)
        };
        header_cells.push(Cell::from(col_name.as_str()).style(style));
    }
    let header = Row::new(header_cells).height(1);

    // Build rows
    let mut rows = Vec::new();
    for (idx, job) in view.last_jobs.iter().enumerate() {
        let mut cells = Vec::new();

        for col_name in &cols_to_render {
            let width = view.column_widths.get(col_name).copied().unwrap_or(8);
            let content = match col_name.as_str() {
                "JOBID" => job.job_id.clone(),
                "STATE" => job.state.clone(),
                "NAME" => truncate(&job.name, width as usize),
                "USER" => truncate(&job.user, width as usize),
                "TIME" => job.time_used.clone(),
                "TIME_LEFT" => {
                    let (text, _color) = time_left(job);
                    text
                }
                "PARTITION" => truncate(&job.partition, width as usize),
                "NODES" => job.num_nodes.clone(),
                "CPUS" => job.num_cpus.clone(),
                "QOS" => truncate(&job.qos, width as usize),
                "TIME_LIMIT" => job.time_limit.clone(),
                "NODELIST(REASON)" => {
                    if job.nodelist.is_empty() || job.nodelist == "None" {
                        truncate(&job.reason, width as usize)
                    } else {
                        truncate(&job.nodelist, width as usize)
                    }
                }
                _ => String::new(),
            };

            let style = if col_name == "STATE" {
                Style::default().fg(*STATE_COLORS
                    .get(job.state.as_str())
                    .unwrap_or(&Color::White))
            } else {
                Style::default()
            };

            cells.push(Cell::from(content).style(style));
        }

        let row_style = if Some(idx) == view.table_state.selected() {
            Style::default().bg(Color::DarkGray)
        } else {
            Style::default()
        };

        rows.push(Row::new(cells).style(row_style));
    }

    // Build title with filter indicators
    let mut title_parts = vec!["Jobs".to_string()];
    if view.filter_mine {
        title_parts.push(format!("(mine: {})", current_user));
    }
    if !view.search_query.is_empty() {
        title_parts.push(format!("\"{}\"", view.search_query));
    }
    let title = title_parts.join(" · ");

    // Build widths vector
    let widths: Vec<ratatui::layout::Constraint> = cols_to_render
        .iter()
        .map(|name| {
            let w = view.column_widths.get(name).copied().unwrap_or(8);
            ratatui::layout::Constraint::Length(w)
        })
        .collect();

    let table = Table::new(rows, widths)
        .header(header)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(title)
                .style(Style::default().fg(Color::White)),
        )
        .row_highlight_style(Style::default().bg(Color::DarkGray));

    // Convert to ratatui TableState
    let mut ratatui_state = ratatui::widgets::TableState::default();
    ratatui_state.select(view.table_state.selected());
    f.render_stateful_widget(table, area, &mut ratatui_state);

    // Update view state from ratatui state (in case it changed)
    view.table_state.select(ratatui_state.selected());
}

/// Truncate a string to fit width, adding ellipsis if needed.
fn truncate(s: &str, width: usize) -> String {
    if s.len() <= width {
        s.to_string()
    } else if width <= 3 {
        s.chars().take(width).collect()
    } else {
        let mut result: String = s.chars().take(width - 3).collect();
        result.push_str("...");
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[allow(clippy::too_many_arguments)]
    fn make_job(
        job_id: &str,
        name: &str,
        user: &str,
        state: &str,
        partition: &str,
        reason: &str,
        nodelist: &str,
        qos: &str,
    ) -> Job {
        Job {
            job_id: job_id.to_string(),
            name: name.to_string(),
            user: user.to_string(),
            state: state.to_string(),
            partition: partition.to_string(),
            nodes: "1".to_string(),
            num_nodes: "1".to_string(),
            num_cpus: "8".to_string(),
            time_used: "00:02:10".to_string(),
            time_limit: "01:00:00".to_string(),
            reason: reason.to_string(),
            nodelist: nodelist.to_string(),
            qos: qos.to_string(),
        }
    }

    #[test]
    fn test_job_matches_search_empty_query() {
        let job = make_job(
            "12345", "training", "alice", "RUNNING", "compute", "", "node001", "normal",
        );
        assert!(job_matches_search(&job, ""));
    }

    #[test]
    fn test_job_matches_search_name() {
        let job = make_job(
            "12345",
            "training-run",
            "alice",
            "RUNNING",
            "compute",
            "",
            "node001",
            "normal",
        );
        assert!(job_matches_search(&job, "training"));
        assert!(job_matches_search(&job, "TRAINING"));
        assert!(!job_matches_search(&job, "xyz"));
    }

    #[test]
    fn test_job_matches_search_state() {
        let job = make_job(
            "12345", "training", "alice", "RUNNING", "compute", "", "node001", "normal",
        );
        assert!(job_matches_search(&job, "running"));
        assert!(job_matches_search(&job, "RUN"));
    }

    #[test]
    fn test_job_matches_search_user() {
        let job = make_job(
            "12345", "training", "alice", "RUNNING", "compute", "", "node001", "normal",
        );
        assert!(job_matches_search(&job, "alice"));
        assert!(job_matches_search(&job, "ALICE"));
    }

    #[test]
    fn test_job_matches_search_partition() {
        let job = make_job(
            "12345", "training", "alice", "RUNNING", "compute", "", "node001", "normal",
        );
        assert!(job_matches_search(&job, "compute"));
        assert!(job_matches_search(&job, "COMP"));
    }

    #[test]
    fn test_job_matches_search_qos() {
        let job = make_job(
            "12345",
            "training",
            "alice",
            "RUNNING",
            "compute",
            "",
            "node001",
            "high-prio",
        );
        assert!(job_matches_search(&job, "high"));
        assert!(job_matches_search(&job, "PRIO"));
    }

    #[test]
    fn test_job_matches_search_reason() {
        let job = make_job(
            "12345",
            "training",
            "alice",
            "PENDING",
            "compute",
            "Resources",
            "node001",
            "normal",
        );
        assert!(job_matches_search(&job, "resources"));
        assert!(job_matches_search(&job, "RES"));
    }

    #[test]
    fn test_job_matches_search_nodelist() {
        let job = make_job(
            "12345", "training", "alice", "RUNNING", "compute", "", "gpu001", "normal",
        );
        assert!(job_matches_search(&job, "gpu001"));
        assert!(job_matches_search(&job, "GPU"));
    }

    #[test]
    fn test_job_matches_search_job_id() {
        let job = make_job(
            "12345", "training", "alice", "RUNNING", "compute", "", "node001", "normal",
        );
        assert!(job_matches_search(&job, "12345"));
        assert!(job_matches_search(&job, "234"));
    }

    #[test]
    fn test_filter_mine() {
        let jobs = vec![
            make_job("1", "job1", "alice", "RUNNING", "compute", "", "", "normal"),
            make_job("2", "job2", "bob", "RUNNING", "compute", "", "", "normal"),
            make_job("3", "job3", "alice", "PENDING", "compute", "", "", "normal"),
        ];

        let mut view = JobsView::new();
        view.filter_mine = true;
        let filtered = view.apply_filters(&jobs, "alice");

        assert_eq!(filtered.len(), 2);
        assert_eq!(filtered[0].job_id, "1");
        assert_eq!(filtered[1].job_id, "3");
    }

    #[test]
    fn test_search_filter() {
        let jobs = vec![
            make_job(
                "1", "training", "alice", "RUNNING", "compute", "", "", "normal",
            ),
            make_job(
                "2",
                "inference",
                "bob",
                "RUNNING",
                "compute",
                "",
                "",
                "normal",
            ),
            make_job(
                "3",
                "training-v2",
                "alice",
                "PENDING",
                "compute",
                "",
                "",
                "normal",
            ),
        ];

        let mut view = JobsView::new();
        view.search_query = "training".to_string();
        let filtered = view.apply_filters(&jobs, "alice");

        assert_eq!(filtered.len(), 2);
        assert_eq!(filtered[0].job_id, "1");
        assert_eq!(filtered[1].job_id, "3");
    }

    #[test]
    fn test_filter_pipeline_order() {
        let jobs = vec![
            make_job(
                "1", "training", "alice", "RUNNING", "compute", "", "", "normal",
            ),
            make_job(
                "2", "training", "bob", "RUNNING", "compute", "", "", "normal",
            ),
            make_job(
                "3",
                "inference",
                "alice",
                "PENDING",
                "compute",
                "",
                "",
                "normal",
            ),
        ];

        let mut view = JobsView::new();
        view.filter_mine = true;
        view.search_query = "training".to_string();
        let filtered = view.apply_filters(&jobs, "alice");

        // Should apply mine first (keeps 1, 3), then search (keeps only 1)
        assert_eq!(filtered.len(), 1);
        assert_eq!(filtered[0].job_id, "1");
    }

    #[test]
    fn test_default_sort() {
        let jobs = vec![
            make_job("3", "job3", "alice", "PENDING", "compute", "", "", "normal"),
            make_job("1", "job1", "alice", "RUNNING", "compute", "", "", "normal"),
            make_job(
                "2",
                "job2",
                "alice",
                "COMPLETING",
                "compute",
                "",
                "",
                "normal",
            ),
        ];

        let view = JobsView::new();
        let sorted = view.apply_filters(&jobs, "alice");

        // Should sort by state priority: COMPLETING(0), RUNNING(1), PENDING(2)
        assert_eq!(sorted[0].job_id, "2"); // COMPLETING
        assert_eq!(sorted[1].job_id, "1"); // RUNNING
        assert_eq!(sorted[2].job_id, "3"); // PENDING
    }

    #[test]
    fn test_sort_by_state() {
        let jobs = vec![
            make_job("1", "job1", "alice", "RUNNING", "compute", "", "", "normal"),
            make_job("2", "job2", "alice", "PENDING", "compute", "", "", "normal"),
            make_job("3", "job3", "alice", "FAILED", "compute", "", "", "normal"),
        ];

        let mut view = JobsView::new();
        view.toggle_sort("STATE");
        let sorted = view.apply_filters(&jobs, "alice");

        // Alphabetical: FAILED, PENDING, RUNNING
        assert_eq!(sorted[0].state, "FAILED");
        assert_eq!(sorted[1].state, "PENDING");
        assert_eq!(sorted[2].state, "RUNNING");

        // Reverse
        view.toggle_sort("STATE");
        let sorted = view.apply_filters(&jobs, "alice");
        assert_eq!(sorted[0].state, "RUNNING");
        assert_eq!(sorted[1].state, "PENDING");
        assert_eq!(sorted[2].state, "FAILED");
    }

    #[test]
    fn test_truncate() {
        assert_eq!(truncate("hello", 10), "hello");
        assert_eq!(truncate("hello world", 8), "hello...");
        assert_eq!(truncate("hi", 1), "h");
        assert_eq!(truncate("hello", 3), "hel");
    }

    #[test]
    fn test_capture_restore_state() {
        let jobs = vec![
            make_job("1", "job1", "alice", "RUNNING", "compute", "", "", "normal"),
            make_job("2", "job2", "alice", "PENDING", "compute", "", "", "normal"),
            make_job("3", "job3", "alice", "FAILED", "compute", "", "", "normal"),
        ];

        let mut view = JobsView::new();
        view.update(jobs.clone(), "alice");

        // Select second job
        view.table_state.select(Some(1));
        let state = view.capture_state();
        assert_eq!(state.anchor, Some("2".to_string()));

        // Update with re-sorted data
        view.toggle_sort("STATE");
        view.update(jobs, "alice");
        view.restore_state(state);

        // Cursor should track job 2
        let selected = view.selected_job();
        assert!(selected.is_some());
        assert_eq!(selected.unwrap().job_id, "2");
    }

    #[test]
    fn test_format_duration_negative() {
        assert_eq!(format_duration(-1), "—");
        assert_eq!(format_duration(-100), "—");
    }

    #[test]
    fn test_format_duration_zero() {
        assert_eq!(format_duration(0), "00:00:00");
    }

    #[test]
    fn test_format_duration_seconds() {
        assert_eq!(format_duration(45), "00:00:45");
    }

    #[test]
    fn test_format_duration_minutes_seconds() {
        assert_eq!(format_duration(125), "00:02:05");
    }

    #[test]
    fn test_format_duration_hours_minutes_seconds() {
        assert_eq!(format_duration(3661), "01:01:01");
    }

    #[test]
    fn test_format_duration_days() {
        assert_eq!(format_duration(90061), "1-01:01:01");
        assert_eq!(format_duration(172800), "2-00:00:00");
    }

    #[test]
    fn test_time_left_unlimited() {
        let job = make_job("1", "job1", "alice", "RUNNING", "compute", "", "", "normal");
        let mut job = job;
        job.time_limit = "UNLIMITED".to_string();
        job.time_used = "00:10:00".to_string();

        let (text, color) = time_left(&job);
        assert_eq!(text, "UNLIMITED");
        assert_eq!(color, Color::DarkGray);
    }

    #[test]
    fn test_time_left_invalid_used() {
        let job = make_job("1", "job1", "alice", "RUNNING", "compute", "", "", "normal");
        let mut job = job;
        job.time_limit = "01:00:00".to_string();
        job.time_used = "INVALID".to_string();

        let (text, color) = time_left(&job);
        assert_eq!(text, "—");
        assert_eq!(color, Color::DarkGray);
    }

    #[test]
    fn test_time_left_clamp_at_zero() {
        let job = make_job("1", "job1", "alice", "RUNNING", "compute", "", "", "normal");
        let mut job = job;
        job.time_limit = "00:10:00".to_string();
        job.time_used = "00:15:00".to_string();

        let (text, _color) = time_left(&job);
        assert_eq!(text, "00:00:00");
    }

    #[test]
    fn test_time_left_color_green() {
        let job = make_job("1", "job1", "alice", "RUNNING", "compute", "", "", "normal");
        let mut job = job;
        job.time_limit = "01:00:00".to_string(); // 3600 seconds
        job.time_used = "00:10:00".to_string(); // 600 seconds
                                                // Remaining: 3000/3600 = 0.833 > 0.50 -> green

        let (_text, color) = time_left(&job);
        assert_eq!(color, Color::Green);
    }

    #[test]
    fn test_time_left_color_yellow() {
        let job = make_job("1", "job1", "alice", "RUNNING", "compute", "", "", "normal");
        let mut job = job;
        job.time_limit = "01:00:00".to_string(); // 3600 seconds
        job.time_used = "00:50:00".to_string(); // 3000 seconds
                                                // Remaining: 600/3600 = 0.167, >= 0.10 and <= 0.50 -> yellow

        let (_text, color) = time_left(&job);
        assert_eq!(color, Color::Yellow);
    }

    #[test]
    fn test_time_left_color_red() {
        let job = make_job("1", "job1", "alice", "RUNNING", "compute", "", "", "normal");
        let mut job = job;
        job.time_limit = "01:00:00".to_string(); // 3600 seconds
        job.time_used = "00:58:00".to_string(); // 3480 seconds
                                                // Remaining: 120/3600 = 0.033 < 0.10 -> red

        let (_text, color) = time_left(&job);
        assert_eq!(color, Color::Red);
    }

    #[test]
    fn test_time_left_zero_limit() {
        let job = make_job("1", "job1", "alice", "RUNNING", "compute", "", "", "normal");
        let mut job = job;
        job.time_limit = "0".to_string();
        job.time_used = "00:00:00".to_string();

        let (_text, color) = time_left(&job);
        assert_eq!(color, Color::DarkGray);
    }

    #[test]
    fn test_state_persists_across_updates() {
        use crate::config::Config;

        let jobs = vec![
            make_job("1", "job1", "alice", "RUNNING", "compute", "", "", "normal"),
            make_job("2", "job2", "bob", "PENDING", "compute", "", "", "normal"),
            make_job("3", "job3", "alice", "RUNNING", "compute", "", "", "normal"),
        ];

        let config = Config::default();
        let mut view = JobsView::from_config(&config);

        // First update
        view.update(jobs.clone(), "alice");

        // Set some state
        view.set_search_query("job1".to_string());
        view.toggle_filter_mine();
        view.table_state.select(Some(0));

        let captured_search = view.search_query.clone();
        let captured_mine = view.filter_mine;

        // Second update - state should persist
        view.update(jobs.clone(), "alice");

        assert_eq!(view.search_query, captured_search);
        assert_eq!(view.filter_mine, captured_mine);
        // Cursor position will be restored via anchor
    }

    #[test]
    fn test_cursor_anchor_survives_resort() {
        use crate::config::Config;

        let jobs = vec![
            make_job("1", "job1", "alice", "PENDING", "compute", "", "", "normal"),
            make_job("2", "job2", "alice", "RUNNING", "compute", "", "", "normal"),
            make_job("3", "job3", "alice", "FAILED", "compute", "", "", "normal"),
        ];

        let config = Config::default();
        let mut view = JobsView::from_config(&config);

        // First update - default sort (RUNNING, PENDING, FAILED by state priority)
        let state = view.update(jobs.clone(), "alice");
        view.restore_state(state);

        // Select job 2 (which is first in default sort: RUNNING)
        view.table_state.select(Some(0));
        assert_eq!(view.selected_job().unwrap().job_id, "2");

        // Capture state before resort
        let captured = view.capture_state();

        // Sort by STATE (alphabetically: FAILED, PENDING, RUNNING)
        view.toggle_sort("STATE");
        let _state = view.update(jobs, "alice");
        view.restore_state(captured);

        // Job 2 should still be selected, but now at index 2
        assert_eq!(view.selected_job().unwrap().job_id, "2");
    }

    #[test]
    fn test_search_input_mode() {
        let mut view = JobsView::new();
        use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

        // Enter search mode with /
        let key = KeyEvent::new(KeyCode::Char('/'), KeyModifiers::NONE);
        assert!(view.handle_key(key));
        assert!(view.search_input_active);
        assert_eq!(view.search_query, "");

        // Type "test"
        for c in ['t', 'e', 's', 't'] {
            let key = KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE);
            assert!(view.handle_key(key));
        }
        assert_eq!(view.search_query, "test");

        // Escape clears and exits
        let key = KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE);
        assert!(view.handle_key(key));
        assert!(!view.search_input_active);
        assert_eq!(view.search_query, "");
    }

    #[test]
    fn test_visual_mode_selection() {
        let jobs = vec![
            make_job("1", "job1", "alice", "RUNNING", "compute", "", "", "normal"),
            make_job("2", "job2", "alice", "PENDING", "compute", "", "", "normal"),
            make_job("3", "job3", "alice", "FAILED", "compute", "", "", "normal"),
        ];

        let mut view = JobsView::new();
        view.update(jobs, "alice");
        view.table_state.select(Some(0));

        use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

        // Enter visual mode with 'v'
        let key = KeyEvent::new(KeyCode::Char('v'), KeyModifiers::NONE);
        assert!(view.handle_key(key));
        assert!(view.visual_selection.is_active());

        // Press 'j' to move down and extend selection
        let key = KeyEvent::new(KeyCode::Char('j'), KeyModifiers::NONE);
        assert!(view.handle_key(key));

        // Should have selected rows 0 and 1
        let selected = view.selected_jobs();
        assert_eq!(selected.len(), 2);
        assert_eq!(selected[0].job_id, "1");
        assert_eq!(selected[1].job_id, "2");
    }

    #[test]
    fn test_yank_tsv() {
        use crate::views::visual::yank_tsv;
        use std::collections::BTreeSet;

        let jobs = vec![
            make_job("1", "job1", "alice", "RUNNING", "compute", "", "", "normal"),
            make_job("2", "job2", "bob", "PENDING", "gpu", "", "", "high"),
        ];

        let mut rows = BTreeSet::new();
        rows.insert(0);
        rows.insert(1);

        let text = yank_tsv(&rows, &jobs, |job| {
            format!("{}\t{}\t{}\t{}", job.job_id, job.name, job.state, job.user)
        });

        let expected = "1\tjob1\tRUNNING\talice\n2\tjob2\tPENDING\tbob\n";
        assert_eq!(text, expected);
    }

    #[test]
    fn test_clipboard_copy_remote() {
        use crate::clipboard::copy;
        use crate::config::Config;

        let config = Config::default();
        let text = "test data";

        // With remote host, should use OSC52 transport
        let result = copy(text, &config.clipboard, Some("remote.example.com"));
        // Should attempt OSC52 and not fall back to subprocess
        assert_eq!(result.transport, crate::clipboard::Transport::Osc52);
    }

    #[test]
    fn test_cycle_reorder_target_wraps() {
        let mut view = JobsView::new();
        view.column_widths.insert("A".to_string(), 10);
        view.column_widths.insert("B".to_string(), 10);
        view.column_widths.insert("C".to_string(), 10);

        assert_eq!(view.reorder_target_idx, 0);
        view.cycle_reorder_target();
        assert_eq!(view.reorder_target_idx, 1);
        view.cycle_reorder_target();
        assert_eq!(view.reorder_target_idx, 2);
        view.cycle_reorder_target();
        // Should wrap back to 0
        assert_eq!(view.reorder_target_idx, 0);
    }

    #[test]
    fn test_cycle_reorder_target_zero_columns() {
        let mut view = JobsView::new();
        // No columns
        assert_eq!(view.column_widths.len(), 0);
        view.cycle_reorder_target();
        // Should not panic and idx should remain 0
        assert_eq!(view.reorder_target_idx, 0);
    }

    #[test]
    fn test_shift_column_right_and_left() {
        let mut view = JobsView::new();
        view.column_order = vec!["A".to_string(), "B".to_string(), "C".to_string()];
        view.column_widths.insert("A".to_string(), 10);
        view.column_widths.insert("B".to_string(), 10);
        view.column_widths.insert("C".to_string(), 10);

        // Target first column (A) at idx 0
        view.reorder_target_idx = 0;

        // Shift right: A should move to position 1
        view.shift_column_right();
        assert_eq!(
            view.column_order,
            vec!["B".to_string(), "A".to_string(), "C".to_string()]
        );
        assert_eq!(view.reorder_target_idx, 1);

        // Shift left: A should move back to position 0
        view.shift_column_left();
        assert_eq!(
            view.column_order,
            vec!["A".to_string(), "B".to_string(), "C".to_string()]
        );
        assert_eq!(view.reorder_target_idx, 0);
    }

    #[test]
    fn test_shift_at_edges_clamps() {
        let mut view = JobsView::new();
        view.column_order = vec!["A".to_string(), "B".to_string(), "C".to_string()];
        view.column_widths.insert("A".to_string(), 10);
        view.column_widths.insert("B".to_string(), 10);
        view.column_widths.insert("C".to_string(), 10);

        // Target first column
        view.reorder_target_idx = 0;
        let initial_order = view.column_order.clone();

        // Try to shift left at left edge
        view.shift_column_left();
        // Should not move past the start
        assert_eq!(view.column_order, initial_order);
        assert_eq!(view.reorder_target_idx, 0);

        // Target last column
        view.reorder_target_idx = 2;
        let order_before = view.column_order.clone();

        // Try to shift right at right edge
        view.shift_column_right();
        // Should not move past the end
        assert_eq!(view.column_order, order_before);
        assert_eq!(view.reorder_target_idx, 2);
    }

    #[test]
    fn test_reorder_target_visible_space_only() {
        let mut view = JobsView::new();

        // Set up column order with A, B, C
        view.column_order = vec!["A".to_string(), "B".to_string(), "C".to_string()];

        // Only A and C are visible (B is hidden by having no width)
        view.column_widths.insert("A".to_string(), 10);
        view.column_widths.insert("C".to_string(), 10);
        // B has no width (hidden)

        // Reorder target is in visible-space: idx 0 = A, idx 1 = C
        view.reorder_target_idx = 0;
        assert_eq!(view.visible_column_names().len(), 2);

        // Cycling should only iterate over visible columns
        view.cycle_reorder_target();
        assert_eq!(view.reorder_target_idx, 1);
        view.cycle_reorder_target();
        assert_eq!(view.reorder_target_idx, 0); // Wraps back
    }

    #[test]
    fn test_reorder_keys() {
        use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

        let mut view = JobsView::new();
        view.column_widths.insert("A".to_string(), 10);
        view.column_widths.insert("B".to_string(), 10);

        // Test '.' key
        let key = KeyEvent::new(KeyCode::Char('.'), KeyModifiers::NONE);
        assert!(view.handle_key(key));
        assert_eq!(view.reorder_target_idx, 1);

        // Test '[' key (will be no-op at position 1 since we need column_order set up)
        view.column_order = vec!["A".to_string(), "B".to_string()];
        view.reorder_target_idx = 1;
        let key = KeyEvent::new(KeyCode::Char('['), KeyModifiers::NONE);
        assert!(view.handle_key(key));

        // Test ']' key
        view.reorder_target_idx = 0;
        let key = KeyEvent::new(KeyCode::Char(']'), KeyModifiers::NONE);
        assert!(view.handle_key(key));
    }
}

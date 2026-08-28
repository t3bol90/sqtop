//! Nodes view — sinfo-style table with utilization bars.
//!
//! This module implements the Nodes tab, displaying a live table of Slurm nodes
//! with CPU/GPU utilization bars, state filtering, and column reordering.

use crate::columns::{nodes_columns, reconcile_order};
use crate::config::Config;
use crate::responsive::{allocate_columns, tier_for, ColumnSpec, CHROME_OVERHEAD};
use crate::slurm::model::Node;
use crate::views::table_state::{CapturedTableState, CyclicTableState};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::{
    layout::Rect,
    style::{Color, Modifier, Style},
    text::Line,
    widgets::{Block, Borders, Cell, Paragraph, Row, Table, TableState},
    Frame,
};
use std::collections::HashMap;
use std::sync::LazyLock;
use toml;

/// State filter cycle for the Nodes view (SPEC §17.2).
///
/// This is intentionally NOT persisted to config — it is runtime-only state.
/// The cycle order is: "" (all) -> "idle" -> "allocated" -> "mixed" -> "down" -> "gpu" -> "" (wraps)
const FILTER_CYCLE: &[&str] = &["", "idle", "allocated", "mixed", "down", "gpu"];

/// State color mapping for node states.
static STATE_COLORS: LazyLock<HashMap<&str, Color>> = LazyLock::new(|| {
    let mut m = HashMap::new();
    m.insert("idle", Color::Green);
    m.insert("allocated", Color::Cyan);
    m.insert("mixed", Color::Yellow);
    m.insert("down", Color::Red);
    m.insert("drain", Color::Red);
    m.insert("draining", Color::Magenta);
    m.insert("unknown", Color::DarkGray);
    m
});

/// Nodes view state.
///
/// Holds runtime state for the Nodes tab, including filter, sort, column visibility,
/// and cursor position.
#[derive(Debug)]
pub struct NodesView {
    /// Transient runtime filter (SPEC §17.2). NOT persisted to config.
    filter_state: String,
    /// Current sort column: "state", "cpu", "mem", or empty string.
    sort_col: String,
    /// Whether sort is reversed (descending).
    sort_reversed: bool,
    /// Hidden column names.
    hidden_cols: Vec<String>,
    /// User-defined column order.
    column_order: Vec<String>,
    /// Reorder target column index (for keyboard-based column reordering).
    reorder_target_idx: usize,
    /// Mouse drag: column being dragged (visible-space index)
    drag_col_index: Option<usize>,
    /// Mouse drag: press X position (area-local)
    drag_press_x: u16,
    /// Mouse drag: press Y position
    drag_press_y: u16,
    /// Mouse drag: threshold crossed
    dragging: bool,
    /// Cyclic table cursor state.
    table_state: CyclicTableState,
    /// Visual selection state.
    pub visual_selection: crate::views::visual::VisualSelection,
    /// Last filtered and sorted nodes (for cursor restoration).
    pub last_sorted_nodes: Vec<Node>,
    /// Last current columns (name, width) for rendering.
    current_cols: Vec<(String, u16)>,
    /// Rebuild cache: width used for last column rebuild.
    rebuild_cache_width: u16,
    /// Rebuild cache: visible column names from last rebuild.
    rebuild_cache_names: Vec<String>,
    /// Warn threshold for down nodes.
    warn_down_nodes: usize,
    /// Pending config update to persist (set by view actions, consumed by app).
    pending_config_update: Option<HashMap<String, toml::Value>>,
}

impl NodesView {
    /// Create a new NodesView with state loaded from config.
    pub fn new(config: &Config) -> Self {
        let sort_col = config.view_state.nodes_sort_col.clone();
        let sort_col = if matches!(sort_col.as_str(), "state" | "cpu" | "mem") {
            sort_col
        } else {
            String::new()
        };
        let sort_reversed = config.view_state.nodes_sort_reversed;
        let hidden_cols = config.columns.nodes_hidden.clone();
        let saved_order = config.columns.nodes_order.clone();
        let default_order: Vec<String> = nodes_columns().iter().map(|c| c.name.clone()).collect();
        let column_order = reconcile_order(&saved_order, &default_order);
        let warn_down_nodes = config.health.warn_down_nodes.max(0) as usize;

        Self {
            filter_state: String::new(),
            sort_col,
            sort_reversed,
            hidden_cols,
            column_order,
            reorder_target_idx: 0,
            drag_col_index: None,
            drag_press_x: 0,
            drag_press_y: 0,
            dragging: false,
            table_state: CyclicTableState::new(),
            visual_selection: crate::views::visual::VisualSelection::new(),
            last_sorted_nodes: Vec::new(),
            current_cols: Vec::new(),
            rebuild_cache_width: 0,
            rebuild_cache_names: Vec::new(),
            warn_down_nodes,
            pending_config_update: None,
        }
    }

    /// Take pending config update (returns and clears it).
    pub fn take_pending_config_update(&mut self) -> Option<HashMap<String, toml::Value>> {
        self.pending_config_update.take()
    }

    /// Apply the state filter to a list of nodes.
    ///
    /// - `""` is a no-op and returns the input list verbatim.
    /// - `"gpu"` selects GPU-capable nodes (gpu_total > 0).
    /// - `"down"` matches nodes containing "down" OR "drain" in state.
    /// - Other values do case-insensitive substring match against node.state.
    fn apply_state_filter<'a>(&self, nodes: &'a [Node]) -> Vec<&'a Node> {
        if self.filter_state.is_empty() {
            return nodes.iter().collect();
        }
        if self.filter_state == "gpu" {
            return nodes.iter().filter(|n| n.gpu_total > 0).collect();
        }
        if self.filter_state == "down" {
            return nodes
                .iter()
                .filter(|n| {
                    let state_lower = n.state.to_lowercase();
                    state_lower.contains("down") || state_lower.contains("drain")
                })
                .collect();
        }
        let filter_lower = self.filter_state.to_lowercase();
        nodes
            .iter()
            .filter(|n| n.state.to_lowercase().contains(&filter_lower))
            .collect()
    }

    /// Sort and filter nodes.
    fn sorted_visible<'a>(&self, nodes: &'a [Node]) -> Vec<&'a Node> {
        let mut visible: Vec<&Node> = self.apply_state_filter(nodes);

        match self.sort_col.as_str() {
            "state" => {
                visible.sort_by(|a, b| {
                    let cmp = a.state.cmp(&b.state);
                    if self.sort_reversed {
                        cmp.reverse()
                    } else {
                        cmp
                    }
                });
            }
            "cpu" => {
                visible.sort_by(|a, b| {
                    let a_pct = cpu_pct(a);
                    let b_pct = cpu_pct(b);
                    let cmp = a_pct
                        .partial_cmp(&b_pct)
                        .unwrap_or(std::cmp::Ordering::Equal);
                    if self.sort_reversed {
                        cmp.reverse()
                    } else {
                        cmp
                    }
                });
            }
            "mem" => {
                visible.sort_by(|a, b| {
                    let a_mem = free_mem(a);
                    let b_mem = free_mem(b);
                    let cmp = a_mem.cmp(&b_mem);
                    if self.sort_reversed {
                        cmp.reverse()
                    } else {
                        cmp
                    }
                });
            }
            _ => {}
        }

        visible
    }

    /// Rebuild column layout using budget allocation.
    ///
    /// Returns true if the layout changed.
    fn rebuild_columns(&mut self, width: u16, force: bool) -> bool {
        let new_cols = self.visible_cols_filtered(width);
        let visible_names: Vec<String> = new_cols.iter().map(|(n, _)| n.clone()).collect();

        if !force && width == self.rebuild_cache_width && visible_names == self.rebuild_cache_names
        {
            return false;
        }

        self.rebuild_cache_width = width;
        self.rebuild_cache_names = visible_names.clone();

        if !force && new_cols == self.current_cols {
            return false;
        }

        self.current_cols = new_cols;
        true
    }

    /// Return budget-allocated columns for the given terminal width, in user-defined order.
    fn visible_cols_filtered(&self, width: u16) -> Vec<(String, u16)> {
        let budget = width.saturating_sub(CHROME_OVERHEAD);
        let col_specs = nodes_columns();
        let col_map: HashMap<String, ColumnSpec> =
            col_specs.into_iter().map(|c| (c.name.clone(), c)).collect();

        let cols: Vec<ColumnSpec> = self
            .column_order
            .iter()
            .filter_map(|name| {
                if !self.hidden_cols.contains(name) {
                    col_map.get(name).cloned()
                } else {
                    None
                }
            })
            .collect();

        allocate_columns(budget, &cols, tier_for(width))
    }

    /// Capture table state for restoration after refresh/re-sort.
    fn capture_table_state(&self) -> CapturedTableState {
        let anchor = self
            .table_state
            .selected()
            .and_then(|idx| self.last_sorted_nodes.get(idx).map(|n| n.name.clone()));
        CapturedTableState::new(anchor)
    }

    /// Restore table state after refresh/re-sort.
    fn restore_table_state(&mut self, state: &CapturedTableState) {
        if self.last_sorted_nodes.is_empty() {
            return;
        }
        if let Some(idx) = state.restore(self.last_sorted_nodes.len(), |i| {
            self.last_sorted_nodes.get(i).map(|n| n.name.clone())
        }) {
            self.table_state.select(Some(idx));
        } else if let Some(selected) = self.table_state.selected() {
            let clamped = selected.min(self.last_sorted_nodes.len().saturating_sub(1));
            self.table_state.select(Some(clamped));
        } else {
            self.table_state.select(Some(0));
        }
    }

    /// Cycle the state filter to the next value.
    pub fn cycle_state_filter(&mut self) {
        let current_idx = FILTER_CYCLE
            .iter()
            .position(|&f| f == self.filter_state)
            .unwrap_or(0);
        let next_idx = (current_idx + 1) % FILTER_CYCLE.len();
        self.filter_state = FILTER_CYCLE[next_idx].to_string();
    }

    /// Set the sort column and direction.
    pub fn set_sort(&mut self, col: &str) {
        if self.sort_col == col {
            self.sort_reversed = !self.sort_reversed;
        } else {
            self.sort_col = col.to_string();
            self.sort_reversed = false;
        }
        // Persist sort state
        let mut view_state = toml::Table::new();
        view_state.insert(
            "nodes_sort_col".to_string(),
            toml::Value::String(self.sort_col.clone()),
        );
        view_state.insert(
            "nodes_sort_reversed".to_string(),
            toml::Value::Boolean(self.sort_reversed),
        );
        let mut update = HashMap::new();
        update.insert("view_state".to_string(), toml::Value::Table(view_state));
        self.pending_config_update = Some(update);
    }
    /// Cycle the reorder target to the next visible column (wraps).
    pub fn cycle_reorder_target(&mut self) {
        let visible_count = self.current_cols.len();
        if visible_count > 0 {
            self.reorder_target_idx = (self.reorder_target_idx + 1) % visible_count;
        }
    }

    /// Shift the targeted column left in the absolute column_order.
    pub fn shift_column_left(&mut self) {
        if self.current_cols.is_empty() || self.reorder_target_idx >= self.current_cols.len() {
            return;
        }

        let target_name = &self.current_cols[self.reorder_target_idx].0;

        // Find position in absolute column_order
        let abs_idx = self.column_order.iter().position(|n| n == target_name);
        if let Some(idx) = abs_idx {
            if idx > 0 {
                // Move left in absolute order
                self.column_order.swap(idx, idx - 1);
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
                columns.insert("nodes_order".to_string(), toml::Value::Array(order_array));
                let mut update = HashMap::new();
                update.insert("columns".to_string(), toml::Value::Table(columns));
                self.pending_config_update = Some(update);
            }
        }
    }

    /// Shift the targeted column right in the absolute column_order.
    pub fn shift_column_right(&mut self) {
        if self.current_cols.is_empty() || self.reorder_target_idx >= self.current_cols.len() {
            return;
        }

        let target_name = &self.current_cols[self.reorder_target_idx].0;

        // Find position in absolute column_order
        let abs_idx = self.column_order.iter().position(|n| n == target_name);
        if let Some(idx) = abs_idx {
            if idx < self.column_order.len() - 1 {
                // Move right in absolute order
                self.column_order.swap(idx, idx + 1);
                // Clamp target index
                let visible_count = self.current_cols.len();
                self.reorder_target_idx = (self.reorder_target_idx + 1).min(visible_count - 1);
                // Persist column order
                let mut columns = toml::Table::new();
                let order_array: Vec<toml::Value> = self
                    .column_order
                    .iter()
                    .map(|s| toml::Value::String(s.clone()))
                    .collect();
                columns.insert("nodes_order".to_string(), toml::Value::Array(order_array));
                let mut update = HashMap::new();
                update.insert("columns".to_string(), toml::Value::Table(columns));
                self.pending_config_update = Some(update);
            }
        }
    }

    /// Handle mouse down event on header - start drag if on header row.
    pub fn on_mouse_down(&mut self, mouse_x: u16, mouse_y: u16, area: ratatui::layout::Rect) {
        // Check if click is on header row (first row of the table area)
        if mouse_y != area.y {
            return;
        }

        // Map X coordinate to column index using current column widths
        let col_idx = self.x_to_col_index(mouse_x, area);
        if let Some(idx) = col_idx {
            self.drag_col_index = Some(idx);
            self.drag_press_x = mouse_x;
            self.drag_press_y = mouse_y;
            self.dragging = false;
        }
    }

    /// Handle mouse move - activate drag mode if threshold crossed.
    pub fn on_mouse_move(&mut self, mouse_x: u16, _mouse_y: u16) {
        if self.drag_col_index.is_none() {
            return;
        }
        let delta = (mouse_x as i32 - self.drag_press_x as i32).unsigned_abs();
        if delta >= 2 {
            // DRAG_THRESHOLD_CELLS
            self.dragging = true;
        }
    }

    /// Handle mouse up - complete or cancel drag.
    pub fn on_mouse_up(&mut self, mouse_x: u16, _mouse_y: u16, area: ratatui::layout::Rect) {
        if let Some(from_idx) = self.drag_col_index {
            if self.dragging {
                // Find the insertion boundary
                let to_idx = self.x_to_boundary_index(mouse_x, area);
                if from_idx != to_idx {
                    // Perform the reorder using move_in_order
                    self.reorder_column_drag(from_idx, to_idx);
                }
            }
        }
        self.reset_drag_state();
    }

    /// Cancel drag (called on Escape key).
    pub fn cancel_drag(&mut self) -> bool {
        if self.dragging {
            self.reset_drag_state();
            true
        } else {
            false
        }
    }

    /// Reset drag state.
    fn reset_drag_state(&mut self) {
        self.drag_col_index = None;
        self.drag_press_x = 0;
        self.drag_press_y = 0;
        self.dragging = false;
    }

    /// Map area-local X coordinate to column index (0-based visible column).
    fn x_to_col_index(&self, x: u16, area: ratatui::layout::Rect) -> Option<usize> {
        if x < area.x {
            return None;
        }
        let widget_x = x - area.x;
        let mut pos = 0u16;

        for (idx, (_, width)) in self.current_cols.iter().enumerate() {
            if widget_x >= pos && widget_x < pos + width {
                return Some(idx);
            }
            pos += width;
        }

        None
    }

    /// Map area-local X coordinate to nearest boundary index (for insertion).
    fn x_to_boundary_index(&self, x: u16, area: ratatui::layout::Rect) -> usize {
        if x < area.x {
            return 0;
        }
        let widget_x = x - area.x;
        let boundaries = self.column_boundaries();

        if boundaries.is_empty() {
            return 0;
        }

        // Find closest boundary
        let mut best_idx = 0;
        let mut best_dist = (widget_x as i32 - boundaries[0] as i32).unsigned_abs();

        for (i, &boundary) in boundaries.iter().enumerate().skip(1) {
            let dist = (widget_x as i32 - boundary as i32).unsigned_abs();
            if dist < best_dist {
                best_dist = dist;
                best_idx = i;
            }
        }

        best_idx.min(self.current_cols.len())
    }

    /// Get column boundaries (cumulative widths).
    fn column_boundaries(&self) -> Vec<u16> {
        let mut boundaries = vec![0];
        let mut pos = 0;
        for (_, width) in &self.current_cols {
            pos += width;
            boundaries.push(pos);
        }
        boundaries
    }

    /// Perform column reorder from drag.
    fn reorder_column_drag(&mut self, from_idx: usize, to_idx: usize) {
        use crate::columns::move_in_order;

        if self.current_cols.is_empty() || from_idx >= self.current_cols.len() {
            return;
        }

        let visible: Vec<String> = self
            .current_cols
            .iter()
            .map(|(name, _)| name.clone())
            .collect();
        let target_name = &visible[from_idx];

        // Calculate the insertion point in absolute column_order
        let before = if to_idx == 0 {
            // Moving to the start
            Some(visible.first().map(|s| s.as_str()).unwrap_or(""))
        } else if to_idx >= visible.len() {
            // Moving to the end
            None
        } else {
            // Moving before visible[to_idx]
            Some(visible.get(to_idx).map(|s| s.as_str()).unwrap_or(""))
        };

        self.column_order = move_in_order(&self.column_order, target_name, before);

        // Persist column order
        let mut columns = toml::Table::new();
        let order_array: Vec<toml::Value> = self
            .column_order
            .iter()
            .map(|s| toml::Value::String(s.clone()))
            .collect();
        columns.insert("nodes_order".to_string(), toml::Value::Array(order_array));
        let mut update = HashMap::new();
        update.insert("columns".to_string(), toml::Value::Table(columns));
        self.pending_config_update = Some(update);
    }

    /// Handle key events for the Nodes view.
    ///
    /// Returns true if the key was handled, false otherwise.
    pub fn handle_key(&mut self, key: KeyEvent) -> bool {
        // Cancel drag on Escape (if dragging)
        if key.code == KeyCode::Esc && self.cancel_drag() {
            return true;
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
                        .move_cursor(1, self.last_sorted_nodes.len(), cursor_row);
                    // Also move table cursor to match visual cursor
                    if let Some(vc) = self.visual_selection.cursor() {
                        self.table_state.select(Some(vc));
                    }
                    return true;
                }
                (KeyCode::Up | KeyCode::Char('k'), KeyModifiers::NONE) => {
                    let cursor_row = self.table_state.selected().unwrap_or(0);
                    self.visual_selection
                        .move_cursor(-1, self.last_sorted_nodes.len(), cursor_row);
                    // Also move table cursor to match visual cursor
                    if let Some(vc) = self.visual_selection.cursor() {
                        self.table_state.select(Some(vc));
                    }
                    return true;
                }
                _ => {}
            }
        }

        match (key.code, key.modifiers) {
            (KeyCode::Char('f'), KeyModifiers::NONE) => {
                self.cycle_state_filter();
                true
            }
            (KeyCode::Char('s'), KeyModifiers::NONE) => {
                self.set_sort("state");
                true
            }
            (KeyCode::Char('p'), KeyModifiers::NONE) => {
                self.set_sort("cpu");
                true
            }
            (KeyCode::Char('m'), KeyModifiers::NONE) => {
                self.set_sort("mem");
                true
            }
            (KeyCode::Char('v') | KeyCode::Char('V'), KeyModifiers::NONE | KeyModifiers::SHIFT) => {
                // Enter visual mode at current cursor
                if let Some(cursor_row) = self.table_state.selected() {
                    self.visual_selection.enter(cursor_row);
                }
                true
            }
            (KeyCode::Down | KeyCode::Char('j'), KeyModifiers::NONE) => {
                self.table_state.next();
                true
            }
            (KeyCode::Up | KeyCode::Char('k'), KeyModifiers::NONE) => {
                self.table_state.prev();
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

    /// Render the Nodes view.
    pub fn render(&mut self, f: &mut Frame, nodes: &[Node], area: Rect) {
        // Rebuild columns if needed
        let _width_changed = self.rebuild_columns(area.width, false);

        // Capture state before update
        let state = self.capture_table_state();

        // Filter and sort nodes
        let sorted: Vec<&Node> = self.sorted_visible(nodes);
        self.last_sorted_nodes = sorted.iter().map(|&n| n.clone()).collect();
        self.table_state.set_row_count(self.last_sorted_nodes.len());

        // Restore cursor position
        self.restore_table_state(&state);

        // Render header
        let header_area = Rect {
            x: area.x,
            y: area.y,
            width: area.width,
            height: 1,
        };
        self.render_header(f, nodes, header_area, tier_for(area.width));

        // Render table
        let table_area = Rect {
            x: area.x,
            y: area.y + 1,
            width: area.width,
            height: area.height.saturating_sub(1),
        };
        self.render_table(f, table_area);
    }

    /// Get the currently selected node.
    /// Get the currently selected node.
    pub fn current_node<'a>(&self, nodes: &'a [Node]) -> Option<&'a Node> {
        let selected = self.table_state.selected()?;
        // Use last_sorted_nodes to get the index, then find that node in the input
        let node_in_sorted = self.last_sorted_nodes.get(selected)?;
        // Find the same node in the input slice by name
        nodes.iter().find(|n| n.name == node_in_sorted.name)
    }

    /// Render the header line.
    fn render_header(
        &self,
        f: &mut Frame,
        all_nodes: &[Node],
        area: Rect,
        tier: crate::responsive::Tier,
    ) {
        // Count states
        let mut idle = 0;
        let mut alloc = 0;
        let mut mixed = 0;
        let mut down = 0;

        for node in all_nodes {
            let s = node.state.to_lowercase();
            if s.contains("idle") {
                idle += 1;
            } else if s.contains("alloc") {
                alloc += 1;
            } else if s.contains("mixed") {
                mixed += 1;
            }
            if s.contains("down") || s.contains("drain") {
                down += 1;
            }
        }

        let filter_tag = if !self.filter_state.is_empty() {
            format!("  · {}", self.filter_state.to_uppercase())
        } else {
            String::new()
        };

        let sort_tag = if !self.sort_col.is_empty() {
            let arrow = if self.sort_reversed { "↑" } else { "↓" };
            format!("  sort:{}{}", self.sort_col, arrow)
        } else {
            String::new()
        };

        let warn_tag = if down >= self.warn_down_nodes {
            format!("  ! {} DOWN/DRAIN", down)
        } else {
            String::new()
        };

        let text = if tier == crate::responsive::Tier::Xs {
            // xs: compact — most signal-bearing pair: idle / down
            let warn = if down >= self.warn_down_nodes {
                format!("  ! {} DOWN", down)
            } else {
                String::new()
            };
            format!("sinfo  {} idle  {} down{}", idle, down, warn)
        } else {
            format!(
                "sinfo  {} idle  {} alloc  {} mixed  {} down  {} total{}{}{}",
                idle,
                alloc,
                mixed,
                down,
                all_nodes.len(),
                filter_tag,
                sort_tag,
                warn_tag
            )
        };

        let line = Line::from(text);
        let paragraph = Paragraph::new(line);
        f.render_widget(paragraph, area);
    }

    /// Render the table.
    fn render_table(&mut self, f: &mut Frame, area: Rect) {
        if self.last_sorted_nodes.is_empty() {
            let text = vec![Line::from("No nodes found")];
            let block = Block::default().borders(Borders::ALL).title("Nodes");
            let paragraph = Paragraph::new(text).block(block);
            f.render_widget(paragraph, area);
            return;
        }

        // Build header row
        let header_cells: Vec<Cell> = self
            .current_cols
            .iter()
            .enumerate()
            .map(|(idx, (name, _))| {
                if idx == self.reorder_target_idx % self.current_cols.len().max(1) {
                    Cell::from(name.clone())
                        .style(Style::default().add_modifier(Modifier::REVERSED | Modifier::BOLD))
                } else {
                    Cell::from(name.clone()).style(Style::default().add_modifier(Modifier::BOLD))
                }
            })
            .collect();
        let header = Row::new(header_cells).height(1);

        // Build data rows
        let rows: Vec<Row> = self
            .last_sorted_nodes
            .iter()
            .map(|node| {
                let cells: Vec<Cell> = self
                    .current_cols
                    .iter()
                    .map(|(name, _)| self.render_cell(node, name))
                    .collect();
                Row::new(cells).height(1)
            })
            .collect();

        // Column widths
        let widths: Vec<ratatui::layout::Constraint> = self
            .current_cols
            .iter()
            .map(|(_, w)| ratatui::layout::Constraint::Length(*w))
            .collect();

        let block = Block::default().borders(Borders::ALL).title("Nodes");

        let table = Table::new(rows, widths)
            .header(header)
            .block(block)
            .row_highlight_style(Style::default().add_modifier(Modifier::REVERSED));

        // Convert CyclicTableState to ratatui TableState
        let mut ratatui_state = TableState::default();
        if let Some(selected) = self.table_state.selected() {
            ratatui_state.select(Some(selected));
        }

        f.render_stateful_widget(table, area, &mut ratatui_state);
    }

    /// Render a single cell for a node and column.
    fn render_cell(&self, node: &Node, col_name: &str) -> Cell<'static> {
        match col_name {
            "NODE" => {
                Cell::from(node.name.clone()).style(Style::default().add_modifier(Modifier::BOLD))
            }
            "STATE" => {
                let state_normalized = node.state.to_lowercase();
                let state_lower = state_normalized
                    .split('*')
                    .next()
                    .unwrap_or("")
                    .trim_end_matches('-');
                let color = STATE_COLORS
                    .get(state_lower)
                    .copied()
                    .unwrap_or(Color::White);
                Cell::from(node.state.clone()).style(Style::default().fg(color))
            }
            "CPU%" => {
                let (text, color) = cpu_bar(&node.cpus_alloc, &node.cpus_total);
                if let Some(c) = color {
                    Cell::from(text).style(Style::default().fg(c))
                } else {
                    Cell::from(text)
                }
            }
            "GPU%" => {
                let (text, color) = gpu_bar(node.gpu_alloc, node.gpu_total);
                if let Some(c) = color {
                    Cell::from(text).style(Style::default().fg(c))
                } else {
                    Cell::from(text)
                }
            }
            "CPUS A/T" => Cell::from(format!("{}/{}", node.cpus_alloc, node.cpus_total)),
            "GPU A/T" => {
                if node.gpu_total > 0 {
                    let free = node.gpu_total - node.gpu_alloc;
                    let color = if free > 0 { Color::Green } else { Color::Red };
                    Cell::from(format!("{}/{}", node.gpu_alloc, node.gpu_total))
                        .style(Style::default().fg(color))
                } else {
                    Cell::from("—").style(Style::default().fg(Color::DarkGray))
                }
            }
            "MEM FREE" => Cell::from(format!("{}M", node.memory_free)),
            "PARTITION" => Cell::from(node.partition.clone()),
            "MEM TOTAL" => Cell::from(format!("{}M", node.memory_total)),
            "LOAD" => Cell::from(node.load.clone()),
            _ => Cell::from(""),
        }
    }
}

/// Calculate CPU percentage for a node.
fn cpu_pct(node: &Node) -> f64 {
    let alloc = node.cpus_alloc.parse::<i64>().unwrap_or(0);
    let total = node.cpus_total.parse::<i64>().unwrap_or(0);
    if total > 0 {
        (alloc as f64) / (total as f64)
    } else {
        0.0
    }
}

/// Calculate free memory for a node.
fn free_mem(node: &Node) -> i64 {
    node.memory_free.parse::<i64>().unwrap_or(0)
}

/// Render a CPU utilization bar.
///
/// Matches Python `_cpu_bar`: returns "─" * 8 on parse error or total==0,
/// otherwise computes pct = round(a/t*100), filled = round(pct/100*8).
fn cpu_bar(alloc: &str, total: &str) -> (String, Option<Color>) {
    // Parse both; on failure return box-drawing dashes
    let a = match alloc.parse::<i64>() {
        Ok(v) => v,
        Err(_) => return ("────────".to_string(), None),
    };
    let t = match total.parse::<i64>() {
        Ok(v) => v,
        Err(_) => return ("────────".to_string(), None),
    };

    // If total is 0, return dashes
    if t == 0 {
        return ("────────".to_string(), None);
    }

    // Compute percentage (round half away from zero, matching Python)
    let pct = ((a as f64 / t as f64) * 100.0).round_ties_even() as u8;

    // Compute filled cells
    let bar_width = 8;
    let filled = ((pct as f64 / 100.0) * (bar_width as f64)).round_ties_even() as usize;

    // Build bar
    let bar = "█".repeat(filled) + &"░".repeat(bar_width - filled);

    // Determine color: green if pct < 60, yellow if pct < 90, else red
    let color = if pct < 60 {
        Color::Green
    } else if pct < 90 {
        Color::Yellow
    } else {
        Color::Red
    };

    (format!("{} {:3}%", bar, pct), Some(color))
}

/// Render a GPU utilization bar.
///
/// Matches Python `_gpu_bar`: returns "—" (single em-dash) when total==0,
/// otherwise computes pct = round(a/t*100), filled = round(pct/100*8).
fn gpu_bar(alloc: u32, total: u32) -> (String, Option<Color>) {
    // If total is 0, return single em-dash with dim color
    if total == 0 {
        return ("—".to_string(), Some(Color::DarkGray));
    }

    // Compute percentage
    let pct = ((alloc as f64 / total as f64) * 100.0).round_ties_even() as u8;

    // Compute filled cells
    let bar_width = 8;
    let filled = ((pct as f64 / 100.0) * (bar_width as f64)).round_ties_even() as usize;

    // Build bar
    let bar = "█".repeat(filled) + &"░".repeat(bar_width - filled);

    // Determine color
    let color = if pct < 60 {
        Color::Green
    } else if pct < 90 {
        Color::Yellow
    } else {
        Color::Red
    };

    (format!("{} {:3}%", bar, pct), Some(color))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;

    fn make_node(name: &str, state: &str, gpu_total: u32, gpu_alloc: u32, partition: &str) -> Node {
        Node {
            name: name.to_string(),
            state: state.to_string(),
            partition: partition.to_string(),
            cpus_total: "64".to_string(),
            cpus_alloc: "0".to_string(),
            memory_total: "256000".to_string(),
            memory_free: "200000".to_string(),
            load: "0.10".to_string(),
            gpu_total,
            gpu_alloc,
        }
    }

    // ── Filter helper unit tests ──────────────────────────────────────────────

    #[test]
    fn test_apply_state_filter_idle_only() {
        let config = Config::default();
        let mut view = NodesView::new(&config);
        let nodes = vec![
            make_node("c1", "idle", 0, 0, "main"),
            make_node("c2", "idle*", 0, 0, "main"), // decorated
            make_node("c3", "allocated", 0, 0, "main"),
            make_node("c4", "mixed", 0, 0, "main"),
            make_node("c5", "down", 0, 0, "main"),
            make_node("c6", "drain", 0, 0, "main"),
            make_node("c7", "drained", 0, 0, "main"),
        ];
        view.filter_state = "idle".to_string();
        let out = view.apply_state_filter(&nodes);
        let names: Vec<&str> = out.iter().map(|n| n.name.as_str()).collect();
        assert_eq!(names, vec!["c1", "c2"]);
    }

    #[test]
    fn test_apply_state_filter_allocated_substring() {
        let config = Config::default();
        let mut view = NodesView::new(&config);
        let nodes = vec![
            make_node("c1", "idle", 0, 0, "main"),
            make_node("c2", "allocated", 0, 0, "main"),
            make_node("c3", "ALLOCATED*", 0, 0, "main"), // decorated + uppercase
            make_node("c4", "mixed", 0, 0, "main"),
            make_node("c5", "down", 0, 0, "main"),
        ];
        view.filter_state = "allocated".to_string();
        let out = view.apply_state_filter(&nodes);
        let names: Vec<&str> = out.iter().map(|n| n.name.as_str()).collect();
        assert_eq!(names, vec!["c2", "c3"]);
    }

    #[test]
    fn test_apply_state_filter_mixed_with_decoration() {
        let config = Config::default();
        let mut view = NodesView::new(&config);
        let nodes = vec![
            make_node("c1", "mixed", 0, 0, "main"),
            make_node("c2", "mixed-", 0, 0, "main"),
            make_node("c3", "mixed*", 0, 0, "main"),
            make_node("c4", "idle", 0, 0, "main"),
            make_node("c5", "allocated", 0, 0, "main"),
        ];
        view.filter_state = "mixed".to_string();
        let out = view.apply_state_filter(&nodes);
        let names: Vec<&str> = out.iter().map(|n| n.name.as_str()).collect();
        assert_eq!(names, vec!["c1", "c2", "c3"]);
    }

    #[test]
    fn test_apply_state_filter_down_combines_drain() {
        let config = Config::default();
        let mut view = NodesView::new(&config);
        let nodes = vec![
            make_node("c1", "down", 0, 0, "main"),
            make_node("c2", "drain", 0, 0, "main"),
            make_node("c3", "drained", 0, 0, "main"),
            make_node("c4", "idle+drain", 0, 0, "main"), // decorated drain combo
            make_node("c5", "idle", 0, 0, "main"),
            make_node("c6", "allocated", 0, 0, "main"),
            make_node("c7", "mixed", 0, 0, "main"),
        ];
        view.filter_state = "down".to_string();
        let out = view.apply_state_filter(&nodes);
        let names: Vec<&str> = out.iter().map(|n| n.name.as_str()).collect();
        // Down/drain/drained/idle+drain all pass; idle/allocated/mixed do not.
        assert_eq!(names, vec!["c1", "c2", "c3", "c4"]);
    }

    #[test]
    fn test_apply_state_filter_gpu_only() {
        let config = Config::default();
        let mut view = NodesView::new(&config);
        let nodes = vec![
            make_node("c1", "idle", 0, 0, "main"),
            make_node("c2", "allocated", 0, 0, "main"),
            make_node("c3", "idle", 4, 0, "main"),
            make_node("c4", "mixed", 2, 0, "main"),
        ];
        view.filter_state = "gpu".to_string();
        let out = view.apply_state_filter(&nodes);
        let names: Vec<&str> = out.iter().map(|n| n.name.as_str()).collect();
        assert_eq!(names, vec!["c3", "c4"]);
    }

    #[test]
    fn test_apply_state_filter_empty_returns_all() {
        let config = Config::default();
        let mut view = NodesView::new(&config);
        let nodes = vec![
            make_node("c1", "idle", 0, 0, "main"),
            make_node("c2", "down", 0, 0, "main"),
            make_node("c3", "allocated", 4, 0, "main"),
        ];
        view.filter_state = String::new();
        let out = view.apply_state_filter(&nodes);
        // Empty filter returns all nodes
        assert_eq!(out.len(), 3);
        let names: Vec<&str> = out.iter().map(|n| n.name.as_str()).collect();
        assert_eq!(names, vec!["c1", "c2", "c3"]);
    }

    // ── action_cycle_state_filter wiring ──────────────────────────────────────

    #[test]
    fn test_cycle_state_filter_advances_through_cycle() {
        let config = Config::default();
        let mut view = NodesView::new(&config);

        // Expected cycle: "" -> "idle" -> "allocated" -> "mixed" -> "down" -> "gpu" -> ""
        let expected = ["idle", "allocated", "mixed", "down", "gpu", ""];

        assert_eq!(view.filter_state, "");

        for expected_state in &expected {
            view.cycle_state_filter();
            assert_eq!(view.filter_state, *expected_state);
        }

        // One more cycle should wrap back to "idle"
        view.cycle_state_filter();
        assert_eq!(view.filter_state, "idle");
    }

    #[test]
    fn test_filter_composes_with_sort() {
        let config = Config::default();
        let mut view = NodesView::new(&config);
        let nodes = vec![
            make_node("c1", "idle", 0, 0, "main"),
            make_node("c2", "down", 0, 0, "main"),
            make_node("c3", "drain", 0, 0, "main"),
            make_node("c4", "allocated", 0, 0, "main"),
        ];
        view.filter_state = "down".to_string();
        view.sort_col = "state".to_string();
        view.sort_reversed = false;

        let out = view.sorted_visible(&nodes);
        let names: Vec<&str> = out.iter().map(|n| n.name.as_str()).collect();
        // Only down + drain pass the filter; sorted by state ascending.
        assert_eq!(names, vec!["c2", "c3"]);
    }

    // ── Sort tests ─────────────────────────────────────────────────────────────

    #[test]
    fn test_sort_by_state() {
        let config = Config::default();
        let mut view = NodesView::new(&config);
        let nodes = vec![
            make_node("c1", "mixed", 0, 0, "main"),
            make_node("c2", "allocated", 0, 0, "main"),
            make_node("c3", "idle", 0, 0, "main"),
        ];
        view.sort_col = "state".to_string();
        view.sort_reversed = false;

        let out = view.sorted_visible(&nodes);
        let names: Vec<&str> = out.iter().map(|n| n.name.as_str()).collect();
        assert_eq!(names, vec!["c2", "c3", "c1"]); // allocated, idle, mixed
    }

    #[test]
    fn test_sort_by_cpu() {
        let config = Config::default();
        let mut view = NodesView::new(&config);
        let mut nodes = vec![
            make_node("c1", "idle", 0, 0, "main"),
            make_node("c2", "mixed", 0, 0, "main"),
            make_node("c3", "allocated", 0, 0, "main"),
        ];
        // Modify CPU allocations
        nodes[0].cpus_alloc = "32".to_string(); // c1: 32/64 = 50%
        nodes[1].cpus_alloc = "16".to_string(); // c2: 16/64 = 25%
        nodes[2].cpus_alloc = "48".to_string(); // c3: 48/64 = 75%

        view.sort_col = "cpu".to_string();
        view.sort_reversed = false;

        let out = view.sorted_visible(&nodes);
        let names: Vec<&str> = out.iter().map(|n| n.name.as_str()).collect();
        assert_eq!(names, vec!["c2", "c1", "c3"]); // 25%, 50%, 75%
    }

    #[test]
    fn test_sort_by_mem() {
        let config = Config::default();
        let mut view = NodesView::new(&config);
        let mut nodes = vec![
            make_node("c1", "idle", 0, 0, "main"),
            make_node("c2", "mixed", 0, 0, "main"),
            make_node("c3", "allocated", 0, 0, "main"),
        ];
        // Modify memory free
        nodes[0].memory_free = "100000".to_string();
        nodes[1].memory_free = "300000".to_string();
        nodes[2].memory_free = "200000".to_string();

        view.sort_col = "mem".to_string();
        view.sort_reversed = false;

        let out = view.sorted_visible(&nodes);
        let names: Vec<&str> = out.iter().map(|n| n.name.as_str()).collect();
        assert_eq!(names, vec!["c1", "c3", "c2"]); // 100k, 200k, 300k
    }

    #[test]
    fn test_set_sort_toggles_reversed() {
        let config = Config::default();
        let mut view = NodesView::new(&config);

        view.set_sort("state");
        assert_eq!(view.sort_col, "state");
        assert!(!view.sort_reversed);

        view.set_sort("state");
        assert_eq!(view.sort_col, "state");
        assert!(view.sort_reversed);

        view.set_sort("cpu");
        assert_eq!(view.sort_col, "cpu");
        assert!(!view.sort_reversed);
    }

    // ── Filter cycle order ─────────────────────────────────────────────────────

    #[test]
    fn test_filter_cycle_order_matches_spec() {
        // Cycle order is fixed: "" -> idle -> allocated -> mixed -> down -> gpu -> ""
        assert_eq!(
            FILTER_CYCLE,
            &["", "idle", "allocated", "mixed", "down", "gpu"]
        );
    }

    // ── State persistence tests ───────────────────────────────────────────────

    #[test]
    fn test_filter_state_persists_across_operations() {
        let config = Config::default();
        let mut view = NodesView::new(&config);

        // Cycle filter twice
        view.cycle_state_filter();
        assert_eq!(view.filter_state, "idle");
        view.cycle_state_filter();
        assert_eq!(view.filter_state, "allocated");

        // State should still be "allocated"
        assert_eq!(view.filter_state, "allocated");
    }

    #[test]
    fn test_sort_state_persists_after_render() {
        let config = Config::default();
        let mut view = NodesView::new(&config);

        // Set sort column
        view.set_sort("cpu");
        assert_eq!(view.sort_col, "cpu");
        assert!(!view.sort_reversed);

        // Simulate a render by calling sorted_visible
        let nodes = vec![make_node("c1", "idle", 0, 0, "main")];
        let _ = view.sorted_visible(&nodes);

        // Sort state should persist
        assert_eq!(view.sort_col, "cpu");
        assert!(!view.sort_reversed);
    }

    #[test]
    fn test_cursor_position_tracks_through_filter_change() {
        let config = Config::default();
        let mut view = NodesView::new(&config);

        let nodes = vec![
            make_node("c1", "idle", 0, 0, "main"),
            make_node("c2", "allocated", 0, 0, "main"),
            make_node("c3", "idle", 0, 0, "main"),
        ];

        // Initial render with no filter
        view.filter_state = String::new();
        let sorted = view.sorted_visible(&nodes);
        view.last_sorted_nodes = sorted.iter().map(|&n| n.clone()).collect();
        view.table_state.set_row_count(3);
        view.table_state.select(Some(1)); // Select "c2"

        // Change filter to idle - "c2" disappears
        view.filter_state = "idle".to_string();
        let sorted2 = view.sorted_visible(&nodes);
        assert_eq!(sorted2.len(), 2); // Only c1 and c3

        // Cursor should still have a valid position
        view.table_state.set_row_count(sorted2.len());
        assert!(view.table_state.selected().unwrap() < sorted2.len());
    }

    // ── CPU/GPU bar edge case tests ───────────────────────────────────────────

    #[test]
    fn test_cpu_bar_at_0_percent() {
        let (text, color) = cpu_bar("0", "64");
        assert!(text.contains("░░░░░░░░")); // All empty
        assert!(text.contains("  0%"));
        assert_eq!(color, Some(Color::Green)); // < 60
    }

    #[test]
    fn test_cpu_bar_at_59_percent() {
        // 59% should be green
        let (text, color) = cpu_bar("38", "64"); // 38/64 ≈ 59.375% -> rounds to 59%
        assert!(text.contains("59%"));
        assert_eq!(color, Some(Color::Green));
    }

    #[test]
    fn test_cpu_bar_at_60_percent() {
        // 60% should be yellow (>= 60, < 90)
        let (text, color) = cpu_bar("38", "63"); // 38/63 ≈ 60.317% -> rounds to 60%
        assert!(text.contains("60%"));
        assert_eq!(color, Some(Color::Yellow));
    }

    #[test]
    fn test_cpu_bar_at_89_percent() {
        // 89% should be yellow
        let (text, color) = cpu_bar("57", "64"); // 57/64 ≈ 89.0625% -> rounds to 89%
        assert!(text.contains("89%"));
        assert_eq!(color, Some(Color::Yellow));
    }

    #[test]
    fn test_cpu_bar_at_90_percent() {
        // 90% should be red (>= 90)
        let (text, color) = cpu_bar("58", "64"); // 58/64 ≈ 90.625% -> rounds to 91%
        assert!(text.contains("91%"));
        assert_eq!(color, Some(Color::Red));
    }

    #[test]
    fn test_cpu_bar_at_100_percent() {
        let (text, color) = cpu_bar("64", "64");
        assert!(text.contains("████████")); // All filled
        assert!(text.contains("100%"));
        assert_eq!(color, Some(Color::Red));
    }

    #[test]
    fn test_cpu_bar_parse_error_alloc() {
        let (text, color) = cpu_bar("invalid", "64");
        assert_eq!(text, "────────");
        assert_eq!(color, None);
    }

    #[test]
    fn test_cpu_bar_parse_error_total() {
        let (text, color) = cpu_bar("32", "invalid");
        assert_eq!(text, "────────");
        assert_eq!(color, None);
    }

    #[test]
    fn test_cpu_bar_total_zero() {
        let (text, color) = cpu_bar("0", "0");
        assert_eq!(text, "────────");
        assert_eq!(color, None);
    }

    #[test]
    fn test_gpu_bar_at_0_percent() {
        let (text, color) = gpu_bar(0, 8);
        assert!(text.contains("░░░░░░░░"));
        assert!(text.contains("  0%"));
        assert_eq!(color, Some(Color::Green));
    }

    #[test]
    fn test_gpu_bar_at_59_percent() {
        // 59% should be green
        let (text, color) = gpu_bar(47, 80); // 47/80 = 58.75% -> rounds to 59%
        assert!(text.contains("59%"));
        assert_eq!(color, Some(Color::Green));
    }

    #[test]
    fn test_gpu_bar_at_60_percent() {
        let (text, color) = gpu_bar(48, 80); // 48/80 = 60%
        assert!(text.contains("60%"));
        assert_eq!(color, Some(Color::Yellow));
    }

    #[test]
    fn test_gpu_bar_at_89_percent() {
        let (text, color) = gpu_bar(71, 80); // 71/80 = 88.75% -> rounds to 89%
        assert!(text.contains("89%"));
        assert_eq!(color, Some(Color::Yellow));
    }

    #[test]
    fn test_gpu_bar_at_90_percent() {
        let (text, color) = gpu_bar(72, 80); // 72/80 = 90%
        assert!(text.contains("90%"));
        assert_eq!(color, Some(Color::Red));
    }

    #[test]
    fn test_gpu_bar_at_100_percent() {
        let (text, color) = gpu_bar(8, 8);
        assert!(text.contains("████████"));
        assert!(text.contains("100%"));
        assert_eq!(color, Some(Color::Red));
    }

    #[test]
    fn test_gpu_bar_total_zero() {
        let (text, color) = gpu_bar(0, 0);
        assert_eq!(text, "—"); // Single em-dash
        assert_eq!(color, Some(Color::DarkGray));
    }

    #[test]
    fn test_cpu_bar_filled_cells_rounding() {
        // Test that filled cells are computed correctly with rounding
        // At 50%, with bar_width=8: filled = round(50/100 * 8) = round(4.0) = 4
        let (text, _) = cpu_bar("32", "64");
        assert!(text.contains("████░░░░")); // 4 filled, 4 empty
        assert!(text.contains("50%"));
    }

    #[test]
    fn test_gpu_bar_rounds_ties_to_even_like_python() {
        // 5/8 = 62.5%. Python's round() is banker's rounding (half to even) and
        // yields 62, so we use round_ties_even to keep the displayed value identical.
        // An 8-GPU/8-CPU node makes this tie common, not a corner case.
        let (text, _) = gpu_bar(5, 8);
        assert!(text.contains("█████░░░")); // 5 filled, 3 empty
        assert!(text.contains("62%"));
    }
}

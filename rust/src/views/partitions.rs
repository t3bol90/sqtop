//! Partitions view - sinfo summary table with per-partition availability.

use crate::app::App;
use crate::responsive::{allocate_columns, truncate_cell, ColumnSpec, Tier, CHROME_OVERHEAD};
use crate::slurm::model::ClusterSummary;
use crate::views::table_state::CapturedTableState;
use ratatui::layout::{Constraint, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::Span;
use ratatui::widgets::{Block, Borders, Row, Table};

/// Column specifications for the partitions table.
///
/// Matches COLUMNS from Python src/sqtop/views/partitions.py:
/// ```python
/// COLUMNS: list[ColumnSpec] = [
///     ColumnSpec("PARTITION",  14, 20, 100, "xs"),
///     ColumnSpec("AVAIL",       7,  8,  90, "xs"),
///     ColumnSpec("STATE",      12, 16,  85, "xs"),
///     ColumnSpec("TIMELIMIT",  12, 16,  70, "sm"),
///     ColumnSpec("NODES",       7,  8,  65, "sm"),
///     ColumnSpec("NODELIST",   30, 40,  30, "md"),
/// ]
/// ```
fn partitions_columns() -> Vec<ColumnSpec> {
    vec![
        ColumnSpec::new("PARTITION", 14, 20, 100, Tier::Xs),
        ColumnSpec::new("AVAIL", 7, 8, 90, Tier::Xs),
        ColumnSpec::new("STATE", 12, 16, 85, Tier::Xs),
        ColumnSpec::new("TIMELIMIT", 12, 16, 70, Tier::Sm),
        ColumnSpec::new("NODES", 7, 8, 65, Tier::Sm),
        ColumnSpec::new("NODELIST", 30, 40, 30, Tier::Md),
    ]
}

/// Return the appropriate color for an AVAIL value.
///
/// Matches AVAIL_COLORS from Python:
/// ```python
/// AVAIL_COLORS = {
///     "up":   "green",
///     "down": "red",
///     "inact": "dim",
///     "drain": "yellow",
/// }
/// ```
fn avail_color(avail: &str) -> Color {
    match avail.to_lowercase().as_str() {
        "up" => Color::Green,
        "down" => Color::Red,
        "inact" => Color::DarkGray,
        "drain" => Color::Yellow,
        _ => Color::White,
    }
}

/// Return the appropriate color for a STATE value.
///
/// Matches STATE_COLORS from Python:
/// ```python
/// STATE_COLORS = {
///     "idle":      "green",
///     "allocated": "cyan",
///     "mixed":     "yellow",
///     "down":      "red",
///     "drain":     "red",
///     "draining":  "magenta",
///     "unknown":   "dim",
/// }
/// ```
fn state_color(state: &str) -> Color {
    // Strip trailing asterisks and dashes like Python does
    let normalized = state
        .to_lowercase()
        .trim_end_matches('*')
        .trim_end_matches('-')
        .to_string();

    match normalized.as_str() {
        "idle" => Color::Green,
        "allocated" => Color::Cyan,
        "mixed" => Color::Yellow,
        "down" => Color::Red,
        "drain" => Color::Red,
        "draining" => Color::Magenta,
        "unknown" => Color::DarkGray,
        _ => Color::White,
    }
}

/// Get the plain (unformatted) cell value for a partition column.
fn plain_cell(partition: &ClusterSummary, col_name: &str) -> String {
    match col_name {
        "PARTITION" => partition.partition.clone(),
        "AVAIL" => partition.avail.clone(),
        "STATE" => partition.state.clone(),
        "TIMELIMIT" => partition.timelimit.clone(),
        "NODES" => partition.nodes.clone(),
        "NODELIST" => partition.nodelist.clone(),
        _ => String::new(),
    }
}

/// Format a cell for a partition column, with color and truncation.
fn format_cell<'a>(partition: &ClusterSummary, col_name: &str, width: usize) -> Span<'a> {
    let plain = plain_cell(partition, col_name);
    let text = truncate_cell(&plain, width);

    match col_name {
        "PARTITION" => Span::styled(text, Style::default().add_modifier(Modifier::BOLD)),
        "AVAIL" => Span::styled(text, Style::default().fg(avail_color(&partition.avail))),
        "STATE" => Span::styled(text, Style::default().fg(state_color(&partition.state))),
        _ => Span::raw(text),
    }
}

/// Capture the current cursor state, anchored on partition name.
///
/// Returns None if the table is empty or cursor is out of bounds.
pub fn capture_cursor_state(
    partitions: &[ClusterSummary],
    cursor_row: Option<usize>,
) -> CapturedTableState {
    let anchor = cursor_row
        .filter(|&row| row < partitions.len())
        .map(|row| partitions[row].partition.clone());
    CapturedTableState::new(anchor, 0.0)
}

/// Restore cursor position to the row matching the anchor partition name.
///
/// If the anchor is not found, returns the saved row clamped to the table size.
/// If the table is empty, returns None.
pub fn restore_cursor_position(
    state: &CapturedTableState,
    partitions: &[ClusterSummary],
    saved_row: usize,
) -> Option<usize> {
    if partitions.is_empty() {
        return None;
    }

    // Try to find the anchor
    if let Some(row) = state.restore(partitions.len(), |i| {
        partitions.get(i).map(|p| p.partition.clone())
    }) {
        return Some(row);
    }

    // Fall back to clamped saved row
    Some(saved_row.min(partitions.len() - 1))
}

/// Render the partitions table.
pub fn render(f: &mut ratatui::Frame, app: &App, area: Rect) {
    // Check for too-small area (would panic in ratatui's constraint solver)
    if area.width < 10 || area.height < 3 {
        let block = Block::default()
            .borders(Borders::ALL)
            .title("Partitions (too small)");
        f.render_widget(block, area);
        return;
    }

    // Calculate available width for table content
    let budget = area.width.saturating_sub(CHROME_OVERHEAD);
    let terminal_width = area.width;

    // Allocate columns based on budget
    let allocated = allocate_columns(
        budget,
        &partitions_columns(),
        crate::responsive::tier_for(terminal_width),
    );

    if allocated.is_empty() {
        let block = Block::default()
            .borders(Borders::ALL)
            .title("Partitions (no space)");
        f.render_widget(block, area);
        return;
    }

    // Build header
    let header_cells: Vec<_> = allocated
        .iter()
        .map(|(name, _)| {
            Span::styled(
                name.clone(),
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD),
            )
        })
        .collect();
    let header = Row::new(header_cells).height(1).bottom_margin(0);

    // Build rows
    let rows: Vec<Row> = app
        .partitions
        .iter()
        .map(|partition| {
            let cells: Vec<Span> = allocated
                .iter()
                .map(|(name, width)| format_cell(partition, name, *width as usize))
                .collect();
            Row::new(cells)
        })
        .collect();

    // Build column widths
    let widths: Vec<Constraint> = allocated
        .iter()
        .map(|(_, w)| Constraint::Length(*w))
        .collect();

    // Count partitions by availability
    let up_count = app
        .partitions
        .iter()
        .filter(|p| p.avail.to_lowercase() == "up")
        .count();

    // Build title based on tier (xs = compact, others = detailed)
    let tier = crate::responsive::tier_for(terminal_width);
    let title = if tier == Tier::Xs {
        format!("sinfo  {} partitions", app.partitions.len())
    } else {
        format!(
            "sinfo  {} up  {} partitions",
            up_count,
            app.partitions.len()
        )
    };

    // Create table widget
    let table = Table::new(rows, widths)
        .header(header)
        .block(Block::default().borders(Borders::ALL).title(title))
        .row_highlight_style(Style::default().add_modifier(Modifier::REVERSED));

    // Render without state for now (cursor management will be added by integration)
    f.render_widget(table, area);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;
    use ratatui::backend::TestBackend;
    use ratatui::Terminal;

    fn make_partition(name: &str, avail: &str, state: &str) -> ClusterSummary {
        ClusterSummary {
            partition: name.to_string(),
            avail: avail.to_string(),
            state: state.to_string(),
            timelimit: "UNLIMITED".to_string(),
            nodes: "4".to_string(),
            nodelist: "node[01-04]".to_string(),
        }
    }

    // Column allocation tests at different terminal widths
    #[test]
    fn test_partitions_columns_xs_width() {
        // At xs tier (40-79), should show only xs-priority columns
        let budget = 40 - CHROME_OVERHEAD;
        let allocated = allocate_columns(budget, &partitions_columns(), Tier::Xs);

        // Should have at least PARTITION, AVAIL, STATE (all xs tier)
        assert!(!allocated.is_empty());
        let names: Vec<&str> = allocated.iter().map(|(n, _)| n.as_str()).collect();
        assert!(names.contains(&"PARTITION"));
    }

    #[test]
    fn test_partitions_columns_sm_width() {
        // At sm tier (80-109), should show xs + sm columns
        let budget = 80 - CHROME_OVERHEAD;
        let allocated = allocate_columns(budget, &partitions_columns(), Tier::Sm);

        let names: Vec<&str> = allocated.iter().map(|(n, _)| n.as_str()).collect();
        assert!(names.contains(&"PARTITION"));
        assert!(names.contains(&"AVAIL"));
        assert!(names.contains(&"STATE"));
    }

    #[test]
    fn test_partitions_columns_md_width() {
        // At md tier (110-159), should show xs + sm + md columns
        let budget = 110 - CHROME_OVERHEAD;
        let allocated = allocate_columns(budget, &partitions_columns(), Tier::Md);

        let names: Vec<&str> = allocated.iter().map(|(n, _)| n.as_str()).collect();
        assert!(names.contains(&"PARTITION"));
        assert!(names.contains(&"NODELIST")); // md-tier column
    }

    #[test]
    fn test_partitions_columns_lg_width() {
        // At lg tier (160+), should show all columns
        let budget = 160 - CHROME_OVERHEAD;
        let allocated = allocate_columns(budget, &partitions_columns(), Tier::Lg);

        assert!(!allocated.is_empty());
        let names: Vec<&str> = allocated.iter().map(|(n, _)| n.as_str()).collect();
        assert!(names.contains(&"PARTITION"));
        assert!(names.contains(&"NODELIST"));
    }

    // Color tests
    #[test]
    fn test_avail_color_up() {
        assert_eq!(avail_color("up"), Color::Green);
        assert_eq!(avail_color("UP"), Color::Green);
    }

    #[test]
    fn test_avail_color_down() {
        assert_eq!(avail_color("down"), Color::Red);
    }

    #[test]
    fn test_avail_color_drain() {
        assert_eq!(avail_color("drain"), Color::Yellow);
    }

    #[test]
    fn test_avail_color_inact() {
        assert_eq!(avail_color("inact"), Color::DarkGray);
    }

    #[test]
    fn test_state_color_idle() {
        assert_eq!(state_color("idle"), Color::Green);
    }

    #[test]
    fn test_state_color_allocated() {
        assert_eq!(state_color("allocated"), Color::Cyan);
    }

    #[test]
    fn test_state_color_mixed() {
        assert_eq!(state_color("mixed"), Color::Yellow);
    }

    #[test]
    fn test_state_color_drain() {
        assert_eq!(state_color("drain"), Color::Red);
    }

    #[test]
    fn test_state_color_draining() {
        assert_eq!(state_color("draining"), Color::Magenta);
    }

    #[test]
    fn test_state_color_strips_asterisk() {
        assert_eq!(state_color("idle*"), Color::Green);
        assert_eq!(state_color("mixed*"), Color::Yellow);
    }

    #[test]
    fn test_state_color_strips_dash() {
        assert_eq!(state_color("idle-"), Color::Green);
    }

    // Cursor anchor tests
    #[test]
    fn test_capture_cursor_state_valid() {
        let partitions = vec![
            make_partition("gpu", "up", "idle"),
            make_partition("cpu", "up", "mixed"),
            make_partition("debug", "up", "idle"),
        ];

        let state = capture_cursor_state(&partitions, Some(1));
        assert_eq!(state.anchor, Some("cpu".to_string()));
    }

    #[test]
    fn test_capture_cursor_state_out_of_bounds() {
        let partitions = vec![make_partition("gpu", "up", "idle")];

        let state = capture_cursor_state(&partitions, Some(5));
        assert_eq!(state.anchor, None);
    }

    #[test]
    fn test_capture_cursor_state_empty() {
        let partitions: Vec<ClusterSummary> = vec![];

        let state = capture_cursor_state(&partitions, Some(0));
        assert_eq!(state.anchor, None);
    }

    #[test]
    fn test_restore_cursor_position_anchor_found() {
        let partitions = vec![
            make_partition("gpu", "up", "idle"),
            make_partition("cpu", "up", "mixed"),
            make_partition("debug", "up", "idle"),
        ];

        let state = CapturedTableState::new(Some("debug".to_string()), 0.0);
        let restored = restore_cursor_position(&state, &partitions, 0);

        assert_eq!(restored, Some(2)); // "debug" is at index 2
    }

    #[test]
    fn test_restore_cursor_position_anchor_not_found_fallback() {
        let partitions = vec![
            make_partition("gpu", "up", "idle"),
            make_partition("cpu", "up", "mixed"),
        ];

        let state = CapturedTableState::new(Some("missing".to_string()), 0.0);
        let restored = restore_cursor_position(&state, &partitions, 1);

        // Should fall back to saved row (1), which is valid
        assert_eq!(restored, Some(1));
    }

    #[test]
    fn test_restore_cursor_position_saved_row_clamped() {
        let partitions = vec![make_partition("gpu", "up", "idle")];

        let state = CapturedTableState::new(Some("missing".to_string()), 0.0);
        let restored = restore_cursor_position(&state, &partitions, 10);

        // Should clamp saved row to max valid index (0)
        assert_eq!(restored, Some(0));
    }

    #[test]
    fn test_restore_cursor_position_empty_partitions() {
        let partitions: Vec<ClusterSummary> = vec![];

        let state = CapturedTableState::new(Some("gpu".to_string()), 0.0);
        let restored = restore_cursor_position(&state, &partitions, 0);

        assert_eq!(restored, None);
    }

    #[test]
    fn test_cursor_tracks_partition_across_reorder() {
        // Initial order
        let mut partitions = vec![
            make_partition("gpu", "up", "idle"),
            make_partition("cpu", "up", "mixed"),
            make_partition("debug", "up", "idle"),
        ];

        // Cursor on "cpu" (index 1)
        let state = capture_cursor_state(&partitions, Some(1));
        assert_eq!(state.anchor, Some("cpu".to_string()));

        // Reorder partitions (e.g., sort by name)
        partitions.sort_by(|a, b| a.partition.cmp(&b.partition));
        // New order: ["cpu", "debug", "gpu"]

        // Restore cursor - should find "cpu" at new index 0
        let restored = restore_cursor_position(&state, &partitions, 1);
        assert_eq!(restored, Some(0));
        assert_eq!(partitions[restored.unwrap()].partition, "cpu");
    }

    // Render tests
    #[test]
    fn test_render_empty_partitions() {
        let config = Config::default();
        let app = App::new(config);

        let backend = TestBackend::new(120, 30);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|f| {
                let area = f.area();
                render(f, &app, area);
            })
            .unwrap();

        // Should not panic with empty partitions
    }

    #[test]
    fn test_render_too_small_area() {
        let config = Config::default();
        let app = App::new(config);

        let backend = TestBackend::new(8, 2);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|f| {
                let area = f.area();
                render(f, &app, area);
            })
            .unwrap();

        // Should render "too small" message without panicking
    }

    #[test]
    fn test_render_xs_tier_compact_title() {
        let config = Config::default();
        let mut app = App::new(config);
        app.partitions = vec![
            make_partition("gpu", "up", "idle"),
            make_partition("cpu", "down", "mixed"),
        ];

        let backend = TestBackend::new(60, 30); // xs tier
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|f| {
                let area = f.area();
                render(f, &app, area);
            })
            .unwrap();

        // Should use compact title at xs tier
    }

    #[test]
    fn test_render_md_tier_detailed_title() {
        let config = Config::default();
        let mut app = App::new(config);
        app.partitions = vec![
            make_partition("gpu", "up", "idle"),
            make_partition("cpu", "up", "mixed"),
            make_partition("debug", "down", "idle"),
        ];

        let backend = TestBackend::new(120, 30); // md tier
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|f| {
                let area = f.area();
                render(f, &app, area);
            })
            .unwrap();

        // Should use detailed title with up count at md tier
    }

    #[test]
    fn test_plain_cell_all_columns() {
        let p = make_partition("gpu", "up", "idle");

        assert_eq!(plain_cell(&p, "PARTITION"), "gpu");
        assert_eq!(plain_cell(&p, "AVAIL"), "up");
        assert_eq!(plain_cell(&p, "STATE"), "idle");
        assert_eq!(plain_cell(&p, "TIMELIMIT"), "UNLIMITED");
        assert_eq!(plain_cell(&p, "NODES"), "4");
        assert_eq!(plain_cell(&p, "NODELIST"), "node[01-04]");
        assert_eq!(plain_cell(&p, "UNKNOWN"), "");
    }
}

//! Visual selection mode for data-table views.
//!
//! Ports Python `src/sqtop/views/mixins.py::VisualSelectMixin`.
//!
//! Provides Vim-like visual row-selection mode:
//! - `v` / `V` enters visual mode, anchoring at the current cursor row
//! - cursor movement extends the selection range
//! - `y` yanks the selection to clipboard
//! - `escape` exits without copying
//!
//! The selection is independent of any persistent multi-select state
//! (e.g., JobsView's selected_job_ids).

use std::collections::BTreeSet;

/// Visual selection state for a data-table view.
///
/// Tracks whether visual mode is active and the anchor/cursor row indices.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VisualSelection {
    active: bool,
    anchor: Option<usize>,
    cursor: Option<usize>,
}

impl Default for VisualSelection {
    fn default() -> Self {
        Self::new()
    }
}

impl VisualSelection {
    /// Create a new inactive visual selection.
    pub fn new() -> Self {
        Self {
            active: false,
            anchor: None,
            cursor: None,
        }
    }

    /// Enter visual mode anchored at the given cursor row.
    ///
    /// Sets both anchor and cursor to the same row.
    pub fn enter(&mut self, cursor_row: usize) {
        self.active = true;
        self.anchor = Some(cursor_row);
        self.cursor = Some(cursor_row);
    }

    /// Exit visual mode, clearing all state.
    pub fn exit(&mut self) {
        self.active = false;
        self.anchor = None;
        self.cursor = None;
    }

    /// Update the cursor position while visual mode is active.
    ///
    /// This extends the selection range. Has no effect if visual mode is inactive.
    pub fn set_cursor(&mut self, row: usize) {
        if self.active {
            self.cursor = Some(row);
        }
    }

    /// Move the visual cursor by a signed delta (relative movement).
    ///
    /// When cursor is None, uses `table_cursor_row` as the starting position.
    /// Clamps the result to [0, row_count - 1].
    /// No-op when row_count == 0 or visual mode is inactive.
    ///
    /// Matches Python `_move_visual_cursor(delta=...)`.
    pub fn move_cursor(&mut self, delta: i64, row_count: usize, table_cursor_row: usize) {
        if !self.active || row_count == 0 {
            return;
        }

        let current = self.cursor.unwrap_or(table_cursor_row);
        // Do arithmetic in i64 to handle negative results
        let new_pos = (current as i64).saturating_add(delta);
        // Clamp to valid range
        let clamped = new_pos.max(0).min((row_count - 1) as i64) as usize;
        self.cursor = Some(clamped);
    }

    /// Move the visual cursor to an absolute row position.
    ///
    /// Clamps the result to [0, row_count - 1].
    /// No-op when row_count == 0 or visual mode is inactive.
    ///
    /// Matches Python `_move_visual_cursor(absolute=...)`.
    pub fn move_cursor_to(&mut self, absolute: usize, row_count: usize) {
        if !self.active || row_count == 0 {
            return;
        }

        let clamped = absolute.min(row_count - 1);
        self.cursor = Some(clamped);
    }

    /// Return the inclusive (min, max) range of selected rows, or None if inactive.
    ///
    /// When cursor is None, falls back to anchor (single-row selection).
    pub fn range(&self) -> Option<(usize, usize)> {
        if !self.active {
            return None;
        }
        let anchor = self.anchor?;
        let cursor = self.cursor.unwrap_or(anchor);
        Some((anchor.min(cursor), anchor.max(cursor)))
    }

    /// Return the set of selected row indices.
    ///
    /// Empty when inactive or anchor is None.
    pub fn rows(&self) -> BTreeSet<usize> {
        match self.range() {
            Some((min, max)) => (min..=max).collect(),
            None => BTreeSet::new(),
        }
    }

    /// Whether visual mode is currently active.
    pub fn is_active(&self) -> bool {
        self.active
    }

    /// Get the current anchor row, if any.
    pub fn anchor(&self) -> Option<usize> {
        self.anchor
    }

    /// Get the current cursor row, if any.
    pub fn cursor(&self) -> Option<usize> {
        self.cursor
    }
}

/// Assemble yank payload (TSV text) from selected rows.
///
/// Takes:
/// - `selected_rows`: the row indices to include (from `VisualSelection::rows()`)
/// - `items`: the full list of items
/// - `row_tsv`: a closure that converts one item to a TSV line (no trailing newline)
///
/// Returns the TSV text with a trailing newline.
///
/// Matches Python `_visual_yank_payload(start, end)`.
pub fn yank_tsv<T, F>(selected_rows: &BTreeSet<usize>, items: &[T], row_tsv: F) -> String
where
    F: Fn(&T) -> String,
{
    let mut lines = Vec::new();
    for &idx in selected_rows {
        if let Some(item) = items.get(idx) {
            lines.push(row_tsv(item));
        }
    }
    if lines.is_empty() {
        return "\n".to_string();
    }
    lines.join("\n") + "\n"
}

// ── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Debug, Clone, PartialEq, Eq)]
    struct FakeJob {
        job_id: String,
        name: String,
        state: String,
    }

    fn make_jobs(n: usize) -> Vec<FakeJob> {
        (0..n)
            .map(|i| FakeJob {
                job_id: (i + 1).to_string(),
                name: format!("job{}", i + 1),
                state: "RUNNING".to_string(),
            })
            .collect()
    }

    fn job_to_tsv(job: &FakeJob) -> String {
        format!("{}\t{}\t{}", job.job_id, job.name, job.state)
    }

    #[test]
    fn test_initial_state_is_inactive() {
        let vs = VisualSelection::new();
        assert!(!vs.is_active());
        assert_eq!(vs.anchor(), None);
        assert_eq!(vs.cursor(), None);
    }

    #[test]
    fn test_enter_sets_anchor_and_cursor() {
        let mut vs = VisualSelection::new();
        vs.enter(2);
        assert!(vs.is_active());
        assert_eq!(vs.anchor(), Some(2));
        assert_eq!(vs.cursor(), Some(2));
    }

    #[test]
    fn test_exit_clears_state() {
        let mut vs = VisualSelection::new();
        vs.enter(3);
        vs.exit();
        assert!(!vs.is_active());
        assert_eq!(vs.anchor(), None);
        assert_eq!(vs.cursor(), None);
    }

    #[test]
    fn test_exit_when_inactive_is_noop() {
        let mut vs = VisualSelection::new();
        vs.exit(); // should not panic
        assert!(!vs.is_active());
    }

    #[test]
    fn test_range_single_row() {
        let mut vs = VisualSelection::new();
        vs.enter(4);
        assert_eq!(vs.range(), Some((4, 4)));
        assert_eq!(vs.rows(), [4].iter().copied().collect());
    }

    #[test]
    fn test_range_extend_down() {
        let mut vs = VisualSelection::new();
        vs.enter(2);
        vs.set_cursor(5);
        assert_eq!(vs.range(), Some((2, 5)));
        assert_eq!(vs.rows(), [2, 3, 4, 5].iter().copied().collect());
    }

    #[test]
    fn test_range_extend_up() {
        let mut vs = VisualSelection::new();
        vs.enter(5);
        vs.set_cursor(3);
        assert_eq!(vs.range(), Some((3, 5)));
        assert_eq!(vs.rows(), [3, 4, 5].iter().copied().collect());
    }

    #[test]
    fn test_range_when_inactive_is_none() {
        let vs = VisualSelection::new();
        assert_eq!(vs.range(), None);
    }

    #[test]
    fn test_rows_empty_when_inactive() {
        let vs = VisualSelection::new();
        assert_eq!(vs.rows(), BTreeSet::new());
    }

    #[test]
    fn test_set_cursor_has_no_effect_when_inactive() {
        let mut vs = VisualSelection::new();
        vs.set_cursor(10);
        assert_eq!(vs.cursor(), None);
    }

    #[test]
    fn test_range_with_cursor_none_falls_back_to_anchor() {
        let mut vs = VisualSelection::new();
        vs.active = true;
        vs.anchor = Some(7);
        vs.cursor = None;
        assert_eq!(vs.range(), Some((7, 7)));
    }

    #[test]
    fn test_yank_tsv_single_row() {
        let jobs = make_jobs(10);
        let mut vs = VisualSelection::new();
        vs.enter(0);
        let text = yank_tsv(&vs.rows(), &jobs, job_to_tsv);
        assert_eq!(text, "1\tjob1\tRUNNING\n");
    }

    #[test]
    fn test_yank_tsv_multi_row() {
        let jobs = make_jobs(10);
        let mut vs = VisualSelection::new();
        vs.enter(2);
        vs.set_cursor(5);
        let text = yank_tsv(&vs.rows(), &jobs, job_to_tsv);
        let lines: Vec<_> = text.trim_end_matches('\n').split('\n').collect();
        assert_eq!(lines.len(), 4);
        assert_eq!(lines[0], "3\tjob3\tRUNNING");
        assert_eq!(lines[1], "4\tjob4\tRUNNING");
        assert_eq!(lines[2], "5\tjob5\tRUNNING");
        assert_eq!(lines[3], "6\tjob6\tRUNNING");
    }

    #[test]
    fn test_yank_tsv_empty_selection() {
        let jobs = make_jobs(10);
        let vs = VisualSelection::new(); // inactive
        let text = yank_tsv(&vs.rows(), &jobs, job_to_tsv);
        assert_eq!(text, "\n");
    }

    #[test]
    fn test_yank_tsv_no_header() {
        let jobs = make_jobs(5);
        let mut vs = VisualSelection::new();
        vs.enter(0);
        vs.set_cursor(4);
        let text = yank_tsv(&vs.rows(), &jobs, job_to_tsv);
        let lines: Vec<_> = text.trim_end_matches('\n').split('\n').collect();
        // First field should be numeric job_id, not "JOBID"
        for line in lines {
            let parts: Vec<_> = line.split('\t').collect();
            assert!(
                parts[0].chars().all(|c| c.is_ascii_digit()),
                "Expected numeric job_id, got {:?}",
                parts[0]
            );
        }
    }

    #[test]
    fn test_yank_tsv_trailing_newline() {
        let jobs = make_jobs(3);
        let mut vs = VisualSelection::new();
        vs.enter(0);
        vs.set_cursor(2);
        let text = yank_tsv(&vs.rows(), &jobs, job_to_tsv);
        assert!(text.ends_with('\n'));
    }

    #[test]
    fn test_yank_tsv_correct_row_count() {
        let jobs = make_jobs(8);
        let mut vs = VisualSelection::new();
        vs.enter(2);
        vs.set_cursor(5);
        let text = yank_tsv(&vs.rows(), &jobs, job_to_tsv);
        let lines: Vec<_> = text.trim_end_matches('\n').split('\n').collect();
        assert_eq!(lines.len(), 4); // rows 2,3,4,5
    }

    #[test]
    fn test_visual_top_bottom() {
        let mut vs = VisualSelection::new();
        vs.enter(5);
        vs.set_cursor(0); // top
        assert_eq!(vs.range(), Some((0, 5)));
        vs.set_cursor(9); // bottom
        assert_eq!(vs.range(), Some((5, 9)));
    }

    #[test]
    fn test_bidirectional_extension() {
        let mut vs = VisualSelection::new();
        vs.enter(5);
        // Extend down
        vs.set_cursor(7);
        assert_eq!(vs.range(), Some((5, 7)));
        // Then extend up past anchor
        vs.set_cursor(3);
        assert_eq!(vs.range(), Some((3, 5)));
        // Then back down again
        vs.set_cursor(6);
        assert_eq!(vs.range(), Some((5, 6)));
    }

    #[test]
    fn test_move_cursor_up_from_row_0_clamps() {
        let mut vs = VisualSelection::new();
        vs.enter(0);
        vs.move_cursor(-1, 10, 0);
        assert_eq!(vs.cursor(), Some(0)); // not usize::MAX
        assert_eq!(vs.range(), Some((0, 0)));
    }

    #[test]
    fn test_move_cursor_down_from_last_row_clamps() {
        let mut vs = VisualSelection::new();
        vs.enter(9);
        vs.move_cursor(1, 10, 9);
        assert_eq!(vs.cursor(), Some(9));
        assert_eq!(vs.range(), Some((9, 9)));
    }

    #[test]
    fn test_move_cursor_with_empty_table_is_noop() {
        let mut vs = VisualSelection::new();
        vs.enter(0);
        vs.move_cursor(1, 0, 0); // row_count = 0
                                 // Should not panic and should not change state
        assert_eq!(vs.cursor(), Some(0));
    }

    #[test]
    fn test_move_cursor_to_with_empty_table_is_noop() {
        let mut vs = VisualSelection::new();
        vs.enter(5);
        vs.move_cursor_to(10, 0); // row_count = 0
        assert_eq!(vs.cursor(), Some(5));
    }

    #[test]
    fn test_move_cursor_to_creates_full_range() {
        let mut vs = VisualSelection::new();
        vs.enter(5);
        vs.move_cursor_to(0, 10);
        assert_eq!(vs.range(), Some((0, 5)));
        vs.move_cursor_to(9, 10);
        assert_eq!(vs.range(), Some((5, 9)));
    }

    #[test]
    fn test_move_cursor_uses_table_cursor_when_cursor_is_none() {
        let mut vs = VisualSelection::new();
        vs.active = true;
        vs.anchor = Some(5);
        vs.cursor = None;
        vs.move_cursor(2, 10, 3); // table_cursor_row = 3
        assert_eq!(vs.cursor(), Some(5)); // 3 + 2 = 5
    }

    #[test]
    fn test_move_cursor_while_inactive_does_nothing() {
        let mut vs = VisualSelection::new();
        vs.move_cursor(5, 10, 0);
        assert_eq!(vs.cursor(), None);
    }

    #[test]
    fn test_move_cursor_to_while_inactive_does_nothing() {
        let mut vs = VisualSelection::new();
        vs.move_cursor_to(7, 10);
        assert_eq!(vs.cursor(), None);
    }

    #[test]
    fn test_move_cursor_large_negative_delta_clamps() {
        let mut vs = VisualSelection::new();
        vs.enter(5);
        vs.move_cursor(-100, 10, 5);
        assert_eq!(vs.cursor(), Some(0));
    }

    #[test]
    fn test_move_cursor_large_positive_delta_clamps() {
        let mut vs = VisualSelection::new();
        vs.enter(5);
        vs.move_cursor(100, 10, 5);
        assert_eq!(vs.cursor(), Some(9));
    }
}

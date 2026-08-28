//! Cyclic table cursor behavior and anchor-based state capture/restore.
//!
//! This module provides helpers for managing table cursor state across refreshes,
//! including cyclic cursor wrapping (next wraps from last to first, prev from first
//! to last) and anchor-based state capture/restore so the cursor tracks the same
//! item across refresh and re-sort operations.

/// Cyclic table cursor state.
///
/// Wraps ratatui's TableState to provide:
/// - Next/prev that wrap at boundaries (last->first, first->last)
/// - Empty-table safety (never panics)
/// - Anchor-based capture/restore for tracking items across refresh
#[derive(Debug, Default, Clone)]
pub struct CyclicTableState {
    /// Currently selected row index (0-based), or None if no selection.
    selected: Option<usize>,
    /// Number of rows in the table.
    row_count: usize,
}

impl CyclicTableState {
    /// Create a new empty table state.
    pub fn new() -> Self {
        Self::default()
    }

    /// Create a table state with the given row count.
    #[cfg(test)]
    pub fn with_row_count(row_count: usize) -> Self {
        Self {
            selected: if row_count > 0 { Some(0) } else { None },
            row_count,
        }
    }

    /// Get the currently selected row index, or None if no selection.
    pub fn selected(&self) -> Option<usize> {
        self.selected
    }

    /// Set the selected row index.
    pub fn select(&mut self, index: Option<usize>) {
        self.selected = index;
    }

    /// Set the row count and adjust selection if needed.
    pub fn set_row_count(&mut self, count: usize) {
        self.row_count = count;
        if count == 0 {
            self.selected = None;
        } else if let Some(sel) = self.selected {
            if sel >= count {
                self.selected = Some(count - 1);
            }
        } else if count > 0 {
            self.selected = Some(0);
        }
    }

    /// Move cursor down, wrapping from last row to first.
    pub fn next(&mut self) {
        if self.row_count == 0 {
            return;
        }
        self.selected = Some(match self.selected {
            Some(i) if i >= self.row_count - 1 => 0,
            Some(i) => i + 1,
            None => 0,
        });
    }

    /// Move cursor up, wrapping from first row to last.
    pub fn prev(&mut self) {
        if self.row_count == 0 {
            return;
        }
        self.selected = Some(match self.selected {
            Some(0) | None => self.row_count - 1,
            Some(i) => i - 1,
        });
    }
}

/// Captured table state for restoration after refresh/re-sort.
///
/// Uses an anchor (job_id or node name) instead of row index so the cursor
/// tracks the same item even when the table is re-sorted or filtered.
#[derive(Debug, Clone)]
pub struct CapturedTableState {
    /// The anchor value (job_id or node name) of the selected row, if any.
    pub anchor: Option<String>,
}

impl CapturedTableState {
    /// Create a new captured state.
    pub fn new(anchor: Option<String>) -> Self {
        Self { anchor }
    }

    /// Restore cursor position by finding the row with the matching anchor.
    ///
    /// Returns the row index if found, or None otherwise.
    pub fn restore<F>(&self, row_count: usize, get_anchor: F) -> Option<usize>
    where
        F: Fn(usize) -> Option<String>,
    {
        let anchor = self.anchor.as_ref()?;
        (0..row_count).find(|&i| get_anchor(i).as_ref() == Some(anchor))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Cyclic cursor wrapping tests
    #[test]
    fn test_next_wraps_from_last_to_first() {
        let mut state = CyclicTableState::with_row_count(3);
        state.select(Some(2));
        state.next();
        assert_eq!(state.selected(), Some(0));
    }

    #[test]
    fn test_prev_wraps_from_first_to_last() {
        let mut state = CyclicTableState::with_row_count(3);
        state.select(Some(0));
        state.prev();
        assert_eq!(state.selected(), Some(2));
    }

    #[test]
    fn test_next_on_empty_table_is_noop() {
        let mut state = CyclicTableState::with_row_count(0);
        state.next();
        assert_eq!(state.selected(), None);
    }

    #[test]
    fn test_prev_on_empty_table_is_noop() {
        let mut state = CyclicTableState::with_row_count(0);
        state.prev();
        assert_eq!(state.selected(), None);
    }

    #[test]
    fn test_next_from_middle() {
        let mut state = CyclicTableState::with_row_count(5);
        state.select(Some(2));
        state.next();
        assert_eq!(state.selected(), Some(3));
    }

    #[test]
    fn test_prev_from_middle() {
        let mut state = CyclicTableState::with_row_count(5);
        state.select(Some(2));
        state.prev();
        assert_eq!(state.selected(), Some(1));
    }

    #[test]
    fn test_set_row_count_adjusts_selection() {
        let mut state = CyclicTableState::with_row_count(10);
        state.select(Some(8));
        state.set_row_count(5);
        assert_eq!(state.selected(), Some(4));
    }

    #[test]
    fn test_set_row_count_to_zero_clears_selection() {
        let mut state = CyclicTableState::with_row_count(10);
        state.select(Some(5));
        state.set_row_count(0);
        assert_eq!(state.selected(), None);
    }

    // Anchor-based capture/restore tests
    #[test]
    fn test_restore_finds_matching_anchor() {
        let state = CapturedTableState::new(Some("job123".to_string()));
        let anchors = ["job100", "job123", "job200"];
        let get_anchor = |i: usize| anchors.get(i).map(|s| s.to_string());
        assert_eq!(state.restore(3, get_anchor), Some(1));
    }

    #[test]
    fn test_restore_returns_none_when_anchor_not_found() {
        let state = CapturedTableState::new(Some("job999".to_string()));
        let anchors = ["job100", "job123", "job200"];
        let get_anchor = |i: usize| anchors.get(i).map(|s| s.to_string());
        assert_eq!(state.restore(3, get_anchor), None);
    }

    #[test]
    fn test_restore_returns_none_when_no_anchor() {
        let state = CapturedTableState::new(None);
        let anchors = ["job100", "job123", "job200"];
        let get_anchor = |i: usize| anchors.get(i).map(|s| s.to_string());
        assert_eq!(state.restore(3, get_anchor), None);
    }

    #[test]
    fn test_restore_first_match_when_duplicates() {
        let state = CapturedTableState::new(Some("job123".to_string()));
        let anchors = ["job123", "job123", "job200"];
        let get_anchor = |i: usize| anchors.get(i).map(|s| s.to_string());
        assert_eq!(state.restore(3, get_anchor), Some(0));
    }
}

//! Column visibility toggle modal — per-view column show/hide.

use super::{centered_rect, ModalOutcome};
use crate::config::Config;
use crossterm::event::{KeyCode, KeyEvent};
use ratatui::layout::{Constraint, Direction, Layout};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph};
use ratatui::Frame;
use std::collections::HashSet;

/// Result of column toggle modal.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ColumnToggleResult {
    /// User closed modal without reset.
    None,
    /// User clicked "Reset to default order".
    Reset,
}

/// State for the column toggle modal.
#[derive(Debug, Clone)]
pub struct ColumnToggleState {
    view_name: String,
    hidden: HashSet<String>,
    display_order: Vec<String>,
    focused: usize,
    // Number of column checkboxes before buttons
    checkbox_count: usize,
    // Flag indicating config was modified (for persistence)
    pub config_modified: bool,
}

impl ColumnToggleState {
    /// Create a new column toggle modal.
    pub fn new(
        view_name: String,
        all_columns: Vec<String>,
        hidden_columns: Vec<String>,
        column_order: Option<Vec<String>>,
    ) -> Self {
        let hidden: HashSet<String> = hidden_columns.into_iter().collect();

        // If column_order provided, use it; otherwise use all_columns order
        let display_order = if let Some(order) = column_order {
            let col_set: HashSet<_> = all_columns.iter().cloned().collect();
            let mut ordered: Vec<_> = order.into_iter().filter(|c| col_set.contains(c)).collect();
            let ordered_set: HashSet<_> = ordered.iter().cloned().collect();
            let remaining: Vec<_> = all_columns
                .into_iter()
                .filter(|c| !ordered_set.contains(c))
                .collect();
            ordered.extend(remaining);
            ordered
        } else {
            all_columns
        };

        let checkbox_count = display_order.len();

        Self {
            view_name,
            hidden,
            display_order,
            focused: 0,
            checkbox_count,
            config_modified: false,
        }
    }

    /// Handle a key event.
    pub fn handle_key(
        &mut self,
        key: KeyEvent,
        config: &mut Config,
    ) -> ModalOutcome<ColumnToggleResult> {
        match key.code {
            KeyCode::Esc | KeyCode::Char('C') => ModalOutcome::Dismiss(ColumnToggleResult::None),
            KeyCode::Down | KeyCode::Tab => {
                self.focus_next();
                ModalOutcome::Continue
            }
            KeyCode::Up | KeyCode::BackTab => {
                self.focus_prev();
                ModalOutcome::Continue
            }
            KeyCode::Char(' ') | KeyCode::Enter => {
                if self.focused < self.checkbox_count {
                    // Toggle checkbox
                    let col = &self.display_order[self.focused];
                    if self.hidden.contains(col) {
                        self.hidden.remove(col);
                    } else {
                        self.hidden.insert(col.clone());
                    }
                    // Update config
                    self.update_config(config);
                    self.config_modified = true;
                    ModalOutcome::Continue
                } else if self.focused == self.checkbox_count {
                    // Reset button
                    ModalOutcome::Dismiss(ColumnToggleResult::Reset)
                } else {
                    // Close button
                    ModalOutcome::Dismiss(ColumnToggleResult::None)
                }
            }
            _ => ModalOutcome::Continue,
        }
    }

    fn focus_next(&mut self) {
        let total = self.checkbox_count + 2; // checkboxes + reset + close
        self.focused = (self.focused + 1) % total;
    }

    fn focus_prev(&mut self) {
        let total = self.checkbox_count + 2;
        self.focused = if self.focused == 0 {
            total - 1
        } else {
            self.focused - 1
        };
    }

    fn update_config(&self, config: &mut Config) {
        let hidden_vec: Vec<String> = self.hidden.iter().cloned().collect();

        match self.view_name.as_str() {
            "Jobs" => config.columns.jobs_hidden = hidden_vec,
            "Nodes" => config.columns.nodes_hidden = hidden_vec,
            "Partitions" => config.columns.partitions_hidden = hidden_vec,
            _ => {}
        }
    }

    /// Render the modal.
    pub fn render(&self, f: &mut Frame, area: ratatui::layout::Rect) {
        let height = 3 + self.checkbox_count as u16 + 2; // title + checkboxes + buttons
        let modal_area = centered_rect(area, 42, height.min(area.height.saturating_sub(2)));

        f.render_widget(Clear, modal_area);

        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::Cyan))
            .title(" Column Visibility ");

        let inner = block.inner(modal_area);
        f.render_widget(block, modal_area);

        // Layout
        let mut constraints = vec![Constraint::Length(1)]; // title
        for _ in 0..self.checkbox_count {
            constraints.push(Constraint::Length(1));
        }
        constraints.push(Constraint::Length(1)); // reset button
        constraints.push(Constraint::Length(1)); // close button

        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints(constraints)
            .split(inner);

        // Title
        let title_text = format!("Column visibility — {}", self.view_name);
        f.render_widget(Paragraph::new(title_text), chunks[0]);

        // Checkboxes
        for (i, col) in self.display_order.iter().enumerate() {
            let is_checked = !self.hidden.contains(col);
            let is_focused = i == self.focused;

            let checkbox = if is_checked { "[✓]" } else { "[ ]" };

            let style = if is_focused {
                Style::default()
                    .fg(Color::Black)
                    .bg(Color::White)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default()
            };

            let line = Line::from(vec![
                Span::raw(" "),
                Span::styled(checkbox, style),
                Span::raw(" "),
                Span::styled(col, style),
            ]);

            f.render_widget(Paragraph::new(line), chunks[1 + i]);
        }

        // Reset button
        let reset_focused = self.focused == self.checkbox_count;
        let reset_style = if reset_focused {
            Style::default()
                .fg(Color::Black)
                .bg(Color::White)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default()
        };
        let reset_line = if reset_focused {
            Line::from(vec![
                Span::raw(" > "),
                Span::styled("Reset to default order", reset_style),
            ])
        } else {
            Line::from(vec![
                Span::raw("   "),
                Span::styled("Reset to default order", reset_style),
            ])
        };
        f.render_widget(Paragraph::new(reset_line), chunks[1 + self.checkbox_count]);

        // Close button
        let close_focused = self.focused == self.checkbox_count + 1;
        let close_style = if close_focused {
            Style::default()
                .fg(Color::Black)
                .bg(Color::White)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default()
        };
        let close_line = if close_focused {
            Line::from(vec![
                Span::raw(" > "),
                Span::styled("Close  [esc]", close_style),
            ])
        } else {
            Line::from(vec![
                Span::raw("   "),
                Span::styled("Close  [esc]", close_style),
            ])
        };
        f.render_widget(Paragraph::new(close_line), chunks[2 + self.checkbox_count]);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_column_toggle_esc_dismisses() {
        let mut state = ColumnToggleState::new(
            "Jobs".to_string(),
            vec!["ID".to_string(), "Name".to_string()],
            vec![],
            None,
        );
        let mut cfg = Config::default();
        let outcome = state.handle_key(KeyEvent::from(KeyCode::Esc), &mut cfg);
        assert_eq!(outcome, ModalOutcome::Dismiss(ColumnToggleResult::None));
    }

    #[test]
    fn test_column_toggle_space_toggles_checkbox() {
        let mut state = ColumnToggleState::new(
            "Jobs".to_string(),
            vec!["ID".to_string(), "Name".to_string()],
            vec![],
            None,
        );
        let mut cfg = Config::default();

        // Initially not hidden
        assert!(!state.hidden.contains("ID"));

        // Press space to toggle
        state.handle_key(KeyEvent::from(KeyCode::Char(' ')), &mut cfg);
        assert!(state.hidden.contains("ID"));

        // Press space again to toggle back
        state.handle_key(KeyEvent::from(KeyCode::Char(' ')), &mut cfg);
        assert!(!state.hidden.contains("ID"));
    }

    #[test]
    fn test_column_toggle_arrow_navigation() {
        let mut state = ColumnToggleState::new(
            "Jobs".to_string(),
            vec!["ID".to_string(), "Name".to_string()],
            vec![],
            None,
        );
        let mut cfg = Config::default();

        assert_eq!(state.focused, 0);

        state.handle_key(KeyEvent::from(KeyCode::Down), &mut cfg);
        assert_eq!(state.focused, 1);

        state.handle_key(KeyEvent::from(KeyCode::Up), &mut cfg);
        assert_eq!(state.focused, 0);
    }

    #[test]
    fn test_column_toggle_reset_button() {
        let mut state = ColumnToggleState::new(
            "Jobs".to_string(),
            vec!["ID".to_string(), "Name".to_string()],
            vec![],
            None,
        );
        let mut cfg = Config::default();

        // Navigate to reset button (after 2 checkboxes)
        state.focused = 2;
        let outcome = state.handle_key(KeyEvent::from(KeyCode::Enter), &mut cfg);
        assert_eq!(outcome, ModalOutcome::Dismiss(ColumnToggleResult::Reset));
    }
}

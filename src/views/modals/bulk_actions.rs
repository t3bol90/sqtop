//! Bulk job action modal.

use super::{centered_rect, ModalOutcome};
use crate::config::Config;
use crossterm::event::{KeyCode, KeyEvent};
use ratatui::layout::{Constraint, Direction, Layout};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph};
use ratatui::Frame;

/// Action selected from the bulk actions modal.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BulkAction {
    Cancel,
    Hold,
    Release,
    Requeue,
}

/// State for the bulk actions modal.
#[derive(Debug, Clone)]
pub struct BulkActionState {
    pub selected_count: usize,
    focused: usize,
    options: Vec<BulkActionOption>,
}

#[derive(Debug, Clone)]
struct BulkActionOption {
    label: String,
    action: Option<BulkAction>,
    style_variant: ButtonVariant,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ButtonVariant {
    Primary,
    Default,
    Warning,
    Error,
}

impl BulkActionState {
    /// Create a new bulk action modal for the given number of selected jobs.
    pub fn new(selected_count: usize) -> Self {
        let options = vec![
            BulkActionOption {
                label: "Cancel selected".to_string(),
                action: Some(BulkAction::Cancel),
                style_variant: ButtonVariant::Error,
            },
            BulkActionOption {
                label: "Hold selected".to_string(),
                action: Some(BulkAction::Hold),
                style_variant: ButtonVariant::Warning,
            },
            BulkActionOption {
                label: "Release selected".to_string(),
                action: Some(BulkAction::Release),
                style_variant: ButtonVariant::Default,
            },
            BulkActionOption {
                label: "Requeue selected".to_string(),
                action: Some(BulkAction::Requeue),
                style_variant: ButtonVariant::Primary,
            },
            BulkActionOption {
                label: "Close  [esc]".to_string(),
                action: None,
                style_variant: ButtonVariant::Default,
            },
        ];

        Self {
            selected_count,
            focused: 0,
            options,
        }
    }

    /// Handle a key event.
    pub fn handle_key(
        &mut self,
        key: KeyEvent,
        _config: &Config,
    ) -> ModalOutcome<Option<BulkAction>> {
        match key.code {
            KeyCode::Esc => ModalOutcome::Dismiss(None),
            KeyCode::Down | KeyCode::Tab => {
                self.focus_next();
                ModalOutcome::Continue
            }
            KeyCode::Up | KeyCode::BackTab => {
                self.focus_prev();
                ModalOutcome::Continue
            }
            KeyCode::Enter => {
                let action = self.options[self.focused].action;
                ModalOutcome::Dismiss(action)
            }
            _ => ModalOutcome::Continue,
        }
    }

    fn focus_next(&mut self) {
        self.focused = (self.focused + 1) % self.options.len();
    }

    fn focus_prev(&mut self) {
        self.focused = if self.focused == 0 {
            self.options.len() - 1
        } else {
            self.focused - 1
        };
    }

    /// Render the modal.
    pub fn render(&self, f: &mut Frame, area: ratatui::layout::Rect) {
        let height = 3 + self.options.len() as u16; // title + options + padding
        let modal_area = centered_rect(area, 54, height.min(area.height.saturating_sub(2)));

        f.render_widget(Clear, modal_area);

        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::Cyan))
            .title(" Bulk Actions ");

        let inner = block.inner(modal_area);
        f.render_widget(block, modal_area);

        // Split: title + options
        let constraints: Vec<_> = std::iter::once(Constraint::Length(1))
            .chain((0..self.options.len()).map(|_| Constraint::Length(1)))
            .collect();

        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints(constraints)
            .split(inner);

        // Title
        let title_text = format!("Bulk actions for {} selected jobs", self.selected_count);
        f.render_widget(Paragraph::new(title_text), chunks[0]);

        // Options
        for (i, opt) in self.options.iter().enumerate() {
            let is_focused = i == self.focused;
            let style = self.button_style(opt, is_focused);

            let line = if is_focused {
                Line::from(vec![Span::raw(" > "), Span::styled(&opt.label, style)])
            } else {
                Line::from(vec![Span::raw("   "), Span::styled(&opt.label, style)])
            };

            f.render_widget(Paragraph::new(line), chunks[1 + i]);
        }
    }

    fn button_style(&self, opt: &BulkActionOption, is_focused: bool) -> Style {
        let base_color = match opt.style_variant {
            ButtonVariant::Primary => Color::Cyan,
            ButtonVariant::Default => Color::White,
            ButtonVariant::Warning => Color::Yellow,
            ButtonVariant::Error => Color::Red,
        };

        if is_focused {
            Style::default()
                .fg(Color::Black)
                .bg(base_color)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(base_color)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_bulk_action_modal_esc_dismisses() {
        let mut state = BulkActionState::new(5);
        let cfg = Config::default();
        let outcome = state.handle_key(KeyEvent::from(KeyCode::Esc), &cfg);
        assert_eq!(outcome, ModalOutcome::Dismiss(None));
    }

    #[test]
    fn test_bulk_action_modal_arrow_navigation() {
        let mut state = BulkActionState::new(5);
        let cfg = Config::default();

        assert_eq!(state.focused, 0);

        state.handle_key(KeyEvent::from(KeyCode::Down), &cfg);
        assert_eq!(state.focused, 1);

        state.handle_key(KeyEvent::from(KeyCode::Up), &cfg);
        assert_eq!(state.focused, 0);
    }

    #[test]
    fn test_bulk_action_modal_wraps_navigation() {
        let mut state = BulkActionState::new(5);
        let cfg = Config::default();
        let len = state.options.len();

        // Go up from first -> wraps to last
        state.handle_key(KeyEvent::from(KeyCode::Up), &cfg);
        assert_eq!(state.focused, len - 1);

        // Go down from last -> wraps to first
        state.handle_key(KeyEvent::from(KeyCode::Down), &cfg);
        assert_eq!(state.focused, 0);
    }

    #[test]
    fn test_bulk_action_enter_returns_action() {
        let mut state = BulkActionState::new(5);
        let cfg = Config::default();

        // Focus on cancel (index 0)
        let outcome = state.handle_key(KeyEvent::from(KeyCode::Enter), &cfg);
        assert_eq!(outcome, ModalOutcome::Dismiss(Some(BulkAction::Cancel)));
    }
}

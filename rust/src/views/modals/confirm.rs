//! Generic Yes/No confirmation modal.

use super::{centered_rect, ModalOutcome};
use crate::config::Config;
use crossterm::event::{KeyCode, KeyEvent};
use ratatui::layout::{Constraint, Direction, Layout};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph, Wrap};
use ratatui::Frame;

/// Result of user confirming or canceling.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConfirmResult {
    Yes,
    No,
}

/// State for a Yes/No confirmation modal.
#[derive(Debug, Clone)]
pub struct ConfirmState {
    message: String,
    focused: FocusButton,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FocusButton {
    Yes,
    No,
}

impl ConfirmState {
    /// Create a new confirmation modal with the given message.
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            focused: FocusButton::Yes,
        }
    }

    /// Handle a key event. Returns Dismiss(result) or Continue.
    pub fn handle_key(&mut self, key: KeyEvent, _config: &Config) -> ModalOutcome<ConfirmResult> {
        match key.code {
            KeyCode::Char('y') | KeyCode::Char('Y') => ModalOutcome::Dismiss(ConfirmResult::Yes),
            KeyCode::Char('n') | KeyCode::Char('N') | KeyCode::Esc => {
                ModalOutcome::Dismiss(ConfirmResult::No)
            }
            KeyCode::Down | KeyCode::Tab => {
                self.focused = match self.focused {
                    FocusButton::Yes => FocusButton::No,
                    FocusButton::No => FocusButton::Yes,
                };
                ModalOutcome::Continue
            }
            KeyCode::Up | KeyCode::BackTab => {
                self.focused = match self.focused {
                    FocusButton::Yes => FocusButton::No,
                    FocusButton::No => FocusButton::Yes,
                };
                ModalOutcome::Continue
            }
            KeyCode::Enter => match self.focused {
                FocusButton::Yes => ModalOutcome::Dismiss(ConfirmResult::Yes),
                FocusButton::No => ModalOutcome::Dismiss(ConfirmResult::No),
            },
            _ => ModalOutcome::Continue,
        }
    }

    /// Render the modal.
    pub fn render(&self, f: &mut Frame, area: ratatui::layout::Rect) {
        let modal_area = centered_rect(area, 52, 8);

        // Clear the area
        f.render_widget(Clear, modal_area);

        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::Yellow))
            .title(" Confirm ");

        let inner = block.inner(modal_area);
        f.render_widget(block, modal_area);

        // Split into message and buttons
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(2), // message
                Constraint::Length(1), // gap
                Constraint::Length(1), // yes button
                Constraint::Length(1), // no button
            ])
            .split(inner);

        // Message
        let msg_para = Paragraph::new(self.message.clone()).wrap(Wrap { trim: false });
        f.render_widget(msg_para, chunks[0]);

        // Yes button
        let yes_style = if self.focused == FocusButton::Yes {
            Style::default()
                .fg(Color::White)
                .bg(Color::Red)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(Color::Red)
        };
        let yes_line = Line::from(vec![
            Span::raw("  "),
            Span::styled("Yes", yes_style),
            Span::raw("  "),
            Span::styled("[y]", Style::default().fg(Color::DarkGray)),
        ]);
        f.render_widget(Paragraph::new(yes_line), chunks[2]);

        // No button
        let no_style = if self.focused == FocusButton::No {
            Style::default()
                .fg(Color::White)
                .bg(Color::Gray)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(Color::White)
        };
        let no_line = Line::from(vec![
            Span::raw("  "),
            Span::styled("No", no_style),
            Span::raw("   "),
            Span::styled("[n / esc]", Style::default().fg(Color::DarkGray)),
        ]);
        f.render_widget(Paragraph::new(no_line), chunks[3]);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_confirm_y_returns_yes() {
        let mut state = ConfirmState::new("Delete?");
        let cfg = Config::default();
        let outcome = state.handle_key(KeyEvent::from(KeyCode::Char('y')), &cfg);
        assert_eq!(outcome, ModalOutcome::Dismiss(ConfirmResult::Yes));
    }

    #[test]
    fn test_confirm_n_returns_no() {
        let mut state = ConfirmState::new("Delete?");
        let cfg = Config::default();
        let outcome = state.handle_key(KeyEvent::from(KeyCode::Char('n')), &cfg);
        assert_eq!(outcome, ModalOutcome::Dismiss(ConfirmResult::No));
    }

    #[test]
    fn test_confirm_esc_returns_no() {
        let mut state = ConfirmState::new("Delete?");
        let cfg = Config::default();
        let outcome = state.handle_key(KeyEvent::from(KeyCode::Esc), &cfg);
        assert_eq!(outcome, ModalOutcome::Dismiss(ConfirmResult::No));
    }

    #[test]
    fn test_confirm_arrow_keys_cycle_focus() {
        let mut state = ConfirmState::new("Delete?");
        let cfg = Config::default();

        assert_eq!(state.focused, FocusButton::Yes);

        state.handle_key(KeyEvent::from(KeyCode::Down), &cfg);
        assert_eq!(state.focused, FocusButton::No);

        state.handle_key(KeyEvent::from(KeyCode::Down), &cfg);
        assert_eq!(state.focused, FocusButton::Yes);
    }
}

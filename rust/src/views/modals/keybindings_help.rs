//! Modal screen to show keybindings for the active pane.

use super::{centered_rect, ModalOutcome};
use crate::config::Config;
use crossterm::event::{KeyCode, KeyEvent};
// Layout imports removed - unused
use ratatui::style::{Color, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph, Wrap};
use ratatui::Frame;

/// State for the keybindings help modal.
#[derive(Debug, Clone)]
pub struct KeybindingsHelpState {
    pane_name: String,
    scroll_offset: usize,
}

impl KeybindingsHelpState {
    /// Create a new keybindings help modal.
    pub fn new(pane_name: String) -> Self {
        Self {
            pane_name,
            scroll_offset: 0,
        }
    }

    /// Handle a key event.
    pub fn handle_key(&mut self, key: KeyEvent, _config: &Config) -> ModalOutcome<()> {
        match key.code {
            KeyCode::Esc | KeyCode::Char('q') | KeyCode::Char('?') => ModalOutcome::Dismiss(()),
            KeyCode::Down | KeyCode::Char('j') => {
                self.scroll_offset = self.scroll_offset.saturating_add(1);
                ModalOutcome::Continue
            }
            KeyCode::Up | KeyCode::Char('k') => {
                self.scroll_offset = self.scroll_offset.saturating_sub(1);
                ModalOutcome::Continue
            }
            _ => ModalOutcome::Continue,
        }
    }

    /// Render the modal.
    pub fn render(&self, f: &mut Frame, area: ratatui::layout::Rect) {
        let modal_area = centered_rect(area, 76, 28.min(area.height.saturating_sub(2)));

        f.render_widget(Clear, modal_area);

        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::Cyan))
            .title(format!(" Keybindings — {} pane ", self.pane_name));

        let inner = block.inner(modal_area);
        f.render_widget(block, modal_area);

        // Build help text
        let mut lines = Vec::new();

        // Global bindings
        lines.push(Line::from(Span::styled(
            "Global",
            Style::default().fg(Color::Cyan),
        )));
        lines.extend(self.global_bindings());
        lines.push(Line::from(""));

        // Pane-specific bindings
        lines.push(Line::from(Span::styled(
            self.pane_name.clone(),
            Style::default().fg(Color::Cyan),
        )));
        lines.extend(self.pane_bindings());
        lines.push(Line::from(""));

        // Clipboard bindings
        lines.push(Line::from(Span::styled(
            "Clipboard",
            Style::default().fg(Color::Cyan),
        )));
        lines.extend(self.clipboard_bindings());
        lines.push(Line::from(""));

        lines.push(Line::from(Span::styled(
            "Press Esc, q, or ? to close",
            Style::default().fg(Color::DarkGray),
        )));

        // Skip lines based on scroll offset
        let visible_lines: Vec<_> = lines.into_iter().skip(self.scroll_offset).collect();

        let para = Paragraph::new(visible_lines).wrap(Wrap { trim: false });
        f.render_widget(para, inner);
    }

    fn global_bindings(&self) -> Vec<Line<'static>> {
        vec![
            self.binding_line("?", "Show keybindings"),
            self.binding_line("Tab", "Next tab"),
            self.binding_line("Shift+Tab", "Previous tab"),
            self.binding_line("S", "Settings"),
            self.binding_line("R", "Reload config"),
            self.binding_line("q", "Quit"),
        ]
    }

    fn pane_bindings(&self) -> Vec<Line<'static>> {
        match self.pane_name.as_str() {
            "Jobs" => vec![
                self.binding_line("Enter", "Job actions"),
                self.binding_line("c", "Cancel job"),
                self.binding_line("h", "Hold job"),
                self.binding_line("r", "Release job"),
                self.binding_line("s", "Sort by state"),
                self.binding_line("t", "Sort by time"),
                self.binding_line("u", "Sort by user"),
                self.binding_line("m", "Toggle my jobs filter"),
                self.binding_line("/", "Search"),
                self.binding_line("C", "Column visibility"),
                self.binding_line("B", "Bulk actions"),
            ],
            "Nodes" => vec![
                self.binding_line("Enter", "Node details"),
                self.binding_line("s", "Sort by state"),
                self.binding_line("n", "Sort by name"),
                self.binding_line("C", "Column visibility"),
            ],
            "Partitions" => vec![
                self.binding_line("s", "Sort by name"),
                self.binding_line("a", "Sort by available"),
            ],
            _ => vec![],
        }
    }

    fn clipboard_bindings(&self) -> Vec<Line<'static>> {
        vec![
            self.binding_line("y", "Copy job ID (Jobs) / Yank visual selection"),
            self.binding_line("Y", "Copy current row as TSV (Jobs only)"),
            self.binding_line("v", "Enter visual selection mode (data tables)"),
            self.binding_line("V", "Enter visual-line mode (data tables)"),
            self.binding_line("Esc", "Exit visual mode"),
            self.binding_line("Ctrl+Shift+Y", "Copy entire pane as TSV"),
            self.binding_line("Ctrl+C", "Copy selection (text-pane modals)"),
        ]
    }

    fn binding_line(&self, key: &str, desc: &str) -> Line<'static> {
        Line::from(vec![
            Span::raw("  "),
            Span::styled(format!("{:<14}", key), Style::default().fg(Color::Cyan)),
            Span::raw(desc.to_string()),
        ])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_keybindings_help_esc_dismisses() {
        let mut state = KeybindingsHelpState::new("Jobs".to_string());
        let cfg = Config::default();
        let outcome = state.handle_key(KeyEvent::from(KeyCode::Esc), &cfg);
        assert_eq!(outcome, ModalOutcome::Dismiss(()));
    }

    #[test]
    fn test_keybindings_help_q_dismisses() {
        let mut state = KeybindingsHelpState::new("Jobs".to_string());
        let cfg = Config::default();
        let outcome = state.handle_key(KeyEvent::from(KeyCode::Char('q')), &cfg);
        assert_eq!(outcome, ModalOutcome::Dismiss(()));
    }

    #[test]
    fn test_keybindings_help_question_dismisses() {
        let mut state = KeybindingsHelpState::new("Jobs".to_string());
        let cfg = Config::default();
        let outcome = state.handle_key(KeyEvent::from(KeyCode::Char('?')), &cfg);
        assert_eq!(outcome, ModalOutcome::Dismiss(()));
    }

    #[test]
    fn test_keybindings_help_scroll_down() {
        let mut state = KeybindingsHelpState::new("Jobs".to_string());
        let cfg = Config::default();

        assert_eq!(state.scroll_offset, 0);

        state.handle_key(KeyEvent::from(KeyCode::Down), &cfg);
        assert_eq!(state.scroll_offset, 1);

        state.handle_key(KeyEvent::from(KeyCode::Down), &cfg);
        assert_eq!(state.scroll_offset, 2);
    }

    #[test]
    fn test_keybindings_help_scroll_up() {
        let mut state = KeybindingsHelpState::new("Jobs".to_string());
        let cfg = Config::default();

        state.scroll_offset = 5;

        state.handle_key(KeyEvent::from(KeyCode::Up), &cfg);
        assert_eq!(state.scroll_offset, 4);

        // Should saturate at 0
        state.scroll_offset = 1;
        state.handle_key(KeyEvent::from(KeyCode::Up), &cfg);
        assert_eq!(state.scroll_offset, 0);

        state.handle_key(KeyEvent::from(KeyCode::Up), &cfg);
        assert_eq!(state.scroll_offset, 0);
    }
}

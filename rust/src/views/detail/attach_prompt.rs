//! Prompt for optional node override when attaching to a job.

use crate::config::AttachConfig;
use crate::slurm::fetch::build_attach_command;
use crate::views::detail::Outcome;
use crossterm::event::{KeyCode, KeyEvent};
use ratatui::{
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph, Wrap},
    Frame,
};

/// Modal prompt for node expression override when attaching to a job.
pub struct AttachPromptScreen {
    pub job_id: String,
    input: String,
    title: String,
    placeholder: String,
}

impl AttachPromptScreen {
    /// Create a new attach prompt with the default node.
    pub fn new(job_id: String, default_node: String) -> Self {
        Self {
            job_id,
            input: default_node,
            title: "Attach with node override".to_string(),
            placeholder: "node name/expression (empty to skip -w)".to_string(),
        }
    }

    /// Check if attach is enabled in config.
    pub fn check_enabled(config: &AttachConfig) -> Result<(), String> {
        if !config.enabled {
            Err("Attach is disabled in config ([attach] enabled = false)".to_string())
        } else {
            Ok(())
        }
    }

    /// Build the attach command using the config and user's node override.
    ///
    /// Returns the command vector ready for execution, or None if the
    /// node override is invalid.
    pub fn build_command(&self, job_id: &str, config: &AttachConfig) -> Vec<String> {
        let node = if self.input.trim().is_empty() {
            None
        } else {
            Some(self.input.trim())
        };

        build_attach_command(job_id, node, &config.default_command, &config.extra_args)
    }

    /// Handle key input.
    pub fn handle_key(&mut self, key: KeyEvent) -> Outcome {
        match key.code {
            KeyCode::Esc => Outcome::Close,
            KeyCode::Enter => {
                let value = self.input.trim().to_string();
                Outcome::Value(value)
            }
            KeyCode::Char(c) => {
                self.input.push(c);
                Outcome::None
            }
            KeyCode::Backspace => {
                self.input.pop();
                Outcome::None
            }
            _ => Outcome::None,
        }
    }

    /// Render the attach prompt.
    pub fn render(&self, f: &mut Frame, area: Rect) {
        // Center the dialog
        let dialog_width = 60.min(area.width.saturating_sub(4));
        let dialog_height = 8;
        let dialog_area = centered_rect(dialog_width, dialog_height, area);

        // Clear background
        f.render_widget(
            Block::default().style(Style::default().bg(Color::Reset)),
            dialog_area,
        );

        // Render border
        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            )
            .style(Style::default().bg(Color::Black));
        f.render_widget(block.clone(), dialog_area);

        // Inner layout
        let inner = block.inner(dialog_area);
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(1), // title
                Constraint::Length(1), // spacing
                Constraint::Length(3), // input
                Constraint::Length(1), // spacing
                Constraint::Length(1), // help
            ])
            .split(inner);

        // Title
        let title = Paragraph::new(Span::styled(
            &self.title,
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        ))
        .alignment(Alignment::Center);
        f.render_widget(title, chunks[0]);

        // Input field
        let input_text = if self.input.is_empty() {
            Line::from(Span::styled(
                &self.placeholder,
                Style::default().fg(Color::DarkGray),
            ))
        } else {
            Line::from(Span::raw(&self.input))
        };
        let input = Paragraph::new(input_text)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(Color::White)),
            )
            .wrap(Wrap { trim: false });
        f.render_widget(input, chunks[2]);

        // Help text
        let help = Paragraph::new(Line::from(vec![
            Span::styled("Enter", Style::default().fg(Color::Green)),
            Span::raw(" = attach  "),
            Span::styled("Esc", Style::default().fg(Color::Red)),
            Span::raw(" = cancel"),
        ]))
        .alignment(Alignment::Center);
        f.render_widget(help, chunks[4]);
    }
}

/// Create a centered rect with the given width and height.
fn centered_rect(width: u16, height: u16, area: Rect) -> Rect {
    let x = area.x + (area.width.saturating_sub(width)) / 2;
    let y = area.y + (area.height.saturating_sub(height)) / 2;
    Rect {
        x,
        y,
        width: width.min(area.width),
        height: height.min(area.height),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_check_enabled_when_true() {
        let config = AttachConfig {
            enabled: true,
            default_command: "bash".to_string(),
            extra_args: "".to_string(),
        };
        assert!(AttachPromptScreen::check_enabled(&config).is_ok());
    }

    #[test]
    fn test_check_enabled_when_false() {
        let config = AttachConfig {
            enabled: false,
            default_command: "bash".to_string(),
            extra_args: "".to_string(),
        };
        let result = AttachPromptScreen::check_enabled(&config);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("disabled"));
    }

    #[test]
    fn test_build_command_with_node_and_extra_args() {
        let config = AttachConfig {
            enabled: true,
            default_command: "bash -l".to_string(),
            extra_args: "--mpi=none".to_string(),
        };
        let mut screen = AttachPromptScreen::new("job123".to_string(), "c1".to_string());
        screen.input = "c2".to_string();

        let cmd = screen.build_command("12345", &config);
        assert_eq!(
            cmd,
            vec![
                "srun",
                "--pty",
                "--overlap",
                "--mpi=none",
                "--jobid",
                "12345",
                "-w",
                "c2",
                "bash",
                "-l",
            ]
        );
    }

    #[test]
    fn test_build_command_without_node() {
        let config = AttachConfig {
            enabled: true,
            default_command: "bash -l".to_string(),
            extra_args: "".to_string(),
        };
        let mut screen = AttachPromptScreen::new("job123".to_string(), "".to_string());
        screen.input = "".to_string();

        let cmd = screen.build_command("12345", &config);
        assert!(!cmd.contains(&"-w".to_string()));
        assert!(cmd.contains(&"bash".to_string()));
        assert!(cmd.contains(&"12345".to_string()));
    }

    #[test]
    fn test_build_command_with_whitespace_node_treated_as_empty() {
        let config = AttachConfig {
            enabled: true,
            default_command: "bash".to_string(),
            extra_args: "".to_string(),
        };
        let mut screen = AttachPromptScreen::new("job123".to_string(), "".to_string());
        screen.input = "   ".to_string();

        let cmd = screen.build_command("12345", &config);
        assert!(!cmd.contains(&"-w".to_string()));
    }

    #[test]
    fn test_build_command_honors_default_command_from_config() {
        let config = AttachConfig {
            enabled: true,
            default_command: "zsh -l".to_string(),
            extra_args: "".to_string(),
        };
        let screen = AttachPromptScreen::new("job123".to_string(), "".to_string());

        let cmd = screen.build_command("12345", &config);
        assert!(cmd.contains(&"zsh".to_string()));
        assert!(cmd.contains(&"-l".to_string()));
    }

    #[test]
    fn test_build_command_honors_extra_args_from_config() {
        let config = AttachConfig {
            enabled: true,
            default_command: "bash".to_string(),
            extra_args: "--exclusive --mpi=pmi2".to_string(),
        };
        let screen = AttachPromptScreen::new("job123".to_string(), "".to_string());

        let cmd = screen.build_command("12345", &config);
        assert!(cmd.contains(&"--exclusive".to_string()));
        assert!(cmd.contains(&"--mpi=pmi2".to_string()));
    }

    #[test]
    fn test_handle_key_enter_returns_value() {
        let mut screen = AttachPromptScreen::new("job123".to_string(), "node01".to_string());
        screen.input = "node02".to_string();

        let outcome = screen.handle_key(KeyEvent::from(KeyCode::Enter));
        assert_eq!(outcome, Outcome::Value("node02".to_string()));
    }

    #[test]
    fn test_handle_key_esc_closes() {
        let mut screen = AttachPromptScreen::new("job123".to_string(), "".to_string());
        let outcome = screen.handle_key(KeyEvent::from(KeyCode::Esc));
        assert_eq!(outcome, Outcome::Close);
    }

    #[test]
    fn test_handle_key_char_appends_to_input() {
        let mut screen = AttachPromptScreen::new("job123".to_string(), "".to_string());
        screen.input.clear();

        screen.handle_key(KeyEvent::from(KeyCode::Char('c')));
        screen.handle_key(KeyEvent::from(KeyCode::Char('1')));
        assert_eq!(screen.input, "c1");
    }

    #[test]
    fn test_handle_key_backspace_removes_char() {
        let mut screen = AttachPromptScreen::new("job123".to_string(), "".to_string());
        screen.input = "abc".to_string();

        screen.handle_key(KeyEvent::from(KeyCode::Backspace));
        assert_eq!(screen.input, "ab");
    }
}

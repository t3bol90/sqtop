//! Job dependency graph modal.

use crate::slurm::fetch::JobDependency;
use crate::slurm::model::Job;
use crate::views::detail::Outcome;
use crossterm::event::{KeyCode, KeyEvent};
use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph, Wrap},
    Frame,
};

/// Job dependency graph viewer.
pub struct DependencyScreen {
    job: Job,
    deps: Vec<JobDependency>,
}

impl DependencyScreen {
    /// Create a new dependency screen.
    pub fn new(job: Job, deps: Vec<JobDependency>) -> Self {
        Self { job, deps }
    }

    /// Handle key input.
    pub fn handle_key(&mut self, key: KeyEvent) -> Outcome {
        match key.code {
            KeyCode::Esc | KeyCode::Char('q') | KeyCode::Char('D') => Outcome::Close,
            _ => Outcome::None,
        }
    }

    /// Render the dependency screen.
    pub fn render(&self, f: &mut Frame, area: Rect) {
        // Smaller dialog (60 width, auto height)
        let width = (area.width * 9 / 10).clamp(40, 60);
        let height = (area.height * 8 / 10).clamp(10, 20);
        let dialog_area = centered_rect(width, height, area);

        // Render border
        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            )
            .style(Style::default().bg(Color::Black));
        // Clear the cells first. A Block only restyles them, so without this the
        // table underneath stays visible through the dialog.
        f.render_widget(ratatui::widgets::Clear, dialog_area);
        f.render_widget(block.clone(), dialog_area);

        // Inner layout
        let inner = block.inner(dialog_area);
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Length(1), Constraint::Min(0)])
            .split(inner);

        // Title
        let header_text = format!("Dependencies — Job {} ({})", self.job.job_id, self.job.name);
        let title = Paragraph::new(Span::styled(
            header_text,
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        ));
        f.render_widget(title, chunks[0]);

        // Content
        let mut lines = Vec::new();

        // Show the current job
        let job_color = if self.job.state == "RUNNING" {
            Color::Green
        } else {
            Color::Yellow
        };
        lines.push(Line::from(vec![Span::styled(
            format!("{} {}  {}", self.job.job_id, self.job.name, self.job.state),
            Style::default().fg(job_color).add_modifier(Modifier::BOLD),
        )]));

        // Show dependencies
        if self.deps.is_empty() {
            lines.push(Line::from(Span::styled(
                "  (no dependencies)",
                Style::default().fg(Color::DarkGray),
            )));
        } else {
            for dep in &self.deps {
                let state_color = if dep.state == "COMPLETED" {
                    Color::Green
                } else if dep.state == "FAILED" || dep.state == "CANCELLED" {
                    Color::Red
                } else {
                    Color::Yellow
                };

                let icon = if dep.state == "COMPLETED" {
                    Span::styled("✓", Style::default().fg(Color::Green))
                } else if dep.state == "FAILED" || dep.state == "CANCELLED" {
                    Span::styled("✗", Style::default().fg(Color::Red))
                } else {
                    Span::styled("…", Style::default().fg(Color::Yellow))
                };

                let state_display = if dep.state.is_empty() {
                    "COMPLETED"
                } else {
                    &dep.state
                };

                lines.push(Line::from(vec![
                    Span::raw("  "),
                    icon,
                    Span::raw(" "),
                    Span::styled(&dep.job_id, Style::default().fg(state_color)),
                    Span::raw("  "),
                    Span::styled(&dep.dep_type, Style::default().fg(Color::DarkGray)),
                    Span::raw("  "),
                    Span::styled(state_display, Style::default().fg(state_color)),
                ]));
            }
        }

        let content = Paragraph::new(lines).wrap(Wrap { trim: false });
        f.render_widget(content, chunks[1]);
    }
}

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
    fn test_dependency_screen_creation() {
        let job = Job {
            job_id: "12345".to_string(),
            name: "test_job".to_string(),
            state: "RUNNING".to_string(),
            ..Default::default()
        };

        let deps = vec![JobDependency {
            dep_type: "afterok".to_string(),
            job_id: "12340".to_string(),
            state: "COMPLETED".to_string(),
        }];

        let screen = DependencyScreen::new(job, deps);
        assert_eq!(screen.job.job_id, "12345");
        assert_eq!(screen.deps.len(), 1);
    }
}

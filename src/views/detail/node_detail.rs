//! Node detail modal.

use crate::slurm::model::{Job, Node};
use crate::views::detail::Outcome;
use crossterm::event::{KeyCode, KeyEvent};
use ratatui::{
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph, Wrap},
    Frame,
};

/// Node detail viewer.
pub struct NodeDetailScreen {
    node: Node,
    fields: Vec<(String, String)>,
    scroll_offset: usize,
    lines: Vec<String>,
    jobs: Vec<Job>,
}

impl NodeDetailScreen {
    /// Create a new node detail screen.
    pub fn new(node: Node, fields: Vec<(String, String)>) -> Self {
        let lines = build_detail_lines(&node, &fields);
        Self {
            node,
            fields,
            scroll_offset: 0,
            lines,
            jobs: Vec::new(),
        }
    }

    /// Set the jobs running on this node.
    pub fn set_jobs(&mut self, jobs: Vec<Job>) {
        self.jobs = jobs;
        // Rebuild lines to include jobs
        self.lines = build_detail_lines_with_jobs(&self.node, &self.fields, &self.jobs);
    }

    /// Handle key input.
    pub fn handle_key(&mut self, key: KeyEvent) -> Outcome {
        match key.code {
            KeyCode::Esc | KeyCode::Char('q') => Outcome::Close,
            KeyCode::Up | KeyCode::Char('k') => {
                self.scroll_offset = self.scroll_offset.saturating_sub(1);
                Outcome::None
            }
            KeyCode::Down | KeyCode::Char('j') => {
                if self.scroll_offset < self.lines.len().saturating_sub(1) {
                    self.scroll_offset += 1;
                }
                Outcome::None
            }
            KeyCode::PageUp => {
                self.scroll_offset = self.scroll_offset.saturating_sub(10);
                Outcome::None
            }
            KeyCode::PageDown => {
                let max_scroll = self.lines.len().saturating_sub(1);
                self.scroll_offset = (self.scroll_offset + 10).min(max_scroll);
                Outcome::None
            }
            _ => Outcome::None,
        }
    }

    /// Render the node detail screen.
    pub fn render(&self, f: &mut Frame, area: Rect) {
        // Center the dialog (90% width, 85% height)
        let width = (area.width * 9 / 10).clamp(60, 140);
        let height = (area.height * 85 / 100).clamp(20, 50);
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
        let header_text = format!("Node {}  [{}]", self.node.name, self.node.state);
        let title = Paragraph::new(Span::styled(
            header_text,
            Style::default()
                .fg(Color::Black)
                .bg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        ))
        .alignment(Alignment::Center);
        f.render_widget(title, chunks[0]);

        // Detail content
        let content_height = chunks[1].height as usize;
        let visible_lines: Vec<Line> = self
            .lines
            .iter()
            .skip(self.scroll_offset)
            .take(content_height)
            .map(|s| Line::from(Span::raw(s.as_str())))
            .collect();

        let content = Paragraph::new(visible_lines).wrap(Wrap { trim: false });
        f.render_widget(content, chunks[1]);
    }

    /// Get the plain text content for clipboard copy.
    pub fn plain_text(&self) -> String {
        self.lines.join("\n")
    }

    /// Get the label for clipboard copy.
    pub fn label(&self) -> String {
        format!("Node {} Detail", self.node.name)
    }
}

fn build_detail_lines(node: &Node, fields: &[(String, String)]) -> Vec<String> {
    build_detail_lines_with_jobs(node, fields, &[])
}

fn build_detail_lines_with_jobs(
    _node: &Node,
    fields: &[(String, String)],
    jobs: &[Job],
) -> Vec<String> {
    // scontrol order, matching the Python version. See job_detail.rs.
    let mut lines = vec!["Node Detail\n".to_string()];
    for (k, v) in fields {
        lines.push(format!("  {}: {}", k, v));
    }

    // Jobs section
    if !jobs.is_empty() {
        lines.push(String::new());
        lines.push(format!("Jobs on Node ({}):", jobs.len()));
        for job in jobs {
            lines.push(format!(
                "  {} - {} [{}] - {}",
                job.job_id, job.name, job.state, job.user
            ));
        }
    }

    lines
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
    fn test_build_detail_lines_includes_highlight_keys() {
        let node = Node {
            name: "node01".to_string(),
            state: "idle".to_string(),
            ..Default::default()
        };
        let fields = vec![
            ("NodeName".to_string(), "node01".to_string()),
            ("State".to_string(), "idle".to_string()),
            ("CPUTot".to_string(), "32".to_string()),
        ];

        let lines = build_detail_lines(&node, &fields);
        assert_eq!(lines[1], "  NodeName: node01");
        assert_eq!(lines[2], "  State: idle");
        assert_eq!(lines[3], "  CPUTot: 32");
    }
}

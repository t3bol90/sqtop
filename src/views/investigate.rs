//! Investigation screen — evidence-based per-job/node report (SPEC sec. 8).
//!
//! This module implements the investigation modal screen that displays an
//! InvestigationReport with scrolling. The domain logic (ReasonTable,
//! InvestigationReport, render_report) is in the investigation module; this
//! screen only handles layout and display.

use crate::investigation::{render_report, InvestigationReport};
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{
    Block, Borders, Paragraph, Scrollbar, ScrollbarOrientation, ScrollbarState,
};
use ratatui::Frame;

/// Investigation screen state.
#[derive(Debug, Clone)]
pub struct InvestigationScreen {
    /// The target identifier (job_id or node_name)
    pub target_id: String,
    /// Optional display name
    pub target_name: Option<String>,
    /// Whether this is a job or node investigation
    pub is_job: bool,
    /// Scroll offset (line number)
    pub scroll_offset: u16,
    /// Rendered report text (lines)
    pub report_lines: Vec<String>,
    /// Whether the report is loaded
    pub loaded: bool,
}

impl InvestigationScreen {
    /// Create a new investigation screen for a job.
    pub fn for_job(job_id: String, job_name: Option<String>) -> Self {
        Self {
            target_id: job_id,
            target_name: job_name,
            is_job: true,
            scroll_offset: 0,
            report_lines: vec!["Loading…".to_string()],
            loaded: false,
        }
    }

    /// Create a new investigation screen for a node.
    pub fn for_node(node_name: String) -> Self {
        Self {
            target_id: node_name,
            target_name: None,
            is_job: false,
            scroll_offset: 0,
            report_lines: vec!["Loading…".to_string()],
            loaded: false,
        }
    }

    /// Load a report and render it to text.
    pub fn load_report(&mut self, report: InvestigationReport) {
        let text = render_report(&report);
        self.report_lines = text.lines().map(|s| s.to_string()).collect();
        self.loaded = true;
    }

    /// Scroll down by the given number of lines.
    pub fn scroll_down(&mut self, amount: u16) {
        let max_offset = self.report_lines.len().saturating_sub(1) as u16;
        self.scroll_offset = (self.scroll_offset + amount).min(max_offset);
    }

    /// Scroll up by the given number of lines.
    pub fn scroll_up(&mut self, amount: u16) {
        self.scroll_offset = self.scroll_offset.saturating_sub(amount);
    }

    /// Scroll to the top.
    pub fn scroll_to_top(&mut self) {
        self.scroll_offset = 0;
    }

    /// Scroll to the bottom.
    pub fn scroll_to_bottom(&mut self) {
        let max_offset = self.report_lines.len().saturating_sub(1) as u16;
        self.scroll_offset = max_offset;
    }

    /// Handle key input for the investigation screen.
    pub fn handle_key(&mut self, key: crossterm::event::KeyEvent) -> crate::views::detail::Outcome {
        use crate::views::detail::Outcome;
        use crossterm::event::KeyCode;

        match key.code {
            KeyCode::Esc | KeyCode::Char('q') => Outcome::Close,
            KeyCode::Up | KeyCode::Char('k') => {
                self.scroll_up(1);
                Outcome::None
            }
            KeyCode::Down | KeyCode::Char('j') => {
                self.scroll_down(1);
                Outcome::None
            }
            KeyCode::PageUp => {
                self.scroll_up(10);
                Outcome::None
            }
            KeyCode::PageDown => {
                self.scroll_down(10);
                Outcome::None
            }
            KeyCode::Home | KeyCode::Char('g') => {
                self.scroll_to_top();
                Outcome::None
            }
            KeyCode::End | KeyCode::Char('G') => {
                self.scroll_to_bottom();
                Outcome::None
            }
            _ => Outcome::None,
        }
    }

    /// Get the title for the screen.
    fn title(&self) -> String {
        let kind = if self.is_job { "Job" } else { "Node" };
        let mut title = format!("Investigate {} {}", kind, self.target_id);
        if let Some(ref name) = self.target_name {
            title.push_str(&format!(" — {}", name));
        }
        title
    }
}

/// Render the investigation screen.
pub fn render(f: &mut Frame, area: Rect, screen: &mut InvestigationScreen) {
    // Calculate centered dialog area (90% width, 85% height)
    let dialog_width = (area.width * 90 / 100).clamp(60, 140);
    let dialog_height = (area.height * 85 / 100).clamp(20, 50);

    // Handle xs tier: full screen
    let (dialog_width, dialog_height) = if area.width < 80 || area.height < 24 {
        (area.width, area.height)
    } else {
        (dialog_width, dialog_height)
    };

    let h_margin = (area.width.saturating_sub(dialog_width)) / 2;
    let v_margin = (area.height.saturating_sub(dialog_height)) / 2;

    let dialog_rect = Rect {
        x: area.x + h_margin,
        y: area.y + v_margin,
        width: dialog_width,
        height: dialog_height,
    };

    // Clear the cells first. A Block only restyles them, so without this the
    // table underneath stays visible through the dialog.
    f.render_widget(ratatui::widgets::Clear, dialog_rect);

    // Split dialog into title and content
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3), // Title
            Constraint::Min(1),    // Content
        ])
        .split(dialog_rect);

    // Render title
    let title_block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Cyan))
        .style(Style::default().bg(Color::Black));

    let title_text = vec![Line::from(vec![Span::styled(
        screen.title(),
        Style::default()
            .fg(Color::Cyan)
            .add_modifier(Modifier::BOLD),
    )])];

    let title_para = Paragraph::new(title_text)
        .block(title_block)
        .style(Style::default());

    f.render_widget(title_para, chunks[0]);

    // Render content with scrolling
    let content_block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::White))
        .style(Style::default().bg(Color::Black));

    let inner = content_block.inner(chunks[1]);
    let visible_height = inner.height as usize;

    // Clamp scroll offset
    let max_scroll = screen.report_lines.len().saturating_sub(visible_height);
    screen.scroll_offset = (screen.scroll_offset as usize).min(max_scroll) as u16;

    let start = screen.scroll_offset as usize;
    let end = (start + visible_height).min(screen.report_lines.len());
    let visible_lines: Vec<Line> = screen.report_lines[start..end]
        .iter()
        .map(|s| Line::from(s.as_str()))
        .collect();

    let content = Paragraph::new(visible_lines)
        .block(content_block)
        .style(Style::default());

    f.render_widget(content, chunks[1]);

    // Render scrollbar if needed
    if screen.report_lines.len() > visible_height {
        let scrollbar = Scrollbar::new(ScrollbarOrientation::VerticalRight)
            .begin_symbol(Some("↑"))
            .end_symbol(Some("↓"));

        let mut scrollbar_state =
            ScrollbarState::new(screen.report_lines.len()).position(screen.scroll_offset as usize);

        f.render_stateful_widget(scrollbar, chunks[1], &mut scrollbar_state);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::investigation::{InvestigationReport, InvestigationTarget};
    use std::collections::HashMap;

    #[test]
    fn test_investigation_screen_for_job() {
        let screen =
            InvestigationScreen::for_job("12345".to_string(), Some("train-a100".to_string()));
        assert_eq!(screen.target_id, "12345");
        assert_eq!(screen.target_name, Some("train-a100".to_string()));
        assert!(screen.is_job);
        assert!(!screen.loaded);
        assert_eq!(screen.scroll_offset, 0);
    }

    #[test]
    fn test_investigation_screen_for_node() {
        let screen = InvestigationScreen::for_node("gpu-a100-02".to_string());
        assert_eq!(screen.target_id, "gpu-a100-02");
        assert_eq!(screen.target_name, None);
        assert!(!screen.is_job);
        assert!(!screen.loaded);
    }

    #[test]
    fn test_load_report() {
        let mut screen = InvestigationScreen::for_job("12345".to_string(), None);
        let target = InvestigationTarget {
            kind: "job".to_string(),
            identifier: "12345".to_string(),
            source: "cursor".to_string(),
        };
        let report = InvestigationReport {
            target,
            summary: Vec::new(),
            evidence: Vec::new(),
            explanations: Vec::new(),
            related_jobs: Vec::new(),
            related_nodes: Vec::new(),
            suggested_actions: Vec::new(),
            raw_sections: HashMap::new(),
            errors: Vec::new(),
        };

        screen.load_report(report);
        assert!(screen.loaded);
        assert!(!screen.report_lines.is_empty());
    }

    #[test]
    fn test_scroll_down() {
        let mut screen = InvestigationScreen::for_job("12345".to_string(), None);
        screen.report_lines = (0..100).map(|i| format!("Line {}", i)).collect();

        screen.scroll_down(5);
        assert_eq!(screen.scroll_offset, 5);

        screen.scroll_down(10);
        assert_eq!(screen.scroll_offset, 15);

        // Can't scroll past the end
        screen.scroll_down(1000);
        assert_eq!(screen.scroll_offset, 99);
    }

    #[test]
    fn test_scroll_up() {
        let mut screen = InvestigationScreen::for_job("12345".to_string(), None);
        screen.report_lines = (0..100).map(|i| format!("Line {}", i)).collect();
        screen.scroll_offset = 50;

        screen.scroll_up(10);
        assert_eq!(screen.scroll_offset, 40);

        screen.scroll_up(100);
        assert_eq!(screen.scroll_offset, 0);
    }

    #[test]
    fn test_scroll_to_top_bottom() {
        let mut screen = InvestigationScreen::for_job("12345".to_string(), None);
        screen.report_lines = (0..100).map(|i| format!("Line {}", i)).collect();

        screen.scroll_offset = 50;
        screen.scroll_to_top();
        assert_eq!(screen.scroll_offset, 0);

        screen.scroll_to_bottom();
        assert_eq!(screen.scroll_offset, 99);
    }

    #[test]
    fn test_title_job() {
        let screen = InvestigationScreen::for_job("12345".to_string(), Some("train".to_string()));
        assert_eq!(screen.title(), "Investigate Job 12345 — train");
    }

    #[test]
    fn test_title_node() {
        let screen = InvestigationScreen::for_node("node01".to_string());
        assert_eq!(screen.title(), "Investigate Node node01");
    }

    #[test]
    fn test_scroll_clamp_empty() {
        let mut screen = InvestigationScreen::for_job("12345".to_string(), None);
        screen.report_lines = vec![];

        screen.scroll_down(10);
        assert_eq!(screen.scroll_offset, 0);
    }

    #[test]
    fn test_scroll_clamp_single_line() {
        let mut screen = InvestigationScreen::for_job("12345".to_string(), None);
        screen.report_lines = vec!["Only line".to_string()];

        screen.scroll_down(10);
        assert_eq!(screen.scroll_offset, 0);
    }
}

//! Batch script viewer modal.

use crate::views::detail::Outcome;
use crossterm::event::{KeyCode, KeyEvent};
use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph, Wrap},
    Frame,
};

/// Scroll position: either at a specific offset or at the bottom.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Scroll {
    At(usize),
    Bottom,
}

/// Scrollable batch script viewer.
pub struct BatchScriptScreen {
    job_id: String,
    script: String,
    lines: Vec<String>,
    scroll: Scroll,
    /// Viewport height from the last render; lets key handling resolve `Bottom`
    /// to the same offset the user is actually looking at.
    viewport_height: usize,
}

impl BatchScriptScreen {
    /// Create a new batch script viewer with the given job ID and script content.
    pub fn new(job_id: String, script: String) -> Self {
        let lines: Vec<String> = script.lines().map(|s| s.to_string()).collect();
        Self {
            job_id,
            script,
            lines,
            scroll: Scroll::At(0),
            viewport_height: 0,
        }
    }

    /// Resolve the scroll position to a concrete offset given the viewport height.
    pub fn resolved_offset(&self, content_height: usize) -> usize {
        match self.scroll {
            Scroll::At(offset) => offset.min(self.lines.len().saturating_sub(content_height)),
            Scroll::Bottom => self.lines.len().saturating_sub(content_height),
        }
    }

    /// Handle key input.
    pub fn handle_key(&mut self, key: KeyEvent) -> Outcome {
        match key.code {
            KeyCode::Esc | KeyCode::Char('q') => Outcome::Close,
            KeyCode::Up | KeyCode::Char('k') => {
                let current = self.resolved_offset(self.viewport_height);
                self.scroll = Scroll::At(current.saturating_sub(1));
                Outcome::None
            }
            KeyCode::Down | KeyCode::Char('j') => {
                let current = self.resolved_offset(self.viewport_height);
                self.scroll = Scroll::At(current.saturating_add(1));
                Outcome::None
            }
            KeyCode::PageUp => {
                let current = self.resolved_offset(self.viewport_height);
                self.scroll = Scroll::At(current.saturating_sub(10));
                Outcome::None
            }
            KeyCode::PageDown => {
                let current = self.resolved_offset(self.viewport_height);
                self.scroll = Scroll::At(current.saturating_add(10));
                Outcome::None
            }
            KeyCode::Home => {
                self.scroll = Scroll::At(0);
                Outcome::None
            }
            KeyCode::End => {
                self.scroll = Scroll::Bottom;
                Outcome::None
            }
            _ => Outcome::None,
        }
    }

    /// Render the batch script viewer.
    pub fn render(&mut self, f: &mut Frame, area: Rect) {
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
        f.render_widget(block.clone(), dialog_area);

        // Inner layout
        let inner = block.inner(dialog_area);
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Length(1), Constraint::Min(0)])
            .split(inner);

        // Header
        let header = Paragraph::new(Line::from(vec![
            Span::styled(
                "batch script",
                Style::default().add_modifier(Modifier::BOLD),
            ),
            Span::raw("  job "),
            Span::raw(&self.job_id),
            Span::raw("  "),
            Span::styled("esc=close", Style::default().fg(Color::DarkGray)),
        ]))
        .style(Style::default().bg(Color::DarkGray));
        f.render_widget(header, chunks[0]);

        // Script content with resolved scroll position
        let content_height = chunks[1].height as usize;
        self.viewport_height = content_height;
        let offset = self.resolved_offset(content_height);

        let visible_lines: Vec<Line> = self
            .lines
            .iter()
            .skip(offset)
            .take(content_height)
            .map(|s| Line::from(Span::raw(s.as_str())))
            .collect();

        let content = Paragraph::new(visible_lines).wrap(Wrap { trim: false });
        f.render_widget(content, chunks[1]);
    }

    /// Get the script content for clipboard copy.
    pub fn content(&self) -> &str {
        &self.script
    }

    /// Get the label for clipboard copy.
    pub fn label(&self) -> String {
        format!("Batch Script job {}", self.job_id)
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
    fn test_scroll_down_increments_offset() {
        let mut screen =
            BatchScriptScreen::new("123".to_string(), "line1\nline2\nline3".to_string());
        // 3 lines, viewport 2 -> max scroll is 1
        assert_eq!(screen.resolved_offset(2), 0);

        screen.handle_key(KeyEvent::from(KeyCode::Down));
        assert_eq!(screen.resolved_offset(2), 1);

        screen.handle_key(KeyEvent::from(KeyCode::Down));
        // At(1) -> Down -> At(2), resolved to min(2, 1) = 1
        assert_eq!(screen.resolved_offset(2), 1);
    }

    #[test]
    fn test_scroll_up_decrements_offset() {
        let mut screen =
            BatchScriptScreen::new("123".to_string(), "line1\nline2\nline3".to_string());
        screen.scroll = Scroll::At(2);

        screen.handle_key(KeyEvent::from(KeyCode::Up));
        // At(2) -> Up -> At(1), resolved to min(1, 1) = 1
        assert_eq!(screen.resolved_offset(2), 1);

        screen.handle_key(KeyEvent::from(KeyCode::Up));
        // At(1) -> Up -> At(0), resolved to 0
        assert_eq!(screen.resolved_offset(2), 0);

        screen.handle_key(KeyEvent::from(KeyCode::Up));
        // At(0) saturating_sub stays at 0
        assert_eq!(screen.resolved_offset(2), 0);
    }

    #[test]
    fn test_scroll_up_at_top_stays_at_zero() {
        let mut screen = BatchScriptScreen::new("123".to_string(), "line1\nline2".to_string());
        assert_eq!(screen.resolved_offset(1), 0);

        screen.handle_key(KeyEvent::from(KeyCode::Up));
        assert_eq!(screen.resolved_offset(1), 0);
    }

    #[test]
    fn test_page_down_increments_by_10() {
        let mut screen = BatchScriptScreen::new("123".to_string(), "line\n".repeat(50));
        screen.handle_key(KeyEvent::from(KeyCode::PageDown));
        assert_eq!(screen.resolved_offset(10), 10);

        screen.handle_key(KeyEvent::from(KeyCode::PageDown));
        assert_eq!(screen.resolved_offset(10), 20);
    }

    #[test]
    fn test_page_up_decrements_by_10() {
        let mut screen = BatchScriptScreen::new("123".to_string(), "line\n".repeat(50));
        screen.scroll = Scroll::At(25);

        screen.handle_key(KeyEvent::from(KeyCode::PageUp));
        assert_eq!(screen.resolved_offset(10), 15);

        screen.handle_key(KeyEvent::from(KeyCode::PageUp));
        assert_eq!(screen.resolved_offset(10), 5);
    }

    #[test]
    fn test_home_jumps_to_top() {
        let mut screen = BatchScriptScreen::new("123".to_string(), "line\n".repeat(50));
        screen.scroll = Scroll::At(25);

        screen.handle_key(KeyEvent::from(KeyCode::Home));
        assert_eq!(screen.resolved_offset(10), 0);
    }

    #[test]
    fn test_end_sets_bottom() {
        let mut screen = BatchScriptScreen::new("123".to_string(), "line\n".repeat(20));
        screen.handle_key(KeyEvent::from(KeyCode::End));
        assert_eq!(screen.scroll, Scroll::Bottom);
    }

    #[test]
    fn test_bottom_resolves_to_keep_viewport_full() {
        let screen = BatchScriptScreen::new("123".to_string(), "line\n".repeat(20));
        assert_eq!(screen.resolved_offset(5), 0); // starts at top

        let mut screen2 = screen;
        screen2.scroll = Scroll::Bottom;
        // 20 lines, viewport 5 -> bottom resolves to 15
        assert_eq!(screen2.resolved_offset(5), 15);
    }

    #[test]
    fn test_bottom_on_empty_content_resolves_to_zero() {
        let mut screen = BatchScriptScreen::new("123".to_string(), "".to_string());
        screen.scroll = Scroll::Bottom;
        assert_eq!(screen.resolved_offset(10), 0);
    }

    #[test]
    fn test_bottom_on_short_content_resolves_to_zero() {
        let mut screen = BatchScriptScreen::new("123".to_string(), "a\nb\nc".to_string());
        screen.scroll = Scroll::Bottom;
        // 3 lines, viewport 10 -> saturating_sub gives 0
        assert_eq!(screen.resolved_offset(10), 0);
    }

    #[test]
    fn test_empty_content_does_not_panic() {
        let mut screen = BatchScriptScreen::new("123".to_string(), "".to_string());
        assert_eq!(screen.lines.len(), 0);

        screen.handle_key(KeyEvent::from(KeyCode::Down));
        screen.handle_key(KeyEvent::from(KeyCode::Up));
        screen.handle_key(KeyEvent::from(KeyCode::PageDown));
        assert_eq!(screen.resolved_offset(10), 0);
    }

    #[test]
    fn test_single_line_does_not_underflow() {
        let mut screen = BatchScriptScreen::new("123".to_string(), "single line".to_string());
        assert_eq!(screen.lines.len(), 1);

        screen.handle_key(KeyEvent::from(KeyCode::Down));
        screen.handle_key(KeyEvent::from(KeyCode::Down));
        screen.handle_key(KeyEvent::from(KeyCode::Up));
        screen.handle_key(KeyEvent::from(KeyCode::Up));
        screen.handle_key(KeyEvent::from(KeyCode::Up));
        assert_eq!(screen.resolved_offset(1), 0);
    }

    #[test]
    fn test_page_down_on_short_buffer_clamped_by_render() {
        let mut screen = BatchScriptScreen::new("123".to_string(), "a\nb\nc".to_string());
        assert_eq!(screen.lines.len(), 3);

        screen.handle_key(KeyEvent::from(KeyCode::PageDown));
        // At(10), but with viewport 5, max is 0 (3 - 5 saturates)
        assert_eq!(screen.resolved_offset(5), 0);
    }

    #[test]
    fn test_esc_returns_close() {
        let mut screen = BatchScriptScreen::new("123".to_string(), "content".to_string());
        let outcome = screen.handle_key(KeyEvent::from(KeyCode::Esc));
        assert_eq!(outcome, Outcome::Close);
    }

    #[test]
    fn test_q_returns_close() {
        let mut screen = BatchScriptScreen::new("123".to_string(), "content".to_string());
        let outcome = screen.handle_key(KeyEvent::from(KeyCode::Char('q')));
        assert_eq!(outcome, Outcome::Close);
    }

    #[test]
    fn test_resolved_offset_clamps_at_to_max() {
        let mut screen = BatchScriptScreen::new("123".to_string(), "line\n".repeat(20));
        screen.scroll = Scroll::At(100); // way past the end

        // 20 lines, viewport 5 -> max offset is 15
        assert_eq!(screen.resolved_offset(5), 15);
    }
}

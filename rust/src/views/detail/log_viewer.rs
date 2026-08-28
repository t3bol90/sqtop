//! Log viewer modal with auto-refresh.

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

/// Auto-refreshing log viewer.
pub struct LogViewerScreen {
    job_id: String,
    log_path: String,
    log_type: String, // "stdout" or "stderr"
    content: String,
    lines: Vec<String>,
    follow: bool,
    scroll: Scroll,
}

impl LogViewerScreen {
    /// Create a new log viewer.
    pub fn new(job_id: String, log_path: String, log_type: String, content: String) -> Self {
        let lines: Vec<String> = content.lines().map(|s| s.to_string()).collect();
        Self {
            job_id,
            log_path,
            log_type,
            content,
            lines,
            follow: true,
            scroll: Scroll::Bottom,
        }
    }

    /// Resolve the scroll position to a concrete offset given the viewport height.
    pub fn resolved_offset(&self, content_height: usize) -> usize {
        match self.scroll {
            Scroll::At(offset) => offset.min(self.lines.len().saturating_sub(content_height)),
            Scroll::Bottom => self.lines.len().saturating_sub(content_height),
        }
    }

    /// Update the log content.
    pub fn update_content(&mut self, content: String) {
        if content == self.content {
            return;
        }
        self.content = content;
        self.lines = self.content.lines().map(|s| s.to_string()).collect();

        // Auto-scroll to bottom when following
        if self.follow {
            self.scroll = Scroll::Bottom;
        }
    }

    /// Toggle follow mode.
    pub fn toggle_follow(&mut self) {
        self.follow = !self.follow;
        if self.follow {
            self.scroll = Scroll::Bottom;
        }
    }

    /// Handle key input.
    pub fn handle_key(&mut self, key: KeyEvent) -> Outcome {
        match key.code {
            KeyCode::Esc | KeyCode::Char('q') => Outcome::Close,
            KeyCode::Char('f') => {
                self.toggle_follow();
                Outcome::None
            }
            KeyCode::Up | KeyCode::Char('k') => {
                let current = match self.scroll {
                    Scroll::At(n) => n,
                    Scroll::Bottom => self.lines.len().saturating_sub(1),
                };
                self.scroll = Scroll::At(current.saturating_sub(1));
                self.follow = false;
                Outcome::None
            }
            KeyCode::Down | KeyCode::Char('j') => {
                let current = match self.scroll {
                    Scroll::At(n) => n,
                    Scroll::Bottom => self.lines.len().saturating_sub(1),
                };
                self.scroll = Scroll::At(current.saturating_add(1));
                self.follow = false;
                Outcome::None
            }
            KeyCode::PageUp => {
                let current = match self.scroll {
                    Scroll::At(n) => n,
                    Scroll::Bottom => self.lines.len().saturating_sub(1),
                };
                self.scroll = Scroll::At(current.saturating_sub(10));
                self.follow = false;
                Outcome::None
            }
            KeyCode::PageDown => {
                let current = match self.scroll {
                    Scroll::At(n) => n,
                    Scroll::Bottom => self.lines.len().saturating_sub(1),
                };
                self.scroll = Scroll::At(current.saturating_add(10));
                self.follow = false;
                Outcome::None
            }
            KeyCode::Home => {
                self.scroll = Scroll::At(0);
                self.follow = false;
                Outcome::None
            }
            KeyCode::End => {
                self.scroll = Scroll::Bottom;
                self.follow = true;
                Outcome::None
            }
            _ => Outcome::None,
        }
    }

    /// Render the log viewer.
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
        f.render_widget(block.clone(), dialog_area);

        // Inner layout
        let inner = block.inner(dialog_area);
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Length(1), Constraint::Min(0)])
            .split(inner);

        // Header
        let follow_status = if self.follow {
            Span::styled("following", Style::default().fg(Color::Green))
        } else {
            Span::styled("paused", Style::default().fg(Color::DarkGray))
        };

        let header = Paragraph::new(Line::from(vec![
            Span::styled(
                &self.log_type,
                Style::default().add_modifier(Modifier::BOLD),
            ),
            Span::raw("  "),
            Span::raw(&self.log_path),
            Span::raw("  "),
            follow_status,
            Span::raw("  "),
            Span::styled(
                "esc=close  f=toggle follow",
                Style::default().fg(Color::DarkGray),
            ),
        ]))
        .style(Style::default().bg(Color::DarkGray));
        f.render_widget(header, chunks[0]);

        // Log content with resolved scroll position
        let content_height = chunks[1].height as usize;
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

    /// Get the content for clipboard copy.
    pub fn content(&self) -> &str {
        &self.content
    }

    /// Get the label for clipboard copy.
    pub fn label(&self) -> String {
        format!("Log {} job {}", self.log_type, self.job_id)
    }

    /// Check if follow mode is enabled.
    pub fn is_following(&self) -> bool {
        self.follow
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
    fn test_new_starts_in_follow_mode() {
        let screen = LogViewerScreen::new(
            "123".to_string(),
            "/path/to/log".to_string(),
            "stdout".to_string(),
            "line1\nline2".to_string(),
        );
        assert!(screen.is_following());
    }

    #[test]
    fn test_new_starts_at_bottom() {
        let screen = LogViewerScreen::new(
            "123".to_string(),
            "/path/to/log".to_string(),
            "stdout".to_string(),
            "line1\nline2".to_string(),
        );
        assert_eq!(screen.scroll, Scroll::Bottom);
    }

    #[test]
    fn test_bottom_resolves_to_keep_viewport_full() {
        let mut screen = LogViewerScreen::new(
            "123".to_string(),
            "/path/to/log".to_string(),
            "stdout".to_string(),
            "line\n".repeat(20),
        );
        screen.scroll = Scroll::Bottom;

        // 20 lines, viewport 5 -> bottom should resolve to 15
        assert_eq!(screen.resolved_offset(5), 15);
    }

    #[test]
    fn test_bottom_on_empty_content_resolves_to_zero() {
        let mut screen = LogViewerScreen::new(
            "123".to_string(),
            "/path/to/log".to_string(),
            "stdout".to_string(),
            "".to_string(),
        );
        screen.scroll = Scroll::Bottom;
        assert_eq!(screen.resolved_offset(10), 0);
    }

    #[test]
    fn test_update_content_in_follow_mode_scrolls_to_bottom() {
        let mut screen = LogViewerScreen::new(
            "123".to_string(),
            "/path/to/log".to_string(),
            "stdout".to_string(),
            "line1".to_string(),
        );
        assert!(screen.is_following());

        screen.update_content("line1\nline2\nline3".to_string());
        assert!(screen.is_following());
        assert_eq!(screen.scroll, Scroll::Bottom);
    }

    #[test]
    fn test_update_content_when_not_following_leaves_offset() {
        let mut screen = LogViewerScreen::new(
            "123".to_string(),
            "/path/to/log".to_string(),
            "stdout".to_string(),
            "line1\nline2".to_string(),
        );
        screen.scroll = Scroll::At(0);
        screen.follow = false;

        screen.update_content("line1\nline2\nline3".to_string());
        assert!(!screen.is_following());
        assert_eq!(screen.scroll, Scroll::At(0)); // Should stay where user put it
    }

    #[test]
    fn test_toggle_follow_when_off_scrolls_to_bottom() {
        let mut screen = LogViewerScreen::new(
            "123".to_string(),
            "/path/to/log".to_string(),
            "stdout".to_string(),
            "line1\nline2".to_string(),
        );
        screen.scroll = Scroll::At(0);
        screen.follow = false;

        screen.toggle_follow();
        assert!(screen.is_following());
        assert_eq!(screen.scroll, Scroll::Bottom);
    }

    #[test]
    fn test_toggle_follow_when_on_stays_at_current_offset() {
        let mut screen = LogViewerScreen::new(
            "123".to_string(),
            "/path/to/log".to_string(),
            "stdout".to_string(),
            "line1\nline2".to_string(),
        );
        screen.scroll = Scroll::Bottom;
        screen.follow = true;

        screen.toggle_follow();
        assert!(!screen.is_following());
        // scroll stays Bottom, just follow flag changes
        assert_eq!(screen.scroll, Scroll::Bottom);
    }

    #[test]
    fn test_scroll_up_disables_follow() {
        let mut screen = LogViewerScreen::new(
            "123".to_string(),
            "/path/to/log".to_string(),
            "stdout".to_string(),
            "line1\nline2\nline3".to_string(),
        );
        assert!(screen.is_following());

        screen.handle_key(KeyEvent::from(KeyCode::Up));
        assert!(!screen.is_following());
    }

    #[test]
    fn test_scroll_down_disables_follow() {
        let mut screen = LogViewerScreen::new(
            "123".to_string(),
            "/path/to/log".to_string(),
            "stdout".to_string(),
            "line1\nline2".to_string(),
        );
        screen.scroll = Scroll::At(0);

        screen.handle_key(KeyEvent::from(KeyCode::Down));
        assert!(!screen.is_following());
    }

    #[test]
    fn test_end_key_enables_follow() {
        let mut screen = LogViewerScreen::new(
            "123".to_string(),
            "/path/to/log".to_string(),
            "stdout".to_string(),
            "line1\nline2".to_string(),
        );
        screen.scroll = Scroll::At(0);
        screen.follow = false;

        screen.handle_key(KeyEvent::from(KeyCode::End));
        assert!(screen.is_following());
        assert_eq!(screen.scroll, Scroll::Bottom);
    }

    #[test]
    fn test_home_key_disables_follow() {
        let mut screen = LogViewerScreen::new(
            "123".to_string(),
            "/path/to/log".to_string(),
            "stdout".to_string(),
            "line1\nline2".to_string(),
        );

        screen.handle_key(KeyEvent::from(KeyCode::Home));
        assert!(!screen.is_following());
        assert_eq!(screen.scroll, Scroll::At(0));
    }

    #[test]
    fn test_page_up_disables_follow() {
        let mut screen = LogViewerScreen::new(
            "123".to_string(),
            "/path/to/log".to_string(),
            "stdout".to_string(),
            "line\n".repeat(50),
        );

        screen.handle_key(KeyEvent::from(KeyCode::PageUp));
        assert!(!screen.is_following());
    }

    #[test]
    fn test_page_down_disables_follow() {
        let mut screen = LogViewerScreen::new(
            "123".to_string(),
            "/path/to/log".to_string(),
            "stdout".to_string(),
            "line\n".repeat(50),
        );
        screen.scroll = Scroll::At(0);

        screen.handle_key(KeyEvent::from(KeyCode::PageDown));
        assert!(!screen.is_following());
    }

    #[test]
    fn test_f_key_toggles_follow() {
        let mut screen = LogViewerScreen::new(
            "123".to_string(),
            "/path/to/log".to_string(),
            "stdout".to_string(),
            "line1".to_string(),
        );
        let initial = screen.is_following();

        screen.handle_key(KeyEvent::from(KeyCode::Char('f')));
        assert_eq!(screen.is_following(), !initial);

        screen.handle_key(KeyEvent::from(KeyCode::Char('f')));
        assert_eq!(screen.is_following(), initial);
    }

    #[test]
    fn test_scroll_up_at_top_stays_at_zero() {
        let mut screen = LogViewerScreen::new(
            "123".to_string(),
            "/path/to/log".to_string(),
            "stdout".to_string(),
            "line1\nline2".to_string(),
        );
        screen.scroll = Scroll::At(0);

        screen.handle_key(KeyEvent::from(KeyCode::Up));
        assert_eq!(screen.resolved_offset(10), 0);
    }

    #[test]
    fn test_empty_content_does_not_panic() {
        let mut screen = LogViewerScreen::new(
            "123".to_string(),
            "/path/to/log".to_string(),
            "stdout".to_string(),
            "".to_string(),
        );
        assert_eq!(screen.lines.len(), 0);

        screen.handle_key(KeyEvent::from(KeyCode::Down));
        screen.handle_key(KeyEvent::from(KeyCode::Up));
        screen.handle_key(KeyEvent::from(KeyCode::PageDown));
        // Should not panic
        assert_eq!(screen.resolved_offset(10), 0);
    }

    #[test]
    fn test_page_down_on_short_buffer_is_safe() {
        let mut screen = LogViewerScreen::new(
            "123".to_string(),
            "/path/to/log".to_string(),
            "stdout".to_string(),
            "a\nb\nc".to_string(),
        );
        screen.scroll = Scroll::At(0);

        screen.handle_key(KeyEvent::from(KeyCode::PageDown));
        // Offset goes to At(10), but render with viewport 5 clamps to 0 (3 - 5 saturates)
        assert_eq!(screen.resolved_offset(5), 0);
    }

    #[test]
    fn test_update_content_with_same_content_does_not_reset() {
        let mut screen = LogViewerScreen::new(
            "123".to_string(),
            "/path/to/log".to_string(),
            "stdout".to_string(),
            "line1\nline2".to_string(),
        );
        screen.scroll = Scroll::At(0);
        screen.follow = false;

        screen.update_content("line1\nline2".to_string());
        assert_eq!(screen.scroll, Scroll::At(0));
        assert!(!screen.is_following());
    }

    #[test]
    fn test_resolved_offset_clamps_at_to_max() {
        let mut screen = LogViewerScreen::new(
            "123".to_string(),
            "/path/to/log".to_string(),
            "stdout".to_string(),
            "line\n".repeat(20),
        );
        screen.scroll = Scroll::At(100); // way past the end

        // 20 lines, viewport 5 -> max offset is 15
        assert_eq!(screen.resolved_offset(5), 15);
    }
}

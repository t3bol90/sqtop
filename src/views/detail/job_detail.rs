//! Job detail modal with efficiency bars.

use crate::slurm::fetch::JobEfficiency;
use crate::views::detail::Outcome;
use crossterm::event::{KeyCode, KeyEvent};
use ratatui::{
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph, Wrap},
    Frame,
};
use std::collections::HashMap;

/// Job detail viewer with efficiency bars for terminal-state jobs.
pub struct JobDetailScreen {
    job_id: String,
    data: HashMap<String, String>,
    efficiency: Option<JobEfficiency>,
    scroll_offset: usize,
    lines: Vec<String>,
}

impl JobDetailScreen {
    /// Create a new job detail screen.
    pub fn new(job_id: String, fields: Vec<(String, String)>) -> Self {
        let lines = build_detail_lines(&fields);
        let data: HashMap<String, String> = fields.iter().cloned().collect();
        Self {
            job_id,
            data,
            efficiency: None,
            scroll_offset: 0,
            lines,
        }
    }

    /// Set the efficiency data.
    pub fn set_efficiency(&mut self, eff: JobEfficiency) {
        self.efficiency = Some(eff);
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

    /// Render the job detail screen.
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

        // Calculate constraints based on efficiency availability
        // Decide ONCE whether the efficiency band is drawn, so the layout and
        // the render agree. Reserving the slot but not drawing into it pushed
        // the detail text into the 3-row efficiency chunk, which truncated it
        // to about three visible lines.
        let show_eff = self
            .efficiency
            .as_ref()
            .is_some_and(|eff| eff.available && (eff.mem_peak_mb > 0 || eff.cpu_eff > 0.0));

        let mut constraints = vec![Constraint::Length(1)]; // title
        if show_eff {
            constraints.push(Constraint::Length(3)); // efficiency bars
        }
        constraints.push(Constraint::Min(0)); // detail content

        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints(constraints)
            .split(inner);

        // Title
        let name = self.data.get("JobName").map(|s| s.as_str()).unwrap_or("");
        let header_text = if name.is_empty() {
            format!("Job {}", self.job_id)
        } else {
            format!("Job {} \u{2014} {}", self.job_id, name)
        };
        let title = Paragraph::new(Span::styled(
            header_text,
            Style::default()
                .fg(Color::Black)
                .bg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        ))
        .alignment(Alignment::Center);
        f.render_widget(title, chunks[0]);

        if show_eff {
            if let Some(eff) = &self.efficiency {
                let eff_para = Paragraph::new(build_efficiency_text(eff));
                f.render_widget(eff_para, chunks[1]);
            }
        }

        // Content is always the last chunk.
        let content_chunk_idx = chunks.len() - 1;

        // Detail content
        let content_height = chunks[content_chunk_idx].height as usize;
        let visible_lines: Vec<Line> = self
            .lines
            .iter()
            .skip(self.scroll_offset)
            .take(content_height)
            .map(|s| Line::from(Span::raw(s.as_str())))
            .collect();

        let content = Paragraph::new(visible_lines).wrap(Wrap { trim: false });
        f.render_widget(content, chunks[content_chunk_idx]);
    }

    /// Get the plain text content for clipboard copy.
    pub fn plain_text(&self) -> String {
        self.lines.join("\n")
    }

    /// Get the label for clipboard copy.
    pub fn label(&self) -> String {
        format!("Job {} Detail", self.job_id)
    }
}

fn build_detail_lines(fields: &[(String, String)]) -> Vec<String> {
    // Render in scontrol's own order, like the Python version, which iterated
    // an insertion-ordered dict. Sorting or grouping here would shuffle fields
    // between refreshes and hide the grouping scontrol already provides.
    let mut lines = vec!["Job Detail\n".to_string()];
    for (k, v) in fields {
        lines.push(format!("  {}: {}", k, v));
    }
    lines
}

fn build_efficiency_text(eff: &JobEfficiency) -> Vec<Line<'_>> {
    let cpu_bar = eff_bar(eff.cpu_eff);
    let mem_bar = eff_bar(eff.mem_eff);

    vec![
        Line::from(vec![
            Span::styled(
                "  CPU efficiency:  ",
                Style::default().add_modifier(Modifier::BOLD),
            ),
            cpu_bar,
            Span::styled(
                format!(
                    "   (used {} of {} CPU-time)",
                    eff.cpu_used_str, eff.cpu_alloc_str
                ),
                Style::default().fg(Color::DarkGray),
            ),
        ]),
        Line::from(vec![
            Span::styled(
                "  Mem efficiency:  ",
                Style::default().add_modifier(Modifier::BOLD),
            ),
            mem_bar,
            Span::styled(
                format!(
                    "   (peak {} MB of {} MB allocated)",
                    eff.mem_peak_mb, eff.mem_alloc_mb
                ),
                Style::default().fg(Color::DarkGray),
            ),
        ]),
    ]
}

fn eff_bar(fraction: f64) -> Span<'static> {
    const BAR_WIDTH: usize = 10;
    let pct = (fraction.clamp(0.0, 1.0) * 100.0).round() as u32;
    let filled = ((pct as f64 / 100.0) * BAR_WIDTH as f64).round() as usize;
    let bar = "█".repeat(filled) + &"░".repeat(BAR_WIDTH - filled);

    let color = if pct >= 70 {
        Color::Green
    } else if pct >= 40 {
        Color::Yellow
    } else {
        Color::Red
    };

    Span::styled(format!("{}  {:3}%", bar, pct), Style::default().fg(color))
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
    fn test_build_detail_lines_preserves_scontrol_order() {
        let fields = vec![
            ("JobId".to_string(), "12345".to_string()),
            ("JobName".to_string(), "test_job".to_string()),
            ("JobState".to_string(), "RUNNING".to_string()),
        ];

        let lines = build_detail_lines(&fields);
        assert_eq!(lines[1], "  JobId: 12345");
        assert_eq!(lines[2], "  JobName: test_job");
        assert_eq!(lines[3], "  JobState: RUNNING");
    }

    #[test]
    fn test_eff_bar_green_above_70() {
        let span = eff_bar(0.75);
        // The bar should contain 8 filled chars and 2 empty for 75%
        assert!(span.content.contains("██████"));
    }

    #[test]
    fn test_eff_bar_red_below_40() {
        let span = eff_bar(0.25);
        // The bar should contain 3 filled chars and 7 empty for 25%
        assert!(span.content.contains("███░"));
    }

    #[test]
    fn test_set_efficiency_updates_screen_state() {
        use crate::slurm::fetch::JobEfficiency;
        let mut screen = JobDetailScreen::new("12345".to_string(), Vec::new());

        let eff = JobEfficiency {
            available: true,
            cpu_eff: 0.85,
            mem_eff: 0.65,
            cpu_used_str: "1:23:45".to_string(),
            cpu_alloc_str: "1:38:40".to_string(),
            mem_peak_mb: 650,
            mem_alloc_mb: 1000,
        };

        screen.set_efficiency(eff.clone());
        assert!(screen.efficiency.is_some());
        assert_eq!(screen.efficiency.as_ref().unwrap().cpu_eff, 0.85);
        assert_eq!(screen.efficiency.as_ref().unwrap().mem_eff, 0.65);
    }
}

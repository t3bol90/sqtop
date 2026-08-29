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
    pub fn new(job_id: String, data: HashMap<String, String>) -> Self {
        let lines = build_detail_lines(&job_id, &data);
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
        f.render_widget(block.clone(), dialog_area);

        // Inner layout
        let inner = block.inner(dialog_area);

        // Calculate constraints based on efficiency availability
        let mut constraints = vec![Constraint::Length(1)]; // title
        if self.efficiency.is_some() {
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
            format!("Job {} — {}", self.job_id, name)
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

        // Efficiency bars (if available)
        let content_chunk_idx = if let Some(eff) = &self.efficiency {
            if eff.available && (eff.mem_peak_mb > 0 || eff.cpu_eff > 0.0) {
                let eff_text = build_efficiency_text(eff);
                let eff_para = Paragraph::new(eff_text);
                f.render_widget(eff_para, chunks[1]);
                2
            } else {
                1
            }
        } else {
            1
        };

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

fn build_detail_lines(_job_id: &str, data: &HashMap<String, String>) -> Vec<String> {
    let mut lines = vec!["Job Detail\n".to_string()];

    // Highlight keys (shown first)
    let highlight_keys = [
        "JobId",
        "JobName",
        "UserId",
        "JobState",
        "NumNodes",
        "NumCPUs",
        "TimeLimit",
        "SubmitTime",
        "StartTime",
        "EndTime",
        "Partition",
        "NodeList",
        "Reason",
        "Priority",
    ];

    for key in &highlight_keys {
        if let Some(value) = data.get(*key) {
            lines.push(format!("  {}: {}", key, value));
        }
    }

    // Other keys
    for (k, v) in data {
        if !highlight_keys.contains(&k.as_str()) {
            lines.push(format!("  {}: {}", k, v));
        }
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
    fn test_build_detail_lines_includes_highlight_keys() {
        let mut data = HashMap::new();
        data.insert("JobId".to_string(), "12345".to_string());
        data.insert("JobName".to_string(), "test_job".to_string());
        data.insert("JobState".to_string(), "RUNNING".to_string());

        let lines = build_detail_lines("12345", &data);
        assert!(lines.iter().any(|l| l.contains("JobId: 12345")));
        assert!(lines.iter().any(|l| l.contains("JobName: test_job")));
        assert!(lines.iter().any(|l| l.contains("JobState: RUNNING")));
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
        let data = HashMap::new();
        let mut screen = JobDetailScreen::new("12345".to_string(), data);

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

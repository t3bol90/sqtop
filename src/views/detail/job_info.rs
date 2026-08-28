//! Job info popup with rich formatting.

use crate::slurm::fetch::JobDependency;
use crate::slurm::model::Job;
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

/// Rich job information viewer.
pub struct JobInfoScreen {
    job: Job,
    scroll_offset: usize,
    lines: Vec<Line<'static>>,
    plain_text: String,
}

impl JobInfoScreen {
    /// Create a new job info screen.
    pub fn new(job: Job, detail: HashMap<String, String>, deps: Vec<JobDependency>) -> Self {
        let (lines, plain_text) = build_info_content(&job, &detail, &deps);
        Self {
            job,
            scroll_offset: 0,
            lines,
            plain_text,
        }
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

    /// Render the job info screen.
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

        // Title
        let header_text = if self.job.name.is_empty() {
            format!("Job {}", self.job.job_id)
        } else {
            format!("Job {} — {}", self.job.job_id, self.job.name)
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

        // Content
        let content_height = chunks[1].height as usize;
        let visible_lines: Vec<Line> = self
            .lines
            .iter()
            .skip(self.scroll_offset)
            .take(content_height)
            .cloned()
            .collect();

        let content = Paragraph::new(visible_lines).wrap(Wrap { trim: false });
        f.render_widget(content, chunks[1]);
    }

    /// Get the plain text content for clipboard copy.
    pub fn plain_text(&self) -> &str {
        &self.plain_text
    }

    /// Get the label for clipboard copy.
    pub fn label(&self) -> String {
        format!("Job {} Info", self.job.job_id)
    }
}

fn build_info_content(
    job: &Job,
    detail: &HashMap<String, String>,
    deps: &[JobDependency],
) -> (Vec<Line<'static>>, String) {
    let mut lines = Vec::new();
    let mut plain = Vec::new();

    // Identity section
    lines.push(Line::from(Span::styled(
        "── Identity ──────────────────────────────".to_string(),
        Style::default()
            .fg(Color::Cyan)
            .add_modifier(Modifier::BOLD),
    )));
    plain.push("── Identity ──────────────────────────────".to_string());

    add_field(&mut lines, &mut plain, "Job ID:", &job.job_id);
    add_field(
        &mut lines,
        &mut plain,
        "Name:",
        if job.name.is_empty() {
            "(none)"
        } else {
            &job.name
        },
    );
    add_field(&mut lines, &mut plain, "User:", &job.user);
    add_field(&mut lines, &mut plain, "Partition:", &job.partition);

    let state_color = state_color(&job.state);
    lines.push(Line::from(vec![
        Span::raw("  ".to_string()),
        Span::styled(
            "State:".to_string(),
            Style::default().add_modifier(Modifier::BOLD),
        ),
        Span::raw("      ".to_string()),
        Span::styled(job.state.clone(), Style::default().fg(state_color)),
    ]));
    plain.push(format!("  State:      {}", job.state));

    lines.push(Line::from("".to_string()));
    plain.push("".to_string());

    // Reason section
    let reason = detail
        .get("Reason")
        .map(|s| s.as_str())
        .unwrap_or_else(|| job.reason.as_str());
    let reason = if reason.is_empty() || reason == "None" {
        "(none)"
    } else {
        reason
    };
    let reason_color = if reason == "(none)" {
        Color::DarkGray
    } else {
        Color::Yellow
    };

    lines.push(Line::from(Span::styled(
        "── Reason ────────────────────────────────".to_string(),
        Style::default()
            .fg(Color::Cyan)
            .add_modifier(Modifier::BOLD),
    )));
    plain.push("── Reason ────────────────────────────────".to_string());

    lines.push(Line::from(vec![
        Span::raw("  ".to_string()),
        Span::styled(reason.to_string(), Style::default().fg(reason_color)),
    ]));
    plain.push(format!("  {}", reason));

    lines.push(Line::from("".to_string()));
    plain.push("".to_string());

    // Timing section
    if let Some(submit_time) = detail.get("SubmitTime") {
        if !submit_time.is_empty() {
            lines.push(Line::from(Span::styled(
                "── Timing ────────────────────────────────".to_string(),
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            )));
            plain.push("── Timing ────────────────────────────────".to_string());

            add_field(&mut lines, &mut plain, "Submitted:", submit_time);

            if let Some(start_time) = detail.get("StartTime") {
                if !start_time.is_empty() && start_time != "N/A" && start_time != "Unknown" {
                    add_field(&mut lines, &mut plain, "Started:", start_time);
                }
            }

            if let Some(end_time) = detail.get("EndTime") {
                if !end_time.is_empty() && end_time != "N/A" && end_time != "Unknown" {
                    add_field(&mut lines, &mut plain, "End:", end_time);
                }
            }

            let time_used = detail
                .get("RunTime")
                .map(|s| s.as_str())
                .unwrap_or_else(|| job.time_used.as_str());
            if !time_used.is_empty() {
                add_field(&mut lines, &mut plain, "Time used:", time_used);
            }

            let time_limit = detail
                .get("TimeLimit")
                .map(|s| s.as_str())
                .unwrap_or_else(|| job.time_limit.as_str());
            if !time_limit.is_empty() {
                add_field(&mut lines, &mut plain, "Time limit:", time_limit);
            }

            lines.push(Line::from("".to_string()));
            plain.push("".to_string());
        }
    }

    // Resources section
    lines.push(Line::from(Span::styled(
        "── Resources ─────────────────────────────".to_string(),
        Style::default()
            .fg(Color::Cyan)
            .add_modifier(Modifier::BOLD),
    )));
    plain.push("── Resources ─────────────────────────────".to_string());

    let num_nodes = detail
        .get("NumNodes")
        .map(|s| s.as_str())
        .unwrap_or_else(|| job.num_nodes.as_str());
    let num_cpus = detail
        .get("NumCPUs")
        .map(|s| s.as_str())
        .unwrap_or_else(|| job.num_cpus.as_str());

    add_field(&mut lines, &mut plain, "Nodes:", num_nodes);
    add_field(&mut lines, &mut plain, "CPUs:", num_cpus);

    if let Some(mem) = detail.get("MinMemoryNode").or_else(|| detail.get("mem")) {
        if !mem.is_empty() {
            add_field(&mut lines, &mut plain, "Memory:", mem);
        }
    }

    let nodelist = detail
        .get("NodeList")
        .map(|s| s.as_str())
        .unwrap_or_else(|| job.nodelist.as_str());
    if !nodelist.is_empty() && nodelist != "(null)" && nodelist != "N/A" {
        add_field(&mut lines, &mut plain, "Nodelist:", nodelist);
    }

    if let Some(tres) = detail.get("TRES") {
        if !tres.is_empty() {
            add_field(&mut lines, &mut plain, "TRES:", tres);
        }
    }

    lines.push(Line::from("".to_string()));
    plain.push("".to_string());

    // Paths section
    let work_dir = detail.get("WorkDir").map(|s| s.as_str()).unwrap_or("");
    let command = detail.get("Command").map(|s| s.as_str()).unwrap_or("");
    let stdout = detail.get("StdOut").map(|s| s.as_str()).unwrap_or("");
    let stderr = detail.get("StdErr").map(|s| s.as_str()).unwrap_or("");

    if !work_dir.is_empty() || !command.is_empty() || !stdout.is_empty() || !stderr.is_empty() {
        lines.push(Line::from(Span::styled(
            "── Paths ──────────────────────────────────".to_string(),
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        )));
        plain.push("── Paths ──────────────────────────────────".to_string());

        if !work_dir.is_empty() {
            add_field(&mut lines, &mut plain, "WorkDir:", work_dir);
        }
        if !command.is_empty() {
            add_field(&mut lines, &mut plain, "Script:", command);
        }
        if !stdout.is_empty() {
            add_field(&mut lines, &mut plain, "StdOut:", stdout);
        }
        if !stderr.is_empty() {
            add_field(&mut lines, &mut plain, "StdErr:", stderr);
        }

        lines.push(Line::from("".to_string()));
        plain.push("".to_string());
    }

    // Dependencies section
    if !deps.is_empty() {
        lines.push(Line::from(Span::styled(
            "── Dependencies ──────────────────────────".to_string(),
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        )));
        plain.push("── Dependencies ──────────────────────────".to_string());

        for dep in deps {
            let dep_color = if dep.state == "COMPLETED" {
                Color::Green
            } else {
                Color::Yellow
            };
            lines.push(Line::from(vec![
                Span::raw("  ".to_string()),
                Span::styled(
                    format!("{}:{}  [{}]", dep.dep_type, dep.job_id, dep.state),
                    Style::default().fg(dep_color),
                ),
            ]));
            plain.push(format!(
                "  {}:{}  [{}]",
                dep.dep_type, dep.job_id, dep.state
            ));
        }

        lines.push(Line::from("".to_string()));
        plain.push("".to_string());
    } else if let Some(dep_str) = detail.get("Dependency") {
        if !dep_str.is_empty() && dep_str != "None" && dep_str != "(null)" {
            lines.push(Line::from(Span::styled(
                "── Dependencies ──────────────────────────".to_string(),
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            )));
            plain.push("── Dependencies ──────────────────────────".to_string());

            lines.push(Line::from(vec![
                Span::raw("  ".to_string()),
                Span::styled(dep_str.clone(), Style::default().fg(Color::DarkGray)),
            ]));
            plain.push(format!("  {}", dep_str));

            lines.push(Line::from("".to_string()));
            plain.push("".to_string());
        }
    }

    // Footer
    lines.push(Line::from(Span::styled(
        "  Press q or Esc to close".to_string(),
        Style::default().fg(Color::DarkGray),
    )));
    plain.push("  Press q or Esc to close".to_string());

    (lines, plain.join("\n"))
}

fn add_field(lines: &mut Vec<Line<'static>>, plain: &mut Vec<String>, label: &str, value: &str) {
    lines.push(Line::from(vec![
        Span::raw("  ".to_string()),
        Span::styled(
            format!("{:14}", label),
            Style::default().add_modifier(Modifier::BOLD),
        ),
        Span::raw(value.to_string()),
    ]));
    plain.push(format!("  {:14} {}", label, value));
}

fn state_color(state: &str) -> Color {
    match state.to_uppercase().as_str() {
        "RUNNING" => Color::Green,
        "PENDING" => Color::Yellow,
        "FAILED" | "CANCELLED" | "NODE_FAIL" => Color::Red,
        "COMPLETED" => Color::DarkGray,
        "TIMEOUT" => Color::Magenta,
        "PREEMPTED" => Color::Yellow,
        "COMPLETING" => Color::Cyan,
        _ => Color::White,
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
    fn test_build_info_content_includes_identity_section() {
        let job = Job {
            job_id: "12345".to_string(),
            name: "test_job".to_string(),
            user: "alice".to_string(),
            state: "RUNNING".to_string(),
            partition: "gpu".to_string(),
            ..Default::default()
        };
        let detail = HashMap::new();
        let deps = Vec::new();

        let (_, plain) = build_info_content(&job, &detail, &deps);
        assert!(plain.contains("── Identity"));
        assert!(plain.contains("Job ID:"));
        assert!(plain.contains("12345"));
        assert!(plain.contains("test_job"));
    }

    #[test]
    fn test_state_color_green_for_running() {
        assert_eq!(state_color("RUNNING"), Color::Green);
    }

    #[test]
    fn test_state_color_red_for_failed() {
        assert_eq!(state_color("FAILED"), Color::Red);
    }
}

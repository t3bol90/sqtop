//! Array task expansion modal.

use crate::slurm::model::Job;
use crate::views::detail::Outcome;
use crate::views::table_state::CyclicTableState;
use crossterm::event::{KeyCode, KeyEvent};
use ratatui::{
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Cell, Paragraph, Row, Table},
    Frame,
};

/// Array task viewer showing individual tasks of a job array.
pub struct ArrayTaskScreen {
    job: Job,
    tasks: Vec<Job>,
    table_state: CyclicTableState,
}

impl ArrayTaskScreen {
    /// Create a new array task screen.
    pub fn new(job: Job, tasks: Vec<Job>) -> Self {
        let mut table_state = CyclicTableState::new();
        table_state.set_row_count(tasks.len());
        Self {
            job,
            tasks,
            table_state,
        }
    }

    /// Handle key input.
    pub fn handle_key(&mut self, key: KeyEvent) -> Outcome {
        match key.code {
            KeyCode::Esc | KeyCode::Char('q') => Outcome::Close,
            KeyCode::Down | KeyCode::Char('j') => {
                self.table_state.next();
                Outcome::None
            }
            KeyCode::Up | KeyCode::Char('k') => {
                self.table_state.prev();
                Outcome::None
            }
            _ => Outcome::None,
        }
    }

    /// Render the array task screen.
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
            .constraints([
                Constraint::Length(1), // title
                Constraint::Length(1), // status
                Constraint::Min(0),    // table
            ])
            .split(inner);

        // Title
        let header_text = format!("Array {} — {}", self.job.job_id, self.job.name);
        let title = Paragraph::new(Span::styled(
            header_text,
            Style::default()
                .fg(Color::Black)
                .bg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        ))
        .alignment(Alignment::Center);
        f.render_widget(title, chunks[0]);

        // Status line
        let (running, pending, done) = count_task_states(&self.tasks);
        let status = Line::from(vec![
            Span::styled(
                format!("{} running", running),
                Style::default().fg(Color::Green),
            ),
            Span::raw("  "),
            Span::styled(
                format!("{} pending", pending),
                Style::default().fg(Color::Yellow),
            ),
            Span::raw("  "),
            Span::styled(
                format!("{} done  {} total", done, self.tasks.len()),
                Style::default().fg(Color::DarkGray),
            ),
        ]);
        let status_para = Paragraph::new(status);
        f.render_widget(status_para, chunks[1]);

        // Table
        let header = Row::new(vec!["TASK_ID", "STATE", "TIME", "NODELIST"])
            .style(Style::default().add_modifier(Modifier::BOLD))
            .bottom_margin(0);

        let rows: Vec<Row> = self
            .tasks
            .iter()
            .map(|task| {
                let task_id = extract_task_id(&task.job_id);
                let color = state_color(&task.state);
                let nodelist = if task.nodelist.is_empty() {
                    &task.reason
                } else {
                    &task.nodelist
                };

                Row::new(vec![
                    Cell::from(Span::styled(task_id, Style::default().fg(color))),
                    Cell::from(Span::styled(&task.state, Style::default().fg(color))),
                    Cell::from(task.time_used.clone()),
                    Cell::from(nodelist.clone()),
                ])
            })
            .collect();

        let table = Table::new(
            rows,
            [
                Constraint::Length(12),
                Constraint::Length(12),
                Constraint::Length(12),
                Constraint::Min(24),
            ],
        )
        .header(header)
        .row_highlight_style(Style::default().bg(Color::DarkGray));

        // Convert CyclicTableState to ratatui TableState
        let mut ratatui_state = ratatui::widgets::TableState::default();
        ratatui_state.select(self.table_state.selected());

        f.render_stateful_widget(table, chunks[2], &mut ratatui_state);
    }
}

fn extract_task_id(job_id: &str) -> String {
    if let Some((_base, task)) = job_id.split_once('_') {
        task.to_string()
    } else {
        job_id.to_string()
    }
}

fn count_task_states(tasks: &[Job]) -> (usize, usize, usize) {
    let running = tasks.iter().filter(|t| t.state == "RUNNING").count();
    let pending = tasks.iter().filter(|t| t.state == "PENDING").count();
    let terminal_states = [
        "COMPLETED",
        "FAILED",
        "CANCELLED",
        "TIMEOUT",
        "NODE_FAIL",
        "PREEMPTED",
    ];
    let done = tasks
        .iter()
        .filter(|t| terminal_states.contains(&t.state.as_str()))
        .count();
    (running, pending, done)
}

fn state_color(state: &str) -> Color {
    match state.to_uppercase().as_str() {
        "RUNNING" => Color::Green,
        "PENDING" => Color::Yellow,
        "FAILED" | "CANCELLED" | "NODE_FAIL" => Color::Red,
        "COMPLETED" => Color::DarkGray,
        "TIMEOUT" => Color::Magenta,
        "PREEMPTED" => Color::Yellow,
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
    fn test_extract_task_id_with_underscore() {
        assert_eq!(extract_task_id("12345_3"), "3");
    }

    #[test]
    fn test_extract_task_id_without_underscore() {
        assert_eq!(extract_task_id("12345"), "12345");
    }

    #[test]
    fn test_count_task_states() {
        let tasks = vec![
            Job {
                state: "RUNNING".to_string(),
                ..Default::default()
            },
            Job {
                state: "PENDING".to_string(),
                ..Default::default()
            },
            Job {
                state: "COMPLETED".to_string(),
                ..Default::default()
            },
            Job {
                state: "FAILED".to_string(),
                ..Default::default()
            },
        ];

        let (running, pending, done) = count_task_states(&tasks);
        assert_eq!(running, 1);
        assert_eq!(pending, 1);
        assert_eq!(done, 2);
    }
}

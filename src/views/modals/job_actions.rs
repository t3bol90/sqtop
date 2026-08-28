//! Job actions modal — inspect logs, view details, cancel.

use super::{centered_rect, ModalOutcome};
use crate::config::Config;
use crate::slurm::model::Job;
use crossterm::event::{KeyCode, KeyEvent};
use ratatui::layout::{Constraint, Direction, Layout};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph};
use ratatui::Frame;

/// Action selected from the job actions modal.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum JobAction {
    AttachFirst,
    AttachCustom,
    Stdout,
    Stderr,
    Detail,
    BatchScript,
    Dependencies,
    ArrayTasks,
    Cancel,
}

/// State for the job actions modal.
#[derive(Debug, Clone)]
pub struct JobActionState {
    pub job: Job,
    focused: usize,
    options: Vec<JobActionOption>,
}

#[derive(Debug, Clone)]
struct JobActionOption {
    label: String,
    action: Option<JobAction>,
    enabled: bool,
    style_variant: ButtonVariant,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ButtonVariant {
    Primary,
    Default,
    Error,
}

impl JobActionState {
    /// Create a new job action modal for the given job.
    pub fn new(job: Job) -> Self {
        let can_attach = job.state == "RUNNING";

        let options = vec![
            JobActionOption {
                label: "Attach shell (first node)".to_string(),
                action: Some(JobAction::AttachFirst),
                enabled: can_attach,
                style_variant: ButtonVariant::Primary,
            },
            JobActionOption {
                label: "Attach with node override...".to_string(),
                action: Some(JobAction::AttachCustom),
                enabled: can_attach,
                style_variant: ButtonVariant::Default,
            },
            JobActionOption {
                label: "View stdout log".to_string(),
                action: Some(JobAction::Stdout),
                enabled: true,
                style_variant: ButtonVariant::Primary,
            },
            JobActionOption {
                label: "View stderr log".to_string(),
                action: Some(JobAction::Stderr),
                enabled: true,
                style_variant: ButtonVariant::Default,
            },
            JobActionOption {
                label: "Show details".to_string(),
                action: Some(JobAction::Detail),
                enabled: true,
                style_variant: ButtonVariant::Default,
            },
            JobActionOption {
                label: "View batch script".to_string(),
                action: Some(JobAction::BatchScript),
                enabled: true,
                style_variant: ButtonVariant::Default,
            },
            JobActionOption {
                label: "View dependencies".to_string(),
                action: Some(JobAction::Dependencies),
                enabled: true,
                style_variant: ButtonVariant::Default,
            },
            JobActionOption {
                label: "View array tasks".to_string(),
                action: Some(JobAction::ArrayTasks),
                enabled: job.job_id.contains('_'),
                style_variant: ButtonVariant::Default,
            },
            JobActionOption {
                label: "Cancel job [scancel]".to_string(),
                action: Some(JobAction::Cancel),
                enabled: true,
                style_variant: ButtonVariant::Error,
            },
            JobActionOption {
                label: "Close  [esc]".to_string(),
                action: None,
                enabled: true,
                style_variant: ButtonVariant::Default,
            },
        ];

        Self {
            job,
            focused: 0,
            options,
        }
    }

    /// Handle a key event.
    pub fn handle_key(
        &mut self,
        key: KeyEvent,
        _config: &Config,
    ) -> ModalOutcome<Option<JobAction>> {
        match key.code {
            KeyCode::Esc => ModalOutcome::Dismiss(None),
            KeyCode::Down | KeyCode::Tab => {
                self.focus_next();
                ModalOutcome::Continue
            }
            KeyCode::Up | KeyCode::BackTab => {
                self.focus_prev();
                ModalOutcome::Continue
            }
            KeyCode::Enter => {
                let action = self.options[self.focused].action.clone();
                ModalOutcome::Dismiss(action)
            }
            _ => ModalOutcome::Continue,
        }
    }

    fn focus_next(&mut self) {
        self.focused = (self.focused + 1) % self.options.len();
    }

    fn focus_prev(&mut self) {
        self.focused = if self.focused == 0 {
            self.options.len() - 1
        } else {
            self.focused - 1
        };
    }

    /// Render the modal.
    pub fn render(&self, f: &mut Frame, area: ratatui::layout::Rect) {
        let height = 4 + self.options.len() as u16; // title + state + options + padding
        let modal_area = centered_rect(area, 52, height.min(area.height.saturating_sub(2)));

        f.render_widget(Clear, modal_area);

        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::Cyan))
            .title(format!(" Job {} ", self.job.job_id));

        let inner = block.inner(modal_area);
        f.render_widget(block, modal_area);

        // Split: title line + state line + options
        let title_text = self.job.name.clone();
        let state_text = format!("State: {}  User: {}", self.job.state, self.job.user);

        let constraints: Vec<_> = std::iter::once(Constraint::Length(1))
            .chain(std::iter::once(Constraint::Length(1)))
            .chain((0..self.options.len()).map(|_| Constraint::Length(1)))
            .collect();

        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints(constraints)
            .split(inner);

        // Title
        f.render_widget(Paragraph::new(title_text), chunks[0]);

        // State
        f.render_widget(
            Paragraph::new(state_text).style(Style::default().fg(Color::Cyan)),
            chunks[1],
        );

        // Options
        for (i, opt) in self.options.iter().enumerate() {
            let is_focused = i == self.focused;
            let style = self.button_style(opt, is_focused);

            let label = if opt.enabled {
                opt.label.clone()
            } else {
                format!("{} (disabled)", opt.label)
            };

            let line = if is_focused {
                Line::from(vec![Span::raw(" > "), Span::styled(label, style)])
            } else {
                Line::from(vec![Span::raw("   "), Span::styled(label, style)])
            };

            f.render_widget(Paragraph::new(line), chunks[2 + i]);
        }
    }

    fn button_style(&self, opt: &JobActionOption, is_focused: bool) -> Style {
        if !opt.enabled {
            return Style::default().fg(Color::DarkGray);
        }

        let base_color = match opt.style_variant {
            ButtonVariant::Primary => Color::Cyan,
            ButtonVariant::Default => Color::White,
            ButtonVariant::Error => Color::Red,
        };

        if is_focused {
            Style::default()
                .fg(Color::Black)
                .bg(base_color)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(base_color)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_test_job() -> Job {
        Job {
            job_id: "12345".to_string(),
            name: "test_job".to_string(),
            user: "alice".to_string(),
            state: "RUNNING".to_string(),
            partition: "gpu".to_string(),
            nodes: "node01".to_string(),
            num_nodes: "1".to_string(),
            num_cpus: "4".to_string(),
            time_used: "1:23:45".to_string(),
            time_limit: "10:00:00".to_string(),
            reason: "".to_string(),
            nodelist: "node01".to_string(),
            qos: "normal".to_string(),
        }
    }

    #[test]
    fn test_job_action_modal_esc_dismisses() {
        let mut state = JobActionState::new(make_test_job());
        let cfg = Config::default();
        let outcome = state.handle_key(KeyEvent::from(KeyCode::Esc), &cfg);
        assert_eq!(outcome, ModalOutcome::Dismiss(None));
    }

    #[test]
    fn test_job_action_modal_arrow_navigation() {
        let mut state = JobActionState::new(make_test_job());
        let cfg = Config::default();

        assert_eq!(state.focused, 0);

        state.handle_key(KeyEvent::from(KeyCode::Down), &cfg);
        assert_eq!(state.focused, 1);

        state.handle_key(KeyEvent::from(KeyCode::Up), &cfg);
        assert_eq!(state.focused, 0);
    }

    #[test]
    fn test_job_action_modal_wraps_navigation() {
        let mut state = JobActionState::new(make_test_job());
        let cfg = Config::default();
        let len = state.options.len();

        // Go up from first -> wraps to last
        state.handle_key(KeyEvent::from(KeyCode::Up), &cfg);
        assert_eq!(state.focused, len - 1);

        // Go down from last -> wraps to first
        state.handle_key(KeyEvent::from(KeyCode::Down), &cfg);
        assert_eq!(state.focused, 0);
    }

    #[test]
    fn test_job_action_running_enables_attach() {
        let job = make_test_job();
        let state = JobActionState::new(job);

        // First two options are attach options
        assert!(state.options[0].enabled);
        assert!(state.options[1].enabled);
    }

    #[test]
    fn test_job_action_non_running_disables_attach() {
        let mut job = make_test_job();
        job.state = "PENDING".to_string();
        let state = JobActionState::new(job);

        assert!(!state.options[0].enabled);
        assert!(!state.options[1].enabled);
    }

    #[test]
    fn test_non_array_job_disables_array_tasks() {
        let job = make_test_job(); // job_id is "12345", no underscore
        let state = JobActionState::new(job);

        // Find the array tasks option
        let array_option = state
            .options
            .iter()
            .find(|opt| opt.action == Some(JobAction::ArrayTasks));
        assert!(array_option.is_some());
        assert!(!array_option.unwrap().enabled);
    }

    #[test]
    fn test_array_job_enables_array_tasks() {
        let mut job = make_test_job();
        job.job_id = "12345_0".to_string(); // Array job with underscore
        let state = JobActionState::new(job);

        // Find the array tasks option
        let array_option = state
            .options
            .iter()
            .find(|opt| opt.action == Some(JobAction::ArrayTasks));
        assert!(array_option.is_some());
        assert!(array_option.unwrap().enabled);
    }
}

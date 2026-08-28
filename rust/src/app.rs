//! Application state and key dispatch.

use crate::config::Config;
use crate::slurm::exec::Runner;
use crate::slurm::fetch;
use crate::slurm::model::{ClusterSummary, Job, Node};
use crate::views;
use crate::views::jobs::JobsView;
use crate::views::nodes::NodesView;
use anyhow::Result;
use crossterm::event::{self, Event, KeyCode, KeyModifiers};
use ratatui::backend::Backend;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::Line;
use ratatui::widgets::{Block, Borders, Paragraph, Tabs};
use ratatui::Terminal;
use std::sync::mpsc;
use std::time::{Duration, Instant};

/// Top-level tabs, in display order.
///
/// Note: Python app.py also has History (4) and Health (5) tabs,
/// which are pending implementation in the Rust port.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tab {
    Jobs,
    Nodes,
    Partitions,
}

impl Tab {
    /// All tabs in display order.
    pub fn all() -> &'static [Tab] {
        &[Tab::Jobs, Tab::Nodes, Tab::Partitions]
    }

    /// Tab title for display.
    pub fn title(self) -> &'static str {
        match self {
            Tab::Jobs => "Jobs",
            Tab::Nodes => "Nodes",
            Tab::Partitions => "Partitions",
        }
    }

    /// Cycle to the next tab.
    pub fn next(self) -> Tab {
        match self {
            Tab::Jobs => Tab::Nodes,
            Tab::Nodes => Tab::Partitions,
            Tab::Partitions => Tab::Jobs,
        }
    }

    /// Cycle to the previous tab.
    pub fn prev(self) -> Tab {
        match self {
            Tab::Jobs => Tab::Partitions,
            Tab::Nodes => Tab::Jobs,
            Tab::Partitions => Tab::Nodes,
        }
    }

    /// Get the refresh interval for this tab from config.
    pub fn interval(self, config: &Config) -> Duration {
        let seconds = match self {
            Tab::Jobs => config.interval.jobs,
            Tab::Nodes => config.interval.nodes,
            Tab::Partitions => config.interval.partitions,
        };
        Duration::from_secs_f64(seconds)
    }
}

/// Pending action awaiting confirmation.
#[derive(Debug, Clone)]
enum PendingAction {
    CancelJob(String),
    BulkAction {
        action: String,
        job_ids: Vec<String>,
    },
}

/// Modal overlay state. View workers will add variants as needed.
#[derive(Debug)]
pub enum Modal {
    /// No modal active.
    None,
    /// Yes/No confirmation modal.
    Confirm(crate::views::modals::confirm::ConfirmState),
    /// Job action picker.
    JobAction(Box<crate::views::modals::job_actions::JobActionState>),
    /// Bulk action picker.
    BulkAction(crate::views::modals::bulk_actions::BulkActionState),
    /// Column visibility toggle.
    ColumnToggle(crate::views::modals::column_toggle::ColumnToggleState),
    /// Keybindings help.
    KeybindingsHelp(crate::views::modals::keybindings_help::KeybindingsHelpState),
}

/// Messages from background refresh worker to main thread.
#[derive(Debug, Clone)]
pub enum Msg {
    Jobs(Vec<Job>),
    Nodes(Vec<Node>),
    Partitions(Vec<ClusterSummary>),
    Error(String),
}

/// Application state.
pub struct App {
    pub config: Config,
    pub runner: Runner,
    pub tab: Tab,
    pub jobs: Vec<Job>,
    pub nodes: Vec<Node>,
    pub partitions: Vec<ClusterSummary>,
    pub status: Option<String>,
    pub should_quit: bool,
    pub modal: Modal,
    pub jobs_view: JobsView,
    pub nodes_view: NodesView,
    refresh_rx: mpsc::Receiver<Msg>,
    request_tx: mpsc::Sender<Tab>,
    last_refresh: Option<Instant>,
    pending_action: Option<PendingAction>,
}

impl App {
    /// Create a new App with the given config.
    pub fn new(config: Config) -> Self {
        let runner = Runner::new();
        let (msg_tx, msg_rx) = mpsc::channel();
        let (request_tx, request_rx) = mpsc::channel();

        // One worker thread serves fetch requests so the UI never blocks on Slurm.
        let worker_runner = runner.clone();
        std::thread::spawn(move || refresh_worker(worker_runner, request_rx, msg_tx));

        let jobs_view = JobsView::from_config(&config);
        let nodes_view = NodesView::new(&config);

        Self {
            jobs_view,
            nodes_view,
            config,
            runner,
            tab: Tab::Jobs,
            jobs: Vec::new(),
            nodes: Vec::new(),
            partitions: Vec::new(),
            status: None,
            should_quit: false,
            modal: Modal::None,
            refresh_rx: msg_rx,
            request_tx,
            last_refresh: None,
            pending_action: None,
        }
    }

    /// Process any pending messages from the refresh worker.
    pub fn drain_messages(&mut self) {
        while let Ok(msg) = self.refresh_rx.try_recv() {
            match msg {
                Msg::Jobs(jobs) => {
                    self.jobs = jobs;
                    self.status = None;
                }
                Msg::Nodes(nodes) => {
                    self.nodes = nodes;
                    self.status = None;
                }
                Msg::Partitions(partitions) => {
                    self.partitions = partitions;
                    self.status = None;
                }
                Msg::Error(err) => {
                    self.status = Some(format!("Error: {}", err));
                }
            }
        }
    }

    /// Check if expert mode is enabled.
    fn expert_mode_enabled(&self) -> bool {
        self.config.ui.expert_mode
    }

    /// Check if single-job cancel confirmation is enabled.
    fn confirm_cancel_single_enabled(&self) -> bool {
        self.config.safety.confirm_cancel_single
    }

    /// Check if bulk action confirmation is enabled.
    fn confirm_bulk_actions_enabled(&self) -> bool {
        self.config.safety.confirm_bulk_actions
    }

    /// Get the currently selected job (on Jobs tab, cursor position).
    fn current_job(&self) -> Option<&Job> {
        if self.tab != Tab::Jobs {
            return None;
        }
        let filtered = self.jobs_view.filtered_jobs(&self.jobs);
        let cursor = self.jobs_view.cursor_row();
        filtered.get(cursor).copied()
    }

    /// Execute a pending action after confirmation.
    fn execute_pending_action(&mut self) {
        if let Some(action) = self.pending_action.take() {
            match action {
                PendingAction::CancelJob(job_id) => {
                    self.execute_cancel_job(&job_id);
                }
                PendingAction::BulkAction { action, job_ids } => {
                    self.execute_bulk_action(&action, &job_ids);
                }
            }
        }
    }

    /// Execute cancel job action.
    fn execute_cancel_job(&mut self, job_id: &str) {
        use crate::slurm::exec;
        let (ok, message) = exec::cancel_job_result(&self.runner, job_id);
        if ok {
            self.status = Some(format!("Cancelled job {}", job_id));
        } else {
            self.status = Some(format!("Cancel failed: {}", message));
        }
        self.request_refresh();
    }

    /// Execute bulk action.
    fn execute_bulk_action(&mut self, action: &str, job_ids: &[String]) {
        use crate::slurm::exec;
        let results = exec::run_bulk_job_action(&self.runner, action, job_ids);

        let ok_count = results.iter().filter(|r| r.ok).count();
        let total = results.len();

        if ok_count == total {
            self.status = Some(format!("{} {} job(s)", action.to_uppercase(), total));
        } else {
            self.status = Some(format!(
                "{} {}/{} job(s)",
                action.to_uppercase(),
                ok_count,
                total
            ));
        }
        self.request_refresh();
    }

    /// Open confirmation modal for the pending action.
    fn open_confirm_modal(&mut self, message: String) {
        use crate::views::modals::confirm::ConfirmState;
        self.modal = Modal::Confirm(ConfirmState::new(message));
    }

    /// Handle job action selection from modal.
    fn handle_job_action(&mut self, job_id: String, action: &str) {
        match action {
            "cancel" => {
                // Check if confirmation is needed
                let need_confirm =
                    !self.expert_mode_enabled() && self.confirm_cancel_single_enabled();
                if need_confirm {
                    self.pending_action = Some(PendingAction::CancelJob(job_id.clone()));
                    self.open_confirm_modal(format!("Cancel job {}?", job_id));
                } else {
                    self.execute_cancel_job(&job_id);
                }
            }
            _ => {
                // Other actions not yet implemented
                self.status = Some(format!("Action '{}' not yet implemented", action));
            }
        }
    }

    /// Handle bulk action selection from modal.
    fn handle_bulk_action(&mut self, action: &str, job_ids: Vec<String>) {
        // Determine if confirmation is needed
        let need_confirm = if action == "cancel" {
            !self.expert_mode_enabled()
        } else {
            !self.expert_mode_enabled() && self.confirm_bulk_actions_enabled()
        };

        if need_confirm {
            self.pending_action = Some(PendingAction::BulkAction {
                action: action.to_string(),
                job_ids: job_ids.clone(),
            });
            self.open_confirm_modal(format!("{} {} job(s)?", action, job_ids.len()));
        } else {
            self.execute_bulk_action(action, &job_ids);
        }
    }

    /// Handle a key event. Returns true if the event was handled.
    pub fn handle_key(&mut self, key: KeyCode, modifiers: KeyModifiers) -> bool {
        // Route to modal first if active
        if !matches!(self.modal, Modal::None) {
            use crate::views::modals::bulk_actions::BulkAction;
            use crate::views::modals::column_toggle::ColumnToggleResult;
            use crate::views::modals::confirm::ConfirmResult;
            use crate::views::modals::job_actions::JobAction;
            use crate::views::modals::ModalOutcome;

            let key_event = crossterm::event::KeyEvent::new(key, modifiers);

            match &mut self.modal {
                Modal::Confirm(state) => match state.handle_key(key_event, &self.config) {
                    ModalOutcome::Dismiss(result) => {
                        self.modal = Modal::None;
                        if result == ConfirmResult::Yes {
                            self.execute_pending_action();
                        } else {
                            self.pending_action = None;
                        }
                        return true;
                    }
                    ModalOutcome::Continue => return true,
                },
                Modal::JobAction(state) => {
                    match state.handle_key(key_event, &self.config) {
                        ModalOutcome::Dismiss(action) => {
                            // Extract job_id before dropping the modal
                            let job_id = state.job.job_id.clone();
                            self.modal = Modal::None;
                            if let Some(action) = action {
                                match action {
                                    JobAction::Cancel => self.handle_job_action(job_id, "cancel"),
                                    JobAction::AttachFirst => {
                                        self.status =
                                            Some("Attach not yet implemented".to_string());
                                    }
                                    JobAction::AttachCustom => {
                                        self.status =
                                            Some("Attach not yet implemented".to_string());
                                    }
                                    JobAction::Stdout => {
                                        self.status =
                                            Some("Log viewing not yet implemented".to_string());
                                    }
                                    JobAction::Stderr => {
                                        self.status =
                                            Some("Log viewing not yet implemented".to_string());
                                    }
                                    JobAction::Detail => {
                                        self.status =
                                            Some("Detail view not yet implemented".to_string());
                                    }
                                    JobAction::BatchScript => {
                                        self.status = Some(
                                            "Batch script view not yet implemented".to_string(),
                                        );
                                    }
                                    JobAction::Dependencies => {
                                        self.status = Some(
                                            "Dependencies view not yet implemented".to_string(),
                                        );
                                    }
                                }
                            }
                            return true;
                        }
                        ModalOutcome::Continue => return true,
                    }
                }
                Modal::BulkAction(state) => {
                    match state.handle_key(key_event, &self.config) {
                        ModalOutcome::Dismiss(action) => {
                            let _selected_count = state.selected_count;
                            self.modal = Modal::None;
                            if let Some(action) = action {
                                // Get selected job IDs from jobs_view
                                let job_ids: Vec<String> = self
                                    .jobs_view
                                    .selected_jobs(&self.jobs)
                                    .iter()
                                    .map(|j| j.job_id.clone())
                                    .collect();

                                if job_ids.is_empty() {
                                    self.status = Some("No jobs selected".to_string());
                                } else {
                                    match action {
                                        BulkAction::Cancel => {
                                            self.handle_bulk_action("cancel", job_ids)
                                        }
                                        BulkAction::Hold => {
                                            self.handle_bulk_action("hold", job_ids)
                                        }
                                        BulkAction::Release => {
                                            self.handle_bulk_action("release", job_ids)
                                        }
                                        BulkAction::Requeue => {
                                            self.handle_bulk_action("requeue", job_ids)
                                        }
                                    }
                                }
                            }
                            return true;
                        }
                        ModalOutcome::Continue => return true,
                    }
                }
                Modal::ColumnToggle(state) => {
                    match state.handle_key(key_event, &mut self.config) {
                        ModalOutcome::Dismiss(result) => {
                            self.modal = Modal::None;
                            if result == ColumnToggleResult::Reset {
                                // Reset column order for the current tab
                                match self.tab {
                                    Tab::Jobs => {
                                        self.config.columns.jobs_order.clear();
                                        self.config.columns.jobs_hidden.clear();
                                    }
                                    Tab::Nodes => {
                                        self.config.columns.nodes_order.clear();
                                        self.config.columns.nodes_hidden.clear();
                                    }
                                    Tab::Partitions => {
                                        self.config.columns.partitions_order.clear();
                                        self.config.columns.partitions_hidden.clear();
                                    }
                                }
                                // Reload views with new column config
                                self.jobs_view = JobsView::from_config(&self.config);
                                self.nodes_view = NodesView::new(&self.config);
                            }
                            return true;
                        }
                        ModalOutcome::Continue => return true,
                    }
                }
                Modal::KeybindingsHelp(state) => match state.handle_key(key_event, &self.config) {
                    ModalOutcome::Dismiss(_) => {
                        self.modal = Modal::None;
                        return true;
                    }
                    ModalOutcome::Continue => return true,
                },
                Modal::None => {}
            }
        }

        // App-level bindings (uppercase or ctrl+ to avoid colliding with view bindings)
        match (key, modifiers) {
            // Quit
            (KeyCode::Char('q'), KeyModifiers::NONE) => {
                self.should_quit = true;
                true
            }
            (KeyCode::Char('c'), KeyModifiers::CONTROL) => {
                self.should_quit = true;
                true
            }
            // Tab switching
            (KeyCode::Char('1'), KeyModifiers::NONE) => {
                self.tab = Tab::Jobs;
                self.last_refresh = None;
                true
            }
            (KeyCode::Char('2'), KeyModifiers::NONE) => {
                self.tab = Tab::Nodes;
                self.last_refresh = None;
                true
            }
            (KeyCode::Char('3'), KeyModifiers::NONE) => {
                self.tab = Tab::Partitions;
                self.last_refresh = None;
                true
            }
            // Refresh
            (KeyCode::Char('r'), KeyModifiers::NONE) => {
                self.last_refresh = None;
                true
            }
            // Column toggle
            (KeyCode::Char('C'), KeyModifiers::NONE) => {
                use crate::columns::{jobs_columns, nodes_columns};
                use crate::views::modals::column_toggle::ColumnToggleState;

                match self.tab {
                    Tab::Jobs => {
                        let all_cols: Vec<String> =
                            jobs_columns().iter().map(|c| c.name.clone()).collect();
                        let hidden = self.config.columns.jobs_hidden.clone();
                        let order = if self.config.columns.jobs_order.is_empty() {
                            None
                        } else {
                            Some(self.config.columns.jobs_order.clone())
                        };
                        self.modal = Modal::ColumnToggle(ColumnToggleState::new(
                            "Jobs".to_string(),
                            all_cols,
                            hidden,
                            order,
                        ));
                    }
                    Tab::Nodes => {
                        let all_cols: Vec<String> =
                            nodes_columns().iter().map(|c| c.name.clone()).collect();
                        let hidden = self.config.columns.nodes_hidden.clone();
                        let order = if self.config.columns.nodes_order.is_empty() {
                            None
                        } else {
                            Some(self.config.columns.nodes_order.clone())
                        };
                        self.modal = Modal::ColumnToggle(ColumnToggleState::new(
                            "Nodes".to_string(),
                            all_cols,
                            hidden,
                            order,
                        ));
                    }
                    Tab::Partitions => {
                        // Partitions column toggle not yet implemented
                        self.status =
                            Some("Column toggle not available for Partitions".to_string());
                    }
                }
                true
            }
            // Keybindings help
            (KeyCode::Char('?'), KeyModifiers::NONE) => {
                use crate::views::modals::keybindings_help::KeybindingsHelpState;

                let pane_name = match self.tab {
                    Tab::Jobs => "Jobs",
                    Tab::Nodes => "Nodes",
                    Tab::Partitions => "Partitions",
                };
                self.modal =
                    Modal::KeybindingsHelp(KeybindingsHelpState::new(pane_name.to_string()));
                true
            }
            // Jobs tab: Enter for job actions
            (KeyCode::Enter, KeyModifiers::NONE) if self.tab == Tab::Jobs => {
                if let Some(job) = self.current_job() {
                    use crate::views::modals::job_actions::JobActionState;
                    self.modal = Modal::JobAction(Box::new(JobActionState::new(job.clone())));
                }
                true
            }
            // Jobs tab: B for bulk actions
            (KeyCode::Char('B'), KeyModifiers::NONE) if self.tab == Tab::Jobs => {
                let selected_count = self.jobs_view.selection_count();
                if selected_count > 0 {
                    use crate::views::modals::bulk_actions::BulkActionState;
                    self.modal = Modal::BulkAction(BulkActionState::new(selected_count));
                } else {
                    self.status = Some("No jobs selected".to_string());
                }
                true
            }
            _ => false,
        }
    }

    /// Check if it's time to refresh the current tab.
    pub fn should_refresh(&self) -> bool {
        match self.last_refresh {
            None => true,
            Some(last) => last.elapsed() >= self.tab.interval(&self.config),
        }
    }

    /// Ask the worker to refresh the current tab and reset the interval clock.
    pub fn request_refresh(&mut self) {
        let _ = self.request_tx.send(self.tab);
        self.last_refresh = Some(Instant::now());
    }
}

/// Background worker: serves fetch requests so the UI thread never blocks on Slurm.
///
/// The main loop owns the refresh cadence (see `App::should_refresh`) and sends the
/// tab to refresh; this thread answers with the fetched data.
fn refresh_worker(runner: Runner, requests: mpsc::Receiver<Tab>, tx: mpsc::Sender<Msg>) {
    for tab in requests {
        let msg = match tab {
            Tab::Jobs => Msg::Jobs(fetch::fetch_jobs(&runner)),
            Tab::Nodes => Msg::Nodes(fetch::fetch_nodes(&runner)),
            Tab::Partitions => Msg::Partitions(fetch::fetch_cluster_summary(&runner)),
        };
        if tx.send(msg).is_err() {
            break; // UI is gone
        }
    }
}

/// Run the application event loop until the user quits.
pub fn run<B: Backend>(terminal: &mut Terminal<B>, config: Config) -> Result<()> {
    let mut app = App::new(config);

    loop {
        app.drain_messages();

        terminal.draw(|f| render(f, &mut app))?;

        if app.should_quit {
            break;
        }

        if event::poll(Duration::from_millis(50))? {
            if let Event::Key(key) = event::read()? {
                app.handle_key(key.code, key.modifiers);
            }
        }

        // Ask the worker for fresh data when the tab's interval has elapsed.
        if app.should_refresh() {
            app.request_refresh();
        }
    }

    Ok(())
}

/// Render the app to the terminal.
fn render(f: &mut ratatui::Frame, app: &mut App) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3), // Tab bar
            Constraint::Min(0),    // Content
            Constraint::Length(1), // Status bar
        ])
        .split(f.area());

    render_tabs(f, app, chunks[0]);
    render_content(f, app, chunks[1]);
    render_status(f, app, chunks[2]);

    // Render modal overlay if active
    if !matches!(app.modal, Modal::None) {
        render_modal(f, app, f.area());
    }
}

/// Render the tab bar.
fn render_tabs(f: &mut ratatui::Frame, app: &App, area: Rect) {
    let titles: Vec<Line> = Tab::all().iter().map(|t| Line::from(t.title())).collect();

    let selected = match app.tab {
        Tab::Jobs => 0,
        Tab::Nodes => 1,
        Tab::Partitions => 2,
    };

    let tabs = Tabs::new(titles)
        .block(Block::default().borders(Borders::ALL).title("sqtop"))
        .select(selected)
        .style(Style::default().fg(Color::White))
        .highlight_style(
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        );

    f.render_widget(tabs, area);
}

/// Render the main content area.
fn render_content(f: &mut ratatui::Frame, app: &mut App, area: Rect) {
    match app.tab {
        Tab::Jobs => views::render_jobs(f, app, area),
        Tab::Nodes => views::render_nodes(f, app, area),
        Tab::Partitions => views::render_partitions(f, app, area),
    }
}

/// Render the status bar.
fn render_status(f: &mut ratatui::Frame, app: &App, area: Rect) {
    let status_text = if let Some(ref msg) = app.status {
        msg.clone()
    } else {
        format!(
            "Jobs: {} | Nodes: {} | Partitions: {} | Press 'q' to quit, '1-3' to switch tabs, 'r' to refresh",
            app.jobs.len(),
            app.nodes.len(),
            app.partitions.len()
        )
    };

    let status = Paragraph::new(status_text).style(Style::default().fg(Color::White));
    f.render_widget(status, area);
}

/// Render the active modal overlay.
fn render_modal(f: &mut ratatui::Frame, app: &App, area: Rect) {
    match &app.modal {
        Modal::Confirm(state) => state.render(f, area),
        Modal::JobAction(state) => state.render(f, area),
        Modal::BulkAction(state) => state.render(f, area),
        Modal::ColumnToggle(state) => state.render(f, area),
        Modal::KeybindingsHelp(state) => state.render(f, area),
        Modal::None => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tab_cycle() {
        assert_eq!(Tab::Jobs.next(), Tab::Nodes);
        assert_eq!(Tab::Nodes.next(), Tab::Partitions);
        assert_eq!(Tab::Partitions.next(), Tab::Jobs);

        assert_eq!(Tab::Jobs.prev(), Tab::Partitions);
        assert_eq!(Tab::Nodes.prev(), Tab::Jobs);
        assert_eq!(Tab::Partitions.prev(), Tab::Nodes);
    }

    #[test]
    fn tab_titles() {
        assert_eq!(Tab::Jobs.title(), "Jobs");
        assert_eq!(Tab::Nodes.title(), "Nodes");
        assert_eq!(Tab::Partitions.title(), "Partitions");
    }

    #[test]
    fn tab_all() {
        let all = Tab::all();
        assert_eq!(all.len(), 3);
        assert_eq!(all[0], Tab::Jobs);
        assert_eq!(all[1], Tab::Nodes);
        assert_eq!(all[2], Tab::Partitions);
    }

    #[test]
    fn quit_keys() {
        let config = Config::default();
        let mut app = App::new(config);

        assert!(!app.should_quit);
        app.handle_key(KeyCode::Char('q'), KeyModifiers::NONE);
        assert!(app.should_quit);

        let mut app2 = App::new(Config::default());
        app2.handle_key(KeyCode::Char('c'), KeyModifiers::CONTROL);
        assert!(app2.should_quit);
    }

    #[test]
    fn tab_switch_keys() {
        let config = Config::default();
        let mut app = App::new(config);

        app.handle_key(KeyCode::Char('2'), KeyModifiers::NONE);
        assert_eq!(app.tab, Tab::Nodes);

        app.handle_key(KeyCode::Char('3'), KeyModifiers::NONE);
        assert_eq!(app.tab, Tab::Partitions);

        app.handle_key(KeyCode::Char('1'), KeyModifiers::NONE);
        assert_eq!(app.tab, Tab::Jobs);
    }

    #[test]
    fn message_handling() {
        let config = Config::default();
        let mut app = App::new(config);

        // Initially empty
        assert!(app.jobs.is_empty());
        assert!(app.nodes.is_empty());
        assert!(app.partitions.is_empty());

        // Drain messages (should be no-op initially)
        app.drain_messages();
        assert!(app.jobs.is_empty());
    }

    #[test]
    fn smoke_render() {
        use ratatui::backend::TestBackend;
        use ratatui::Terminal;

        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend).unwrap();
        let config = Config::default();

        // Should not panic
        terminal
            .draw(|f| {
                let mut app = App::new(config);
                render(f, &mut app);
            })
            .unwrap();
    }
}

#[cfg(test)]
mod modal_tests {
    use super::*;

    #[test]
    fn test_expert_mode_skips_cancel_confirmation() {
        let mut config = Config::default();
        config.ui.expert_mode = true;
        config.safety.confirm_cancel_single = true;

        let app = App::new(config);

        // expert_mode_enabled should return true
        assert!(app.expert_mode_enabled());

        // With expert mode, confirmation should be skipped
        let need_confirm = !app.expert_mode_enabled() && app.confirm_cancel_single_enabled();
        assert!(!need_confirm, "Expert mode should skip confirmation");
    }

    #[test]
    fn test_confirm_cancel_single_flag_enables_confirmation() {
        let mut config = Config::default();
        config.ui.expert_mode = false;
        config.safety.confirm_cancel_single = true;

        let app = App::new(config);

        let need_confirm = !app.expert_mode_enabled() && app.confirm_cancel_single_enabled();
        assert!(
            need_confirm,
            "Should require confirmation when expert_mode=false and confirm_cancel_single=true"
        );
    }

    #[test]
    fn test_confirm_cancel_single_disabled_skips_confirmation() {
        let mut config = Config::default();
        config.ui.expert_mode = false;
        config.safety.confirm_cancel_single = false;

        let app = App::new(config);

        let need_confirm = !app.expert_mode_enabled() && app.confirm_cancel_single_enabled();
        assert!(
            !need_confirm,
            "Should skip confirmation when confirm_cancel_single=false"
        );
    }

    #[test]
    fn test_bulk_cancel_always_needs_confirmation_unless_expert() {
        let mut config = Config::default();
        config.ui.expert_mode = false;
        config.safety.confirm_bulk_actions = false;

        let app = App::new(config);

        // For cancel action
        let action = "cancel";
        let need_confirm = if action == "cancel" {
            !app.expert_mode_enabled()
        } else {
            !app.expert_mode_enabled() && app.confirm_bulk_actions_enabled()
        };

        assert!(
            need_confirm,
            "Bulk cancel should need confirmation even when confirm_bulk_actions=false"
        );
    }

    #[test]
    fn test_bulk_non_cancel_respects_confirm_bulk_actions() {
        let mut config = Config::default();
        config.ui.expert_mode = false;
        config.safety.confirm_bulk_actions = false;

        let app = App::new(config);

        // For hold action
        let action = "hold";
        let need_confirm = if action == "cancel" {
            !app.expert_mode_enabled()
        } else {
            !app.expert_mode_enabled() && app.confirm_bulk_actions_enabled()
        };

        assert!(
            !need_confirm,
            "Bulk hold should skip confirmation when confirm_bulk_actions=false"
        );
    }
}

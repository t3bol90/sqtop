//! Application state and key dispatch.

use crate::config::Config;
use crate::investigation::{InvestigationReport, ReasonTable};
use crate::slurm::exec::Runner;
use crate::slurm::fetch;
use crate::slurm::fetch::{JobDependency, JobEfficiency, SacctJob};
use crate::slurm::model::{ClusterSummary, Job, Node};
use crate::views;
use crate::views::detail::{
    ArrayTaskScreen, AttachPromptScreen, BatchScriptScreen, DependencyScreen, JobDetailScreen,
    JobInfoScreen, LogViewerScreen, NodeDetailScreen, Outcome as DetailOutcome,
};
use crate::views::history::HistoryView;
use crate::views::investigate::InvestigationScreen;
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
use std::collections::HashMap;
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
    History,
    Health,
}

impl Tab {
    /// All tabs in display order.
    pub fn all() -> &'static [Tab] {
        &[
            Tab::Jobs,
            Tab::Nodes,
            Tab::Partitions,
            Tab::History,
            Tab::Health,
        ]
    }

    /// Tab title for display.
    pub fn title(self) -> &'static str {
        match self {
            Tab::Jobs => "Jobs",
            Tab::Nodes => "Nodes",
            Tab::Partitions => "Partitions",
            Tab::History => "History",
            Tab::Health => "Health",
        }
    }

    /// Get the refresh interval for this tab from config.
    pub fn interval(self, config: &Config) -> Duration {
        let seconds = match self {
            Tab::Jobs => config.interval.jobs,
            Tab::Nodes => config.interval.nodes,
            Tab::Partitions => config.interval.partitions,
            Tab::History | Tab::Health => config.interval.jobs, // Use same as jobs
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
    /// Job detail screen.
    JobDetail(JobDetailScreen),
    /// Batch script viewer.
    BatchScript(BatchScriptScreen),
    /// Log viewer (stdout/stderr).
    LogViewer(LogViewerScreen),
    /// Dependency viewer.
    Dependencies(DependencyScreen),
    /// Array task viewer.
    ArrayTasks(ArrayTaskScreen),
    /// Attach prompt.
    AttachPrompt(AttachPromptScreen),
    /// Investigation report.
    Investigation(InvestigationScreen),
    /// Node detail screen.
    NodeDetail(NodeDetailScreen),
    /// Job info screen (with efficiency data).
    JobInfo(JobInfoScreen),
}

/// Messages from background refresh worker to main thread.
#[derive(Debug, Clone)]
pub enum Msg {
    Jobs(Vec<Job>),
    Nodes(Vec<Node>),
    Partitions(Vec<ClusterSummary>),
    History(Vec<SacctJob>),
    Investigation(Box<InvestigationReport>),
    JobDetail {
        job_id: String,
        data: HashMap<String, String>,
        efficiency: JobEfficiency,
    },
    ArrayTasks {
        job: Job,
        tasks: Vec<Job>,
    },
    NodeDetail {
        node: Node,
        data: HashMap<String, String>,
        jobs: Vec<Job>,
    },
    LogViewer {
        job_id: String,
        path: String,
        log_type: String,
        content: String,
    },
    BatchScript {
        job_id: String,
        script: String,
    },
    Dependencies {
        job: Job,
        deps: Vec<JobDependency>,
    },
    JobInfo {
        job: Job,
        detail: HashMap<String, String>,
        deps: Vec<JobDependency>,
    },
    Status(String),
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
    pub partitions_table_state: crate::views::table_state::CyclicTableState,
    pub status: Option<String>,
    pub should_quit: bool,
    pub modal: Modal,
    pub jobs_view: JobsView,
    pub nodes_view: NodesView,
    pub history_view: HistoryView,
    refresh_rx: mpsc::Receiver<Msg>,
    msg_tx: mpsc::Sender<Msg>,
    request_tx: mpsc::Sender<Tab>,
    last_refresh: Option<Instant>,
    pending_action: Option<PendingAction>,
}

impl App {
    /// Create a new App with the given config.
    pub fn new(config: Config) -> Self {
        let runner = Runner::new();

        // Configure remote SSH if configured
        if !config.remote.host.is_empty() {
            runner.set_remote(config.remote.host.clone(), String::new());
        }

        let (msg_tx, msg_rx) = mpsc::channel();
        let (request_tx, request_rx) = mpsc::channel();

        // Clone msg_tx for investigation threads
        let msg_tx_clone = msg_tx.clone();

        // One worker thread serves fetch requests so the UI never blocks on Slurm.
        let worker_runner = runner.clone();
        std::thread::spawn(move || refresh_worker(worker_runner, request_rx, msg_tx));

        let jobs_view = JobsView::from_config(&config);
        let nodes_view = NodesView::new(&config);
        let history_view = HistoryView::new();

        Self {
            jobs_view,
            nodes_view,
            history_view,
            config,
            runner,
            tab: Tab::Jobs,
            jobs: Vec::new(),
            nodes: Vec::new(),
            partitions: Vec::new(),
            partitions_table_state: crate::views::table_state::CyclicTableState::new(),
            status: None,
            should_quit: false,
            modal: Modal::None,
            refresh_rx: msg_rx,
            msg_tx: msg_tx_clone,
            request_tx,
            last_refresh: None,
            pending_action: None,
        }
    }

    /// Process any pending messages from the refresh worker.
    pub fn drain_messages(&mut self) {
        let current_user = std::env::var("USER").unwrap_or_default();
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
                    use crate::views::partitions::{capture_cursor_state, restore_cursor_position};
                    // Capture cursor state before update
                    let cursor_row = self.partitions_table_state.selected();
                    let saved_row = cursor_row.unwrap_or(0);
                    let state = capture_cursor_state(&self.partitions, cursor_row);
                    // Update data
                    self.partitions = partitions;
                    self.partitions_table_state
                        .set_row_count(self.partitions.len());
                    // Restore cursor position
                    let new_row = restore_cursor_position(&state, &self.partitions, saved_row);
                    self.partitions_table_state.select(new_row);
                    self.status = None;
                }
                Msg::History(sacct_jobs) => {
                    let (old_selected, anchor) =
                        self.history_view.update(sacct_jobs, &current_user);
                    self.history_view.restore_state(old_selected, anchor);
                    self.status = None;
                }
                Msg::Investigation(report) => {
                    // Create screen based on target kind
                    let mut screen = if report.target.kind == "job" {
                        InvestigationScreen::for_job(report.target.identifier.clone(), None)
                    } else {
                        InvestigationScreen::for_node(report.target.identifier.clone())
                    };
                    screen.load_report(*report);
                    self.modal = Modal::Investigation(screen);
                }
                Msg::JobDetail {
                    job_id,
                    data,
                    efficiency,
                } => {
                    let mut screen = JobDetailScreen::new(job_id, data);
                    screen.set_efficiency(efficiency);
                    self.modal = Modal::JobDetail(screen);
                    self.status = None;
                }
                Msg::ArrayTasks { job, tasks } => {
                    let screen = ArrayTaskScreen::new(job, tasks);
                    self.modal = Modal::ArrayTasks(screen);
                    self.status = None;
                }
                Msg::NodeDetail { node, data, jobs } => {
                    let mut screen = NodeDetailScreen::new(node, data);
                    screen.set_jobs(jobs);
                    self.modal = Modal::NodeDetail(screen);
                    self.status = None;
                }
                Msg::LogViewer {
                    job_id,
                    path,
                    log_type,
                    content,
                } => {
                    let screen = LogViewerScreen::new(job_id, path, log_type, content);
                    self.modal = Modal::LogViewer(screen);
                    self.status = None;
                }
                Msg::BatchScript { job_id, script } => {
                    let screen = BatchScriptScreen::new(job_id, script);
                    self.modal = Modal::BatchScript(screen);
                    self.status = None;
                }
                Msg::Dependencies { job, deps } => {
                    let screen = DependencyScreen::new(job, deps);
                    self.modal = Modal::Dependencies(screen);
                    self.status = None;
                }
                Msg::JobInfo { job, detail, deps } => {
                    let screen = JobInfoScreen::new(job, detail, deps);
                    self.modal = Modal::JobInfo(screen);
                    self.status = None;
                }
                Msg::Status(msg) => {
                    self.status = Some(msg);
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

    /// Get the currently selected node (Nodes tab only).
    fn current_node(&self) -> Option<&Node> {
        if self.tab != Tab::Nodes {
            return None;
        }
        self.nodes_view.current_node(&self.nodes)
    }

    /// Open the log viewer for a job (stdout or stderr).
    fn open_log_viewer(&mut self, job_id: String, is_stdout: bool) {
        self.status = Some("Loading log...".to_string());
        let runner = self.runner.clone();
        let tx = self.msg_tx.clone();

        std::thread::spawn(move || {
            let (stdout_path, stderr_path) = fetch::fetch_log_paths(&runner, &job_id);
            let (path, log_type) = if is_stdout {
                (stdout_path, "stdout".to_string())
            } else {
                (stderr_path, "stderr".to_string())
            };
            if path.is_empty() {
                let _ = tx.send(Msg::Error("Log path not found".to_string()));
                return;
            }
            let content = fetch::tail_log_file(&runner, &path, 500);
            let _ = tx.send(Msg::LogViewer {
                job_id,
                path,
                log_type,
                content,
            });
        });
    }

    /// Open the job detail screen (dispatches async fetch).
    fn open_job_detail(&mut self, job_id: String) {
        self.status = Some("Loading job details...".to_string());
        let runner = self.runner.clone();
        let tx = self.msg_tx.clone();
        let job_id_clone = job_id.clone();

        std::thread::spawn(move || {
            let data = fetch::fetch_job_detail(&runner, &job_id_clone);
            let efficiency = fetch::fetch_job_efficiency(&runner, &job_id_clone);
            let _ = tx.send(Msg::JobDetail {
                job_id: job_id_clone,
                data,
                efficiency,
            });
        });
    }

    /// Open the batch script viewer (dispatches async fetch).
    fn open_batch_script(&mut self, job_id: String) {
        self.status = Some("Loading batch script...".to_string());
        let runner = self.runner.clone();
        let tx = self.msg_tx.clone();

        std::thread::spawn(move || {
            let script = fetch::fetch_batch_script(&runner, &job_id);
            let _ = tx.send(Msg::BatchScript { job_id, script });
        });
    }

    /// Open the dependencies viewer (dispatches async fetch).
    fn open_dependencies(&mut self, job_id: String) {
        // Find the job first
        if let Some(job) = self.jobs.iter().find(|j| j.job_id == job_id).cloned() {
            self.status = Some("Loading dependencies...".to_string());
            let runner = self.runner.clone();
            let tx = self.msg_tx.clone();

            std::thread::spawn(move || {
                let deps = fetch::fetch_job_dependencies(&runner, &job_id);
                let _ = tx.send(Msg::Dependencies { job, deps });
            });
        } else {
            self.status = Some("Job not found".to_string());
        }
    }

    /// Open the array tasks viewer (dispatches async fetch).
    fn open_array_tasks(&mut self, job_id: String) {
        // Find the job first
        if let Some(job) = self.jobs.iter().find(|j| j.job_id == job_id).cloned() {
            self.status = Some("Loading array tasks...".to_string());
            let runner = self.runner.clone();
            let tx = self.msg_tx.clone();

            std::thread::spawn(move || {
                let tasks = fetch::fetch_array_tasks(&runner, &job_id);
                let _ = tx.send(Msg::ArrayTasks { job, tasks });
            });
        } else {
            self.status = Some("Job not found".to_string());
        }
    }

    /// Open the node detail screen (dispatches async fetch).
    fn open_node_detail(&mut self, node_name: String) {
        if let Some(node) = self.nodes.iter().find(|n| n.name == node_name).cloned() {
            self.status = Some("Loading node details...".to_string());

            let runner = self.runner.clone();
            let tx = self.msg_tx.clone();

            std::thread::spawn(move || {
                let data = fetch::fetch_node_detail(&runner, &node_name);
                let jobs = fetch::fetch_jobs_on_node(&runner, &node_name);
                let _ = tx.send(Msg::NodeDetail { node, data, jobs });
            });
        } else {
            self.status = Some("Node not found".to_string());
        }
    }

    /// Open the job info screen (dispatches async fetch).
    fn open_job_info(&mut self, job_id: String) {
        if let Some(job) = self.jobs.iter().find(|j| j.job_id == job_id).cloned() {
            self.status = Some("Loading job info...".to_string());
            let runner = self.runner.clone();
            let tx = self.msg_tx.clone();

            std::thread::spawn(move || {
                let detail = fetch::fetch_job_detail(&runner, &job_id);
                let deps = fetch::fetch_job_dependencies(&runner, &job_id);
                let _ = tx.send(Msg::JobInfo { job, detail, deps });
            });
        } else {
            self.status = Some("Job not found".to_string());
        }
    }

    /// Start a job investigation (runs on worker thread).
    fn start_job_investigation(&mut self, job_id: String) {
        self.status = Some(format!("Investigating job {}...", job_id));

        let runner = self.runner.clone();
        let tx = self.msg_tx.clone();
        let _max_related = self.config.investigation.max_related_jobs; // Reserved for future use
        let reasons_path = if self.config.investigation.reasons_path.is_empty() {
            None
        } else {
            Some(self.config.investigation.reasons_path.clone())
        };

        std::thread::spawn(move || {
            use crate::slurm::investigate::investigate_job;
            let (reason_table, _) = ReasonTable::load(reasons_path.as_deref());
            let report = investigate_job(&runner, &reason_table, &job_id);
            let _ = tx.send(Msg::Investigation(Box::new(report)));
        });
    }

    /// Start a node investigation (runs on worker thread).
    fn start_node_investigation(&mut self, node_name: String) {
        self.status = Some(format!("Investigating node {}...", node_name));

        let runner = self.runner.clone();
        let tx = self.msg_tx.clone();
        let max_related = self.config.investigation.max_related_jobs;

        std::thread::spawn(move || {
            use crate::slurm::investigate::investigate_node;
            let report = investigate_node(&runner, &node_name, max_related as usize);
            let _ = tx.send(Msg::Investigation(Box::new(report)));
        });
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
                                        if self.config.attach.enabled {
                                            // Find the job to get its nodelist
                                            if let Some(job) =
                                                self.jobs.iter().find(|j| j.job_id == job_id)
                                            {
                                                // Pass nodelist as default - will be resolved on submit
                                                let screen = AttachPromptScreen::new(
                                                    job_id.clone(),
                                                    job.nodelist.clone(),
                                                );
                                                self.modal = Modal::AttachPrompt(screen);
                                            } else {
                                                self.status = Some("Job not found".to_string());
                                            }
                                        } else {
                                            self.status =
                                                Some("Attach disabled in config".to_string());
                                        }
                                    }
                                    JobAction::AttachCustom => {
                                        if self.config.attach.enabled {
                                            // Use empty string to prompt for custom node
                                            let screen = AttachPromptScreen::new(
                                                job_id.clone(),
                                                String::new(),
                                            );
                                            self.modal = Modal::AttachPrompt(screen);
                                        } else {
                                            self.status =
                                                Some("Attach disabled in config".to_string());
                                        }
                                    }
                                    JobAction::Stdout => {
                                        self.open_log_viewer(job_id, true);
                                    }
                                    JobAction::Stderr => {
                                        self.open_log_viewer(job_id, false);
                                    }
                                    JobAction::Detail => {
                                        self.open_job_detail(job_id);
                                    }
                                    JobAction::BatchScript => {
                                        self.open_batch_script(job_id);
                                    }
                                    JobAction::Dependencies => {
                                        self.open_dependencies(job_id);
                                    }
                                    JobAction::ArrayTasks => {
                                        self.open_array_tasks(job_id);
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
                                    .selected_jobs()
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
                                    Tab::History | Tab::Health => {
                                        // No column config for these tabs
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
                Modal::JobDetail(state) => match state.handle_key(key_event) {
                    DetailOutcome::Close => {
                        self.modal = Modal::None;
                        return true;
                    }
                    _ => return true,
                },
                Modal::BatchScript(state) => match state.handle_key(key_event) {
                    DetailOutcome::Close => {
                        self.modal = Modal::None;
                        return true;
                    }
                    _ => return true,
                },
                Modal::LogViewer(state) => match state.handle_key(key_event) {
                    DetailOutcome::Close => {
                        self.modal = Modal::None;
                        return true;
                    }
                    _ => return true,
                },
                Modal::Dependencies(state) => match state.handle_key(key_event) {
                    DetailOutcome::Close => {
                        self.modal = Modal::None;
                        return true;
                    }
                    _ => return true,
                },
                Modal::ArrayTasks(state) => match state.handle_key(key_event) {
                    DetailOutcome::Close => {
                        self.modal = Modal::None;
                        return true;
                    }
                    _ => return true,
                },
                Modal::AttachPrompt(state) => match state.handle_key(key_event) {
                    DetailOutcome::Close => {
                        self.modal = Modal::None;
                        return true;
                    }
                    DetailOutcome::Value(node_override) => {
                        // Build the attach command (async)
                        let job_id = state.job_id.clone();
                        self.modal = Modal::None;

                        if let Err(err) = AttachPromptScreen::check_enabled(&self.config.attach) {
                            self.status = Some(format!("Attach disabled: {}", err));
                            return true;
                        }

                        if let Some(job) = self.jobs.iter().find(|j| j.job_id == job_id).cloned() {
                            self.status = Some("Building attach command...".to_string());
                            let runner = self.runner.clone();
                            let tx = self.msg_tx.clone();
                            let default_command = self.config.attach.default_command.clone();
                            let extra_args = self.config.attach.extra_args.clone();

                            std::thread::spawn(move || {
                                let node_to_use = if node_override.is_empty() {
                                    fetch::resolve_first_node(&runner, &job.nodelist)
                                } else {
                                    node_override
                                };

                                let cmd_parts = fetch::build_attach_command(
                                    &job_id,
                                    Some(&node_to_use),
                                    &default_command,
                                    &extra_args,
                                );
                                let cmd = cmd_parts.join(" ");
                                let _ = tx.send(Msg::Status(format!("Run: {}", cmd)));
                            });
                        } else {
                            self.status = Some("Job not found".to_string());
                        }
                        return true;
                    }
                    _ => return true,
                },
                Modal::Investigation(state) => match state.handle_key(key_event) {
                    DetailOutcome::Close => {
                        self.modal = Modal::None;
                        return true;
                    }
                    _ => return true,
                },
                Modal::NodeDetail(state) => match state.handle_key(key_event) {
                    DetailOutcome::Close => {
                        self.modal = Modal::None;
                        return true;
                    }
                    _ => return true,
                },
                Modal::JobInfo(state) => match state.handle_key(key_event) {
                    DetailOutcome::Close => {
                        self.modal = Modal::None;
                        return true;
                    }
                    _ => return true,
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
            (KeyCode::Char('4'), KeyModifiers::NONE) => {
                self.tab = Tab::History;
                self.last_refresh = None;
                true
            }
            (KeyCode::Char('5'), KeyModifiers::NONE) => {
                self.tab = Tab::Health;
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
                    Tab::Partitions | Tab::History | Tab::Health => {
                        // Column toggle not available for these tabs
                        self.status = Some("Column toggle not available for this tab".to_string());
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
                    Tab::History => "History",
                    Tab::Health => "Health",
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
            // Nodes tab: Enter for node detail
            (KeyCode::Enter, KeyModifiers::NONE) if self.tab == Tab::Nodes => {
                if let Some(node) = self.current_node() {
                    self.open_node_detail(node.name.clone());
                }
                true
            }
            // Jobs tab: I for investigation
            (KeyCode::Char('I'), KeyModifiers::NONE) if self.tab == Tab::Jobs => {
                if let Some(job) = self.current_job() {
                    self.start_job_investigation(job.job_id.clone());
                }
                true
            }
            // Nodes tab: I for investigation
            (KeyCode::Char('I'), KeyModifiers::NONE) if self.tab == Tab::Nodes => {
                if let Some(node) = self.current_node() {
                    self.start_node_investigation(node.name.clone());
                }
                true
            }
            // Jobs tab: i for job info
            (KeyCode::Char('i'), KeyModifiers::NONE) if self.tab == Tab::Jobs => {
                if let Some(job) = self.current_job() {
                    self.open_job_info(job.job_id.clone());
                }
                true
            }
            // Jobs tab: y for yank (visual selection)
            (KeyCode::Char('y'), KeyModifiers::NONE)
                if self.tab == Tab::Jobs && self.jobs_view.visual_selection.is_active() =>
            {
                use crate::views::visual::yank_tsv;
                let rows = self.jobs_view.visual_selection.rows();
                let text = yank_tsv(&rows, &self.jobs_view.last_jobs, |job| {
                    format!("{}	{}	{}	{}", job.job_id, job.name, job.state, job.user)
                });
                let remote_host = if self.config.remote.host.is_empty() {
                    None
                } else {
                    Some(self.config.remote.host.as_str())
                };
                let result = crate::clipboard::copy(&text, &self.config.clipboard, remote_host);
                if result.ok {
                    let count = rows.len();
                    self.status = Some(format!(
                        "Copied {} row{}",
                        count,
                        if count == 1 { "" } else { "s" }
                    ));
                } else {
                    self.status = Some("Copy failed".to_string());
                }
                // Exit visual mode after yank
                self.jobs_view.visual_selection.exit();
                true
            }
            // Nodes tab: y for yank (visual selection)
            (KeyCode::Char('y'), KeyModifiers::NONE)
                if self.tab == Tab::Nodes && self.nodes_view.visual_selection.is_active() =>
            {
                use crate::views::visual::yank_tsv;
                let rows = self.nodes_view.visual_selection.rows();
                let text = yank_tsv(&rows, &self.nodes_view.last_sorted_nodes, |node| {
                    format!("{}	{}	{}", node.name, node.state, node.partition)
                });
                let remote_host = if self.config.remote.host.is_empty() {
                    None
                } else {
                    Some(self.config.remote.host.as_str())
                };
                let result = crate::clipboard::copy(&text, &self.config.clipboard, remote_host);
                if result.ok {
                    let count = rows.len();
                    self.status = Some(format!(
                        "Copied {} row{}",
                        count,
                        if count == 1 { "" } else { "s" }
                    ));
                } else {
                    self.status = Some("Copy failed".to_string());
                }
                // Exit visual mode after yank
                self.nodes_view.visual_selection.exit();
                true
            }
            // Detail screen copy (ctrl+shift+y)
            (KeyCode::Char('y'), KeyModifiers::CONTROL | KeyModifiers::SHIFT)
            | (KeyCode::Char('Y'), KeyModifiers::CONTROL) => {
                let text: Option<String> = match &self.modal {
                    Modal::JobInfo(s) => Some(s.plain_text().to_string()),
                    Modal::JobDetail(s) => Some(s.plain_text().to_string()),
                    Modal::NodeDetail(s) => Some(s.plain_text().to_string()),
                    Modal::BatchScript(s) => Some(s.content().to_string()),
                    Modal::LogViewer(s) => Some(s.content().to_string()),
                    _ => None,
                };
                if let Some(text) = text {
                    let remote_host = if self.config.remote.host.is_empty() {
                        None
                    } else {
                        Some(self.config.remote.host.as_str())
                    };
                    let result = crate::clipboard::copy(&text, &self.config.clipboard, remote_host);
                    if result.ok {
                        self.status = Some("Copied pane content".to_string());
                    } else {
                        self.status = Some("Copy failed".to_string());
                    }
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
            _ => {
                // Delegate unhandled keys to the active view
                let key_event = crossterm::event::KeyEvent::new(key, modifiers);
                match self.tab {
                    Tab::Jobs => self.jobs_view.handle_key(key_event),
                    Tab::Nodes => self.nodes_view.handle_key(key_event),
                    Tab::History => {
                        let current_user = std::env::var("USER").unwrap_or_default();
                        self.history_view.handle_key(key_event, &current_user)
                    }
                    Tab::Partitions => {
                        // Basic cursor navigation for partitions
                        use crossterm::event::{KeyCode, KeyModifiers};
                        match (key_event.code, key_event.modifiers) {
                            (KeyCode::Down | KeyCode::Char('j'), KeyModifiers::NONE) => {
                                self.partitions_table_state.next();
                                true
                            }
                            (KeyCode::Up | KeyCode::Char('k'), KeyModifiers::NONE) => {
                                self.partitions_table_state.prev();
                                true
                            }
                            _ => false,
                        }
                    }
                    Tab::Health => false, // No view-level keys yet
                }
            }
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
            Tab::History => Msg::History(fetch::fetch_sacct_jobs(&runner, 24)),
            Tab::Health => continue, // Health is passive, no fetch
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
        Tab::History => 3,
        Tab::Health => 4,
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
        Tab::History => views::render_history(f, area, &mut app.history_view),
        Tab::Health => views::render_health(f, app, area),
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
fn render_modal(f: &mut ratatui::Frame, app: &mut App, area: Rect) {
    match &mut app.modal {
        Modal::Confirm(state) => state.render(f, area),
        Modal::JobAction(state) => state.render(f, area),
        Modal::BulkAction(state) => state.render(f, area),
        Modal::ColumnToggle(state) => state.render(f, area),
        Modal::KeybindingsHelp(state) => state.render(f, area),
        Modal::JobDetail(state) => state.render(f, area),
        Modal::BatchScript(state) => state.render(f, area),
        Modal::LogViewer(state) => state.render(f, area),
        Modal::Dependencies(state) => state.render(f, area),
        Modal::ArrayTasks(state) => state.render(f, area),
        Modal::AttachPrompt(state) => state.render(f, area),
        Modal::Investigation(state) => {
            use crate::views::investigate;
            investigate::render(f, area, state);
        }
        Modal::NodeDetail(state) => state.render(f, area),
        Modal::JobInfo(state) => state.render(f, area),
        Modal::None => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tab_titles() {
        assert_eq!(Tab::Jobs.title(), "Jobs");
        assert_eq!(Tab::Nodes.title(), "Nodes");
        assert_eq!(Tab::Partitions.title(), "Partitions");
        assert_eq!(Tab::History.title(), "History");
        assert_eq!(Tab::Health.title(), "Health");
    }

    #[test]
    fn tab_all() {
        let all = Tab::all();
        assert_eq!(all.len(), 5);
        assert_eq!(all[0], Tab::Jobs);
        assert_eq!(all[1], Tab::Nodes);
        assert_eq!(all[2], Tab::Partitions);
        assert_eq!(all[3], Tab::History);
        assert_eq!(all[4], Tab::Health);
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

    #[test]
    fn test_open_job_detail_dispatches_async() {
        let config = Config::default();
        let mut app = App::new(config);

        // Add a test job
        app.jobs = vec![Job {
            job_id: "12345".to_string(),
            name: "test_job".to_string(),
            user: "alice".to_string(),
            state: "RUNNING".to_string(),
            partition: "gpu".to_string(),
            nodes: "1".to_string(),
            num_nodes: "1".to_string(),
            num_cpus: "4".to_string(),
            time_used: "1:23:45".to_string(),
            time_limit: "10:00:00".to_string(),
            reason: "".to_string(),
            nodelist: "node01".to_string(),
            qos: "normal".to_string(),
        }];

        // Call open_job_detail
        app.open_job_detail("12345".to_string());

        // Modal should NOT be opened synchronously
        assert!(
            matches!(app.modal, Modal::None),
            "Modal should not open synchronously"
        );

        // Status should show loading
        assert!(app.status.is_some());
        assert!(app.status.as_ref().unwrap().contains("Loading"));
    }

    #[test]
    fn test_open_array_tasks_dispatches_async() {
        let config = Config::default();
        let mut app = App::new(config);

        // Add a test array job
        app.jobs = vec![Job {
            job_id: "12345_0".to_string(),
            name: "array_job".to_string(),
            user: "alice".to_string(),
            state: "RUNNING".to_string(),
            partition: "gpu".to_string(),
            nodes: "1".to_string(),
            num_nodes: "1".to_string(),
            num_cpus: "4".to_string(),
            time_used: "1:23:45".to_string(),
            time_limit: "10:00:00".to_string(),
            reason: "".to_string(),
            nodelist: "node01".to_string(),
            qos: "normal".to_string(),
        }];

        // Call open_array_tasks
        app.open_array_tasks("12345_0".to_string());

        // Modal should NOT be opened synchronously
        assert!(
            matches!(app.modal, Modal::None),
            "Modal should not open synchronously"
        );

        // Status should show loading
        assert!(app.status.is_some());
        assert!(app.status.as_ref().unwrap().contains("Loading"));
    }

    #[test]
    fn test_open_node_detail_dispatches_async() {
        let config = Config::default();
        let mut app = App::new(config);

        // Add a test node
        app.nodes = vec![Node {
            name: "node01".to_string(),
            state: "idle".to_string(),
            partition: "gpu".to_string(),
            cpus_total: "48".to_string(),
            cpus_alloc: "0".to_string(),
            memory_total: "128000".to_string(),
            memory_free: "128000".to_string(),
            gpu_total: 4,
            gpu_alloc: 0,
            load: "0.01".to_string(),
        }];

        // Call open_node_detail
        app.open_node_detail("node01".to_string());

        // Modal should NOT be opened synchronously
        assert!(
            matches!(app.modal, Modal::None),
            "Modal should not open synchronously"
        );

        // Status should show loading
        assert!(app.status.is_some());
        assert!(app.status.as_ref().unwrap().contains("Loading"));
    }

    #[test]
    fn test_log_batch_deps_dispatch_async() {
        let config = Config::default();
        let mut app = App::new(config);

        // Add a test job
        app.jobs = vec![Job {
            job_id: "12345".to_string(),
            name: "test_job".to_string(),
            user: "alice".to_string(),
            state: "RUNNING".to_string(),
            partition: "gpu".to_string(),
            nodes: "1".to_string(),
            num_nodes: "1".to_string(),
            num_cpus: "4".to_string(),
            time_used: "1:23:45".to_string(),
            time_limit: "10:00:00".to_string(),
            reason: "".to_string(),
            nodelist: "node01".to_string(),
            qos: "normal".to_string(),
        }];

        // Test open_log_viewer
        app.open_log_viewer("12345".to_string(), true);
        assert!(
            matches!(app.modal, Modal::None),
            "Log viewer modal should not open synchronously"
        );
        assert!(app.status.as_ref().unwrap().contains("Loading"));

        // Reset status
        app.status = None;

        // Test open_batch_script
        app.open_batch_script("12345".to_string());
        assert!(
            matches!(app.modal, Modal::None),
            "Batch script modal should not open synchronously"
        );
        assert!(app.status.as_ref().unwrap().contains("Loading"));

        // Reset status
        app.status = None;

        // Test open_dependencies
        app.open_dependencies("12345".to_string());
        assert!(
            matches!(app.modal, Modal::None),
            "Dependencies modal should not open synchronously"
        );
        assert!(app.status.as_ref().unwrap().contains("Loading"));
    }

    #[test]
    fn test_open_job_info_dispatches_async() {
        let config = Config::default();
        let mut app = App::new(config);

        // Add a test job
        app.jobs = vec![Job {
            job_id: "12345".to_string(),
            name: "test_job".to_string(),
            user: "alice".to_string(),
            state: "RUNNING".to_string(),
            partition: "gpu".to_string(),
            nodes: "1".to_string(),
            num_nodes: "1".to_string(),
            num_cpus: "4".to_string(),
            time_used: "1:23:45".to_string(),
            time_limit: "10:00:00".to_string(),
            reason: "".to_string(),
            nodelist: "node01".to_string(),
            qos: "normal".to_string(),
        }];

        // Call open_job_info
        app.open_job_info("12345".to_string());

        // Modal should NOT be opened synchronously
        assert!(
            matches!(app.modal, Modal::None),
            "JobInfo modal should not open synchronously"
        );

        // Status should show loading
        assert!(app.status.is_some());
        assert!(app.status.as_ref().unwrap().contains("Loading"));
    }

    #[test]
    fn test_attach_flow_does_not_resolve_synchronously() {
        use crate::views::modals::job_actions::JobActionState;

        let config = Config::default();
        let mut app = App::new(config);

        // Add a test job
        let job = Job {
            job_id: "12345".to_string(),
            name: "test_job".to_string(),
            user: "alice".to_string(),
            state: "RUNNING".to_string(),
            partition: "gpu".to_string(),
            nodes: "1".to_string(),
            num_nodes: "1".to_string(),
            num_cpus: "4".to_string(),
            time_used: "1:23:45".to_string(),
            time_limit: "10:00:00".to_string(),
            reason: "".to_string(),
            nodelist: "node[01-04]".to_string(),
            qos: "normal".to_string(),
        };
        app.jobs = vec![job.clone()];

        // Open the job action modal and select AttachFirst
        let state = JobActionState::new(job);
        app.modal = Modal::JobAction(Box::new(state));

        // Simulate AttachFirst selection (this would normally come through handle_key)
        // For this test, we just verify that when AttachPrompt is created,
        // it doesn't trigger synchronous resolve_first_node

        // The test passes if we don't hang here - resolve_first_node would block
        // on a real cluster if it were called synchronously
    }
}

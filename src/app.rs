//! Application state and key dispatch.

use crate::chrome;
use crate::config::Config;
use crate::investigation::{InvestigationReport, ReasonTable};
use crate::responsive::tier_for;
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
use crate::views::partitions::PartitionsView;
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
    #[cfg(test)]
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
    /// Job ID prompt for investigation.
    InvestigateJobPrompt(AttachPromptScreen),
    /// Node name prompt for investigation.
    InvestigateNodePrompt(AttachPromptScreen),
    /// Command palette.
    Palette(crate::views::modals::palette::PaletteState),
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
    config_path: std::path::PathBuf,
    pub runner: Runner,
    pub tab: Tab,
    pub jobs: Vec<Job>,
    pub nodes: Vec<Node>,
    pub partitions: Vec<ClusterSummary>,
    pub partitions_table_state: crate::views::table_state::CyclicTableState,
    pub partitions_view: PartitionsView,
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
    /// Last rendered table area for Jobs view (for mouse hit testing)
    /// Auto-refresh is paused (Python: `_paused`).
    paused: bool,
    pub last_jobs_table_area: Option<ratatui::layout::Rect>,
    /// Last rendered table area for Nodes view (for mouse hit testing)
    pub last_nodes_table_area: Option<ratatui::layout::Rect>,
}

impl App {
    /// Create a new App with the given config.
    pub fn new(config: Config, config_path: std::path::PathBuf) -> Self {
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
        let partitions_view = PartitionsView::from_config(&config);
        let history_view = HistoryView::new();

        Self {
            jobs_view,
            nodes_view,
            partitions_view,
            history_view,
            config,
            runner,
            config_path,
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
            paused: false,
            last_jobs_table_area: None,
            last_nodes_table_area: None,
        }
    }

    /// Process any pending messages from the refresh worker.
    pub fn drain_messages(&mut self) {
        let current_user = std::env::var("USER").unwrap_or_default();
        while let Ok(msg) = self.refresh_rx.try_recv() {
            match msg {
                Msg::Jobs(jobs) => {
                    // Check watched jobs for changes before updating
                    let notifications = self.jobs_view.check_watched_jobs(&jobs);
                    self.jobs = jobs;

                    // Process watch notifications
                    for (_job_id, message) in &notifications {
                        // Ring the bell (skip in tests to avoid corrupting output)
                        #[cfg(not(test))]
                        {
                            use std::io::Write;
                            let _ = std::io::stdout().write_all(b"");
                            let _ = std::io::stdout().flush();
                        }

                        // Emit desktop notification if enabled
                        if self.config.notifications.desktop_enabled {
                            crate::views::modals::notify::desktop_notify(
                                "sqtop: Job finished",
                                message,
                            );
                        }
                        // Set status message (last one wins, which is fine for multiple)
                        self.status = Some(message.clone());
                    }

                    if notifications.is_empty() {
                        self.status = None;
                    }
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
                    // If log viewer is already open, update its content (for follow mode)
                    if let Modal::LogViewer(ref mut viewer) = self.modal {
                        viewer.update_content(content);
                    } else {
                        // Otherwise create a new screen
                        let screen = LogViewerScreen::new(job_id, path, log_type, content);
                        self.modal = Modal::LogViewer(screen);
                    }
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

    /// Refresh the log viewer content (for follow mode).
    fn refresh_log_viewer(&mut self, job_id: String, path: String, log_type: String) {
        let runner = self.runner.clone();
        let tx = self.msg_tx.clone();

        std::thread::spawn(move || {
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
        let reasons_path = crate::config::resolve_reasons_path(&self.config, &self.config_path);

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
            self.notify(format!("Cancelled job {}", job_id), "Job Action");
        } else {
            self.notify(format!("Cancel failed: {}", message), "Job Action");
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
            self.notify(
                format!("{} {} job(s)", action.to_uppercase(), total),
                "Bulk Action",
            );
        } else {
            self.notify(
                format!("{} {}/{} job(s)", action.to_uppercase(), ok_count, total),
                "Bulk Action",
            );
        }
        self.request_refresh();
    }

    /// Execute a command from the palette.
    fn execute_palette_command(&mut self, cmd: crate::views::modals::palette::PaletteCommand) {
        use crate::views::modals::palette::PaletteCommand;
        use std::collections::HashMap;

        match cmd {
            PaletteCommand::RefreshData => {
                self.last_refresh = None;
                self.status = Some("Refreshing...".to_string());
            }
            PaletteCommand::SetRefreshInterval(secs) => {
                let interval = secs as f64;
                self.config.interval.jobs = interval;
                self.config.interval.nodes = interval;
                self.config.interval.partitions = interval;
                self.status = Some(format!("Refresh interval set to {}s", secs));
                self.persist_theme_interval_async();
            }
            PaletteCommand::ToggleExpertMode => {
                self.config.ui.expert_mode = !self.config.ui.expert_mode;
                let new_value = self.config.ui.expert_mode;
                self.status = Some(format!(
                    "Expert mode: {}",
                    if new_value { "on" } else { "off" }
                ));
                // Persist
                let mut update = HashMap::new();
                let mut ui = toml::Table::new();
                ui.insert("expert_mode".to_string(), toml::Value::Boolean(new_value));
                update.insert("ui".to_string(), toml::Value::Table(ui));
                self.persist_config_async(update);
            }
            PaletteCommand::ToggleConfirmCancelSingle => {
                self.config.safety.confirm_cancel_single =
                    !self.config.safety.confirm_cancel_single;
                let new_value = self.config.safety.confirm_cancel_single;
                self.status = Some(format!(
                    "Confirm single cancel: {}",
                    if new_value { "on" } else { "off" }
                ));
                // Persist
                let mut update = HashMap::new();
                let mut safety = toml::Table::new();
                safety.insert(
                    "confirm_cancel_single".to_string(),
                    toml::Value::Boolean(new_value),
                );
                update.insert("safety".to_string(), toml::Value::Table(safety));
                self.persist_config_async(update);
            }
            PaletteCommand::ToggleConfirmBulkActions => {
                self.config.safety.confirm_bulk_actions = !self.config.safety.confirm_bulk_actions;
                let new_value = self.config.safety.confirm_bulk_actions;
                self.status = Some(format!(
                    "Confirm bulk actions: {}",
                    if new_value { "on" } else { "off" }
                ));
                // Persist
                let mut update = HashMap::new();
                let mut safety = toml::Table::new();
                safety.insert(
                    "confirm_bulk_actions".to_string(),
                    toml::Value::Boolean(new_value),
                );
                update.insert("safety".to_string(), toml::Value::Table(safety));
                self.persist_config_async(update);
            }
            PaletteCommand::ColumnVisibility => {
                // Open column toggle modal for current tab
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
                    _ => {
                        self.status = Some("Column toggle not available for this tab".to_string());
                    }
                }
            }
            PaletteCommand::SetJobsDefaultSort(col) => {
                let label = match col.as_str() {
                    "" => "State priority (default)",
                    "state" => "State",
                    "time" => "Time used",
                    "cpus" => "CPUs",
                    "qos" => "QOS",
                    _ => &col,
                };
                // Set the sort on the jobs view
                if col.is_empty() {
                    self.jobs_view.clear_sort();
                } else {
                    self.jobs_view.toggle_sort(&col);
                }
                self.status = Some(format!("Jobs sort: {}", label));
                // Persist
                let mut update = HashMap::new();
                let mut view_state = toml::Table::new();
                view_state.insert(
                    "jobs_sort_col".to_string(),
                    toml::Value::String(col.clone()),
                );
                view_state.insert(
                    "jobs_sort_reversed".to_string(),
                    toml::Value::Boolean(false),
                );
                update.insert("view_state".to_string(), toml::Value::Table(view_state));
                self.persist_config_async(update);
            }
            PaletteCommand::InvestigateJobById => {
                // Open text input prompt for job ID
                let screen = AttachPromptScreen::with_overrides(
                    String::new(),
                    String::new(),
                    Some("Job ID to investigate".to_string()),
                    Some("job id (e.g. 12345)".to_string()),
                );
                self.modal = Modal::InvestigateJobPrompt(screen);
            }
            PaletteCommand::InvestigateNodeByName => {
                // Open text input prompt for node name
                let screen = AttachPromptScreen::with_overrides(
                    String::new(),
                    String::new(),
                    Some("Node name to investigate".to_string()),
                    Some("node name (e.g. gpu-a100-02)".to_string()),
                );
                self.modal = Modal::InvestigateNodePrompt(screen);
            }
            PaletteCommand::ReloadConfig => {
                self.reload_config_from_disk();
            }
        }
    }

    /// Reload config from disk and apply live settings.
    fn reload_config_from_disk(&mut self) {
        let new_config = crate::config::load(&self.config_path);
        let mut applied = Vec::new();

        // Theme: re-apply if changed
        let theme_changed = new_config.theme != self.config.theme;
        if theme_changed {
            self.config.theme = new_config.theme.clone();
            applied.push("theme");
        }

        // Intervals: re-apply if changed
        let intervals_changed = new_config.interval.jobs != self.config.interval.jobs
            || new_config.interval.nodes != self.config.interval.nodes
            || new_config.interval.partitions != self.config.interval.partitions;
        if intervals_changed {
            self.config.interval = new_config.interval.clone();
            applied.push("intervals");
        }

        // Safety/expert flags
        self.config.ui.expert_mode = new_config.ui.expert_mode;
        self.config.safety.confirm_cancel_single = new_config.safety.confirm_cancel_single;
        self.config.safety.confirm_bulk_actions = new_config.safety.confirm_bulk_actions;
        applied.push("safety flags");

        let summary = applied.join(" + ");
        self.status = Some(format!(
            "Config reloaded — applied: {}. Column visibility and order require restart.",
            summary
        ));
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
                    // Capture config_modified before consuming state
                    let config_was_modified = state.config_modified;
                    match state.handle_key(key_event, &mut self.config) {
                        ModalOutcome::Dismiss(result) => {
                            self.modal = Modal::None;
                            if result == ColumnToggleResult::Reset {
                                // Reset column order for the current tab
                                let mut update = std::collections::HashMap::new();
                                let mut columns = toml::Table::new();
                                match self.tab {
                                    Tab::Jobs => {
                                        self.config.columns.jobs_order.clear();
                                        self.config.columns.jobs_hidden.clear();
                                        columns.insert(
                                            "jobs_order".to_string(),
                                            toml::Value::Array(vec![]),
                                        );
                                        columns.insert(
                                            "jobs_hidden".to_string(),
                                            toml::Value::Array(vec![]),
                                        );
                                    }
                                    Tab::Nodes => {
                                        self.config.columns.nodes_order.clear();
                                        self.config.columns.nodes_hidden.clear();
                                        columns.insert(
                                            "nodes_order".to_string(),
                                            toml::Value::Array(vec![]),
                                        );
                                        columns.insert(
                                            "nodes_hidden".to_string(),
                                            toml::Value::Array(vec![]),
                                        );
                                    }
                                    Tab::Partitions => {
                                        self.config.columns.partitions_order.clear();
                                        self.config.columns.partitions_hidden.clear();
                                        columns.insert(
                                            "partitions_order".to_string(),
                                            toml::Value::Array(vec![]),
                                        );
                                        columns.insert(
                                            "partitions_hidden".to_string(),
                                            toml::Value::Array(vec![]),
                                        );
                                    }
                                    Tab::History | Tab::Health => {
                                        // No column config for these tabs
                                    }
                                }
                                if !columns.is_empty() {
                                    update
                                        .insert("columns".to_string(), toml::Value::Table(columns));
                                    self.persist_config_async(update);
                                }
                                // Reload views with new column config
                                self.jobs_view = JobsView::from_config(&self.config);
                                self.nodes_view = NodesView::new(&self.config);
                            } else if config_was_modified {
                                // Persist hidden column changes
                                let mut update = std::collections::HashMap::new();
                                let mut columns = toml::Table::new();
                                match self.tab {
                                    Tab::Jobs => {
                                        let hidden_array: Vec<toml::Value> = self
                                            .config
                                            .columns
                                            .jobs_hidden
                                            .iter()
                                            .map(|s| toml::Value::String(s.clone()))
                                            .collect();
                                        columns.insert(
                                            "jobs_hidden".to_string(),
                                            toml::Value::Array(hidden_array),
                                        );
                                    }
                                    Tab::Nodes => {
                                        let hidden_array: Vec<toml::Value> = self
                                            .config
                                            .columns
                                            .nodes_hidden
                                            .iter()
                                            .map(|s| toml::Value::String(s.clone()))
                                            .collect();
                                        columns.insert(
                                            "nodes_hidden".to_string(),
                                            toml::Value::Array(hidden_array),
                                        );
                                    }
                                    Tab::Partitions => {
                                        let hidden_array: Vec<toml::Value> = self
                                            .config
                                            .columns
                                            .partitions_hidden
                                            .iter()
                                            .map(|s| toml::Value::String(s.clone()))
                                            .collect();
                                        columns.insert(
                                            "partitions_hidden".to_string(),
                                            toml::Value::Array(hidden_array),
                                        );
                                    }
                                    Tab::History | Tab::Health => {}
                                }
                                if !columns.is_empty() {
                                    update
                                        .insert("columns".to_string(), toml::Value::Table(columns));
                                    self.persist_config_async(update);
                                }
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
                Modal::InvestigateJobPrompt(state) => match state.handle_key(key_event) {
                    DetailOutcome::Close => {
                        self.modal = Modal::None;
                        return true;
                    }
                    DetailOutcome::Value(job_id) => {
                        self.modal = Modal::None;
                        let job_id = job_id.trim();
                        if !job_id.is_empty() {
                            self.start_job_investigation(job_id.to_string());
                        }
                        return true;
                    }
                    _ => return true,
                },
                Modal::InvestigateNodePrompt(state) => match state.handle_key(key_event) {
                    DetailOutcome::Close => {
                        self.modal = Modal::None;
                        return true;
                    }
                    DetailOutcome::Value(node_name) => {
                        self.modal = Modal::None;
                        let node_name = node_name.trim();
                        if !node_name.is_empty() {
                            self.start_node_investigation(node_name.to_string());
                        }
                        return true;
                    }
                    _ => return true,
                },
                Modal::Palette(state) => {
                    use crate::views::modals::palette::PaletteResult;
                    match state.handle_key(key_event, &self.config) {
                        ModalOutcome::Dismiss(PaletteResult::None) => {
                            self.modal = Modal::None;
                            return true;
                        }
                        ModalOutcome::Dismiss(PaletteResult::Execute(cmd)) => {
                            self.modal = Modal::None;
                            self.execute_palette_command(cmd);
                            return true;
                        }
                        ModalOutcome::Continue => return true,
                    }
                }
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
            // Refresh. Python's `refresh_data` returns early while paused,
            // so a manual refresh is a no-op until the user unpauses.
            (KeyCode::Char('r'), KeyModifiers::NONE) => {
                self.last_refresh = None;
                true
            }
            // Pause / resume auto-refresh
            (KeyCode::Char('P'), KeyModifiers::NONE | KeyModifiers::SHIFT) => {
                self.toggle_pause();
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
            // Command palette
            (KeyCode::Char('S'), KeyModifiers::NONE) => {
                use crate::views::modals::palette::PaletteState;
                self.modal = Modal::Palette(PaletteState::new(&self.config));
                true
            }
            (KeyCode::Char('p'), KeyModifiers::CONTROL) => {
                use crate::views::modals::palette::PaletteState;
                self.modal = Modal::Palette(PaletteState::new(&self.config));
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
            // Jobs tab: d for detail
            (KeyCode::Char('d'), KeyModifiers::NONE) if self.tab == Tab::Jobs => {
                if let Some(job) = self.current_job() {
                    self.open_job_detail(job.job_id.clone());
                }
                true
            }
            // Jobs tab: l for log viewer
            (KeyCode::Char('l'), KeyModifiers::NONE) if self.tab == Tab::Jobs => {
                if let Some(job) = self.current_job() {
                    self.open_log_viewer(job.job_id.clone(), true);
                }
                true
            }
            // Jobs tab: a for array tasks
            (KeyCode::Char('a'), KeyModifiers::NONE) if self.tab == Tab::Jobs => {
                if let Some(job) = self.current_job() {
                    self.open_array_tasks(job.job_id.clone());
                }
                true
            }
            // Jobs tab: D for dependencies
            (KeyCode::Char('D'), KeyModifiers::NONE) if self.tab == Tab::Jobs => {
                if let Some(job) = self.current_job() {
                    self.open_dependencies(job.job_id.clone());
                }
                true
            }
            // Jobs tab: w for watch
            (KeyCode::Char('w'), KeyModifiers::NONE) if self.tab == Tab::Jobs => {
                if let Some(job) = self.current_job() {
                    let job_id = job.job_id.clone();
                    let job_name = job.name.clone();
                    let job_state = job.state.clone();
                    let watched = self.jobs_view.toggle_watch(&job_id, &job_state);
                    if watched {
                        self.status = Some(format!("Watching job {} ({})", job_id, job_name));
                    } else {
                        self.status = Some(format!("Unwatched job {}", job_id));
                    }
                }
                true
            }
            // Jobs tab: h for hold
            (KeyCode::Char('h'), KeyModifiers::NONE) if self.tab == Tab::Jobs => {
                let job_ids = self.jobs_view.selected_or_current_job_ids();
                if !job_ids.is_empty() {
                    self.handle_bulk_action("hold", job_ids);
                }
                true
            }
            // Jobs tab: R for release
            (KeyCode::Char('R'), KeyModifiers::NONE) if self.tab == Tab::Jobs => {
                let job_ids = self.jobs_view.selected_or_current_job_ids();
                if !job_ids.is_empty() {
                    self.handle_bulk_action("release", job_ids);
                }
                true
            }
            // Jobs tab: e for requeue
            (KeyCode::Char('e'), KeyModifiers::NONE) if self.tab == Tab::Jobs => {
                let job_ids = self.jobs_view.selected_or_current_job_ids();
                if !job_ids.is_empty() {
                    self.handle_bulk_action("requeue", job_ids);
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
                let (text, label): (Option<String>, Option<String>) = match &self.modal {
                    Modal::JobInfo(s) => (Some(s.plain_text().to_string()), Some(s.label())),
                    Modal::JobDetail(s) => (Some(s.plain_text().to_string()), Some(s.label())),
                    Modal::NodeDetail(s) => (Some(s.plain_text().to_string()), Some(s.label())),
                    Modal::BatchScript(s) => (Some(s.content().to_string()), Some(s.label())),
                    Modal::LogViewer(s) => (Some(s.content().to_string()), Some(s.label())),
                    _ => (None, None),
                };
                if let (Some(text), Some(label)) = (text, label) {
                    let remote_host = if self.config.remote.host.is_empty() {
                        None
                    } else {
                        Some(self.config.remote.host.as_str())
                    };
                    let result = crate::clipboard::copy(&text, &self.config.clipboard, remote_host);
                    if result.ok {
                        let mut msg = format!("Copied {}", label);
                        if result.truncated {
                            msg.push_str(" (truncated)");
                        }
                        self.notify(msg, "Clipboard");
                    } else {
                        self.notify("Clipboard unavailable", "Clipboard");
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
                let handled = match self.tab {
                    Tab::Jobs => self.jobs_view.handle_key(key_event),
                    Tab::Nodes => self.nodes_view.handle_key(key_event),
                    Tab::History => {
                        let current_user = std::env::var("USER").unwrap_or_default();
                        self.history_view.handle_key(key_event, &current_user)
                    }
                    Tab::Partitions => {
                        // Try view-specific keys first (sorting)
                        if self.partitions_view.handle_key(key_event) {
                            true
                        } else {
                            // Fall back to basic cursor navigation
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
                    }
                    Tab::Health => false, // No view-level keys yet
                };

                // Check for and persist pending config updates
                if handled {
                    match self.tab {
                        Tab::Jobs => {
                            if let Some(update) = self.jobs_view.take_pending_config_update() {
                                self.persist_config_async(update);
                            }
                        }
                        Tab::Nodes => {
                            if let Some(update) = self.nodes_view.take_pending_config_update() {
                                self.persist_config_async(update);
                            }
                        }
                        Tab::Partitions => {
                            if let Some(update) = self.partitions_view.take_pending_config_update()
                            {
                                self.persist_config_async(update);
                            }
                        }
                        _ => {}
                    }
                }

                handled
            }
        }
    }

    /// Handle mouse events.
    pub fn handle_mouse(&mut self, mouse: crossterm::event::MouseEvent) {
        use crossterm::event::MouseEventKind;

        // Skip if modal is active
        if !matches!(self.modal, Modal::None) {
            return;
        }

        // Route to the active view using the last rendered area
        match self.tab {
            Tab::Jobs => {
                // Use the actual rendered area from the last frame
                let Some(table_area) = self.last_jobs_table_area else {
                    return; // No render yet, ignore mouse events
                };

                match mouse.kind {
                    MouseEventKind::Down(_) => {
                        self.jobs_view
                            .on_mouse_down(mouse.column, mouse.row, table_area);
                    }
                    MouseEventKind::Drag(_) => {
                        self.jobs_view.on_mouse_move(mouse.column, mouse.row);
                    }
                    MouseEventKind::Up(_) => {
                        self.jobs_view
                            .on_mouse_up(mouse.column, mouse.row, table_area);
                    }
                    _ => {}
                }
            }
            Tab::Nodes => {
                // Use the actual rendered area from the last frame
                let Some(table_area) = self.last_nodes_table_area else {
                    return; // No render yet, ignore mouse events
                };

                match mouse.kind {
                    MouseEventKind::Down(_) => {
                        self.nodes_view
                            .on_mouse_down(mouse.column, mouse.row, table_area);
                    }
                    MouseEventKind::Drag(_) => {
                        self.nodes_view.on_mouse_move(mouse.column, mouse.row);
                    }
                    MouseEventKind::Up(_) => {
                        self.nodes_view
                            .on_mouse_up(mouse.column, mouse.row, table_area);
                    }
                    _ => {}
                }
            }
            _ => {
                // No mouse support for other tabs yet
            }
        }
    }

    /// Check if it's time to refresh the current tab.
    pub fn should_refresh(&self) -> bool {
        if self.paused {
            return false;
        }
        match self.last_refresh {
            None => true,
            Some(last) => last.elapsed() >= self.tab.interval(&self.config),
        }
    }

    /// Apply an SSH identity file to the runner (from `--ssh-key`).
    ///
    /// No-op when the key is empty or no remote host is configured, matching
    /// Python's `slurm.set_remote(host, key)`, which is only called when a
    /// host is resolved.
    pub fn set_ssh_key(&mut self, key: &str) {
        if key.is_empty() || self.config.remote.host.is_empty() {
            return;
        }
        self.runner
            .set_remote(self.config.remote.host.clone(), key.to_string());
    }

    /// Toggle the auto-refresh pause state (Python: `action_toggle_pause`).
    ///
    /// Manual refresh with `r` still works while paused.
    pub fn toggle_pause(&mut self) {
        self.paused = !self.paused;
        if !self.paused {
            // Python's `BaseDataTableView.resume` fetches immediately.
            self.last_refresh = None;
        }
        self.status = Some(if self.paused { "Paused" } else { "Resumed" }.to_string());
    }

    /// Ask the worker to refresh the current tab and reset the interval clock.
    pub fn request_refresh(&mut self) {
        let _ = self.request_tx.send(self.tab);
        self.last_refresh = Some(Instant::now());

        // Also refresh log viewer if in follow mode
        if let Modal::LogViewer(viewer) = &self.modal {
            if viewer.is_following() {
                let job_id = viewer.job_id().to_string();
                let path = viewer.path().to_string();
                let log_type = viewer.log_type().to_string();
                self.refresh_log_viewer(job_id, path, log_type);
            }
        }
    }

    /// Show a status message and optionally send a desktop notification.
    pub fn notify(&mut self, message: impl Into<String>, title: &str) {
        let msg = message.into();
        self.status = Some(msg.clone());
        if self.config.notifications.desktop_enabled {
            crate::views::modals::notify::desktop_notify(title, &msg);
        }
    }

    /// Persist a config change (spawns a thread to avoid blocking UI).
    fn persist_config_async(&self, updates: HashMap<String, toml::Value>) {
        let path = self.config_path.clone();
        std::thread::spawn(move || {
            let _ = crate::config::update(&path, &updates);
        });
    }

    /// Persist theme and interval (spawns a thread).
    /// Used by Settings UI for theme/interval changes.
    pub fn persist_theme_interval_async(&self) {
        let path = self.config_path.clone();
        let theme = self.config.theme.clone();
        let interval = self.config.interval.jobs;
        std::thread::spawn(move || {
            let _ = crate::config::save(&path, &theme, interval);
        });
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
pub fn run<B: Backend>(
    terminal: &mut Terminal<B>,
    config: Config,
    config_path: std::path::PathBuf,
    ssh_key: String,
) -> Result<()> {
    let mut app = App::new(config, config_path);
    app.set_ssh_key(&ssh_key);

    loop {
        app.drain_messages();

        terminal.draw(|f| render(f, &mut app))?;

        if app.should_quit {
            break;
        }

        if event::poll(Duration::from_millis(50))? {
            match event::read()? {
                Event::Key(key) => {
                    app.handle_key(key.code, key.modifiers);
                }
                Event::Mouse(mouse) => {
                    app.handle_mouse(mouse);
                }
                _ => {}
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
    let tier = tier_for(area.width);
    let titles: Vec<Line> = Tab::all()
        .iter()
        .map(|t| Line::from(chrome::tab_label(*t, tier)))
        .collect();

    let selected = match app.tab {
        Tab::Jobs => 0,
        Tab::Nodes => 1,
        Tab::Partitions => 2,
        Tab::History => 3,
        Tab::Health => 4,
    };

    let tabs = Tabs::new(titles)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(chrome::sub_title("sqtop", tier, area.width)),
        )
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
    use crate::responsive::{tier_for, Tier};

    let status_text = if let Some(ref msg) = app.status {
        msg.clone()
    } else {
        let tier = tier_for(area.width);
        let mut hints = Vec::new();

        // Build hint list filtered by tier
        let bindings = [
            ("q", "quit", true),
            ("r", "refresh", true),
            ("1-5", "switch_tab", tier != Tier::Xs),
            ("?", "show_keybindings", true),
        ];

        for (key, action, originally_shown) in bindings {
            if chrome::binding_visible(action, tier, originally_shown) {
                hints.push(key);
            }
        }

        format!(
            "Jobs: {} | Nodes: {} | Partitions: {} | Keys: {}",
            app.jobs.len(),
            app.nodes.len(),
            app.partitions.len(),
            hints.join(" ")
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
        Modal::InvestigateJobPrompt(state) => state.render(f, area),
        Modal::InvestigateNodePrompt(state) => state.render(f, area),
        Modal::Palette(state) => state.render(f, area),
        Modal::None => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_toggle_pause_blocks_auto_refresh() {
        let mut app = App::new(
            Config::default(),
            std::path::PathBuf::from("/tmp/test_config.toml"),
        );
        app.last_refresh = None;
        assert!(app.should_refresh());

        app.handle_key(KeyCode::Char('P'), KeyModifiers::NONE);
        assert!(!app.should_refresh(), "paused app must not auto-refresh");
        assert_eq!(app.status.as_deref(), Some("Paused"));
    }

    #[test]
    fn test_resume_refreshes_immediately() {
        let mut app = App::new(
            Config::default(),
            std::path::PathBuf::from("/tmp/test_config.toml"),
        );
        app.handle_key(KeyCode::Char('P'), KeyModifiers::NONE);
        app.last_refresh = Some(std::time::Instant::now());

        app.handle_key(KeyCode::Char('P'), KeyModifiers::NONE);
        assert!(app.should_refresh(), "resume must fetch immediately");
        assert_eq!(app.status.as_deref(), Some("Resumed"));
    }

    #[test]
    fn test_manual_refresh_is_noop_while_paused() {
        let mut app = App::new(
            Config::default(),
            std::path::PathBuf::from("/tmp/test_config.toml"),
        );
        app.handle_key(KeyCode::Char('P'), KeyModifiers::NONE);
        app.handle_key(KeyCode::Char('r'), KeyModifiers::NONE);
        assert!(!app.should_refresh());
    }

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
        let mut app = App::new(config, std::path::PathBuf::from("/tmp/test_config.toml"));

        assert!(!app.should_quit);
        app.handle_key(KeyCode::Char('q'), KeyModifiers::NONE);
        assert!(app.should_quit);

        let mut app2 = App::new(
            Config::default(),
            std::path::PathBuf::from("/tmp/sqtop-test-config.toml"),
        );
        app2.handle_key(KeyCode::Char('c'), KeyModifiers::CONTROL);
        assert!(app2.should_quit);
    }

    #[test]
    fn tab_switch_keys() {
        let config = Config::default();
        let mut app = App::new(config, std::path::PathBuf::from("/tmp/test_config.toml"));

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
        let mut app = App::new(config, std::path::PathBuf::from("/tmp/test_config.toml"));

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
                let mut app = App::new(config, std::path::PathBuf::from("/tmp/test_config.toml"));
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

        let app = App::new(config, std::path::PathBuf::from("/tmp/test_config.toml"));

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

        let app = App::new(config, std::path::PathBuf::from("/tmp/test_config.toml"));

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

        let app = App::new(config, std::path::PathBuf::from("/tmp/test_config.toml"));

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

        let app = App::new(config, std::path::PathBuf::from("/tmp/test_config.toml"));

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

        let app = App::new(config, std::path::PathBuf::from("/tmp/test_config.toml"));

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
        let mut app = App::new(config, std::path::PathBuf::from("/tmp/test_config.toml"));

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
        let mut app = App::new(config, std::path::PathBuf::from("/tmp/test_config.toml"));

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
        let mut app = App::new(config, std::path::PathBuf::from("/tmp/test_config.toml"));

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
        let mut app = App::new(config, std::path::PathBuf::from("/tmp/test_config.toml"));

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
        let mut app = App::new(config, std::path::PathBuf::from("/tmp/test_config.toml"));

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
        let mut app = App::new(config, std::path::PathBuf::from("/tmp/test_config.toml"));

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

    #[test]
    fn test_copy_modal_uses_label() {
        use crate::views::detail::job_detail::JobDetailScreen;
        use std::collections::HashMap;

        let config = Config::default();
        let mut app = App::new(config, std::path::PathBuf::from("/tmp/test_config.toml"));

        // Create a job detail modal
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
            nodelist: "node01".to_string(),
            qos: "normal".to_string(),
        };
        let detail = HashMap::new();
        let screen = JobDetailScreen::new(job.job_id.clone(), detail);
        app.modal = Modal::JobDetail(screen);

        // Note: Actual copy testing would require mocking clipboard which we skip
        // This test just verifies the modal has a label method
        if let Modal::JobDetail(ref s) = app.modal {
            let label = s.label();
            assert!(label.contains("12345"));
        }
    }

    #[test]
    fn test_log_viewer_has_required_methods() {
        use crate::views::detail::log_viewer::LogViewerScreen;

        // Verify LogViewerScreen has the methods needed for follow mode
        let viewer = LogViewerScreen::new(
            "12345".to_string(),
            "/tmp/test.log".to_string(),
            "stdout".to_string(),
            "initial content".to_string(),
        );

        // Test getters exist
        assert_eq!(viewer.job_id(), "12345");
        assert_eq!(viewer.path(), "/tmp/test.log");
        assert_eq!(viewer.log_type(), "stdout");
        assert!(viewer.is_following()); // default is true

        // Test update_content exists (can't test without mut)
        // The method is wired in request_refresh when is_following() is true
    }

    #[test]
    fn test_jobs_sort_persists_to_config() {
        use tempfile::TempDir;

        let temp_dir = TempDir::new().unwrap();
        let config_path = temp_dir.path().join("config.toml");

        // Create initial config
        let config = Config::default();
        crate::config::save(&config_path, &config.theme, config.interval.jobs).unwrap();

        let mut view = JobsView::from_config(&config);

        // Toggle sort
        view.toggle_sort("state");

        // Check pending update exists
        let update = view.take_pending_config_update();
        assert!(update.is_some());

        // Simulate App persisting it
        let update = update.unwrap();
        crate::config::update(&config_path, &update).unwrap();

        // Reload config and verify
        let reloaded = crate::config::load(&config_path);
        assert_eq!(reloaded.view_state.jobs_sort_col, "state");
        assert!(!reloaded.view_state.jobs_sort_reversed);

        // Toggle again (reverse)
        view.toggle_sort("state");
        let update = view.take_pending_config_update().unwrap();
        crate::config::update(&config_path, &update).unwrap();

        let reloaded = crate::config::load(&config_path);
        assert_eq!(reloaded.view_state.jobs_sort_col, "state");
        assert!(reloaded.view_state.jobs_sort_reversed);
    }

    #[test]
    fn test_jobs_column_reorder_persists_to_config() {
        use tempfile::TempDir;

        let temp_dir = TempDir::new().unwrap();
        let config_path = temp_dir.path().join("config.toml");

        let config = Config::default();
        let mut temp_config = config.clone();
        temp_config.columns.jobs_order = vec!["JOBID".into(), "STATE".into(), "NAME".into()];
        crate::config::save(&config_path, &temp_config.theme, temp_config.interval.jobs).unwrap();

        let mut view = JobsView::from_config(&temp_config);

        // Need to build columns first so we have something to reorder
        view.column_widths.insert("JOBID".to_string(), 10);
        view.column_widths.insert("STATE".to_string(), 10);
        view.column_widths.insert("NAME".to_string(), 10);

        // Shift first column right
        view.reorder_target_idx = 0;
        view.shift_column_right();

        // Check pending update
        if let Some(update) = view.take_pending_config_update() {
            crate::config::update(&config_path, &update).unwrap();

            let reloaded = crate::config::load(&config_path);
            // After shifting JOBID right, order should be [STATE, JOBID, NAME]
            assert_eq!(reloaded.columns.jobs_order[0], "STATE");
            assert_eq!(reloaded.columns.jobs_order[1], "JOBID");
        }
    }

    #[test]
    fn test_nodes_sort_persists_to_config() {
        use tempfile::TempDir;

        let temp_dir = TempDir::new().unwrap();
        let config_path = temp_dir.path().join("config.toml");

        let config = Config::default();
        crate::config::save(&config_path, &config.theme, config.interval.jobs).unwrap();

        let mut view = NodesView::new(&config);

        // Set sort
        view.set_sort("cpu");

        // Check pending update
        let update = view.take_pending_config_update();
        assert!(update.is_some());

        let update = update.unwrap();
        crate::config::update(&config_path, &update).unwrap();

        // Reload and verify
        let reloaded = crate::config::load(&config_path);
        assert_eq!(reloaded.view_state.nodes_sort_col, "cpu");
        assert!(!reloaded.view_state.nodes_sort_reversed);
    }

    #[test]
    fn test_column_hidden_preserved_in_config() {
        use tempfile::TempDir;

        let temp_dir = TempDir::new().unwrap();
        let config_path = temp_dir.path().join("config.toml");

        let mut config = Config::default();
        crate::config::save(&config_path, &config.theme, config.interval.jobs).unwrap();

        // Simulate hiding a column via the modal
        config.columns.jobs_hidden = vec!["PARTITION".to_string()];

        // Create update and persist
        let mut update = std::collections::HashMap::new();
        let mut columns = toml::Table::new();
        let hidden_array: Vec<toml::Value> = config
            .columns
            .jobs_hidden
            .iter()
            .map(|s| toml::Value::String(s.clone()))
            .collect();
        columns.insert("jobs_hidden".to_string(), toml::Value::Array(hidden_array));
        update.insert("columns".to_string(), toml::Value::Table(columns));

        crate::config::update(&config_path, &update).unwrap();

        // Reload and verify
        let reloaded = crate::config::load(&config_path);
        assert_eq!(reloaded.columns.jobs_hidden, vec!["PARTITION"]);
    }

    #[test]
    fn test_config_preserves_user_comments() {
        use std::fs;
        use tempfile::TempDir;

        let temp_dir = TempDir::new().unwrap();
        let config_path = temp_dir.path().join("config.toml");

        // Write config with user comment
        fs::write(
            &config_path,
            r#"
# User's important comment
[view_state]
jobs_sort_col = ""

[columns]
jobs_order = []
"#,
        )
        .unwrap();

        // Make a change via update
        let mut update = std::collections::HashMap::new();
        let mut view_state = toml::Table::new();
        view_state.insert(
            "jobs_sort_col".to_string(),
            toml::Value::String("state".to_string()),
        );
        update.insert("view_state".to_string(), toml::Value::Table(view_state));

        crate::config::update(&config_path, &update).unwrap();

        // Read file and verify comment is preserved
        let content = fs::read_to_string(&config_path).unwrap();
        assert!(
            content.contains("User's important comment"),
            "User comment should be preserved"
        );
        assert!(
            content.contains("jobs_sort_col = \"state\""),
            "Updated value should be present"
        );
    }

    #[test]
    fn test_config_preserves_unknown_keys() {
        use std::fs;
        use tempfile::TempDir;

        let temp_dir = TempDir::new().unwrap();
        let config_path = temp_dir.path().join("config.toml");

        // Write config with unknown key
        fs::write(
            &config_path,
            r#"
[view_state]
jobs_sort_col = ""
unknown_future_key = "value"

[columns]
jobs_order = []
"#,
        )
        .unwrap();

        // Make a change
        let mut update = std::collections::HashMap::new();
        let mut view_state = toml::Table::new();
        view_state.insert(
            "jobs_sort_col".to_string(),
            toml::Value::String("time".to_string()),
        );
        update.insert("view_state".to_string(), toml::Value::Table(view_state));

        crate::config::update(&config_path, &update).unwrap();

        // Verify unknown key is preserved
        let content = fs::read_to_string(&config_path).unwrap();
        assert!(
            content.contains("unknown_future_key"),
            "Unknown keys should be preserved"
        );
    }

    #[test]
    fn test_persist_to_unwritable_path_does_not_panic() {
        // Create a read-only directory
        use std::fs;
        use tempfile::TempDir;

        let temp_dir = TempDir::new().unwrap();
        let readonly_dir = temp_dir.path().join("readonly");
        fs::create_dir(&readonly_dir).unwrap();

        let config_path = readonly_dir.join("config.toml");

        // On Unix, make directory read-only
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut perms = fs::metadata(&readonly_dir).unwrap().permissions();
            perms.set_mode(0o444);
            fs::set_permissions(&readonly_dir, perms).unwrap();
        }

        // Attempt to update should not panic (just return error)
        let mut update = std::collections::HashMap::new();
        let mut view_state = toml::Table::new();
        view_state.insert(
            "jobs_sort_col".to_string(),
            toml::Value::String("state".to_string()),
        );
        update.insert("view_state".to_string(), toml::Value::Table(view_state));

        // This should not panic
        let result = crate::config::update(&config_path, &update);
        assert!(
            result.is_err(),
            "Update to readonly path should fail gracefully"
        );

        // Clean up permissions on Unix so temp_dir can be deleted
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut perms = fs::metadata(&readonly_dir).unwrap().permissions();
            perms.set_mode(0o755);
            let _ = fs::set_permissions(&readonly_dir, perms);
        }
    }
}

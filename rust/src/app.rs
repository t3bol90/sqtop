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

/// Modal overlay state. View workers will add variants as needed.
///
/// When a modal is active, key events route to the modal first,
/// then fall through to the main view if the modal doesn't handle them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Modal {
    /// No modal active.
    None,
    // View workers add variants like:
    // JobAction(String),
    // NodeDetail(String),
    // Settings,
    // etc.
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

    /// Handle a key event. Returns true if the event was handled.
    pub fn handle_key(&mut self, key: KeyCode, modifiers: KeyModifiers) -> bool {
        // Route to modal first if active
        if self.modal != Modal::None {
            // View workers implement modal key handling
            return false;
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

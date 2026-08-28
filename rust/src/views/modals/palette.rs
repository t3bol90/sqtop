//! Command palette modal — searchable system command list with settings.

use super::{centered_rect, ModalOutcome};
use crate::config::Config;
use crate::responsive::{TOO_SMALL_HEIGHT, TOO_SMALL_WIDTH};
use crate::views::table_state::CyclicTableState;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, List, ListItem, Paragraph};
use ratatui::Frame;

/// Command palette result.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PaletteResult {
    /// User closed without selecting.
    None,
    /// User selected a command.
    Execute(PaletteCommand),
}

/// Command to execute from palette.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PaletteCommand {
    RefreshData,
    SetRefreshInterval(u64), // in seconds
    ToggleExpertMode,
    ToggleConfirmCancelSingle,
    ToggleConfirmBulkActions,
    ColumnVisibility,
    SetJobsDefaultSort(String),
    InvestigateJobById,
    InvestigateNodeByName,
    ReloadConfig,
}

/// A single palette command entry.
#[derive(Debug, Clone)]
struct CommandEntry {
    title: String,
    description: String,
    command: PaletteCommand,
    discover: bool, // false = only shown when searched
}

/// State for the command palette modal.
#[derive(Debug, Clone)]
pub struct PaletteState {
    /// Search query (filter commands by title)
    search_query: String,
    /// All commands (rebuilt each time palette opens to show current state)
    commands: Vec<CommandEntry>,
    /// Filtered/displayed commands
    filtered: Vec<usize>, // indices into commands
    /// Cursor position in filtered list
    table_state: CyclicTableState,
}

impl PaletteState {
    /// Create a new palette state with commands built from current config.
    pub fn new(config: &Config) -> Self {
        let commands = build_commands(config);
        let filtered = build_filtered(&commands, "");
        let mut table_state = CyclicTableState::default();
        table_state.set_row_count(filtered.len());
        Self {
            search_query: String::new(),
            commands,
            filtered,
            table_state,
        }
    }

    /// Handle a key event.
    pub fn handle_key(&mut self, key: KeyEvent, _config: &Config) -> ModalOutcome<PaletteResult> {
        match (key.code, key.modifiers) {
            // Close
            (KeyCode::Esc, _) => ModalOutcome::Dismiss(PaletteResult::None),
            // Execute selected command
            (KeyCode::Enter, _) => {
                if let Some(selected_idx) = self.table_state.selected() {
                    if let Some(&idx) = self.filtered.get(selected_idx) {
                        let cmd = self.commands[idx].command.clone();
                        return ModalOutcome::Dismiss(PaletteResult::Execute(cmd));
                    }
                }
                ModalOutcome::Dismiss(PaletteResult::None)
            }
            // Navigate
            (KeyCode::Down, KeyModifiers::NONE) | (KeyCode::Char('j'), KeyModifiers::NONE) => {
                self.table_state.next();
                ModalOutcome::Continue
            }
            (KeyCode::Up, KeyModifiers::NONE) | (KeyCode::Char('k'), KeyModifiers::NONE) => {
                self.table_state.prev();
                ModalOutcome::Continue
            }
            // Typing: filter commands
            (KeyCode::Char(c), KeyModifiers::NONE | KeyModifiers::SHIFT) => {
                self.search_query.push(c);
                self.update_filter();
                ModalOutcome::Continue
            }
            (KeyCode::Backspace, _) => {
                self.search_query.pop();
                self.update_filter();
                ModalOutcome::Continue
            }
            _ => ModalOutcome::Continue,
        }
    }

    /// Update the filtered list based on current search query.
    fn update_filter(&mut self) {
        self.filtered = build_filtered(&self.commands, &self.search_query);
        // Reset cursor and row count
        self.table_state.set_row_count(self.filtered.len());
    }

    /// Render the palette modal.
    pub fn render(&mut self, f: &mut Frame, area: Rect) {
        // Too-small check
        if area.width < TOO_SMALL_WIDTH || area.height < TOO_SMALL_HEIGHT {
            let block = Block::default()
                .borders(Borders::ALL)
                .title("Command Palette (terminal too small)")
                .style(Style::default().fg(Color::White).bg(Color::Red));
            let para = Paragraph::new("Terminal too small").block(block);
            let rect = centered_rect(area, 50, 3);
            f.render_widget(Clear, rect);
            f.render_widget(para, rect);
            return;
        }

        // Calculate modal size
        let width = area.width.saturating_sub(4).min(80);
        let height = area.height.saturating_sub(4).min(30);
        let rect = centered_rect(area, width, height);

        f.render_widget(Clear, rect);

        // Split into search bar and list
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Length(3), Constraint::Min(0)])
            .split(rect);

        // Search bar
        let search_text = if self.search_query.is_empty() {
            "Type to filter commands...".to_string()
        } else {
            self.search_query.clone()
        };
        let search_block = Block::default()
            .borders(Borders::ALL)
            .title("Command Palette")
            .style(Style::default().fg(Color::White));
        let search_para = Paragraph::new(search_text)
            .block(search_block)
            .style(Style::default().fg(Color::Cyan));
        f.render_widget(search_para, chunks[0]);

        // Command list
        let items: Vec<ListItem> = self
            .filtered
            .iter()
            .enumerate()
            .map(|(i, &cmd_idx)| {
                let cmd = &self.commands[cmd_idx];
                let style = if Some(i) == self.table_state.selected() {
                    Style::default()
                        .fg(Color::Black)
                        .bg(Color::Cyan)
                        .add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(Color::White)
                };
                let line = Line::from(vec![
                    Span::styled(&cmd.title, style),
                    Span::raw("  "),
                    Span::styled(&cmd.description, Style::default().fg(Color::DarkGray)),
                ]);
                ListItem::new(line)
            })
            .collect();

        let list_block = Block::default()
            .borders(Borders::ALL)
            .title(format!(
                "Commands ({}/{})",
                self.filtered.len(),
                self.commands.len()
            ))
            .style(Style::default().fg(Color::White));
        let list = List::new(items).block(list_block);
        f.render_widget(list, chunks[1]);
    }
}

/// Build the full command list with labels reflecting current config state.
fn build_commands(config: &Config) -> Vec<CommandEntry> {
    let mut commands = Vec::new();

    // Refresh data
    commands.push(CommandEntry {
        title: "Refresh data".to_string(),
        description: "Refresh all views now".to_string(),
        command: PaletteCommand::RefreshData,
        discover: true,
    });

    // Set refresh intervals (discover=false)
    for secs in [1, 2, 5, 10, 30] {
        commands.push(CommandEntry {
            title: format!("Set refresh: {}s", secs),
            description: format!("Set auto-refresh interval to {}s", secs),
            command: PaletteCommand::SetRefreshInterval(secs),
            discover: false,
        });
    }

    // Expert mode toggle
    let expert_mode = config.ui.expert_mode;
    let mode_str = if expert_mode { "on" } else { "off" };
    commands.push(CommandEntry {
        title: format!("Expert mode: {} → toggle", mode_str),
        description: "Toggle expert mode (fewer confirmation dialogs)".to_string(),
        command: PaletteCommand::ToggleExpertMode,
        discover: true,
    });

    // Confirm single cancel toggle
    let confirm_single = config.safety.confirm_cancel_single;
    let ccs_str = if confirm_single { "on" } else { "off" };
    commands.push(CommandEntry {
        title: format!("Confirm single cancel: {} → toggle", ccs_str),
        description: "Toggle confirmation dialog for single job cancel".to_string(),
        command: PaletteCommand::ToggleConfirmCancelSingle,
        discover: true,
    });

    // Confirm bulk actions toggle
    let confirm_bulk = config.safety.confirm_bulk_actions;
    let cba_str = if confirm_bulk { "on" } else { "off" };
    commands.push(CommandEntry {
        title: format!("Confirm bulk actions: {} → toggle", cba_str),
        description: "Toggle confirmation for bulk operations".to_string(),
        command: PaletteCommand::ToggleConfirmBulkActions,
        discover: true,
    });

    // Column visibility
    commands.push(CommandEntry {
        title: "Column visibility".to_string(),
        description: "Show/hide columns for the current view".to_string(),
        command: PaletteCommand::ColumnVisibility,
        discover: true,
    });

    // Jobs default sort options (discover=false)
    let sort_options = [
        ("", "State priority (default)"),
        ("state", "State"),
        ("time", "Time used"),
        ("cpus", "CPUs"),
        ("qos", "QOS"),
    ];
    for (sort_val, sort_label) in sort_options {
        commands.push(CommandEntry {
            title: format!("Jobs default sort: {}", sort_label),
            description: format!("Set jobs default sort to '{}' and persist", sort_label),
            command: PaletteCommand::SetJobsDefaultSort(sort_val.to_string()),
            discover: false,
        });
    }

    // Investigate job by ID (discover=false)
    commands.push(CommandEntry {
        title: "Investigate job by ID".to_string(),
        description: "Open the investigation report for a job ID you type".to_string(),
        command: PaletteCommand::InvestigateJobById,
        discover: false,
    });

    // Investigate node by name (discover=false)
    commands.push(CommandEntry {
        title: "Investigate node by name".to_string(),
        description: "Open the investigation report for a node name you type".to_string(),
        command: PaletteCommand::InvestigateNodeByName,
        discover: false,
    });

    // Reload config
    commands.push(CommandEntry {
        title: "Reload config".to_string(),
        description: "Re-read config and apply theme, expert mode, and confirmation settings"
            .to_string(),
        command: PaletteCommand::ReloadConfig,
        discover: true,
    });

    commands
}

/// Build the filtered command indices based on search query.
fn build_filtered(commands: &[CommandEntry], query: &str) -> Vec<usize> {
    let query_lower = query.to_lowercase();
    if query_lower.is_empty() {
        // No search: show only discover=true commands
        commands
            .iter()
            .enumerate()
            .filter(|(_, cmd)| cmd.discover)
            .map(|(i, _)| i)
            .collect()
    } else {
        // With search: show all commands matching query (case-insensitive substring)
        commands
            .iter()
            .enumerate()
            .filter(|(_, cmd)| cmd.title.to_lowercase().contains(&query_lower))
            .map(|(i, _)| i)
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_palette_new() {
        let config = Config::default();
        let state = PaletteState::new(&config);
        // Should have some commands
        assert!(!state.commands.is_empty());
        // With no search, only discover=true commands shown
        let discoverable_count = state.commands.iter().filter(|c| c.discover).count();
        assert_eq!(state.filtered.len(), discoverable_count);
    }

    #[test]
    fn test_palette_filter_discover() {
        let config = Config::default();
        let state = PaletteState::new(&config);
        // With empty search, only discover=true commands are shown
        let discoverable_count = state.commands.iter().filter(|c| c.discover).count();
        assert_eq!(state.filtered.len(), discoverable_count);
    }

    #[test]
    fn test_palette_filter_search() {
        let config = Config::default();
        let mut state = PaletteState::new(&config);
        // Type "refresh" to filter
        for c in "refresh".chars() {
            state.search_query.push(c);
        }
        state.update_filter();
        // Should match "Refresh data" and "Set refresh: Xs" commands
        assert!(!state.filtered.is_empty());
        for &idx in &state.filtered {
            assert!(state.commands[idx].title.to_lowercase().contains("refresh"));
        }
    }

    #[test]
    fn test_palette_key_typing() {
        let config = Config::default();
        let mut state = PaletteState::new(&config);
        let initial_count = state.filtered.len();

        // Type 'r'
        let key = KeyEvent::new(KeyCode::Char('r'), KeyModifiers::NONE);
        let outcome = state.handle_key(key, &config);
        assert_eq!(outcome, ModalOutcome::Continue);
        assert_eq!(state.search_query, "r");
        // Filter should have changed
        assert_ne!(state.filtered.len(), initial_count);
    }

    #[test]
    fn test_palette_key_backspace() {
        let config = Config::default();
        let mut state = PaletteState::new(&config);
        state.search_query = "test".to_string();
        state.update_filter();

        let key = KeyEvent::new(KeyCode::Backspace, KeyModifiers::NONE);
        let outcome = state.handle_key(key, &config);
        assert_eq!(outcome, ModalOutcome::Continue);
        assert_eq!(state.search_query, "tes");
    }

    #[test]
    fn test_palette_key_esc() {
        let config = Config::default();
        let mut state = PaletteState::new(&config);
        let key = KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE);
        let outcome = state.handle_key(key, &config);
        assert_eq!(outcome, ModalOutcome::Dismiss(PaletteResult::None));
    }

    #[test]
    fn test_palette_key_enter() {
        let config = Config::default();
        let mut state = PaletteState::new(&config);
        // Select first command
        if !state.filtered.is_empty() {
            state.table_state.select(Some(0));
            let key = KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE);
            let outcome = state.handle_key(key, &config);
            match outcome {
                ModalOutcome::Dismiss(PaletteResult::Execute(_)) => {
                    // OK
                }
                _ => panic!("Expected Execute outcome"),
            }
        }
    }

    #[test]
    fn test_palette_key_navigation() {
        let config = Config::default();
        let mut state = PaletteState::new(&config);
        if state.filtered.len() > 1 {
            state.table_state.select(Some(0));
            let key = KeyEvent::new(KeyCode::Down, KeyModifiers::NONE);
            let outcome = state.handle_key(key, &config);
            assert_eq!(outcome, ModalOutcome::Continue);
            assert_eq!(state.table_state.selected(), Some(1));

            let key = KeyEvent::new(KeyCode::Up, KeyModifiers::NONE);
            let outcome = state.handle_key(key, &config);
            assert_eq!(outcome, ModalOutcome::Continue);
            assert_eq!(state.table_state.selected(), Some(0));
        }
    }

    #[test]
    fn test_commands_reflect_config_state() {
        let mut config = Config::default();
        config.ui.expert_mode = true;
        let state = PaletteState::new(&config);
        // Find the expert mode command
        let expert_cmd = state
            .commands
            .iter()
            .find(|c| matches!(c.command, PaletteCommand::ToggleExpertMode));
        assert!(expert_cmd.is_some());
        assert!(expert_cmd.unwrap().title.contains("on"));

        // Now with expert_mode off
        config.ui.expert_mode = false;
        let state = PaletteState::new(&config);
        let expert_cmd = state
            .commands
            .iter()
            .find(|c| matches!(c.command, PaletteCommand::ToggleExpertMode));
        assert!(expert_cmd.is_some());
        assert!(expert_cmd.unwrap().title.contains("off"));
    }
    #[test]
    fn test_commands_set_refresh_interval() {
        let config = Config::default();
        let state = PaletteState::new(&config);
        // Find a "Set refresh: 5s" command
        let refresh_cmd = state
            .commands
            .iter()
            .find(|c| matches!(c.command, PaletteCommand::SetRefreshInterval(5)));
        assert!(refresh_cmd.is_some());
        assert_eq!(refresh_cmd.unwrap().title, "Set refresh: 5s");
    }

    #[test]
    fn test_commands_jobs_sort_options() {
        let config = Config::default();
        let state = PaletteState::new(&config);
        // Check that all sort options are present
        let sort_options = [
            "State priority (default)",
            "State",
            "Time used",
            "CPUs",
            "QOS",
        ];
        for label in sort_options {
            let found = state
                .commands
                .iter()
                .any(|c| c.title == format!("Jobs default sort: {}", label));
            assert!(found, "Missing sort option: {}", label);
        }
    }

    #[test]
    fn test_discover_false_commands_hidden() {
        let config = Config::default();
        let state = PaletteState::new(&config);
        // With empty search, discover=false commands should not be in filtered
        for &idx in &state.filtered {
            assert!(
                state.commands[idx].discover,
                "Non-discoverable command '{}' shown without search",
                state.commands[idx].title
            );
        }
    }

    #[test]
    fn test_discover_false_commands_shown_with_search() {
        let config = Config::default();
        let mut state = PaletteState::new(&config);
        // Search for "Set refresh" which should show discover=false commands
        state.search_query = "Set refresh".to_string();
        state.update_filter();
        // Should find at least one "Set refresh: Xs" command
        let found = state
            .filtered
            .iter()
            .any(|&idx| state.commands[idx].title.starts_with("Set refresh:"));
        assert!(
            found,
            "Discover=false 'Set refresh' commands not shown when searched"
        );
    }

    #[test]
    fn test_investigate_commands_present() {
        let config = Config::default();
        let state = PaletteState::new(&config);
        // Check investigation commands exist
        let job_inv = state
            .commands
            .iter()
            .any(|c| matches!(c.command, PaletteCommand::InvestigateJobById));
        let node_inv = state
            .commands
            .iter()
            .any(|c| matches!(c.command, PaletteCommand::InvestigateNodeByName));
        assert!(job_inv, "Missing InvestigateJobById command");
        assert!(node_inv, "Missing InvestigateNodeByName command");
    }

    #[test]
    fn test_reload_config_command_present() {
        let config = Config::default();
        let state = PaletteState::new(&config);
        let reload_cmd = state
            .commands
            .iter()
            .any(|c| matches!(c.command, PaletteCommand::ReloadConfig));
        assert!(reload_cmd, "Missing ReloadConfig command");
    }

    #[test]
    fn test_filter_case_insensitive() {
        let config = Config::default();
        let mut state = PaletteState::new(&config);
        // Search with different case
        state.search_query = "EXPERT".to_string();
        state.update_filter();
        // Should find "Expert mode" command
        let found = state
            .filtered
            .iter()
            .any(|&idx| state.commands[idx].title.to_lowercase().contains("expert"));
        assert!(found, "Case-insensitive search failed");
    }
    #[test]
    fn test_investigate_job_command() {
        let config = Config::default();
        let state = PaletteState::new(&config);
        // Find the InvestigateJobById command
        let cmd = state.commands.iter()
            .find(|c| matches!(c.command, PaletteCommand::InvestigateJobById));
        assert!(cmd.is_some(), "InvestigateJobById command not found");
        assert_eq!(cmd.unwrap().title, "Investigate job by ID");
        assert!(!cmd.unwrap().discover, "Investigation command should be discover=false");
    }

    #[test]
    fn test_investigate_node_command() {
        let config = Config::default();
        let state = PaletteState::new(&config);
        // Find the InvestigateNodeByName command
        let cmd = state.commands.iter()
            .find(|c| matches!(c.command, PaletteCommand::InvestigateNodeByName));
        assert!(cmd.is_some(), "InvestigateNodeByName command not found");
        assert_eq!(cmd.unwrap().title, "Investigate node by name");
        assert!(!cmd.unwrap().discover, "Investigation command should be discover=false");
    }

    #[test]
    fn test_execute_investigate_job_returns_correct_command() {
        let config = Config::default();
        let mut state = PaletteState::new(&config);
        // Search for "Investigate job"
        state.search_query = "Investigate job".to_string();
        state.update_filter();
        // Should find the command
        assert!(!state.filtered.is_empty(), "Should find investigation command");
        // Select and execute it
        state.table_state.select(Some(0));
        let key = KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE);
        let outcome = state.handle_key(key, &config);
        match outcome {
            ModalOutcome::Dismiss(PaletteResult::Execute(PaletteCommand::InvestigateJobById)) => {
                // Success
            }
            _ => panic!("Expected InvestigateJobById command to be executed"),
        }
    }

    #[test]
    fn test_execute_investigate_node_returns_correct_command() {
        let config = Config::default();
        let mut state = PaletteState::new(&config);
        // Search for "Investigate node"
        state.search_query = "Investigate node".to_string();
        state.update_filter();
        // Should find the command
        assert!(!state.filtered.is_empty(), "Should find investigation command");
        // Select and execute it
        state.table_state.select(Some(0));
        let key = KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE);
        let outcome = state.handle_key(key, &config);
        match outcome {
            ModalOutcome::Dismiss(PaletteResult::Execute(PaletteCommand::InvestigateNodeByName)) => {
                // Success
            }
            _ => panic!("Expected InvestigateNodeByName command to be executed"),
        }
    }

}

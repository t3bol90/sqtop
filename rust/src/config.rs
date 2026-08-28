//! Persistent configuration for sqtop.
//!
//! Stored at ~/.config/sqtop/config.toml. The config file is the single
//! persistent source of truth for user preferences.
//!
//! Writes are round-trip preserving: comments, key order, unknown sections, and
//! unknown keys present in the on-disk file are retained when only specific keys
//! are mutated by save() / update(). Persisted writes are atomic via a same-
//! directory temp file plus fs::rename().

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use toml_edit::{DocumentMut, Item, Table, Value};

// ── Schema ────────────────────────────────────────────────────────────────────

/// Full configuration schema matching the Python implementation.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Config {
    pub theme: String,
    pub interval: IntervalConfig,
    pub jobs: JobsConfig,
    pub attach: AttachConfig,
    pub ui: UiConfig,
    pub safety: SafetyConfig,
    pub health: HealthConfig,
    pub view_state: ViewStateConfig,
    pub columns: ColumnsConfig,
    pub notifications: NotificationsConfig,
    pub remote: RemoteConfig,
    pub clipboard: ClipboardConfig,
    pub investigation: InvestigationConfig,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct IntervalConfig {
    pub jobs: f64,
    pub nodes: f64,
    pub partitions: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct JobsConfig {
    pub name_max: i64,
    pub user_max: i64,
    pub partition_max: i64,
    pub nodelist_reason_max: i64,
    pub qos_max: i64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AttachConfig {
    pub enabled: bool,
    pub default_command: String,
    pub extra_args: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct UiConfig {
    pub expert_mode: bool,
    pub show_palette_hints: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SafetyConfig {
    pub confirm_cancel_single: bool,
    pub confirm_bulk_actions: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HealthConfig {
    pub enabled: bool,
    pub history_size: i64,
    pub warn_pending_ratio: f64,
    pub warn_down_nodes: i64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct ViewStateConfig {
    pub jobs_sort_col: String,
    pub jobs_sort_reversed: bool,
    pub nodes_sort_col: String,
    pub nodes_sort_reversed: bool,
    pub partitions_sort_col: String,
    pub partitions_sort_reversed: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct ColumnsConfig {
    pub jobs_hidden: Vec<String>,
    pub nodes_hidden: Vec<String>,
    pub partitions_hidden: Vec<String>,
    pub jobs_order: Vec<String>,
    pub nodes_order: Vec<String>,
    pub partitions_order: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct NotificationsConfig {
    pub desktop_enabled: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct RemoteConfig {
    pub host: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ClipboardConfig {
    pub transport: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct InvestigationConfig {
    pub reasons_path: String,
    pub max_related_jobs: i64,
}

// ── Defaults ──────────────────────────────────────────────────────────────────

const SECTION_ORDER: &[&str] = &[
    "interval",
    "jobs",
    "attach",
    "ui",
    "safety",
    "health",
    "investigation",
    "view_state",
    "columns",
    "notifications",
    "remote",
    "clipboard",
];

const SECTION_COMMENTS: &[(&str, &str)] = &[
    ("interval", "Auto-refresh seconds per view."),
    ("jobs", "Jobs view column width caps."),
    ("attach", "Attach-via-srun behavior."),
    ("ui", "UI visual behavior and confirmation toggles."),
    ("safety", "Confirmation prompts for destructive actions."),
    ("health", "Health view diagnostics and warning thresholds."),
    ("investigation", "Investigation Mode behavior. Set reasons_path to extend pending-reason explanations from a TOML file."),
    ("view_state", "Persisted sort/filter state."),
    ("columns", "Hidden columns and explicit column order."),
    ("notifications", "Desktop notification behavior."),
    ("remote", "Default SSH host for remote mode."),
    ("clipboard", "Clipboard transport selection."),
];

impl Default for Config {
    fn default() -> Self {
        Self {
            theme: "dracula".to_string(),
            interval: IntervalConfig::default(),
            jobs: JobsConfig::default(),
            attach: AttachConfig::default(),
            ui: UiConfig::default(),
            safety: SafetyConfig::default(),
            health: HealthConfig::default(),
            view_state: ViewStateConfig::default(),
            columns: ColumnsConfig::default(),
            notifications: NotificationsConfig::default(),
            remote: RemoteConfig::default(),
            clipboard: ClipboardConfig::default(),
            investigation: InvestigationConfig::default(),
        }
    }
}

impl Default for IntervalConfig {
    fn default() -> Self {
        Self {
            jobs: 2.0,
            nodes: 2.0,
            partitions: 5.0,
        }
    }
}

impl Default for JobsConfig {
    fn default() -> Self {
        Self {
            name_max: 24,
            user_max: 12,
            partition_max: 14,
            nodelist_reason_max: 40,
            qos_max: 12,
        }
    }
}

impl Default for AttachConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            default_command: "$SHELL -l".to_string(),
            extra_args: String::new(),
        }
    }
}

impl Default for UiConfig {
    fn default() -> Self {
        Self {
            expert_mode: false,
            show_palette_hints: true,
        }
    }
}

impl Default for SafetyConfig {
    fn default() -> Self {
        Self {
            confirm_cancel_single: true,
            confirm_bulk_actions: true,
        }
    }
}

impl Default for HealthConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            history_size: 100,
            warn_pending_ratio: 0.7,
            warn_down_nodes: 1,
        }
    }
}

impl Default for NotificationsConfig {
    fn default() -> Self {
        Self {
            desktop_enabled: true,
        }
    }
}

impl Default for ClipboardConfig {
    fn default() -> Self {
        Self {
            transport: "auto".to_string(),
        }
    }
}

impl Default for InvestigationConfig {
    fn default() -> Self {
        Self {
            reasons_path: String::new(),
            max_related_jobs: 20,
        }
    }
}

// ── Path Resolution ───────────────────────────────────────────────────────────

/// Resolve the config file path with precedence: CLI arg > SQTOP_CONFIG env > default.
///
/// Empty or whitespace-only values fall through to the next source.
///
/// Precedence order:
/// 1. CLI argument (--config), trimmed and non-empty
/// 2. SQTOP_CONFIG environment variable, trimmed and non-empty
/// 3. Default XDG location: ~/.config/sqtop/config.toml
pub fn resolve_config_path(cli_arg: Option<PathBuf>) -> PathBuf {
    // Check CLI arg: trim and use if non-empty
    if let Some(path) = cli_arg {
        if let Some(s) = path.to_str() {
            let trimmed = s.trim();
            if !trimmed.is_empty() {
                return expand_tilde(&PathBuf::from(trimmed));
            }
        }
    }

    // Check env var: trim and use if non-empty
    if let Ok(env_path) = std::env::var("SQTOP_CONFIG") {
        let trimmed = env_path.trim();
        if !trimmed.is_empty() {
            return expand_tilde(&PathBuf::from(trimmed));
        }
    }

    default_config_path()
}

/// Get the default XDG config path.
fn default_config_path() -> PathBuf {
    let home = std::env::var("HOME").unwrap_or_else(|_| ".".to_string());
    PathBuf::from(home)
        .join(".config")
        .join("sqtop")
        .join("config.toml")
}

/// Expand ~ prefix to home directory.
fn expand_tilde(path: &Path) -> PathBuf {
    if let Some(stripped) = path.to_str().and_then(|s| s.strip_prefix("~/")) {
        let home = std::env::var("HOME").unwrap_or_else(|_| ".".to_string());
        PathBuf::from(home).join(stripped)
    } else {
        path.to_path_buf()
    }
}

// ── Load ──────────────────────────────────────────────────────────────────────

/// Return config from the given path, falling back to defaults on any error.
pub fn load(path: &Path) -> Config {
    load_inner(path).unwrap_or_default()
}

fn load_inner(path: &Path) -> Result<Config> {
    if !path.exists() {
        return Ok(Config::default());
    }

    let content = fs::read_to_string(path)
        .with_context(|| format!("failed to read config file: {}", path.display()))?;

    if content.trim().is_empty() {
        return Ok(Config::default());
    }

    let parsed: toml::Value = toml::from_str(&content)
        .with_context(|| format!("failed to parse config file: {}", path.display()))?;

    let mut config = Config::default();

    // Top-level theme
    if let Some(theme) = parsed.get("theme").and_then(|v| v.as_str()) {
        config.theme = theme.to_string();
    }

    // Interval - handle legacy bare float
    if let Some(interval_val) = parsed.get("interval") {
        if let Some(legacy_float) = interval_val.as_float() {
            // Legacy bare `interval = X` broadcasts to all three
            config.interval.jobs = legacy_float;
            config.interval.nodes = legacy_float;
            config.interval.partitions = legacy_float;
        } else if let Some(legacy_int) = interval_val.as_integer() {
            let f = legacy_int as f64;
            config.interval.jobs = f;
            config.interval.nodes = f;
            config.interval.partitions = f;
        } else if let Some(table) = interval_val.as_table() {
            if let Some(v) = table
                .get("jobs")
                .and_then(|v| v.as_float().or_else(|| v.as_integer().map(|i| i as f64)))
            {
                config.interval.jobs = v;
            }
            if let Some(v) = table
                .get("nodes")
                .and_then(|v| v.as_float().or_else(|| v.as_integer().map(|i| i as f64)))
            {
                config.interval.nodes = v;
            }
            if let Some(v) = table
                .get("partitions")
                .and_then(|v| v.as_float().or_else(|| v.as_integer().map(|i| i as f64)))
            {
                config.interval.partitions = v;
            }
        }
    }

    // Jobs
    if let Some(table) = parsed.get("jobs").and_then(|v| v.as_table()) {
        if let Some(v) = table.get("name_max").and_then(|v| v.as_integer()) {
            config.jobs.name_max = v;
        }
        if let Some(v) = table.get("user_max").and_then(|v| v.as_integer()) {
            config.jobs.user_max = v;
        }
        if let Some(v) = table.get("partition_max").and_then(|v| v.as_integer()) {
            config.jobs.partition_max = v;
        }
        if let Some(v) = table
            .get("nodelist_reason_max")
            .and_then(|v| v.as_integer())
        {
            config.jobs.nodelist_reason_max = v;
        }
        if let Some(v) = table.get("qos_max").and_then(|v| v.as_integer()) {
            config.jobs.qos_max = v;
        }
    }

    // Attach
    if let Some(table) = parsed.get("attach").and_then(|v| v.as_table()) {
        if let Some(v) = table.get("enabled").and_then(|v| v.as_bool()) {
            config.attach.enabled = v;
        }
        if let Some(v) = table.get("default_command").and_then(|v| v.as_str()) {
            config.attach.default_command = v.to_string();
        }
        if let Some(v) = table.get("extra_args").and_then(|v| v.as_str()) {
            config.attach.extra_args = v.to_string();
        }
    }

    // UI
    if let Some(table) = parsed.get("ui").and_then(|v| v.as_table()) {
        if let Some(v) = table.get("expert_mode").and_then(|v| v.as_bool()) {
            config.ui.expert_mode = v;
        }
        if let Some(v) = table.get("show_palette_hints").and_then(|v| v.as_bool()) {
            config.ui.show_palette_hints = v;
        }
    }

    // Safety
    if let Some(table) = parsed.get("safety").and_then(|v| v.as_table()) {
        if let Some(v) = table.get("confirm_cancel_single").and_then(|v| v.as_bool()) {
            config.safety.confirm_cancel_single = v;
        }
        if let Some(v) = table.get("confirm_bulk_actions").and_then(|v| v.as_bool()) {
            config.safety.confirm_bulk_actions = v;
        }
    }

    // Health
    if let Some(table) = parsed.get("health").and_then(|v| v.as_table()) {
        if let Some(v) = table.get("enabled").and_then(|v| v.as_bool()) {
            config.health.enabled = v;
        }
        if let Some(v) = table.get("history_size").and_then(|v| v.as_integer()) {
            config.health.history_size = v;
        }
        if let Some(v) = table
            .get("warn_pending_ratio")
            .and_then(|v| v.as_float().or_else(|| v.as_integer().map(|i| i as f64)))
        {
            config.health.warn_pending_ratio = v;
        }
        if let Some(v) = table.get("warn_down_nodes").and_then(|v| v.as_integer()) {
            config.health.warn_down_nodes = v;
        }
    }

    // View state
    if let Some(table) = parsed.get("view_state").and_then(|v| v.as_table()) {
        if let Some(v) = table.get("jobs_sort_col").and_then(|v| v.as_str()) {
            config.view_state.jobs_sort_col = v.to_string();
        }
        if let Some(v) = table.get("jobs_sort_reversed").and_then(|v| v.as_bool()) {
            config.view_state.jobs_sort_reversed = v;
        }
        if let Some(v) = table.get("nodes_sort_col").and_then(|v| v.as_str()) {
            config.view_state.nodes_sort_col = v.to_string();
        }
        if let Some(v) = table.get("nodes_sort_reversed").and_then(|v| v.as_bool()) {
            config.view_state.nodes_sort_reversed = v;
        }
        if let Some(v) = table.get("partitions_sort_col").and_then(|v| v.as_str()) {
            config.view_state.partitions_sort_col = v.to_string();
        }
        if let Some(v) = table
            .get("partitions_sort_reversed")
            .and_then(|v| v.as_bool())
        {
            config.view_state.partitions_sort_reversed = v;
        }
    }

    // Columns
    if let Some(table) = parsed.get("columns").and_then(|v| v.as_table()) {
        if let Some(arr) = table.get("jobs_hidden").and_then(|v| v.as_array()) {
            config.columns.jobs_hidden = arr
                .iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect();
        }
        if let Some(arr) = table.get("nodes_hidden").and_then(|v| v.as_array()) {
            config.columns.nodes_hidden = arr
                .iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect();
        }
        if let Some(arr) = table.get("partitions_hidden").and_then(|v| v.as_array()) {
            config.columns.partitions_hidden = arr
                .iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect();
        }
        if let Some(arr) = table.get("jobs_order").and_then(|v| v.as_array()) {
            config.columns.jobs_order = arr
                .iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect();
        }
        if let Some(arr) = table.get("nodes_order").and_then(|v| v.as_array()) {
            config.columns.nodes_order = arr
                .iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect();
        }
        if let Some(arr) = table.get("partitions_order").and_then(|v| v.as_array()) {
            config.columns.partitions_order = arr
                .iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect();
        }
    }

    // Notifications
    if let Some(table) = parsed.get("notifications").and_then(|v| v.as_table()) {
        if let Some(v) = table.get("desktop_enabled").and_then(|v| v.as_bool()) {
            config.notifications.desktop_enabled = v;
        }
    }

    // Remote
    if let Some(table) = parsed.get("remote").and_then(|v| v.as_table()) {
        if let Some(v) = table.get("host").and_then(|v| v.as_str()) {
            config.remote.host = v.to_string();
        }
    }

    // Clipboard
    if let Some(table) = parsed.get("clipboard").and_then(|v| v.as_table()) {
        if let Some(v) = table.get("transport").and_then(|v| v.as_str()) {
            config.clipboard.transport = v.to_string();
        }
    }

    // Investigation
    if let Some(table) = parsed.get("investigation").and_then(|v| v.as_table()) {
        if let Some(v) = table.get("reasons_path").and_then(|v| v.as_str()) {
            config.investigation.reasons_path = v.to_string();
        }
        if let Some(v) = table.get("max_related_jobs").and_then(|v| v.as_integer()) {
            config.investigation.max_related_jobs = v;
        }
    }

    Ok(config)
}

// ── Save and Update ───────────────────────────────────────────────────────────

/// Persist theme and broadcast interval to all three view keys.
pub fn save(path: &Path, theme: &str, interval: f64) -> Result<()> {
    let mut updates = HashMap::new();
    updates.insert("theme".to_string(), toml::Value::String(theme.to_string()));

    let mut interval_table = toml::map::Map::new();
    interval_table.insert("jobs".to_string(), toml::Value::Float(interval));
    interval_table.insert("nodes".to_string(), toml::Value::Float(interval));
    interval_table.insert("partitions".to_string(), toml::Value::Float(interval));
    updates.insert("interval".to_string(), toml::Value::Table(interval_table));

    apply_updates_to_disk(path, &updates)
}

/// Update config with shallow+section merge and persist.
pub fn update(path: &Path, overrides: &HashMap<String, toml::Value>) -> Result<()> {
    apply_updates_to_disk(path, overrides)
}

// ── toml_edit round-trip writer ──────────────────────────────────────────────

fn default_document() -> DocumentMut {
    let mut doc = DocumentMut::new();

    doc.insert("theme", Item::Value(Value::from("dracula")));

    for &section in SECTION_ORDER {
        // Add section comment
        if let Some(&(_, comment)) = SECTION_COMMENTS.iter().find(|(name, _)| *name == section) {
            doc.insert(section, Item::None);
            if let Some(item) = doc.get_mut(section) {
                item.or_insert(Item::Table(Table::new()));
                if let Some(table) = item.as_table_mut() {
                    table.set_dotted(false);
                    table.decor_mut().set_prefix(format!(
                        "# {}
",
                        comment
                    ));
                }
            }
        }

        let table = match section {
            "interval" => {
                let mut t = Table::new();
                t.insert("jobs", Item::Value(Value::from(2.0)));
                t.insert("nodes", Item::Value(Value::from(2.0)));
                t.insert("partitions", Item::Value(Value::from(5.0)));
                t
            }
            "jobs" => {
                let mut t = Table::new();
                t.insert("name_max", Item::Value(Value::from(24)));
                t.insert("user_max", Item::Value(Value::from(12)));
                t.insert("partition_max", Item::Value(Value::from(14)));
                t.insert("nodelist_reason_max", Item::Value(Value::from(40)));
                t.insert("qos_max", Item::Value(Value::from(12)));
                t
            }
            "attach" => {
                let mut t = Table::new();
                t.insert("enabled", Item::Value(Value::from(true)));
                t.insert("default_command", Item::Value(Value::from("$SHELL -l")));
                t.insert("extra_args", Item::Value(Value::from("")));
                t
            }
            "ui" => {
                let mut t = Table::new();
                t.insert("expert_mode", Item::Value(Value::from(false)));
                t.insert("show_palette_hints", Item::Value(Value::from(true)));
                t
            }
            "safety" => {
                let mut t = Table::new();
                t.insert("confirm_cancel_single", Item::Value(Value::from(true)));
                t.insert("confirm_bulk_actions", Item::Value(Value::from(true)));
                t
            }
            "health" => {
                let mut t = Table::new();
                t.insert("enabled", Item::Value(Value::from(true)));
                t.insert("history_size", Item::Value(Value::from(100)));
                t.insert("warn_pending_ratio", Item::Value(Value::from(0.7)));
                t.insert("warn_down_nodes", Item::Value(Value::from(1)));
                t
            }
            "investigation" => {
                let mut t = Table::new();
                t.insert("reasons_path", Item::Value(Value::from("")));
                t.insert("max_related_jobs", Item::Value(Value::from(20)));
                t
            }
            "view_state" => {
                let mut t = Table::new();
                t.insert("jobs_sort_col", Item::Value(Value::from("")));
                t.insert("jobs_sort_reversed", Item::Value(Value::from(false)));
                t.insert("nodes_sort_col", Item::Value(Value::from("")));
                t.insert("nodes_sort_reversed", Item::Value(Value::from(false)));
                t.insert("partitions_sort_col", Item::Value(Value::from("")));
                t.insert("partitions_sort_reversed", Item::Value(Value::from(false)));
                t
            }
            "columns" => {
                let mut t = Table::new();
                t.insert(
                    "jobs_hidden",
                    Item::Value(Value::from(toml_edit::Array::new())),
                );
                t.insert(
                    "nodes_hidden",
                    Item::Value(Value::from(toml_edit::Array::new())),
                );
                t.insert(
                    "partitions_hidden",
                    Item::Value(Value::from(toml_edit::Array::new())),
                );
                t.insert(
                    "jobs_order",
                    Item::Value(Value::from(toml_edit::Array::new())),
                );
                t.insert(
                    "nodes_order",
                    Item::Value(Value::from(toml_edit::Array::new())),
                );
                t.insert(
                    "partitions_order",
                    Item::Value(Value::from(toml_edit::Array::new())),
                );
                t
            }
            "notifications" => {
                let mut t = Table::new();
                t.insert("desktop_enabled", Item::Value(Value::from(true)));
                t
            }
            "remote" => {
                let mut t = Table::new();
                t.insert("host", Item::Value(Value::from("")));
                t
            }
            "clipboard" => {
                let mut t = Table::new();
                t.insert("transport", Item::Value(Value::from("auto")));
                t
            }
            _ => Table::new(),
        };

        doc.insert(section, Item::Table(table));
    }

    doc
}

fn read_or_init_document(path: &Path) -> DocumentMut {
    if path.exists() {
        if let Ok(content) = fs::read_to_string(path) {
            if let Ok(doc) = content.parse::<DocumentMut>() {
                return doc;
            }
        }
    }
    default_document()
}

fn migrate_legacy_interval(doc: &mut DocumentMut) {
    // Check if interval is a bare scalar (legacy format)
    if let Some(item) = doc.get("interval") {
        if item.is_value() {
            if let Some(val) = item.as_value() {
                let broadcast = if let Some(f) = val.as_float() {
                    f
                } else if let Some(i) = val.as_integer() {
                    i as f64
                } else {
                    return; // Not a number, skip migration
                };

                // Remove old scalar and replace with table
                doc.remove("interval");
                let mut table = Table::new();
                table.insert("jobs", Item::Value(Value::from(broadcast)));
                table.insert("nodes", Item::Value(Value::from(broadcast)));
                table.insert("partitions", Item::Value(Value::from(broadcast)));
                doc.insert("interval", Item::Table(table));
            }
        }
    }
}

fn ensure_table<'a>(doc: &'a mut DocumentMut, section: &str) -> Result<&'a mut Table> {
    if !doc.contains_key(section) {
        doc.insert(section, Item::Table(Table::new()));
    }

    doc[section]
        .as_table_mut()
        .ok_or_else(|| anyhow::anyhow!("config section [{}] is not a table", section))
}

fn toml_value_to_item(val: &toml::Value) -> Item {
    match val {
        toml::Value::String(s) => Item::Value(Value::from(s.as_str())),
        toml::Value::Integer(i) => Item::Value(Value::from(*i)),
        toml::Value::Float(f) => Item::Value(Value::from(*f)),
        toml::Value::Boolean(b) => Item::Value(Value::from(*b)),
        toml::Value::Array(arr) => {
            let mut result = toml_edit::Array::new();
            for item in arr {
                if let toml::Value::String(s) = item {
                    result.push(s.as_str());
                }
            }
            Item::Value(Value::from(result))
        }
        toml::Value::Table(_) => Item::Table(Table::new()),
        _ => Item::None,
    }
}

fn apply_section_updates(table: &mut Table, updates: &toml::map::Map<String, toml::Value>) {
    for (key, value) in updates {
        table.insert(key, toml_value_to_item(value));
    }
}

fn apply_updates_to_disk(path: &Path, updates: &HashMap<String, toml::Value>) -> Result<()> {
    let mut doc = read_or_init_document(path);
    migrate_legacy_interval(&mut doc);

    let nested_sections: std::collections::HashSet<&str> = SECTION_ORDER.iter().copied().collect();

    for (key, value) in updates {
        if nested_sections.contains(key.as_str()) {
            if let toml::Value::Table(table_updates) = value {
                let table = ensure_table(&mut doc, key)?;
                apply_section_updates(table, table_updates);
            }
        } else {
            // Top-level scalar
            doc.insert(key, toml_value_to_item(value));
        }
    }

    atomic_write(path, &doc.to_string())
}

fn atomic_write(path: &Path, text: &str) -> Result<()> {
    let dir = path.parent().context("config path has no parent")?;
    fs::create_dir_all(dir).context("failed to create config directory")?;

    // Create temp file in same directory
    let temp_path = dir.join(format!(".config.{}.toml.tmp", std::process::id()));

    // Write to temp file
    fs::write(&temp_path, text)
        .with_context(|| format!("failed to write temp config: {}", temp_path.display()))?;

    // Atomic rename
    fs::rename(&temp_path, path).with_context(|| {
        format!(
            "failed to rename {} to {}",
            temp_path.display(),
            path.display()
        )
    })?;

    Ok(())
}

// ── Legacy helper (kept for API parity) ───────────────────────────────────────

/// Escape a string for TOML basic-string literals.
///
/// Retained for legacy callers and tests; toml_edit handles escaping internally.
pub fn toml_escape(value: &str) -> String {
    value.replace('\\', "\\\\").replace('"', "\\\"")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::sync::Mutex;
    use tempfile::TempDir;

    // Serialize tests that mutate SQTOP_CONFIG environment variable
    static ENV_MUTEX: Mutex<()> = Mutex::new(());

    // ── load returns full default tree ────────────────────────────────────────

    #[test]
    fn test_load_returns_full_default_tree_when_no_file() {
        let temp_dir = TempDir::new().unwrap();
        let config_file = temp_dir.path().join("config.toml");

        let cfg = load(&config_file);
        assert_eq!(cfg.theme, "dracula");
        assert_eq!(
            cfg.interval,
            IntervalConfig {
                jobs: 2.0,
                nodes: 2.0,
                partitions: 5.0
            }
        );
    }

    #[test]
    fn test_load_empty_file_returns_defaults() {
        let temp_dir = TempDir::new().unwrap();
        let config_file = temp_dir.path().join("config.toml");
        fs::write(&config_file, "").unwrap();

        let cfg = load(&config_file);
        assert_eq!(cfg.theme, "dracula");
        assert_eq!(cfg.interval.jobs, 2.0);
        assert_eq!(cfg.jobs.name_max, 24);
        assert!(cfg.attach.enabled);
        assert!(cfg.safety.confirm_cancel_single);
    }

    #[test]
    fn test_load_preserves_user_set_values_per_section() {
        let temp_dir = TempDir::new().unwrap();
        let config_file = temp_dir.path().join("config.toml");
        fs::write(
            &config_file,
            r#"theme = "tokyo-night"

[interval]
jobs = 1.0
nodes = 3.0
partitions = 7.0

[jobs]
name_max = 40

[attach]
enabled = false

[ui]
expert_mode = true

[safety]
confirm_cancel_single = false

[health]
history_size = 250

[view_state]
jobs_sort_col = "JOBID"

[columns]
jobs_hidden = ["JOBID"]

[notifications]
desktop_enabled = false

[remote]
host = "login.example.org"

[clipboard]
transport = "osc52"
"#,
        )
        .unwrap();

        let cfg = load(&config_file);
        assert_eq!(cfg.theme, "tokyo-night");
        assert_eq!(cfg.interval.jobs, 1.0);
        assert_eq!(cfg.interval.nodes, 3.0);
        assert_eq!(cfg.interval.partitions, 7.0);
        assert_eq!(cfg.jobs.name_max, 40);
        assert!(!cfg.attach.enabled);
        assert!(cfg.ui.expert_mode);
        assert!(!cfg.safety.confirm_cancel_single);
        assert_eq!(cfg.health.history_size, 250);
        assert_eq!(cfg.view_state.jobs_sort_col, "JOBID");
        assert_eq!(cfg.columns.jobs_hidden, vec!["JOBID"]);
        assert!(!cfg.notifications.desktop_enabled);
        assert_eq!(cfg.remote.host, "login.example.org");
        assert_eq!(cfg.clipboard.transport, "osc52");
    }

    // ── interval back-compat & merge ──────────────────────────────────────────

    #[test]
    fn test_load_legacy_interval_top_level_float_back_compat() {
        let temp_dir = TempDir::new().unwrap();
        let config_file = temp_dir.path().join("config.toml");
        fs::write(
            &config_file,
            "interval = 3.5
",
        )
        .unwrap();

        let cfg = load(&config_file);
        assert_eq!(cfg.interval.jobs, 3.5);
        assert_eq!(cfg.interval.nodes, 3.5);
        assert_eq!(cfg.interval.partitions, 3.5);
    }

    #[test]
    fn test_load_interval_table_partial_fills_remaining_with_defaults() {
        let temp_dir = TempDir::new().unwrap();
        let config_file = temp_dir.path().join("config.toml");
        fs::write(
            &config_file,
            "[interval]
jobs = 1.0
",
        )
        .unwrap();

        let cfg = load(&config_file);
        assert_eq!(cfg.interval.jobs, 1.0);
        assert_eq!(cfg.interval.nodes, 2.0);
        assert_eq!(cfg.interval.partitions, 5.0);
    }

    // ── save / update ─────────────────────────────────────────────────────────

    #[test]
    fn test_save_broadcasts_interval_float_to_all_three_keys() {
        let temp_dir = TempDir::new().unwrap();
        let config_file = temp_dir.path().join("config.toml");

        save(&config_file, "dracula", 7.0).unwrap();

        let cfg = load(&config_file);
        assert_eq!(cfg.interval.jobs, 7.0);
        assert_eq!(cfg.interval.nodes, 7.0);
        assert_eq!(cfg.interval.partitions, 7.0);

        // Check on-disk format
        let content = fs::read_to_string(&config_file).unwrap();
        assert!(content.contains("[interval]"));
        assert!(content.contains("jobs = 7.0"));
    }

    #[test]
    fn test_update_partial_interval_does_not_clobber_other_keys() {
        let temp_dir = TempDir::new().unwrap();
        let config_file = temp_dir.path().join("config.toml");

        let mut updates = HashMap::new();
        let mut interval_table = toml::map::Map::new();
        interval_table.insert("jobs".to_string(), toml::Value::Float(1.5));
        updates.insert("interval".to_string(), toml::Value::Table(interval_table));

        update(&config_file, &updates).unwrap();

        let cfg = load(&config_file);
        assert_eq!(cfg.interval.jobs, 1.5);
        assert_eq!(cfg.interval.nodes, 2.0);
        assert_eq!(cfg.interval.partitions, 5.0);
    }

    #[test]
    fn test_update_idempotent() {
        let temp_dir = TempDir::new().unwrap();
        let config_file = temp_dir.path().join("config.toml");

        let mut updates = HashMap::new();
        let mut jobs_table = toml::map::Map::new();
        jobs_table.insert("name_max".to_string(), toml::Value::Integer(30));
        updates.insert("jobs".to_string(), toml::Value::Table(jobs_table));

        update(&config_file, &updates).unwrap();
        update(&config_file, &updates).unwrap();

        let cfg = load(&config_file);
        assert_eq!(cfg.jobs.name_max, 30);
        assert_eq!(cfg.jobs.user_max, 12);
    }

    // ── toml_escape ───────────────────────────────────────────────────────────

    #[test]
    fn test_toml_escape_backslash() {
        assert_eq!(toml_escape("a\\b"), "a\\\\b");
    }

    #[test]
    fn test_toml_escape_double_quote() {
        assert_eq!(toml_escape("a\"b"), "a\\\"b");
    }

    #[test]
    fn test_toml_escape_combined() {
        assert_eq!(toml_escape("a\\\"b"), "a\\\\\\\"b");
    }

    #[test]
    fn test_toml_escape_no_special_chars() {
        assert_eq!(toml_escape("hello world"), "hello world");
    }

    // ── legacy interval migration ─────────────────────────────────────────────

    #[test]
    fn test_legacy_top_level_interval_is_migrated_to_table_on_first_write() {
        let temp_dir = TempDir::new().unwrap();
        let config_file = temp_dir.path().join("config.toml");
        fs::write(&config_file, "interval = 3.0\ntheme = \"dracula\"\n").unwrap();

        let mut updates = HashMap::new();
        updates.insert("theme".to_string(), toml::Value::String("nord".to_string()));
        update(&config_file, &updates).unwrap();

        let rewritten = fs::read_to_string(&config_file).unwrap();
        assert!(!rewritten.contains("interval = 3.0"));
        assert!(rewritten.contains("[interval]"));
        assert!(rewritten.contains("jobs = 3.0"));
        assert!(rewritten.contains("nodes = 3.0"));
        assert!(rewritten.contains("partitions = 3.0"));
        assert!(rewritten.contains("theme = \"nord\""));

        let cfg = load(&config_file);
        assert_eq!(cfg.interval.jobs, 3.0);
        assert_eq!(cfg.interval.nodes, 3.0);
        assert_eq!(cfg.interval.partitions, 3.0);
    }

    // ── investigation section ─────────────────────────────────────────────────

    #[test]
    fn test_load_returns_investigation_section_with_reasons_path_default() {
        let temp_dir = TempDir::new().unwrap();
        let config_file = temp_dir.path().join("config.toml");

        let cfg = load(&config_file);
        assert_eq!(cfg.investigation.reasons_path, "");
        assert_eq!(cfg.investigation.max_related_jobs, 20);
    }

    #[test]
    fn test_update_persists_reasons_path() {
        let temp_dir = TempDir::new().unwrap();
        let config_file = temp_dir.path().join("config.toml");

        let mut updates = HashMap::new();
        let mut inv_table = toml::map::Map::new();
        inv_table.insert(
            "reasons_path".to_string(),
            toml::Value::String("/tmp/reasons.toml".to_string()),
        );
        updates.insert("investigation".to_string(), toml::Value::Table(inv_table));

        update(&config_file, &updates).unwrap();

        let cfg = load(&config_file);
        assert_eq!(cfg.investigation.reasons_path, "/tmp/reasons.toml");

        let raw = fs::read_to_string(&config_file).unwrap();
        assert!(raw.contains("[investigation]"));
        assert!(raw.contains("reasons_path = \"/tmp/reasons.toml\""));
    }

    #[test]
    fn test_update_persists_max_related_jobs() {
        let temp_dir = TempDir::new().unwrap();
        let config_file = temp_dir.path().join("config.toml");

        let mut updates = HashMap::new();
        let mut inv_table = toml::map::Map::new();
        inv_table.insert("max_related_jobs".to_string(), toml::Value::Integer(50));
        updates.insert("investigation".to_string(), toml::Value::Table(inv_table));

        update(&config_file, &updates).unwrap();

        let cfg = load(&config_file);
        assert_eq!(cfg.investigation.max_related_jobs, 50);

        let raw = fs::read_to_string(&config_file).unwrap();
        assert!(raw.contains("[investigation]"));
        assert!(raw.contains("max_related_jobs = 50"));
    }

    // ── resolve_config_path ───────────────────────────────────────────────────

    #[test]
    fn test_resolve_config_path_cli_arg_wins() {
        let _lock = ENV_MUTEX.lock().unwrap();
        let cli_path = PathBuf::from("/custom/path.toml");
        std::env::set_var("SQTOP_CONFIG", "/env/path.toml");

        let result = resolve_config_path(Some(cli_path.clone()));
        assert_eq!(result, cli_path);

        std::env::remove_var("SQTOP_CONFIG");
    }

    #[test]
    fn test_resolve_config_path_env_var_used_when_no_cli() {
        let _lock = ENV_MUTEX.lock().unwrap();
        std::env::set_var("SQTOP_CONFIG", "/env/path.toml");

        let result = resolve_config_path(None);
        assert_eq!(result, PathBuf::from("/env/path.toml"));

        std::env::remove_var("SQTOP_CONFIG");
    }

    #[test]
    fn test_resolve_config_path_default_when_no_overrides() {
        let _lock = ENV_MUTEX.lock().unwrap();
        std::env::remove_var("SQTOP_CONFIG");

        let result = resolve_config_path(None);
        let expected = default_config_path();
        assert_eq!(result, expected);
    }

    #[test]
    fn test_resolve_config_path_expands_tilde() {
        let _lock = ENV_MUTEX.lock().unwrap();
        let result = resolve_config_path(Some(PathBuf::from("~/sqtop.toml")));
        assert!(!result.to_string_lossy().contains('~'));
        assert!(result.is_absolute() || result.starts_with(std::env::var("HOME").unwrap()));
    }

    #[test]
    fn test_resolve_config_path_empty_cli_arg_falls_through_to_env() {
        let _lock = ENV_MUTEX.lock().unwrap();
        std::env::set_var("SQTOP_CONFIG", "/env/path.toml");

        let result = resolve_config_path(Some(PathBuf::from("")));
        assert_eq!(result, PathBuf::from("/env/path.toml"));

        std::env::remove_var("SQTOP_CONFIG");
    }

    #[test]
    fn test_resolve_config_path_whitespace_cli_arg_falls_through_to_env() {
        let _lock = ENV_MUTEX.lock().unwrap();
        std::env::set_var("SQTOP_CONFIG", "/env/path.toml");

        let result = resolve_config_path(Some(PathBuf::from("  ")));
        assert_eq!(result, PathBuf::from("/env/path.toml"));

        std::env::remove_var("SQTOP_CONFIG");
    }

    #[test]
    fn test_resolve_config_path_empty_env_falls_through_to_default() {
        let _lock = ENV_MUTEX.lock().unwrap();
        std::env::set_var("SQTOP_CONFIG", "");

        let result = resolve_config_path(None);
        let expected = default_config_path();
        assert_eq!(result, expected);

        std::env::remove_var("SQTOP_CONFIG");
    }

    #[test]
    fn test_resolve_config_path_whitespace_env_falls_through_to_default() {
        let _lock = ENV_MUTEX.lock().unwrap();
        std::env::set_var("SQTOP_CONFIG", "  ");

        let result = resolve_config_path(None);
        let expected = default_config_path();
        assert_eq!(result, expected);

        std::env::remove_var("SQTOP_CONFIG");
    }

    // ── user-supplied malformed config ────────────────────────────────────────

    #[test]
    fn test_update_returns_error_when_section_is_not_table() {
        let temp_dir = TempDir::new().unwrap();
        let config_file = temp_dir.path().join("config.toml");
        fs::write(
            &config_file,
            "jobs = 5
",
        )
        .unwrap();

        let mut updates = HashMap::new();
        let mut jobs_table = toml::map::Map::new();
        jobs_table.insert("name_max".to_string(), toml::Value::Integer(30));
        updates.insert("jobs".to_string(), toml::Value::Table(jobs_table));

        let result = update(&config_file, &updates);
        assert!(result.is_err());
        let err_msg = result.unwrap_err().to_string();
        assert!(err_msg.contains("not a table") || err_msg.contains("jobs"));
    }
}

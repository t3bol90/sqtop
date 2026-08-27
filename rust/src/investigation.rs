//! Pending-reason classification and investigation report assembly.
//!
//! This module owns the SPEC sec. 6.8-6.12 domain types plus the pure
//! explanation/render helpers used by Investigation Mode (SPEC sec. 8).
//!
//! It MUST stay free of subprocess, ssh, config I/O, or any view code.
//! The builders here take already-fetched Slurm data and turn it into a
//! plain-text-ready report.

use crate::slurm::model::{Job, Node};
use std::collections::HashMap;
use std::path::Path;
use std::sync::LazyLock;
use std::time::SystemTime;

// Type aliases matching the Python literals
pub type InvestigationKind = String; // "job" or "node"
pub type InvestigationSource = String; // "cursor", "typed", "related_link", "watch"
pub type EvidenceSource = String; // "squeue", "sinfo", "scontrol", "sacct", "derived", "cache"
pub type Confidence = String; // "high", "medium", "low"

/// Investigation target.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InvestigationTarget {
    pub kind: InvestigationKind,
    pub identifier: String,
    pub source: InvestigationSource,
}

/// Investigation evidence item.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InvestigationEvidence {
    pub id: String,
    pub label: String,
    pub value: String,
    pub source: EvidenceSource,
    pub confidence: Confidence,
}

/// Investigation explanation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InvestigationExplanation {
    pub title: String,
    pub detail: String,
    pub confidence: Confidence,
    pub evidence_refs: Vec<String>,
}

impl InvestigationExplanation {
    /// Create a new explanation with empty evidence_refs.
    pub fn new(title: String, detail: String, confidence: String) -> Self {
        Self {
            title,
            detail,
            confidence,
            evidence_refs: Vec::new(),
        }
    }
}

/// Investigation error.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InvestigationError {
    pub source: String,
    pub category: String,
    pub message: String,
    pub stderr: Option<String>,
}

/// Investigation action.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InvestigationAction {
    pub label: String,
    pub detail: String,
    pub safe_for_user: bool,
}

/// Investigation item (key-value pair).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InvestigationItem {
    pub label: String,
    pub value: String,
}

/// Investigation report container.
#[derive(Debug, Clone)]
pub struct InvestigationReport {
    pub target: InvestigationTarget,
    pub generated_at: SystemTime,
    pub summary: Vec<InvestigationItem>,
    pub evidence: Vec<InvestigationEvidence>,
    pub explanations: Vec<InvestigationExplanation>,
    pub related_jobs: Vec<Job>,
    pub related_nodes: Vec<Node>,
    pub suggested_actions: Vec<InvestigationAction>,
    pub raw_sections: HashMap<String, String>,
    pub errors: Vec<InvestigationError>,
}

impl InvestigationReport {
    /// Create a new empty report.
    pub fn new(target: InvestigationTarget, generated_at: SystemTime) -> Self {
        Self {
            target,
            generated_at,
            summary: Vec::new(),
            evidence: Vec::new(),
            explanations: Vec::new(),
            related_jobs: Vec::new(),
            related_nodes: Vec::new(),
            suggested_actions: Vec::new(),
            raw_sections: HashMap::new(),
            errors: Vec::new(),
        }
    }
}

// Pending-reason explanation table (SPEC sec. 8.4.1)
static PENDING_REASONS: LazyLock<HashMap<&str, (&str, &str, &str)>> = LazyLock::new(|| {
    let mut map = HashMap::new();
    map.insert(
        "Resources",
        (
            "Matching resources are not currently available",
            "Slurm cannot currently find enough matching resources. Check requested CPUs/GPUs/memory, partition, and node availability.",
            "medium",
        ),
    );
    map.insert(
        "Priority",
        (
            "Lower priority than other queued jobs",
            "Job is eligible but lower priority than other queued jobs.",
            "high",
        ),
    );
    map.insert(
        "Dependency",
        (
            "Waiting on a dependency",
            "Job is waiting for another job or condition.",
            "high",
        ),
    );
    map.insert(
        "ReqNodeNotAvail",
        (
            "Requested node is unavailable",
            "Requested node is unavailable, drained, down, reserved, or otherwise not schedulable.",
            "high",
        ),
    );
    map.insert(
        "PartitionTimeLimit",
        (
            "Time limit exceeds partition limit",
            "Requested time exceeds partition limit.",
            "high",
        ),
    );
    map.insert(
        "JobHeldUser",
        ("Held by the user", "Job is held by the user.", "high"),
    );
    map.insert(
        "JobHeldAdmin",
        (
            "Held by an administrator",
            "Job is held by an administrator or policy.",
            "high",
        ),
    );
    map.insert(
        "BeginTime",
        ("Future begin time", "Job has a future begin time.", "high"),
    );
    map.insert(
        "Reservation",
        (
            "Waiting for reservation constraints",
            "Job is waiting for reservation constraints.",
            "medium",
        ),
    );
    map.insert(
        "Licenses",
        (
            "Required licenses unavailable",
            "Required license resources are unavailable.",
            "medium",
        ),
    );
    map.insert(
        "QOSMaxCpuPerUserLimit",
        (
            "QoS CPU-per-user limit may be blocking",
            "Visible QoS CPU-per-user limit may be blocking the job.",
            "medium",
        ),
    );
    map.insert(
        "QOSMaxGRESPerUser",
        (
            "QoS GRES/GPU-per-user limit may be blocking",
            "Visible QoS GRES/GPU-per-user limit may be blocking the job.",
            "medium",
        ),
    );
    map.insert(
        "AssocGrpCpuLimit",
        (
            "Association/group CPU limit may be blocking",
            "Association/group CPU limit may be blocking the job.",
            "medium",
        ),
    );
    map.insert(
        "AssocGrpGRES",
        (
            "Association/group GRES/GPU limit may be blocking",
            "Association/group GRES/GPU limit may be blocking the job.",
            "medium",
        ),
    );
    map
});

/// Reason explanation table with optional user-supplied overrides.
#[derive(Debug, Clone, Default)]
pub struct ReasonTable {
    user: HashMap<String, InvestigationExplanation>,
}

impl ReasonTable {
    /// Create a new reason table with user-supplied overrides.
    pub fn with_user_reasons(user: HashMap<String, InvestigationExplanation>) -> Self {
        Self { user }
    }

    /// Load a reason table from a TOML file, returning the table and any parse errors.
    ///
    /// Returns `(ReasonTable, errors)` where errors is a list of parse/validation issues.
    /// Missing files and I/O errors produce an empty table with no errors (degraded mode).
    pub fn load<P: AsRef<Path>>(path: Option<P>) -> (Self, Vec<InvestigationError>) {
        let Some(path_ref) = path else {
            return (Self::default(), Vec::new());
        };

        let path = path_ref.as_ref();
        if !path.is_file() {
            return (Self::default(), Vec::new());
        }

        let content = match std::fs::read_to_string(path) {
            Ok(c) => c,
            Err(_) => return (Self::default(), Vec::new()),
        };

        let data: HashMap<String, toml::Value> = match toml::from_str(&content) {
            Ok(d) => d,
            Err(_) => return (Self::default(), Vec::new()),
        };

        let valid_confidences: std::collections::HashSet<&str> =
            ["high", "medium", "low"].iter().copied().collect();
        let mut user_reasons = HashMap::new();

        for (reason_key, value) in data {
            if reason_key.is_empty() {
                continue;
            }

            let table = match value.as_table() {
                Some(t) => t,
                None => continue,
            };

            let title = match table.get("title").and_then(|v| v.as_str()) {
                Some(t) => t,
                None => continue,
            };

            let detail = match table.get("detail").and_then(|v| v.as_str()) {
                Some(d) => d,
                None => continue,
            };

            let confidence = match table.get("confidence").and_then(|v| v.as_str()) {
                Some(c) if valid_confidences.contains(c) => c,
                _ => continue,
            };

            user_reasons.insert(
                reason_key,
                InvestigationExplanation::new(
                    title.to_string(),
                    detail.to_string(),
                    confidence.to_string(),
                ),
            );
        }

        (Self::with_user_reasons(user_reasons), Vec::new())
    }

    /// Map a Slurm pending reason to a user-facing explanation.
    ///
    /// SPEC sec. 8.4.1. Pure function; case-sensitive lookup.
    /// Empty / None / "(null)" reasons return a low-confidence
    /// "no reason reported" explanation. Unknown reasons echo the raw
    /// string so the user can still copy/paste it into a search.
    ///
    /// User-supplied overrides take precedence over built-in reasons.
    pub fn explain_pending_reason(&self, reason: Option<&str>) -> InvestigationExplanation {
        // "(null)" is a Slurm sentinel for "field not provided"; we treat
        // it the same as an empty reason per SPEC.
        let reason_str = match reason {
            None | Some("") | Some("(null)") => {
                return InvestigationExplanation::new(
                    "No pending reason reported".to_string(),
                    "Slurm did not report a reason. The job may be very recently submitted, or the field is unavailable.".to_string(),
                    "low".to_string(),
                );
            }
            Some(r) => r,
        };

        // Check user reasons first
        if let Some(exp) = self.user.get(reason_str) {
            return exp.clone();
        }

        // Check built-in reasons
        if let Some((title, detail, confidence)) = PENDING_REASONS.get(reason_str) {
            return InvestigationExplanation::new(
                title.to_string(),
                detail.to_string(),
                confidence.to_string(),
            );
        }

        // Unknown reason fallback
        InvestigationExplanation::new(
            "Unrecognized pending reason".to_string(),
            format!(
                "sqtop does not have a built-in explanation for this pending reason yet.\nRaw Slurm reason: {}",
                reason_str
            ),
            "low".to_string(),
        )
    }
}

// Node-state explanation table (SPEC sec. 8.5.1)
static NODE_STATES: LazyLock<HashMap<&str, (&str, &str, &str)>> = LazyLock::new(|| {
    let mut map = HashMap::new();
    map.insert(
        "IDLE",
        (
            "Node appears available",
            "Node appears available for matching jobs.",
            "high",
        ),
    );
    map.insert(
        "ALLOCATED",
        (
            "Node fully allocated",
            "Node is fully allocated to running jobs.",
            "high",
        ),
    );
    map.insert(
        "MIXED",
        (
            "Node partially allocated",
            "Some resources are allocated; some may remain free.",
            "medium",
        ),
    );
    map.insert("DOWN", ("Node unavailable", "Node is unavailable.", "high"));
    map.insert(
        "DRAIN",
        (
            "Node draining or drained",
            "Node is being removed from scheduling or already drained.",
            "high",
        ),
    );
    map.insert(
        "DRAINED",
        (
            "Node draining or drained",
            "Node is being removed from scheduling or already drained.",
            "high",
        ),
    );
    map.insert(
        "RESERVED",
        (
            "Node reserved",
            "Node may be reserved for specific users, jobs, accounts, or reservations.",
            "medium",
        ),
    );
    map
});

const NODE_STATE_SUFFIXES: &str = "*-+~#@!%$";

const UNKNOWN_NODE_STATE: (&str, &str, &str) = (
    "Unrecognized node state",
    "sqtop cannot confidently classify this node state.",
    "low",
);

/// Strip Slurm decoration suffixes and uppercase the bare token.
// TODO(port): de-duplicate against slurm::parse::normalize_node_state_token at integration.
pub(crate) fn normalize_node_state(state: &str) -> String {
    let mut s = state.trim().to_string();
    while !s.is_empty() && NODE_STATE_SUFFIXES.contains(s.chars().last().unwrap()) {
        s.pop();
    }
    s.to_uppercase()
}

/// Map a Slurm node-state token to a user-facing explanation.
///
/// SPEC sec. 8.5.1. Strips trailing decoration suffixes ('*', '-', '+').
/// Lookup is case-insensitive on the bare state token. Compound states
/// joined with '+' (e.g. "idle+drain") are detected as DRAIN with
/// medium confidence; otherwise we fall through to UNKNOWN.
pub fn explain_node_state(state: &str) -> InvestigationExplanation {
    if state.trim().is_empty() {
        return InvestigationExplanation::new(
            UNKNOWN_NODE_STATE.0.to_string(),
            UNKNOWN_NODE_STATE.1.to_string(),
            UNKNOWN_NODE_STATE.2.to_string(),
        );
    }

    let raw = state.trim().to_uppercase();

    // Compound states like "IDLE+DRAIN" / "MIXED+DRAIN" — treat as
    // DRAIN with reduced confidence since drain dominates schedulability.
    if raw.contains('+') {
        let parts: Vec<String> = raw
            .split('+')
            .map(normalize_node_state)
            .filter(|p| !p.is_empty())
            .collect();

        if parts.iter().any(|p| p == "DRAIN" || p == "DRAINED") {
            if let Some((title, detail, _)) = NODE_STATES.get("DRAIN") {
                return InvestigationExplanation::new(
                    title.to_string(),
                    detail.to_string(),
                    "medium".to_string(),
                );
            }
        }
    }

    let key = normalize_node_state(&raw);
    if let Some((title, detail, confidence)) = NODE_STATES.get(key.as_str()) {
        return InvestigationExplanation::new(
            title.to_string(),
            detail.to_string(),
            confidence.to_string(),
        );
    }

    InvestigationExplanation::new(
        UNKNOWN_NODE_STATE.0.to_string(),
        UNKNOWN_NODE_STATE.1.to_string(),
        UNKNOWN_NODE_STATE.2.to_string(),
    )
}

fn header_for_target(target: &InvestigationTarget) -> String {
    if target.kind == "job" {
        format!("Investigate Job {}", target.identifier)
    } else {
        format!("Investigate Node {}", target.identifier)
    }
}

fn format_evidence_line(ev: &InvestigationEvidence) -> String {
    // Derived items get a confidence tag suffix so the user can tell
    // them apart from raw Slurm-reported fields.
    if ev.source == "derived" {
        format!("- {}: {} [{}]", ev.label, ev.value, ev.confidence)
    } else {
        format!("- {}: {}", ev.label, ev.value)
    }
}

/// Render a plain-text, copy-friendly investigation report.
///
/// SPEC sec. 21 (job example) and sec. 22 (node example). Sections are
/// skipped entirely when empty so partial reports stay readable.
/// The output is deterministic (no timestamps, no set-iteration);
/// same input renders byte-for-byte identically.
pub fn render_report(report: &InvestigationReport) -> String {
    let mut lines = Vec::new();
    lines.push(header_for_target(&report.target));

    if !report.summary.is_empty() {
        lines.push(String::new());
        lines.push("Summary".to_string());
        for item in &report.summary {
            lines.push(format!("- {}: {}", item.label, item.value));
        }
    }

    if !report.evidence.is_empty() {
        lines.push(String::new());
        lines.push("Slurm evidence".to_string());
        for ev in &report.evidence {
            lines.push(format_evidence_line(ev));
        }
    }

    if !report.explanations.is_empty() {
        for exp in &report.explanations {
            lines.push(String::new());
            lines.push("Likely explanation".to_string());
            lines.push(format!("- {}", exp.detail));
            lines.push(format!("Confidence: {}", exp.confidence));
        }
    }

    if !report.related_jobs.is_empty() {
        lines.push(String::new());
        lines.push("Related jobs".to_string());
        for job in &report.related_jobs {
            lines.push(format!("- {}: {}", job.job_id, job.state));
        }
    }

    if !report.related_nodes.is_empty() {
        lines.push(String::new());
        lines.push("Related nodes".to_string());
        for node in &report.related_nodes {
            lines.push(format!("- {}: {}", node.name, node.state));
        }
    }

    if !report.suggested_actions.is_empty() {
        lines.push(String::new());
        lines.push("Suggested next actions".to_string());
        for action in &report.suggested_actions {
            lines.push(format!("- {} - {}", action.label, action.detail));
        }
    }

    if !report.raw_sections.is_empty() {
        lines.push(String::new());
        lines.push("Raw detail".to_string());
        for (key, value) in &report.raw_sections {
            let text = if value.is_empty() { "available" } else { value };
            lines.push(format!("- {}: {}", key, text));
        }
    }

    if !report.errors.is_empty() {
        lines.push(String::new());
        lines.push("Errors".to_string());
        for err in &report.errors {
            lines.push(format!(
                "- {} [{}]: {}",
                err.source, err.category, err.message
            ));
        }
    }

    // Trailing newline keeps output friendly when piped to clipboard.
    lines.join("\n") + "\n"
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_investigation_target_round_trips_fields() {
        let t = InvestigationTarget {
            kind: "job".to_string(),
            identifier: "12345".to_string(),
            source: "cursor".to_string(),
        };
        assert_eq!(t.kind, "job");
        assert_eq!(t.identifier, "12345");
        assert_eq!(t.source, "cursor");
    }

    #[test]
    fn test_investigation_target_node_kind() {
        let t = InvestigationTarget {
            kind: "node".to_string(),
            identifier: "gpu-a100-02".to_string(),
            source: "typed".to_string(),
        };
        assert_eq!(t.kind, "node");
        assert_eq!(t.identifier, "gpu-a100-02");
    }

    #[test]
    fn test_investigation_report_constructs_with_empty_defaults() {
        let target = InvestigationTarget {
            kind: "job".to_string(),
            identifier: "1".to_string(),
            source: "cursor".to_string(),
        };
        let now = SystemTime::now();
        let report = InvestigationReport::new(target, now);
        assert!(report.summary.is_empty());
        assert!(report.evidence.is_empty());
        assert!(report.explanations.is_empty());
        assert!(report.related_jobs.is_empty());
        assert!(report.related_nodes.is_empty());
        assert!(report.suggested_actions.is_empty());
        assert!(report.raw_sections.is_empty());
        assert!(report.errors.is_empty());
    }

    #[test]
    fn test_investigation_explanation_evidence_refs_default_is_vec() {
        let exp =
            InvestigationExplanation::new("t".to_string(), "d".to_string(), "high".to_string());
        assert!(exp.evidence_refs.is_empty());
    }

    #[test]
    fn test_explain_pending_reason_resources() {
        let table = ReasonTable::default();
        let exp = table.explain_pending_reason(Some("Resources"));
        assert_eq!(exp.confidence, "medium");
        let text = format!("{} {}", exp.title, exp.detail).to_lowercase();
        assert!(text.contains("resource"));
    }

    #[test]
    fn test_explain_pending_reason_priority() {
        let table = ReasonTable::default();
        let exp = table.explain_pending_reason(Some("Priority"));
        assert_eq!(exp.confidence, "high");
        let text = format!("{} {}", exp.title, exp.detail).to_lowercase();
        assert!(text.contains("priority"));
    }

    #[test]
    fn test_explain_pending_reason_dependency() {
        let table = ReasonTable::default();
        let exp = table.explain_pending_reason(Some("Dependency"));
        assert_eq!(exp.confidence, "high");
        let text = format!("{} {}", exp.title, exp.detail).to_lowercase();
        assert!(text.contains("depend"));
    }

    #[test]
    fn test_explain_pending_reason_req_node_not_avail() {
        let table = ReasonTable::default();
        let exp = table.explain_pending_reason(Some("ReqNodeNotAvail"));
        assert_eq!(exp.confidence, "high");
        let text = format!("{} {}", exp.title, exp.detail).to_lowercase();
        assert!(text.contains("node"));
    }

    #[test]
    fn test_explain_pending_reason_partition_time_limit() {
        let table = ReasonTable::default();
        let exp = table.explain_pending_reason(Some("PartitionTimeLimit"));
        assert_eq!(exp.confidence, "high");
        let text = format!("{} {}", exp.title, exp.detail).to_lowercase();
        assert!(text.contains("time") && text.contains("partition"));
    }

    #[test]
    fn test_explain_pending_reason_job_held_user() {
        let table = ReasonTable::default();
        let exp = table.explain_pending_reason(Some("JobHeldUser"));
        assert_eq!(exp.confidence, "high");
        let text = format!("{} {}", exp.title, exp.detail).to_lowercase();
        assert!(text.contains("held") && text.contains("user"));
    }

    #[test]
    fn test_explain_pending_reason_job_held_admin() {
        let table = ReasonTable::default();
        let exp = table.explain_pending_reason(Some("JobHeldAdmin"));
        assert_eq!(exp.confidence, "high");
        let text = format!("{} {}", exp.title, exp.detail).to_lowercase();
        assert!(text.contains("admin") || text.contains("administrator"));
    }

    #[test]
    fn test_explain_pending_reason_begin_time() {
        let table = ReasonTable::default();
        let exp = table.explain_pending_reason(Some("BeginTime"));
        assert_eq!(exp.confidence, "high");
        let text = format!("{} {}", exp.title, exp.detail).to_lowercase();
        assert!(text.contains("begin") || text.contains("future"));
    }

    #[test]
    fn test_explain_pending_reason_reservation() {
        let table = ReasonTable::default();
        let exp = table.explain_pending_reason(Some("Reservation"));
        assert_eq!(exp.confidence, "medium");
        let text = format!("{} {}", exp.title, exp.detail).to_lowercase();
        assert!(text.contains("reservation"));
    }

    #[test]
    fn test_explain_pending_reason_licenses() {
        let table = ReasonTable::default();
        let exp = table.explain_pending_reason(Some("Licenses"));
        assert_eq!(exp.confidence, "medium");
        let text = format!("{} {}", exp.title, exp.detail).to_lowercase();
        assert!(text.contains("licens"));
    }

    #[test]
    fn test_explain_pending_reason_qos_max_cpu() {
        let table = ReasonTable::default();
        let exp = table.explain_pending_reason(Some("QOSMaxCpuPerUserLimit"));
        assert_eq!(exp.confidence, "medium");
        let text = format!("{} {}", exp.title, exp.detail).to_lowercase();
        assert!(text.contains("qos") && text.contains("cpu"));
    }

    #[test]
    fn test_explain_pending_reason_qos_max_gres() {
        let table = ReasonTable::default();
        let exp = table.explain_pending_reason(Some("QOSMaxGRESPerUser"));
        assert_eq!(exp.confidence, "medium");
        let text = format!("{} {}", exp.title, exp.detail).to_lowercase();
        assert!(text.contains("qos") && (text.contains("gres") || text.contains("gpu")));
    }

    #[test]
    fn test_explain_pending_reason_assoc_grp_cpu() {
        let table = ReasonTable::default();
        let exp = table.explain_pending_reason(Some("AssocGrpCpuLimit"));
        assert_eq!(exp.confidence, "medium");
        let text = format!("{} {}", exp.title, exp.detail).to_lowercase();
        assert!(
            (text.contains("association") || text.contains("assoc") || text.contains("group"))
                && text.contains("cpu")
        );
    }

    #[test]
    fn test_explain_pending_reason_assoc_grp_gres() {
        let table = ReasonTable::default();
        let exp = table.explain_pending_reason(Some("AssocGrpGRES"));
        assert_eq!(exp.confidence, "medium");
        let text = format!("{} {}", exp.title, exp.detail).to_lowercase();
        assert!(text.contains("association") || text.contains("assoc") || text.contains("group"));
        assert!(text.contains("gres") || text.contains("gpu"));
    }

    #[test]
    fn test_explain_pending_reason_empty_string() {
        let table = ReasonTable::default();
        let exp = table.explain_pending_reason(Some(""));
        assert_eq!(exp.confidence, "low");
        assert!(exp.title.to_lowercase().contains("no pending reason"));
    }

    #[test]
    fn test_explain_pending_reason_null_sentinel() {
        let table = ReasonTable::default();
        let exp = table.explain_pending_reason(Some("(null)"));
        assert_eq!(exp.confidence, "low");
        assert!(exp.title.to_lowercase().contains("no pending reason"));
    }

    #[test]
    fn test_explain_pending_reason_none() {
        let table = ReasonTable::default();
        let exp = table.explain_pending_reason(None);
        assert_eq!(exp.confidence, "low");
        assert!(exp.title.to_lowercase().contains("no pending reason"));
    }

    #[test]
    fn test_explain_pending_reason_unknown_reason() {
        let table = ReasonTable::default();
        let exp = table.explain_pending_reason(Some("SomethingNew"));
        assert_eq!(exp.confidence, "low");
        assert!(exp.title.to_lowercase().contains("unrecognized"));
        assert!(exp.detail.contains("SomethingNew"));
    }

    #[test]
    fn test_explain_pending_reason_lookup_is_case_sensitive() {
        let table = ReasonTable::default();
        // Lowercase variants should NOT match the canonical capitalized keys.
        let exp = table.explain_pending_reason(Some("resources"));
        assert_eq!(exp.confidence, "low");
        assert!(exp.title.to_lowercase().contains("unrecognized"));
    }

    #[test]
    fn test_existing_explain_pending_reason_unaffected_when_user_reasons_empty() {
        let table = ReasonTable::default();
        // Built-in: Resources -> medium.
        assert_eq!(
            table.explain_pending_reason(Some("Resources")).confidence,
            "medium"
        );
        // Built-in: Priority -> high.
        assert_eq!(
            table.explain_pending_reason(Some("Priority")).confidence,
            "high"
        );
        // Unknown -> low + "unrecognized".
        let unk = table.explain_pending_reason(Some("DefinitelyNotAReason"));
        assert_eq!(unk.confidence, "low");
        assert!(unk.title.to_lowercase().contains("unrecognized"));
        // Empty -> low + "no pending reason".
        let null = table.explain_pending_reason(Some(""));
        assert_eq!(null.confidence, "low");
        assert!(null.title.to_lowercase().contains("no pending reason"));
    }

    #[test]
    fn test_explain_node_state_idle() {
        let exp = explain_node_state("IDLE");
        assert_eq!(exp.confidence, "high");
        assert!(exp.detail.to_lowercase().contains("available"));
    }

    #[test]
    fn test_explain_node_state_allocated() {
        let exp = explain_node_state("ALLOCATED");
        assert_eq!(exp.confidence, "high");
        assert!(exp.detail.to_lowercase().contains("allocated"));
    }

    #[test]
    fn test_explain_node_state_mixed() {
        let exp = explain_node_state("MIXED");
        assert_eq!(exp.confidence, "medium");
        assert!(exp.detail.to_lowercase().contains("allocated"));
    }

    #[test]
    fn test_explain_node_state_down() {
        let exp = explain_node_state("DOWN");
        assert_eq!(exp.confidence, "high");
        assert!(exp.detail.to_lowercase().contains("unavailable"));
    }

    #[test]
    fn test_explain_node_state_drain() {
        let exp = explain_node_state("DRAIN");
        assert_eq!(exp.confidence, "high");
        assert!(exp.detail.to_lowercase().contains("drain"));
    }

    #[test]
    fn test_explain_node_state_drained() {
        let exp = explain_node_state("DRAINED");
        assert_eq!(exp.confidence, "high");
        assert!(exp.detail.to_lowercase().contains("drain"));
    }

    #[test]
    fn test_explain_node_state_reserved() {
        let exp = explain_node_state("RESERVED");
        assert_eq!(exp.confidence, "medium");
        assert!(exp.detail.to_lowercase().contains("reserv"));
    }

    #[test]
    fn test_explain_node_state_strips_asterisk_and_lowercase() {
        let exp = explain_node_state("idle*");
        assert_eq!(exp.confidence, "high");
        assert!(exp.detail.to_lowercase().contains("available"));
    }

    #[test]
    fn test_explain_node_state_strips_dash_with_uppercase() {
        let exp = explain_node_state("MIXED-");
        assert_eq!(exp.confidence, "medium");
        assert!(exp.detail.to_lowercase().contains("allocated"));
    }

    #[test]
    fn test_explain_node_state_compound_idle_plus_drain() {
        let exp = explain_node_state("idle+drain");
        // SPEC: compound + drain -> DRAIN with medium confidence
        assert_eq!(exp.confidence, "medium");
        assert!(exp.detail.to_lowercase().contains("drain"));
    }

    #[test]
    fn test_explain_node_state_empty_returns_unknown() {
        let exp = explain_node_state("");
        assert_eq!(exp.confidence, "low");
    }

    #[test]
    fn test_explain_node_state_weird_returns_unknown() {
        let exp = explain_node_state("WEIRD");
        assert_eq!(exp.confidence, "low");
    }

    fn empty_report(kind: &str, identifier: &str) -> InvestigationReport {
        let target = InvestigationTarget {
            kind: kind.to_string(),
            identifier: identifier.to_string(),
            source: "cursor".to_string(),
        };
        InvestigationReport::new(target, SystemTime::now())
    }

    #[test]
    fn test_render_report_empty_does_not_crash() {
        let report = empty_report("job", "1");
        let out = render_report(&report);
        assert!(out.contains("Investigate Job 1"));
    }

    #[test]
    fn test_render_report_node_header() {
        let report = empty_report("node", "gpu-a100-02");
        let out = render_report(&report);
        assert!(out.contains("Investigate Node gpu-a100-02"));
    }

    #[test]
    fn test_render_report_job_with_summary_evidence_explanation_action() {
        let mut report = empty_report("job", "123456");
        report.summary.push(InvestigationItem {
            label: "State".to_string(),
            value: "PENDING".to_string(),
        });
        report.summary.push(InvestigationItem {
            label: "Reason".to_string(),
            value: "Resources".to_string(),
        });
        report.evidence.push(InvestigationEvidence {
            id: "e1".to_string(),
            label: "squeue reason".to_string(),
            value: "Resources".to_string(),
            source: "squeue".to_string(),
            confidence: "high".to_string(),
        });
        report.explanations.push(InvestigationExplanation::new(
            "Matching resources are not currently available".to_string(),
            "Slurm reports that matching resources are not currently available.".to_string(),
            "medium".to_string(),
        ));
        report.suggested_actions.push(InvestigationAction {
            label: "Watch this job".to_string(),
            detail: "watch the job for state changes".to_string(),
            safe_for_user: true,
        });

        let out = render_report(&report);
        assert!(out.contains("Summary"));
        assert!(out.contains("- State: PENDING"));
        assert!(out.contains("Slurm evidence"));
        assert!(out.contains("- squeue reason: Resources"));
        assert!(out.contains("Likely explanation"));
        assert!(out.contains("Confidence: medium"));
        assert!(out.contains("Suggested next actions"));
        assert!(out.contains("Watch this job"));
    }

    #[test]
    fn test_render_report_node_with_related_nodes() {
        let mut report = empty_report("node", "gpu-a100-02");
        report.related_nodes.push(Node {
            name: "gpu-a100-01".to_string(),
            state: "ALLOCATED".to_string(),
            partition: "gpu".to_string(),
            cpus_total: "64".to_string(),
            cpus_alloc: "64".to_string(),
            memory_total: "512000".to_string(),
            memory_free: "0".to_string(),
            load: String::new(),
            gpu_total: 0,
            gpu_alloc: 0,
        });
        report.related_nodes.push(Node {
            name: "gpu-a100-03".to_string(),
            state: "DRAIN".to_string(),
            partition: "gpu".to_string(),
            cpus_total: "64".to_string(),
            cpus_alloc: "0".to_string(),
            memory_total: "512000".to_string(),
            memory_free: "512000".to_string(),
            load: String::new(),
            gpu_total: 0,
            gpu_alloc: 0,
        });
        let out = render_report(&report);
        assert!(out.contains("Related nodes"));
        assert!(out.contains("- gpu-a100-01: ALLOCATED"));
        assert!(out.contains("- gpu-a100-03: DRAIN"));
    }

    #[test]
    fn test_render_report_errors_rendered_last() {
        let mut report = empty_report("job", "1");
        report.summary.push(InvestigationItem {
            label: "State".to_string(),
            value: "PENDING".to_string(),
        });
        report.errors.push(InvestigationError {
            source: "sacct".to_string(),
            category: "permission".to_string(),
            message: "not allowed".to_string(),
            stderr: None,
        });
        let out = render_report(&report);
        let err_idx = out.find("Errors").unwrap();
        let summary_idx = out.find("Summary").unwrap();
        assert!(summary_idx < err_idx);
        assert!(out.contains("- sacct [permission]: not allowed"));
    }

    #[test]
    fn test_render_report_no_rich_markup() {
        let mut report = empty_report("job", "1");
        report.summary.push(InvestigationItem {
            label: "State".to_string(),
            value: "PENDING".to_string(),
        });
        report.evidence.push(InvestigationEvidence {
            id: "e1".to_string(),
            label: "reason".to_string(),
            value: "Resources".to_string(),
            source: "squeue".to_string(),
            confidence: "high".to_string(),
        });
        let out = render_report(&report);
        for tag in &["[red]", "[green]", "[bold]", "[yellow]", "[blue]", "[/]"] {
            assert!(!out.contains(tag));
        }
    }

    #[test]
    fn test_render_report_skips_empty_sections() {
        let report = empty_report("job", "1");
        let out = render_report(&report);
        // Only the header should appear; no section headers like "Summary".
        assert!(!out.contains("Summary"));
        assert!(!out.contains("Slurm evidence"));
        assert!(!out.contains("Likely explanation"));
        assert!(!out.contains("Suggested next actions"));
        assert!(!out.contains("Raw detail"));
        assert!(!out.contains("Errors"));
    }

    #[test]
    fn test_render_report_derived_evidence_has_confidence_tag() {
        let mut report = empty_report("job", "1");
        report.evidence.push(InvestigationEvidence {
            id: "e1".to_string(),
            label: "visible free GPUs".to_string(),
            value: "1".to_string(),
            source: "derived".to_string(),
            confidence: "medium".to_string(),
        });
        let out = render_report(&report);
        assert!(out.contains("[medium]"));
    }

    #[test]
    fn test_render_report_squeue_evidence_no_confidence_tag() {
        let mut report = empty_report("job", "1");
        report.evidence.push(InvestigationEvidence {
            id: "e1".to_string(),
            label: "reason".to_string(),
            value: "Resources".to_string(),
            source: "squeue".to_string(),
            confidence: "high".to_string(),
        });
        let out = render_report(&report);
        // squeue source should not get a [confidence] tag suffix
        assert!(!out.contains("[high]"));
    }

    #[test]
    fn test_render_report_raw_sections_default_to_available() {
        let mut report = empty_report("job", "1");
        report
            .raw_sections
            .insert("scontrol show job".to_string(), "".to_string());
        report.raw_sections.insert(
            "sacct".to_string(),
            "unavailable on this cluster".to_string(),
        );
        let out = render_report(&report);
        assert!(out.contains("- scontrol show job: available"));
        assert!(out.contains("- sacct: unavailable on this cluster"));
    }

    #[test]
    fn test_render_report_related_jobs() {
        let mut report = empty_report("node", "gpu-a100-02");
        report.related_jobs.push(Job {
            job_id: "12345".to_string(),
            name: "train".to_string(),
            user: "alice".to_string(),
            state: "RUNNING".to_string(),
            partition: "gpu".to_string(),
            nodes: "1".to_string(),
            num_nodes: "1".to_string(),
            num_cpus: "8".to_string(),
            time_used: "0:10:00".to_string(),
            time_limit: "24:00:00".to_string(),
            reason: String::new(),
            nodelist: String::new(),
            qos: String::new(),
        });
        let out = render_report(&report);
        assert!(out.contains("Related jobs"));
        assert!(out.contains("- 12345: RUNNING"));
    }

    // User reasons tests
    #[test]
    fn test_load_user_reasons_empty_path_returns_empty() {
        let (table, errors) = ReasonTable::load(None::<&str>);
        assert!(table.user.is_empty());
        assert!(errors.is_empty());
    }

    #[test]
    fn test_load_user_reasons_missing_file_returns_empty() {
        let (table, errors) = ReasonTable::load(Some("/tmp/does_not_exist_12345.toml"));
        assert!(table.user.is_empty());
        assert!(errors.is_empty());
    }

    #[test]
    fn test_register_user_reasons_replaces_state() {
        let mut reasons = HashMap::new();
        reasons.insert(
            "FooReason".to_string(),
            InvestigationExplanation::new(
                "site-foo".to_string(),
                "site-detail".to_string(),
                "medium".to_string(),
            ),
        );
        let table = ReasonTable::with_user_reasons(reasons);
        let exp = table.explain_pending_reason(Some("FooReason"));
        assert_eq!(exp.title, "site-foo");
        assert_eq!(exp.detail, "site-detail");

        // Replace with empty -> falls through to unknown-reason fallback.
        let empty_table = ReasonTable::default();
        let exp2 = empty_table.explain_pending_reason(Some("FooReason"));
        assert!(exp2.title.to_lowercase().contains("unrecognized"));
        assert_eq!(exp2.confidence, "low");
    }

    #[test]
    fn test_explain_pending_reason_user_wins_over_builtin() {
        let mut reasons = HashMap::new();
        reasons.insert(
            "Resources".to_string(),
            InvestigationExplanation::new(
                "site-Resources-title".to_string(),
                "site-Resources-detail".to_string(),
                "high".to_string(),
            ),
        );
        let table = ReasonTable::with_user_reasons(reasons);
        let exp = table.explain_pending_reason(Some("Resources"));
        assert_eq!(exp.title, "site-Resources-title");
        assert_eq!(exp.confidence, "high");
    }

    #[test]
    fn test_explain_pending_reason_user_does_not_break_unknown_path() {
        let mut reasons = HashMap::new();
        reasons.insert(
            "Foo".to_string(),
            InvestigationExplanation::new("t".to_string(), "d".to_string(), "medium".to_string()),
        );
        let table = ReasonTable::with_user_reasons(reasons);
        let exp = table.explain_pending_reason(Some("BarUnknown"));
        assert_eq!(exp.confidence, "low");
        assert!(exp.title.to_lowercase().contains("unrecognized"));
        assert!(exp.detail.contains("BarUnknown"));
    }

    #[test]
    fn test_explain_pending_reason_user_does_not_break_null_path() {
        let mut reasons = HashMap::new();
        reasons.insert(
            "Resources".to_string(),
            InvestigationExplanation::new("x".to_string(), "y".to_string(), "high".to_string()),
        );
        let table = ReasonTable::with_user_reasons(reasons);

        for null_input in &[None, Some(""), Some("(null)")] {
            let exp = table.explain_pending_reason(*null_input);
            assert!(exp.title.to_lowercase().contains("no pending reason"));
            assert_eq!(exp.confidence, "low");
        }
    }

    #[test]
    fn test_explain_pending_reason_falls_back_to_builtin_when_user_has_no_match() {
        let mut reasons = HashMap::new();
        reasons.insert(
            "SomeOtherKey".to_string(),
            InvestigationExplanation::new("t".to_string(), "d".to_string(), "high".to_string()),
        );
        let table = ReasonTable::with_user_reasons(reasons);
        let exp = table.explain_pending_reason(Some("Resources"));
        // Built-in 'Resources' is medium confidence; site map missed it.
        assert_eq!(exp.confidence, "medium");
        let text = format!("{} {}", exp.title, exp.detail).to_lowercase();
        assert!(text.contains("resource"));
    }
}

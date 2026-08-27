//! Shared domain types for the Slurm data layer.
//!
//! These mirror the dataclasses in the Python implementation and form the
//! contract between the data layer (`slurm`) and the UI layer (`views`).

/// One recorded Slurm CLI invocation, surfaced by the Health view.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandStat {
    pub command: String,
    pub ok: bool,
    pub latency_ms: u64,
    pub stderr: String,
    pub error_category: Option<ErrorCategory>,
}

/// Outcome of a single job action (cancel, hold, release, requeue).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActionResult {
    pub job_id: String,
    pub action: String,
    pub ok: bool,
    pub message: String,
}

/// Normalized failure classes for Slurm CLI and SSH transport errors.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ErrorCategory {
    SlurmCommandNotFound,
    SlurmCommandTimeout,
    SlurmCommandFailed,
    SlurmPermissionDenied,
    SlurmFieldUnavailable,
    SshConnectionFailed,
    SshAuthFailed,
    SshCommandTimeout,
    JobNotFound,
    NodeNotFound,
}

impl ErrorCategory {
    /// Stable wire name, identical to the Python string constants.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::SlurmCommandNotFound => "slurm_command_not_found",
            Self::SlurmCommandTimeout => "slurm_command_timeout",
            Self::SlurmCommandFailed => "slurm_command_failed",
            Self::SlurmPermissionDenied => "slurm_permission_denied",
            Self::SlurmFieldUnavailable => "slurm_field_unavailable",
            Self::SshConnectionFailed => "ssh_connection_failed",
            Self::SshAuthFailed => "ssh_auth_failed",
            Self::SshCommandTimeout => "ssh_command_timeout",
            Self::JobNotFound => "job_not_found",
            Self::NodeNotFound => "node_not_found",
        }
    }
}

/// One queue entry, parsed from `squeue`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Job {
    pub job_id: String,
    pub name: String,
    pub user: String,
    pub state: String,
    pub partition: String,
    pub nodes: String,
    pub num_nodes: String,
    pub num_cpus: String,
    pub time_used: String,
    pub time_limit: String,
    pub reason: String,
    pub nodelist: String,
    pub qos: String,
}

/// One compute node, parsed from `sinfo` and enriched from `scontrol`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Node {
    pub name: String,
    pub state: String,
    pub partition: String,
    pub cpus_total: String,
    pub cpus_alloc: String,
    pub memory_total: String,
    pub memory_free: String,
    pub load: String,
    pub gpu_total: u32,
    pub gpu_alloc: u32,
}

/// One partition row, parsed from `sinfo`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ClusterSummary {
    pub partition: String,
    pub avail: String,
    pub timelimit: String,
    pub nodes: String,
    pub state: String,
    pub nodelist: String,
}

//! Command execution, error classification, and action invocation.
//!
//! Ports the Python `slurm.py` module's `_run`, `_run_result`, `classify_error`,
//! and action functions (cancel/hold/release/requeue).

use std::collections::VecDeque;
use std::process::{Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use crate::slurm::model::{ActionResult, CommandStat, ErrorCategory};

/// Classify a command failure into a normalized error category.
///
/// Pure function; matches Python `classify_error` semantics exactly.
/// Returns `None` on success (exit code 0), `Some(ErrorCategory)` otherwise.
pub fn classify_error(code: Option<i32>, stderr: &str) -> Option<ErrorCategory> {
    if code == Some(0) {
        return None;
    }

    let text = stderr.to_lowercase();

    // Exception path: code=None with distinguishing stderr substring
    if code.is_none() {
        if text.contains("timeout") {
            return Some(ErrorCategory::SlurmCommandTimeout);
        }
        if text.contains("not found") {
            return Some(ErrorCategory::SlurmCommandNotFound);
        }
        return Some(ErrorCategory::SlurmCommandFailed);
    }

    // Non-zero returncode: inspect stderr in priority order.
    // Exit code 127 from sh -c means command not found.
    if code == Some(127) {
        return Some(ErrorCategory::SlurmCommandNotFound);
    }
    // Match SSH publickey auth failure BEFORE generic "permission denied".
    if text.contains("publickey") || text.contains("authentication failed") {
        return Some(ErrorCategory::SshAuthFailed);
    }
    if text.contains("permission denied")
        || text.contains("unauthorized")
        || text.contains("not allowed")
    {
        return Some(ErrorCategory::SlurmPermissionDenied);
    }
    if text.contains("connection refused")
        || text.contains("could not resolve hostname")
        || text.contains("connection closed")
    {
        return Some(ErrorCategory::SshConnectionFailed);
    }
    if text.contains("timeout") {
        return Some(ErrorCategory::SlurmCommandTimeout);
    }
    if text.contains("invalid job id")
        || text.contains("job not found")
        || text.contains("unknown job")
    {
        return Some(ErrorCategory::JobNotFound);
    }
    if text.contains("invalid node")
        || text.contains("node not found")
        || text.contains("unknown node")
    {
        return Some(ErrorCategory::NodeNotFound);
    }
    Some(ErrorCategory::SlurmCommandFailed)
}

const HISTORY_CAP: usize = 100;

struct RemoteTarget {
    ssh_host: Option<String>,
    ssh_key: Option<String>,
}

/// Slurm command runner with SSH remoting and bounded command history.
///
/// Matches Python `_run`, `_run_result`, `_record_command`, and SSH globals.
#[derive(Clone)]
pub struct Runner {
    inner: Arc<RunnerInner>,
}

struct RunnerInner {
    remote: Mutex<RemoteTarget>,
    history: Mutex<VecDeque<CommandStat>>,
}

impl Runner {
    /// Create a new runner with empty SSH config and history.
    pub fn new() -> Self {
        Self {
            inner: Arc::new(RunnerInner {
                remote: Mutex::new(RemoteTarget {
                    ssh_host: None,
                    ssh_key: None,
                }),
                history: Mutex::new(VecDeque::with_capacity(HISTORY_CAP)),
            }),
        }
    }

    /// Configure SSH remoting.
    ///
    /// Matches Python `set_remote(host, key)`. Empty strings clear the config.
    pub fn set_remote(&self, host: String, key: String) {
        let host_trimmed = host.trim();
        let key_trimmed = key.trim();
        let mut remote = self.inner.remote.lock().unwrap();
        remote.ssh_host = if host_trimmed.is_empty() {
            None
        } else {
            Some(host_trimmed.to_string())
        };
        remote.ssh_key = if key_trimmed.is_empty() {
            None
        } else {
            Some(key_trimmed.to_string())
        };
    }

    /// Run a command and return (stdout, ok, stderr).
    ///
    /// Matches Python `_run_result(cmd)` semantics: 10 s timeout, records stats,
    /// classifies errors. Runs locally or via SSH depending on `set_remote`.
    pub fn run_result(&self, cmd: &str) -> (String, bool, String) {
        let start = Instant::now();

        // Build command: local or SSH
        let mut process_cmd = self.build_command(cmd);

        // Run with timeout
        let result = run_with_timeout(&mut process_cmd, Duration::from_secs(10));
        let latency_ms = start.elapsed().as_millis() as u64;

        match result {
            Ok((stdout, exit_code, stderr)) => {
                let stderr_trimmed = stderr.trim().to_string();
                let ok = exit_code == Some(0);
                let error_category = classify_error(exit_code, &stderr_trimmed);
                self.record_command(cmd, ok, latency_ms, stderr_trimmed.clone(), error_category);
                (stdout, ok, stderr_trimmed)
            }
            Err(err_msg) => {
                // Timeout or other error
                let error_category = classify_error(None, &err_msg);
                self.record_command(cmd, false, latency_ms, err_msg.clone(), error_category);
                (String::new(), false, err_msg)
            }
        }
    }

    /// Run a command and return stdout only.
    ///
    /// Matches Python `_run(cmd)` wrapper.
    pub fn run(&self, cmd: &str) -> String {
        let (stdout, _, _) = self.run_result(cmd);
        stdout
    }

    /// Fetch the last `limit` command history entries.
    ///
    /// Matches Python `fetch_command_health(limit)`.
    pub fn history(&self, limit: usize) -> Vec<CommandStat> {
        if limit == 0 {
            return vec![];
        }
        let hist = self.inner.history.lock().unwrap();
        let len = hist.len();
        if len <= limit {
            hist.iter().cloned().collect()
        } else {
            hist.iter().skip(len - limit).cloned().collect()
        }
    }

    fn build_command(&self, cmd: &str) -> Command {
        let remote = self.inner.remote.lock().unwrap();
        let ssh_host = remote.ssh_host.clone();
        let ssh_key = remote.ssh_key.clone();
        drop(remote);

        if let Some(host) = ssh_host {
            let mut ssh = Command::new("ssh");
            ssh.arg("-q")
                .arg("-o")
                .arg("BatchMode=yes")
                .arg("-o")
                .arg("ConnectTimeout=8");
            if let Some(key) = ssh_key {
                ssh.arg("-i").arg(key);
            }
            ssh.arg(host).arg(cmd);
            ssh.stdout(Stdio::piped()).stderr(Stdio::piped());
            ssh
        } else {
            // Local execution: use shell, matching remote path for consistent quoting
            let mut sh = Command::new("sh");
            sh.arg("-c")
                .arg(cmd)
                .stdout(Stdio::piped())
                .stderr(Stdio::piped());
            sh
        }
    }

    fn record_command(
        &self,
        command: &str,
        ok: bool,
        latency_ms: u64,
        stderr: String,
        error_category: Option<ErrorCategory>,
    ) {
        let mut hist = self.inner.history.lock().unwrap();
        if hist.len() >= HISTORY_CAP {
            hist.pop_front();
        }
        hist.push_back(CommandStat {
            command: command.to_string(),
            ok,
            latency_ms,
            stderr,
            error_category,
        });
    }
}

impl Default for Runner {
    fn default() -> Self {
        Self::new()
    }
}

/// Run a command with a timeout.
///
/// Returns `Ok((stdout, exit_code, stderr))` on completion, `Err(msg)` on timeout or spawn failure.
///
/// Drains stdout/stderr on background threads to prevent deadlock when output exceeds pipe buffer.
fn run_with_timeout(
    cmd: &mut Command,
    timeout: Duration,
) -> Result<(String, Option<i32>, String), String> {
    use std::io::Read;
    use std::thread;

    let mut child = cmd
        .spawn()
        .map_err(|e| format!("command not found: {}", e))?;

    // Take pipes and spawn reader threads to drain concurrently
    let mut out = child.stdout.take().ok_or("stdout not piped".to_string())?;
    let mut err = child.stderr.take().ok_or("stderr not piped".to_string())?;

    let out_h = thread::spawn(move || {
        let mut b = Vec::new();
        let _ = out.read_to_end(&mut b);
        b
    });
    let err_h = thread::spawn(move || {
        let mut b = Vec::new();
        let _ = err.read_to_end(&mut b);
        b
    });

    // Poll for completion or timeout
    let start = std::time::Instant::now();
    let status = loop {
        match child.try_wait() {
            Ok(Some(s)) => break s,
            Ok(None) => {
                if start.elapsed() > timeout {
                    let _ = child.kill();
                    let _ = child.wait();
                    return Err("timeout".to_string());
                }
                thread::sleep(Duration::from_millis(20));
            }
            Err(e) => return Err(format!("OS error: {}", e)),
        }
    };

    // Collect drained output
    let stdout = String::from_utf8_lossy(&out_h.join().unwrap_or_default()).to_string();
    let stderr = String::from_utf8_lossy(&err_h.join().unwrap_or_default()).to_string();
    Ok((stdout, status.code(), stderr))
}

// ── Job actions ──────────────────────────────────────────────────────────────

/// Cancel a job.
///
/// Matches Python `cancel_job_result(job_id)`.
pub fn cancel_job_result(runner: &Runner, job_id: &str) -> (bool, String) {
    let cmd = format!("scancel {}", shell_quote(job_id));
    let (_, ok, stderr) = runner.run_result(&cmd);
    (ok, stderr)
}

/// Hold a job.
///
/// Matches Python `hold_job_result(job_id)`.
pub fn hold_job_result(runner: &Runner, job_id: &str) -> (bool, String) {
    let cmd = format!("scontrol hold {}", shell_quote(job_id));
    let (_, ok, stderr) = runner.run_result(&cmd);
    (ok, stderr)
}

/// Release a job.
///
/// Matches Python `release_job_result(job_id)`.
pub fn release_job_result(runner: &Runner, job_id: &str) -> (bool, String) {
    let cmd = format!("scontrol release {}", shell_quote(job_id));
    let (_, ok, stderr) = runner.run_result(&cmd);
    (ok, stderr)
}

/// Requeue a job.
///
/// Matches Python `requeue_job_result(job_id)`.
pub fn requeue_job_result(runner: &Runner, job_id: &str) -> (bool, String) {
    let cmd = format!("scontrol requeue {}", shell_quote(job_id));
    let (_, ok, stderr) = runner.run_result(&cmd);
    (ok, stderr)
}

/// Execute a per-job action with normalized result.
///
/// Matches Python `run_job_action(action, job_id)`.
pub fn run_job_action(runner: &Runner, action: &str, job_id: &str) -> ActionResult {
    let action_lower = action.to_lowercase();
    let (ok, err) = match action_lower.as_str() {
        "cancel" => cancel_job_result(runner, job_id),
        "hold" => hold_job_result(runner, job_id),
        "release" => release_job_result(runner, job_id),
        "requeue" => requeue_job_result(runner, job_id),
        _ => {
            return ActionResult {
                job_id: job_id.to_string(),
                action: action.to_string(),
                ok: false,
                message: "unsupported action".to_string(),
            }
        }
    };
    let message = if err.is_empty() {
        if ok {
            "ok".to_string()
        } else {
            "failed".to_string()
        }
    } else {
        err
    };
    ActionResult {
        job_id: job_id.to_string(),
        action: action.to_string(),
        ok,
        message,
    }
}

/// Execute a bulk job action.
///
/// Matches Python `run_bulk_job_action(action, job_ids)`.
pub fn run_bulk_job_action(runner: &Runner, action: &str, job_ids: &[String]) -> Vec<ActionResult> {
    job_ids
        .iter()
        .map(|job_id| run_job_action(runner, action, job_id))
        .collect()
}

/// Minimal shell quoting (single quotes, escape embedded single quotes).
fn shell_quote(s: &str) -> String {
    if s.is_empty() {
        return "''".to_string();
    }
    // Simple heuristic: if s contains only safe chars, return as-is
    let safe = s
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-' || c == '.');
    if safe {
        return s.to_string();
    }
    // Otherwise single-quote and escape embedded single quotes
    format!("'{}'", s.replace('\'', r"'\''"))
}

// ── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    // ── classify_error: table-driven ─────────────────────────────────────────

    #[test]
    fn test_classify_error_success() {
        assert_eq!(classify_error(Some(0), ""), None);
        assert_eq!(classify_error(Some(0), "warning: foo"), None);
    }

    #[test]
    fn test_classify_error_exception_path() {
        assert_eq!(
            classify_error(None, "timeout"),
            Some(ErrorCategory::SlurmCommandTimeout)
        );
        assert_eq!(
            classify_error(None, "command not found"),
            Some(ErrorCategory::SlurmCommandNotFound)
        );
        assert_eq!(
            classify_error(None, "OS error: too many open files"),
            Some(ErrorCategory::SlurmCommandFailed)
        );
    }

    #[test]
    fn test_classify_error_permission_denied() {
        assert_eq!(
            classify_error(Some(1), "scancel: error: Permission denied"),
            Some(ErrorCategory::SlurmPermissionDenied)
        );
        assert_eq!(
            classify_error(Some(1), "user unauthorized"),
            Some(ErrorCategory::SlurmPermissionDenied)
        );
        assert_eq!(
            classify_error(Some(1), "operation not allowed"),
            Some(ErrorCategory::SlurmPermissionDenied)
        );
    }

    #[test]
    fn test_classify_error_ssh_connection() {
        assert_eq!(
            classify_error(Some(255), "ssh: connect to host x: Connection refused"),
            Some(ErrorCategory::SshConnectionFailed)
        );
        assert_eq!(
            classify_error(Some(255), "ssh: Could not resolve hostname x"),
            Some(ErrorCategory::SshConnectionFailed)
        );
        assert_eq!(
            classify_error(Some(255), "Connection closed by remote host"),
            Some(ErrorCategory::SshConnectionFailed)
        );
    }

    #[test]
    fn test_classify_error_ssh_auth_priority() {
        // Must be ssh_auth_failed even though stderr contains "Permission denied"
        assert_eq!(
            classify_error(Some(255), "Permission denied (publickey)."),
            Some(ErrorCategory::SshAuthFailed)
        );
        assert_eq!(
            classify_error(Some(1), "authentication failed"),
            Some(ErrorCategory::SshAuthFailed)
        );
    }

    #[test]
    fn test_classify_error_timeout() {
        assert_eq!(
            classify_error(Some(1), "timeout waiting for response"),
            Some(ErrorCategory::SlurmCommandTimeout)
        );
    }

    #[test]
    fn test_classify_error_job_not_found() {
        assert_eq!(
            classify_error(Some(1), "scontrol: error: Invalid job id 9999"),
            Some(ErrorCategory::JobNotFound)
        );
        assert_eq!(
            classify_error(Some(1), "job not found"),
            Some(ErrorCategory::JobNotFound)
        );
        assert_eq!(
            classify_error(Some(1), "unknown job specification"),
            Some(ErrorCategory::JobNotFound)
        );
    }

    #[test]
    fn test_classify_error_node_not_found() {
        assert_eq!(
            classify_error(Some(1), "scontrol: error: invalid node specification"),
            Some(ErrorCategory::NodeNotFound)
        );
        assert_eq!(
            classify_error(Some(1), "node not found"),
            Some(ErrorCategory::NodeNotFound)
        );
        assert_eq!(
            classify_error(Some(1), "unknown node specification"),
            Some(ErrorCategory::NodeNotFound)
        );
    }

    #[test]
    fn test_classify_error_exit_127() {
        // sh -c returns 127 when command not found
        assert_eq!(
            classify_error(Some(127), "command not found"),
            Some(ErrorCategory::SlurmCommandNotFound)
        );
    }

    #[test]
    fn test_classify_error_generic_failure() {
        assert_eq!(
            classify_error(Some(1), ""),
            Some(ErrorCategory::SlurmCommandFailed)
        );
        assert_eq!(
            classify_error(Some(2), "some opaque slurm message"),
            Some(ErrorCategory::SlurmCommandFailed)
        );
    }

    // ── Runner ───────────────────────────────────────────────────────────────

    #[test]
    fn test_runner_new() {
        let runner = Runner::new();
        assert_eq!(runner.history(10).len(), 0);
    }

    #[test]
    fn test_runner_set_remote() {
        let runner = Runner::new();
        runner.set_remote("host".to_string(), "key".to_string());
        {
            let remote = runner.inner.remote.lock().unwrap();
            assert_eq!(remote.ssh_host, Some("host".to_string()));
            assert_eq!(remote.ssh_key, Some("key".to_string()));
        }

        // Clear with empty strings
        runner.set_remote("".to_string(), "".to_string());
        {
            let remote = runner.inner.remote.lock().unwrap();
            assert_eq!(remote.ssh_host, None);
            assert_eq!(remote.ssh_key, None);
        }
    }

    #[test]
    fn test_runner_run_result_success() {
        let runner = Runner::new();
        let (stdout, ok, stderr) = runner.run_result("echo hello");
        assert!(ok);
        assert!(stdout.contains("hello"));
        assert!(stderr.is_empty());

        let history = runner.history(1);
        assert_eq!(history.len(), 1);
        assert!(history[0].ok);
        assert_eq!(history[0].error_category, None);
    }

    #[test]
    fn test_runner_run_result_failure() {
        let runner = Runner::new();
        let (_, ok, _) = runner.run_result("false");
        assert!(!ok);

        let history = runner.history(1);
        assert_eq!(history.len(), 1);
        assert!(!history[0].ok);
        assert_eq!(
            history[0].error_category,
            Some(ErrorCategory::SlurmCommandFailed)
        );
    }

    #[test]
    fn test_runner_run_wrapper() {
        let runner = Runner::new();
        let stdout = runner.run("echo test");
        assert!(stdout.contains("test"));
    }

    #[test]
    fn test_runner_history_limit() {
        let runner = Runner::new();
        // Record 5 commands
        for i in 0..5 {
            runner.run_result(&format!("echo {}", i));
        }
        assert_eq!(runner.history(0).len(), 0);
        assert_eq!(runner.history(2).len(), 2);
        assert_eq!(runner.history(10).len(), 5);
    }

    #[test]
    fn test_runner_history_bounded() {
        let runner = Runner::new();
        // Record 150 commands, only last 100 should remain
        for i in 0..150 {
            runner.run_result(&format!("echo {}", i));
        }
        let history = runner.history(200);
        assert_eq!(history.len(), 100);
        // The first command should be "echo 50" (we dropped 0..49)
        assert!(history[0].command.contains("echo 50"));
    }

    #[test]
    fn test_runner_command_not_found() {
        let runner = Runner::new();
        let (_, ok, stderr) = runner.run_result("nonexistent_command_xyz");
        assert!(!ok);
        let history = runner.history(1);
        assert_eq!(history.len(), 1);
        // Should be classified as command not found
        assert_eq!(
            history[0].error_category,
            Some(ErrorCategory::SlurmCommandNotFound)
        );
        assert!(stderr.contains("not found"));
    }

    // ── Action functions ─────────────────────────────────────────────────────

    #[test]
    fn test_run_job_action_unsupported() {
        let runner = Runner::new();
        let result = run_job_action(&runner, "noop", "100");
        assert!(!result.ok);
        assert!(result.message.contains("unsupported"));
    }

    #[test]
    fn test_run_bulk_job_action() {
        let runner = Runner::new();
        let job_ids = vec!["1".to_string(), "2".to_string(), "3".to_string()];
        let results = run_bulk_job_action(&runner, "noop", &job_ids);
        assert_eq!(results.len(), 3);
        assert!(results.iter().all(|r| !r.ok));
    }

    #[test]
    fn test_shell_quote_simple() {
        assert_eq!(shell_quote("abc123"), "abc123");
        assert_eq!(shell_quote("job_id"), "job_id");
    }

    #[test]
    fn test_shell_quote_with_spaces() {
        assert_eq!(shell_quote("hello world"), "'hello world'");
    }

    #[test]
    fn test_shell_quote_with_single_quote() {
        assert_eq!(shell_quote("it's"), "'it'\\''s'");
    }

    #[test]
    fn test_shell_quote_empty() {
        assert_eq!(shell_quote(""), "''");
    }

    #[test]
    fn test_runner_large_output_no_deadlock() {
        // Regression test: ensure we don't deadlock when output exceeds pipe buffer (~64 KB).
        // Command emits 500 KB, well over the buffer limit.
        let runner = Runner::new();
        let (stdout, ok, stderr) = runner.run_result("head -c 500000 /dev/zero | tr '\\0' 'x'");
        assert!(
            ok,
            "large output command should succeed, got stderr: {}",
            stderr
        );
        assert_eq!(stdout.len(), 500000, "should receive full 500 KB output");
    }
}

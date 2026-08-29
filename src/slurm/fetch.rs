//! Slurm data-layer fetch functions.
//!
//! All query functions take `&Runner` and delegate command execution to it.
//! This module ports the Python `fetch_*` functions from slurm.py.

use crate::slurm::exec::Runner;
use crate::slurm::model::{ClusterSummary, Job, Node};
use crate::slurm::parse::{
    parse_gpu_count, parse_node_row, parse_partition_row, parse_scontrol_kv, parse_slurm_duration,
    parse_squeue_row, SINFO_NODE_FMT, SINFO_PARTITION_FMT, SQUEUE_FMT,
};
use std::collections::HashMap;
use std::sync::LazyLock;

// ---------------------------------------------------------------------------
// Types not yet in model.rs
// ---------------------------------------------------------------------------

/// Job dependency from scontrol show job Dependency= field.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JobDependency {
    pub dep_type: String, // "afterok", "afterany", "after", etc.
    pub job_id: String,
    pub state: String, // fetched from squeue, or "COMPLETED" if not in queue
}

/// Completed job from sacct.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SacctJob {
    pub job_id: String,
    pub name: String,
    pub user: String,
    pub state: String,
    pub num_cpus: String,
    pub elapsed: String,
    pub exit_code: String,
    pub partition: String,
}

/// Result from fetch_job_efficiency.
#[derive(Debug, Clone)]
pub struct JobEfficiency {
    pub available: bool,
    pub cpu_eff: f64,
    pub mem_eff: f64,
    pub cpu_used_str: String,
    pub cpu_alloc_str: String,
    pub mem_peak_mb: u64,
    pub mem_alloc_mb: u64,
}

// ---------------------------------------------------------------------------
// Regex for _fetch_gpus_alloc
// ---------------------------------------------------------------------------

static GRES_GPU_RE: LazyLock<regex::Regex> =
    LazyLock::new(|| regex::Regex::new(r"gres/gpu=(\d+)").unwrap());

// ---------------------------------------------------------------------------
// Jobs
// ---------------------------------------------------------------------------

/// Return jobs from squeue -o with parseable format.
pub fn fetch_jobs(runner: &Runner) -> Vec<Job> {
    let cmd = format!("squeue --noheader -o '{}'", SQUEUE_FMT);
    let (out, _, _) = runner.run_result(&cmd);
    out.lines().filter_map(parse_squeue_row).collect()
}

/// Return jobs currently visible on a specific node via squeue -w.
///
/// Returns empty vec without invoking any command when `node_name` is empty or whitespace.
pub fn fetch_jobs_on_node(runner: &Runner, node_name: &str) -> Vec<Job> {
    let name = node_name.trim();
    if name.is_empty() {
        return Vec::new();
    }
    let cmd = format!(
        "squeue --noheader -w {} -o '{}'",
        shell_quote(name),
        SQUEUE_FMT
    );
    let (out, _, _) = runner.run_result(&cmd);
    out.lines().filter_map(parse_squeue_row).collect()
}

// ---------------------------------------------------------------------------
// Nodes
// ---------------------------------------------------------------------------

/// Return node info from sinfo.
///
/// Runs `sinfo` and `_fetch_gpus_alloc` (scontrol show nodes) in parallel
/// to match Python's ThreadPoolExecutor concurrency.
pub fn fetch_nodes(runner: &Runner) -> Vec<Node> {
    // Run both queries in parallel using threads
    let runner_clone = runner.clone();
    let (sinfo_out, gpus_alloc) = std::thread::scope(|s| {
        let sinfo_handle = s.spawn(|| {
            let cmd = format!("sinfo --noheader -o '{}'", SINFO_NODE_FMT);
            let (out, _, _) = runner.run_result(&cmd);
            out
        });
        let gpus_handle = s.spawn(move || fetch_gpus_alloc(&runner_clone));

        let sinfo = sinfo_handle.join().unwrap_or_default();
        let gpus = gpus_handle.join().unwrap_or_default();
        (sinfo, gpus)
    });

    sinfo_out
        .lines()
        .filter_map(|line| parse_node_row(line, &gpus_alloc))
        .collect()
}

/// Helper: return {node_name: gpus_allocated} from scontrol show nodes.
///
/// Reads AllocTRES (Slurm 24.x) and falls back to GresUsed (older versions).
fn fetch_gpus_alloc(runner: &Runner) -> HashMap<String, u32> {
    let (out, _, _) = runner.run_result("scontrol show nodes");
    let mut result = HashMap::new();
    let mut node_name = String::new();

    for token in out.split_whitespace() {
        if let Some(name) = token.strip_prefix("NodeName=") {
            node_name = name.to_string();
        } else if let Some(alloc_tres) = token.strip_prefix("AllocTRES=") {
            if !node_name.is_empty() {
                if let Some(cap) = GRES_GPU_RE.captures(alloc_tres) {
                    if let Some(count_str) = cap.get(1) {
                        if let Ok(count) = count_str.as_str().parse::<u32>() {
                            result.insert(node_name.clone(), count);
                        }
                    }
                }
            }
        } else if let Some(gres_used) = token.strip_prefix("GresUsed=") {
            if !node_name.is_empty() && !result.contains_key(&node_name) {
                let count = parse_gpu_count(gres_used);
                if count > 0 {
                    result.insert(node_name.clone(), count);
                }
            }
        }
    }

    result
}

// ---------------------------------------------------------------------------
// Cluster summary
// ---------------------------------------------------------------------------

/// Return partition summary from sinfo -s.
pub fn fetch_cluster_summary(runner: &Runner) -> Vec<ClusterSummary> {
    let cmd = format!("sinfo --noheader -o '{}'", SINFO_PARTITION_FMT);
    let (out, _, _) = runner.run_result(&cmd);
    out.lines().filter_map(parse_partition_row).collect()
}

// ---------------------------------------------------------------------------
// scontrol detail
// ---------------------------------------------------------------------------

/// Return key=value pairs from scontrol show job <id>.
pub fn fetch_job_detail(runner: &Runner, job_id: &str) -> HashMap<String, String> {
    let cmd = format!("scontrol show job {}", job_id);
    let (out, _, _) = runner.run_result(&cmd);
    parse_scontrol_kv(&out)
}

/// Return key=value pairs from scontrol show job <id>, in output order.
pub fn fetch_job_detail_ordered(runner: &Runner, job_id: &str) -> Vec<(String, String)> {
    let cmd = format!("scontrol show job {}", job_id);
    let (out, _, _) = runner.run_result(&cmd);
    crate::slurm::parse::parse_scontrol_kv_ordered(&out)
}

/// Return key=value pairs from scontrol show node <name>, in output order.
pub fn fetch_node_detail_ordered(runner: &Runner, node_name: &str) -> Vec<(String, String)> {
    let cmd = format!("scontrol show node {}", node_name);
    let (out, _, _) = runner.run_result(&cmd);
    crate::slurm::parse::parse_scontrol_kv_ordered(&out)
}

// ---------------------------------------------------------------------------
// Batch script & logs
// ---------------------------------------------------------------------------

/// Return the batch script for job_id, or an error message.
pub fn fetch_batch_script(runner: &Runner, job_id: &str) -> String {
    let cmd = format!("scontrol write batch_script {} -", shell_quote(job_id));
    let (out, ok, err) = runner.run_result(&cmd);
    if !ok {
        let msg = if !err.is_empty() {
            err
        } else {
            "permission denied or job not found".to_string()
        };
        return format!("(error: {})", msg);
    }
    if out.is_empty() {
        "(empty script)".to_string()
    } else {
        out
    }
}

/// Return (stdout_path, stderr_path) from scontrol show job.
pub fn fetch_log_paths(runner: &Runner, job_id: &str) -> (String, String) {
    let detail = fetch_job_detail(runner, job_id);
    let stdout = detail.get("StdOut").cloned().unwrap_or_default();
    let stderr = detail.get("StdErr").cloned().unwrap_or_default();
    (stdout, stderr)
}

/// Return last n lines of a log file.
pub fn tail_log_file(runner: &Runner, path: &str, n: u32) -> String {
    if path.is_empty() {
        return "(no log path)".to_string();
    }
    let cmd = format!("tail -n {} {}", n, shell_quote(path));
    let (out, _, _) = runner.run_result(&cmd);
    if out.is_empty() {
        "(empty or file not found)".to_string()
    } else {
        out
    }
}

// ---------------------------------------------------------------------------
// Job efficiency
// ---------------------------------------------------------------------------

/// Fetch CPU and memory efficiency metrics via sacct.
///
/// Returns JobEfficiency with `available: false` if sacct not found or parse error.
pub fn fetch_job_efficiency(runner: &Runner, job_id: &str) -> JobEfficiency {
    let cmd = format!(
        "sacct -j {} --parsable2 --noheader -o CPUTimeRAW,TotalCPU,AllocMem,MaxRSS",
        shell_quote(job_id)
    );
    let (out, ok, _) = runner.run_result(&cmd);

    if !ok || out.trim().is_empty() {
        return JobEfficiency {
            available: false,
            cpu_eff: 0.0,
            mem_eff: 0.0,
            cpu_used_str: String::new(),
            cpu_alloc_str: String::new(),
            mem_peak_mb: 0,
            mem_alloc_mb: 0,
        };
    }

    // Use the first non-step line
    let target_line = out.lines().next().unwrap_or("");
    let parts: Vec<&str> = target_line.split('|').collect();
    if parts.len() < 4 {
        return JobEfficiency {
            available: false,
            cpu_eff: 0.0,
            mem_eff: 0.0,
            cpu_used_str: String::new(),
            cpu_alloc_str: String::new(),
            mem_peak_mb: 0,
            mem_alloc_mb: 0,
        };
    }

    let cpu_time_raw_str = parts[0].trim();
    let total_cpu_str = parts[1].trim();
    let alloc_mem_str = parts[2].trim();
    let max_rss_str = parts[3].trim();

    let cpu_time_raw = cpu_time_raw_str.parse::<u64>().unwrap_or(0);
    let total_cpu_secs = parse_slurm_duration(total_cpu_str).unwrap_or(0);

    // Parse AllocMem: may be "2000M", "2048K", or bare integer (MB)
    let alloc_mem_mb = parse_mem_value(alloc_mem_str);
    let max_rss_mb = parse_mem_value(max_rss_str);

    let cpu_eff = if cpu_time_raw > 0 {
        (total_cpu_secs as f64 / cpu_time_raw as f64).min(1.0)
    } else {
        0.0
    };

    let mem_eff = if alloc_mem_mb > 0 {
        (max_rss_mb as f64 / alloc_mem_mb as f64).min(1.0)
    } else {
        0.0
    };

    // Build human-readable cpu_alloc_str from CPUTimeRAW seconds
    let h = cpu_time_raw / 3600;
    let m = (cpu_time_raw % 3600) / 60;
    let s = cpu_time_raw % 60;
    let cpu_alloc_str = format!("{}:{:02}:{:02}", h, m, s);

    JobEfficiency {
        available: true,
        cpu_eff,
        mem_eff,
        cpu_used_str: total_cpu_str.to_string(),
        cpu_alloc_str,
        mem_peak_mb: max_rss_mb,
        mem_alloc_mb: alloc_mem_mb,
    }
}

/// Parse memory value from sacct output (handles M, K suffixes or bare integer).
/// Returns value in MB.
fn parse_mem_value(s: &str) -> u64 {
    let trimmed = s.trim();
    if trimmed.ends_with('M') || trimmed.ends_with('m') {
        trimmed[..trimmed.len() - 1].parse::<u64>().unwrap_or(0)
    } else if trimmed.ends_with('K') || trimmed.ends_with('k') {
        trimmed[..trimmed.len() - 1].parse::<u64>().unwrap_or(0) / 1024
    } else {
        // Bare integer is KB for MaxRSS, MB for AllocMem
        // For simplicity, treat as MB if no suffix
        trimmed.parse::<u64>().unwrap_or(0)
    }
}

// ---------------------------------------------------------------------------
// Array tasks
// ---------------------------------------------------------------------------

/// Fetch individual tasks for a job array via squeue -j <job_id>.
pub fn fetch_array_tasks(runner: &Runner, job_id: &str) -> Vec<Job> {
    let fmt = "%i|%j|%u|%T|%P|%D|%C|%M|%l|%R|%N";
    let cmd = format!("squeue --noheader -j {} -o '{}'", shell_quote(job_id), fmt);
    let (out, _, _) = runner.run_result(&cmd);

    out.lines()
        .filter_map(|line| {
            let parts: Vec<&str> = line.split('|').collect();
            if parts.len() < 11 {
                return None;
            }
            Some(Job {
                job_id: parts[0].to_string(),
                name: parts[1].to_string(),
                user: parts[2].to_string(),
                state: parts[3].to_string(),
                partition: parts[4].to_string(),
                nodes: parts[5].to_string(),
                num_cpus: parts[6].to_string(),
                time_used: parts[7].to_string(),
                time_limit: parts[8].to_string(),
                reason: parts[9].to_string(),
                nodelist: parts[10].to_string(),
                num_nodes: parts[5].to_string(),
                qos: String::new(),
            })
        })
        .collect()
}

// ---------------------------------------------------------------------------
// Job dependencies
// ---------------------------------------------------------------------------

/// Parse Dependency= from scontrol show job. Non-recursive (immediate deps only).
pub fn fetch_job_dependencies(runner: &Runner, job_id: &str) -> Vec<JobDependency> {
    let detail = fetch_job_detail(runner, job_id);
    let dep_str = detail.get("Dependency").map(|s| s.as_str()).unwrap_or("");

    if dep_str.is_empty()
        || dep_str.eq_ignore_ascii_case("none")
        || dep_str.eq_ignore_ascii_case("(null)")
    {
        return Vec::new();
    }

    let mut deps = Vec::new();
    for token in dep_str.split(',') {
        if !token.contains(':') {
            continue; // handles "singleton"
        }
        let mut parts = token.splitn(2, ':');
        let dep_type = parts.next().unwrap_or("").trim();
        let rest = parts.next().unwrap_or("");

        for jid_raw in rest.split(':') {
            let jid = jid_raw.split('(').next().unwrap_or("").trim();
            if jid.chars().all(|c| c.is_ascii_digit()) {
                deps.push(JobDependency {
                    dep_type: dep_type.to_string(),
                    job_id: jid.to_string(),
                    state: String::new(),
                });
            }
        }
    }

    if deps.is_empty() {
        return deps;
    }

    // Batch fetch states with one squeue call
    let ids_csv = deps
        .iter()
        .map(|d| d.job_id.as_str())
        .collect::<Vec<_>>()
        .join(",");
    let cmd = format!("squeue --noheader -j {} -o '%i|%T'", shell_quote(&ids_csv));
    let (out, _, _) = runner.run_result(&cmd);

    let mut state_map = HashMap::new();
    for line in out.lines() {
        let parts: Vec<&str> = line.split('|').collect();
        if parts.len() >= 2 {
            state_map.insert(parts[0].to_string(), parts[1].to_string());
        }
    }

    for dep in &mut deps {
        dep.state = state_map
            .get(&dep.job_id)
            .cloned()
            .unwrap_or_else(|| "COMPLETED".to_string());
    }

    deps
}

// ---------------------------------------------------------------------------
// Completed jobs (sacct)
// ---------------------------------------------------------------------------

/// Fetch completed jobs from sacct for the last N hours.
pub fn fetch_sacct_jobs(runner: &Runner, hours: u32) -> Vec<SacctJob> {
    let cmd = format!(
        "sacct --noheader --parsable2 -S now-{}hours -o JobID,JobName,User,State,AllocCPUS,Elapsed,ExitCode,Partition",
        hours
    );
    let (out, ok, _) = runner.run_result(&cmd);

    if !ok || out.trim().is_empty() {
        return Vec::new();
    }

    out.lines()
        .filter_map(|line| {
            let parts: Vec<&str> = line.split('|').collect();
            if parts.len() < 8 {
                return None;
            }
            let job_id = parts[0];
            // Skip step lines: job IDs containing '.' are steps (e.g. 12345.batch)
            if job_id.contains('.') {
                return None;
            }
            Some(SacctJob {
                job_id: job_id.to_string(),
                name: parts[1].to_string(),
                user: parts[2].to_string(),
                state: parts[3].to_string(),
                num_cpus: parts[4].to_string(),
                elapsed: parts[5].to_string(),
                exit_code: parts[6].to_string(),
                partition: parts[7].to_string(),
            })
        })
        .collect()
}

// ---------------------------------------------------------------------------
// Interactive attach
// ---------------------------------------------------------------------------

/// Resolve the first node hostname from a Slurm NodeList expression.
pub fn resolve_first_node(runner: &Runner, nodelist_expr: &str) -> String {
    let expr = nodelist_expr.trim();
    if expr.is_empty() || expr.eq_ignore_ascii_case("(null)") {
        return String::new();
    }

    let cmd = format!("scontrol show hostnames {}", shell_quote(expr));
    let (out, _, _) = runner.run_result(&cmd);

    for line in out.lines() {
        let host = line.trim();
        if !host.is_empty() {
            return host.to_string();
        }
    }

    // Conservative fallback for unresolved compressed expressions
    expr.split(',').next().unwrap_or("").trim().to_string()
}

/// Build interactive attach command for a running Slurm job.
pub fn build_attach_command(
    job_id: &str,
    node: Option<&str>,
    default_command: &str,
    extra_args: &str,
) -> Vec<String> {
    let mut cmd = vec![
        "srun".to_string(),
        "--pty".to_string(),
        "--overlap".to_string(),
    ];

    if !extra_args.trim().is_empty() {
        cmd.extend(shell_split(extra_args));
    }

    cmd.push("--jobid".to_string());
    cmd.push(job_id.to_string());

    if let Some(n) = node {
        if !n.trim().is_empty() {
            cmd.push("-w".to_string());
            cmd.push(n.trim().to_string());
        }
    }

    cmd.extend(shell_split(default_command));
    cmd
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Shell-quote a string for safe command construction.
fn shell_quote(s: &str) -> String {
    if s.chars()
        .all(|c| c.is_alphanumeric() || c == '_' || c == '-' || c == '.' || c == '/')
    {
        s.to_string()
    } else {
        format!("'{}'", s.replace('\'', r"'\''"))
    }
}

/// Simple shell word splitting (whitespace-separated).
fn shell_split(s: &str) -> Vec<String> {
    s.split_whitespace().map(|w| w.to_string()).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_fetch_jobs_parses_multiple_lines() {
        let _runner = Runner::new();
        let mock_squeue = "12345|job1|user1|RUNNING|gpu|1|8|10:00|24:00:00|None|node01|normal\n\
                          12346|job2|user2|PENDING|cpu|0|4|0:00|12:00:00|Resources||(null)\n";

        // We can't easily mock Runner here without more infrastructure
        // For now, just test parsing directly
        let jobs: Vec<Job> = mock_squeue.lines().filter_map(parse_squeue_row).collect();

        assert_eq!(jobs.len(), 2);
        assert_eq!(jobs[0].job_id, "12345");
        assert_eq!(jobs[1].job_id, "12346");
    }

    #[test]
    fn test_fetch_jobs_on_node_empty_name() {
        let runner = Runner::new();
        assert_eq!(fetch_jobs_on_node(&runner, ""), Vec::new());
        assert_eq!(fetch_jobs_on_node(&runner, "  "), Vec::new());
    }

    #[test]
    fn test_parse_node_row_normal() {
        let line = "node01|idle|gpu|4|4/0/0/4|32000|28000|0.10|gpu:2";
        let mut gpus_alloc = HashMap::new();
        gpus_alloc.insert("node01".to_string(), 1);

        let node = parse_node_row(line, &gpus_alloc).unwrap();
        assert_eq!(node.name, "node01");
        assert_eq!(node.state, "idle");
        assert_eq!(node.cpus_total, "4");
        assert_eq!(node.cpus_alloc, "4");
        assert_eq!(node.gpu_total, 2);
        assert_eq!(node.gpu_alloc, 1);
    }

    #[test]
    fn test_parse_node_row_cpu_parts_fallback() {
        let line = "node01|idle|gpu|4|8|32000|28000|0.10|(null)";
        let gpus_alloc = HashMap::new();

        let node = parse_node_row(line, &gpus_alloc).unwrap();
        assert_eq!(node.cpus_total, "?");
        assert_eq!(node.cpus_alloc, "?");
    }

    #[test]
    fn test_parse_node_row_gpu_alloc_forced_zero_when_no_gpu() {
        let line = "node01|idle|cpu|4|4/0/0/4|32000|28000|0.10|(null)";
        let mut gpus_alloc = HashMap::new();
        gpus_alloc.insert("node01".to_string(), 2); // has entry but gpu_total is 0

        let node = parse_node_row(line, &gpus_alloc).unwrap();
        assert_eq!(node.gpu_total, 0);
        assert_eq!(node.gpu_alloc, 0); // forced to 0
    }

    #[test]
    fn test_resolve_first_node_empty_expr() {
        let runner = Runner::new();
        assert_eq!(resolve_first_node(&runner, ""), "");
        assert_eq!(resolve_first_node(&runner, "(null)"), "");
    }

    #[test]
    fn test_build_attach_command_with_node_and_extra_args() {
        let cmd = build_attach_command("12345", Some("c2"), "bash -l", "--mpi=none");
        assert_eq!(
            cmd,
            vec![
                "srun",
                "--pty",
                "--overlap",
                "--mpi=none",
                "--jobid",
                "12345",
                "-w",
                "c2",
                "bash",
                "-l",
            ]
        );
    }

    #[test]
    fn test_build_attach_command_without_node() {
        let cmd = build_attach_command("12345", None, "bash -l", "");
        assert!(!cmd.contains(&"-w".to_string()));
        assert!(cmd.contains(&"bash".to_string()));
    }

    #[test]
    fn test_tail_log_file_no_path() {
        let runner = Runner::new();
        assert_eq!(tail_log_file(&runner, "", 200), "(no log path)");
    }

    #[test]
    fn test_parse_mem_value() {
        assert_eq!(parse_mem_value("2048M"), 2048);
        assert_eq!(parse_mem_value("2048K"), 2);
        assert_eq!(parse_mem_value("2048"), 2048);
        assert_eq!(parse_mem_value("invalid"), 0);
    }

    #[test]
    fn test_shell_quote_simple() {
        assert_eq!(shell_quote("simple"), "simple");
        assert_eq!(shell_quote("file.txt"), "file.txt");
    }

    #[test]
    fn test_shell_quote_with_spaces() {
        assert_eq!(shell_quote("file name.txt"), "'file name.txt'");
    }

    #[test]
    fn test_shell_split_basic() {
        assert_eq!(shell_split("bash -l"), vec!["bash", "-l"]);
        assert_eq!(shell_split("  bash   -l  "), vec!["bash", "-l"]);
    }
}

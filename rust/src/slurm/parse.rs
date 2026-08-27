//! Pure parsers for Slurm CLI output. No I/O lives here.

use crate::slurm::model::{ClusterSummary, Job};
use regex::Regex;
use std::collections::HashMap;
use std::sync::LazyLock;

/// Regex for parsing GPU count from GRES strings.
static GPU_COUNT_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\bgpu:(?:[^:,()\s]+:)?(\d+)").unwrap());

/// Regex for parsing allocated GPUs from AllocTRES.
static ALLOC_TRES_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"gres/gpu=(\d+)").unwrap());

/// Shared `squeue` format string. The field count is fixed at 12; any change
/// here must be matched in `parse_squeue_row`.
#[allow(dead_code)]
pub const SQUEUE_FMT: &str = "%i|%j|%u|%T|%P|%D|%C|%M|%l|%R|%N|%q";

/// Shared `sinfo` format string for the Partitions view.
#[allow(dead_code)]
pub const SINFO_PARTITION_FMT: &str = "%P|%a|%l|%D|%T|%N";

/// Slurm null sentinels we treat as "not provided".
pub const NULL_SENTINELS: &[&str] = &["", "(null)", "N/A", "None", "none"];

/// Pending job states (full and abbreviated).
#[allow(dead_code)]
pub const PENDING_STATES: &[&str] = &["PENDING", "PD"];

/// Running job states (full and abbreviated).
#[allow(dead_code)]
pub const RUNNING_STATES: &[&str] = &["RUNNING", "R"];

/// Terminal job states (full and abbreviated).
#[allow(dead_code)]
pub const TERMINAL_STATES: &[&str] = &[
    "COMPLETED",
    "CD",
    "FAILED",
    "F",
    "CANCELLED",
    "CA",
    "TIMEOUT",
    "TO",
    "NODE_FAIL",
    "NF",
    "PREEMPTED",
    "PR",
    "OUT_OF_MEMORY",
    "OOM",
];

/// Parse one squeue row produced with `SQUEUE_FMT` into a `Job`.
///
/// Returns `None` for malformed rows (fewer than 12 pipe-separated fields)
/// so callers can keep going on partial output.
#[allow(dead_code)]
pub fn parse_squeue_row(line: &str) -> Option<Job> {
    let parts: Vec<&str> = line.split('|').collect();
    if parts.len() < 12 {
        return None;
    }

    let qos_raw = parts[11];
    let qos = if qos_raw == "N/A" || qos_raw == "(null)" {
        String::new()
    } else {
        qos_raw.to_string()
    };

    Some(Job {
        job_id: parts[0].to_string(),
        name: parts[1].to_string(),
        user: parts[2].to_string(),
        state: parts[3].to_string(),
        partition: parts[4].to_string(),
        nodes: parts[5].to_string(),
        num_nodes: parts[5].to_string(),
        num_cpus: parts[6].to_string(),
        time_used: parts[7].to_string(),
        time_limit: parts[8].to_string(),
        reason: parts[9].to_string(),
        nodelist: parts[10].to_string(),
        qos,
    })
}

/// Parse one sinfo row for partition data into a `ClusterSummary`.
///
/// Returns `None` for malformed rows (fewer than 6 pipe-separated fields).
#[allow(dead_code)]
pub fn parse_partition_row(line: &str) -> Option<ClusterSummary> {
    let parts: Vec<&str> = line.split('|').collect();
    if parts.len() < 6 {
        return None;
    }

    Some(ClusterSummary {
        partition: parts[0].to_string(),
        avail: parts[1].to_string(),
        timelimit: parts[2].to_string(),
        nodes: parts[3].to_string(),
        state: parts[4].to_string(),
        nodelist: parts[5].to_string(),
    })
}

/// Extract total GPU count from a GRES string.
///
/// Sums across multiple gpu: groups, e.g. `gpu:a100:4,gpu:b100:2` → 6.
/// Single-group strings (`gpu:4`, `gpu:a100:4`, `gpu:a100:4(IDX:...)`) are
/// unchanged. Empty / non-GPU / `(null)` inputs return 0.
#[allow(dead_code)]
pub fn parse_gpu_count(gres_str: &str) -> u32 {
    GPU_COUNT_RE
        .captures_iter(gres_str)
        .filter_map(|cap| cap.get(1)?.as_str().parse::<u32>().ok())
        .sum()
}

/// Parse allocated GPUs from `scontrol show nodes` output.
///
/// Returns a map of node_name → gpus_allocated.
/// Reads AllocTRES (present in Slurm 24.x) and falls back to GresUsed
/// (older Slurm versions).
#[allow(dead_code)]
pub fn parse_gpus_alloc(scontrol_output: &str) -> HashMap<String, u32> {
    let mut result = HashMap::new();
    let mut node_name = String::new();

    for token in scontrol_output.split_whitespace() {
        if let Some(name) = token.strip_prefix("NodeName=") {
            node_name = name.to_string();
        } else if token.starts_with("AllocTRES=") && !node_name.is_empty() {
            if let Some(value_part) = token.strip_prefix("AllocTRES=") {
                if let Some(cap) = ALLOC_TRES_RE.captures(value_part) {
                    if let Some(count_str) = cap.get(1) {
                        if let Ok(count) = count_str.as_str().parse::<u32>() {
                            result.insert(node_name.clone(), count);
                        }
                    }
                }
            }
        } else if token.starts_with("GresUsed=") && !node_name.is_empty() {
            // Fallback to GresUsed only if we haven't already recorded this node
            if !result.contains_key(&node_name) {
                if let Some(value_part) = token.strip_prefix("GresUsed=") {
                    let count = parse_gpu_count(value_part);
                    if count > 0 {
                        result.insert(node_name.clone(), count);
                    }
                }
            }
        }
    }

    result
}

/// Parse Slurm HH:MM:SS (or D-HH:MM:SS) duration string to total seconds.
///
/// Returns `Some(0)` for empty or "0" input, `None` for malformed input.
/// Callers should treat `None` as 0 to match Python behavior.
#[allow(dead_code)]
pub fn parse_slurm_duration(s: &str) -> Option<u64> {
    let s = s.trim();
    if s.is_empty() || s == "0" {
        return Some(0);
    }

    let (days, time_part) = if let Some((day_str, rest)) = s.split_once('-') {
        let d = day_str.parse::<u64>().ok()?;
        (d, rest)
    } else {
        (0, s)
    };

    let parts: Vec<&str> = time_part.split(':').collect();
    let seconds = match parts.len() {
        3 => {
            let h = parts[0].parse::<u64>().ok()?;
            let m = parts[1].parse::<u64>().ok()?;
            let s = parts[2].parse::<u64>().ok()?;
            days * 86400 + h * 3600 + m * 60 + s
        }
        2 => {
            let m = parts[0].parse::<u64>().ok()?;
            let s = parts[1].parse::<u64>().ok()?;
            days * 86400 + m * 60 + s
        }
        1 => {
            let s = parts[0].parse::<u64>().ok()?;
            days * 86400 + s
        }
        _ => return None,
    };

    Some(seconds)
}

/// Parse key=value pairs from scontrol output.
///
/// Tokens without '=' are silently skipped.
#[allow(dead_code)]
pub fn parse_scontrol_kv(output: &str) -> HashMap<String, String> {
    let mut result = HashMap::new();

    for token in output.split_whitespace() {
        if let Some((key, value)) = token.split_once('=') {
            result.insert(key.to_string(), value.to_string());
        }
    }

    result
}

/// Check if a value carries real Slurm content (not a null sentinel).
#[allow(dead_code)]
pub fn is_present(value: Option<&str>) -> bool {
    match value {
        None => false,
        Some(v) => !NULL_SENTINELS.contains(&v.trim()),
    }
}

/// Format a value for display, mapping nulls to "(unavailable)".
#[allow(dead_code)]
pub fn display(value: Option<&str>) -> String {
    match value {
        Some(v) if !NULL_SENTINELS.contains(&v.trim()) => v.trim().to_string(),
        _ => "(unavailable)".to_string(),
    }
}

/// Strip Slurm decoration suffixes and uppercase the bare token.
///
/// Decoration suffixes: `*-+~#@!%$`
#[allow(dead_code)]
pub fn normalize_node_state_token(state: &str) -> String {
    const SUFFIXES: &str = "*-+~#@!%$";

    state
        .trim()
        .trim_end_matches(|c| SUFFIXES.contains(c))
        .to_uppercase()
}

/// Parse a numeric Slurm field; return `None` on any failure.
#[allow(dead_code)]
pub fn safe_int(value: Option<&str>) -> Option<i32> {
    match value {
        None => None,
        Some(v) => {
            let trimmed = v.trim();
            if trimmed.is_empty() || trimmed == "?" {
                None
            } else {
                trimmed.parse().ok()
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── parse_squeue_row ──────────────────────────────────────────────────

    #[test]
    fn test_parse_squeue_row_normal() {
        let line = "123|myjob|alice|RUNNING|gpu|1|8|1:00:00|8:00:00|None|node01|normal";
        let job = parse_squeue_row(line).unwrap();

        assert_eq!(job.job_id, "123");
        assert_eq!(job.name, "myjob");
        assert_eq!(job.user, "alice");
        assert_eq!(job.state, "RUNNING");
        assert_eq!(job.partition, "gpu");
        assert_eq!(job.num_cpus, "8");
        assert_eq!(job.time_used, "1:00:00");
        assert_eq!(job.time_limit, "8:00:00");
        assert_eq!(job.reason, "None");
        assert_eq!(job.nodelist, "node01");
        assert_eq!(job.qos, "normal");
    }

    #[test]
    fn test_parse_squeue_row_qos_normalization() {
        let line1 = "1|a|alice|RUNNING|gpu|1|4|0:01|8:00:00|None|node01|N/A";
        let job1 = parse_squeue_row(line1).unwrap();
        assert_eq!(job1.qos, "");

        let line2 = "2|b|bob|RUNNING|gpu|1|4|0:01|8:00:00|None|node02|(null)";
        let job2 = parse_squeue_row(line2).unwrap();
        assert_eq!(job2.qos, "");
    }

    #[test]
    fn test_parse_squeue_row_qos_none_verbatim() {
        let line = "3|c|carol|RUNNING|gpu|1|4|0:01|8:00:00|None|node03|None";
        let job = parse_squeue_row(line).unwrap();
        assert_eq!(job.qos, "None");
    }

    #[test]
    fn test_parse_squeue_row_malformed() {
        let line = "123|myjob|alice";
        assert!(parse_squeue_row(line).is_none());
    }

    // ── parse_partition_row ───────────────────────────────────────────────

    #[test]
    fn test_parse_partition_row_normal() {
        let line = "gpu|up|8:00:00|4|idle|node[01-04]";
        let summary = parse_partition_row(line).unwrap();

        assert_eq!(summary.partition, "gpu");
        assert_eq!(summary.avail, "up");
        assert_eq!(summary.timelimit, "8:00:00");
        assert_eq!(summary.nodes, "4");
        assert_eq!(summary.state, "idle");
        assert_eq!(summary.nodelist, "node[01-04]");
    }

    #[test]
    fn test_parse_partition_row_malformed() {
        let line = "gpu|up|8:00:00";
        assert!(parse_partition_row(line).is_none());
    }

    // ── parse_gpu_count ───────────────────────────────────────────────────

    #[test]
    fn test_parse_gpu_count_single_type() {
        assert_eq!(parse_gpu_count("gpu:4"), 4);
        assert_eq!(parse_gpu_count("gpu:a100:4"), 4);
        assert_eq!(parse_gpu_count("gpu:a100:4(IDX:0,1,2,3)"), 4);
    }

    #[test]
    fn test_parse_gpu_count_no_gpu() {
        assert_eq!(parse_gpu_count(""), 0);
        assert_eq!(parse_gpu_count("cpu:8"), 0);
        assert_eq!(parse_gpu_count("(null)"), 0);
    }

    #[test]
    fn test_parse_gpu_count_multi_type_comma() {
        assert_eq!(parse_gpu_count("gpu:a100:4,gpu:b100:2"), 6);
    }

    #[test]
    fn test_parse_gpu_count_multi_type_space() {
        assert_eq!(parse_gpu_count("gpu:a100:4 gpu:b100:2"), 6);
    }

    #[test]
    fn test_parse_gpu_count_multi_type_three() {
        assert_eq!(parse_gpu_count("gpu:a100:4,gpu:b100:2,gpu:l4:1"), 7);
    }

    #[test]
    fn test_parse_gpu_count_multi_type_with_idx() {
        assert_eq!(
            parse_gpu_count("gpu:a100:4(IDX:0,1,2,3),gpu:b100:2(IDX:0,1)"),
            6
        );
    }

    #[test]
    fn test_parse_gpu_count_multi_type_no_typenames() {
        assert_eq!(parse_gpu_count("gpu:4,gpu:2"), 6);
    }

    #[test]
    fn test_parse_gpu_count_gpu_after_non_gpu() {
        assert_eq!(parse_gpu_count("cpu:16,gpu:a100:8"), 8);
    }

    #[test]
    fn test_parse_gpu_count_zero_in_one_group() {
        assert_eq!(parse_gpu_count("gpu:a100:0,gpu:b100:2"), 2);
    }

    // ── parse_gpus_alloc ──────────────────────────────────────────────────

    #[test]
    fn test_parse_gpus_alloc_from_alloc_tres() {
        let output = "NodeName=node01 AllocTRES=cpu=4,mem=16G,gres/gpu=2
                      NodeName=node02 AllocTRES=cpu=8
";
        let result = parse_gpus_alloc(output);

        assert_eq!(result.get("node01"), Some(&2));
        assert_eq!(result.get("node02"), None);
    }

    #[test]
    fn test_parse_gpus_alloc_fallback_gres_used() {
        let output = "NodeName=node01 AllocTRES=cpu=4 GresUsed=gpu:a100:3(IDX:0,1,2)";
        let result = parse_gpus_alloc(output);

        assert_eq!(result.get("node01"), Some(&3));
    }

    #[test]
    fn test_parse_gpus_alloc_no_gpu_node() {
        let output = "NodeName=node01 AllocTRES=cpu=4,mem=8G";
        let result = parse_gpus_alloc(output);

        assert_eq!(result.get("node01"), None);
    }

    // ── parse_slurm_duration ──────────────────────────────────────────────

    #[test]
    fn test_parse_slurm_duration_hms() {
        assert_eq!(parse_slurm_duration("1:30:45"), Some(5445)); // 1*3600 + 30*60 + 45
    }

    #[test]
    fn test_parse_slurm_duration_ms() {
        assert_eq!(parse_slurm_duration("30:45"), Some(1845)); // 30*60 + 45
    }

    #[test]
    fn test_parse_slurm_duration_s() {
        assert_eq!(parse_slurm_duration("45"), Some(45));
    }

    #[test]
    fn test_parse_slurm_duration_days() {
        assert_eq!(
            parse_slurm_duration("2-10:30:45"),
            Some(2 * 86400 + 10 * 3600 + 30 * 60 + 45)
        );
    }

    #[test]
    fn test_parse_slurm_duration_empty() {
        assert_eq!(parse_slurm_duration(""), Some(0));
        assert_eq!(parse_slurm_duration("0"), Some(0));
    }

    #[test]
    fn test_parse_slurm_duration_malformed() {
        assert_eq!(parse_slurm_duration("invalid"), None);
        assert_eq!(parse_slurm_duration("1:2:3:4"), None);
    }

    // ── parse_scontrol_kv ─────────────────────────────────────────────────

    #[test]
    fn test_parse_scontrol_kv_normal() {
        let output = "JobId=123 JobName=myjob UserId=alice(1001) JobState=RUNNING";
        let kv = parse_scontrol_kv(output);

        assert_eq!(kv.get("JobId"), Some(&"123".to_string()));
        assert_eq!(kv.get("JobName"), Some(&"myjob".to_string()));
        assert_eq!(kv.get("JobState"), Some(&"RUNNING".to_string()));
    }

    #[test]
    fn test_parse_scontrol_kv_skips_no_equals() {
        let output = "NodeName=node01 State=idle SOMETOKEN";
        let kv = parse_scontrol_kv(output);

        assert_eq!(kv.get("NodeName"), Some(&"node01".to_string()));
        assert_eq!(kv.get("State"), Some(&"idle".to_string()));
        assert_eq!(kv.get("SOMETOKEN"), None);
    }

    // ── is_present ────────────────────────────────────────────────────────

    #[test]
    fn test_is_present() {
        assert!(!is_present(None));
        assert!(!is_present(Some("")));
        assert!(!is_present(Some("(null)")));
        assert!(!is_present(Some("N/A")));
        assert!(!is_present(Some("None")));
        assert!(!is_present(Some("none")));
        assert!(is_present(Some("real_value")));
    }

    // ── display ───────────────────────────────────────────────────────────

    #[test]
    fn test_display() {
        assert_eq!(display(None), "(unavailable)");
        assert_eq!(display(Some("")), "(unavailable)");
        assert_eq!(display(Some("(null)")), "(unavailable)");
        assert_eq!(display(Some("real_value")), "real_value");
        assert_eq!(display(Some("  trimmed  ")), "trimmed");
    }

    // ── normalize_node_state_token ────────────────────────────────────────

    #[test]
    fn test_normalize_node_state_token() {
        assert_eq!(normalize_node_state_token("idle"), "IDLE");
        assert_eq!(normalize_node_state_token("allocated*"), "ALLOCATED");
        assert_eq!(normalize_node_state_token("drain+"), "DRAIN");
        assert_eq!(normalize_node_state_token("down#"), "DOWN");
        assert_eq!(normalize_node_state_token("mixed~@!"), "MIXED");
    }

    // ── safe_int ──────────────────────────────────────────────────────────

    #[test]
    fn debug_regex() {
        let test_str = "gpu:4";
        println!("Testing: {}", test_str);
        let result = parse_gpu_count(test_str);
        println!("Result: {}", result);

        use regex::Regex;
        let re = Regex::new(r"\bgpu:(?:[^:,()\s]+:)?(\d+)").unwrap();
        println!("Regex is valid");

        for cap in re.captures_iter(test_str) {
            println!("Capture: {:?}", cap);
            if let Some(m) = cap.get(1) {
                println!("Group 1: {}", m.as_str());
            }
        }
    }

    #[test]
    fn test_safe_int() {
        assert_eq!(safe_int(None), None);
        assert_eq!(safe_int(Some("")), None);
        assert_eq!(safe_int(Some("?")), None);
        assert_eq!(safe_int(Some("123")), Some(123));
        assert_eq!(safe_int(Some("  456  ")), Some(456));
        assert_eq!(safe_int(Some("invalid")), None);
    }
}

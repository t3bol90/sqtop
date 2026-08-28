//! Job and node investigation orchestration (SPEC sec. 8.4, 8.5, 9.3, 10.3).
//!
//! This module contains the data-layer functions that gather evidence from
//! multiple Slurm sources (scontrol, squeue, sinfo, sacct) and assemble a
//! complete investigation report. Both functions are tolerant of partial
//! failure: any individual command error is recorded and the rest of the
//! report is still produced.

use crate::investigation::{
    explain_node_state, InvestigationError, InvestigationEvidence, InvestigationExplanation,
    InvestigationItem, InvestigationReport, InvestigationTarget, ReasonTable,
};
use crate::slurm::exec::Runner;
use crate::slurm::parse::{display, is_present, normalize_node_state_token};
use std::collections::{HashMap, HashSet};
use std::sync::LazyLock;
use std::time::SystemTime;

// Job-state sets (SPEC sec. 6.7)
static PENDING_STATES: LazyLock<HashSet<&str>> =
    LazyLock::new(|| ["PENDING", "PD"].iter().copied().collect());

static RUNNING_STATES: LazyLock<HashSet<&str>> =
    LazyLock::new(|| ["RUNNING", "R"].iter().copied().collect());

static TERMINAL_STATES: LazyLock<HashSet<&str>> = LazyLock::new(|| {
    [
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
    ]
    .iter()
    .copied()
    .collect()
});

// Node-state sets (SPEC sec. 8.5.1)
static NODE_ACTIVE_STATES: LazyLock<HashSet<&str>> =
    LazyLock::new(|| ["ALLOCATED", "MIXED"].iter().copied().collect());

static NODE_UNAVAILABLE_STATES: LazyLock<HashSet<&str>> =
    LazyLock::new(|| ["DOWN", "DRAIN", "DRAINED"].iter().copied().collect());

/// Command execution abstraction for testing.
///
/// Production code uses Runner; tests use FakeCommands.
pub trait CommandSource {
    /// Execute a command, returning (stdout, ok, stderr).
    fn run_result(&self, cmd: &str) -> (String, bool, String);

    /// Execute a command, returning stdout only (stderr discarded).
    fn run(&self, cmd: &str) -> String {
        let (stdout, _, _) = self.run_result(cmd);
        stdout
    }
}

impl CommandSource for Runner {
    fn run_result(&self, cmd: &str) -> (String, bool, String) {
        Runner::run_result(self, cmd)
    }

    fn run(&self, cmd: &str) -> String {
        Runner::run(self, cmd)
    }
}

/// Build an InvestigationReport for a single job.
///
/// SPEC sec. 8.4 / 9.3 / 10.3. Tolerant of partial failure: scontrol
/// unavailable, job_id absent from the live squeue snapshot, or
/// dependency parse errors must NOT abort the whole report. A report
/// with errors is always preferable to no report.
pub fn investigate_job(
    source: &impl CommandSource,
    reasons: &ReasonTable,
    job_id: &str,
) -> InvestigationReport {
    let target = InvestigationTarget {
        kind: "job".to_string(),
        identifier: job_id.to_string(),
        source: "typed".to_string(),
    };

    let mut report = InvestigationReport {
        target,
        generated_at: SystemTime::now(),
        summary: Vec::new(),
        evidence: Vec::new(),
        explanations: Vec::new(),
        related_jobs: Vec::new(),
        related_nodes: Vec::new(),
        suggested_actions: Vec::new(),
        raw_sections: HashMap::new(),
        errors: Vec::new(),
    };

    // ---- scontrol show job ------------------------------------------------
    let (scontrol_out, scontrol_ok, scontrol_err) =
        source.run_result(&format!("scontrol show job {}", job_id));

    let mut detail: HashMap<String, String> = HashMap::new();
    if scontrol_ok {
        for token in scontrol_out.split_whitespace() {
            if let Some((k, v)) = token.split_once('=') {
                detail.insert(k.to_string(), v.to_string());
            }
        }
        report
            .raw_sections
            .insert("scontrol show job".to_string(), "available".to_string());
    } else {
        report
            .raw_sections
            .insert("scontrol show job".to_string(), "unavailable".to_string());
        if !scontrol_err.is_empty() {
            report.errors.push(InvestigationError {
                source: "scontrol".to_string(),
                category: "slurm_command_failed".to_string(),
                message: format!("scontrol show job {} failed", job_id),
                stderr: Some(scontrol_err),
            });
        }
    }

    // ---- live squeue row --------------------------------------------------
    use crate::slurm::parse::parse_squeue_row;
    let squeue_out = source.run(&format!(
        "squeue --noheader -o '{}' 2>/dev/null",
        crate::slurm::parse::SQUEUE_FMT
    ));

    let live = squeue_out
        .lines()
        .filter_map(parse_squeue_row)
        .find(|j| j.job_id == job_id);

    if live.is_none() {
        report.errors.push(InvestigationError {
            source: "squeue".to_string(),
            category: "job_not_found".to_string(),
            message: "Job not in current squeue snapshot".to_string(),
            stderr: None,
        });
    }

    // ---- determine state and reason --------------------------------------
    let state_raw = if let Some(ref j) = live {
        &j.state
    } else {
        detail.get("JobState").map(|s| s.as_str()).unwrap_or("")
    };
    let state = state_raw.to_uppercase();

    let mut reason_raw = String::new();
    let mut reason_source_id = String::new();

    if let Some(ref j) = live {
        if is_present(Some(&j.reason)) {
            reason_raw = j.reason.clone();
            reason_source_id = "squeue.reason".to_string();
        }
    }

    if reason_raw.is_empty() {
        if let Some(r) = detail.get("Reason") {
            if is_present(Some(r)) {
                reason_raw = r.clone();
                reason_source_id = "scontrol.Reason".to_string();
            }
        }
    }

    // ---- summary items ----------------------------------------------------
    let user = if let Some(ref j) = live {
        &j.user
    } else {
        detail.get("UserId").map(|s| s.as_str()).unwrap_or("")
    };

    // scontrol form: "alice(1001)"
    let user = if let Some(idx) = user.find('(') {
        &user[..idx]
    } else {
        user
    };

    let partition = if let Some(ref j) = live {
        &j.partition
    } else {
        detail.get("Partition").map(|s| s.as_str()).unwrap_or("")
    };

    let submit_time = detail.get("SubmitTime").map(|s| s.as_str()).unwrap_or("");
    let start_time = detail.get("StartTime").map(|s| s.as_str()).unwrap_or("");

    let time_used = if let Some(ref j) = live {
        &j.time_used
    } else {
        detail.get("RunTime").map(|s| s.as_str()).unwrap_or("")
    };

    let time_limit = if let Some(ref j) = live {
        &j.time_limit
    } else {
        detail.get("TimeLimit").map(|s| s.as_str()).unwrap_or("")
    };

    let num_nodes = if let Some(ref j) = live {
        &j.num_nodes
    } else {
        detail.get("NumNodes").map(|s| s.as_str()).unwrap_or("")
    };

    let num_cpus = if let Some(ref j) = live {
        &j.num_cpus
    } else {
        detail.get("NumCPUs").map(|s| s.as_str()).unwrap_or("")
    };

    let tres = detail
        .get("TRES")
        .or_else(|| detail.get("ReqTRES"))
        .map(|s| s.as_str())
        .unwrap_or("");

    // GPU count derived from TRES when present
    let gpu_request = if !tres.is_empty() {
        use regex::Regex;
        static GPU_RE: LazyLock<Regex> =
            LazyLock::new(|| Regex::new(r"gres/gpu(?::[^=]+)?=(\d+)").unwrap());
        GPU_RE
            .captures(tres)
            .and_then(|c| c.get(1))
            .map(|m| m.as_str())
            .unwrap_or("")
    } else {
        ""
    };

    report.summary.extend([
        InvestigationItem {
            label: "State".to_string(),
            value: display(Some(state_raw)),
        },
        InvestigationItem {
            label: "Reason".to_string(),
            value: display(Some(&reason_raw)),
        },
        InvestigationItem {
            label: "User".to_string(),
            value: display(Some(user)),
        },
        InvestigationItem {
            label: "Partition".to_string(),
            value: display(Some(partition)),
        },
        InvestigationItem {
            label: "Requested nodes".to_string(),
            value: display(Some(num_nodes)),
        },
        InvestigationItem {
            label: "Requested CPUs".to_string(),
            value: display(Some(num_cpus)),
        },
        InvestigationItem {
            label: "Requested GPUs".to_string(),
            value: display(Some(gpu_request)),
        },
        InvestigationItem {
            label: "Time used".to_string(),
            value: display(Some(time_used)),
        },
        InvestigationItem {
            label: "Time limit".to_string(),
            value: display(Some(time_limit)),
        },
        InvestigationItem {
            label: "Submit time".to_string(),
            value: display(Some(submit_time)),
        },
        InvestigationItem {
            label: "Start time".to_string(),
            value: display(Some(start_time)),
        },
    ]);

    // ---- evidence ---------------------------------------------------------
    if let Some(ref j) = live {
        report.evidence.push(InvestigationEvidence {
            id: "squeue.state".to_string(),
            label: "State".to_string(),
            value: j.state.clone(),
            source: "squeue".to_string(),
            confidence: "high".to_string(),
        });

        report.evidence.push(InvestigationEvidence {
            id: "squeue.reason".to_string(),
            label: "Reason".to_string(),
            value: if j.reason.is_empty() {
                "(none)".to_string()
            } else {
                j.reason.clone()
            },
            source: "squeue".to_string(),
            confidence: "high".to_string(),
        });
    }

    if !detail.is_empty() {
        if let Some(v) = detail.get("NumNodes") {
            if is_present(Some(v)) {
                report.evidence.push(InvestigationEvidence {
                    id: "scontrol.NumNodes".to_string(),
                    label: "NumNodes".to_string(),
                    value: v.clone(),
                    source: "scontrol".to_string(),
                    confidence: "high".to_string(),
                });
            }
        }

        if let Some(v) = detail.get("NumCPUs") {
            if is_present(Some(v)) {
                report.evidence.push(InvestigationEvidence {
                    id: "scontrol.NumCPUs".to_string(),
                    label: "NumCPUs".to_string(),
                    value: v.clone(),
                    source: "scontrol".to_string(),
                    confidence: "high".to_string(),
                });
            }
        }

        if let Some(v) = detail.get("TRES") {
            if is_present(Some(v)) {
                report.evidence.push(InvestigationEvidence {
                    id: "scontrol.TRES".to_string(),
                    label: "TRES".to_string(),
                    value: v.clone(),
                    source: "scontrol".to_string(),
                    confidence: "high".to_string(),
                });
            }
        }

        if let Some(v) = detail.get("Partition") {
            if is_present(Some(v)) {
                report.evidence.push(InvestigationEvidence {
                    id: "scontrol.Partition".to_string(),
                    label: "Partition".to_string(),
                    value: v.clone(),
                    source: "scontrol".to_string(),
                    confidence: "high".to_string(),
                });
            }
        }

        // QOS visibility varies by site policy
        if let Some(v) = detail.get("QOS") {
            if is_present(Some(v)) {
                report.evidence.push(InvestigationEvidence {
                    id: "scontrol.QOS".to_string(),
                    label: "QOS".to_string(),
                    value: v.clone(),
                    source: "scontrol".to_string(),
                    confidence: "medium".to_string(),
                });
            }
        }

        if let Some(v) = detail.get("TimeLimit") {
            if is_present(Some(v)) {
                report.evidence.push(InvestigationEvidence {
                    id: "scontrol.TimeLimit".to_string(),
                    label: "TimeLimit".to_string(),
                    value: v.clone(),
                    source: "scontrol".to_string(),
                    confidence: "high".to_string(),
                });
            }
        }

        if let Some(v) = detail.get("Dependency") {
            if is_present(Some(v)) {
                report.evidence.push(InvestigationEvidence {
                    id: "scontrol.Dependency".to_string(),
                    label: "Dependency".to_string(),
                    value: v.clone(),
                    source: "scontrol".to_string(),
                    confidence: "high".to_string(),
                });
            }
        }

        if RUNNING_STATES.contains(state.as_str()) {
            if let Some(v) = detail.get("NodeList") {
                if is_present(Some(v)) {
                    report.evidence.push(InvestigationEvidence {
                        id: "scontrol.NodeList".to_string(),
                        label: "NodeList".to_string(),
                        value: v.clone(),
                        source: "scontrol".to_string(),
                        confidence: "high".to_string(),
                    });
                }
            }
        }
    }

    // ---- pending-reason explanation --------------------------------------
    if PENDING_STATES.contains(state.as_str()) {
        let explanation = reasons.explain_pending_reason(if is_present(Some(&reason_raw)) {
            Some(&reason_raw)
        } else {
            None
        });

        let evidence_refs = if !reason_source_id.is_empty() {
            vec![reason_source_id.clone()]
        } else {
            Vec::new()
        };

        report.explanations.push(InvestigationExplanation {
            title: explanation.title,
            detail: explanation.detail,
            confidence: explanation.confidence,
            evidence_refs,
        });
    }

    // ---- dependencies ----------------------------------------------------
    use crate::slurm::fetch::JobDependency;
    let deps = if let Some(dep_str) = detail.get("Dependency") {
        if is_present(Some(dep_str)) && dep_str != "(null)" {
            // Parse dependencies: afterok:123,afterany:456
            let mut parsed_deps = Vec::new();
            for part in dep_str.split(',') {
                if let Some((dep_type, job_id)) = part.split_once(':') {
                    // Try to find the state via squeue -j
                    let dep_state_out =
                        source.run(&format!("squeue --noheader -j {} -o '%T'", job_id));
                    let state = dep_state_out.trim().to_string();
                    let state = if state.is_empty() {
                        "COMPLETED".to_string()
                    } else {
                        state
                    };

                    parsed_deps.push(JobDependency {
                        dep_type: dep_type.to_string(),
                        job_id: job_id.to_string(),
                        state,
                    });
                }
            }
            parsed_deps
        } else {
            Vec::new()
        }
    } else {
        Vec::new()
    };

    for dep in &deps {
        report.evidence.push(InvestigationEvidence {
            id: format!("dep.{}", dep.job_id),
            label: format!("Dependency {}:{}", dep.dep_type, dep.job_id),
            value: if dep.state.is_empty() {
                "(unknown)".to_string()
            } else {
                dep.state.clone()
            },
            source: "squeue".to_string(),
            confidence: "high".to_string(),
        });
    }

    if PENDING_STATES.contains(state.as_str()) && !deps.is_empty() {
        for dep in &deps {
            // Treat anything not COMPLETED as "unsatisfied"
            let dep_state = dep.state.to_uppercase();
            if dep_state != "COMPLETED" && dep_state != "CD" {
                report.explanations.push(InvestigationExplanation {
                    title: "Dependency".to_string(),
                    detail: format!(
                        "Job is waiting on dependency {}:{} (state: {}).",
                        dep.dep_type,
                        dep.job_id,
                        if dep.state.is_empty() {
                            "unknown"
                        } else {
                            &dep.state
                        }
                    ),
                    confidence: "high".to_string(),
                    evidence_refs: vec!["scontrol.Dependency".to_string()],
                });
            }
        }
    }

    // ---- related nodes ---------------------------------------------------
    let nodelist_expr = if RUNNING_STATES.contains(state.as_str()) {
        if let Some(ref j) = live {
            j.nodelist.as_str()
        } else {
            detail.get("NodeList").map(|s| s.as_str()).unwrap_or("")
        }
    } else if PENDING_STATES.contains(state.as_str()) {
        detail.get("ReqNodeList").map(|s| s.as_str()).unwrap_or("")
    } else {
        ""
    };

    if is_present(Some(nodelist_expr)) {
        let hosts_out = source.run(&format!("scontrol show hostnames {}", nodelist_expr));
        let requested_names: HashSet<_> = hosts_out
            .lines()
            .map(|l| l.trim())
            .filter(|l| !l.is_empty())
            .collect();

        if !requested_names.is_empty() {
            // Fetch nodes via sinfo
            use crate::slurm::parse::{parse_gpus_alloc, parse_node_row};
            let sinfo_out = source.run(&format!(
                "sinfo --noheader -N -o '{}'",
                crate::slurm::parse::SINFO_PARTITION_FMT
            ));

            let gres_out = source.run("sinfo --noheader -N -o '%N %G'");
            let gpus_alloc = parse_gpus_alloc(&gres_out);

            for line in sinfo_out.lines() {
                if let Some(node) = parse_node_row(line, &gpus_alloc) {
                    if requested_names.contains(node.name.as_str()) {
                        report.related_nodes.push(node);
                    }
                }
            }
        }
    }

    // ---- sacct accounting (terminal-state jobs only, SPEC sec. 8.4) ------
    if TERMINAL_STATES.contains(state.as_str()) {
        use crate::slurm::parse::parse_slurm_duration;

        let sacct_out = source.run(&format!(
            "sacct -j {} --noheader -P -o JobID,CPUTimeRAW,TotalCPU,ReqMem,MaxRSS",
            job_id
        ));

        let mut available = false;
        let mut cpu_eff = 0.0;
        let mut mem_eff = 0.0;
        let mut cpu_used_str = String::new();
        let mut cpu_alloc_str = String::new();
        let mut mem_peak_mb = 0u64;
        let mut mem_alloc_mb = 0u64;

        for line in sacct_out.lines() {
            let parts: Vec<&str> = line.split('|').collect();
            if parts.len() >= 5 && parts[0] == job_id {
                if let Ok(cpu_time_raw) = parts[1].parse::<u64>() {
                    if let Some(total_cpu_sec) = parse_slurm_duration(parts[2]) {
                        if cpu_time_raw > 0 {
                            cpu_eff = total_cpu_sec as f64 / cpu_time_raw as f64;
                            cpu_used_str = parts[2].to_string();
                            cpu_alloc_str = format!("{}s", cpu_time_raw);
                        }
                    }
                }

                // Parse memory
                let req_mem = parts[3];
                let max_rss = parts[4];

                if let Some(req_mb) = req_mem
                    .strip_suffix('M')
                    .and_then(|s| s.parse::<u64>().ok())
                {
                    mem_alloc_mb = req_mb;
                }

                if let Some(peak_kb) = max_rss
                    .strip_suffix('K')
                    .and_then(|s| s.parse::<u64>().ok())
                {
                    mem_peak_mb = peak_kb / 1024;
                } else if let Some(peak_mb) = max_rss
                    .strip_suffix('M')
                    .and_then(|s| s.parse::<u64>().ok())
                {
                    mem_peak_mb = peak_mb;
                }

                if mem_alloc_mb > 0 {
                    mem_eff = mem_peak_mb as f64 / mem_alloc_mb as f64;
                }

                available = true;
                break;
            }
        }

        if available {
            report.evidence.push(InvestigationEvidence {
                id: "sacct.cpu_eff".to_string(),
                label: "CPU efficiency".to_string(),
                value: format!(
                    "{}% (used {} of {})",
                    (cpu_eff * 100.0).round() as i32,
                    cpu_used_str,
                    cpu_alloc_str
                ),
                source: "sacct".to_string(),
                confidence: "high".to_string(),
            });

            report.evidence.push(InvestigationEvidence {
                id: "sacct.mem_eff".to_string(),
                label: "Memory efficiency".to_string(),
                value: format!(
                    "{}% (peak {} MB of {} MB allocated)",
                    (mem_eff * 100.0).round() as i32,
                    mem_peak_mb,
                    mem_alloc_mb
                ),
                source: "sacct".to_string(),
                confidence: "high".to_string(),
            });

            report
                .raw_sections
                .insert("sacct".to_string(), "available".to_string());
        } else {
            report
                .raw_sections
                .insert("sacct".to_string(), "unavailable".to_string());
            report.errors.push(InvestigationError {
                source: "sacct".to_string(),
                category: "slurm_field_unavailable".to_string(),
                message: "sacct accounting not available for this job".to_string(),
                stderr: None,
            });
        }
    }

    report
}

/// Build an InvestigationReport for a single node.
///
/// SPEC sec. 8.5 / 9.3. Tolerant of partial failure: scontrol
/// unavailable, node missing from the live sinfo snapshot, or
/// fetch_jobs_on_node returning empty must NOT raise. A report with
/// errors is always preferable to no report.
pub fn investigate_node(
    source: &impl CommandSource,
    node_name: &str,
    max_related_jobs: usize,
) -> InvestigationReport {
    let target = InvestigationTarget {
        kind: "node".to_string(),
        identifier: node_name.to_string(),
        source: "typed".to_string(),
    };

    let mut report = InvestigationReport {
        target,
        generated_at: SystemTime::now(),
        summary: Vec::new(),
        evidence: Vec::new(),
        explanations: Vec::new(),
        related_jobs: Vec::new(),
        related_nodes: Vec::new(),
        suggested_actions: Vec::new(),
        raw_sections: HashMap::new(),
        errors: Vec::new(),
    };

    // ---- scontrol show node ----------------------------------------------
    let (scontrol_out, scontrol_ok, scontrol_err) =
        source.run_result(&format!("scontrol show node {}", node_name));

    let mut detail: HashMap<String, String> = HashMap::new();
    if scontrol_ok {
        for token in scontrol_out.split_whitespace() {
            if let Some((k, v)) = token.split_once('=') {
                detail.insert(k.to_string(), v.to_string());
            }
        }
        report
            .raw_sections
            .insert("scontrol show node".to_string(), "available".to_string());
    } else {
        report
            .raw_sections
            .insert("scontrol show node".to_string(), "unavailable".to_string());
        if !scontrol_err.is_empty() {
            report.errors.push(InvestigationError {
                source: "scontrol".to_string(),
                category: "slurm_command_failed".to_string(),
                message: format!("scontrol show node {} failed", node_name),
                stderr: Some(scontrol_err),
            });
        }
    }

    // ---- live sinfo snapshot ---------------------------------------------
    use crate::slurm::parse::{parse_gpus_alloc, parse_node_row};
    let sinfo_out = source.run(&format!(
        "sinfo --noheader -N -o '{}'",
        crate::slurm::parse::SINFO_PARTITION_FMT
    ));

    let gres_out = source.run("sinfo --noheader -N -o '%N %G'");
    let gpus_alloc = parse_gpus_alloc(&gres_out);

    let live = sinfo_out
        .lines()
        .filter_map(|line| parse_node_row(line, &gpus_alloc))
        .find(|n| n.name == node_name);

    if live.is_none() {
        report.errors.push(InvestigationError {
            source: "sinfo".to_string(),
            category: "node_not_found".to_string(),
            message: "Node not in current sinfo snapshot".to_string(),
            stderr: None,
        });
    }

    // ---- determine state and reason --------------------------------------
    let state_raw = if let Some(ref n) = live {
        &n.state
    } else {
        detail.get("State").map(|s| s.as_str()).unwrap_or("")
    };

    let state_token = normalize_node_state_token(state_raw);

    let partition = if let Some(ref n) = live {
        &n.partition
    } else {
        detail.get("Partitions").map(|s| s.as_str()).unwrap_or("")
    };

    let cpus_total_str = if let Some(ref n) = live {
        &n.cpus_total
    } else {
        detail.get("CPUTot").map(|s| s.as_str()).unwrap_or("")
    };

    let cpus_alloc_str = if let Some(ref n) = live {
        &n.cpus_alloc
    } else {
        detail.get("CPUAlloc").map(|s| s.as_str()).unwrap_or("")
    };

    let memory_total = if let Some(ref n) = live {
        &n.memory_total
    } else {
        detail.get("RealMemory").map(|s| s.as_str()).unwrap_or("")
    };

    let memory_free = if let Some(ref n) = live {
        &n.memory_free
    } else {
        detail.get("FreeMem").map(|s| s.as_str()).unwrap_or("")
    };

    let load = if let Some(ref n) = live {
        &n.load
    } else {
        detail.get("CPULoad").map(|s| s.as_str()).unwrap_or("")
    };

    let gpu_total = if let Some(ref n) = live {
        n.gpu_total
    } else {
        0
    };

    let gpu_alloc = if let Some(ref n) = live {
        n.gpu_alloc
    } else {
        0
    };

    let reason_raw = detail.get("Reason").map(|s| s.as_str()).unwrap_or("");

    // ---- summary items ----------------------------------------------------
    report.summary.push(InvestigationItem {
        label: "State".to_string(),
        value: display(Some(state_raw)),
    });

    report.summary.push(InvestigationItem {
        label: "Partition".to_string(),
        value: display(Some(partition)),
    });

    let cpus_display = if !cpus_alloc_str.is_empty() || !cpus_total_str.is_empty() {
        format!("{}/{}", cpus_alloc_str, cpus_total_str)
    } else {
        "(unavailable)".to_string()
    };

    report.summary.push(InvestigationItem {
        label: "CPUs allocated/total".to_string(),
        value: cpus_display,
    });

    // SPEC sec. 6.2: missing GPU data MUST NOT imply zero GPUs.
    if gpu_total > 0 {
        report.summary.push(InvestigationItem {
            label: "GPUs allocated/total".to_string(),
            value: format!("{}/{}", gpu_alloc, gpu_total),
        });
    }

    report.summary.push(InvestigationItem {
        label: "Memory free/total".to_string(),
        value: if !memory_free.is_empty() || !memory_total.is_empty() {
            format!("{}/{}", memory_free, memory_total)
        } else {
            "(unavailable)".to_string()
        },
    });

    report.summary.push(InvestigationItem {
        label: "Load".to_string(),
        value: display(Some(load)),
    });

    report.summary.push(InvestigationItem {
        label: "Reason".to_string(),
        value: display(Some(reason_raw)),
    });

    // ---- evidence ---------------------------------------------------------
    if let Some(ref n) = live {
        report.evidence.push(InvestigationEvidence {
            id: "sinfo.state".to_string(),
            label: "State".to_string(),
            value: n.state.clone(),
            source: "sinfo".to_string(),
            confidence: "high".to_string(),
        });

        if !n.cpus_total.is_empty() {
            report.evidence.push(InvestigationEvidence {
                id: "sinfo.cpus_total".to_string(),
                label: "CPUs (total)".to_string(),
                value: n.cpus_total.clone(),
                source: "sinfo".to_string(),
                confidence: "high".to_string(),
            });
        }

        if !n.cpus_alloc.is_empty() {
            report.evidence.push(InvestigationEvidence {
                id: "sinfo.cpus_alloc".to_string(),
                label: "CPUs (allocated)".to_string(),
                value: n.cpus_alloc.clone(),
                source: "sinfo".to_string(),
                confidence: "high".to_string(),
            });
        }

        if n.gpu_total > 0 {
            report.evidence.push(InvestigationEvidence {
                id: "sinfo.gpu_total".to_string(),
                label: "GPUs (total)".to_string(),
                value: n.gpu_total.to_string(),
                source: "sinfo".to_string(),
                confidence: "high".to_string(),
            });

            report.evidence.push(InvestigationEvidence {
                id: "sinfo.gpu_alloc".to_string(),
                label: "GPUs (allocated)".to_string(),
                value: n.gpu_alloc.to_string(),
                source: "sinfo".to_string(),
                confidence: "high".to_string(),
            });
        }

        if !n.memory_total.is_empty() {
            report.evidence.push(InvestigationEvidence {
                id: "sinfo.memory_total".to_string(),
                label: "Memory (total)".to_string(),
                value: n.memory_total.clone(),
                source: "sinfo".to_string(),
                confidence: "high".to_string(),
            });
        }

        if !n.memory_free.is_empty() {
            report.evidence.push(InvestigationEvidence {
                id: "sinfo.memory_free".to_string(),
                label: "Memory (free)".to_string(),
                value: n.memory_free.clone(),
                source: "sinfo".to_string(),
                confidence: "high".to_string(),
            });
        }
    }

    if !detail.is_empty() {
        if let Some(v) = detail.get("Gres") {
            if is_present(Some(v)) {
                report.evidence.push(InvestigationEvidence {
                    id: "scontrol.Gres".to_string(),
                    label: "Gres".to_string(),
                    value: v.clone(),
                    source: "scontrol".to_string(),
                    confidence: "high".to_string(),
                });
            }
        }

        if let Some(v) = detail.get("Reason") {
            if is_present(Some(v)) {
                report.evidence.push(InvestigationEvidence {
                    id: "scontrol.Reason".to_string(),
                    label: "Reason".to_string(),
                    value: v.clone(),
                    source: "scontrol".to_string(),
                    confidence: "high".to_string(),
                });
            }
        }

        if let Some(v) = detail.get("CPULoad") {
            if is_present(Some(v)) {
                report.evidence.push(InvestigationEvidence {
                    id: "scontrol.CPULoad".to_string(),
                    label: "CPULoad".to_string(),
                    value: v.clone(),
                    source: "scontrol".to_string(),
                    confidence: "medium".to_string(),
                });
            }
        }
    }

    // ---- state explanation -----------------------------------------------
    let explanation = explain_node_state(&state_token);
    report.explanations.push(explanation);

    // ---- related jobs ----------------------------------------------------
    use crate::slurm::parse::parse_squeue_row;
    let jobs_out = source.run(&format!(
        "squeue --noheader -w {} -o '{}'",
        node_name,
        crate::slurm::parse::SQUEUE_FMT
    ));

    let jobs_on_node: Vec<_> = jobs_out.lines().filter_map(parse_squeue_row).collect();

    if jobs_on_node.is_empty() {
        // No visible jobs - add explanation based on state
        if NODE_ACTIVE_STATES.contains(state_token.as_str()) {
            report.explanations.push(InvestigationExplanation {
                title: "No jobs visible on node".to_string(),
                detail: "squeue -w shows no jobs, but the node is marked ALLOCATED or MIXED. This can occur when jobs belong to other users and are hidden by Slurm ACLs.".to_string(),
                confidence: "medium".to_string(),
                evidence_refs: vec!["sinfo.state".to_string()],
            });
        } else if !NODE_UNAVAILABLE_STATES.contains(state_token.as_str()) {
            // Not active and not unavailable - probably idle
            report.explanations.push(InvestigationExplanation {
                title: "No jobs on node".to_string(),
                detail: "The node appears available for new work.".to_string(),
                confidence: "high".to_string(),
                evidence_refs: vec!["sinfo.state".to_string()],
            });
        }
    } else {
        // Cap to max_related_jobs
        let limit = if max_related_jobs == 0 {
            0
        } else {
            max_related_jobs
        };
        report.related_jobs = jobs_on_node.into_iter().take(limit).collect();
    }

    report
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::investigation::ReasonTable;
    use std::collections::HashMap;

    /// Fake command source for testing.
    struct FakeCommands {
        responses: HashMap<String, (String, bool, String)>,
    }

    impl FakeCommands {
        fn new() -> Self {
            Self {
                responses: HashMap::new(),
            }
        }

        /// Add a canned response for commands containing the given substring.
        fn add(&mut self, pattern: &str, stdout: &str) {
            self.responses.insert(
                pattern.to_string(),
                (stdout.to_string(), true, String::new()),
            );
        }

        /// Add a failure response.
        fn add_fail(&mut self, pattern: &str, stderr: &str) {
            self.responses.insert(
                pattern.to_string(),
                (String::new(), false, stderr.to_string()),
            );
        }
    }

    impl CommandSource for FakeCommands {
        fn run_result(&self, cmd: &str) -> (String, bool, String) {
            // Match by substring
            for (pattern, response) in &self.responses {
                if cmd.contains(pattern) {
                    return response.clone();
                }
            }
            // Default: command not found
            (String::new(), false, "command not found".to_string())
        }
    }

    // State constant tests
    #[test]
    fn test_pending_states() {
        assert!(PENDING_STATES.contains("PENDING"));
        assert!(PENDING_STATES.contains("PD"));
        assert_eq!(PENDING_STATES.len(), 2);
    }

    #[test]
    fn test_running_states() {
        assert!(RUNNING_STATES.contains("RUNNING"));
        assert!(RUNNING_STATES.contains("R"));
        assert_eq!(RUNNING_STATES.len(), 2);
    }

    #[test]
    fn test_terminal_states() {
        assert!(TERMINAL_STATES.contains("COMPLETED"));
        assert!(TERMINAL_STATES.contains("CD"));
        assert!(TERMINAL_STATES.contains("FAILED"));
        assert!(TERMINAL_STATES.contains("F"));
        assert!(TERMINAL_STATES.contains("CANCELLED"));
        assert!(TERMINAL_STATES.contains("CA"));
        assert!(TERMINAL_STATES.contains("TIMEOUT"));
        assert!(TERMINAL_STATES.contains("TO"));
        assert!(TERMINAL_STATES.contains("NODE_FAIL"));
        assert!(TERMINAL_STATES.contains("NF"));
        assert!(TERMINAL_STATES.contains("PREEMPTED"));
        assert!(TERMINAL_STATES.contains("PR"));
        assert!(TERMINAL_STATES.contains("OUT_OF_MEMORY"));
        assert!(TERMINAL_STATES.contains("OOM"));
        assert_eq!(TERMINAL_STATES.len(), 14);
    }

    #[test]
    fn test_node_active_states() {
        assert!(NODE_ACTIVE_STATES.contains("ALLOCATED"));
        assert!(NODE_ACTIVE_STATES.contains("MIXED"));
        assert_eq!(NODE_ACTIVE_STATES.len(), 2);
    }

    #[test]
    fn test_node_unavailable_states() {
        assert!(NODE_UNAVAILABLE_STATES.contains("DOWN"));
        assert!(NODE_UNAVAILABLE_STATES.contains("DRAIN"));
        assert!(NODE_UNAVAILABLE_STATES.contains("DRAINED"));
        assert_eq!(NODE_UNAVAILABLE_STATES.len(), 3);
    }

    // Behavioral tests for investigate_job

    #[test]
    fn test_investigate_job_pending_resources() {
        let mut fake = FakeCommands::new();
        let job_id = "12346";

        // scontrol show job
        fake.add(
            "scontrol show job",
            &format!(
                "JobId={} JobName=preprocess UserId=bob(1002) \
                 JobState=PENDING Reason=Resources Dependency=(null) \
                 Partition=gpu QOS=normal \
                 NumNodes=1 NumCPUs=16 TimeLimit=24:00:00 RunTime=00:00:00 \
                 TRES=cpu=16,mem=128G,node=1,gres/gpu=1 \
                 SubmitTime=2026-05-08T11:00:00 StartTime=Unknown \
                 NodeList=(null) ReqNodeList=(null)",
                job_id
            ),
        );

        // squeue
        fake.add(
            "squeue --noheader",
            &format!(
                "{}|preprocess|bob|PENDING|gpu|1|16|0:00|24:00:00|Resources||normal\n",
                job_id
            ),
        );

        let reasons = ReasonTable::default();
        let report = investigate_job(&fake, &reasons, job_id);

        assert_eq!(report.target.kind, "job");
        assert_eq!(report.target.identifier, job_id);
        assert!(report.errors.is_empty());

        // Should have Resources reason in evidence
        let reason_ev: Vec<_> = report
            .evidence
            .iter()
            .filter(|e| e.id == "squeue.reason")
            .collect();
        assert_eq!(reason_ev.len(), 1);
        assert_eq!(reason_ev[0].value, "Resources");

        // Should have explanation about resources
        assert!(!report.explanations.is_empty());
        let has_resource_explanation = report.explanations.iter().any(|exp| {
            exp.title.to_lowercase().contains("resource")
                || exp.detail.to_lowercase().contains("resource")
        });
        assert!(has_resource_explanation);
    }

    #[test]
    fn test_investigate_job_pending_dependency() {
        let mut fake = FakeCommands::new();
        let job_id = "12347";
        let dep_id = "99999";

        // scontrol show job
        fake.add(
            "scontrol show job",
            &format!(
                "JobId={} JobName=postproc UserId=carol(1003) \
                 JobState=PENDING Reason=Dependency Dependency=afterok:{} \
                 Partition=cpu QOS=normal \
                 NumNodes=1 NumCPUs=4 TimeLimit=01:00:00 RunTime=00:00:00 \
                 TRES=cpu=4,mem=8G,node=1 \
                 SubmitTime=2026-05-08T12:00:00 StartTime=Unknown \
                 NodeList=(null) ReqNodeList=(null)",
                job_id, dep_id
            ),
        );

        // squeue main
        fake.add(
            "squeue --noheader -o",
            &format!(
                "{}|postproc|carol|PENDING|cpu|1|4|0:00|1:00:00|Dependency||normal\n",
                job_id
            ),
        );

        // squeue -j for dependency check
        fake.add(&format!("squeue --noheader -j {}", dep_id), "RUNNING\n");

        let reasons = ReasonTable::default();
        let report = investigate_job(&fake, &reasons, job_id);

        assert!(report.errors.is_empty());

        // Should have dependency in evidence
        let dep_ev: Vec<_> = report
            .evidence
            .iter()
            .filter(|e| e.id == format!("dep.{}", dep_id))
            .collect();
        assert_eq!(dep_ev.len(), 1);
        assert_eq!(dep_ev[0].value, "RUNNING");

        // Should have dependency explanation
        let dep_exp: Vec<_> = report
            .explanations
            .iter()
            .filter(|exp| exp.title == "Dependency")
            .collect();
        assert_eq!(dep_exp.len(), 1);
        assert!(dep_exp[0].detail.contains(&dep_id.to_string()));
        assert!(dep_exp[0].detail.contains("RUNNING"));
    }

    #[test]
    fn test_investigate_job_running_populates_nodes() {
        let mut fake = FakeCommands::new();
        let job_id = "12348";

        // scontrol show job
        fake.add(
            "scontrol show job",
            &format!(
                "JobId={} JobName=train UserId=alice(1001) \
                 JobState=RUNNING Reason=None Dependency=(null) \
                 Partition=gpu QOS=normal \
                 NumNodes=1 NumCPUs=8 TimeLimit=08:00:00 RunTime=00:30:00 \
                 TRES=cpu=8,mem=64G,node=1,gres/gpu=2 \
                 SubmitTime=2026-05-08T10:00:00 StartTime=2026-05-08T10:15:00 \
                 NodeList=node01 ReqNodeList=(null)",
                job_id
            ),
        );

        // squeue
        fake.add(
            "squeue --noheader -o",
            &format!(
                "{}|train|alice|RUNNING|gpu|1|8|0:30:00|8:00:00|None|node01|normal\n",
                job_id
            ),
        );

        // scontrol show hostnames
        fake.add("scontrol show hostnames", "node01\n");

        // sinfo for node
        fake.add(
            "sinfo --noheader -N -o",
            "node01|mixed|gpu|16|8/4/0/12|128000|64000|8.45|gpu:4\n",
        );

        // sinfo gres
        fake.add("sinfo --noheader -N -o '%N %G'", "node01 gpu:4(S:0)\n");

        let reasons = ReasonTable::default();
        let report = investigate_job(&fake, &reasons, job_id);

        assert!(report.errors.is_empty());

        // Should have NodeList in evidence
        let nodelist_ev: Vec<_> = report
            .evidence
            .iter()
            .filter(|e| e.id == "scontrol.NodeList")
            .collect();
        assert_eq!(nodelist_ev.len(), 1);
        assert_eq!(nodelist_ev[0].value, "node01");

        // Should have related node
        assert_eq!(report.related_nodes.len(), 1);
        assert_eq!(report.related_nodes[0].name, "node01");
    }

    #[test]
    fn test_investigate_job_completed_with_sacct() {
        let mut fake = FakeCommands::new();
        let job_id = "12349";

        // scontrol show job
        fake.add(
            "scontrol show job",
            &format!(
                "JobId={} JobName=process UserId=bob(1002) \
                 JobState=COMPLETED Reason=None Dependency=(null) \
                 Partition=cpu QOS=normal \
                 NumNodes=1 NumCPUs=4 TimeLimit=02:00:00 RunTime=01:45:30 \
                 TRES=cpu=4,mem=16G,node=1 \
                 SubmitTime=2026-05-07T14:00:00 StartTime=2026-05-07T14:05:00 \
                 NodeList=node02 ReqNodeList=(null)",
                job_id
            ),
        );

        // squeue (job not in queue)
        fake.add("squeue --noheader -o", "");

        // sacct
        fake.add(
            "sacct -j",
            &format!("{}|14400|3600.5|16000M|8192000K\n", job_id),
        );

        let reasons = ReasonTable::default();
        let report = investigate_job(&fake, &reasons, job_id);

        // Should have sacct evidence
        let cpu_eff_ev: Vec<_> = report
            .evidence
            .iter()
            .filter(|e| e.id == "sacct.cpu_eff")
            .collect();
        assert_eq!(cpu_eff_ev.len(), 1);
        assert!(cpu_eff_ev[0].value.contains('%'));

        let mem_eff_ev: Vec<_> = report
            .evidence
            .iter()
            .filter(|e| e.id == "sacct.mem_eff")
            .collect();
        assert_eq!(mem_eff_ev.len(), 1);
        assert!(mem_eff_ev[0].value.contains('%'));

        assert_eq!(report.raw_sections.get("sacct").unwrap(), "available");
    }

    #[test]
    fn test_investigate_job_scontrol_failure_partial_report() {
        let mut fake = FakeCommands::new();
        let job_id = "12350";

        // scontrol fails
        fake.add_fail("scontrol show job", "permission denied");

        // squeue succeeds
        fake.add(
            "squeue --noheader",
            &format!(
                "{}|myjob|alice|RUNNING|gpu|1|8|1:00:00|4:00:00|None|node01|normal\n",
                job_id
            ),
        );

        let reasons = ReasonTable::default();
        let report = investigate_job(&fake, &reasons, job_id);

        // Should have an error about scontrol
        assert_eq!(report.errors.len(), 1);
        assert_eq!(report.errors[0].source, "scontrol");
        assert!(report.errors[0]
            .stderr
            .as_ref()
            .unwrap()
            .contains("permission"));

        // But should still have squeue evidence
        let state_ev: Vec<_> = report
            .evidence
            .iter()
            .filter(|e| e.id == "squeue.state")
            .collect();
        assert_eq!(state_ev.len(), 1);
        assert_eq!(state_ev[0].value, "RUNNING");

        // Summary should still be populated
        assert!(!report.summary.is_empty());
    }

    // Behavioral tests for investigate_node

    #[test]
    fn test_investigate_node_idle() {
        let mut fake = FakeCommands::new();
        let node_name = "node01";

        // scontrol show node
        fake.add(
            "scontrol show node",
            &format!(
                "NodeName={} State=IDLE CPUAlloc=0 CPUTot=16 \
                 Partitions=gpu RealMemory=128000 FreeMem=120000 \
                 CPULoad=0.05 Gres=gpu:4 AllocTRES= Reason=none",
                node_name
            ),
        );

        // sinfo
        fake.add(
            "sinfo --noheader -N -o",
            "node01|idle|gpu|16|0/16/0/16|128000|120000|0.05|gpu:4\n",
        );

        // sinfo gres
        fake.add("sinfo --noheader -N -o '%N %G'", "node01 gpu:4(S:0)\n");

        // squeue -w (no jobs)
        fake.add("squeue --noheader -w", "");

        let report = investigate_node(&fake, node_name, 20);

        assert_eq!(report.target.kind, "node");
        assert_eq!(report.target.identifier, node_name);
        assert!(report.errors.is_empty());

        // Should have state evidence
        let state_ev: Vec<_> = report
            .evidence
            .iter()
            .filter(|e| e.id == "sinfo.state")
            .collect();
        assert_eq!(state_ev.len(), 1);
        assert_eq!(state_ev[0].value, "idle");

        // Should have explanation about idle state
        assert!(!report.explanations.is_empty());

        // Should have "no jobs" explanation
        let no_jobs_exp: Vec<_> = report
            .explanations
            .iter()
            .filter(|exp| exp.title.contains("No jobs"))
            .collect();
        assert_eq!(no_jobs_exp.len(), 1);
    }

    #[test]
    fn test_investigate_node_allocated_with_jobs() {
        let mut fake = FakeCommands::new();
        let node_name = "node02";

        // scontrol show node
        fake.add(
            "scontrol show node",
            &format!(
                "NodeName={} State=ALLOCATED CPUAlloc=16 CPUTot=16 \
                 Partitions=gpu RealMemory=128000 FreeMem=32000 \
                 CPULoad=15.2 Gres=gpu:4 AllocTRES=cpu=16,mem=96000M,gres/gpu=4 \
                 Reason=none",
                node_name
            ),
        );

        // sinfo
        fake.add(
            "sinfo --noheader -N -o",
            "node02|allocated|gpu|16|16/0/0/16|128000|32000|15.2|gpu:4\n",
        );

        // sinfo gres
        fake.add("sinfo --noheader -N -o '%N %G'", "node02 gpu:4(S:0-3)\n");

        // squeue -w (2 jobs)
        fake.add(
            "squeue --noheader -w",
            "100|job1|user1|RUNNING|gpu|1|8|1:00:00|4:00:00|None|node02|normal\n\
             101|job2|user2|RUNNING|gpu|1|8|0:30:00|2:00:00|None|node02|normal\n",
        );

        let report = investigate_node(&fake, node_name, 20);

        assert!(report.errors.is_empty());

        // Should have 2 related jobs
        assert_eq!(report.related_jobs.len(), 2);
        assert_eq!(report.related_jobs[0].job_id, "100");
        assert_eq!(report.related_jobs[1].job_id, "101");
    }

    #[test]
    fn test_investigate_node_drain() {
        let mut fake = FakeCommands::new();
        let node_name = "node03";

        // scontrol show node
        fake.add(
            "scontrol show node",
            &format!(
                "NodeName={} State=IDLE+DRAIN CPUAlloc=0 CPUTot=16 \
                 Partitions=gpu RealMemory=128000 FreeMem=120000 \
                 CPULoad=0.00 Gres=gpu:4 AllocTRES= \
                 Reason=maintenance[root@2026-05-08T08:00:00]",
                node_name
            ),
        );

        // sinfo
        fake.add(
            "sinfo --noheader -N -o",
            "node03|idle+drain|gpu|16|0/16/0/16|128000|120000|0.00|gpu:4\n",
        );

        // sinfo gres
        fake.add("sinfo --noheader -N -o '%N %G'", "node03 gpu:4(S:0)\n");

        // squeue -w
        fake.add("squeue --noheader -w", "");

        let report = investigate_node(&fake, node_name, 20);

        assert!(report.errors.is_empty());

        // Should have Reason in evidence
        let reason_ev: Vec<_> = report
            .evidence
            .iter()
            .filter(|e| e.id == "scontrol.Reason")
            .collect();
        assert_eq!(reason_ev.len(), 1);
        assert!(reason_ev[0].value.contains("maintenance"));

        // Explanation should mention drain
        let exp_text = report
            .explanations
            .iter()
            .map(|e| format!("{} {}", e.title, e.detail).to_lowercase())
            .collect::<Vec<_>>()
            .join(" ");
        assert!(exp_text.contains("drain"));
    }

    #[test]
    fn test_investigate_node_caps_related_jobs_at_limit() {
        let mut fake = FakeCommands::new();
        let node_name = "node04";

        // scontrol show node
        fake.add(
            "scontrol show node",
            "NodeName=node04 State=ALLOCATED CPUAlloc=16 CPUTot=16 \
             Partitions=gpu RealMemory=128000 FreeMem=32000 \
             CPULoad=15.5 Gres=gpu:4 AllocTRES=cpu=16 Reason=none",
        );

        // sinfo
        fake.add(
            "sinfo --noheader -N -o",
            "node04|allocated|gpu|16|16/0/0/16|128000|32000|15.5|gpu:4\n",
        );

        // sinfo gres
        fake.add("sinfo --noheader -N -o '%N %G'", "node04 gpu:4(S:0-3)\n");

        // squeue -w (10 jobs)
        let mut jobs = String::new();
        for i in 1..=10 {
            jobs.push_str(&format!(
                "{}|job{}|user|RUNNING|gpu|1|2|1:00:00|4:00:00|None|node04|normal\n",
                i, i
            ));
        }
        fake.add("squeue --noheader -w", &jobs);

        // Cap at 5
        let report = investigate_node(&fake, node_name, 5);

        assert_eq!(report.related_jobs.len(), 5);
    }

    #[test]
    fn test_investigate_node_zero_cap_disables_limit() {
        let mut fake = FakeCommands::new();
        let node_name = "node05";

        fake.add(
            "scontrol show node",
            "NodeName=node05 State=ALLOCATED CPUAlloc=8 CPUTot=16 \
             Partitions=gpu RealMemory=128000 FreeMem=64000 \
             CPULoad=8.0 Gres=gpu:2 AllocTRES=cpu=8 Reason=none",
        );

        fake.add(
            "sinfo --noheader -N -o",
            "node05|allocated|gpu|16|8/8/0/16|128000|64000|8.0|gpu:2\n",
        );

        fake.add("sinfo --noheader -N -o '%N %G'", "node05 gpu:2(S:0-1)\n");

        fake.add(
            "squeue --noheader -w",
            "200|job1|user|RUNNING|gpu|1|4|0:30:00|2:00:00|None|node05|normal\n",
        );

        // max_related_jobs = 0 should yield no jobs
        let report = investigate_node(&fake, node_name, 0);

        assert!(report.related_jobs.is_empty());
    }

    #[test]
    fn test_investigate_node_scontrol_failure_partial_report() {
        let mut fake = FakeCommands::new();
        let node_name = "node06";

        // scontrol fails
        fake.add_fail("scontrol show node", "node not found");

        // sinfo succeeds
        fake.add(
            "sinfo --noheader -N -o",
            "node06|idle|cpu|8|0/8/0/8|32000|30000|0.1|(null)\n",
        );

        // sinfo gres
        fake.add("sinfo --noheader -N -o '%N %G'", "node06 (null)\n");

        // squeue -w
        fake.add("squeue --noheader -w", "");

        let report = investigate_node(&fake, node_name, 20);

        // Should have error about scontrol
        assert_eq!(report.errors.len(), 1);
        assert_eq!(report.errors[0].source, "scontrol");
        assert!(report.errors[0]
            .stderr
            .as_ref()
            .unwrap()
            .contains("not found"));

        // But should still have sinfo evidence
        let state_ev: Vec<_> = report
            .evidence
            .iter()
            .filter(|e| e.id == "sinfo.state")
            .collect();
        assert_eq!(state_ev.len(), 1);
        assert_eq!(state_ev[0].value, "idle");

        // Summary should still be populated
        assert!(!report.summary.is_empty());
    }
}

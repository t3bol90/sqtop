"""Slurm data fetching — wraps squeue, sinfo, scontrol commands."""

from __future__ import annotations

import subprocess
import shlex
from dataclasses import dataclass, field
from typing import Any


def _run(cmd: str) -> str:
    """Run a shell command and return stdout, or empty string on error."""
    try:
        result = subprocess.run(
            shlex.split(cmd),
            capture_output=True,
            text=True,
            timeout=10,
        )
        return result.stdout
    except (subprocess.TimeoutExpired, FileNotFoundError):
        return ""


# ---------------------------------------------------------------------------
# Jobs (squeue)
# ---------------------------------------------------------------------------

@dataclass
class Job:
    job_id: str
    name: str
    user: str
    state: str        # RUNNING, PENDING, COMPLETED, FAILED, ...
    partition: str
    nodes: str
    num_nodes: str
    num_cpus: str
    time_used: str
    time_limit: str
    reason: str = ""
    nodelist: str = ""


def fetch_jobs() -> list[Job]:
    """Return jobs from squeue -o with parseable format."""
    fmt = "%i|%j|%u|%T|%P|%D|%C|%M|%l|%R|%N"
    out = _run(f"squeue --noheader -o '{fmt}'")
    jobs = []
    for line in out.strip().splitlines():
        parts = line.split("|")
        if len(parts) < 11:
            continue
        jobs.append(Job(
            job_id=parts[0],
            name=parts[1],
            user=parts[2],
            state=parts[3],
            partition=parts[4],
            nodes=parts[5],
            num_cpus=parts[6],
            time_used=parts[7],
            time_limit=parts[8],
            reason=parts[9],
            nodelist=parts[10],
            num_nodes=parts[5],
        ))
    return jobs


# ---------------------------------------------------------------------------
# Nodes (sinfo)
# ---------------------------------------------------------------------------

@dataclass
class Node:
    name: str
    state: str       # idle, allocated, mixed, down, drain, ...
    partition: str
    cpus_total: str
    cpus_alloc: str
    memory_total: str
    memory_free: str
    load: str = "N/A"


def fetch_nodes() -> list[Node]:
    """Return node info from sinfo."""
    fmt = "%n|%T|%P|%c|%C|%m|%e|%O"
    out = _run(f"sinfo --noheader -o '{fmt}'")
    nodes = []
    for line in out.strip().splitlines():
        parts = line.split("|")
        if len(parts) < 8:
            continue
        # %C = allocated/idle/other/total  e.g. "2/6/0/8"
        cpu_parts = parts[4].split("/")
        nodes.append(Node(
            name=parts[0],
            state=parts[1],
            partition=parts[2],
            cpus_total=cpu_parts[3] if len(cpu_parts) == 4 else parts[3],
            cpus_alloc=cpu_parts[0] if len(cpu_parts) == 4 else "?",
            memory_total=parts[5],
            memory_free=parts[6],
            load=parts[7],
        ))
    return nodes


# ---------------------------------------------------------------------------
# Cluster summary (sinfo -s)
# ---------------------------------------------------------------------------

@dataclass
class ClusterSummary:
    partition: str
    avail: str
    timelimit: str
    nodes: str
    state: str
    nodelist: str


def fetch_cluster_summary() -> list[ClusterSummary]:
    fmt = "%P|%a|%l|%D|%T|%N"
    out = _run(f"sinfo --noheader -o '{fmt}'")
    summaries = []
    for line in out.strip().splitlines():
        parts = line.split("|")
        if len(parts) < 6:
            continue
        summaries.append(ClusterSummary(*parts[:6]))
    return summaries


# ---------------------------------------------------------------------------
# scontrol show job <id>
# ---------------------------------------------------------------------------

def fetch_job_detail(job_id: str) -> dict[str, str]:
    """Return key=value pairs from scontrol show job <id>."""
    out = _run(f"scontrol show job {job_id}")
    result: dict[str, str] = {}
    for token in out.split():
        if "=" in token:
            k, _, v = token.partition("=")
            result[k] = v
    return result


def fetch_node_detail(node_name: str) -> dict[str, str]:
    """Return key=value pairs from scontrol show node <name>."""
    out = _run(f"scontrol show node {node_name}")
    result: dict[str, str] = {}
    for token in out.split():
        if "=" in token:
            k, _, v = token.partition("=")
            result[k] = v
    return result


def fetch_log_paths(job_id: str) -> tuple[str, str]:
    """Return (stdout_path, stderr_path) from scontrol show job."""
    detail = fetch_job_detail(job_id)
    stdout = detail.get("StdOut", "")
    stderr = detail.get("StdErr", "")
    return stdout, stderr


def tail_log_file(path: str, n: int = 200) -> str:
    """Return last n lines of a log file inside the slurmctld container."""
    if not path:
        return "(no log path)"
    result = _run(f"tail -n {n} {shlex.quote(path)}")
    return result if result else "(empty or file not found)"

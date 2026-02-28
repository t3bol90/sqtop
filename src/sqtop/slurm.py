"""Slurm data fetching — wraps squeue, sinfo, scontrol commands."""

from __future__ import annotations

import re
import subprocess
import shlex
from dataclasses import dataclass
from collections import deque
from time import monotonic


@dataclass
class CommandStat:
    command: str
    ok: bool
    latency_ms: int
    stderr: str = ""


@dataclass
class ActionResult:
    job_id: str
    action: str
    ok: bool
    message: str = ""


_COMMAND_HISTORY: deque[CommandStat] = deque(maxlen=300)


def _record_command(command: str, ok: bool, latency_ms: int, stderr: str = "") -> None:
    _COMMAND_HISTORY.append(CommandStat(command=command, ok=ok, latency_ms=latency_ms, stderr=stderr))


def _run_result(cmd: str) -> tuple[str, bool, str]:
    """Run command and return (stdout, ok, stderr)."""
    start = monotonic()
    try:
        result = subprocess.run(
            shlex.split(cmd),
            capture_output=True,
            text=True,
            timeout=10,
        )
        ok = result.returncode == 0
        _record_command(
            cmd,
            ok=ok,
            latency_ms=int((monotonic() - start) * 1000),
            stderr=(result.stderr or "").strip(),
        )
        return result.stdout, ok, (result.stderr or "").strip()
    except (subprocess.TimeoutExpired, FileNotFoundError):
        _record_command(
            cmd,
            ok=False,
            latency_ms=int((monotonic() - start) * 1000),
            stderr="timeout or command not found",
        )
        return "", False, "timeout or command not found"


def _run(cmd: str) -> str:
    out, _, _ = _run_result(cmd)
    return out


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

def _parse_gpu_count(gres_str: str) -> int:
    """Extract GPU count from strings like 'gpu:4', 'gpu:a100:4', 'gpu:a100:4(IDX:0,1)'."""
    m = re.search(r'\bgpu:(?:[^:,()\s]+:)?(\d+)', gres_str)
    return int(m.group(1)) if m else 0


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
    gpu_total: int = 0
    gpu_alloc: int = 0


def _fetch_gpus_alloc() -> dict[str, int]:
    """Return {node_name: gpus_allocated} from scontrol show nodes.

    Reads AllocTRES (present in Slurm 24.x) and falls back to GresUsed
    (older Slurm versions) so both are handled.
    """
    out = _run("scontrol show nodes")
    result: dict[str, int] = {}
    node_name = ""
    for token in out.split():
        if token.startswith("NodeName="):
            node_name = token.partition("=")[2]
        elif token.startswith("AllocTRES=") and node_name:
            # AllocTRES=cpu=64,mem=256G,gres/gpu=8
            # gres/gpu= (no colon) is the bare aggregate count
            m = re.search(r'gres/gpu=(\d+)', token.partition("=")[2])
            if m:
                result[node_name] = int(m.group(1))
        elif token.startswith("GresUsed=") and node_name:
            # Older Slurm: GresUsed=gpu:a100:4(IDX:0,1,2,3)
            if node_name not in result:
                result[node_name] = _parse_gpu_count(token.partition("=")[2])
    return result


def fetch_nodes() -> list[Node]:
    """Return node info from sinfo."""
    fmt = "%n|%T|%P|%c|%C|%m|%e|%O|%G"
    out = _run(f"sinfo --noheader -o '{fmt}'")
    gpus_alloc = _fetch_gpus_alloc()
    nodes = []
    for line in out.strip().splitlines():
        parts = line.split("|")
        if len(parts) < 9:
            continue
        # %C = allocated/idle/other/total  e.g. "2/6/0/8"
        cpu_parts = parts[4].split("/")
        gpu_total = _parse_gpu_count(parts[8])
        name = parts[0]
        nodes.append(Node(
            name=name,
            state=parts[1],
            partition=parts[2],
            cpus_total=cpu_parts[3] if len(cpu_parts) == 4 else parts[3],
            cpus_alloc=cpu_parts[0] if len(cpu_parts) == 4 else "?",
            memory_total=parts[5],
            memory_free=parts[6],
            load=parts[7],
            gpu_total=gpu_total,
            gpu_alloc=gpus_alloc.get(name, 0) if gpu_total > 0 else 0,
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


def cancel_job(job_id: str) -> bool:
    """Run scancel <job_id>. Returns True if command succeeded."""
    ok, _ = cancel_job_result(job_id)
    return ok


def cancel_job_result(job_id: str) -> tuple[bool, str]:
    """Run scancel and return (ok, stderr)."""
    _, ok, stderr = _run_result(f"scancel {shlex.quote(job_id)}")
    return ok, stderr


def hold_job_result(job_id: str) -> tuple[bool, str]:
    """Run scontrol hold and return (ok, stderr)."""
    _, ok, stderr = _run_result(f"scontrol hold {shlex.quote(job_id)}")
    return ok, stderr


def release_job_result(job_id: str) -> tuple[bool, str]:
    """Run scontrol release and return (ok, stderr)."""
    _, ok, stderr = _run_result(f"scontrol release {shlex.quote(job_id)}")
    return ok, stderr


def requeue_job_result(job_id: str) -> tuple[bool, str]:
    """Run scontrol requeue and return (ok, stderr)."""
    _, ok, stderr = _run_result(f"scontrol requeue {shlex.quote(job_id)}")
    return ok, stderr


def tail_log_file(path: str, n: int = 200) -> str:
    """Return last n lines of a log file inside the slurmctld container."""
    if not path:
        return "(no log path)"
    result = _run(f"tail -n {n} {shlex.quote(path)}")
    return result if result else "(empty or file not found)"


def resolve_first_node(nodelist_expr: str) -> str:
    """Resolve the first node hostname from a Slurm NodeList expression."""
    expr = (nodelist_expr or "").strip()
    if not expr or expr == "(null)":
        return ""

    out = _run(f"scontrol show hostnames {shlex.quote(expr)}")
    for line in out.splitlines():
        host = line.strip()
        if host:
            return host

    # Conservative fallback for unresolved compressed expressions.
    return expr.split(",", 1)[0].strip()


def build_attach_command(
    job_id: str,
    node: str | None,
    default_command: str,
    extra_args: str = "",
) -> list[str]:
    """Build interactive attach command for a running Slurm job."""
    cmd = ["srun", "--pty", "--overlap"]
    if extra_args.strip():
        cmd.extend(shlex.split(extra_args))
    cmd.extend(["--jobid", str(job_id)])
    if node and node.strip():
        cmd.extend(["-w", node.strip()])
    cmd.extend(shlex.split(default_command))
    return cmd


def run_attach_command(cmd: list[str]) -> int:
    """Run interactive attach command against the controlling terminal."""
    start = monotonic()
    try:
        with open("/dev/tty", "rb+", buffering=0) as tty:
            result = subprocess.run(cmd, stdin=tty, stdout=tty, stderr=tty)
    except OSError:
        # Fallback for environments without /dev/tty.
        result = subprocess.run(cmd)
    _record_command(
        " ".join(cmd),
        ok=(result.returncode == 0),
        latency_ms=int((monotonic() - start) * 1000),
        stderr="" if result.returncode == 0 else f"exit {result.returncode}",
    )
    return result.returncode


def run_job_action(action: str, job_id: str) -> ActionResult:
    """Execute a per-job action with normalized result message."""
    action = action.lower()
    if action == "cancel":
        ok, err = cancel_job_result(job_id)
    elif action == "hold":
        ok, err = hold_job_result(job_id)
    elif action == "release":
        ok, err = release_job_result(job_id)
    elif action == "requeue":
        ok, err = requeue_job_result(job_id)
    else:
        return ActionResult(job_id=job_id, action=action, ok=False, message="unsupported action")
    return ActionResult(job_id=job_id, action=action, ok=ok, message=err or ("ok" if ok else "failed"))


def run_bulk_job_action(action: str, job_ids: list[str]) -> list[ActionResult]:
    return [run_job_action(action, job_id) for job_id in job_ids]


def fetch_command_health(limit: int = 100) -> list[CommandStat]:
    if limit <= 0:
        return []
    return list(_COMMAND_HISTORY)[-limit:]

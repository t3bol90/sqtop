"""Slurm data fetching — wraps squeue, sinfo, scontrol commands."""

from __future__ import annotations

import re
import subprocess
import shlex
from concurrent.futures import ThreadPoolExecutor
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
        if _SSH_HOST:
            ssh = ["ssh", "-q", "-o", "BatchMode=yes", "-o", "ConnectTimeout=8"]
            if _SSH_KEY:
                ssh += ["-i", _SSH_KEY]
            cmd_list = ssh + [_SSH_HOST, cmd]  # cmd as single string → remote shell parses it
        else:
            cmd_list = shlex.split(cmd)
        result = subprocess.run(
            cmd_list,
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
    except (subprocess.TimeoutExpired, FileNotFoundError, OSError):
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
    qos: str = ""


def fetch_jobs() -> list[Job]:
    """Return jobs from squeue -o with parseable format."""
    fmt = "%i|%j|%u|%T|%P|%D|%C|%M|%l|%R|%N|%q"
    out = _run(f"squeue --noheader -o '{fmt}'")
    jobs = []
    for line in out.strip().splitlines():
        parts = line.split("|")
        if len(parts) < 11:
            continue
        qos_raw = parts[11] if len(parts) > 11 else ""
        qos = "" if qos_raw in ("N/A", "(null)") else qos_raw
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
            qos=qos,
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
    with ThreadPoolExecutor(max_workers=2) as pool:
        f_sinfo = pool.submit(_run, f"sinfo --noheader -o '{fmt}'")
        f_gpus  = pool.submit(_fetch_gpus_alloc)
    out = f_sinfo.result()
    gpus_alloc = f_gpus.result()
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
            cpus_total=cpu_parts[3] if len(cpu_parts) == 4 else "?",
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


def _parse_slurm_duration(s: str) -> int:
    """Parse Slurm HH:MM:SS (or D-HH:MM:SS) duration string to total seconds."""
    s = s.strip()
    if not s or s == "0":
        return 0
    days = 0
    if "-" in s:
        day_part, s = s.split("-", 1)
        try:
            days = int(day_part)
        except ValueError:
            return 0
    parts = s.split(":")
    try:
        if len(parts) == 3:
            return days * 86400 + int(parts[0]) * 3600 + int(parts[1]) * 60 + int(parts[2])
        elif len(parts) == 2:
            return days * 86400 + int(parts[0]) * 60 + int(parts[1])
        elif len(parts) == 1:
            return days * 86400 + int(parts[0])
    except ValueError:
        return 0
    return 0


def fetch_job_efficiency(job_id: str) -> dict:
    """Fetch CPU and memory efficiency metrics via sacct.

    Returns dict with keys:
      - cpu_eff: float 0.0-1.0 (TotalCPU / CPUTimeRAW)
      - mem_eff: float 0.0-1.0 (MaxRSS / AllocMem)
      - cpu_used_str: str like "3:12:00"
      - cpu_alloc_str: str like "5:10:00"
      - mem_peak_mb: int
      - mem_alloc_mb: int
      - available: bool (False if sacct not found or parse error)

    Command: sacct -j <job_id> --parsable2 --noheader
             -o CPUTimeRAW,TotalCPU,AllocMem,MaxRSS
    """
    _unavailable: dict = {"available": False}
    cmd = f"sacct -j {shlex.quote(job_id)} --parsable2 --noheader -o CPUTimeRAW,TotalCPU,AllocMem,MaxRSS"
    out, ok, _ = _run_result(cmd)
    if not ok or not out.strip():
        return _unavailable
    try:
        # Use the first non-step line (no dot in the job_id column, i.e. no "12345.batch")
        target_line = ""
        for line in out.strip().splitlines():
            parts = line.split("|")
            if len(parts) < 4:
                continue
            target_line = line
            break
        if not target_line:
            return _unavailable
        parts = target_line.split("|")
        cpu_time_raw_str = parts[0].strip()   # seconds as integer
        total_cpu_str = parts[1].strip()       # HH:MM:SS
        alloc_mem_str = parts[2].strip()       # MB (may have 'M' suffix) or KB
        max_rss_str = parts[3].strip()         # KB (may be 0)

        cpu_time_raw = int(cpu_time_raw_str) if cpu_time_raw_str.isdigit() else 0
        total_cpu_secs = _parse_slurm_duration(total_cpu_str)

        # Parse AllocMem: may be "2000M", "2048K", or bare integer (MB)
        alloc_mem_mb = 0
        if alloc_mem_str.endswith("M") or alloc_mem_str.endswith("m"):
            alloc_mem_mb = int(alloc_mem_str[:-1])
        elif alloc_mem_str.endswith("K") or alloc_mem_str.endswith("k"):
            alloc_mem_mb = int(alloc_mem_str[:-1]) // 1024
        elif alloc_mem_str.isdigit():
            alloc_mem_mb = int(alloc_mem_str)

        # MaxRSS is in KB
        max_rss_mb = 0
        if max_rss_str.endswith("K") or max_rss_str.endswith("k"):
            max_rss_mb = int(max_rss_str[:-1]) // 1024
        elif max_rss_str.endswith("M") or max_rss_str.endswith("m"):
            max_rss_mb = int(max_rss_str[:-1])
        elif max_rss_str.isdigit():
            max_rss_mb = int(max_rss_str) // 1024

        cpu_eff = (total_cpu_secs / cpu_time_raw) if cpu_time_raw > 0 else 0.0
        mem_eff = (max_rss_mb / alloc_mem_mb) if alloc_mem_mb > 0 else 0.0

        # Build human-readable cpu_alloc_str from CPUTimeRAW seconds
        h, rem = divmod(cpu_time_raw, 3600)
        m, s = divmod(rem, 60)
        cpu_alloc_str = f"{h}:{m:02d}:{s:02d}"

        return {
            "available": True,
            "cpu_eff": min(cpu_eff, 1.0),
            "mem_eff": min(mem_eff, 1.0),
            "cpu_used_str": total_cpu_str,
            "cpu_alloc_str": cpu_alloc_str,
            "mem_peak_mb": max_rss_mb,
            "mem_alloc_mb": alloc_mem_mb,
        }
    except (ValueError, IndexError, ZeroDivisionError):
        return _unavailable


# ---------------------------------------------------------------------------
# Job array tasks
# ---------------------------------------------------------------------------

def fetch_array_tasks(job_id: str) -> list[Job]:
    """Fetch individual tasks for a job array via squeue -j <job_id>."""
    fmt = "%i|%j|%u|%T|%P|%D|%C|%M|%l|%R|%N"
    out = _run(f"squeue --noheader -j {shlex.quote(job_id)} -o '{fmt}'")
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
# Job dependencies
# ---------------------------------------------------------------------------

@dataclass
class JobDependency:
    dep_type: str   # "afterok", "afterany", "after", etc.
    job_id: str
    state: str      # fetched from squeue, or "COMPLETED" if not in queue


def fetch_job_dependencies(job_id: str) -> list[JobDependency]:
    """Parse Dependency= from scontrol show job. Non-recursive (immediate deps only)."""
    dep_str = fetch_job_detail(job_id).get("Dependency", "")
    if not dep_str or dep_str.lower() in {"none", "(null)"}:
        return []
    deps = []
    for token in dep_str.split(","):
        if ":" not in token:
            continue  # handles "singleton"
        dep_type, _, rest = token.partition(":")
        for jid_raw in rest.split(":"):
            jid = jid_raw.split("(")[0].strip()
            if jid.isdigit():
                deps.append(JobDependency(dep_type=dep_type.strip(), job_id=jid, state=""))
    if not deps:
        return []
    # Batch fetch states with one squeue call
    ids_csv = ",".join(d.job_id for d in deps)
    out = _run(f"squeue --noheader -j {shlex.quote(ids_csv)} -o '%i|%T'")
    state_map = {p[0]: p[1] for line in out.splitlines() if len((p := line.split("|"))) >= 2}
    for d in deps:
        d.state = state_map.get(d.job_id, "COMPLETED")  # absent = likely completed
    return deps


# ---------------------------------------------------------------------------
# Completed jobs (sacct)
# ---------------------------------------------------------------------------

@dataclass
class SacctJob:
    job_id: str
    name: str
    user: str
    state: str
    num_cpus: str
    elapsed: str
    exit_code: str
    partition: str


def fetch_sacct_jobs(hours: int = 24) -> list[SacctJob]:
    """Fetch completed jobs from sacct for the last N hours."""
    cmd = (
        f"sacct --noheader --parsable2 -S now-{hours}hours"
        " -o JobID,JobName,User,State,AllocCPUS,Elapsed,ExitCode,Partition"
    )
    try:
        out, ok, _ = _run_result(cmd)
    except FileNotFoundError:
        return []
    if not ok:
        return []
    jobs = []
    for line in out.strip().splitlines():
        parts = line.split("|")
        if len(parts) < 8:
            continue
        job_id = parts[0]
        # Skip step lines: job IDs containing '.' are steps (e.g. 12345.batch)
        if "." in job_id:
            continue
        jobs.append(SacctJob(
            job_id=job_id,
            name=parts[1],
            user=parts[2],
            state=parts[3],
            num_cpus=parts[4],
            elapsed=parts[5],
            exit_code=parts[6],
            partition=parts[7],
        ))
    return jobs


# ---------------------------------------------------------------------------
# SSH remote support
# ---------------------------------------------------------------------------

_SSH_HOST: str | None = None
_SSH_KEY: str | None = None


def set_remote(host: str, key: str = "") -> None:
    global _SSH_HOST, _SSH_KEY
    _SSH_HOST = host.strip() or None
    _SSH_KEY = key.strip() or None

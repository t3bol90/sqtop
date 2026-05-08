"""Slurm data fetching — wraps squeue, sinfo, scontrol commands."""

from __future__ import annotations

import re
import subprocess
import shlex
from concurrent.futures import ThreadPoolExecutor
from dataclasses import dataclass
from collections import deque
from datetime import datetime
from time import monotonic


@dataclass
class CommandStat:
    command: str
    ok: bool
    latency_ms: int
    stderr: str = ""
    error_category: str | None = None


@dataclass
class ActionResult:
    job_id: str
    action: str
    ok: bool
    message: str = ""


# Normalized error categories that classify_error() may return. SPEC §10.1
# enumerates additional categories (clipboard_unavailable, accounting_unavailable,
# etc.) that belong to other layers and are intentionally out of scope here.
ERROR_CATEGORIES = frozenset({
    "slurm_command_not_found",
    "slurm_command_timeout",
    "slurm_command_failed",
    "slurm_permission_denied",
    "slurm_field_unavailable",
    "ssh_connection_failed",
    "ssh_auth_failed",
    "ssh_command_timeout",
    "job_not_found",
    "node_not_found",
})


def classify_error(returncode: int | None, stderr: str) -> str | None:
    """Map a (returncode, stderr) pair to a normalized error category.

    Pure function: no subprocess, no SSH, no I/O. See SPEC §10.1.

    Returns:
        - None on success (returncode == 0).
        - A category from ERROR_CATEGORIES otherwise.
    """
    if returncode == 0:
        return None

    text = (stderr or "").lower()

    # Exception path: _run_result() collapses TimeoutExpired/FileNotFoundError/
    # OSError into returncode=None with a distinguishing stderr substring.
    if returncode is None:
        if "timeout" in text:
            return "slurm_command_timeout"
        if "not found" in text:
            return "slurm_command_not_found"
        return "slurm_command_failed"

    # Non-zero returncode: inspect stderr in priority order. Match SSH publickey
    # auth failure BEFORE the generic "permission denied" check so it is
    # disambiguated correctly.
    if "publickey" in text or "authentication failed" in text:
        return "ssh_auth_failed"
    if "permission denied" in text or "unauthorized" in text or "not allowed" in text:
        return "slurm_permission_denied"
    if "connection refused" in text or "could not resolve hostname" in text or "connection closed" in text:
        return "ssh_connection_failed"
    if "timeout" in text:
        return "slurm_command_timeout"
    if "invalid job id" in text or "job not found" in text or "unknown job" in text:
        return "job_not_found"
    if "invalid node" in text or "node not found" in text or "unknown node" in text:
        return "node_not_found"
    return "slurm_command_failed"


_COMMAND_HISTORY: deque[CommandStat] = deque(maxlen=300)


def _record_command(
    command: str,
    ok: bool,
    latency_ms: int,
    stderr: str = "",
    error_category: str | None = None,
) -> None:
    _COMMAND_HISTORY.append(CommandStat(
        command=command,
        ok=ok,
        latency_ms=latency_ms,
        stderr=stderr,
        error_category=error_category,
    ))


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
        stderr_text = (result.stderr or "").strip()
        _record_command(
            cmd,
            ok=ok,
            latency_ms=int((monotonic() - start) * 1000),
            stderr=stderr_text,
            error_category=classify_error(result.returncode, stderr_text),
        )
        return result.stdout, ok, stderr_text
    except subprocess.TimeoutExpired:
        stderr_text = "timeout"
        _record_command(
            cmd,
            ok=False,
            latency_ms=int((monotonic() - start) * 1000),
            stderr=stderr_text,
            error_category=classify_error(None, stderr_text),
        )
        return "", False, stderr_text
    except FileNotFoundError:
        stderr_text = "command not found"
        _record_command(
            cmd,
            ok=False,
            latency_ms=int((monotonic() - start) * 1000),
            stderr=stderr_text,
            error_category=classify_error(None, stderr_text),
        )
        return "", False, stderr_text
    except OSError as exc:
        stderr_text = f"OS error: {exc}"
        _record_command(
            cmd,
            ok=False,
            latency_ms=int((monotonic() - start) * 1000),
            stderr=stderr_text,
            error_category=classify_error(None, stderr_text),
        )
        return "", False, stderr_text


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


# Shared squeue format used by both fetch_jobs() and fetch_jobs_on_node().
# Field count is fixed at 12: any change here MUST be matched in
# _parse_squeue_row()'s minimum-field guard.
_SQUEUE_FMT = "%i|%j|%u|%T|%P|%D|%C|%M|%l|%R|%N|%q"


def _parse_squeue_row(line: str) -> Job | None:
    """Parse one squeue row produced with _SQUEUE_FMT into a Job.

    Returns None for malformed rows (fewer than 12 pipe-separated fields)
    so callers can keep going on partial output. SPEC sec. 9.4 requires
    parsers to tolerate missing/extra fields without crashing.
    """
    parts = line.split("|")
    if len(parts) < 12:
        return None
    qos_raw = parts[11]
    qos = "" if qos_raw in ("N/A", "(null)") else qos_raw
    return Job(
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
    )


def fetch_jobs() -> list[Job]:
    """Return jobs from squeue -o with parseable format."""
    out = _run(f"squeue --noheader -o '{_SQUEUE_FMT}'")
    jobs: list[Job] = []
    for line in out.strip().splitlines():
        job = _parse_squeue_row(line)
        if job is not None:
            jobs.append(job)
    return jobs


def fetch_jobs_on_node(node_name: str) -> list[Job]:
    """Return jobs currently visible on a specific node via squeue -w.

    Returns [] without invoking any command when ``node_name`` is empty
    or whitespace. Failures are logged via ``_run_result`` (through
    ``_run``) and surface as an empty list.
    """
    name = (node_name or "").strip()
    if not name:
        return []
    out = _run(f"squeue --noheader -w {shlex.quote(name)} -o '{_SQUEUE_FMT}'")
    jobs: list[Job] = []
    for line in out.strip().splitlines():
        job = _parse_squeue_row(line)
        if job is not None:
            jobs.append(job)
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


def fetch_batch_script(job_id: str) -> str:
    """Return the batch script for job_id, or an error message."""
    out, ok, err = _run_result(f"scontrol write batch_script {shlex.quote(job_id)} -")
    if not ok:
        return f"(error: {err or 'permission denied or job not found'})"
    return out or "(empty script)"


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
# Investigation Mode (SPEC sec. 8.4, 9.3, 10.3)
# ---------------------------------------------------------------------------

# State token sets used to gate which suggested actions are surfaced.
# Kept as module-level frozensets so investigate_job() does no per-call
# allocation when classifying state.
_PENDING_STATES = frozenset({"PENDING", "PD"})
_RUNNING_STATES = frozenset({"RUNNING", "R"})
_TERMINAL_STATES = frozenset({
    "COMPLETED", "CD",
    "FAILED", "F",
    "CANCELLED", "CA",
    "TIMEOUT", "TO",
    "NODE_FAIL", "NF",
    "PREEMPTED", "PR",
    "OUT_OF_MEMORY", "OOM",
})

# Slurm sentinels we treat as "not provided".
_NULL_SENTINELS = frozenset({"", "(null)", "N/A", "None", "none"})


def _present(value: str | None) -> bool:
    """True if ``value`` carries real Slurm content (not a null sentinel)."""
    if value is None:
        return False
    return value.strip() not in _NULL_SENTINELS


def _display(value: str | None) -> str:
    """Format ``value`` for InvestigationItem display, mapping nulls."""
    if not _present(value):
        return "(unavailable)"
    # Defensive: _present already proved value is a real string.
    return value.strip()  # type: ignore[union-attr]


def investigate_job(job_id: str):
    """Build an InvestigationReport for a single job.

    SPEC sec. 8.4 / 9.3 / 10.3. Tolerant of partial failure: scontrol
    unavailable, job_id absent from the live squeue snapshot, or
    dependency parse errors must NOT raise. A report with errors is
    always preferable to no report.
    """
    # Local import: investigation imports Job/Node from slurm, so we keep
    # the dependency one-way at module load by deferring this import.
    from .investigation import (
        InvestigationAction,
        InvestigationError,
        InvestigationEvidence,
        InvestigationExplanation,
        InvestigationItem,
        InvestigationReport,
        InvestigationTarget,
        explain_pending_reason,
    )

    target = InvestigationTarget(kind="job", identifier=job_id, source="typed")
    report = InvestigationReport(target=target, generated_at=datetime.now())

    # ---- scontrol show job ------------------------------------------------
    scontrol_out, scontrol_ok, scontrol_err = _run_result(
        f"scontrol show job {shlex.quote(job_id)}"
    )
    detail: dict[str, str] = {}
    if scontrol_ok:
        for token in scontrol_out.split():
            if "=" in token:
                k, _, v = token.partition("=")
                detail[k] = v
        report.raw_sections["scontrol show job"] = "available"
    else:
        report.raw_sections["scontrol show job"] = "unavailable"
        report.errors.append(InvestigationError(
            source="scontrol",
            category=classify_error(1, scontrol_err) or "slurm_command_failed",
            message=f"scontrol show job {job_id} failed",
            stderr=scontrol_err or None,
        ))

    # ---- live squeue row --------------------------------------------------
    # Cache once; SPEC requires we never call fetch_nodes/fetch_jobs in a loop.
    live_jobs = fetch_jobs()
    live: Job | None = next((j for j in live_jobs if j.job_id == job_id), None)
    if live is None:
        report.errors.append(InvestigationError(
            source="squeue",
            category="job_not_found",
            message="Job not in current squeue snapshot",
        ))

    # ---- determine state and reason --------------------------------------
    state_raw = live.state if live is not None else detail.get("JobState", "")
    state = state_raw.upper() if state_raw else ""
    reason_raw = ""
    reason_source_id = ""
    if live is not None and _present(live.reason):
        reason_raw = live.reason
        reason_source_id = "squeue.reason"
    elif _present(detail.get("Reason", "")):
        reason_raw = detail.get("Reason", "")
        reason_source_id = "scontrol.Reason"

    # ---- summary items ----------------------------------------------------
    user = live.user if live is not None else detail.get("UserId", "")
    if "(" in user:  # scontrol form: "alice(1001)"
        user = user.split("(", 1)[0]
    partition = live.partition if live is not None else detail.get("Partition", "")
    submit_time = detail.get("SubmitTime", "")
    start_time = detail.get("StartTime", "")
    time_used = live.time_used if live is not None else detail.get("RunTime", "")
    time_limit = live.time_limit if live is not None else detail.get("TimeLimit", "")
    num_nodes = live.num_nodes if live is not None else detail.get("NumNodes", "")
    num_cpus = live.num_cpus if live is not None else detail.get("NumCPUs", "")
    tres = detail.get("TRES", "") or detail.get("ReqTRES", "")
    # GPU count derived from TRES when present; this is informational only.
    gpu_request = ""
    if tres:
        m = re.search(r"gres/gpu(?::[^=]+)?=(\d+)", tres)
        if m:
            gpu_request = m.group(1)

    report.summary.extend([
        InvestigationItem(label="State", value=_display(state_raw)),
        InvestigationItem(label="Reason", value=_display(reason_raw)),
        InvestigationItem(label="User", value=_display(user)),
        InvestigationItem(label="Partition", value=_display(partition)),
        InvestigationItem(label="Requested nodes", value=_display(num_nodes)),
        InvestigationItem(label="Requested CPUs", value=_display(num_cpus)),
        InvestigationItem(label="Requested GPUs", value=_display(gpu_request)),
        InvestigationItem(label="Time used", value=_display(time_used)),
        InvestigationItem(label="Time limit", value=_display(time_limit)),
        InvestigationItem(label="Submit time", value=_display(submit_time)),
        InvestigationItem(label="Start time", value=_display(start_time)),
    ])

    # ---- evidence ---------------------------------------------------------
    if live is not None:
        report.evidence.append(InvestigationEvidence(
            id="squeue.state", label="State", value=live.state,
            source="squeue", confidence="high",
        ))
        report.evidence.append(InvestigationEvidence(
            id="squeue.reason", label="Reason", value=live.reason or "(none)",
            source="squeue", confidence="high",
        ))

    if scontrol_ok:
        if _present(detail.get("NumNodes", "")):
            report.evidence.append(InvestigationEvidence(
                id="scontrol.NumNodes", label="NumNodes",
                value=detail["NumNodes"],
                source="scontrol", confidence="high",
            ))
        if _present(detail.get("NumCPUs", "")):
            report.evidence.append(InvestigationEvidence(
                id="scontrol.NumCPUs", label="NumCPUs",
                value=detail["NumCPUs"],
                source="scontrol", confidence="high",
            ))
        if _present(detail.get("TRES", "")):
            report.evidence.append(InvestigationEvidence(
                id="scontrol.TRES", label="TRES",
                value=detail["TRES"],
                source="scontrol", confidence="high",
            ))
        if _present(detail.get("Partition", "")):
            report.evidence.append(InvestigationEvidence(
                id="scontrol.Partition", label="Partition",
                value=detail["Partition"],
                source="scontrol", confidence="high",
            ))
        # QOS visibility varies by site policy: some clusters strip the
        # field, others expose it via accounting only. Mark as medium so
        # the renderer surfaces the [medium] tag and the user knows to
        # treat it as informational.
        if _present(detail.get("QOS", "")):
            report.evidence.append(InvestigationEvidence(
                id="scontrol.QOS", label="QOS",
                value=detail["QOS"],
                source="scontrol", confidence="medium",
            ))
        if _present(detail.get("TimeLimit", "")):
            report.evidence.append(InvestigationEvidence(
                id="scontrol.TimeLimit", label="TimeLimit",
                value=detail["TimeLimit"],
                source="scontrol", confidence="high",
            ))
        if _present(detail.get("Dependency", "")):
            report.evidence.append(InvestigationEvidence(
                id="scontrol.Dependency", label="Dependency",
                value=detail["Dependency"],
                source="scontrol", confidence="high",
            ))
        if state in _RUNNING_STATES and _present(detail.get("NodeList", "")):
            report.evidence.append(InvestigationEvidence(
                id="scontrol.NodeList", label="NodeList",
                value=detail["NodeList"],
                source="scontrol", confidence="high",
            ))

    # ---- pending-reason explanation --------------------------------------
    if state in _PENDING_STATES:
        explanation = explain_pending_reason(reason_raw if _present(reason_raw) else None)
        evidence_refs = (reason_source_id,) if reason_source_id else ()
        report.explanations.append(InvestigationExplanation(
            title=explanation.title,
            detail=explanation.detail,
            confidence=explanation.confidence,
            evidence_refs=evidence_refs,
        ))

    # ---- dependencies ----------------------------------------------------
    try:
        deps = fetch_job_dependencies(job_id)
    except Exception:
        # Never let a parse-time error in dependency code abort the report.
        deps = []
        report.errors.append(InvestigationError(
            source="scontrol",
            category="dependency_parse_error",
            message="Failed to parse Dependency field",
        ))
    for dep in deps:
        report.evidence.append(InvestigationEvidence(
            id=f"dep.{dep.job_id}",
            label=f"Dependency {dep.dep_type}:{dep.job_id}",
            value=dep.state or "(unknown)",
            source="squeue",
            confidence="high",
        ))
    if state in _PENDING_STATES and deps:
        for dep in deps:
            # Treat anything not COMPLETED as "unsatisfied" for explanation
            # purposes. Slurm semantics vary across afterok/afterany/etc.
            # but we surface them all so the user can see the full chain.
            if (dep.state or "").upper() not in {"COMPLETED", "CD"}:
                report.explanations.append(InvestigationExplanation(
                    title="Dependency",
                    detail=(
                        f"Job is waiting on dependency "
                        f"{dep.dep_type}:{dep.job_id} (state: "
                        f"{dep.state or 'unknown'})."
                    ),
                    confidence="high",
                    evidence_refs=("scontrol.Dependency",),
                ))

    # ---- related nodes ---------------------------------------------------
    nodelist_expr = ""
    if state in _RUNNING_STATES:
        nodelist_expr = (live.nodelist if live is not None else "") or detail.get("NodeList", "")
    elif state in _PENDING_STATES:
        nodelist_expr = detail.get("ReqNodeList", "")

    if _present(nodelist_expr):
        try:
            hosts_out = _run(f"scontrol show hostnames {shlex.quote(nodelist_expr)}")
            requested_names = {h.strip() for h in hosts_out.splitlines() if h.strip()}
        except Exception:
            requested_names = set()
        if requested_names:
            # Cache fetch_nodes() result once; do NOT call inside a loop.
            all_nodes = fetch_nodes()
            for node in all_nodes:
                if node.name in requested_names:
                    report.related_nodes.append(node)

    # ---- suggested actions (SPEC sec. 8.4.6) -----------------------------
    # ALL actions here MUST be safe_for_user=True. Admin verbs (drain,
    # resume, modify partition, set qos) are intentionally absent — Slurm
    # itself will reject them when the underlying CLI is invoked.
    held = "Held" in (detail.get("JobState", "") or "") or "Held" in reason_raw

    report.suggested_actions.append(InvestigationAction(
        label="Watch this job",
        detail="Watch for state changes and notify on completion.",
        safe_for_user=True,
    ))
    report.suggested_actions.append(InvestigationAction(
        label="Inspect raw scontrol detail",
        detail="Open the full scontrol show job output.",
        safe_for_user=True,
    ))
    report.suggested_actions.append(InvestigationAction(
        label="Inspect logs",
        detail="Tail stdout/stderr if visible.",
        safe_for_user=True,
    ))
    report.suggested_actions.append(InvestigationAction(
        label="Copy investigation report",
        detail="Copy this report to clipboard for sharing.",
        safe_for_user=True,
    ))

    if state in _PENDING_STATES and held:
        report.suggested_actions.append(InvestigationAction(
            label="Release this job",
            detail="If you are the owner, release the hold.",
            safe_for_user=True,
        ))
    if state in _PENDING_STATES and deps:
        report.suggested_actions.append(InvestigationAction(
            label="Inspect dependency tree",
            detail="Open the dependency view.",
            safe_for_user=True,
        ))
    if state in _RUNNING_STATES:
        report.suggested_actions.append(InvestigationAction(
            label="Cancel this job",
            detail="Send scancel; only succeeds if you own the job.",
            safe_for_user=True,
        ))
        report.suggested_actions.append(InvestigationAction(
            label="Attach to a running node",
            detail="Open an interactive shell via srun.",
            safe_for_user=True,
        ))
    if state in _TERMINAL_STATES:
        report.suggested_actions.append(InvestigationAction(
            label="Inspect sacct accounting",
            detail="View elapsed time, exit code, efficiency if available.",
            safe_for_user=True,
        ))

    # ---- sacct accounting (terminal-state jobs only, SPEC sec. 8.4) ------
    # fetch_job_efficiency() returns {"available": False} on any failure
    # (sacct missing, parse error, no data) — that path is folded into
    # report.errors as a partial-result, not raised as an exception.
    if state in _TERMINAL_STATES:
        eff = fetch_job_efficiency(job_id)
        if eff.get("available"):
            report.evidence.append(InvestigationEvidence(
                id="sacct.cpu_eff",
                label="CPU efficiency",
                value=(
                    f"{round(eff['cpu_eff'] * 100)}% "
                    f"(used {eff['cpu_used_str']} of {eff['cpu_alloc_str']})"
                ),
                source="sacct",
                confidence="high",
            ))
            report.evidence.append(InvestigationEvidence(
                id="sacct.mem_eff",
                label="Memory efficiency",
                value=(
                    f"{round(eff['mem_eff'] * 100)}% "
                    f"(peak {eff['mem_peak_mb']} MB of "
                    f"{eff['mem_alloc_mb']} MB allocated)"
                ),
                source="sacct",
                confidence="high",
            ))
            report.raw_sections["sacct"] = "available"
        else:
            report.raw_sections["sacct"] = "unavailable"
            report.errors.append(InvestigationError(
                source="sacct",
                category="slurm_field_unavailable",
                message="sacct accounting not available for this job",
                stderr=None,
            ))

    return report


# ---------------------------------------------------------------------------
# Node investigation (SPEC sec. 8.5 / 9.3)
# ---------------------------------------------------------------------------

# Node-state token classes used to gate which "no visible jobs" explanation
# and which suggested actions appear. Lookup is on the bare uppercase token
# returned by ``_normalize_node_state_token`` so decoration suffixes such as
# '*', '-', '+' do not affect membership.
_NODE_ACTIVE_STATES = frozenset({"ALLOCATED", "MIXED"})
_NODE_UNAVAILABLE_STATES = frozenset({"DOWN", "DRAIN", "DRAINED"})


def _normalize_node_state_token(state: str) -> str:
    """Strip Slurm decoration suffixes; uppercase the bare token.

    Mirrors investigation._normalize_node_state but lives here so the
    Slurm data layer can classify state without importing from the
    domain module at call time.
    """
    suffixes = "*-+~#@!%$"
    s = (state or "").strip()
    while s and s[-1] in suffixes:
        s = s[:-1]
    return s.upper()


def _safe_int(value: str | None) -> int | None:
    """Parse a numeric Slurm field; return None on any failure."""
    if value is None:
        return None
    s = value.strip()
    if not s or s == "?":
        return None
    try:
        return int(s)
    except ValueError:
        return None


def investigate_node(node_name: str):
    """Build an InvestigationReport for a single node.

    SPEC sec. 8.5 / 9.3. Tolerant of partial failure: scontrol
    unavailable, node missing from the live sinfo snapshot, or
    fetch_jobs_on_node returning empty must NOT raise. A report with
    errors is always preferable to no report.
    """
    # Local import: investigation imports Job/Node from slurm, so the
    # dependency stays one-way at module load by deferring here.
    from .investigation import (
        InvestigationAction,
        InvestigationError,
        InvestigationEvidence,
        InvestigationExplanation,
        InvestigationItem,
        InvestigationReport,
        InvestigationTarget,
        explain_node_state,
    )

    target = InvestigationTarget(kind="node", identifier=node_name, source="typed")
    report = InvestigationReport(target=target, generated_at=datetime.now())

    # ---- scontrol show node ----------------------------------------------
    scontrol_out, scontrol_ok, scontrol_err = _run_result(
        f"scontrol show node {shlex.quote(node_name)}"
    )
    detail: dict[str, str] = {}
    if scontrol_ok:
        for token in scontrol_out.split():
            if "=" in token:
                k, _, v = token.partition("=")
                detail[k] = v
        report.raw_sections["scontrol show node"] = "available"
    else:
        report.raw_sections["scontrol show node"] = "unavailable"
        report.errors.append(InvestigationError(
            source="scontrol",
            category=classify_error(1, scontrol_err) or "slurm_command_failed",
            message=f"scontrol show node {node_name} failed",
            stderr=scontrol_err or None,
        ))

    # ---- live sinfo snapshot ---------------------------------------------
    # Cache once; SPEC requires we never call fetch_nodes() inside a loop.
    live_nodes = fetch_nodes()
    live: Node | None = next((n for n in live_nodes if n.name == node_name), None)
    if live is None:
        report.errors.append(InvestigationError(
            source="sinfo",
            category="node_not_found",
            message="Node not in current sinfo snapshot",
        ))

    # ---- determine state and reason --------------------------------------
    state_raw = live.state if live is not None else detail.get("State", "")
    state_token = _normalize_node_state_token(state_raw)
    partition = live.partition if live is not None else detail.get("Partitions", "")
    cpus_total_str = live.cpus_total if live is not None else detail.get("CPUTot", "")
    cpus_alloc_str = live.cpus_alloc if live is not None else detail.get("CPUAlloc", "")
    memory_total = live.memory_total if live is not None else detail.get("RealMemory", "")
    memory_free = live.memory_free if live is not None else detail.get("FreeMem", "")
    load = live.load if live is not None else detail.get("CPULoad", "")
    gpu_total = live.gpu_total if live is not None else 0
    gpu_alloc = live.gpu_alloc if live is not None else 0
    gres = detail.get("Gres", "")
    reason_raw = detail.get("Reason", "")

    # ---- summary items ----------------------------------------------------
    report.summary.append(InvestigationItem(label="State", value=_display(state_raw)))
    report.summary.append(InvestigationItem(label="Partition", value=_display(partition)))
    report.summary.append(InvestigationItem(
        label="CPUs allocated/total",
        value=_display(f"{cpus_alloc_str}/{cpus_total_str}")
              if (cpus_alloc_str or cpus_total_str) else "(unavailable)",
    ))
    # SPEC sec. 6.2: missing GPU data MUST NOT imply zero GPUs. Only emit
    # the GPU summary when Slurm actually reported a positive total.
    if gpu_total > 0:
        report.summary.append(InvestigationItem(
            label="GPUs allocated/total",
            value=f"{gpu_alloc}/{gpu_total}",
        ))
    report.summary.append(InvestigationItem(
        label="Memory free/total",
        value=_display(f"{memory_free}/{memory_total}")
              if (memory_free or memory_total) else "(unavailable)",
    ))
    report.summary.append(InvestigationItem(label="Load", value=_display(load)))
    if _present(gres):
        report.summary.append(InvestigationItem(label="GRES", value=_display(gres)))
    if _present(reason_raw):
        report.summary.append(InvestigationItem(label="Reason", value=_display(reason_raw)))

    # ---- evidence ---------------------------------------------------------
    if live is not None:
        report.evidence.append(InvestigationEvidence(
            id="sinfo.state", label="State", value=live.state,
            source="sinfo", confidence="high",
        ))
        report.evidence.append(InvestigationEvidence(
            id="sinfo.cpus", label="CPUs allocated/total",
            value=f"{live.cpus_alloc}/{live.cpus_total}",
            source="sinfo", confidence="high",
        ))
        if live.gpu_total > 0:
            report.evidence.append(InvestigationEvidence(
                id="sinfo.gpus", label="GPUs allocated/total",
                value=f"{live.gpu_alloc}/{live.gpu_total}",
                source="sinfo", confidence="high",
            ))
        # Memory accounting can lag the kernel by minutes; mark as medium
        # so the renderer surfaces the [medium] tag for derived consumers.
        report.evidence.append(InvestigationEvidence(
            id="sinfo.memory_free", label="Memory free",
            value=live.memory_free,
            source="sinfo", confidence="medium",
        ))

    if scontrol_ok:
        if _present(detail.get("Partitions", "")):
            report.evidence.append(InvestigationEvidence(
                id="scontrol.partitions", label="Partitions",
                value=detail["Partitions"],
                source="scontrol", confidence="high",
            ))
        if _present(detail.get("Gres", "")):
            report.evidence.append(InvestigationEvidence(
                id="scontrol.gres", label="GRES",
                value=detail["Gres"],
                source="scontrol", confidence="high",
            ))
        if _present(detail.get("Features", "")) or _present(detail.get("AvailableFeatures", "")):
            feats = detail.get("Features") or detail.get("AvailableFeatures") or ""
            report.evidence.append(InvestigationEvidence(
                id="scontrol.features", label="Features",
                value=feats,
                source="scontrol", confidence="high",
            ))
        if _present(reason_raw):
            report.evidence.append(InvestigationEvidence(
                id="scontrol.reason", label="Reason",
                value=reason_raw,
                source="scontrol", confidence="high",
            ))

    # ---- state explanation -----------------------------------------------
    explanation = explain_node_state(state_raw or "")
    report.explanations.append(InvestigationExplanation(
        title=explanation.title,
        detail=explanation.detail,
        confidence=explanation.confidence,
        evidence_refs=("sinfo.state",),
    ))

    # ---- jobs currently using this node ----------------------------------
    try:
        jobs_on_node = fetch_jobs_on_node(node_name)
    except Exception:
        jobs_on_node = []
    for job in jobs_on_node:
        report.related_jobs.append(job)

    # Cap related_jobs per [investigation].max_related_jobs (SPEC §16.9 example).
    # 0 / negative disables the cap. Default 20. Defensive against malformed
    # config values: anything non-coercible falls back to the documented default.
    from . import config as _config
    try:
        cap_raw = _config.load().get("investigation", {}).get("max_related_jobs", 20)
        cap = int(cap_raw)
    except (TypeError, ValueError):
        cap = 20
    if cap > 0 and len(report.related_jobs) > cap:
        report.related_jobs = report.related_jobs[:cap]

    if not jobs_on_node and state_token in _NODE_ACTIVE_STATES:
        report.explanations.append(InvestigationExplanation(
            title="No matching jobs visible",
            detail=(
                "No matching jobs are visible to sqtop. The node may still "
                "be unavailable due to reservations, drain state, hidden "
                "jobs, or cluster policy."
            ),
            confidence="low",
            evidence_refs=("sinfo.state",),
        ))

    # ---- derived free-resource estimates (SPEC sec. 8.5.3) ---------------
    # Marked source="derived" so render_report() tags them with the
    # confidence label, distinguishing them from raw Slurm-reported
    # fields. Confidence is "medium" because the derivation is
    # arithmetic on values that may themselves lag (memory) or be
    # approximate (CPU alloc as the live snapshot drifts).
    cpus_total_int = _safe_int(cpus_total_str)
    cpus_alloc_int = _safe_int(cpus_alloc_str)
    if cpus_total_int is not None and cpus_alloc_int is not None:
        free_cpus = max(0, cpus_total_int - cpus_alloc_int)
        report.evidence.append(InvestigationEvidence(
            id="derived.cpus_free", label="CPUs free",
            value=f"{free_cpus}/{cpus_total_int}",
            source="derived", confidence="medium",
        ))
    if gpu_total > 0:
        free_gpus = max(0, gpu_total - gpu_alloc)
        report.evidence.append(InvestigationEvidence(
            id="derived.gpus_free", label="GPUs free",
            value=f"{free_gpus}/{gpu_total}",
            source="derived", confidence="medium",
        ))
    if _present(memory_free):
        report.evidence.append(InvestigationEvidence(
            id="derived.memory_free", label="Memory free",
            value=memory_free,
            source="derived", confidence="medium",
        ))

    # ---- suggested actions (SPEC sec. 8.5, safe-for-user only) -----------
    # ALL actions here MUST be safe_for_user=True. Admin verbs (drain,
    # resume, modify partition, set qos, scontrol update, scontrol reboot,
    # sudo) are intentionally absent — these belong to admin tooling, not
    # the user-facing investigation report.
    report.suggested_actions.append(InvestigationAction(
        label="Inspect raw scontrol detail",
        detail="Open the full scontrol show node output.",
        safe_for_user=True,
    ))
    report.suggested_actions.append(InvestigationAction(
        label="Copy investigation report",
        detail="Copy this report to clipboard for sharing.",
        safe_for_user=True,
    ))
    report.suggested_actions.append(InvestigationAction(
        label="Contact admin if state is unexpected",
        detail=(
            "If the reported state looks unexpected, share this report "
            "with cluster admins."
        ),
        safe_for_user=True,
    ))
    if report.related_jobs:
        report.suggested_actions.append(InvestigationAction(
            label="Investigate the jobs using this node",
            detail="Open per-job investigations to understand current load.",
            safe_for_user=True,
        ))

    return report


# ---------------------------------------------------------------------------
# SSH remote support
# ---------------------------------------------------------------------------

_SSH_HOST: str | None = None
_SSH_KEY: str | None = None


def set_remote(host: str, key: str = "") -> None:
    global _SSH_HOST, _SSH_KEY
    _SSH_HOST = host.strip() or None
    _SSH_KEY = key.strip() or None

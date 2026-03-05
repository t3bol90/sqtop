"""Tests for slurm.py data-parsing functions."""
from __future__ import annotations

import subprocess
import pytest
from sqtop import slurm


# ── fetch_jobs ────────────────────────────────────────────────────────────────

def test_fetch_jobs_normal(mock_run):
    mock_run("123|myjob|alice|RUNNING|gpu|1|8|1:00:00|8:00:00|None|node01|normal\n")
    jobs = slurm.fetch_jobs()
    assert len(jobs) == 1
    j = jobs[0]
    assert j.job_id == "123"
    assert j.name == "myjob"
    assert j.user == "alice"
    assert j.state == "RUNNING"
    assert j.partition == "gpu"
    assert j.num_cpus == "8"
    assert j.time_used == "1:00:00"
    assert j.time_limit == "8:00:00"
    assert j.reason == "None"
    assert j.nodelist == "node01"
    assert j.qos == "normal"


def test_fetch_jobs_empty(mock_run):
    mock_run("")
    assert slurm.fetch_jobs() == []


def test_fetch_jobs_qos_normalization(mock_run):
    """N/A and (null) QOS values are normalized to empty string."""
    mock_run("1|a|alice|RUNNING|gpu|1|4|0:01|8:00:00|None|node01|N/A\n")
    assert slurm.fetch_jobs()[0].qos == ""
    mock_run("2|b|bob|RUNNING|gpu|1|4|0:01|8:00:00|None|node02|(null)\n")
    assert slurm.fetch_jobs()[0].qos == ""


def test_fetch_jobs_malformed_line(mock_run):
    """Lines with fewer than 12 fields are silently skipped."""
    mock_run("123|myjob|alice\n")
    assert slurm.fetch_jobs() == []


def test_fetch_jobs_mixed_lines(mock_run):
    """Good lines are kept; malformed lines are skipped."""
    good = "1|a|alice|RUNNING|gpu|1|4|0:01|8:00:00|None|node01|normal"
    bad  = "2|b|bob"
    mock_run(f"{good}\n{bad}\n")
    jobs = slurm.fetch_jobs()
    assert len(jobs) == 1
    assert jobs[0].job_id == "1"


# ── _parse_gpu_count ─────────────────────────────────────────────────────────

@pytest.mark.parametrize("gres,expected", [
    ("gpu:4",                   4),
    ("gpu:a100:4",              4),
    ("gpu:a100:4(IDX:0,1,2,3)", 4),
    ("",                        0),
    ("cpu:8",                   0),
    ("(null)",                  0),
])
def test_parse_gpu_count(gres, expected):
    assert slurm._parse_gpu_count(gres) == expected


# ── _fetch_gpus_alloc ────────────────────────────────────────────────────────

def test_fetch_gpus_alloc_from_alloc_tres(mock_run):
    """Reads GPU count from AllocTRES when present."""
    mock_run(
        "NodeName=node01 AllocTRES=cpu=4,mem=16G,gres/gpu=2\n"
        "NodeName=node02 AllocTRES=cpu=8\n"
    )
    result = slurm._fetch_gpus_alloc()
    assert result.get("node01") == 2
    assert "node02" not in result


def test_fetch_gpus_alloc_fallback_gres_used(mock_run):
    """Falls back to GresUsed when AllocTRES doesn't have gpu."""
    mock_run(
        "NodeName=node01 AllocTRES=cpu=4 GresUsed=gpu:a100:3(IDX:0,1,2)\n"
    )
    result = slurm._fetch_gpus_alloc()
    assert result.get("node01") == 3


def test_fetch_gpus_alloc_no_gpu_node(mock_run):
    """Nodes with no GPU show nothing in the result."""
    mock_run("NodeName=node01 AllocTRES=cpu=4,mem=8G\n")
    result = slurm._fetch_gpus_alloc()
    assert "node01" not in result


# ── fetch_nodes ───────────────────────────────────────────────────────────────

def test_fetch_nodes_normal(monkeypatch):
    sinfo_out = "node01|idle|gpu|4|4/0/0/4|32000|28000|0.10|gpu:2\n"
    monkeypatch.setattr(slurm, "_run", lambda cmd: sinfo_out)
    monkeypatch.setattr(slurm, "_fetch_gpus_alloc", lambda: {"node01": 1})
    nodes = slurm.fetch_nodes()
    assert len(nodes) == 1
    n = nodes[0]
    assert n.name == "node01"
    assert n.state == "idle"
    assert n.cpus_total == "4"
    assert n.cpus_alloc == "4"
    assert n.gpu_total == 2
    assert n.gpu_alloc == 1


def test_fetch_nodes_cpu_parts_fallback(monkeypatch):
    """When cpu_parts has wrong length, cpus_total and cpus_alloc should both be '?'."""
    # cpu field is "8" (not slash-separated), so cpu_parts length will be 1, not 4
    sinfo_out = "node01|idle|gpu|4|8|32000|28000|0.10|(null)\n"
    monkeypatch.setattr(slurm, "_run", lambda cmd: sinfo_out)
    monkeypatch.setattr(slurm, "_fetch_gpus_alloc", lambda: {})
    nodes = slurm.fetch_nodes()
    assert len(nodes) == 1
    assert nodes[0].cpus_total == "?"
    assert nodes[0].cpus_alloc == "?"


# ── fetch_cluster_summary ────────────────────────────────────────────────────

def test_fetch_cluster_summary_normal(mock_run):
    mock_run("gpu|up|8:00:00|4|idle|node[01-04]\n")
    summaries = slurm.fetch_cluster_summary()
    assert len(summaries) == 1
    s = summaries[0]
    assert s.partition == "gpu"
    assert s.avail == "up"
    assert s.timelimit == "8:00:00"
    assert s.nodes == "4"
    assert s.state == "idle"
    assert s.nodelist == "node[01-04]"


def test_fetch_cluster_summary_empty(mock_run):
    mock_run("")
    assert slurm.fetch_cluster_summary() == []


# ── fetch_job_detail / fetch_node_detail ─────────────────────────────────────

def test_fetch_job_detail_parses_kv(mock_run):
    mock_run("JobId=123 JobName=myjob UserId=alice(1001) JobState=RUNNING\n")
    detail = slurm.fetch_job_detail("123")
    assert detail["JobId"] == "123"
    assert detail["JobName"] == "myjob"
    assert detail["JobState"] == "RUNNING"


def test_fetch_node_detail_skips_no_equals(mock_run):
    """Tokens without '=' in them are silently skipped."""
    mock_run("NodeName=node01 State=idle SOMETOKEN\n")
    detail = slurm.fetch_node_detail("node01")
    assert detail["NodeName"] == "node01"
    assert detail["State"] == "idle"
    assert "SOMETOKEN" not in detail


# ── hold / release / requeue ─────────────────────────────────────────────────

def test_hold_job_result_ok(mock_run_result):
    mock_run_result(stdout="", ok=True, stderr="")
    ok, stderr = slurm.hold_job_result("123")
    assert ok is True
    assert stderr == ""


def test_hold_job_result_fail(mock_run_result):
    mock_run_result(stdout="", ok=False, stderr="error: job not found")
    ok, stderr = slurm.hold_job_result("123")
    assert ok is False
    assert "error" in stderr


def test_release_job_result_ok(mock_run_result):
    mock_run_result(ok=True)
    ok, _ = slurm.release_job_result("123")
    assert ok is True


def test_requeue_job_result_ok(mock_run_result):
    mock_run_result(ok=True)
    ok, _ = slurm.requeue_job_result("123")
    assert ok is True


# ── tail_log_file ────────────────────────────────────────────────────────────

def test_tail_log_file_no_path(mock_run):
    mock_run("anything")  # should not be called
    result = slurm.tail_log_file("")
    assert result == "(no log path)"


def test_tail_log_file_non_empty(mock_run):
    mock_run("line1\nline2\n")
    result = slurm.tail_log_file("/some/path.log")
    assert "line1" in result


def test_tail_log_file_empty_output(mock_run):
    mock_run("")
    result = slurm.tail_log_file("/some/path.log")
    assert result == "(empty or file not found)"


# ── _run_result exception handling ───────────────────────────────────────────

def test_run_result_timeout(monkeypatch):
    """TimeoutExpired → returns empty string, ok=False."""
    def fake_run(*args, **kwargs):
        raise subprocess.TimeoutExpired(cmd="squeue", timeout=10)
    monkeypatch.setattr(subprocess, "run", fake_run)
    out, ok, stderr = slurm._run_result("squeue --noheader")
    assert out == ""
    assert ok is False


def test_run_result_file_not_found(monkeypatch):
    """FileNotFoundError → returns empty string, ok=False."""
    def fake_run(*args, **kwargs):
        raise FileNotFoundError("squeue not found")
    monkeypatch.setattr(subprocess, "run", fake_run)
    out, ok, stderr = slurm._run_result("squeue --noheader")
    assert out == ""
    assert ok is False


def test_run_result_oserror(monkeypatch):
    """OSError → returns empty string, ok=False (regression test for bug fix)."""
    def fake_run(*args, **kwargs):
        raise OSError("permission denied")
    monkeypatch.setattr(subprocess, "run", fake_run)
    out, ok, stderr = slurm._run_result("squeue --noheader")
    assert out == ""
    assert ok is False

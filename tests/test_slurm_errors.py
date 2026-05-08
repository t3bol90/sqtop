"""Tests for slurm error categorization (SPEC §10.1)."""
from __future__ import annotations

import subprocess
from types import SimpleNamespace

import pytest

from sqtop import slurm
from sqtop.slurm import (
    ERROR_CATEGORIES,
    CommandStat,
    classify_error,
    fetch_command_health,
)


# ── classify_error: table-driven ─────────────────────────────────────────────

@pytest.mark.parametrize("returncode,stderr,expected", [
    # Success: returncode wins regardless of stderr text.
    (0, "", None),
    (0, "warning: foo", None),

    # Exception path (returncode is None) — distinguished by stderr substring.
    (None, "timeout", "slurm_command_timeout"),
    (None, "command not found", "slurm_command_not_found"),
    (None, "OS error: too many open files", "slurm_command_failed"),

    # Slurm permission-denied variants.
    (1, "scancel: error: Permission denied", "slurm_permission_denied"),
    (1, "user unauthorized", "slurm_permission_denied"),
    (1, "operation not allowed", "slurm_permission_denied"),

    # SSH connection failures.
    (255, "ssh: connect to host x: Connection refused", "ssh_connection_failed"),
    (255, "ssh: Could not resolve hostname x", "ssh_connection_failed"),
    (255, "Connection closed by remote host", "ssh_connection_failed"),

    # SSH auth failure: "publickey" must be classified as ssh_auth_failed
    # even though stderr also contains "Permission denied".
    (255, "Permission denied (publickey).", "ssh_auth_failed"),
    (1, "authentication failed", "ssh_auth_failed"),

    # Timeout reported via stderr text on a non-zero exit.
    (1, "timeout waiting for response", "slurm_command_timeout"),

    # Job-level lookup failures.
    (1, "scontrol: error: Invalid job id 9999", "job_not_found"),
    (1, "job not found", "job_not_found"),
    (1, "unknown job specification", "job_not_found"),

    # Node-level lookup failures.
    (1, "scontrol: error: invalid node specification", "node_not_found"),
    (1, "node not found", "node_not_found"),
    (1, "unknown node specification", "node_not_found"),

    # Generic / empty stderr → slurm_command_failed.
    (1, "", "slurm_command_failed"),
    (2, "some opaque slurm message", "slurm_command_failed"),
])
def test_classify_error_table(returncode, stderr, expected):
    category = classify_error(returncode, stderr)
    assert category == expected
    # Either None (success) or a documented member of ERROR_CATEGORIES.
    assert category is None or category in ERROR_CATEGORIES


def test_classify_error_publickey_priority_beats_permission_denied():
    """A 'Permission denied (publickey)' stderr is SSH auth failure, not
    a generic Slurm permission denial. Priority must be ssh_auth_failed."""
    assert classify_error(255, "Permission denied (publickey).") == "ssh_auth_failed"


# ── _run_result integration ──────────────────────────────────────────────────

def _fake_completed(returncode: int, stdout: str = "", stderr: str = ""):
    return SimpleNamespace(returncode=returncode, stdout=stdout, stderr=stderr)


def test_run_result_success_no_category(monkeypatch):
    """Successful command (returncode 0) records error_category=None."""
    monkeypatch.setattr(
        subprocess, "run",
        lambda *a, **kw: _fake_completed(0, stdout="ok\n", stderr=""),
    )
    out, ok, stderr = slurm._run_result("squeue --noheader")
    assert ok is True
    assert out == "ok\n"
    last = slurm._COMMAND_HISTORY[-1]
    assert last.ok is True
    assert last.error_category is None


def test_run_result_failure_permission_denied(monkeypatch):
    """returncode 1 with 'Permission denied' → slurm_permission_denied."""
    monkeypatch.setattr(
        subprocess, "run",
        lambda *a, **kw: _fake_completed(1, stdout="", stderr="Permission denied"),
    )
    out, ok, stderr = slurm._run_result("scancel 1")
    assert ok is False
    last = slurm._COMMAND_HISTORY[-1]
    assert last.ok is False
    assert last.error_category == "slurm_permission_denied"
    assert last.stderr == "Permission denied"


# ── Backward-compatibility ────────────────────────────────────────────────────

def test_command_stat_default_error_category_is_none():
    """Positional construction without error_category still works; default is None."""
    stat = CommandStat(command="x", ok=True, latency_ms=5)
    assert stat.error_category is None
    assert stat.command == "x"
    assert stat.ok is True
    assert stat.latency_ms == 5
    assert stat.stderr == ""


# ── fetch_command_health surfaces the new field ──────────────────────────────

def test_fetch_command_health_includes_error_category(monkeypatch):
    """A failing call shows up in fetch_command_health() with its category."""
    monkeypatch.setattr(
        subprocess, "run",
        lambda *a, **kw: _fake_completed(1, stdout="", stderr="Permission denied"),
    )
    slurm._run_result("scancel 1")
    history = fetch_command_health(1)
    assert len(history) == 1
    record = history[0]
    assert record.ok is False
    assert record.error_category == "slurm_permission_denied"
    assert record.stderr == "Permission denied"

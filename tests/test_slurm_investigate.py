"""Tests for the Investigation Mode data layer (SPEC sec. 8.4 / 9.3 / 10.3).

Covers:
  * ``fetch_jobs_on_node`` — squeue -w wrapper + shared row parser
  * ``investigate_job`` — partial-result-tolerant report builder

Mocks ``slurm._run`` and ``slurm._run_result`` rather than calling
subprocess. The investigation report MUST never include admin-only
suggested actions, regardless of state — that invariant is asserted
across every parametrized state in this file.
"""
from __future__ import annotations

import pytest

from sqtop import slurm
from sqtop.investigation import (
    InvestigationAction,
    InvestigationError,
    InvestigationEvidence,
    InvestigationReport,
)


_FORBIDDEN_ADMIN_VERBS = ("drain", "resume", "modify partition", "set qos", "sudo")


# ---------------------------------------------------------------------------
# fetch_jobs_on_node
# ---------------------------------------------------------------------------


def test_fetch_jobs_on_node_happy_path(mock_run):
    mock_run("777|train|alice|RUNNING|gpu|1|8|0:30:00|8:00:00|None|node01|normal\n")
    jobs = slurm.fetch_jobs_on_node("node01")
    assert len(jobs) == 1
    j = jobs[0]
    assert j.job_id == "777"
    assert j.user == "alice"
    assert j.state == "RUNNING"
    assert j.nodelist == "node01"
    assert j.qos == "normal"


def test_fetch_jobs_on_node_empty_input_does_not_invoke(monkeypatch):
    """Empty / whitespace-only node names must short-circuit (no command)."""
    calls: list[str] = []

    def sentinel(cmd: str) -> str:
        calls.append(cmd)
        return ""

    monkeypatch.setattr(slurm, "_run", sentinel)
    assert slurm.fetch_jobs_on_node("") == []
    assert slurm.fetch_jobs_on_node("   ") == []
    assert calls == []


def test_fetch_jobs_on_node_malformed_line_skipped(mock_run):
    """Rows with too few fields are skipped via the shared row parser."""
    good = "1|a|alice|RUNNING|gpu|1|4|0:01|8:00:00|None|node01|normal"
    bad = "2|b|bob"
    mock_run(f"{good}\n{bad}\n")
    jobs = slurm.fetch_jobs_on_node("node01")
    assert len(jobs) == 1
    assert jobs[0].job_id == "1"


def test_fetch_jobs_on_node_uses_squeue_filter(monkeypatch):
    """The constructed command must include `-w <node>` and the squeue fmt."""
    captured: dict[str, str] = {}

    def fake_run(cmd: str) -> str:
        captured["cmd"] = cmd
        return ""

    monkeypatch.setattr(slurm, "_run", fake_run)
    slurm.fetch_jobs_on_node("gpu-a100-02")
    assert "squeue" in captured["cmd"]
    assert "-w gpu-a100-02" in captured["cmd"]
    assert "%i|%j|%u|%T|%P|%D|%C|%M|%l|%R|%N|%q" in captured["cmd"]


# ---------------------------------------------------------------------------
# investigate_job — fixtures and helpers
# ---------------------------------------------------------------------------


def _scontrol_running_block(job_id: str = "12345") -> str:
    return (
        f"JobId={job_id} JobName=train UserId=alice(1001) "
        "JobState=RUNNING Reason=None Dependency=(null) "
        "Partition=gpu QOS=normal "
        "NumNodes=1 NumCPUs=8 TimeLimit=08:00:00 RunTime=00:30:00 "
        "TRES=cpu=8,mem=32G,node=1,gres/gpu=1 "
        "SubmitTime=2026-05-08T10:00:00 StartTime=2026-05-08T10:01:00 "
        "NodeList=node01 ReqNodeList=(null)"
    )


def _scontrol_pending_resources_block(job_id: str = "12346") -> str:
    return (
        f"JobId={job_id} JobName=preprocess UserId=bob(1002) "
        "JobState=PENDING Reason=Resources Dependency=(null) "
        "Partition=gpu QOS=normal "
        "NumNodes=1 NumCPUs=16 TimeLimit=24:00:00 RunTime=00:00:00 "
        "TRES=cpu=16,mem=128G,node=1,gres/gpu=1 "
        "SubmitTime=2026-05-08T11:00:00 StartTime=Unknown "
        "NodeList=(null) ReqNodeList=(null)"
    )


def _scontrol_pending_dep_block(job_id: str = "12347", dep_id: str = "99999") -> str:
    return (
        f"JobId={job_id} JobName=postproc UserId=carol(1003) "
        f"JobState=PENDING Reason=Dependency Dependency=afterok:{dep_id} "
        "Partition=cpu QOS=normal "
        "NumNodes=1 NumCPUs=4 TimeLimit=01:00:00 RunTime=00:00:00 "
        "TRES=cpu=4,mem=8G,node=1 "
        "SubmitTime=2026-05-08T12:00:00 StartTime=Unknown "
        "NodeList=(null) ReqNodeList=(null)"
    )


@pytest.fixture
def patch_scontrol(monkeypatch):
    """Factory that patches ``slurm._run_result`` to return a fixed scontrol output."""
    def factory(stdout: str, ok: bool = True, stderr: str = "") -> None:
        monkeypatch.setattr(slurm, "_run_result", lambda cmd: (stdout, ok, stderr))
    return factory


@pytest.fixture
def patch_run(monkeypatch):
    """Factory that patches ``slurm._run`` with a command-router function.

    The callable receives the full command string and returns a string.
    Tests can therefore route different commands (squeue vs scontrol show
    hostnames) to different fakes without juggling multiple monkeypatches.
    """
    def factory(router):
        monkeypatch.setattr(slurm, "_run", router)
    return factory


def _action_text(actions: list[InvestigationAction]) -> str:
    return "\n".join(f"{a.label} :: {a.detail}" for a in actions).lower()


# ---------------------------------------------------------------------------
# investigate_job — happy paths
# ---------------------------------------------------------------------------


def test_investigate_job_pending_resources(patch_scontrol, patch_run):
    """PENDING + Resources: explanation present, evidence intact, no errors."""
    job_id = "12346"
    patch_scontrol(_scontrol_pending_resources_block(job_id))

    squeue_line = (
        f"{job_id}|preprocess|bob|PENDING|gpu|1|16|0:00|24:00:00|"
        "Resources||normal\n"
    )

    def router(cmd: str) -> str:
        if "squeue" in cmd and "--noheader" in cmd and "-j" not in cmd:
            return squeue_line
        # Dependency lookup (squeue -j ids)
        return ""

    patch_run(router)

    report = slurm.investigate_job(job_id)
    assert isinstance(report, InvestigationReport)
    assert report.target.kind == "job"
    assert report.target.identifier == job_id
    assert len(report.summary) > 0
    assert report.errors == []

    # Reason evidence
    reasons = [ev for ev in report.evidence if ev.id == "squeue.reason"]
    assert reasons and reasons[0].value == "Resources"

    # Resources -> "Matching resources are not currently available"
    assert any(
        "resource" in exp.title.lower() or "resource" in exp.detail.lower()
        for exp in report.explanations
    )

    assert any(a.safe_for_user for a in report.suggested_actions)
    text = _action_text(report.suggested_actions)
    for verb in _FORBIDDEN_ADMIN_VERBS:
        assert verb not in text


def test_investigate_job_pending_dependency(patch_scontrol, patch_run):
    """PENDING + Dependency: dep evidence + dep explanation surfaced."""
    job_id = "12347"
    dep_id = "99999"
    patch_scontrol(_scontrol_pending_dep_block(job_id, dep_id))

    squeue_main = (
        f"{job_id}|postproc|carol|PENDING|cpu|1|4|0:00|01:00:00|"
        "Dependency||normal\n"
    )
    # squeue -j 99999 returns the dep job in a non-completed state
    squeue_dep = f"{dep_id}|RUNNING\n"
    scontrol_main = _scontrol_pending_dep_block(job_id, dep_id)

    def router(cmd: str) -> str:
        # fetch_job_dependencies -> fetch_job_detail -> _run("scontrol show job ...")
        if "scontrol show job" in cmd:
            return scontrol_main
        if "squeue" in cmd and "-j" in cmd and dep_id in cmd:
            return squeue_dep
        if "squeue" in cmd and "--noheader" in cmd and "-j" not in cmd:
            return squeue_main
        return ""

    patch_run(router)

    report = slurm.investigate_job(job_id)
    # Dependency evidence keyed by dep job_id
    dep_evidence = [ev for ev in report.evidence if ev.id.startswith(f"dep.{dep_id}")]
    assert dep_evidence, "expected dep.<id> evidence entry"

    # An explanation titled Dependency must be present
    dep_explanations = [exp for exp in report.explanations if "Dependency" in exp.title]
    assert dep_explanations

    text = _action_text(report.suggested_actions)
    for verb in _FORBIDDEN_ADMIN_VERBS:
        assert verb not in text


def test_investigate_job_running_populates_related_nodes(monkeypatch, patch_scontrol, patch_run):
    """RUNNING + NodeList: related_nodes is populated from fetch_nodes()."""
    job_id = "12345"
    patch_scontrol(_scontrol_running_block(job_id))

    squeue_main = (
        f"{job_id}|train|alice|RUNNING|gpu|1|8|0:30:00|08:00:00|"
        "None|node01|normal\n"
    )

    def router(cmd: str) -> str:
        if "scontrol show hostnames" in cmd:
            return "node01\n"
        if "squeue" in cmd and "--noheader" in cmd and "-j" not in cmd:
            return squeue_main
        return ""

    patch_run(router)

    fake_node = slurm.Node(
        name="node01",
        state="allocated",
        partition="gpu",
        cpus_total="8",
        cpus_alloc="8",
        memory_total="32000",
        memory_free="0",
    )
    monkeypatch.setattr(slurm, "fetch_nodes", lambda: [fake_node])

    report = slurm.investigate_job(job_id)
    assert any(n.name == "node01" for n in report.related_nodes)
    # Running job suggested actions include cancel + attach
    text = _action_text(report.suggested_actions)
    assert "cancel this job" in text
    assert "attach" in text
    for verb in _FORBIDDEN_ADMIN_VERBS:
        assert verb not in text


# ---------------------------------------------------------------------------
# investigate_job — partial failure paths (SPEC sec. 10.3)
# ---------------------------------------------------------------------------


def test_investigate_job_scontrol_failure_partial_report(patch_scontrol, patch_run):
    """scontrol failure produces an error entry but does not abort the report."""
    job_id = "55555"
    patch_scontrol("", ok=False, stderr="Permission denied")

    squeue_main = (
        f"{job_id}|x|alice|PENDING|gpu|1|8|0:00|08:00:00|"
        "Resources||normal\n"
    )

    def router(cmd: str) -> str:
        if "squeue" in cmd and "--noheader" in cmd and "-j" not in cmd:
            return squeue_main
        return ""

    patch_run(router)

    report = slurm.investigate_job(job_id)
    assert any(
        isinstance(e, InvestigationError) and e.category == "slurm_permission_denied"
        for e in report.errors
    )
    # Report still constructed
    assert report.target.identifier == job_id
    assert report.raw_sections.get("scontrol show job") == "unavailable"
    # Squeue summary still populated
    assert any(item.label == "State" for item in report.summary)


def test_investigate_job_not_in_squeue_uses_scontrol_only(patch_scontrol, patch_run):
    """Job missing from squeue triggers job_not_found error; scontrol fields still flow."""
    job_id = "12345"
    patch_scontrol(_scontrol_running_block(job_id))

    def router(cmd: str) -> str:
        # squeue snapshot is empty — job has rolled out
        return ""

    patch_run(router)

    report = slurm.investigate_job(job_id)
    assert any(e.category == "job_not_found" for e in report.errors)
    # scontrol-derived summary still present
    assert any(item.label == "Partition" and item.value == "gpu" for item in report.summary)
    assert any(ev.id == "scontrol.NumCPUs" for ev in report.evidence)


# ---------------------------------------------------------------------------
# Admin-action invariant (SPEC sec. 8.4.6)
# ---------------------------------------------------------------------------


@pytest.mark.parametrize(
    "state,reason",
    [
        ("RUNNING", "None"),
        ("PENDING", "Resources"),
        ("PENDING", "Priority"),
        ("PENDING", "Dependency"),
        ("PENDING", "JobHeldUser"),
        ("PENDING", "JobHeldAdmin"),
        ("COMPLETED", ""),
        ("FAILED", ""),
        ("CANCELLED", ""),
    ],
)
def test_investigate_job_never_suggests_admin_actions(
    patch_scontrol, patch_run, state, reason,
):
    job_id = "42"
    scontrol = (
        f"JobId={job_id} JobName=t UserId=u(1) "
        f"JobState={state} Reason={reason or 'None'} Dependency=(null) "
        "Partition=cpu QOS=normal NumNodes=1 NumCPUs=4 "
        "TimeLimit=01:00:00 RunTime=00:00:00 "
        "TRES=cpu=4,node=1 "
        "SubmitTime=2026-05-08T10:00:00 StartTime=Unknown "
        "NodeList=(null) ReqNodeList=(null)"
    )
    patch_scontrol(scontrol)

    squeue_main = (
        f"{job_id}|t|u|{state}|cpu|1|4|0:00|01:00:00|"
        f"{reason or 'None'}||normal\n"
    )

    def router(cmd: str) -> str:
        if "squeue" in cmd and "--noheader" in cmd and "-j" not in cmd:
            return squeue_main
        return ""

    patch_run(router)

    report = slurm.investigate_job(job_id)
    assert all(a.safe_for_user for a in report.suggested_actions)
    text = _action_text(report.suggested_actions)
    for verb in _FORBIDDEN_ADMIN_VERBS:
        assert verb not in text, f"forbidden verb {verb!r} appeared in actions: {text}"

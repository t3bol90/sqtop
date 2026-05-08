"""Tests for ``investigate_node`` data-layer (SPEC sec. 8.5 / 9.3 / 10.3).

Mocks ``slurm._run`` and ``slurm._run_result`` rather than calling
subprocess. The investigation report MUST never include admin-only
suggested actions, regardless of state — that invariant is asserted
across every parametrized state in this file.
"""
from __future__ import annotations

import pytest

from sqtop import config, slurm
from sqtop.investigation import (
    InvestigationAction,
    InvestigationError,
    InvestigationReport,
)


_FORBIDDEN_ADMIN_VERBS = (
    "drain",
    "resume",
    "modify partition",
    "set qos",
    "sudo",
    "scontrol update",
    "scontrol reboot",
)


# ---------------------------------------------------------------------------
# Fixtures
# ---------------------------------------------------------------------------


@pytest.fixture
def patch_scontrol_node(monkeypatch):
    """Factory that patches ``slurm._run_result`` for scontrol show node."""
    def factory(stdout: str, ok: bool = True, stderr: str = "") -> None:
        monkeypatch.setattr(slurm, "_run_result", lambda cmd: (stdout, ok, stderr))
    return factory


@pytest.fixture
def patch_run(monkeypatch):
    """Factory that patches ``slurm._run`` with a command-router function."""
    def factory(router):
        monkeypatch.setattr(slurm, "_run", router)
    return factory


@pytest.fixture
def patch_fetch_nodes(monkeypatch):
    """Factory that patches ``slurm.fetch_nodes`` to return a controlled list."""
    def factory(nodes):
        monkeypatch.setattr(slurm, "fetch_nodes", lambda: list(nodes))
    return factory


def _action_text(actions: list[InvestigationAction]) -> str:
    return "\n".join(f"{a.label} :: {a.detail}" for a in actions).lower()


def _make_node(
    name: str = "node01",
    state: str = "idle",
    partition: str = "gpu",
    cpus_total: str = "8",
    cpus_alloc: str = "0",
    memory_total: str = "32000",
    memory_free: str = "32000",
    load: str = "0.05",
    gpu_total: int = 0,
    gpu_alloc: int = 0,
) -> slurm.Node:
    return slurm.Node(
        name=name,
        state=state,
        partition=partition,
        cpus_total=cpus_total,
        cpus_alloc=cpus_alloc,
        memory_total=memory_total,
        memory_free=memory_free,
        load=load,
        gpu_total=gpu_total,
        gpu_alloc=gpu_alloc,
    )


def _scontrol_node_block(
    node_name: str = "node01",
    state: str = "IDLE",
    partitions: str = "gpu",
    reason: str = "",
    gres: str = "gpu:a100:4",
    features: str = "a100",
) -> str:
    parts = [
        f"NodeName={node_name}",
        f"State={state}",
        f"Partitions={partitions}",
        f"Gres={gres}",
        f"Features={features}",
        "CPUTot=8",
        "CPUAlloc=0",
        "RealMemory=32000",
        "FreeMem=32000",
        "CPULoad=0.05",
    ]
    if reason:
        parts.append(f"Reason={reason}")
    return " ".join(parts)


# ---------------------------------------------------------------------------
# Happy paths
# ---------------------------------------------------------------------------


def test_investigate_node_idle(patch_scontrol_node, patch_run, patch_fetch_nodes):
    """IDLE node: state-explanation is high confidence, no errors, safe actions."""
    patch_scontrol_node(_scontrol_node_block(state="IDLE"))
    patch_fetch_nodes([_make_node(state="idle")])

    def router(cmd: str) -> str:
        # squeue -w node01 returns no jobs
        return ""

    patch_run(router)

    report = slurm.investigate_node("node01")
    assert isinstance(report, InvestigationReport)
    assert report.target.kind == "node"
    assert report.target.identifier == "node01"
    assert report.errors == []

    # State explanation present, high confidence, mentions available/idle
    state_exps = [
        e for e in report.explanations
        if "available" in e.detail.lower() or "idle" in e.detail.lower()
    ]
    assert state_exps
    assert any(e.confidence == "high" for e in state_exps)

    # At least one safe-for-user action
    assert any(a.safe_for_user for a in report.suggested_actions)

    # No admin verbs in any action
    text = _action_text(report.suggested_actions)
    for verb in _FORBIDDEN_ADMIN_VERBS:
        assert verb not in text, f"forbidden verb {verb!r} appeared: {text}"


def test_investigate_node_mixed_with_running_jobs(
    patch_scontrol_node, patch_run, patch_fetch_nodes,
):
    """MIXED node with two visible jobs: derived free evidence + medium confidence."""
    patch_scontrol_node(_scontrol_node_block(state="MIXED"))
    patch_fetch_nodes([_make_node(
        state="mixed",
        cpus_total="8",
        cpus_alloc="4",
        gpu_total=2,
        gpu_alloc=1,
    )])

    job_lines = (
        "111|train|alice|RUNNING|gpu|1|4|0:30:00|8:00:00|None|node01|normal\n"
        "222|preprocess|bob|RUNNING|gpu|1|2|0:10:00|2:00:00|None|node01|normal\n"
    )

    def router(cmd: str) -> str:
        if "squeue" in cmd and "-w" in cmd:
            return job_lines
        return ""

    patch_run(router)

    report = slurm.investigate_node("node01")
    assert len(report.related_jobs) == 2
    assert {j.job_id for j in report.related_jobs} == {"111", "222"}

    # Derived evidence for cpus_free and gpus_free
    derived_ids = {ev.id for ev in report.evidence if ev.source == "derived"}
    assert "derived.cpus_free" in derived_ids
    assert "derived.gpus_free" in derived_ids

    # MIXED state explanation has medium confidence (SPEC sec. 8.5.1)
    mixed_exps = [
        e for e in report.explanations
        if "partial" in e.detail.lower() or "some resources" in e.detail.lower()
    ]
    assert mixed_exps
    assert any(e.confidence == "medium" for e in mixed_exps)


def test_investigate_node_drain_explains_drain(
    patch_scontrol_node, patch_run, patch_fetch_nodes,
):
    """DRAIN with reason: high confidence + scontrol.reason evidence + admin action."""
    patch_scontrol_node(_scontrol_node_block(state="DRAIN", reason="maintenance"))
    patch_fetch_nodes([_make_node(state="drain")])

    def router(cmd: str) -> str:
        return ""

    patch_run(router)

    report = slurm.investigate_node("node01")

    # DRAIN explanation has high confidence
    drain_exps = [
        e for e in report.explanations
        if "drain" in e.detail.lower() or "drain" in e.title.lower()
    ]
    assert drain_exps
    assert any(e.confidence == "high" for e in drain_exps)

    # scontrol.reason evidence has the right value
    reason_evs = [ev for ev in report.evidence if ev.id == "scontrol.reason"]
    assert reason_evs and reason_evs[0].value == "maintenance"

    # "Contact admin" suggested action surfaced (matches "admin" but no forbidden verbs)
    text = _action_text(report.suggested_actions)
    assert "admin" in text
    for verb in _FORBIDDEN_ADMIN_VERBS:
        assert verb not in text


# ---------------------------------------------------------------------------
# Partial-failure paths
# ---------------------------------------------------------------------------


def test_investigate_node_scontrol_failure_partial(
    patch_scontrol_node, patch_run, patch_fetch_nodes,
):
    """scontrol failure: error entry recorded, raw section unavailable, sinfo summary still present."""
    patch_scontrol_node("", ok=False, stderr="Permission denied")
    patch_fetch_nodes([_make_node(state="idle")])

    def router(cmd: str) -> str:
        return ""

    patch_run(router)

    report = slurm.investigate_node("node01")

    perm_errors = [
        e for e in report.errors
        if isinstance(e, InvestigationError) and e.category == "slurm_permission_denied"
    ]
    assert len(perm_errors) == 1

    assert report.raw_sections.get("scontrol show node") == "unavailable"

    # sinfo-derived summary entries are still present (State + CPUs + Memory + Load)
    labels = {item.label for item in report.summary}
    assert "State" in labels
    assert "CPUs allocated/total" in labels
    assert "Memory free/total" in labels
    assert "Load" in labels


def test_investigate_node_not_in_sinfo_snapshot(
    patch_scontrol_node, patch_run, patch_fetch_nodes,
):
    """Node missing from sinfo: node_not_found error; scontrol-derived data still flows."""
    patch_scontrol_node(_scontrol_node_block(state="IDLE"))
    patch_fetch_nodes([])  # empty sinfo snapshot

    def router(cmd: str) -> str:
        return ""

    patch_run(router)

    report = slurm.investigate_node("node01")
    nf = [e for e in report.errors if e.category == "node_not_found"]
    assert len(nf) == 1

    # scontrol-derived evidence still surfaces (e.g. partitions / gres)
    ev_ids = {ev.id for ev in report.evidence}
    assert "scontrol.partitions" in ev_ids
    assert "scontrol.gres" in ev_ids


# ---------------------------------------------------------------------------
# GPU edge cases (SPEC sec. 6.2)
# ---------------------------------------------------------------------------


def test_investigate_node_no_gpus(patch_scontrol_node, patch_run, patch_fetch_nodes):
    """gpu_total == 0: no sinfo.gpus, no derived.gpus_free, no '0/0' anywhere."""
    patch_scontrol_node(_scontrol_node_block(state="IDLE", gres="(null)"))
    patch_fetch_nodes([_make_node(state="idle", gpu_total=0, gpu_alloc=0)])

    def router(cmd: str) -> str:
        return ""

    patch_run(router)

    report = slurm.investigate_node("node01")
    ev_ids = {ev.id for ev in report.evidence}
    assert "sinfo.gpus" not in ev_ids
    assert "derived.gpus_free" not in ev_ids

    # No "0/0" rendering anywhere in summary or evidence
    summary_blob = " ".join(item.value for item in report.summary)
    evidence_blob = " ".join(ev.value for ev in report.evidence)
    assert "0/0" not in summary_blob
    assert "0/0" not in evidence_blob


# ---------------------------------------------------------------------------
# Visibility limits (SPEC sec. 8.5.2)
# ---------------------------------------------------------------------------


def test_investigate_node_allocated_with_no_visible_jobs(
    patch_scontrol_node, patch_run, patch_fetch_nodes,
):
    """ALLOCATED state with no visible jobs: explanation about visibility limits."""
    patch_scontrol_node(_scontrol_node_block(state="ALLOCATED"))
    patch_fetch_nodes([_make_node(
        state="allocated",
        cpus_total="8",
        cpus_alloc="8",
    )])

    def router(cmd: str) -> str:
        # squeue -w node01 returns no rows (jobs hidden / policy / etc.)
        return ""

    patch_run(router)

    report = slurm.investigate_node("node01")
    assert report.related_jobs == []
    visibility_exps = [
        e for e in report.explanations
        if "no matching jobs visible" in e.title.lower()
        or "hidden jobs" in e.detail.lower()
        or "reservation" in e.detail.lower()
    ]
    assert visibility_exps


# ---------------------------------------------------------------------------
# Admin-action invariant (SPEC sec. 8.5)
# ---------------------------------------------------------------------------


@pytest.mark.parametrize(
    "state",
    ["idle", "allocated", "mixed", "down", "drain", "drained", "reserved", "unknown"],
)
def test_investigate_node_never_suggests_admin_actions(
    patch_scontrol_node, patch_run, patch_fetch_nodes, state,
):
    """No suggested action label/detail may contain admin-only verbs."""
    patch_scontrol_node(_scontrol_node_block(state=state.upper()))
    patch_fetch_nodes([_make_node(state=state)])

    def router(cmd: str) -> str:
        return ""

    patch_run(router)

    report = slurm.investigate_node("node01")
    assert all(a.safe_for_user for a in report.suggested_actions)
    text = _action_text(report.suggested_actions)
    for verb in _FORBIDDEN_ADMIN_VERBS:
        assert verb not in text, (
            f"forbidden verb {verb!r} in actions for state={state!r}: {text}"
        )


# ---------------------------------------------------------------------------
# [investigation].max_related_jobs cap (SPEC §16.9 example)
# ---------------------------------------------------------------------------


def _many_job_lines(n: int, node: str = "node01") -> str:
    """Build n synthetic squeue rows in the _SQUEUE_FMT shape (12 fields)."""
    lines = []
    for i in range(n):
        lines.append(
            f"{1000 + i}|train{i}|alice|RUNNING|gpu|1|1|0:00:30|1:00:00|None|{node}|normal"
        )
    return "\n".join(lines) + "\n"


def _setup_busy_node(
    patch_scontrol_node, patch_run, patch_fetch_nodes, n_jobs: int = 30,
):
    """Common harness: MIXED node with n_jobs visible jobs."""
    patch_scontrol_node(_scontrol_node_block(state="MIXED"))
    patch_fetch_nodes([_make_node(
        state="mixed",
        cpus_total="8",
        cpus_alloc="4",
        gpu_total=2,
        gpu_alloc=1,
    )])
    job_lines = _many_job_lines(n_jobs)

    def router(cmd: str) -> str:
        if "squeue" in cmd and "-w" in cmd:
            return job_lines
        return ""

    patch_run(router)


def test_investigate_node_caps_related_jobs_at_default_20(
    temp_config, patch_scontrol_node, patch_run, patch_fetch_nodes,
):
    """Default cap is 20 (SPEC §16.9 example)."""
    _setup_busy_node(patch_scontrol_node, patch_run, patch_fetch_nodes, n_jobs=30)
    report = slurm.investigate_node("node01")
    assert len(report.related_jobs) == 20


def test_investigate_node_respects_custom_max_related_jobs(
    temp_config, patch_scontrol_node, patch_run, patch_fetch_nodes,
):
    """User-configured cap overrides the default."""
    config.update({"investigation": {"max_related_jobs": 5}})
    _setup_busy_node(patch_scontrol_node, patch_run, patch_fetch_nodes, n_jobs=30)
    report = slurm.investigate_node("node01")
    assert len(report.related_jobs) == 5


def test_investigate_node_zero_cap_disables_limit(
    temp_config, patch_scontrol_node, patch_run, patch_fetch_nodes,
):
    """Cap = 0 means include all visible jobs (no cap)."""
    config.update({"investigation": {"max_related_jobs": 0}})
    _setup_busy_node(patch_scontrol_node, patch_run, patch_fetch_nodes, n_jobs=30)
    report = slurm.investigate_node("node01")
    assert len(report.related_jobs) == 30


def test_investigate_node_negative_cap_disables_limit(
    temp_config, patch_scontrol_node, patch_run, patch_fetch_nodes,
):
    """Negative cap also disables the limit."""
    config.update({"investigation": {"max_related_jobs": -1}})
    _setup_busy_node(patch_scontrol_node, patch_run, patch_fetch_nodes, n_jobs=30)
    report = slurm.investigate_node("node01")
    assert len(report.related_jobs) == 30


def test_investigate_node_invalid_cap_falls_back_to_default(
    temp_config, patch_scontrol_node, patch_run, patch_fetch_nodes,
):
    """Malformed config value (non-int) falls back to the default cap of 20."""
    # Bypass the writer (which would reject/coerce) and write raw TOML directly.
    cfg_file = temp_config / "config.toml"
    cfg_file.write_text(
        "[investigation]\nmax_related_jobs = \"bad\"\n",
        encoding="utf-8",
    )
    _setup_busy_node(patch_scontrol_node, patch_run, patch_fetch_nodes, n_jobs=30)
    report = slurm.investigate_node("node01")
    assert len(report.related_jobs) == 20


def test_investigate_node_cap_does_not_affect_evidence(
    temp_config, patch_scontrol_node, patch_run, patch_fetch_nodes,
):
    """Cap trims related_jobs but leaves derived evidence (cpus_free etc.) intact."""
    config.update({"investigation": {"max_related_jobs": 5}})
    _setup_busy_node(patch_scontrol_node, patch_run, patch_fetch_nodes, n_jobs=30)
    report = slurm.investigate_node("node01")
    assert len(report.related_jobs) == 5
    derived = {ev.id: ev.value for ev in report.evidence if ev.source == "derived"}
    # cpus_total=8, cpus_alloc=4 -> 4 free / 8 total, regardless of cap.
    assert derived.get("derived.cpus_free") == "4/8"
    # gpu_total=2, gpu_alloc=1 -> 1 free / 2 total.
    assert derived.get("derived.gpus_free") == "1/2"

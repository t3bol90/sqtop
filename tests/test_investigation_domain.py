"""Tests for the investigation domain module (SPEC sec. 6.8-6.12, 8, 21, 22).

These tests are pure: no subprocess, no config I/O, no fixtures from
``conftest.py``. They cover the dataclasses, the two explanation
helpers, and the plain-text report renderer.
"""
from __future__ import annotations

import dataclasses
from datetime import datetime

import pytest

from sqtop.investigation import (
    InvestigationAction,
    InvestigationError,
    InvestigationEvidence,
    InvestigationExplanation,
    InvestigationItem,
    InvestigationReport,
    InvestigationTarget,
    explain_node_state,
    explain_pending_reason,
    register_user_reasons,
    render_report,
)
from sqtop.slurm import Job, Node


# Defensive: ensure no other test file leaked _USER_REASONS state into ours.
# Mirrors the autouse fixture in test_investigation_user_reasons.py.
@pytest.fixture(autouse=True)
def _reset_user_reasons():
    register_user_reasons({})
    yield
    register_user_reasons({})


# ---------------------------------------------------------------------------
# Domain types
# ---------------------------------------------------------------------------


def test_investigation_target_round_trips_fields():
    t = InvestigationTarget(kind="job", identifier="12345", source="cursor")
    assert t.kind == "job"
    assert t.identifier == "12345"
    assert t.source == "cursor"


def test_investigation_target_node_kind():
    t = InvestigationTarget(kind="node", identifier="gpu-a100-02", source="typed")
    assert t.kind == "node"
    assert t.identifier == "gpu-a100-02"


def test_investigation_report_constructs_with_empty_defaults():
    target = InvestigationTarget(kind="job", identifier="1", source="cursor")
    now = datetime(2026, 5, 8, 10, 14, 0)
    report = InvestigationReport(target=target, generated_at=now)
    assert report.summary == []
    assert report.evidence == []
    assert report.explanations == []
    assert report.related_jobs == []
    assert report.related_nodes == []
    assert report.suggested_actions == []
    assert report.raw_sections == {}
    assert report.errors == []


def test_investigation_target_is_frozen():
    t = InvestigationTarget(kind="job", identifier="1", source="cursor")
    with pytest.raises(dataclasses.FrozenInstanceError):
        t.identifier = "2"  # type: ignore[misc]


def test_investigation_evidence_is_frozen():
    ev = InvestigationEvidence(
        id="e1", label="reason", value="Resources",
        source="squeue", confidence="medium",
    )
    with pytest.raises(dataclasses.FrozenInstanceError):
        ev.value = "Other"  # type: ignore[misc]


def test_investigation_explanation_is_frozen():
    exp = InvestigationExplanation(
        title="t", detail="d", confidence="high",
    )
    with pytest.raises(dataclasses.FrozenInstanceError):
        exp.title = "u"  # type: ignore[misc]


def test_investigation_action_is_frozen():
    a = InvestigationAction(label="Watch", detail="watch this job", safe_for_user=True)
    with pytest.raises(dataclasses.FrozenInstanceError):
        a.safe_for_user = False  # type: ignore[misc]


def test_investigation_item_is_frozen():
    i = InvestigationItem(label="State", value="PENDING")
    with pytest.raises(dataclasses.FrozenInstanceError):
        i.value = "RUNNING"  # type: ignore[misc]


def test_investigation_error_is_frozen():
    err = InvestigationError(
        source="squeue", category="timeout", message="boom", stderr=None,
    )
    with pytest.raises(dataclasses.FrozenInstanceError):
        err.message = "other"  # type: ignore[misc]


def test_investigation_explanation_evidence_refs_default_is_tuple():
    exp = InvestigationExplanation(title="t", detail="d", confidence="high")
    assert exp.evidence_refs == ()
    assert isinstance(exp.evidence_refs, tuple)


# ---------------------------------------------------------------------------
# explain_pending_reason — SPEC sec. 8.4.1
# ---------------------------------------------------------------------------


def test_explain_pending_reason_resources():
    exp = explain_pending_reason("Resources")
    assert exp.confidence == "medium"
    text = (exp.title + " " + exp.detail).lower()
    assert "resource" in text


def test_explain_pending_reason_priority():
    exp = explain_pending_reason("Priority")
    assert exp.confidence == "high"
    assert "priority" in (exp.title + " " + exp.detail).lower()


def test_explain_pending_reason_dependency():
    exp = explain_pending_reason("Dependency")
    assert exp.confidence == "high"
    assert "depend" in (exp.title + " " + exp.detail).lower()


def test_explain_pending_reason_req_node_not_avail():
    exp = explain_pending_reason("ReqNodeNotAvail")
    assert exp.confidence == "high"
    assert "node" in (exp.title + " " + exp.detail).lower()


def test_explain_pending_reason_partition_time_limit():
    exp = explain_pending_reason("PartitionTimeLimit")
    assert exp.confidence == "high"
    text = (exp.title + " " + exp.detail).lower()
    assert "time" in text and "partition" in text


def test_explain_pending_reason_job_held_user():
    exp = explain_pending_reason("JobHeldUser")
    assert exp.confidence == "high"
    text = (exp.title + " " + exp.detail).lower()
    assert "held" in text and "user" in text


def test_explain_pending_reason_job_held_admin():
    exp = explain_pending_reason("JobHeldAdmin")
    assert exp.confidence == "high"
    text = (exp.title + " " + exp.detail).lower()
    assert "admin" in text or "administrator" in text


def test_explain_pending_reason_begin_time():
    exp = explain_pending_reason("BeginTime")
    assert exp.confidence == "high"
    text = (exp.title + " " + exp.detail).lower()
    assert "begin" in text or "future" in text


def test_explain_pending_reason_reservation():
    exp = explain_pending_reason("Reservation")
    assert exp.confidence == "medium"
    assert "reservation" in (exp.title + " " + exp.detail).lower()


def test_explain_pending_reason_licenses():
    exp = explain_pending_reason("Licenses")
    assert exp.confidence == "medium"
    assert "licens" in (exp.title + " " + exp.detail).lower()


def test_explain_pending_reason_qos_max_cpu():
    exp = explain_pending_reason("QOSMaxCpuPerUserLimit")
    assert exp.confidence == "medium"
    text = (exp.title + " " + exp.detail).lower()
    assert "qos" in text and "cpu" in text


def test_explain_pending_reason_qos_max_gres():
    exp = explain_pending_reason("QOSMaxGRESPerUser")
    assert exp.confidence == "medium"
    text = (exp.title + " " + exp.detail).lower()
    assert "qos" in text and ("gres" in text or "gpu" in text)


def test_explain_pending_reason_assoc_grp_cpu():
    exp = explain_pending_reason("AssocGrpCpuLimit")
    assert exp.confidence == "medium"
    text = (exp.title + " " + exp.detail).lower()
    assert ("association" in text or "assoc" in text or "group" in text) and "cpu" in text


def test_explain_pending_reason_assoc_grp_gres():
    exp = explain_pending_reason("AssocGrpGRES")
    assert exp.confidence == "medium"
    text = (exp.title + " " + exp.detail).lower()
    assert ("association" in text or "assoc" in text or "group" in text)
    assert "gres" in text or "gpu" in text


def test_explain_pending_reason_empty_string():
    exp = explain_pending_reason("")
    assert exp.confidence == "low"
    assert "no pending reason" in exp.title.lower()


def test_explain_pending_reason_null_sentinel():
    exp = explain_pending_reason("(null)")
    assert exp.confidence == "low"
    assert "no pending reason" in exp.title.lower()


def test_explain_pending_reason_none():
    exp = explain_pending_reason(None)  # type: ignore[arg-type]
    assert exp.confidence == "low"
    assert "no pending reason" in exp.title.lower()


def test_explain_pending_reason_unknown_reason():
    exp = explain_pending_reason("SomethingNew")
    assert exp.confidence == "low"
    assert "unrecognized" in exp.title.lower()
    assert "SomethingNew" in exp.detail


def test_explain_pending_reason_lookup_is_case_sensitive():
    # Lowercase variants should NOT match the canonical capitalized keys.
    exp = explain_pending_reason("resources")
    assert exp.confidence == "low"
    assert "unrecognized" in exp.title.lower()


def test_existing_explain_pending_reason_unaffected_when_user_reasons_empty():
    """Documents the contract: an empty _USER_REASONS map (the default) must
    not change behavior of explain_pending_reason() for any of the built-in
    reason keys, the unknown-fallback path, or the empty/null path.

    The 14 reason tests above already imply this; this test makes the
    contract explicit and is the regression guard for the override path
    added in PR 8.
    """
    # Built-in: Resources -> medium.
    assert explain_pending_reason("Resources").confidence == "medium"
    # Built-in: Priority -> high.
    assert explain_pending_reason("Priority").confidence == "high"
    # Unknown -> low + "unrecognized".
    unk = explain_pending_reason("DefinitelyNotAReason")
    assert unk.confidence == "low"
    assert "unrecognized" in unk.title.lower()
    # Empty -> low + "no pending reason".
    null = explain_pending_reason("")
    assert null.confidence == "low"
    assert "no pending reason" in null.title.lower()


# ---------------------------------------------------------------------------
# explain_node_state — SPEC sec. 8.5.1
# ---------------------------------------------------------------------------


def test_explain_node_state_idle():
    exp = explain_node_state("IDLE")
    assert exp.confidence == "high"
    assert "available" in exp.detail.lower()


def test_explain_node_state_allocated():
    exp = explain_node_state("ALLOCATED")
    assert exp.confidence == "high"
    assert "allocated" in exp.detail.lower()


def test_explain_node_state_mixed():
    exp = explain_node_state("MIXED")
    assert exp.confidence == "medium"
    assert "allocated" in exp.detail.lower()


def test_explain_node_state_down():
    exp = explain_node_state("DOWN")
    assert exp.confidence == "high"
    assert "unavailable" in exp.detail.lower()


def test_explain_node_state_drain():
    exp = explain_node_state("DRAIN")
    assert exp.confidence == "high"
    assert "drain" in exp.detail.lower()


def test_explain_node_state_drained():
    exp = explain_node_state("DRAINED")
    assert exp.confidence == "high"
    assert "drain" in exp.detail.lower()


def test_explain_node_state_reserved():
    exp = explain_node_state("RESERVED")
    assert exp.confidence == "medium"
    assert "reserv" in exp.detail.lower()


def test_explain_node_state_strips_asterisk_and_lowercase():
    exp = explain_node_state("idle*")
    assert exp.confidence == "high"
    assert "available" in exp.detail.lower()


def test_explain_node_state_strips_dash_with_uppercase():
    exp = explain_node_state("MIXED-")
    assert exp.confidence == "medium"
    assert "allocated" in exp.detail.lower()


def test_explain_node_state_compound_idle_plus_drain():
    exp = explain_node_state("idle+drain")
    # SPEC: compound + drain -> DRAIN with medium confidence
    assert exp.confidence == "medium"
    assert "drain" in exp.detail.lower()


def test_explain_node_state_empty_returns_unknown():
    exp = explain_node_state("")
    assert exp.confidence == "low"


def test_explain_node_state_weird_returns_unknown():
    exp = explain_node_state("WEIRD")
    assert exp.confidence == "low"


# ---------------------------------------------------------------------------
# render_report — SPEC sec. 21 / 22
# ---------------------------------------------------------------------------


def _empty_report(kind: str = "job", identifier: str = "1") -> InvestigationReport:
    target = InvestigationTarget(
        kind=kind,  # type: ignore[arg-type]
        identifier=identifier,
        source="cursor",
    )
    return InvestigationReport(
        target=target,
        generated_at=datetime(2026, 5, 8, 10, 14, 0),
    )


def test_render_report_empty_does_not_crash():
    out = render_report(_empty_report())
    assert "Investigate Job 1" in out


def test_render_report_node_header():
    out = render_report(_empty_report(kind="node", identifier="gpu-a100-02"))
    assert "Investigate Node gpu-a100-02" in out


def test_render_report_job_with_summary_evidence_explanation_action():
    report = _empty_report(kind="job", identifier="123456")
    report.summary.append(InvestigationItem(label="State", value="PENDING"))
    report.summary.append(InvestigationItem(label="Reason", value="Resources"))
    report.evidence.append(InvestigationEvidence(
        id="e1", label="squeue reason", value="Resources",
        source="squeue", confidence="high",
    ))
    report.explanations.append(InvestigationExplanation(
        title="Matching resources are not currently available",
        detail="Slurm reports that matching resources are not currently available.",
        confidence="medium",
    ))
    report.suggested_actions.append(InvestigationAction(
        label="Watch this job", detail="watch the job for state changes",
        safe_for_user=True,
    ))

    out = render_report(report)
    assert "Summary" in out
    assert "- State: PENDING" in out
    assert "Slurm evidence" in out
    assert "- squeue reason: Resources" in out
    assert "Likely explanation" in out
    assert "Confidence: medium" in out
    assert "Suggested next actions" in out
    assert "Watch this job" in out


def test_render_report_node_with_related_nodes():
    report = _empty_report(kind="node", identifier="gpu-a100-02")
    report.related_nodes.append(Node(
        name="gpu-a100-01", state="ALLOCATED", partition="gpu",
        cpus_total="64", cpus_alloc="64",
        memory_total="512000", memory_free="0",
    ))
    report.related_nodes.append(Node(
        name="gpu-a100-03", state="DRAIN", partition="gpu",
        cpus_total="64", cpus_alloc="0",
        memory_total="512000", memory_free="512000",
    ))
    out = render_report(report)
    assert "Related nodes" in out
    assert "- gpu-a100-01: ALLOCATED" in out
    assert "- gpu-a100-03: DRAIN" in out


def test_render_report_errors_rendered_last():
    report = _empty_report()
    report.summary.append(InvestigationItem(label="State", value="PENDING"))
    report.errors.append(InvestigationError(
        source="sacct", category="permission",
        message="not allowed",
    ))
    out = render_report(report)
    err_idx = out.index("Errors")
    summary_idx = out.index("Summary")
    assert summary_idx < err_idx
    assert "- sacct [permission]: not allowed" in out


def test_render_report_is_byte_deterministic():
    report = _empty_report(kind="job", identifier="42")
    report.summary.append(InvestigationItem(label="State", value="PENDING"))
    report.evidence.append(InvestigationEvidence(
        id="e1", label="squeue reason", value="Priority",
        source="squeue", confidence="high",
    ))
    report.explanations.append(InvestigationExplanation(
        title="Lower priority",
        detail="Job is eligible but lower priority than other queued jobs.",
        confidence="high",
    ))
    a = render_report(report)
    b = render_report(report)
    assert a == b


def test_render_report_no_rich_markup():
    report = _empty_report()
    report.summary.append(InvestigationItem(label="State", value="PENDING"))
    report.evidence.append(InvestigationEvidence(
        id="e1", label="reason", value="Resources",
        source="squeue", confidence="high",
    ))
    out = render_report(report)
    for tag in ("[red]", "[green]", "[bold]", "[yellow]", "[blue]", "[/]"):
        assert tag not in out


def test_render_report_skips_empty_sections():
    report = _empty_report()
    out = render_report(report)
    # Only the header should appear; no section headers like "Summary".
    assert "Summary" not in out
    assert "Slurm evidence" not in out
    assert "Likely explanation" not in out
    assert "Suggested next actions" not in out
    assert "Raw detail" not in out
    assert "Errors" not in out


def test_render_report_derived_evidence_has_confidence_tag():
    report = _empty_report()
    report.evidence.append(InvestigationEvidence(
        id="e1", label="visible free GPUs", value="1",
        source="derived", confidence="medium",
    ))
    out = render_report(report)
    assert "[medium]" in out


def test_render_report_squeue_evidence_no_confidence_tag():
    report = _empty_report()
    report.evidence.append(InvestigationEvidence(
        id="e1", label="reason", value="Resources",
        source="squeue", confidence="high",
    ))
    out = render_report(report)
    # squeue source should not get a [confidence] tag suffix
    assert "[high]" not in out


def test_render_report_raw_sections_default_to_available():
    report = _empty_report()
    report.raw_sections["scontrol show job"] = ""
    report.raw_sections["sacct"] = "unavailable on this cluster"
    out = render_report(report)
    assert "- scontrol show job: available" in out
    assert "- sacct: unavailable on this cluster" in out


def test_render_report_related_jobs():
    report = _empty_report(kind="node", identifier="gpu-a100-02")
    report.related_jobs.append(Job(
        job_id="12345", name="train", user="alice", state="RUNNING",
        partition="gpu", nodes="1", num_nodes="1", num_cpus="8",
        time_used="0:10:00", time_limit="24:00:00",
    ))
    out = render_report(report)
    assert "Related jobs" in out
    assert "- 12345: RUNNING" in out

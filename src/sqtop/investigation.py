"""Investigation Mode domain types and pure helpers.

This module owns the SPEC sec. 6.8-6.12 dataclasses plus the pure
explanation/render helpers used by Investigation Mode (SPEC sec. 8).

It MUST stay free of subprocess, ssh, config I/O, or any view code.
The builders here take already-fetched Slurm data and turn it into a
plain-text-ready report.
"""
from __future__ import annotations

import tomllib
from dataclasses import dataclass, field
from datetime import datetime
from pathlib import Path
from typing import Literal

# Re-import existing data types so callers do not need two imports.
from .slurm import Job, Node

__all__ = [
    "InvestigationKind",
    "InvestigationSource",
    "EvidenceSource",
    "Confidence",
    "InvestigationTarget",
    "InvestigationEvidence",
    "InvestigationExplanation",
    "InvestigationError",
    "InvestigationAction",
    "InvestigationItem",
    "InvestigationReport",
    "explain_pending_reason",
    "explain_node_state",
    "render_report",
    "load_user_reasons",
    "register_user_reasons",
]

InvestigationKind = Literal["job", "node"]
InvestigationSource = Literal["cursor", "typed", "related_link", "watch"]
EvidenceSource = Literal["squeue", "sinfo", "scontrol", "sacct", "derived", "cache"]
Confidence = Literal["high", "medium", "low"]


# ---------------------------------------------------------------------------
# Domain dataclasses (SPEC sec. 6.8 - 6.12)
# ---------------------------------------------------------------------------


@dataclass(frozen=True)
class InvestigationTarget:
    kind: InvestigationKind
    identifier: str
    source: InvestigationSource


@dataclass(frozen=True)
class InvestigationEvidence:
    id: str
    label: str
    value: str
    source: EvidenceSource
    confidence: Confidence


@dataclass(frozen=True)
class InvestigationExplanation:
    title: str
    detail: str
    confidence: Confidence
    # Tuple, not list, because the dataclass is frozen and we want
    # explanations to be safely shareable / hashable.
    evidence_refs: tuple[str, ...] = ()


@dataclass(frozen=True)
class InvestigationError:
    source: str
    category: str
    message: str
    stderr: str | None = None


@dataclass(frozen=True)
class InvestigationAction:
    label: str           # short user-facing verb, e.g. "Watch this job"
    detail: str          # one-line explanation
    safe_for_user: bool  # MUST be True for any action shown to non-admin users


@dataclass(frozen=True)
class InvestigationItem:
    label: str
    value: str


@dataclass
class InvestigationReport:
    """Mutable container so builders can incrementally fill sections.

    Lists default to empty so a partially-populated report still renders
    without crashing. Insertion order on `raw_sections` is preserved by
    the standard `dict` (Python 3.7+ guarantee).
    """

    target: InvestigationTarget
    generated_at: datetime
    summary: list[InvestigationItem] = field(default_factory=list)
    evidence: list[InvestigationEvidence] = field(default_factory=list)
    explanations: list[InvestigationExplanation] = field(default_factory=list)
    related_jobs: list[Job] = field(default_factory=list)
    related_nodes: list[Node] = field(default_factory=list)
    suggested_actions: list[InvestigationAction] = field(default_factory=list)
    raw_sections: dict[str, str] = field(default_factory=dict)
    errors: list[InvestigationError] = field(default_factory=list)


# ---------------------------------------------------------------------------
# Pending-reason explanation table (SPEC sec. 8.4.1)
# ---------------------------------------------------------------------------

# Tuples of (title, detail, confidence). Lookup is case-sensitive on the
# Slurm-reported reason, since the keys are the canonical Slurm strings.
_PENDING_REASONS: dict[str, tuple[str, str, Confidence]] = {
    "Resources": (
        "Matching resources are not currently available",
        (
            "Slurm cannot currently find enough matching resources. "
            "Check requested CPUs/GPUs/memory, partition, and node availability."
        ),
        "medium",
    ),
    "Priority": (
        "Lower priority than other queued jobs",
        "Job is eligible but lower priority than other queued jobs.",
        "high",
    ),
    "Dependency": (
        "Waiting on a dependency",
        "Job is waiting for another job or condition.",
        "high",
    ),
    "ReqNodeNotAvail": (
        "Requested node is unavailable",
        (
            "Requested node is unavailable, drained, down, reserved, "
            "or otherwise not schedulable."
        ),
        "high",
    ),
    "PartitionTimeLimit": (
        "Time limit exceeds partition limit",
        "Requested time exceeds partition limit.",
        "high",
    ),
    "JobHeldUser": (
        "Held by the user",
        "Job is held by the user.",
        "high",
    ),
    "JobHeldAdmin": (
        "Held by an administrator",
        "Job is held by an administrator or policy.",
        "high",
    ),
    "BeginTime": (
        "Future begin time",
        "Job has a future begin time.",
        "high",
    ),
    "Reservation": (
        "Waiting for reservation constraints",
        "Job is waiting for reservation constraints.",
        "medium",
    ),
    "Licenses": (
        "Required licenses unavailable",
        "Required license resources are unavailable.",
        "medium",
    ),
    "QOSMaxCpuPerUserLimit": (
        "QoS CPU-per-user limit may be blocking",
        "Visible QoS CPU-per-user limit may be blocking the job.",
        "medium",
    ),
    "QOSMaxGRESPerUser": (
        "QoS GRES/GPU-per-user limit may be blocking",
        "Visible QoS GRES/GPU-per-user limit may be blocking the job.",
        "medium",
    ),
    "AssocGrpCpuLimit": (
        "Association/group CPU limit may be blocking",
        "Association/group CPU limit may be blocking the job.",
        "medium",
    ),
    "AssocGrpGRES": (
        "Association/group GRES/GPU limit may be blocking",
        "Association/group GRES/GPU limit may be blocking the job.",
        "medium",
    ),
}


# ---------------------------------------------------------------------------
# Site-supplied pending-reason overrides (SPEC §20.3)
# ---------------------------------------------------------------------------

# Module-level mutable state for user-supplied reason overrides.
# Empty by default. Replaced wholesale by register_user_reasons().
# Mirrors the existing module-level state pattern used in slurm.py
# (_SSH_HOST, _SSH_KEY, _COMMAND_HISTORY).
_USER_REASONS: dict[str, "InvestigationExplanation"] = {}


def register_user_reasons(reasons: dict[str, "InvestigationExplanation"]) -> None:
    """Replace the user-supplied reason map.

    Pass an empty dict to clear. Subsequent ``explain_pending_reason()``
    calls consult the user map first, falling back to the built-in
    ``_PENDING_REASONS``, then to the unknown-reason default.
    """
    global _USER_REASONS
    _USER_REASONS = dict(reasons)


def load_user_reasons(path: str | Path | None) -> dict[str, "InvestigationExplanation"]:
    """Read a TOML file describing pending-reason overrides.

    File format::

        [SiteSpecificFoo]
        title = "Site-specific foo"
        detail = "Foo is unavailable due to local cluster policy."
        confidence = "medium"

        [AnotherReason]
        title = "..."
        detail = "..."
        confidence = "high"

    Each top-level table key becomes a Slurm reason string. Each table
    must define ``title``, ``detail``, and ``confidence`` (one of
    "high"/"medium"/"low").

    Returns a dict mapping reason -> InvestigationExplanation. On any
    I/O error, malformed TOML, missing required field, or invalid
    confidence value, the offending entry is skipped silently and the
    remaining entries are returned. An empty path or None returns
    ``{}``. A missing file returns ``{}``. The function never raises
    on these paths; callers can rely on degraded-mode behavior.
    """
    if not path:
        return {}
    p = Path(path).expanduser()
    if not p.is_file():
        return {}
    try:
        with p.open("rb") as f:
            data = tomllib.load(f)
    except (tomllib.TOMLDecodeError, OSError):
        return {}

    valid_confidences: set[str] = {"high", "medium", "low"}
    result: dict[str, InvestigationExplanation] = {}
    for reason_key, fields in data.items():
        if not isinstance(reason_key, str) or not reason_key:
            continue
        if not isinstance(fields, dict):
            continue
        title = fields.get("title")
        detail = fields.get("detail")
        confidence = fields.get("confidence")
        if not isinstance(title, str) or not isinstance(detail, str):
            continue
        if confidence not in valid_confidences:
            continue
        result[reason_key] = InvestigationExplanation(
            title=title,
            detail=detail,
            confidence=confidence,  # type: ignore[arg-type]
            evidence_refs=(),
        )
    return result


def explain_pending_reason(reason: str | None) -> InvestigationExplanation:
    """Map a Slurm pending reason to a user-facing explanation.

    SPEC sec. 8.4.1. Pure function; case-sensitive lookup.
    Empty / None / "(null)" reasons return a low-confidence
    "no reason reported" explanation. Unknown reasons echo the raw
    string so the user can still copy/paste it into a search.

    Site-supplied overrides registered via ``register_user_reasons()``
    take precedence over the built-in ``_PENDING_REASONS`` map.
    """
    # "(null)" is a Slurm sentinel for "field not provided"; we treat
    # it the same as an empty reason per SPEC.
    if reason is None or reason == "" or reason == "(null)":
        return InvestigationExplanation(
            title="No pending reason reported",
            detail=(
                "Slurm did not report a reason. The job may be very recently "
                "submitted, or the field is unavailable."
            ),
            confidence="low",
        )

    user_entry = _USER_REASONS.get(reason)
    if user_entry is not None:
        return user_entry

    entry = _PENDING_REASONS.get(reason)
    if entry is None:
        return InvestigationExplanation(
            title="Unrecognized pending reason",
            detail=(
                "sqtop does not have a built-in explanation for this "
                f"pending reason yet.\nRaw Slurm reason: {reason}"
            ),
            confidence="low",
        )

    title, detail, confidence = entry
    return InvestigationExplanation(
        title=title,
        detail=detail,
        confidence=confidence,
    )


# ---------------------------------------------------------------------------
# Node-state explanation table (SPEC sec. 8.5.1)
# ---------------------------------------------------------------------------

# Slurm decorates state strings with `*` (not responding), `-` (drain),
# `+` (cloud/power-saving), `~` (powering down), and so on. We strip
# these before lookup so e.g. "idle*" still matches IDLE.
_NODE_STATE_SUFFIXES = "*-+~#@!%$"

_NODE_STATES: dict[str, tuple[str, str, Confidence]] = {
    "IDLE": (
        "Node appears available",
        "Node appears available for matching jobs.",
        "high",
    ),
    "ALLOCATED": (
        "Node fully allocated",
        "Node is fully allocated to running jobs.",
        "high",
    ),
    "MIXED": (
        "Node partially allocated",
        "Some resources are allocated; some may remain free.",
        "medium",
    ),
    "DOWN": (
        "Node unavailable",
        "Node is unavailable.",
        "high",
    ),
    "DRAIN": (
        "Node draining or drained",
        "Node is being removed from scheduling or already drained.",
        "high",
    ),
    "DRAINED": (
        "Node draining or drained",
        "Node is being removed from scheduling or already drained.",
        "high",
    ),
    "RESERVED": (
        "Node reserved",
        "Node may be reserved for specific users, jobs, accounts, or reservations.",
        "medium",
    ),
}

_UNKNOWN_NODE_STATE = (
    "Unrecognized node state",
    "sqtop cannot confidently classify this node state.",
    "low",
)


def _normalize_node_state(state: str) -> str:
    """Strip Slurm decoration suffixes and uppercase the bare token."""
    s = state.strip()
    # Drop trailing decoration characters like '*', '-', '+'.
    while s and s[-1] in _NODE_STATE_SUFFIXES:
        s = s[:-1]
    return s.upper()


def explain_node_state(state: str) -> InvestigationExplanation:
    """Map a Slurm node-state token to a user-facing explanation.

    SPEC sec. 8.5.1. Strips trailing decoration suffixes ('*', '-', '+').
    Lookup is case-insensitive on the bare state token. Compound states
    joined with '+' (e.g. "idle+drain") are detected as DRAIN with
    medium confidence; otherwise we fall through to UNKNOWN.
    """
    if state is None or state.strip() == "":
        return InvestigationExplanation(
            title=_UNKNOWN_NODE_STATE[0],
            detail=_UNKNOWN_NODE_STATE[1],
            confidence=_UNKNOWN_NODE_STATE[2],
        )

    raw = state.strip().upper()

    # Compound states like "IDLE+DRAIN" / "MIXED+DRAIN" — treat as
    # DRAIN with reduced confidence since drain dominates schedulability.
    # We split on '+' before suffix stripping because '+' is also a
    # decoration suffix; here we use it as a separator only when it
    # joins two recognizable tokens.
    if "+" in raw:
        parts = [p.strip() for p in raw.split("+") if p.strip()]
        normalized_parts = {_normalize_node_state(p) for p in parts}
        if "DRAIN" in normalized_parts or "DRAINED" in normalized_parts:
            title, detail, _ = _NODE_STATES["DRAIN"]
            return InvestigationExplanation(
                title=title,
                detail=detail,
                confidence="medium",
            )

    key = _normalize_node_state(raw)
    entry = _NODE_STATES.get(key)
    if entry is None:
        return InvestigationExplanation(
            title=_UNKNOWN_NODE_STATE[0],
            detail=_UNKNOWN_NODE_STATE[1],
            confidence=_UNKNOWN_NODE_STATE[2],
        )

    title, detail, confidence = entry
    return InvestigationExplanation(
        title=title,
        detail=detail,
        confidence=confidence,
    )


# ---------------------------------------------------------------------------
# Plain-text report renderer (SPEC sec. 21, 22)
# ---------------------------------------------------------------------------


def _header_for_target(target: InvestigationTarget) -> str:
    if target.kind == "job":
        return f"Investigate Job {target.identifier}"
    return f"Investigate Node {target.identifier}"


def _format_evidence_line(ev: InvestigationEvidence) -> str:
    # Derived items get a confidence tag suffix so the user can tell
    # them apart from raw Slurm-reported fields.
    if ev.source == "derived":
        return f"- {ev.label}: {ev.value} [{ev.confidence}]"
    return f"- {ev.label}: {ev.value}"


def render_report(report: InvestigationReport) -> str:
    """Render a plain-text, copy-friendly investigation report.

    SPEC sec. 21 (job example) and sec. 22 (node example). Sections are
    skipped entirely when empty so partial reports stay readable.
    The output is deterministic (no timestamps, no set-iteration);
    same input renders byte-for-byte identically.
    """
    lines: list[str] = []
    lines.append(_header_for_target(report.target))

    if report.summary:
        lines.append("")
        lines.append("Summary")
        for item in report.summary:
            lines.append(f"- {item.label}: {item.value}")

    if report.evidence:
        lines.append("")
        lines.append("Slurm evidence")
        for ev in report.evidence:
            lines.append(_format_evidence_line(ev))

    if report.explanations:
        for exp in report.explanations:
            lines.append("")
            lines.append("Likely explanation")
            lines.append(f"- {exp.detail}")
            lines.append(f"Confidence: {exp.confidence}")

    if report.related_jobs:
        lines.append("")
        lines.append("Related jobs")
        for job in report.related_jobs:
            lines.append(f"- {job.job_id}: {job.state}")

    if report.related_nodes:
        lines.append("")
        lines.append("Related nodes")
        for node in report.related_nodes:
            lines.append(f"- {node.name}: {node.state}")

    if report.suggested_actions:
        lines.append("")
        lines.append("Suggested next actions")
        for action in report.suggested_actions:
            lines.append(f"- {action.label} - {action.detail}")

    if report.raw_sections:
        lines.append("")
        lines.append("Raw detail")
        for key, value in report.raw_sections.items():
            text = value if value else "available"
            lines.append(f"- {key}: {text}")

    if report.errors:
        lines.append("")
        lines.append("Errors")
        for err in report.errors:
            lines.append(f"- {err.source} [{err.category}]: {err.message}")

    # Trailing newline keeps output friendly when piped to clipboard.
    return "\n".join(lines) + "\n"

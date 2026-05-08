"""Tests for the investigation screens and the I keybind wiring.

Covers PR 3b (JobInvestigationScreen + JobsView ``I``) and PR 4-ui
(NodeInvestigationScreen + NodesView ``I``).
"""
from __future__ import annotations

import shutil
from datetime import datetime
from unittest.mock import patch

import pytest

from sqtop import slurm as slurm_mod
from sqtop.investigation import (
    InvestigationAction,
    InvestigationError,
    InvestigationEvidence,
    InvestigationExplanation,
    InvestigationItem,
    InvestigationReport,
    InvestigationTarget,
)
from sqtop.slurm import Job


# ---------------------------------------------------------------------------
# Helpers
# ---------------------------------------------------------------------------


def _make_app(width: int = 120, height: int = 30):
    """Instantiate SqtopApp with a mocked terminal size."""
    from sqtop.app import SqtopApp

    fake_size = shutil.os.terminal_size((width, height))
    with patch("sqtop.app.shutil.get_terminal_size", return_value=fake_size):
        return SqtopApp()


def _job(job_id: str = "12345", name: str = "train-a100") -> Job:
    return Job(
        job_id=job_id,
        name=name,
        user="alice",
        state="PENDING",
        partition="gpu",
        nodes="1",
        num_nodes="1",
        num_cpus="16",
        time_used="00:00:00",
        time_limit="24:00:00",
        reason="Resources",
        nodelist="",
        qos="normal",
    )


def _fake_report(
    job_id: str = "12345",
    *,
    with_error: bool = False,
) -> InvestigationReport:
    """Build a minimally-populated InvestigationReport for tests.

    Includes one item per major section so the renderer emits all the
    SPEC sec. 21 layout markers ("Summary", "Slurm evidence",
    "Likely explanation", "Suggested next actions").
    """
    target = InvestigationTarget(kind="job", identifier=job_id, source="typed")
    report = InvestigationReport(
        target=target,
        generated_at=datetime(2026, 5, 8, 10, 14, 0),
    )
    report.summary.append(InvestigationItem(label="State", value="PENDING"))
    report.evidence.append(
        InvestigationEvidence(
            id="squeue.reason",
            label="squeue reason",
            value="Resources",
            source="squeue",
            confidence="high",
        )
    )
    report.explanations.append(
        InvestigationExplanation(
            title="Matching resources are not currently available",
            detail=(
                "Slurm cannot currently find enough matching resources. "
                "Check requested CPUs/GPUs/memory, partition, and node availability."
            ),
            confidence="medium",
        )
    )
    report.suggested_actions.append(
        InvestigationAction(
            label="Watch this job",
            detail="Get notified when state changes",
            safe_for_user=True,
        )
    )
    if with_error:
        report.errors.append(
            InvestigationError(
                source="scontrol",
                category="slurm_permission_denied",
                message="scontrol show job 12345 failed",
                stderr="permission denied",
            )
        )
    return report


# ---------------------------------------------------------------------------
# Modal screen rendering
# ---------------------------------------------------------------------------


async def test_investigate_screen_renders_report_text(monkeypatch):
    """Mounting the screen runs the worker and populates the TextArea."""
    from sqtop.views import investigate as investigate_mod
    from sqtop.views.investigate import JobInvestigationScreen
    from textual.widgets import TextArea

    monkeypatch.setattr(
        investigate_mod, "investigate_job", lambda jid: _fake_report(jid)
    )

    app = _make_app(120, 30)
    async with app.run_test(size=(120, 30)) as pilot:
        await pilot.pause()
        await pilot.app.push_screen(JobInvestigationScreen("12345"))
        # Drain the worker thread.
        await pilot.app.workers.wait_for_complete()
        await pilot.pause()

        screen = pilot.app.screen
        assert isinstance(screen, JobInvestigationScreen)
        ta = screen.query_one(TextArea)
        text = ta.text
        # Header line from render_report.
        assert "Investigate Job 12345" in text
        # Section header for the explanation.
        assert "Likely explanation" in text
        # render_report emits exp.detail under the explanation header.
        assert "Slurm cannot currently find enough matching resources" in text


async def test_investigate_screen_copy_report_yields_plain_text(monkeypatch):
    """copy_pane returns a plain-text payload with the SPEC sec. 21 section markers."""
    from sqtop.views import investigate as investigate_mod
    from sqtop.views.investigate import JobInvestigationScreen

    monkeypatch.setattr(
        investigate_mod, "investigate_job", lambda jid: _fake_report(jid)
    )

    app = _make_app(120, 30)
    async with app.run_test(size=(120, 30)) as pilot:
        await pilot.pause()
        await pilot.app.push_screen(JobInvestigationScreen("12345"))
        await pilot.app.workers.wait_for_complete()
        await pilot.pause()

        screen = pilot.app.screen
        label, payload, line_count = screen.copy_pane()

        assert label == "Investigation Job 12345"
        # SPEC sec. 21 layout markers — render_report's section headers.
        assert "Summary" in payload
        assert "Slurm evidence" in payload
        assert "Likely explanation" in payload
        assert "Suggested next actions" in payload
        # Plain ASCII; no Rich markup tags.
        for marker in ("[red]", "[/red]", "[bold]", "[/bold]", "[yellow]", "[cyan]"):
            assert marker not in payload, f"unexpected Rich tag {marker!r} in payload"
        # Line count tracks splitlines().
        assert line_count == len(payload.splitlines())


async def test_investigate_screen_handles_partial_report(monkeypatch):
    """A report with errors renders the Errors section and the category string."""
    from sqtop.views import investigate as investigate_mod
    from sqtop.views.investigate import JobInvestigationScreen
    from textual.widgets import TextArea

    monkeypatch.setattr(
        investigate_mod,
        "investigate_job",
        lambda jid: _fake_report(jid, with_error=True),
    )

    app = _make_app(120, 30)
    async with app.run_test(size=(120, 30)) as pilot:
        await pilot.pause()
        await pilot.app.push_screen(JobInvestigationScreen("12345"))
        await pilot.app.workers.wait_for_complete()
        await pilot.pause()

        screen = pilot.app.screen
        ta = screen.query_one(TextArea)
        text = ta.text
        assert "Errors" in text
        assert "slurm_permission_denied" in text


def _patch_notify(app):
    """Replace ``app.notify`` with a list-recording stub.

    Returns the captured-records list. The replacement signature mirrors
    ``Textual.App.notify`` keyword args used by the screens.
    """
    captured: list[dict] = []

    def fake_notify(message, *, title="", severity="information", timeout=None):
        captured.append(
            {
                "message": message,
                "title": title,
                "severity": severity,
                "timeout": timeout,
            }
        )

    app.notify = fake_notify  # type: ignore[method-assign]
    return captured


async def test_job_investigation_notifies_on_partial_report(monkeypatch):
    """When report.errors is non-empty, a warning toast fires after load."""
    from sqtop.views import investigate as investigate_mod
    from sqtop.views.investigate import JobInvestigationScreen

    def _partial(jid: str) -> InvestigationReport:
        report = _fake_report(jid)
        report.errors.append(
            InvestigationError(
                source="sacct",
                category="accounting_unavailable",
                message="sacct unavailable",
            )
        )
        return report

    monkeypatch.setattr(investigate_mod, "investigate_job", _partial)

    app = _make_app(120, 30)
    async with app.run_test(size=(120, 30)) as pilot:
        await pilot.pause()
        captured = _patch_notify(pilot.app)
        await pilot.app.push_screen(JobInvestigationScreen("12345"))
        await pilot.app.workers.wait_for_complete()
        await pilot.pause()
        # Absorb the trailing call_from_thread that fires _notify_partial.
        await pilot.pause()

        warnings = [c for c in captured if c["severity"] == "warning"]
        assert warnings, f"expected a warning notification, got {captured!r}"
        msg = warnings[0]["message"]
        assert "accounting_unavailable" in msg
        assert "1" in msg
        assert "job" in msg
        assert warnings[0]["title"] == "Investigation"


async def test_job_investigation_does_not_notify_on_clean_report(monkeypatch):
    """When report.errors is empty, no warning toast fires."""
    from sqtop.views import investigate as investigate_mod
    from sqtop.views.investigate import JobInvestigationScreen

    monkeypatch.setattr(
        investigate_mod, "investigate_job", lambda jid: _fake_report(jid)
    )

    app = _make_app(120, 30)
    async with app.run_test(size=(120, 30)) as pilot:
        await pilot.pause()
        captured = _patch_notify(pilot.app)
        await pilot.app.push_screen(JobInvestigationScreen("12345"))
        await pilot.app.workers.wait_for_complete()
        await pilot.pause()
        await pilot.pause()

        warnings = [c for c in captured if c["severity"] == "warning"]
        assert warnings == []


# ---------------------------------------------------------------------------
# I-binding wiring on JobsView
# ---------------------------------------------------------------------------


def test_jobs_view_has_uppercase_I_investigate_binding():
    """The I (uppercase) binding maps to investigate_job and is shown in footer."""
    from sqtop.views.jobs import JobsView
    from textual.binding import Binding

    investigate_bindings = [
        b for b in JobsView.BINDINGS
        if isinstance(b, Binding) and b.key == "I"
    ]
    assert len(investigate_bindings) == 1
    assert investigate_bindings[0].action == "investigate_job"
    assert investigate_bindings[0].show is True

    # And the lowercase i still maps to job_info — they coexist.
    info_bindings = [
        b for b in JobsView.BINDINGS
        if isinstance(b, Binding) and b.key == "i"
    ]
    assert len(info_bindings) == 1
    assert info_bindings[0].action == "job_info"


def test_action_investigate_job_pushes_screen_for_cursor_job(temp_config, monkeypatch):
    """action_investigate_job pushes JobInvestigationScreen for the cursor row."""
    from sqtop.views.investigate import JobInvestigationScreen
    from sqtop.views.jobs import JobsView

    view = JobsView()
    target_job = _job(job_id="98765", name="my-job")
    monkeypatch.setattr(view, "_job_for_cursor", lambda: target_job)

    pushed: list = []

    class _FakeApp:
        def push_screen(self, screen, *args, **kwargs):
            pushed.append(screen)

    fake_app = _FakeApp()
    # Replace the `app` property accessor used by the action handler.
    monkeypatch.setattr(JobsView, "app", property(lambda self: fake_app))

    view.action_investigate_job()

    assert len(pushed) == 1
    screen = pushed[0]
    assert isinstance(screen, JobInvestigationScreen)
    assert screen._job_id == "98765"
    assert screen._job_name == "my-job"


def test_action_investigate_job_no_op_when_cursor_empty(temp_config, monkeypatch):
    """When _job_for_cursor returns None, no modal is pushed."""
    from sqtop.views.jobs import JobsView

    view = JobsView()
    monkeypatch.setattr(view, "_job_for_cursor", lambda: None)

    pushed: list = []

    class _FakeApp:
        def push_screen(self, screen, *args, **kwargs):
            pushed.append(screen)

    fake_app = _FakeApp()
    monkeypatch.setattr(JobsView, "app", property(lambda self: fake_app))

    view.action_investigate_job()

    assert pushed == []


# ---------------------------------------------------------------------------
# responsive_clamp + screen smoke
# ---------------------------------------------------------------------------


def test_investigate_screen_responsive_clamp_xs():
    """The screen exposes responsive_clamp and stores the tier marker class."""
    from sqtop.views.investigate import JobInvestigationScreen

    instance = object.__new__(JobInvestigationScreen)
    classes: set[str] = set()

    def _add_class(*names):
        classes.update(names)

    instance.add_class = _add_class  # type: ignore[method-assign]
    JobInvestigationScreen.responsive_clamp(instance, "xs")
    assert "clamp-xs" in classes


# ---------------------------------------------------------------------------
# Palette command wiring
# ---------------------------------------------------------------------------


async def test_palette_offers_investigate_by_id():
    """The system-command palette includes 'Investigate job by ID'."""
    app = _make_app(120, 30)
    async with app.run_test(size=(120, 30)) as pilot:
        await pilot.pause()
        labels = [cmd.title for cmd in pilot.app.get_system_commands(pilot.app.screen)]
        assert "Investigate job by ID" in labels


# ---------------------------------------------------------------------------
# Node investigation — PR 4-ui
# ---------------------------------------------------------------------------


def _fake_node_report(
    node_name: str = "gpu-a100-02",
    *,
    with_error: bool = False,
) -> InvestigationReport:
    """Build a minimally-populated node InvestigationReport for tests.

    Mirrors :func:`_fake_report` but with ``kind="node"`` so the renderer
    emits the SPEC sec. 22 layout.
    """
    target = InvestigationTarget(kind="node", identifier=node_name, source="cursor")
    report = InvestigationReport(
        target=target,
        generated_at=datetime(2026, 5, 8, 10, 14, 0),
    )
    report.summary.append(InvestigationItem(label="State", value="DRAIN"))
    report.evidence.append(
        InvestigationEvidence(
            id="sinfo.state",
            label="sinfo state",
            value="drain",
            source="sinfo",
            confidence="high",
        )
    )
    report.explanations.append(
        InvestigationExplanation(
            title="Node is draining",
            detail=(
                "The node is draining; running jobs are allowed to finish "
                "but no new jobs will be scheduled here until it returns to idle."
            ),
            confidence="medium",
        )
    )
    report.suggested_actions.append(
        InvestigationAction(
            label="Open node detail",
            detail="Inspect raw scontrol show node output",
            safe_for_user=True,
        )
    )
    if with_error:
        report.errors.append(
            InvestigationError(
                source="scontrol",
                category="slurm_permission_denied",
                message=f"scontrol show node {node_name} failed",
                stderr="permission denied",
            )
        )
    return report


async def test_node_investigate_screen_renders_report_text(monkeypatch):
    """Mounting NodeInvestigationScreen runs the worker and populates the TextArea."""
    from sqtop.views import investigate as investigate_mod
    from sqtop.views.investigate import NodeInvestigationScreen
    from textual.widgets import TextArea

    monkeypatch.setattr(
        investigate_mod, "investigate_node", lambda name: _fake_node_report(name)
    )

    app = _make_app(120, 30)
    async with app.run_test(size=(120, 30)) as pilot:
        await pilot.pause()
        await pilot.app.push_screen(NodeInvestigationScreen("gpu-a100-02"))
        await pilot.app.workers.wait_for_complete()
        await pilot.pause()

        screen = pilot.app.screen
        assert isinstance(screen, NodeInvestigationScreen)
        ta = screen.query_one(TextArea)
        text = ta.text
        # Header line from render_report.
        assert "Investigate Node gpu-a100-02" in text
        # render_report emits exp.detail under the explanation header.
        assert "The node is draining" in text


async def test_node_investigate_screen_copy_report_yields_plain_text(monkeypatch):
    """copy_pane returns a plain-text payload with the SPEC sec. 22 section markers."""
    from sqtop.views import investigate as investigate_mod
    from sqtop.views.investigate import NodeInvestigationScreen

    monkeypatch.setattr(
        investigate_mod, "investigate_node", lambda name: _fake_node_report(name)
    )

    app = _make_app(120, 30)
    async with app.run_test(size=(120, 30)) as pilot:
        await pilot.pause()
        await pilot.app.push_screen(NodeInvestigationScreen("gpu-a100-02"))
        await pilot.app.workers.wait_for_complete()
        await pilot.pause()

        screen = pilot.app.screen
        label, payload, line_count = screen.copy_pane()

        assert label == "Investigation Node gpu-a100-02"
        # SPEC sec. 22 layout markers — render_report's section headers.
        assert "Summary" in payload
        assert "Slurm evidence" in payload
        assert "Suggested next actions" in payload
        # Plain ASCII; no Rich markup tags.
        for marker in ("[red]", "[/red]", "[bold]", "[/bold]", "[yellow]", "[cyan]"):
            assert marker not in payload, f"unexpected Rich tag {marker!r} in payload"
        assert line_count == len(payload.splitlines())


async def test_node_investigate_screen_handles_partial_report(monkeypatch):
    """A node report with errors renders the Errors section and the category string."""
    from sqtop.views import investigate as investigate_mod
    from sqtop.views.investigate import NodeInvestigationScreen
    from textual.widgets import TextArea

    monkeypatch.setattr(
        investigate_mod,
        "investigate_node",
        lambda name: _fake_node_report(name, with_error=True),
    )

    app = _make_app(120, 30)
    async with app.run_test(size=(120, 30)) as pilot:
        await pilot.pause()
        await pilot.app.push_screen(NodeInvestigationScreen("gpu-a100-02"))
        await pilot.app.workers.wait_for_complete()
        await pilot.pause()

        screen = pilot.app.screen
        ta = screen.query_one(TextArea)
        text = ta.text
        assert "Errors" in text
        assert "slurm_permission_denied" in text


async def test_node_investigation_notifies_on_partial_report(monkeypatch):
    """When the node report has non-empty errors, a warning toast fires."""
    from sqtop.views import investigate as investigate_mod
    from sqtop.views.investigate import NodeInvestigationScreen

    def _partial(name: str) -> InvestigationReport:
        report = _fake_node_report(name)
        report.errors.append(
            InvestigationError(
                source="scontrol",
                category="slurm_permission_denied",
                message=f"scontrol show node {name} failed",
                stderr="permission denied",
            )
        )
        return report

    monkeypatch.setattr(investigate_mod, "investigate_node", _partial)

    app = _make_app(120, 30)
    async with app.run_test(size=(120, 30)) as pilot:
        await pilot.pause()
        captured = _patch_notify(pilot.app)
        await pilot.app.push_screen(NodeInvestigationScreen("gpu-a100-02"))
        await pilot.app.workers.wait_for_complete()
        await pilot.pause()
        await pilot.pause()

        warnings = [c for c in captured if c["severity"] == "warning"]
        assert warnings, f"expected a warning notification, got {captured!r}"
        msg = warnings[0]["message"]
        assert "node" in msg
        assert "slurm_permission_denied" in msg
        assert "1" in msg
        assert warnings[0]["title"] == "Investigation"


async def test_node_investigation_does_not_notify_on_clean_report(monkeypatch):
    """When the node report has no errors, no warning toast fires."""
    from sqtop.views import investigate as investigate_mod
    from sqtop.views.investigate import NodeInvestigationScreen

    monkeypatch.setattr(
        investigate_mod, "investigate_node", lambda name: _fake_node_report(name)
    )

    app = _make_app(120, 30)
    async with app.run_test(size=(120, 30)) as pilot:
        await pilot.pause()
        captured = _patch_notify(pilot.app)
        await pilot.app.push_screen(NodeInvestigationScreen("gpu-a100-02"))
        await pilot.app.workers.wait_for_complete()
        await pilot.pause()
        await pilot.pause()

        warnings = [c for c in captured if c["severity"] == "warning"]
        assert warnings == []


def _make_node(name: str = "gpu-a100-02"):
    """Build a minimal Node for cursor-row tests."""
    from sqtop.slurm import Node

    return Node(
        name=name,
        state="idle",
        partition="gpu",
        cpus_alloc="0",
        cpus_total="64",
        memory_free="200000",
        memory_total="256000",
        load="0.50",
        gpu_alloc=0,
        gpu_total=4,
    )


def test_nodes_view_I_binding_pushes_node_investigation(monkeypatch):
    """action_investigate_node pushes NodeInvestigationScreen for the cursor row."""
    from sqtop.views.investigate import NodeInvestigationScreen
    from sqtop.views.nodes import NodesView

    view = NodesView()
    target = _make_node("gpu-a100-02")
    view._last_sorted_nodes = [target]

    class _FakeTable:
        cursor_row = 0

    monkeypatch.setattr(view, "query_one", lambda *_args, **_kw: _FakeTable())

    pushed: list = []

    class _FakeApp:
        def push_screen(self, screen, *args, **kwargs):
            pushed.append(screen)

    fake_app = _FakeApp()
    monkeypatch.setattr(NodesView, "app", property(lambda self: fake_app))

    view.action_investigate_node()

    assert len(pushed) == 1
    screen = pushed[0]
    assert isinstance(screen, NodeInvestigationScreen)
    assert screen._node_name == "gpu-a100-02"


def test_nodes_view_I_binding_no_op_when_table_empty(monkeypatch):
    """When _last_sorted_nodes is empty, no modal is pushed."""
    from sqtop.views.nodes import NodesView

    view = NodesView()
    view._last_sorted_nodes = []

    class _FakeTable:
        cursor_row = 0

    monkeypatch.setattr(view, "query_one", lambda *_args, **_kw: _FakeTable())

    pushed: list = []

    class _FakeApp:
        def push_screen(self, screen, *args, **kwargs):
            pushed.append(screen)

    fake_app = _FakeApp()
    monkeypatch.setattr(NodesView, "app", property(lambda self: fake_app))

    view.action_investigate_node()

    assert pushed == []


def test_nodes_view_has_uppercase_I_investigate_binding():
    """The I (uppercase) binding maps to investigate_node and is shown in footer."""
    from sqtop.views.nodes import NodesView
    from textual.binding import Binding

    investigate_bindings = [
        b for b in NodesView.BINDINGS
        if isinstance(b, Binding) and b.key == "I"
    ]
    assert len(investigate_bindings) == 1
    assert investigate_bindings[0].action == "investigate_node"
    assert investigate_bindings[0].show is True


async def test_palette_offers_investigate_node_by_name():
    """The system-command palette includes 'Investigate node by name'."""
    app = _make_app(120, 30)
    async with app.run_test(size=(120, 30)) as pilot:
        await pilot.pause()
        labels = [cmd.title for cmd in pilot.app.get_system_commands(pilot.app.screen)]
        assert "Investigate node by name" in labels


def test_node_investigate_screen_responsive_clamp_xs():
    """NodeInvestigationScreen exposes responsive_clamp and stores the tier marker."""
    from sqtop.views.investigate import NodeInvestigationScreen

    instance = object.__new__(NodeInvestigationScreen)
    classes: set[str] = set()

    def _add_class(*names):
        classes.update(names)

    instance.add_class = _add_class  # type: ignore[method-assign]
    NodeInvestigationScreen.responsive_clamp(instance, "xs")
    assert "clamp-xs" in classes

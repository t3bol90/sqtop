"""Tests for JobInvestigationScreen and the I keybind on JobsView (PR 3b)."""
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

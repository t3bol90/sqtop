"""Tests for HealthView rendering of error_category (PR 5b)."""

from __future__ import annotations

import shutil
from unittest.mock import patch

import pytest

from sqtop.slurm import CommandStat


def _make_app(width: int = 120, height: int = 30):
    """Instantiate SqtopApp with a mocked terminal size."""
    from sqtop.app import SqtopApp

    fake_size = shutil.os.terminal_size((width, height))
    with patch("sqtop.app.shutil.get_terminal_size", return_value=fake_size):
        return SqtopApp()


# ---------------------------------------------------------------------------
# Compose-level: CATEGORY column exists in the table layout
# ---------------------------------------------------------------------------


@pytest.mark.asyncio
async def test_health_table_has_category_column():
    """The health-table has a CATEGORY column between LATENCY and ERROR."""
    from sqtop.views.health import HealthView
    from sqtop.views.widgets import CyclicDataTable

    app = _make_app(120, 30)
    async with app.run_test(size=(120, 30)) as pilot:
        # Switch to the health tab so the view is mounted.
        await pilot.press("5")
        await pilot.pause()
        view = pilot.app.query_one(HealthView)
        table = view.query_one("#health-table", CyclicDataTable)
        col_labels = [str(col.label) for col in table.columns.values()]
        assert "CATEGORY" in col_labels
        # Order: COMMAND, OK, LATENCY, CATEGORY, ERROR
        cat_idx = col_labels.index("CATEGORY")
        err_idx = col_labels.index("ERROR")
        lat_idx = col_labels.index("LATENCY")
        assert lat_idx < cat_idx < err_idx


# ---------------------------------------------------------------------------
# Render-level: CommandStat with a category lands in the CATEGORY cell
# ---------------------------------------------------------------------------


@pytest.mark.asyncio
async def test_health_renders_error_category_when_present():
    """A CommandStat with error_category renders the value in the CATEGORY cell."""
    from sqtop.views.health import HealthView
    from sqtop.views.widgets import CyclicDataTable
    from sqtop import slurm as slurm_mod

    stats = [
        CommandStat(
            command="squeue",
            ok=False,
            latency_ms=5,
            stderr="permission denied",
            error_category="slurm_permission_denied",
        )
    ]

    app = _make_app(120, 30)
    async with app.run_test(size=(120, 30)) as pilot:
        await pilot.press("5")
        await pilot.pause()
        view = pilot.app.query_one(HealthView)
        # Bypass the worker thread — drive _update_table directly on main thread.
        view._update_table(stats)
        table = view.query_one("#health-table", CyclicDataTable)
        assert table.row_count == 1
        row = table.get_row_at(0)
        # row is a list of cells in column order: COMMAND, OK, LATENCY, CATEGORY, ERROR
        category_cell = str(row[3])
        assert "slurm_permission_denied" in category_cell


@pytest.mark.asyncio
async def test_health_renders_empty_category_when_none():
    """A CommandStat with error_category=None renders an empty CATEGORY cell."""
    from sqtop.views.health import HealthView
    from sqtop.views.widgets import CyclicDataTable

    stats = [
        CommandStat(
            command="sinfo",
            ok=True,
            latency_ms=12,
            stderr="",
            error_category=None,
        )
    ]

    app = _make_app(120, 30)
    async with app.run_test(size=(120, 30)) as pilot:
        await pilot.press("5")
        await pilot.pause()
        view = pilot.app.query_one(HealthView)
        view._update_table(stats)
        table = view.query_one("#health-table", CyclicDataTable)
        assert table.row_count == 1
        row = table.get_row_at(0)
        category_cell = str(row[3])
        # Empty cell — no markup, no category text.
        assert category_cell == ""


# ---------------------------------------------------------------------------
# copy_pane includes CATEGORY column header and value
# ---------------------------------------------------------------------------


@pytest.mark.asyncio
async def test_health_copy_pane_includes_category():
    """copy_pane TSV payload includes CATEGORY in header and value."""
    from sqtop.views.health import HealthView

    stats = [
        CommandStat(
            command="scontrol",
            ok=False,
            latency_ms=42,
            stderr="invalid argument",
            error_category="slurm_invalid_arg",
        )
    ]

    app = _make_app(120, 30)
    async with app.run_test(size=(120, 30)) as pilot:
        await pilot.press("5")
        await pilot.pause()
        view = pilot.app.query_one(HealthView)
        view._update_table(stats)
        label, payload, count = view.copy_pane()
        assert label == "Health"
        assert "CATEGORY" in payload
        assert "slurm_invalid_arg" in payload
        assert count == 1

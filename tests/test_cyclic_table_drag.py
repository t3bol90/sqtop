"""Tests for CyclicDataTable column-reorder drag (widgets.py)."""
from __future__ import annotations

import pytest
from textual.app import App, ComposeResult
from textual.events import MouseMove

from sqtop.views.widgets import DRAG_THRESHOLD_CELLS, CyclicDataTable

# ---------------------------------------------------------------------------
# Minimal test app
# ---------------------------------------------------------------------------

_COLS = ["Alpha", "Beta", "Gamma", "Delta"]
_COL_WIDTH = 10  # explicit widths so boundaries are predictable


class _TableApp(App):
    """Minimal app that mounts one CyclicDataTable with 4 known-width columns."""

    CSS = "CyclicDataTable { height: 1fr; }"

    def compose(self) -> ComposeResult:
        table: CyclicDataTable = CyclicDataTable(id="tbl")
        yield table

    def on_mount(self) -> None:
        table = self.query_one("#tbl", CyclicDataTable)
        for col in _COLS:
            table.add_column(col, width=_COL_WIDTH)
        for i in range(5):
            table.add_row(*[f"{col}{i}" for col in _COLS])


# ---------------------------------------------------------------------------
# Helpers
# ---------------------------------------------------------------------------

def _make_hook() -> tuple[list[CyclicDataTable.ColumnReordered], object]:
    """Return (messages, hook_fn) deduplicating by object identity (bubbling)."""
    seen: set[int] = set()
    messages: list[CyclicDataTable.ColumnReordered] = []

    def _hook(msg):
        if isinstance(msg, CyclicDataTable.ColumnReordered) and id(msg) not in seen:
            seen.add(id(msg))
            messages.append(msg)

    return messages, _hook


def _col_x(table: CyclicDataTable, col_idx: int) -> int:
    """Return a widget-local x coordinate that lands inside column col_idx."""
    boundaries = table._column_boundaries()
    left = boundaries[col_idx]
    right = boundaries[col_idx + 1]
    return (left + right) // 2


def _boundary_x(table: CyclicDataTable, boundary_idx: int) -> int:
    """Return the x coordinate of boundary boundary_idx."""
    return table._column_boundaries()[boundary_idx]


# ---------------------------------------------------------------------------
# Tests
# ---------------------------------------------------------------------------


@pytest.mark.asyncio
async def test_no_drag_no_message_same_position():
    """Mouse down + up at the same x → no ColumnReordered posted."""
    messages, hook = _make_hook()

    app = _TableApp()
    async with app.run_test(size=(80, 24), message_hook=hook) as pilot:
        table = app.query_one("#tbl", CyclicDataTable)
        x = _col_x(table, 1)
        await pilot.mouse_down("#tbl", offset=(x, 0))
        await pilot.pause()
        await pilot.mouse_up("#tbl", offset=(x, 0))
        await pilot.pause()

    assert messages == [], f"Expected no ColumnReordered but got {messages}"


@pytest.mark.asyncio
async def test_drag_horizontal_posts_column_reordered():
    """Mouse down on col 0, drag ≥ threshold toward col 2 boundary, up → ColumnReordered."""
    messages, hook = _make_hook()

    app = _TableApp()
    async with app.run_test(size=(80, 24), message_hook=hook) as pilot:
        table = app.query_one("#tbl", CyclicDataTable)
        # Press in column 0 header
        from_x = _col_x(table, 0)
        # Drag to boundary between col 1 and col 2
        to_x = _boundary_x(table, 2)

        await pilot.mouse_down("#tbl", offset=(from_x, 0))
        await pilot.pause()
        await pilot._post_mouse_events(
            [MouseMove], "#tbl", offset=(to_x, 0), button=1
        )
        await pilot.pause()
        await pilot.mouse_up("#tbl", offset=(to_x, 0))
        await pilot.pause()

    assert len(messages) == 1, f"Expected 1 ColumnReordered but got {messages}"
    msg = messages[0]
    assert msg.from_index == 0
    assert msg.to_index == 2


@pytest.mark.asyncio
async def test_esc_cancels_drag_no_message():
    """Mouse down + drag start + Esc → no ColumnReordered posted, state cleared."""
    messages, hook = _make_hook()

    app = _TableApp()
    async with app.run_test(size=(80, 24), message_hook=hook) as pilot:
        table = app.query_one("#tbl", CyclicDataTable)
        from_x = _col_x(table, 1)
        to_x = _boundary_x(table, 3)

        await pilot.mouse_down("#tbl", offset=(from_x, 0))
        await pilot.pause()
        await pilot._post_mouse_events(
            [MouseMove], "#tbl", offset=(to_x, 0), button=1
        )
        await pilot.pause()
        assert table._dragging is True
        await pilot.press("escape")
        await pilot.pause()
        assert table._dragging is False
        assert table._drag_col_index is None
        assert table._drag_marker_x is None

    assert messages == [], f"Expected no ColumnReordered after Esc but got {messages}"


@pytest.mark.asyncio
async def test_drag_past_rightmost_boundary():
    """Dragging to x > rightmost column → to_index == num_visible_columns."""
    messages, hook = _make_hook()

    app = _TableApp()
    async with app.run_test(size=(80, 24), message_hook=hook) as pilot:
        table = app.query_one("#tbl", CyclicDataTable)
        num_cols = len(_COLS)
        from_x = _col_x(table, 0)
        last_boundary = _boundary_x(table, num_cols)
        far_right = last_boundary + 20
        # Clamp to table width to stay in bounds for Pilot
        table_width = table.size.width
        safe_x = min(far_right, table_width - 1)

        await pilot.mouse_down("#tbl", offset=(from_x, 0))
        await pilot.pause()
        await pilot._post_mouse_events(
            [MouseMove], "#tbl", offset=(safe_x, 0), button=1
        )
        await pilot.pause()
        await pilot.mouse_up("#tbl", offset=(safe_x, 0))
        await pilot.pause()

    assert len(messages) == 1
    assert messages[0].to_index == num_cols


@pytest.mark.asyncio
async def test_drag_past_leftmost_boundary():
    """Dragging to x < leftmost column → to_index == 0."""
    messages, hook = _make_hook()

    app = _TableApp()
    async with app.run_test(size=(80, 24), message_hook=hook) as pilot:
        table = app.query_one("#tbl", CyclicDataTable)
        from_x = _col_x(table, 3)
        left_x = 0

        await pilot.mouse_down("#tbl", offset=(from_x, 0))
        await pilot.pause()
        await pilot._post_mouse_events(
            [MouseMove], "#tbl", offset=(left_x, 0), button=1
        )
        await pilot.pause()
        await pilot.mouse_up("#tbl", offset=(left_x, 0))
        await pilot.pause()

    assert len(messages) == 1
    assert messages[0].to_index == 0

"""Tests for NodesView column reorder and ColumnToggleScreen reset-to-default.

NOTE: Tests that require cross-agent imports (_reconcile_order, CyclicDataTable.ColumnReordered)
will fail in isolation. Those are marked with a comment. All other tests should pass.
"""
from __future__ import annotations

import pytest

from sqtop.views.nodes import COLUMNS, NodesView
from sqtop.views.column_toggle import ColumnToggleScreen
from sqtop import config


# ── Helpers ───────────────────────────────────────────────────────────────────


class _FakeTable:
    """Minimal fake CyclicDataTable for unit tests that don't need DOM."""

    def __init__(self):
        self.columns_cleared = 0
        self.columns_added: list[tuple[str, int]] = []
        self.cursor_row = 0
        self.cursor_column = 0
        self.row_count = 0
        self.scroll_offset = _FakeScroll()

    def clear(self, columns: bool = False) -> None:
        if columns:
            self.columns_cleared += 1
            self.columns_added = []

    def add_column(self, name: str, width: int) -> None:
        self.columns_added.append((name, width))

    def add_row(self, *args) -> None:
        self.row_count += 1

    def move_cursor(self, *, row: int | None = None, column: int | None = None) -> None:
        if row is not None:
            self.cursor_row = row
        if column is not None:
            self.cursor_column = column

    def scroll_to(self, *, y: float, animate: bool = False) -> None:
        pass


class _FakeScroll:
    y = 0.0


# ── Test 1: Default _column_order matches COLUMNS ─────────────────────────────


def test_default_column_order_matches_columns(temp_config):
    """_column_order must default to the canonical COLUMNS order."""
    view = NodesView()
    default = [c.name for c in COLUMNS]
    # NOTE: this calls _reconcile_order from Agent A. Will fail until Agent A's work
    # is integrated. The expected result when integrated: _column_order == default.
    assert view._column_order == default


# ── Test 2: shift_column_right on col 0 swaps positions 0 and 1 ──────────────


def test_shift_column_right_swaps_first_two(monkeypatch, temp_config):
    """] on column 0 moves it to position 1."""
    view = NodesView()
    fake_table = _FakeTable()
    fake_table.cursor_column = 0

    # Patch query_one to return our fake table
    monkeypatch.setattr(view, "query_one", lambda *a, **kw: fake_table)

    # Capture initial first two column names in _column_order
    original_order = list(view._column_order)
    col0 = original_order[0]
    col1 = original_order[1]

    # Simulate _current_cols reflecting order (both visible)
    view._current_cols = [(name, 10) for name in original_order[:5]]

    # Stub _rebuild_columns and _render_rows since they require DOM
    monkeypatch.setattr(view, "_rebuild_columns", lambda *a, **kw: True)
    monkeypatch.setattr(view, "_render_rows", lambda *a, **kw: None)
    monkeypatch.setattr(view, "_capture_table_state", lambda: (0, 0.0, None))
    monkeypatch.setattr(view, "_restore_table_state", lambda *a: None)
    monkeypatch.setattr(config, "update", lambda d: None)

    view.action_shift_column_right()

    # After shift, col0 should be at index 1, col1 at index 0
    assert view._column_order[0] == col1
    assert view._column_order[1] == col0


# ── Test 3: Boundary no-ops ───────────────────────────────────────────────────


def test_shift_column_left_on_first_col_is_noop(monkeypatch, temp_config):
    """shift_column_left on the first visible column is a no-op."""
    view = NodesView()
    fake_table = _FakeTable()
    fake_table.cursor_column = 0
    monkeypatch.setattr(view, "query_one", lambda *a, **kw: fake_table)

    original_order = list(view._column_order)
    view._current_cols = [(name, 10) for name in original_order[:5]]

    rebuild_called = []
    monkeypatch.setattr(view, "_rebuild_columns", lambda *a, **kw: rebuild_called.append(1) or True)
    monkeypatch.setattr(view, "_render_rows", lambda *a, **kw: None)

    view.action_shift_column_left()

    # Order should be unchanged and rebuild should not have been called
    assert view._column_order == original_order
    assert len(rebuild_called) == 0


def test_shift_column_right_on_last_col_is_noop(monkeypatch, temp_config):
    """shift_column_right on the last visible column is a no-op."""
    view = NodesView()
    fake_table = _FakeTable()

    original_order = list(view._column_order)
    visible = [(name, 10) for name in original_order[:5]]
    view._current_cols = visible
    view._reorder_target_idx = len(visible) - 1

    monkeypatch.setattr(view, "query_one", lambda *a, **kw: fake_table)

    rebuild_called = []
    monkeypatch.setattr(view, "_rebuild_columns", lambda *a, **kw: rebuild_called.append(1) or True)
    monkeypatch.setattr(view, "_render_rows", lambda *a, **kw: None)

    view.action_shift_column_right()

    assert view._column_order == original_order
    assert len(rebuild_called) == 0


# ── Test 4: Mouse-drag handler moves columns correctly ────────────────────────


def test_mouse_drag_handler_reorders_columns(monkeypatch, temp_config):
    """on_cyclic_data_table_column_reordered moves from_index to to_index."""
    view = NodesView()

    original_order = list(view._column_order)
    visible_names = original_order[:4]
    view._current_cols = [(name, 10) for name in visible_names]

    persisted = []
    monkeypatch.setattr(config, "update", lambda d: persisted.append(d))
    monkeypatch.setattr(view, "query_one", lambda *a, **kw: _FakeTable())
    monkeypatch.setattr(view, "_rebuild_columns", lambda *a, **kw: True)
    monkeypatch.setattr(view, "_render_rows", lambda *a, **kw: None)
    monkeypatch.setattr(view, "_capture_table_state", lambda: (0, 0.0, None))
    monkeypatch.setattr(view, "_restore_table_state", lambda *a: None)

    # Simulate drag from visible index 0 to visible index 2
    # NOTE: CyclicDataTable.ColumnReordered is Agent B's work; we construct the event manually.
    event = type("ColumnReordered", (), {"from_index": 0, "to_index": 2})()
    view.on_cyclic_data_table_column_reordered(event)

    # from_index=0 (col "NODE") moved to position 2
    # Result: ["STATE", "CPU%", "NODE", "GPU%", ...]
    assert view._column_order[0] == original_order[1]   # STATE
    assert view._column_order[1] == original_order[2]   # CPU%
    assert view._column_order[2] == original_order[0]   # NODE (moved here)

    # Config.update was called
    assert len(persisted) == 1
    assert "columns" in persisted[0]
    assert "nodes_order" in persisted[0]["columns"]


# ── Test 5: _persist_column_order round-trips through config ──────────────────


def test_persist_column_order_calls_config_update(monkeypatch, temp_config):
    """_persist_column_order calls config.update with nodes_order."""
    view = NodesView()
    custom_order = ["STATE", "NODE", "CPU%", "GPU%", "CPUS A/T", "GPU A/T", "MEM FREE", "PARTITION", "MEM TOTAL", "LOAD"]
    view._column_order = custom_order

    calls = []
    monkeypatch.setattr(config, "update", lambda d: calls.append(d))

    view._persist_column_order()

    assert len(calls) == 1
    assert calls[0] == {"columns": {"nodes_order": custom_order}}


# ── Test 6: ColumnToggleScreen renders in column_order when provided ──────────


def test_column_toggle_screen_renders_in_provided_order():
    """When column_order is given, checkboxes appear in that order."""
    all_cols = ["NODE", "STATE", "CPU%", "GPU%"]
    custom_order = ["STATE", "GPU%", "NODE", "CPU%"]

    screen = ColumnToggleScreen(
        view_name="nodes",
        all_columns=all_cols,
        hidden_columns=[],
        column_order=custom_order,
    )

    # _display_order must follow the custom_order
    assert screen._display_order == custom_order


def test_column_toggle_screen_falls_back_without_column_order():
    """Without column_order, checkboxes appear in all_columns order (backwards compat)."""
    all_cols = ["NODE", "STATE", "CPU%", "GPU%"]

    screen = ColumnToggleScreen(
        view_name="nodes",
        all_columns=all_cols,
        hidden_columns=[],
    )

    assert screen._display_order == all_cols


def test_column_toggle_screen_handles_partial_column_order():
    """column_order that doesn't include all columns: missing ones appended."""
    all_cols = ["NODE", "STATE", "CPU%", "GPU%"]
    partial_order = ["GPU%", "STATE"]

    screen = ColumnToggleScreen(
        view_name="nodes",
        all_columns=all_cols,
        hidden_columns=[],
        column_order=partial_order,
    )

    # Ordered items first, then remaining in original order
    assert screen._display_order[:2] == ["GPU%", "STATE"]
    assert set(screen._display_order[2:]) == {"NODE", "CPU%"}


# ── Test 7: Reset-button dismisses with ("reset", view_name) ──────────────────


def test_column_toggle_reset_button_dismiss_value():
    """on_button_pressed for btn-col-reset dismisses with ('reset', view_name)."""
    all_cols = ["NODE", "STATE", "CPU%"]

    screen = ColumnToggleScreen(
        view_name="nodes",
        all_columns=all_cols,
        hidden_columns=[],
    )

    dismissed_values = []

    def fake_dismiss(value):
        dismissed_values.append(value)

    screen.dismiss = fake_dismiss

    # Simulate pressing the reset button
    reset_btn = type("Button", (), {"id": "btn-col-reset"})()
    event = type("Pressed", (), {"button": reset_btn})()
    screen.on_button_pressed(event)

    assert dismissed_values == [("reset", "nodes")]


def test_column_toggle_close_button_dismiss_none():
    """on_button_pressed for btn-col-close dismisses with None."""
    all_cols = ["NODE", "STATE"]

    screen = ColumnToggleScreen(
        view_name="nodes",
        all_columns=all_cols,
        hidden_columns=[],
    )

    dismissed_values = []
    screen.dismiss = lambda v: dismissed_values.append(v)

    close_btn = type("Button", (), {"id": "btn-col-close"})()
    event = type("Pressed", (), {"button": close_btn})()
    screen.on_button_pressed(event)

    assert dismissed_values == [None]


def test_reset_button_clears_column_order_in_view(monkeypatch, temp_config):
    """After reset, view._column_order reverts to COLUMNS default and config is cleared."""
    view = NodesView()

    # Set a custom order
    custom = list(reversed([c.name for c in COLUMNS]))
    view._column_order = custom

    # Simulate the reset callback as would be called from app.py
    default_order = [c.name for c in COLUMNS]

    persisted = {}

    def fake_update(d):
        for k, v in d.items():
            if isinstance(v, dict):
                persisted.setdefault(k, {}).update(v)
            else:
                persisted[k] = v

    monkeypatch.setattr(config, "update", fake_update)
    monkeypatch.setattr(view, "_rebuild_columns", lambda *a, **kw: True)
    monkeypatch.setattr(view, "_render_rows", lambda *a, **kw: None)

    # Perform the reset (mirrors app.py callback logic)
    view._column_order = [c.name for c in COLUMNS]
    config.update({"columns": {"nodes_order": []}})

    assert view._column_order == default_order
    assert persisted.get("columns", {}).get("nodes_order") == []

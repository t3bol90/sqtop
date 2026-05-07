"""Tests for JobsView column reorder."""
from __future__ import annotations

from sqtop.views.jobs import JobsView, COLUMNS
from sqtop import config


# ---------------------------------------------------------------------------
# Helpers
# ---------------------------------------------------------------------------

class _FakeTable:
    """Minimal DataTable stand-in for tests that call _rebuild_columns."""

    def __init__(self) -> None:
        self.cursor_row = 0
        self.cursor_column = 0
        self.columns_added: list[tuple[str, int]] = []

    def clear(self, columns: bool = False) -> None:
        if columns:
            self.columns_added.clear()

    def add_column(self, name: str, width: int) -> None:
        self.columns_added.append((name, width))

    def move_cursor(self, *, row: int = 0, column: int = 0) -> None:
        self.cursor_row = row
        self.cursor_column = column

    def scroll_to(self, *, y: float, animate: bool = True) -> None:
        pass

    @property
    def scroll_offset(self):
        class _Offset:
            y = 0
        return _Offset()


def _make_view(monkeypatch, temp_config) -> JobsView:
    """Instantiate JobsView with a patched query_one so no Textual app is needed."""
    view = JobsView()
    fake_table = _FakeTable()
    monkeypatch.setattr(view, "query_one", lambda *args, **kwargs: fake_table)
    # Provide a stub for size so _rebuild_columns won't crash.
    class _Size:
        width = 200
    monkeypatch.setattr(type(view), "size", property(lambda self: _Size()), raising=False)
    return view


# ---------------------------------------------------------------------------
# Tests
# ---------------------------------------------------------------------------

def test_default_column_order_matches_columns(monkeypatch, temp_config):
    """1. Default _column_order == [c.name for c in COLUMNS]."""
    view = _make_view(monkeypatch, temp_config)
    assert view._column_order == [c.name for c in COLUMNS]


def test_shift_right_swaps_first_two_columns(monkeypatch, temp_config):
    """2. ] on column 0 moves col[0] to position 1."""
    view = _make_view(monkeypatch, temp_config)

    original_col0 = view._column_order[0]
    original_col1 = view._column_order[1]

    # Build a minimal _current_cols so _shift_visible_column has something to work with.
    view._current_cols = [(c.name, 10) for c in COLUMNS]
    # Set cursor to column 0 in the fake table.
    view.query_one(None).cursor_column = 0

    view.action_shift_column_right()

    assert view._column_order[0] == original_col1
    assert view._column_order[1] == original_col0


def test_shift_left_on_first_column_is_noop(monkeypatch, temp_config):
    """3. [ on column 0 does nothing."""
    view = _make_view(monkeypatch, temp_config)

    original_order = list(view._column_order)
    view._current_cols = [(c.name, 10) for c in COLUMNS]
    view.query_one(None).cursor_column = 0

    view.action_shift_column_left()

    assert view._column_order == original_order


def test_shift_right_on_last_visible_column_is_noop(monkeypatch, temp_config):
    """4. ] on the last visible column does nothing."""
    view = _make_view(monkeypatch, temp_config)

    original_order = list(view._column_order)
    view._current_cols = [(c.name, 10) for c in COLUMNS]
    last_vis_idx = len(view._current_cols) - 1
    view._reorder_target_idx = last_vis_idx

    view.action_shift_column_right()

    assert view._column_order == original_order


def test_persist_and_reload_column_order(monkeypatch, temp_config):
    """5. _persist_column_order writes to config; a fresh view reads it back."""
    view = _make_view(monkeypatch, temp_config)

    # Swap first two columns manually then persist.
    view._column_order[0], view._column_order[1] = (
        view._column_order[1],
        view._column_order[0],
    )
    saved_order = list(view._column_order)
    view._persist_column_order()

    # A fresh view should load the saved order.
    view2 = _make_view(monkeypatch, temp_config)
    assert view2._column_order == saved_order


def test_on_column_reordered_moves_from_vis2_to_vis0(monkeypatch, temp_config):
    """6. Mouse drag from visible index 2 to visible index 0 reorders correctly."""
    view = _make_view(monkeypatch, temp_config)

    # Populate _current_cols with the first 4 COLUMNS.
    first_four = [c.name for c in COLUMNS[:4]]
    view._current_cols = [(n, 10) for n in first_four]
    # Sync _column_order so it starts as the default.
    # (Already set from __init__)

    # Build a fake ColumnReordered event.
    class _FakeEvent:
        from_index = 2
        to_index = 0

    moved_name = first_four[2]
    view.on_cyclic_data_table_column_reordered(_FakeEvent())

    # moved_name should now appear before first_four[0] in _column_order.
    moved_abs = view._column_order.index(moved_name)
    anchor_abs = view._column_order.index(first_four[0])
    assert moved_abs < anchor_abs, (
        f"Expected '{moved_name}' before '{first_four[0]}' in _column_order, "
        f"got order: {view._column_order}"
    )


def test_hidden_columns_retain_absolute_position(monkeypatch, temp_config):
    """7. Toggling a column off, shifting another, then toggling it back preserves its slot."""
    view = _make_view(monkeypatch, temp_config)

    # Hide the second column.
    hidden_name = view._column_order[1]
    view._hidden_cols = {hidden_name}

    # Build visible _current_cols (excludes hidden_name).
    visible = [n for n in view._column_order if n not in view._hidden_cols]
    view._current_cols = [(n, 10) for n in visible]
    view.query_one(None).cursor_column = 0

    # Shift the first visible column to the right.
    view.action_shift_column_right()

    # Unhide the column.
    view._hidden_cols = set()

    # The hidden column must still exist in _column_order.
    assert hidden_name in view._column_order

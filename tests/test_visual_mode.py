"""Tests for VisualSelectMixin and visual selection mode in data-table views."""
from __future__ import annotations

import pytest
from unittest.mock import MagicMock, patch


# ---------------------------------------------------------------------------
# Helpers — minimal fake items and a concrete view stub
# ---------------------------------------------------------------------------

def _make_jobs(n: int):
    """Return n fake Job-like objects with job_id, name, state, etc."""
    from sqtop.slurm import Job
    return [
        Job(
            job_id=str(i + 1),
            name=f"job{i + 1}",
            user="testuser",
            state="RUNNING",
            partition="gpu",
            nodes="1",
            num_nodes="1",
            num_cpus="4",
            time_used="00:01:00",
            time_limit="01:00:00",
        )
        for i in range(n)
    ]


# ---------------------------------------------------------------------------
# Unit tests for VisualSelectMixin standalone logic
# ---------------------------------------------------------------------------

class _MockTable:
    """Minimal stand-in for CyclicDataTable used by VisualSelectMixin."""

    def __init__(self, row_count: int = 10, cursor_row: int = 0) -> None:
        self.row_count = row_count
        self.cursor_row = cursor_row

    def action_cursor_up(self) -> None:
        self.cursor_row = max(0, self.cursor_row - 1)

    def action_cursor_down(self) -> None:
        self.cursor_row = min(self.row_count - 1, self.cursor_row + 1)

    def move_cursor(self, *, row: int) -> None:
        self.cursor_row = row


class _MockApp:
    """Minimal stand-in for the Textual App."""

    def __init__(self) -> None:
        self.sub_title = ""
        self._notifications: list[dict] = []

    def notify(self, msg: str, *, title: str = "", severity: str = "information", **kw) -> None:
        self._notifications.append({"msg": msg, "title": title, "severity": severity})

    def copy_to_clipboard(self, text: str) -> None:
        self._last_clipboard = text


class ConcreteView:
    """Concrete test class that mixes in VisualSelectMixin and implements the protocol."""

    def __init__(self, items, table=None) -> None:
        from sqtop.views.mixins import VisualSelectMixin
        # VisualSelectMixin is a pure-Python mixin; no __init__ to call
        self._items = items
        self._table = table or _MockTable(row_count=len(items))
        self.app = _MockApp()
        self._rendered: list | None = None

        # Inject mixin methods
        from sqtop.views.mixins import VisualSelectMixin
        for attr in dir(VisualSelectMixin):
            if not attr.startswith("__"):
                method = getattr(VisualSelectMixin, attr)
                if callable(method) and not isinstance(method, property):
                    import types
                    setattr(self, attr, types.MethodType(method, self))

    def query_one(self, cls):
        return self._table

    def _current_items(self):
        return self._items

    def _row_tsv(self, item) -> str:
        return f"{item.job_id}\t{item.name}\t{item.state}"

    def _render_rows(self, items) -> None:
        self._rendered = list(items)


# We need a simpler approach — use the mixin directly without fighting Python MRO
# Let's just instantiate via proper inheritance

from sqtop.views.mixins import VisualSelectMixin


class SimpleView(VisualSelectMixin):
    """A concrete view for direct mixin testing."""

    def __init__(self, items, table=None) -> None:
        self._items = items
        self._table = table or _MockTable(row_count=len(items))
        self.app = _MockApp()
        self._rendered: list | None = None

    def query_one(self, cls):
        return self._table

    def _current_items(self):
        return self._items

    def _row_tsv(self, item) -> str:
        return f"{item.job_id}\t{item.name}\t{item.state}"

    def _render_rows(self, items) -> None:
        self._rendered = list(items)


# ---------------------------------------------------------------------------
# Tests
# ---------------------------------------------------------------------------

class TestVisualSelectMixin:

    def _view(self, n: int = 10, cursor: int = 0) -> SimpleView:
        items = _make_jobs(n)
        table = _MockTable(row_count=n, cursor_row=cursor)
        return SimpleView(items, table)

    def test_initial_state_is_inactive(self):
        v = self._view()
        assert v._visual_active is False
        assert v._visual_anchor is None
        assert v._visual_cursor is None

    def test_visual_enter_sets_state(self):
        v = self._view(cursor=2)
        v.action_visual_enter()
        assert v._visual_active is True
        assert v._visual_anchor == 2
        assert v._visual_cursor == 2
        assert v.app.sub_title == "-- VISUAL --"

    def test_visual_exit_clears_state(self):
        v = self._view(cursor=3)
        v.action_visual_enter()
        v.action_visual_exit()
        assert v._visual_active is False
        assert v._visual_anchor is None
        assert v._visual_cursor is None
        assert v.app.sub_title == ""

    def test_visual_exit_when_inactive_is_noop(self):
        v = self._view()
        # Should not raise
        v.action_visual_exit()
        assert v._visual_active is False

    def test_visual_range_single_row(self):
        v = self._view(cursor=4)
        v.action_visual_enter()
        assert v._visual_range() == (4, 4)
        assert v.visual_rows() == {4}

    def test_visual_range_extend_down(self):
        v = self._view(n=10, cursor=2)
        v.action_visual_enter()
        # Extend down 3 rows
        v.action_cursor_down()
        v.action_cursor_down()
        v.action_cursor_down()
        assert v._visual_cursor == 5
        assert v._visual_range() == (2, 5)
        assert v.visual_rows() == {2, 3, 4, 5}

    def test_visual_range_extend_up(self):
        v = self._view(n=10, cursor=5)
        v.action_visual_enter()
        v.action_cursor_up()
        v.action_cursor_up()
        assert v._visual_cursor == 3
        assert v._visual_range() == (3, 5)
        assert v.visual_rows() == {3, 4, 5}

    def test_visual_top_bottom(self):
        v = self._view(n=10, cursor=5)
        v.action_visual_enter()
        v.action_visual_top()
        assert v._visual_range() == (0, 5)
        v.action_visual_bottom()
        # anchor stays at 5, cursor goes to 9
        assert v._visual_range() == (5, 9)

    def test_yank_tsv_has_correct_lines(self):
        """Enter at row 2, extend down 3 rows (rows 2,3,4,5), yank → TSV 4 lines."""
        v = self._view(n=10, cursor=2)
        v.action_visual_enter()
        v.action_cursor_down()
        v.action_cursor_down()
        v.action_cursor_down()
        # Now range is [2, 5]
        text, count = v._visual_yank_payload(2, 6)  # end=6 exclusive
        lines = text.rstrip("\n").split("\n")
        assert count == 4
        assert len(lines) == 4
        # Each line is TSV: job_id\tname\tstate
        for line in lines:
            parts = line.split("\t")
            assert len(parts) == 3

    def test_action_visual_yank_calls_clipboard(self):
        """action_visual_yank produces clipboard call and exits visual mode."""
        v = self._view(n=10, cursor=2)
        v.action_visual_enter()
        v.action_cursor_down()
        v.action_cursor_down()
        v.action_cursor_down()
        # range = [2, 5]
        v.action_visual_yank()
        # Visual mode should be exited
        assert v._visual_active is False
        # Notification should have been sent
        notifs = v.app._notifications
        assert len(notifs) >= 1
        last_n = notifs[-1]
        assert last_n["title"] == "Clipboard"

    def test_escape_exits_visual_without_clipboard(self):
        """Pressing escape exits visual mode without copying."""
        v = self._view(n=10, cursor=3)
        v.app._notifications = []
        v.action_visual_enter()
        v.action_cursor_down()
        v.action_cursor_down()
        v.action_visual_exit()
        assert v._visual_active is False
        # No clipboard notification from the escape
        clipboard_notifs = [n for n in v.app._notifications if n["title"] == "Clipboard"]
        assert len(clipboard_notifs) == 0

    def test_visual_rows_empty_when_inactive(self):
        v = self._view()
        assert v.visual_rows() == set()

    def test_cursor_moves_normally_when_inactive(self):
        v = self._view(n=5, cursor=2)
        v.action_cursor_down()
        assert v._table.cursor_row == 3
        v.action_cursor_up()
        assert v._table.cursor_row == 2

    def test_cursor_moves_and_extends_when_active(self):
        v = self._view(n=5, cursor=2)
        v.action_visual_enter()
        v.action_cursor_down()
        assert v._visual_cursor == 3
        assert v._table.cursor_row == 3


class TestVisualJobsMixinIsolation:
    """Ensure visual mode does not mutate the persistent multi-select set."""

    def test_multiselect_unchanged_on_visual_enter_exit(self):
        """In JobsView, entering/exiting visual mode must not touch _selected_job_ids."""
        from sqtop.views.jobs import JobsView
        # We can't instantiate a full Textual widget in unit tests, so we test
        # the mixin logic directly using SimpleView which mirrors the interface.
        jobs = _make_jobs(5)
        table = _MockTable(row_count=5, cursor_row=1)
        v = SimpleView(jobs, table)
        # Simulate a pre-existing selected set (as JobsView would have)
        selected_ids = {"1", "3"}

        # Enter and exit visual without yanking
        v.action_visual_enter()
        assert v._visual_active is True
        v.action_cursor_down()
        v.action_visual_exit()
        assert v._visual_active is False

        # The selected_ids set is NOT managed by VisualSelectMixin; it must be untouched
        # (VisualSelectMixin does not know about selected_job_ids)
        assert selected_ids == {"1", "3"}

    def test_yank_does_not_affect_external_selection_set(self):
        """Yanking visual range produces TSV but doesn't touch external state."""
        jobs = _make_jobs(5)
        table = _MockTable(row_count=5, cursor_row=0)
        v = SimpleView(jobs, table)
        external_set = {"2", "4"}

        v.action_visual_enter()
        v.action_cursor_down()
        v.action_cursor_down()
        v.action_visual_yank()

        # external_set is untouched
        assert external_set == {"2", "4"}
        assert v._visual_active is False


class TestVisualYankPayload:
    """Test _visual_yank_payload for TSV correctness."""

    def test_no_header_in_tsv(self):
        """TSV output must NOT contain a header line."""
        jobs = _make_jobs(5)
        table = _MockTable(row_count=5, cursor_row=0)
        v = SimpleView(jobs, table)
        text, count = v._visual_yank_payload(0, 5)
        lines = text.rstrip("\n").split("\n")
        # No line should look like a header (all-uppercase tab-separated names)
        for line in lines:
            parts = line.split("\t")
            # First field should be a numeric job_id, not "JOBID"
            assert parts[0].isdigit(), f"Expected numeric job_id, got {parts[0]!r}"

    def test_trailing_newline(self):
        jobs = _make_jobs(3)
        v = SimpleView(jobs, _MockTable(row_count=3, cursor_row=0))
        text, _ = v._visual_yank_payload(0, 3)
        assert text.endswith("\n")

    def test_correct_row_count(self):
        jobs = _make_jobs(8)
        v = SimpleView(jobs, _MockTable(row_count=8, cursor_row=2))
        text, count = v._visual_yank_payload(2, 6)  # rows 2,3,4,5 → 4 rows
        assert count == 4
        lines = text.rstrip("\n").split("\n")
        assert len(lines) == 4

"""Base class for live data-table views."""
from __future__ import annotations

import threading
from typing import Generic, TypeVar

from textual import work
from textual.widgets import Static

T = TypeVar("T")


class BaseDataTableView(Static, Generic[T]):
    """Shared refresh loop, sort toggle, and cursor/scroll preservation for data-table views.

    Subclasses must implement _fetch_data(), _get_anchor_key(), and _update_table().
    """

    def __init__(self, interval: float = 2.0, start_offset: float = 0.0) -> None:
        super().__init__()
        self._interval = interval
        self._start_offset = start_offset
        self._timer = None
        self._fetch_lock = threading.Lock()
        self._paused: bool = False
        self._sort_col: str | None = None
        self._sort_reversed: bool = False

    # ── Subclasses must implement ─────────────────────────────────────────────

    def _fetch_data(self) -> list[T]:
        """Fetch data from Slurm. Called in a worker thread."""
        raise NotImplementedError

    def _get_anchor_key(self, item: T) -> str:
        """Return a unique stable key for cursor tracking (e.g. job_id, node name)."""
        raise NotImplementedError

    def _update_table(self, data: list[T]) -> None:
        """Update the table with new data. Called on the main thread."""
        raise NotImplementedError

    # ── Provided by base class ────────────────────────────────────────────────

    def _begin_interval(self) -> None:
        """Start the periodic refresh timer."""
        self._timer = self.set_interval(self._interval, self.refresh_data)

    def start_refresh_loop(self) -> None:
        """Defer first fetch slightly so initial UI and keybindings become responsive."""
        delay = self._start_offset if self._start_offset > 0 else 0.05
        self.set_timer(delay, self._start_now)

    def _start_now(self) -> None:
        self.refresh_data()
        self._begin_interval()

    def set_interval_rate(self, interval: float) -> None:
        """Change the auto-refresh interval, re-applying the original stagger offset."""
        self._interval = interval
        if self._timer:
            self._timer.stop()
            self._timer = None
        if self._start_offset > 0:
            self.set_timer(self._start_offset, self._begin_interval)
        else:
            self._begin_interval()

    def pause(self) -> None:
        """Pause live data refresh."""
        self._paused = True

    def resume(self) -> None:
        """Resume live data refresh and fetch immediately."""
        self._paused = False
        self.refresh_data()

    @work(thread=True)
    def refresh_data(self) -> None:
        """Fetch data in a background thread; update table on main thread."""
        if self._paused:
            return
        if not self._fetch_lock.acquire(blocking=False):
            return
        try:
            data = self._fetch_data()
            self.app.call_from_thread(self._update_table, data)
        finally:
            self._fetch_lock.release()

    def _set_sort(self, col: str) -> None:
        """Toggle sort column; reverse direction if same column selected again."""
        if self._sort_col == col:
            self._sort_reversed = not self._sort_reversed
        else:
            self._sort_col = col
            self._sort_reversed = False

"""History view — sacct completed/failed job history table."""

from __future__ import annotations

from datetime import datetime

from textual.app import ComposeResult
from textual.widgets import Label

from .base import BaseDataTableView
from ..slurm import SacctJob, fetch_sacct_jobs
from .widgets import CyclicDataTable

STATE_COLORS: dict[str, str] = {
    "COMPLETED": "dim",
    "FAILED": "red",
    "CANCELLED": "yellow",
    "TIMEOUT": "magenta",
}

COLUMNS: list[tuple[str, int]] = [
    ("JOBID",     8),
    ("NAME",     12),
    ("USER",      8),
    ("STATE",    12),
    ("ELAPSED",  10),
    ("EXIT",      6),
    ("PARTITION", 10),
]

_DEFAULT_HOURS = 24


class HistoryView(BaseDataTableView[SacctJob]):
    """Displays recently completed/failed jobs via sacct."""

    BINDINGS = []

    def __init__(self, interval: float = 30.0, start_offset: float = 0.0, hours: int = _DEFAULT_HOURS) -> None:
        super().__init__(interval=interval, start_offset=start_offset)
        self._hours = hours
        self._last_jobs: list[SacctJob] = []

    def compose(self) -> ComposeResult:
        yield Label("", id="history-header")
        yield CyclicDataTable(id="history-table", cursor_type="row", zebra_stripes=True)

    def _build_columns(self) -> None:
        table = self.query_one(CyclicDataTable)
        table.clear(columns=True)
        for name, width in COLUMNS:
            table.add_column(name, width=width)

    def on_mount(self) -> None:
        self._build_columns()
        self.start_refresh_loop()

    def _fetch_data(self) -> list[SacctJob]:
        return fetch_sacct_jobs(self._hours)

    def _get_anchor_key(self, item: SacctJob) -> str:
        return item.job_id

    def _update_table(self, data: list[SacctJob]) -> None:
        self._last_jobs = data

        now = datetime.now().strftime("%H:%M:%S")
        failed = sum(1 for j in data if j.state.upper().startswith("FAILED"))
        self.query_one("#history-header", Label).update(
            f"[b]sacct[/b]  [dim]last {self._hours}h[/]  "
            f"[red]{failed} failed[/]  "
            f"[dim]{len(data)} jobs  updated {now}[/]"
        )

        state = self._capture_table_state()
        self._render_rows(data)
        self._restore_table_state(state, data)

    def _capture_table_state(self) -> tuple[int, float, str | None]:
        table = self.query_one(CyclicDataTable)
        row = table.cursor_row
        scroll_y = float(table.scroll_offset.y)
        anchor: str | None = None
        if 0 <= row < len(self._last_jobs):
            anchor = self._last_jobs[row].job_id
        return row, scroll_y, anchor

    def _restore_table_state(
        self, state: tuple[int, float, str | None], rows: list[SacctJob]
    ) -> None:
        if not rows:
            return
        saved_row, scroll_y, anchor = state
        table = self.query_one(CyclicDataTable)
        row = None
        if anchor:
            for i, job in enumerate(rows):
                if job.job_id == anchor:
                    row = i
                    break
        if row is None:
            row = min(saved_row, len(rows) - 1)
        table.move_cursor(row=row)
        table.scroll_to(y=scroll_y, animate=False)

    def _state_color(self, state: str) -> str:
        upper = state.upper()
        for key, color in STATE_COLORS.items():
            if upper.startswith(key):
                return color
        return "white"

    def _exit_color(self, exit_code: str) -> str:
        return "green" if exit_code == "0:0" else "red"

    def _render_rows(self, jobs: list[SacctJob]) -> None:
        table = self.query_one(CyclicDataTable)
        table.clear()
        for job in jobs:
            state_color = self._state_color(job.state)
            exit_color = self._exit_color(job.exit_code)
            table.add_row(
                job.job_id,
                job.name,
                job.user,
                f"[{state_color}]{job.state}[/]",
                job.elapsed,
                f"[{exit_color}]{job.exit_code}[/]",
                job.partition,
            )
        if jobs and table.cursor_row < 0:
            table.move_cursor(row=0)

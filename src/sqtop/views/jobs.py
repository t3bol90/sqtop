"""Jobs view — squeue-like table with auto-refresh."""

from __future__ import annotations

from datetime import datetime

from textual.app import ComposeResult
from textual.binding import Binding
from textual.widgets import DataTable, Label, Static
from textual import work

from ..slurm import Job, fetch_jobs, fetch_log_paths
from .job_actions import JobActionScreen
from .log_viewer import LogViewerScreen
from .widgets import CyclicDataTable

_STATE_ORDER = {"COMPLETING": 0, "RUNNING": 1, "PENDING": 2}


def _job_sort_key(job: Job) -> tuple:
    priority = _STATE_ORDER.get(job.state, 3)
    job_id = int(job.job_id) if job.job_id.isdigit() else 0
    return (priority, job_id)


STATE_COLORS = {
    "RUNNING":   "green",
    "PENDING":   "yellow",
    "FAILED":    "red",
    "CANCELLED": "red",
    "COMPLETED": "dim",
    "TIMEOUT":   "magenta",
    "NODE_FAIL": "red",
    "PREEMPTED": "yellow",
}

# (header, col_width, min_terminal_width_to_show)
COLUMNS: list[tuple[str, int, int]] = [
    ("JOBID",              8,   0),
    ("NAME",              16,   0),
    ("STATE",             12,   0),
    ("USER",              10,  65),
    ("TIME",              10,  65),
    ("PARTITION",         11,  90),
    ("NODES",              6,  90),
    ("CPUS",               6, 105),
    ("TIME_LIMIT",        10, 105),
    ("NODELIST(REASON)",  20, 120),
]


def _visible_cols(width: int) -> list[tuple[str, int]]:
    return [(name, w) for name, w, min_w in COLUMNS if min_w <= width]


class JobsView(Static):
    """Displays a live squeue-style table."""

    BINDINGS = [Binding("enter", "open_job", "Open job", show=True)]

    def __init__(self, interval: float = 2.0) -> None:
        super().__init__()
        self._interval = interval
        self._last_jobs: list[Job] = []
        self._current_cols: list[tuple[str, int]] = []
        self._fetching = False
        self._timer = None

    def compose(self) -> ComposeResult:
        yield Label("", id="jobs-header")
        yield CyclicDataTable(id="jobs-table", cursor_type="row", zebra_stripes=True)

    def on_mount(self) -> None:
        self._rebuild_columns(self.size.width)
        self.refresh_data()
        self._timer = self.set_interval(self._interval, self.refresh_data)

    def set_interval_rate(self, interval: float) -> None:
        self._interval = interval
        if self._timer:
            self._timer.stop()
        self._timer = self.set_interval(self._interval, self.refresh_data)

    def on_resize(self, event) -> None:
        new_cols = _visible_cols(event.size.width)
        if new_cols != self._current_cols:
            self._rebuild_columns(event.size.width)
            self._render_rows(self._last_jobs)

    def _rebuild_columns(self, width: int) -> None:
        self._current_cols = _visible_cols(width)
        table = self.query_one(CyclicDataTable)
        table.clear(columns=True)
        for name, col_width in self._current_cols:
            table.add_column(name, width=col_width)

    @work(thread=True)
    def refresh_data(self) -> None:
        if self._fetching:
            return
        self._fetching = True
        try:
            jobs = fetch_jobs()
            self.app.call_from_thread(self._update_table, jobs)
        finally:
            self._fetching = False

    def _update_table(self, jobs: list[Job]) -> None:
        self._last_jobs = sorted(jobs, key=_job_sort_key)
        self._render_rows(self._last_jobs)
        now = datetime.now().strftime("%H:%M:%S")
        running = sum(1 for j in jobs if j.state == "RUNNING")
        pending = sum(1 for j in jobs if j.state == "PENDING")
        self.query_one("#jobs-header", Label).update(
            f"[b]squeue[/b]  [green]{running} running[/]  "
            f"[yellow]{pending} pending[/]  "
            f"[dim]{len(jobs)} total  updated {now}[/]"
        )

    def _render_rows(self, jobs: list[Job]) -> None:
        table = self.query_one(CyclicDataTable)
        saved_row = table.cursor_row
        table.clear()
        for job in jobs:
            color = STATE_COLORS.get(job.state, "white")
            row = []
            for name, _ in self._current_cols:
                if name == "JOBID":
                    row.append(f"[{color}]{job.job_id}[/]")
                elif name == "NAME":
                    row.append(f"[{color}]{job.name}[/]")
                elif name == "STATE":
                    row.append(f"[{color}]{job.state}[/]")
                elif name == "USER":
                    row.append(job.user)
                elif name == "TIME":
                    row.append(job.time_used)
                elif name == "PARTITION":
                    row.append(job.partition)
                elif name == "NODES":
                    row.append(job.nodes)
                elif name == "CPUS":
                    row.append(job.num_cpus)
                elif name == "TIME_LIMIT":
                    row.append(job.time_limit)
                elif name == "NODELIST(REASON)":
                    row.append(job.nodelist or job.reason)
            table.add_row(*row)
        if jobs:
            table.move_cursor(row=min(saved_row, len(jobs) - 1))

    def on_data_table_row_selected(self, event: DataTable.RowSelected) -> None:
        row_idx = event.cursor_row
        if row_idx >= len(self._last_jobs):
            return
        job = self._last_jobs[row_idx]

        def handle_action(action: str | None) -> None:
            if action is None:
                return
            stdout_path, stderr_path = fetch_log_paths(job.job_id)
            log_path = stdout_path if action == "stdout" else stderr_path
            self.app.push_screen(LogViewerScreen(job.job_id, log_path, action))

        self.app.push_screen(JobActionScreen(job), handle_action)

"""Jobs view — squeue-like table with auto-refresh."""

from __future__ import annotations

import os
import subprocess
import sys
from datetime import datetime

from textual.app import ComposeResult
from textual.binding import Binding
from textual.widgets import DataTable, Input, Label, Static
from textual import work

from ..slurm import Job, cancel_job, fetch_job_detail, fetch_jobs, fetch_log_paths
from .. import config
from .confirm import ConfirmScreen
from .job_actions import JobActionScreen
from .job_detail import JobDetailScreen
from .log_viewer import LogViewerScreen
from .widgets import CyclicDataTable

_STATE_ORDER = {"COMPLETING": 0, "RUNNING": 1, "PENDING": 2}
_TERMINAL_STATES = {"COMPLETED", "FAILED", "CANCELLED", "TIMEOUT", "NODE_FAIL", "PREEMPTED"}


def _job_sort_key(job: Job) -> tuple:
    priority = _STATE_ORDER.get(job.state, 3)
    job_id = int(job.job_id) if job.job_id.isdigit() else 0
    return (priority, job_id)


def _copy_to_clipboard(text: str) -> bool:
    """Copy text to system clipboard. Returns True on success."""
    try:
        if sys.platform == "darwin":
            subprocess.run(["pbcopy"], input=text.encode(), check=True, timeout=2)
        elif sys.platform == "win32":
            subprocess.run(["clip"], input=text.encode(), check=True, timeout=2)
        else:
            try:
                subprocess.run(
                    ["xclip", "-selection", "clipboard"],
                    input=text.encode(), check=True, timeout=2,
                )
            except (FileNotFoundError, subprocess.CalledProcessError):
                subprocess.run(
                    ["xsel", "--clipboard", "--input"],
                    input=text.encode(), check=True, timeout=2,
                )
        return True
    except Exception:
        return False


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

# sort key functions keyed by column name
_SORT_KEYS = {
    "state":  lambda j: (j.state, int(j.job_id) if j.job_id.isdigit() else 0),
    "time":   lambda j: j.time_used,
    "cpus":   lambda j: int(j.num_cpus) if j.num_cpus.isdigit() else 0,
}

# (header, min_col_width, min_terminal_width_to_show)
COLUMNS: list[tuple[str, int, int]] = [
    ("JOBID",              8,   0),
    ("NAME",               8,   0),
    ("STATE",             10,   0),
    ("USER",               8,  65),
    ("TIME",              10,  65),
    ("PARTITION",          9,  90),
    ("NODES",              6,  90),
    ("CPUS",               6, 105),
    ("TIME_LIMIT",        10, 105),
    ("NODELIST(REASON)",  14, 120),
]

_DEFAULT_COL_MAX = {
    "NAME": 24,
    "USER": 12,
    "PARTITION": 14,
    "NODELIST(REASON)": 40,
}

_CONFIG_COL_KEYS = {
    "NAME": "name_max",
    "USER": "user_max",
    "PARTITION": "partition_max",
    "NODELIST(REASON)": "nodelist_reason_max",
}


def _visible_cols(width: int) -> list[tuple[str, int]]:
    return [(name, min_w) for name, min_w, min_term_w in COLUMNS if min_term_w <= width]


def _truncate(text: str, max_len: int | None) -> str:
    if max_len is None or max_len <= 0 or len(text) <= max_len:
        return text
    if max_len <= 3:
        return text[:max_len]
    return text[: max_len - 3] + "..."


def _coerce_positive_int(value: object, default: int) -> int:
    try:
        n = int(value)
        return n if n > 0 else default
    except (TypeError, ValueError):
        return default


class JobsView(Static):
    """Displays a live squeue-style table."""

    BINDINGS = [
        Binding("enter", "open_job", "Open job", show=True),
        Binding("u", "toggle_mine", "My jobs", show=True),
        Binding("slash", "activate_search", "Search", show=True),
        Binding("s", "sort_state", show=False),
        Binding("t", "sort_time", show=False),
        Binding("c", "sort_cpus", show=False),
        Binding("y", "yank_job_id", "Copy ID", show=True),
        Binding("w", "watch_job", "Watch", show=True),
    ]

    def __init__(self, interval: float = 2.0) -> None:
        super().__init__()
        self._interval = interval
        self._last_jobs_raw: list[Job] = []
        self._last_jobs: list[Job] = []
        self._current_cols: list[tuple[str, int]] = []
        self._fetching = False
        self._timer = None
        self._filter_mine: bool = False
        self._search_query: str = ""
        self._sort_col: str | None = None   # None = default state-priority sort
        self._sort_reversed: bool = False
        self._watched_states: dict[str, str] = {}  # job_id → last known state
        cfg = config.load().get("jobs", {})
        self._col_max = dict(_DEFAULT_COL_MAX)
        for col, key in _CONFIG_COL_KEYS.items():
            self._col_max[col] = _coerce_positive_int(cfg.get(key), _DEFAULT_COL_MAX[col])

    def compose(self) -> ComposeResult:
        yield Label("", id="jobs-header")
        yield CyclicDataTable(id="jobs-table", cursor_type="row", zebra_stripes=True)
        yield Input(
            placeholder="Filter by name / state / partition…  Esc to close",
            id="search-bar",
        )

    def on_mount(self) -> None:
        self.query_one("#search-bar", Input).display = False
        self._rebuild_columns(self.size.width, [])
        self.refresh_data()
        self._timer = self.set_interval(self._interval, self.refresh_data)

    def set_interval_rate(self, interval: float) -> None:
        self._interval = interval
        if self._timer:
            self._timer.stop()
        self._timer = self.set_interval(self._interval, self.refresh_data)

    def on_resize(self, event) -> None:
        self._rebuild_columns(event.size.width, self._last_jobs)
        self._render_rows(self._last_jobs)

    def _plain_cell(self, job: Job, col_name: str) -> str:
        if col_name == "JOBID":
            return job.job_id
        if col_name == "NAME":
            return job.name
        if col_name == "STATE":
            return job.state
        if col_name == "USER":
            return job.user
        if col_name == "TIME":
            return job.time_used
        if col_name == "PARTITION":
            return job.partition
        if col_name == "NODES":
            return job.nodes
        if col_name == "CPUS":
            return job.num_cpus
        if col_name == "TIME_LIMIT":
            return job.time_limit
        return job.nodelist or job.reason

    def _cell_text(self, job: Job, col_name: str) -> str:
        return _truncate(self._plain_cell(job, col_name), self._col_max.get(col_name))

    def _rebuild_columns(self, width: int, jobs: list[Job]) -> None:
        visible = _visible_cols(width)
        new_cols: list[tuple[str, int]] = []
        for col_name, min_w in visible:
            if jobs:
                longest = max(
                    len(col_name),
                    *(len(self._cell_text(job, col_name)) for job in jobs),
                )
            else:
                longest = len(col_name)
            max_w = self._col_max.get(col_name, max(min_w, longest + 1))
            col_width = max(min_w, min(longest + 1, max_w))
            new_cols.append((col_name, col_width))

        if new_cols == self._current_cols:
            return
        self._current_cols = new_cols
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

    # ── Actions ──────────────────────────────────────────────────────────────

    def action_toggle_mine(self) -> None:
        self._filter_mine = not self._filter_mine
        self._update_table(self._last_jobs_raw)

    def action_activate_search(self) -> None:
        bar = self.query_one("#search-bar", Input)
        bar.display = True
        bar.focus()

    def _set_sort(self, col: str) -> None:
        if self._sort_col == col:
            self._sort_reversed = not self._sort_reversed
        else:
            self._sort_col = col
            self._sort_reversed = False
        self._update_table(self._last_jobs_raw)

    def action_sort_state(self) -> None:
        self._set_sort("state")

    def action_sort_time(self) -> None:
        self._set_sort("time")

    def action_sort_cpus(self) -> None:
        self._set_sort("cpus")

    def action_yank_job_id(self) -> None:
        table = self.query_one(CyclicDataTable)
        row_idx = table.cursor_row
        if row_idx >= len(self._last_jobs):
            return
        job = self._last_jobs[row_idx]
        if _copy_to_clipboard(job.job_id):
            self.app.notify(f"Copied: {job.job_id}", title="Clipboard")
        else:
            self.app.notify("Clipboard unavailable", severity="warning")

    def action_watch_job(self) -> None:
        table = self.query_one(CyclicDataTable)
        row_idx = table.cursor_row
        if row_idx >= len(self._last_jobs):
            return
        job = self._last_jobs[row_idx]
        if job.job_id in self._watched_states:
            del self._watched_states[job.job_id]
            self.app.notify(f"Unwatched job {job.job_id}", title="Watch")
        else:
            self._watched_states[job.job_id] = job.state
            self.app.notify(f"Watching job {job.job_id} ({job.name})", title="Watch")
        self._render_rows(self._last_jobs)

    # ── Input / key events ────────────────────────────────────────────────────

    def on_input_changed(self, event: Input.Changed) -> None:
        if event.input.id == "search-bar":
            self._search_query = event.value
            self._update_table(self._last_jobs_raw)

    def on_input_submitted(self, event: Input.Submitted) -> None:
        if event.input.id == "search-bar":
            self._dismiss_search()

    def on_key(self, event) -> None:
        if event.key == "escape":
            bar = self.query_one("#search-bar", Input)
            if bar.display:
                self._dismiss_search()
                event.stop()

    def _dismiss_search(self) -> None:
        bar = self.query_one("#search-bar", Input)
        bar.display = False
        bar.value = ""
        self._search_query = ""
        self._update_table(self._last_jobs_raw)
        self.query_one(CyclicDataTable).focus()

    # ── Data pipeline ────────────────────────────────────────────────────────

    def _update_table(self, jobs: list[Job]) -> None:
        self._last_jobs_raw = jobs
        self._check_watched_jobs(jobs)

        filtered = jobs
        if self._filter_mine:
            user = os.getenv("USER", "")
            filtered = [j for j in filtered if j.user == user]
        if self._search_query:
            q = self._search_query.lower()
            filtered = [
                j for j in filtered
                if q in j.name.lower() or q in j.state.lower() or q in j.partition.lower()
            ]

        if self._sort_col is None:
            self._last_jobs = sorted(filtered, key=_job_sort_key)
        else:
            key_fn = _SORT_KEYS[self._sort_col]
            self._last_jobs = sorted(filtered, key=key_fn, reverse=self._sort_reversed)

        self._rebuild_columns(self.size.width, self._last_jobs)
        self._render_rows(self._last_jobs)
        self._update_header(jobs)

    def _update_header(self, all_jobs: list[Job]) -> None:
        now = datetime.now().strftime("%H:%M:%S")
        running = sum(1 for j in all_jobs if j.state == "RUNNING")
        pending = sum(1 for j in all_jobs if j.state == "PENDING")

        tags: list[str] = []
        if self._filter_mine:
            tags.append("[cyan]· mine[/]")
        if self._search_query:
            tags.append(f'[yellow]· "{self._search_query}"[/]')
        if self._sort_col is not None:
            arrow = "↑" if self._sort_reversed else "↓"
            tags.append(f"[dim]sort:{self._sort_col}{arrow}[/]")
        if self._watched_states:
            tags.append(f"[magenta]· {len(self._watched_states)} watched[/]")

        suffix = ("  " + "  ".join(tags)) if tags else ""
        self.query_one("#jobs-header", Label).update(
            f"[b]squeue[/b]  [green]{running} running[/]  "
            f"[yellow]{pending} pending[/]  "
            f"[dim]{len(all_jobs)} total  updated {now}[/]"
            f"{suffix}"
        )

    def _check_watched_jobs(self, jobs: list[Job]) -> None:
        if not self._watched_states:
            return
        current = {j.job_id: j.state for j in jobs}
        finished = []
        for job_id, last_state in self._watched_states.items():
            cur = current.get(job_id)
            if cur is None or cur in _TERMINAL_STATES:
                state_str = cur if cur else "gone from queue"
                self.app.bell()
                self.app.notify(
                    f"Job {job_id} → {state_str}",
                    title="Job finished",
                    severity="information",
                    timeout=10,
                )
                finished.append(job_id)
            elif cur != last_state:
                self._watched_states[job_id] = cur
        for job_id in finished:
            del self._watched_states[job_id]

    def _render_rows(self, jobs: list[Job]) -> None:
        table = self.query_one(CyclicDataTable)
        saved_row = table.cursor_row
        table.clear()
        for job in jobs:
            color = STATE_COLORS.get(job.state, "white")
            watched_prefix = "★ " if job.job_id in self._watched_states else ""
            row = []
            for name, _ in self._current_cols:
                if name == "JOBID":
                    row.append(f"[{color}]{watched_prefix}{self._cell_text(job, name)}[/]")
                elif name == "NAME":
                    row.append(f"[{color}]{self._cell_text(job, name)}[/]")
                elif name == "STATE":
                    row.append(f"[{color}]{self._cell_text(job, name)}[/]")
                else:
                    row.append(self._cell_text(job, name))
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
            if action == "detail":
                data = fetch_job_detail(job.job_id)
                self.app.push_screen(JobDetailScreen(job.job_id, data))
            elif action == "cancel":
                def do_cancel(confirmed: bool) -> None:
                    if confirmed:
                        cancel_job(job.job_id)
                self.app.push_screen(
                    ConfirmScreen(f"Cancel job {job.job_id} ({job.name})?"),
                    do_cancel,
                )
            else:
                stdout_path, stderr_path = fetch_log_paths(job.job_id)
                log_path = stdout_path if action == "stdout" else stderr_path
                self.app.push_screen(LogViewerScreen(job.job_id, log_path, action))

        self.app.push_screen(JobActionScreen(job), handle_action)

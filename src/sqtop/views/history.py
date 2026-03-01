"""History view — sacct completed/failed job history table."""

from __future__ import annotations

import os
from datetime import datetime

from textual.app import ComposeResult
from textual.binding import Binding
from textual.screen import ModalScreen
from textual.widgets import Button, Label, Static

from .base import BaseDataTableView
from .mixins import ModalButtonNavMixin
from ..slurm import SacctJob, fetch_log_paths, fetch_sacct_jobs
from .log_viewer import LogViewerScreen, LOG_STDOUT, LOG_STDERR
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


class HistoryActionScreen(ModalButtonNavMixin, ModalScreen[str | None]):
    """Action menu for a completed/failed job in the history view."""

    BINDINGS = [
        *ModalButtonNavMixin.BINDINGS,
        Binding("escape", "dismiss(None)", show=False),
    ]

    CSS = """
    HistoryActionScreen { align: center middle; }
    #dialog {
        width: 50; height: auto;
        border: double $primary;
        background: $surface;
        padding: 1 2;
    }
    #dialog Label { text-style: bold; color: $primary; }
    #btn-stdout, #btn-stderr, #btn-close { width: 100%; margin-top: 1; }
    """

    def __init__(self, job: SacctJob) -> None:
        super().__init__()
        self._job = job

    def compose(self) -> ComposeResult:
        with Static(id="dialog"):
            yield Label(f"Job {self._job.job_id} — {self._job.name}")
            yield Label(f"State: {self._job.state}  User: {self._job.user}")
            yield Button("View stdout log", id="btn-stdout", variant="primary")
            yield Button("View stderr log", id="btn-stderr", variant="default")
            yield Button("Close  [dim]esc[/]", id="btn-close", variant="default")

    def on_button_pressed(self, event: Button.Pressed) -> None:
        if event.button.id == "btn-stdout":
            self.dismiss(LOG_STDOUT)
        elif event.button.id == "btn-stderr":
            self.dismiss(LOG_STDERR)
        else:
            self.dismiss(None)


class HistoryView(BaseDataTableView[SacctJob]):
    """Displays recently completed/failed jobs via sacct."""

    BINDINGS = [
        Binding("enter", "open_job", "Open", show=True),
        Binding("u", "toggle_mine", "My jobs", show=False),
    ]

    def __init__(self, interval: float = 30.0, start_offset: float = 0.0, hours: int = _DEFAULT_HOURS) -> None:
        super().__init__(interval=interval, start_offset=start_offset)
        self._hours = hours
        self._last_jobs_raw: list[SacctJob] = []
        self._last_jobs: list[SacctJob] = []
        self._filter_mine: bool = False

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

    def _job_for_cursor(self) -> SacctJob | None:
        table = self.query_one(CyclicDataTable)
        row = table.cursor_row
        if 0 <= row < len(self._last_jobs):
            return self._last_jobs[row]
        return None

    def action_toggle_mine(self) -> None:
        self._filter_mine = not self._filter_mine
        self._update_table(self._last_jobs_raw)

    def action_open_job(self) -> None:
        job = self._job_for_cursor()
        if not job:
            return

        def handle_action(action: str | None) -> None:
            if action in (LOG_STDOUT, LOG_STDERR):
                stdout_path, stderr_path = fetch_log_paths(job.job_id)
                log_path = stdout_path if action == LOG_STDOUT else stderr_path
                if not log_path:
                    self.app.notify("No log path found for this job", severity="warning")
                    return
                self.app.push_screen(LogViewerScreen(job.job_id, log_path, action))

        self.app.push_screen(HistoryActionScreen(job), handle_action)

    def _update_table(self, data: list[SacctJob]) -> None:
        self._last_jobs_raw = data

        filtered = data
        if self._filter_mine:
            user = os.getenv("USER", "")
            filtered = [j for j in filtered if j.user == user]
        self._last_jobs = filtered

        now = datetime.now().strftime("%H:%M:%S")
        failed = sum(1 for j in filtered if j.state.upper().startswith("FAILED"))
        tags = "[cyan]· mine[/]  " if self._filter_mine else ""
        total_str = f"{len(filtered)}/{len(data)} jobs" if self._filter_mine else f"{len(data)} jobs"
        self.query_one("#history-header", Label).update(
            f"[b]sacct[/b]  [dim]last {self._hours}h[/]  "
            f"{tags}"
            f"[red]{failed} failed[/]  "
            f"[dim]{total_str}  updated {now}[/]"
        )

        state = self._capture_table_state()
        self._render_rows(filtered)
        self._restore_table_state(state, filtered)

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

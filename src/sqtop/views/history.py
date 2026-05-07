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
from ..responsive import (
    ColumnSpec,
    CHROME_OVERHEAD,
    allocate_columns,
    tier_for,
    truncate_cell,
    WidthChanged,
)

STATE_COLORS: dict[str, str] = {
    "COMPLETED": "dim",
    "FAILED": "red",
    "CANCELLED": "yellow",
    "TIMEOUT": "magenta",
}

# ColumnSpec(name, min_width, content_max, priority, min_tier)
COLUMNS: list[ColumnSpec] = [
    ColumnSpec("JOBID",      8, 12, 100, "xs"),
    ColumnSpec("STATE",     12, 16,  95, "xs"),
    ColumnSpec("ELAPSED",   10, 12,  90, "xs"),
    ColumnSpec("NAME",      12, 24,  80, "sm"),
    ColumnSpec("USER",       8, 12,  75, "sm"),
    ColumnSpec("EXIT",       6,  8,  70, "sm"),
    ColumnSpec("PARTITION", 10, 14,  60, "md"),
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
        width: 50; max-width: 90%; height: auto;
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
        Binding("v", "visual_enter", "Visual", show=False),
        Binding("V", "visual_enter", "Visual", show=False),
        Binding("escape", "visual_exit", "Exit visual", show=False),
        Binding("y", "yank", "Copy", show=False),
    ]

    def __init__(self, interval: float = 30.0, start_offset: float = 0.0, hours: int = _DEFAULT_HOURS) -> None:
        super().__init__(interval=interval, start_offset=start_offset)
        self._hours = hours
        self._last_jobs_raw: list[SacctJob] = []
        self._last_jobs: list[SacctJob] = []
        self._filter_mine: bool = False
        self._current_cols: list[tuple[str, int]] = []
        self._rebuild_cache_width: int = -1
        self._rebuild_cache_names: list[str] = []

    def compose(self) -> ComposeResult:
        yield Label("", id="history-header")
        yield CyclicDataTable(id="history-table", cursor_type="row", zebra_stripes=True)

    def _build_columns(self, width: int | None = None, *, force: bool = False) -> bool:
        """Build/rebuild column layout using budget allocation. Returns True if changed."""
        w = width if width is not None else (self._rebuild_cache_width if self._rebuild_cache_width > 0 else 80)
        budget = max(0, w - CHROME_OVERHEAD)
        new_cols = allocate_columns(budget, list(COLUMNS), current_tier=tier_for(w))
        visible_names = [n for n, _ in new_cols]

        if (
            not force
            and w == self._rebuild_cache_width
            and visible_names == self._rebuild_cache_names
        ):
            return False

        self._rebuild_cache_width = w
        self._rebuild_cache_names = visible_names

        if new_cols == self._current_cols:
            return False

        self._current_cols = new_cols
        table = self.query_one(CyclicDataTable)
        table.clear(columns=True)
        for name, col_width in self._current_cols:
            table.add_column(name, width=col_width)
        return True

    def on_width_changed(self, event: WidthChanged) -> None:
        """Recompute column budget on every resize (spec §4.2)."""
        state = self._capture_table_state()
        changed = self._build_columns(event.width)
        if changed and self._last_jobs:
            self._render_rows(self._last_jobs)
            self._restore_table_state(state, self._last_jobs)

    def on_mount(self) -> None:
        self._build_columns(force=True)
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

    def action_yank(self) -> None:
        """Visual yank when in visual mode; no-op otherwise."""
        if self._visual_active:
            self.action_visual_yank()

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
        visual_set = self.visual_rows()
        col_widths = dict(self._current_cols)
        table.clear()
        for idx, job in enumerate(jobs):
            state_color = self._state_color(job.state)
            exit_color = self._exit_color(job.exit_code)
            visual_prefix = "» " if idx in visual_set else ""
            row = []
            for name, w in self._current_cols:
                plain = self._plain_cell(job, name)
                cell = truncate_cell(plain, w)
                if name == "JOBID":
                    row.append(f"{visual_prefix}{cell}")
                elif name == "STATE":
                    row.append(f"[{state_color}]{cell}[/]")
                elif name == "EXIT":
                    exit_color = self._exit_color(job.exit_code)
                    row.append(f"[{exit_color}]{cell}[/]")
                else:
                    row.append(cell)
            table.add_row(*row)
        if jobs and table.cursor_row < 0:
            table.move_cursor(row=0)

    # ── Copy-pane interface ───────────────────────────────────────────────────

    def _pane_label(self) -> str:
        return "History"

    def _current_items(self) -> list[SacctJob]:
        return list(self._last_jobs)

    def _plain_cell(self, job: SacctJob, col_name: str) -> str:
        if col_name == "JOBID":
            return job.job_id
        if col_name == "NAME":
            return job.name
        if col_name == "USER":
            return job.user
        if col_name == "STATE":
            return job.state
        if col_name == "ELAPSED":
            return job.elapsed
        if col_name == "EXIT":
            return job.exit_code
        if col_name == "PARTITION":
            return job.partition
        return ""

    def _row_tsv(self, item: SacctJob) -> str:
        cols = self._current_cols if self._current_cols else [(col.name, col.min_width) for col in COLUMNS]
        return "\t".join(self._plain_cell(item, name) for name, _ in cols)

    def copy_pane(self) -> tuple[str, str, int]:
        """Return (label, tsv_payload, row_count) for the history pane."""
        cols = self._current_cols if self._current_cols else [(col.name, col.min_width) for col in COLUMNS]
        header = "\t".join(name for name, _ in cols)
        items = self._current_items()
        rows = [self._row_tsv(item) for item in items]
        payload = "\n".join([header, *rows]) + "\n"
        return self._pane_label(), payload, len(rows)

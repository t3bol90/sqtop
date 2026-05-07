"""Jobs view — squeue-like table with auto-refresh."""

from __future__ import annotations

import os
import shlex
from datetime import datetime

from rich.text import Text
from textual import work
from textual.app import ComposeResult
from textual.binding import Binding
from textual.widgets import DataTable, Input, Label

from ..slurm import (
    ActionResult,
    Job,
    build_attach_command,
    fetch_job_detail,
    fetch_jobs,
    fetch_log_paths,
    resolve_first_node,
    run_bulk_job_action,
    run_job_action,
    run_attach_command,
)
from .. import config
from ..columns import _reconcile_order
from ..responsive import (
    ColumnSpec,
    CHROME_OVERHEAD,
    allocate_columns,
    tier_for,
    truncate_cell,
    WidthChanged,
)
from .base import BaseDataTableView
from .attach_prompt import AttachNodePromptScreen
from .bulk_actions import BulkActionScreen
from .confirm import ConfirmScreen
from .job_actions import JobActionScreen
from .job_detail import JobDetailScreen
from .job_info import JobInfoScreen
from .array_tasks import ArrayTaskScreen
from .batch_script import BatchScriptScreen
from .log_viewer import LogViewerScreen, LOG_STDOUT, LOG_STDERR
from .widgets import CyclicDataTable

_STATE_ORDER = {"COMPLETING": 0, "RUNNING": 1, "PENDING": 2}
_TERMINAL_STATES = {"COMPLETED", "FAILED", "CANCELLED", "TIMEOUT", "NODE_FAIL", "PREEMPTED"}
_ATTACH_STATES = {"RUNNING"}


def _parse_slurm_duration(s: str) -> int:
    """Convert a Slurm time string to total seconds.

    Supports: D-HH:MM:SS, HH:MM:SS, MM:SS, SS.
    Returns -1 for UNLIMITED, INVALID, empty string, or unparseable values.
    """
    if not s:
        return -1
    su = s.strip().upper()
    if su in {"UNLIMITED", "INVALID", "INFINITE", "N/A", "NOT_SET"}:
        return -1

    days = 0
    rest = su
    if "-" in rest:
        day_part, _, rest = rest.partition("-")
        try:
            days = int(day_part)
        except ValueError:
            return -1

    parts = rest.split(":")
    try:
        if len(parts) == 3:
            hours, minutes, seconds = int(parts[0]), int(parts[1]), int(parts[2])
        elif len(parts) == 2:
            hours, minutes, seconds = 0, int(parts[0]), int(parts[1])
        elif len(parts) == 1:
            hours, minutes, seconds = 0, 0, int(parts[0])
        else:
            return -1
    except ValueError:
        return -1

    return days * 86400 + hours * 3600 + minutes * 60 + seconds


def _format_duration(total_seconds: int) -> str:
    """Format seconds back into D-HH:MM:SS or HH:MM:SS."""
    if total_seconds < 0:
        return "—"
    days, remainder = divmod(total_seconds, 86400)
    hours, remainder = divmod(remainder, 3600)
    minutes, seconds = divmod(remainder, 60)
    if days > 0:
        return f"{days}-{hours:02d}:{minutes:02d}:{seconds:02d}"
    return f"{hours:02d}:{minutes:02d}:{seconds:02d}"


def _time_left(job: Job) -> tuple[str, str]:
    """Return (display_str, color) for remaining wall-clock time."""
    limit_secs = _parse_slurm_duration(job.time_limit)
    if limit_secs < 0:
        return ("UNLIMITED", "dim")

    used_secs = _parse_slurm_duration(job.time_used)
    if used_secs < 0:
        return ("—", "dim")

    remaining = limit_secs - used_secs
    if remaining < 0:
        remaining = 0

    display = _format_duration(remaining)

    if limit_secs == 0:
        color = "dim"
    else:
        pct = remaining / limit_secs
        if pct > 0.50:
            color = "green"
        elif pct >= 0.10:
            color = "yellow"
        else:
            color = "red"

    return (display, color)


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

# sort key functions keyed by column name
_SORT_KEYS = {
    "state":  lambda j: (j.state, int(j.job_id) if j.job_id.isdigit() else 0),
    "time":   lambda j: j.time_used,
    "cpus":   lambda j: int(j.num_cpus) if j.num_cpus.isdigit() else 0,
    "qos":    lambda j: (j.qos.lower(), _job_sort_key(j)),
}

# Default content_max widths for each column.
_DEFAULT_COL_MAX = {
    "JOBID":             12,
    "NAME":              24,
    "STATE":             14,
    "USER":              12,
    "TIME":              12,
    "TIME_LEFT":         12,
    "PARTITION":         14,
    "QOS":               12,
    "NODES":              8,
    "CPUS":               8,
    "TIME_LIMIT":        12,
    "NODELIST(REASON)":  40,
}

_CONFIG_COL_KEYS = {
    "NAME": "name_max",
    "USER": "user_max",
    "PARTITION": "partition_max",
    "QOS": "qos_max",
    "NODELIST(REASON)": "nodelist_reason_max",
}

# ColumnSpec(name, min_width, content_max, priority, min_tier)
# content_max will be overridden at runtime from config via _make_columns().
COLUMNS: list[ColumnSpec] = [
    ColumnSpec("JOBID",             8,  12, 100, "xs"),
    ColumnSpec("STATE",            10,  14,  95, "xs"),
    ColumnSpec("NAME",              8,  24,  90, "xs"),
    ColumnSpec("USER",              8,  12,  80, "sm"),
    ColumnSpec("TIME",             10,  12,  75, "sm"),
    ColumnSpec("TIME_LEFT",        10,  12,  70, "sm"),
    ColumnSpec("PARTITION",         9,  14,  60, "md"),
    ColumnSpec("NODES",             6,   8,  55, "md"),
    ColumnSpec("CPUS",              6,   8,  50, "md"),
    ColumnSpec("QOS",               8,  12,  45, "md"),
    ColumnSpec("TIME_LIMIT",       10,  12,  40, "md"),
    ColumnSpec("NODELIST(REASON)", 14,  40,  30, "lg"),
]


def _coerce_positive_int(value: object, default: int) -> int:
    try:
        n = int(value)
        return n if n > 0 else default
    except (TypeError, ValueError):
        return default


def _coerce_bool(value: object, default: bool) -> bool:
    if isinstance(value, bool):
        return value
    if isinstance(value, str):
        return value.strip().lower() in {"1", "true", "yes", "on"}
    return default


class JobsView(BaseDataTableView[Job]):
    """Displays a live squeue-style table."""

    BINDINGS = [
        Binding("enter", "open_job", "Open", show=True),
        Binding("u", "toggle_mine", "My jobs", show=True),
        Binding("slash", "activate_search", "Search", show=True),
        Binding("space", "toggle_select", "Select", show=True),
        Binding("asterisk", "select_all_visible", "Select all", show=False),
        Binding("x", "clear_selection", "Clear selected", show=False),
        Binding("B", "bulk_actions", "Bulk", show=True),
        Binding("h", "hold_jobs", "Hold", show=False),
        Binding("R", "release_jobs", "Release", show=False),
        Binding("e", "requeue_jobs", "Requeue", show=False),
        Binding("s", "sort_state", show=False),
        Binding("t", "sort_time", show=False),
        Binding("c", "sort_cpus", show=False),
        Binding("full_stop",            "cycle_reorder_target", show=False),
        Binding("left_square_bracket",  "shift_column_left",    show=False),
        Binding("right_square_bracket", "shift_column_right",   show=False),
        Binding("y", "yank", "Copy", show=False),
        Binding("Y", "yank_row", "Copy row", show=False),
        Binding("v", "visual_enter", "Visual", show=False),
        Binding("V", "visual_enter", "Visual", show=False),
        Binding("escape", "escape_or_visual_exit", "Exit", show=False),
        Binding("w", "watch_job", "Watch", show=True),
        Binding("D", "view_dependencies", "Deps", show=False),
        Binding("f", "cycle_state_filter", "Filter", show=True),
        Binding("i", "job_info", "Info", show=True),
        Binding("l", "view_log", "Log", show=True),
        Binding("d", "show_detail", "Detail", show=False),
        Binding("a", "expand_array", "Array", show=False),
    ]

    def __init__(self, interval: float = 2.0, start_offset: float = 0.0) -> None:
        super().__init__(interval=interval, start_offset=start_offset)
        self._last_jobs_raw: list[Job] = []
        self._last_jobs: list[Job] = []
        self._last_jobs_index: dict[str, int] = {}
        self._current_cols: list[tuple[str, int]] = []
        self._rebuild_cache_width: int = -1
        self._rebuild_cache_names: list[str] = []
        self._rebuild_cache_had_jobs: bool = False
        self._rebuild_cache_tier: str = ""
        self._filter_mine: bool = False
        self._filter_state: str = ""
        self._search_query: str = ""
        self._watched_states: dict[str, str] = {}  # job_id → last known state
        cfg_all = config.load()
        cfg = cfg_all.get("jobs", {})
        self._col_max = dict(_DEFAULT_COL_MAX)
        for col, key in _CONFIG_COL_KEYS.items():
            self._col_max[col] = _coerce_positive_int(cfg.get(key), _DEFAULT_COL_MAX[col])
        attach_cfg = cfg_all.get("attach", {})
        self._attach_enabled = _coerce_bool(attach_cfg.get("enabled", True), True)
        self._attach_default_command = str(attach_cfg.get("default_command", "$SHELL -l"))
        self._attach_extra_args = str(attach_cfg.get("extra_args", ""))
        ui_cfg = cfg_all.get("ui", {})
        safety_cfg = cfg_all.get("safety", {})
        self._expert_mode = _coerce_bool(ui_cfg.get("expert_mode", False), False)
        self._confirm_cancel_single = _coerce_bool(
            safety_cfg.get("confirm_cancel_single", True), True
        )
        self._confirm_bulk_actions = _coerce_bool(
            safety_cfg.get("confirm_bulk_actions", True), True
        )
        self._selected_job_ids: set[str] = set()
        view_state = cfg_all.get("view_state", {})
        saved_sort = str(view_state.get("jobs_sort_col", ""))
        if saved_sort in _SORT_KEYS:
            self._sort_col = saved_sort
            self._sort_reversed = bool(view_state.get("jobs_sort_reversed", False))
        self._hidden_cols: set[str] = set(cfg_all.get("columns", {}).get("jobs_hidden", []))
        saved_order = list(cfg_all.get("columns", {}).get("jobs_order", []))
        default_order = [c.name for c in COLUMNS]
        self._column_order: list[str] = _reconcile_order(saved_order, default_order)
        self._reorder_target_idx: int = 0
        self._warn_pending_ratio = float(cfg_all.get("health", {}).get("warn_pending_ratio", 0.7))
        self._desktop_notify_enabled = bool(
            cfg_all.get("notifications", {}).get("desktop_enabled", True)
        )
        self._last_render_fp: tuple = ()
        self._fp_skip_count: int = 0

    def compose(self) -> ComposeResult:
        yield Label("", id="jobs-header")
        yield CyclicDataTable(id="jobs-table", cursor_type="row", zebra_stripes=True)
        yield Input(
            placeholder="Filter by name / state / partition…  Esc to close",
            id="search-bar",
        )

    def on_mount(self) -> None:
        self.query_one("#search-bar", Input).display = False
        self._rebuild_columns(self.size.width, [], force=True)
        self.start_refresh_loop()

    def _fetch_data(self) -> list[Job]:
        return fetch_jobs()

    def _get_anchor_key(self, item: Job) -> str:
        return item.job_id

    def on_resize(self, event) -> None:
        # Handled via WidthChanged broadcast from app; also keep local fallback.
        state = self._capture_table_state()
        self._rebuild_columns(event.size.width, self._last_jobs, force=True)
        self._render_rows(self._last_jobs)
        self._restore_table_state(state, self._last_jobs)

    def on_width_changed(self, event: WidthChanged) -> None:
        """Recompute column budget on every resize (spec §4.2)."""
        state = self._capture_table_state()
        self._rebuild_columns(event.width, self._last_jobs)
        if self._last_jobs:
            self._render_rows(self._last_jobs)
            self._restore_table_state(state, self._last_jobs)

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
        if col_name == "TIME_LEFT":
            display, _ = _time_left(job)
            return display
        if col_name == "PARTITION":
            return job.partition
        if col_name == "QOS":
            return job.qos or ""
        if col_name == "NODES":
            return job.nodes
        if col_name == "CPUS":
            return job.num_cpus
        if col_name == "TIME_LIMIT":
            return job.time_limit
        return job.nodelist or job.reason

    def _cell_text(self, job: Job, col_name: str, col_width: int | None = None) -> str:
        text = self._plain_cell(job, col_name)
        # Truncate to assigned column width if provided, else fall back to col_max.
        if col_width is not None:
            return truncate_cell(text, col_width)
        max_len = self._col_max.get(col_name)
        if max_len is None:
            return text
        return truncate_cell(text, max_len)

    # ── Copy-pane interface ───────────────────────────────────────────────────

    def _pane_label(self) -> str:
        return "Jobs"

    def _current_items(self) -> list[Job]:
        return list(self._last_jobs)

    def _row_tsv(self, item: Job) -> str:
        return "\t".join(self._plain_cell(item, name) for name, _ in self._current_cols)

    def _make_columns(self) -> list[ColumnSpec]:
        """Build ColumnSpec list respecting _column_order and _hidden_cols."""
        col_lookup = {c.name: c for c in COLUMNS}
        result = []
        for name in self._column_order:
            col = col_lookup.get(name)
            if col is None:
                continue
            if name in self._hidden_cols:
                continue
            result.append(
                ColumnSpec(
                    col.name,
                    col.min_width,
                    self._col_max.get(col.name, col.content_max),
                    col.priority,
                    col.min_tier,
                )
            )
        return result

    def _visible_cols_filtered(self, width: int) -> list[tuple[str, int]]:
        """Legacy helper kept for NodesView-style callers; uses allocate_columns."""
        budget = max(0, width - CHROME_OVERHEAD)
        cols = self._make_columns()
        return allocate_columns(budget, cols, current_tier=tier_for(width))

    def _rebuild_columns(self, width: int, jobs: list[Job], *, force: bool = False) -> None:
        budget = max(0, width - CHROME_OVERHEAD)
        current_tier = tier_for(width)
        cols = self._make_columns()

        # Compute target widths via budget algorithm.
        if jobs:
            # Measure actual content lengths and use as content_max hint.
            content_cols: list[ColumnSpec] = []
            for col in cols:
                longest = max(
                    len(col.name),
                    *(len(self._cell_text(job, col.name)) for job in jobs),
                )
                capped = min(longest + 1, col.content_max)
                effective_max = max(col.min_width, capped)
                content_cols.append(ColumnSpec(col.name, col.min_width, effective_max, col.priority, col.min_tier))
        else:
            # No data: use header width as minimum content hint.
            content_cols = [
                ColumnSpec(col.name, col.min_width, max(col.min_width, len(col.name) + 1), col.priority, col.min_tier)
                for col in cols
            ]

        new_cols = allocate_columns(budget, content_cols, current_tier=current_tier)
        visible_names = [n for n, _ in new_cols]
        has_jobs = bool(jobs)

        if (
            not force
            and width == self._rebuild_cache_width
            and visible_names == self._rebuild_cache_names
            and current_tier == self._rebuild_cache_tier
        ):
            # Rebuild only on empty -> non-empty transitions at same width/layout.
            if not (has_jobs and not self._rebuild_cache_had_jobs):
                self._rebuild_cache_had_jobs = has_jobs
                return

        self._rebuild_cache_width = width
        self._rebuild_cache_names = visible_names
        self._rebuild_cache_had_jobs = has_jobs
        self._rebuild_cache_tier = current_tier
        if not force and new_cols == self._current_cols:
            return
        self._current_cols = new_cols
        table = self.query_one(CyclicDataTable)
        table.clear(columns=True)
        target_idx = self._reorder_target_idx % max(1, len(self._current_cols))
        for idx, (name, col_width) in enumerate(self._current_cols):
            label: object = Text(name, style="reverse bold") if idx == target_idx else name
            table.add_column(label, width=col_width)

    def _capture_table_state(self) -> tuple[int, float, str | None]:
        table = self.query_one(CyclicDataTable)
        row = table.cursor_row
        scroll_y = float(table.scroll_offset.y)
        anchor: str | None = None
        if 0 <= row < len(self._last_jobs):
            anchor = self._last_jobs[row].job_id
        return row, scroll_y, anchor

    def _restore_table_state(self, state: tuple[int, float, str | None], jobs: list[Job]) -> None:
        if not jobs:
            return
        saved_row, scroll_y, anchor = state
        table = self.query_one(CyclicDataTable)
        row = self._last_jobs_index.get(anchor) if anchor else None
        if row is None:
            row = min(saved_row, len(jobs) - 1)
        table.move_cursor(row=row)
        table.scroll_to(y=scroll_y, animate=False)

    # ── Actions ──────────────────────────────────────────────────────────────

    def action_toggle_mine(self) -> None:
        self._filter_mine = not self._filter_mine
        self._update_table(self._last_jobs_raw)

    def action_cycle_state_filter(self) -> None:
        _CYCLE = ("", "RUNNING", "PENDING", "FAILED")
        current_idx = _CYCLE.index(self._filter_state) if self._filter_state in _CYCLE else 0
        self._filter_state = _CYCLE[(current_idx + 1) % len(_CYCLE)]
        self._update_table(self._last_jobs_raw)
        self.notify(f"Filter: {self._filter_state or 'ALL'}", title="State Filter")

    def action_activate_search(self) -> None:
        bar = self.query_one("#search-bar", Input)
        bar.display = True
        bar.focus()

    def _set_sort(self, col: str) -> None:
        super()._set_sort(col)
        config.update({"view_state": {"jobs_sort_col": self._sort_col or "", "jobs_sort_reversed": self._sort_reversed}})
        self._update_table(self._last_jobs_raw)

    def action_sort_state(self) -> None:
        self._set_sort("state")

    def action_sort_time(self) -> None:
        self._set_sort("time")

    def action_sort_cpus(self) -> None:
        self._set_sort("cpus")

    def _persist_column_order(self) -> None:
        config.update({"columns": {"jobs_order": list(self._column_order)}})

    def action_cycle_reorder_target(self) -> None:
        """Advance the reorder-target column one step to the right (wraps)."""
        n = len(self._current_cols)
        if n == 0:
            return
        self._reorder_target_idx = (self._reorder_target_idx + 1) % n
        self._rebuild_columns(self.size.width, self._last_jobs, force=True)
        self._render_rows(self._last_jobs)

    def _shift_visible_column(self, direction: int) -> None:
        """Shift the reorder-target column left (-1) or right (+1) in _column_order.

        Operates in visible-column space for the target, then translates to the
        absolute _column_order index for the swap.
        """
        table = self.query_one(CyclicDataTable)
        visible_names = [name for name, _ in self._current_cols]
        if not visible_names:
            return
        vis_idx = self._reorder_target_idx % len(visible_names)
        name = visible_names[vis_idx]

        abs_idx = self._column_order.index(name)
        target_idx = abs_idx + direction

        # Walk past hidden columns to find the real neighbour in _column_order.
        # We only want to swap with another visible column.
        visible_set = set(visible_names)
        if direction < 0:
            # Find the nearest lower abs_idx that is visible.
            candidate = abs_idx - 1
            while candidate >= 0 and self._column_order[candidate] not in visible_set:
                candidate -= 1
            if candidate < 0:
                return  # already at the leftmost visible column
            target_idx = candidate
        else:
            # Find the nearest higher abs_idx that is visible.
            candidate = abs_idx + 1
            while candidate < len(self._column_order) and self._column_order[candidate] not in visible_set:
                candidate += 1
            if candidate >= len(self._column_order):
                return  # already at the rightmost visible column
            target_idx = candidate

        # Perform the swap in _column_order.
        self._column_order[abs_idx], self._column_order[target_idx] = (
            self._column_order[target_idx],
            self._column_order[abs_idx],
        )
        self._persist_column_order()

        # Track the moved column: update target so highlight follows it.
        self._reorder_target_idx = max(0, min(vis_idx + direction, len(visible_names) - 1))

        # Rebuild columns and re-render.
        state = self._capture_table_state()
        self._rebuild_columns(self.size.width, self._last_jobs, force=True)
        self._render_rows(self._last_jobs)
        # Restore data-row cursor (column index is row-mode irrelevant).
        if state[0] >= 0:
            table.move_cursor(row=state[0])

    def action_shift_column_left(self) -> None:
        self._shift_visible_column(-1)

    def action_shift_column_right(self) -> None:
        self._shift_visible_column(1)

    def on_cyclic_data_table_column_reordered(self, event) -> None:
        """Handle mouse drag column reorder from CyclicDataTable.ColumnReordered."""
        from_vis = event.from_index
        to_vis = event.to_index
        visible_names = [name for name, _ in self._current_cols]
        if not visible_names:
            return

        from_vis = max(0, min(from_vis, len(visible_names) - 1))
        # Clamp to_vis; if >= len(visible_names), append at end.
        to_vis = max(0, min(to_vis, len(visible_names)))

        if from_vis >= len(visible_names):
            return

        moved_name = visible_names[from_vis]

        # Remove from _column_order.
        self._column_order.remove(moved_name)

        if to_vis >= len(visible_names):
            # Append at the end of _column_order.
            self._column_order.append(moved_name)
        else:
            # Determine the absolute index corresponding to to_vis in the
            # updated visible_names (after removal).
            updated_visible = [n for n in visible_names if n != moved_name]
            if to_vis >= len(updated_visible):
                self._column_order.append(moved_name)
            else:
                anchor_name = updated_visible[to_vis]
                anchor_abs = self._column_order.index(anchor_name)
                self._column_order.insert(anchor_abs, moved_name)

        self._persist_column_order()
        self._rebuild_columns(self.size.width, self._last_jobs, force=True)
        # Move the reorder target onto the dropped column so the header
        # highlight follows it. Requires another rebuild because the header
        # label is fixed at add_column time.
        new_visible = [n for n, _ in self._current_cols]
        try:
            new_target = new_visible.index(moved_name)
            if new_target != self._reorder_target_idx:
                self._reorder_target_idx = new_target
                self._rebuild_columns(self.size.width, self._last_jobs, force=True)
        except ValueError:
            pass
        self._render_rows(self._last_jobs)

    def action_yank(self) -> None:
        """Dispatch: visual yank when in visual mode, otherwise yank job id."""
        if self._visual_active:
            self.action_visual_yank()
        else:
            self._do_yank_job_id()

    def _do_yank_job_id(self) -> None:
        table = self.query_one(CyclicDataTable)
        row_idx = table.cursor_row
        if row_idx >= len(self._last_jobs):
            return
        job = self._last_jobs[row_idx]
        from ..clipboard import app_copy
        app_copy(self.app, job.job_id, label=f"Job {job.job_id}", count=1)

    def action_yank_row(self) -> None:
        row_idx = self.query_one(CyclicDataTable).cursor_row
        if row_idx >= len(self._last_jobs):
            return
        job = self._last_jobs[row_idx]
        tsv = "\t".join(self._plain_cell(job, name) for name, _ in self._current_cols)
        from ..clipboard import app_copy
        app_copy(self.app, tsv, label=f"Row job {job.job_id}", count=1)

    def action_escape_or_visual_exit(self) -> None:
        """Exit visual mode if active; otherwise fall through to search dismiss."""
        if self._visual_active:
            self.action_visual_exit()
        else:
            bar = self.query_one("#search-bar", Input)
            if bar.display:
                self._dismiss_search()

    def action_view_dependencies(self) -> None:
        if job := self._job_for_cursor():
            from .dependency import JobDependencyScreen
            self.app.push_screen(JobDependencyScreen(job))

    def action_job_info(self) -> None:
        if job := self._job_for_cursor():
            self.app.push_screen(JobInfoScreen(job))

    def action_expand_array(self) -> None:
        if job := self._job_for_cursor():
            self.app.push_screen(ArrayTaskScreen(job))

    @work(thread=True)
    def action_view_log(self) -> None:
        job = self._job_for_cursor()
        if not job:
            return
        stdout_path, _ = fetch_log_paths(job.job_id)
        if not stdout_path:
            self.app.call_from_thread(
                self.app.notify, "No log path found", severity="warning"
            )
            return
        self.app.call_from_thread(
            self.app.push_screen, LogViewerScreen(job.job_id, stdout_path, LOG_STDOUT)
        )

    @work(thread=True)
    def action_show_detail(self) -> None:
        job = self._job_for_cursor()
        if not job:
            return
        data = fetch_job_detail(job.job_id)
        self.app.call_from_thread(
            self.app.push_screen, JobDetailScreen(job.job_id, data)
        )

    def _reload_column_visibility(self) -> None:
        cfg = config.load()
        self._hidden_cols = set(cfg.get("columns", {}).get("jobs_hidden", []))
        saved_order = list(cfg.get("columns", {}).get("jobs_order", []))
        default_order = [c.name for c in COLUMNS]
        self._column_order = _reconcile_order(saved_order, default_order)
        self._rebuild_columns(self.size.width, self._last_jobs, force=True)
        self._render_rows(self._last_jobs)

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

    def _expert_mode_enabled(self) -> bool:
        return bool(getattr(self.app, "expert_mode", self._expert_mode))

    def _confirm_single_cancel_enabled(self) -> bool:
        return bool(getattr(self.app, "confirm_cancel_single", self._confirm_cancel_single))

    def _confirm_bulk_actions_enabled(self) -> bool:
        return bool(getattr(self.app, "confirm_bulk_actions", self._confirm_bulk_actions))

    def _job_for_cursor(self) -> Job | None:
        table = self.query_one(CyclicDataTable)
        row_idx = table.cursor_row
        if row_idx >= len(self._last_jobs):
            return None
        return self._last_jobs[row_idx]

    def action_toggle_select(self) -> None:
        job = self._job_for_cursor()
        if job is None:
            return
        if job.job_id in self._selected_job_ids:
            self._selected_job_ids.remove(job.job_id)
        else:
            self._selected_job_ids.add(job.job_id)
        self._render_rows(self._last_jobs)
        self._update_header(self._last_jobs_raw)

    def action_select_all_visible(self) -> None:
        self._selected_job_ids.update(j.job_id for j in self._last_jobs)
        self._render_rows(self._last_jobs)
        self._update_header(self._last_jobs_raw)

    def action_clear_selection(self) -> None:
        self._selected_job_ids.clear()
        self._render_rows(self._last_jobs)
        self._update_header(self._last_jobs_raw)

    def _selected_or_current_job_ids(self) -> list[str]:
        if self._selected_job_ids:
            visible = {j.job_id for j in self._last_jobs}
            return [job_id for job_id in self._selected_job_ids if job_id in visible]
        job = self._job_for_cursor()
        return [job.job_id] if job else []

    def _run_action_results(self, action: str, results: list[ActionResult]) -> None:
        ok_count = sum(1 for r in results if r.ok)
        fail_count = len(results) - ok_count
        if fail_count == 0:
            self.app.notify(f"{action}: {ok_count} succeeded", title="Bulk action")
            return
        first_error = next((r.message for r in results if not r.ok and r.message), "failed")
        self.app.notify(
            f"{action}: {ok_count} ok, {fail_count} failed ({first_error})",
            title="Bulk action",
            severity="warning",
            timeout=8,
        )

    def _run_bulk_action(self, action: str, job_ids: list[str]) -> None:
        if not job_ids:
            self.app.notify("No jobs selected", severity="warning")
            return

        def execute() -> None:
            results = run_bulk_job_action(action, job_ids)
            self._run_action_results(action, results)
            self.refresh_data()

        expert_mode = self._expert_mode_enabled()
        need_confirm = (
            (not expert_mode and self._confirm_bulk_actions_enabled())
            or (action == "cancel" and not expert_mode)
        )
        if need_confirm:
            self.app.push_screen(
                ConfirmScreen(f"{action.title()} {len(job_ids)} job(s)?"),
                lambda confirmed: execute() if confirmed else None,
            )
        else:
            execute()

    def action_bulk_actions(self) -> None:
        selected_ids = self._selected_or_current_job_ids()
        if not selected_ids:
            self.app.notify("No jobs selected", severity="warning")
            return

        def handle(action: str | None) -> None:
            if action is None:
                return
            self._run_bulk_action(action, selected_ids)

        self.app.push_screen(BulkActionScreen(len(selected_ids)), handle)

    def action_hold_jobs(self) -> None:
        self._run_bulk_action("hold", self._selected_or_current_job_ids())

    def action_release_jobs(self) -> None:
        self._run_bulk_action("release", self._selected_or_current_job_ids())

    def action_requeue_jobs(self) -> None:
        self._run_bulk_action("requeue", self._selected_or_current_job_ids())

    # ── Input / key events ────────────────────────────────────────────────────

    def on_input_changed(self, event: Input.Changed) -> None:
        if event.input.id == "search-bar":
            self._search_query = event.value
            self._update_table(self._last_jobs_raw)

    def on_input_submitted(self, event: Input.Submitted) -> None:
        if event.input.id == "search-bar":
            self._dismiss_search()

    def on_key(self, event) -> None:
        # escape is handled via action_escape_or_visual_exit binding
        pass

    def _dismiss_search(self) -> None:
        bar = self.query_one("#search-bar", Input)
        bar.display = False
        bar.value = ""
        self._search_query = ""
        self._update_table(self._last_jobs_raw)
        self.query_one(CyclicDataTable).focus()

    def _resolve_attach_command(self) -> str:
        command = self._attach_default_command.strip() or "$SHELL -l"
        if command == "$SHELL -l":
            shell = os.getenv("SHELL", "").strip()
            shell_name = os.path.basename(shell) if shell else ""
            command = f"{shell_name} -l" if shell_name else "bash -l"
        try:
            parts = shlex.split(command)
        except ValueError:
            return command
        if not parts:
            return "bash -l"
        if parts[0].startswith("/"):
            parts[0] = os.path.basename(parts[0])
        return " ".join(shlex.quote(p) for p in parts)

    def _run_attach(self, job: Job, node_override: str | None = None) -> None:
        if not self._attach_enabled:
            self.app.notify("Attach disabled in config [attach].enabled", severity="warning")
            return
        if job.state not in _ATTACH_STATES:
            self.app.notify("Attach is only available for RUNNING jobs.", severity="warning")
            return

        detail = fetch_job_detail(job.job_id)
        attach_job_id = detail.get("JobId", "").strip() or job.job_id

        node = (node_override or "").strip()
        if not node:
            node = resolve_first_node(job.nodelist)
            if not node:
                node = resolve_first_node(detail.get("NodeList", ""))

        try:
            resolved_command = self._resolve_attach_command()
            command = build_attach_command(
                job_id=attach_job_id,
                node=node or None,
                default_command=resolved_command,
                extra_args=self._attach_extra_args,
            )
        except ValueError as exc:
            self.app.notify(f"Attach command parse error: {exc}", severity="error")
            return

        self.app.notify(
            "Launching attach session. Exit shell to return to sqtop.",
            title="Attach",
            timeout=4,
        )
        retried_with_bash = False
        with self.app.suspend():
            rc = run_attach_command(command)
            if rc != 0 and resolved_command != "bash -l":
                fallback = build_attach_command(
                    job_id=attach_job_id,
                    node=node or None,
                    default_command="bash -l",
                    extra_args=self._attach_extra_args,
                )
                rc = run_attach_command(fallback)
                retried_with_bash = True

        if rc == 0:
            message = "Attach session ended"
            if retried_with_bash:
                message += " (fallback shell: bash)"
            self.app.notify(message, title="Attach")
        else:
            self.app.notify(f"Attach exited with code {rc}", title="Attach", severity="warning")
        self.refresh_data()

    # ── Data pipeline ────────────────────────────────────────────────────────

    def _update_table(self, jobs: list[Job]) -> None:
        state = self._capture_table_state()
        self._last_jobs_raw = jobs
        valid_ids = {j.job_id for j in jobs}
        self._selected_job_ids.intersection_update(valid_ids)
        self._check_watched_jobs(jobs)

        filtered = jobs
        if self._filter_mine:
            user = os.getenv("USER", "")
            filtered = [j for j in filtered if j.user == user]
        if self._filter_state:
            _FILTER_TERMINAL_STATES = {"FAILED", "CANCELLED", "TIMEOUT", "NODE_FAIL", "PREEMPTED", "OUT_OF_MEMORY"}
            if self._filter_state == "FAILED":
                filtered = [j for j in filtered if j.state in _FILTER_TERMINAL_STATES]
            else:
                filtered = [j for j in filtered if j.state == self._filter_state]
        if self._search_query:
            q = self._search_query.lower()
            filtered = [
                j for j in filtered
                if q in j.name.lower() or q in j.state.lower() or q in j.partition.lower() or q in j.job_id
            ]

        if self._sort_col is None:
            self._last_jobs = sorted(filtered, key=_job_sort_key)
        else:
            key_fn = _SORT_KEYS[self._sort_col]
            self._last_jobs = sorted(filtered, key=key_fn, reverse=self._sort_reversed)
        self._last_jobs_index = {j.job_id: i for i, j in enumerate(self._last_jobs)}

        new_fp = (
            tuple((j.job_id, j.state) for j in self._last_jobs),
            frozenset(self._watched_states),
            frozenset(self._selected_job_ids),
        )
        if new_fp == self._last_render_fp:
            self._fp_skip_count += 1
            if self._fp_skip_count < 5:
                self._update_header(jobs)
                return
            self._fp_skip_count = 0
        else:
            self._fp_skip_count = 0
            self._last_render_fp = new_fp

        self._rebuild_columns(self.size.width, self._last_jobs)
        self._render_rows(self._last_jobs)
        self._restore_table_state(state, self._last_jobs)
        self._update_header(jobs)

    def _update_header(self, all_jobs: list[Job]) -> None:
        total = len(all_jobs)
        tier = getattr(getattr(self, "app", None), "tier", "sm")

        if tier == "xs":
            # xs: compact — just total count
            self.query_one("#jobs-header", Label).update(
                f"[b]squeue[/b]  [dim]{total} total[/]"
            )
            return

        now = datetime.now().strftime("%H:%M:%S")
        running = sum(1 for j in all_jobs if j.state == "RUNNING")
        pending = sum(1 for j in all_jobs if j.state == "PENDING")
        filtered = len(self._last_jobs)
        count_str = f"{filtered}/{total} jobs" if filtered != total else f"{total} total"

        tags: list[str] = []
        if self._filter_mine:
            tags.append("[cyan]· mine[/]")
        if self._filter_state:
            tags.append(f"[cyan]· {self._filter_state}[/]")
        if self._search_query:
            tags.append(f'[yellow]· "{self._search_query}"[/]')
        if self._sort_col is not None:
            arrow = "↑" if self._sort_reversed else "↓"
            tags.append(f"[dim]sort:{self._sort_col}{arrow}[/]")
        if self._watched_states:
            tags.append(f"[magenta]· {len(self._watched_states)} watched[/]")
        if self._selected_job_ids:
            tags.append(f"[blue]· {len(self._selected_job_ids)} selected[/]")
        if self._expert_mode_enabled():
            tags.append("[red]· expert[/]")
        if all_jobs and (pending / len(all_jobs)) > self._warn_pending_ratio:
            tags.append(f"[red bold]! {pending}/{len(all_jobs)} pending[/]")

        suffix = ("  " + "  ".join(tags)) if tags else ""
        self.query_one("#jobs-header", Label).update(
            f"[b]squeue[/b]  [green]{running} running[/]  "
            f"[yellow]{pending} pending[/]  "
            f"[dim]{count_str}  updated {now}[/]"
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
                if self._desktop_notify_enabled:
                    from .notify import desktop_notify
                    desktop_notify("sqtop: Job finished", f"Job {job_id} → {state_str}")
                finished.append(job_id)
            elif cur != last_state:
                self._watched_states[job_id] = cur
        for job_id in finished:
            del self._watched_states[job_id]

    def _render_rows(self, jobs: list[Job]) -> None:
        table = self.query_one(CyclicDataTable)
        saved_row = table.cursor_row
        visual_set = self.visual_rows()
        col_widths = dict(self._current_cols)
        table.clear()
        for idx, job in enumerate(jobs):
            color = STATE_COLORS.get(job.state, "white")
            watched_prefix = "★ " if job.job_id in self._watched_states else ""
            selected_prefix = "✓ " if job.job_id in self._selected_job_ids else ""
            visual_prefix = "» " if idx in visual_set else ""
            row = []
            for name, w in self._current_cols:
                cell = self._cell_text(job, name, w)
                if name == "JOBID":
                    row.append(
                        f"[{color}]{selected_prefix}{visual_prefix}{watched_prefix}{cell}[/]"
                    )
                elif name == "NAME":
                    row.append(f"[{color}]{cell}[/]")
                elif name == "STATE":
                    row.append(f"[{color}]{cell}[/]")
                elif name == "TIME_LEFT":
                    tl_display, tl_color = _time_left(job)
                    row.append(f"[{tl_color}]{truncate_cell(tl_display, w)}[/]")
                else:
                    row.append(cell)
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
            if action == "dependencies":
                from .dependency import JobDependencyScreen
                self.app.push_screen(JobDependencyScreen(job))
            elif action == "attach_first":
                self._run_attach(job)
            elif action == "attach_custom":
                default_node = resolve_first_node(job.nodelist)

                def do_attach(node_value: str | None) -> None:
                    if node_value is None:
                        return
                    self._run_attach(job, node_value)

                self.app.push_screen(AttachNodePromptScreen(default_node), do_attach)
            elif action == "detail":
                data = fetch_job_detail(job.job_id)
                self.app.push_screen(JobDetailScreen(job.job_id, data))
            elif action == "batch_script":
                self.app.push_screen(BatchScriptScreen(job.job_id))
            elif action == "cancel":
                def execute_cancel() -> None:
                    result = run_job_action("cancel", job.job_id)
                    if result.ok:
                        self.app.notify(f"Cancelled {job.job_id}", title="Job action")
                    else:
                        self.app.notify(
                            f"Cancel failed: {result.message}",
                            title="Job action",
                            severity="warning",
                        )
                    self.refresh_data()

                need_confirm = (not self._expert_mode_enabled()) and self._confirm_single_cancel_enabled()
                if need_confirm:
                    self.app.push_screen(
                        ConfirmScreen(f"Cancel job {job.job_id} ({job.name})?"),
                        lambda confirmed: execute_cancel() if confirmed else None,
                    )
                else:
                    execute_cancel()
            else:
                stdout_path, stderr_path = fetch_log_paths(job.job_id)
                log_path = stdout_path if action == LOG_STDOUT else stderr_path
                self.app.push_screen(LogViewerScreen(job.job_id, log_path, action))

        self.app.push_screen(JobActionScreen(job), handle_action)

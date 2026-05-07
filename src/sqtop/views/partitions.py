"""Partitions view — sinfo summary table with per-partition availability."""

from __future__ import annotations

from datetime import datetime

from textual.app import ComposeResult
from textual.binding import Binding
from textual.widgets import DataTable, Label

from .base import BaseDataTableView
from ..slurm import ClusterSummary, fetch_cluster_summary
from .widgets import CyclicDataTable
from .. import config
from ..responsive import (
    ColumnSpec,
    CHROME_OVERHEAD,
    allocate_columns,
    tier_for,
    truncate_cell,
    WidthChanged,
)

AVAIL_COLORS = {
    "up":   "green",
    "down": "red",
    "inact": "dim",
    "drain": "yellow",
}

STATE_COLORS = {
    "idle":      "green",
    "allocated": "cyan",
    "mixed":     "yellow",
    "down":      "red",
    "drain":     "red",
    "draining":  "magenta",
    "unknown":   "dim",
}

# ColumnSpec(name, min_width, content_max, priority, min_tier)
COLUMNS: list[ColumnSpec] = [
    ColumnSpec("PARTITION",  14, 20, 100, "xs"),
    ColumnSpec("AVAIL",       7,  8,  90, "xs"),
    ColumnSpec("STATE",      12, 16,  85, "xs"),
    ColumnSpec("TIMELIMIT",  12, 16,  70, "sm"),
    ColumnSpec("NODES",       7,  8,  65, "sm"),
    ColumnSpec("NODELIST",   30, 40,  30, "md"),
]


class PartitionsView(BaseDataTableView[ClusterSummary]):
    """Displays a live sinfo-style partition summary table."""

    BINDINGS = [
        Binding("s", "sort_partition", show=False),
        Binding("n", "sort_nodes", show=False),
        Binding("v", "visual_enter", "Visual", show=False),
        Binding("V", "visual_enter", "Visual", show=False),
        Binding("escape", "visual_exit", "Exit visual", show=False),
        Binding("y", "yank", "Copy", show=False),
    ]

    def __init__(self, interval: float = 5.0, start_offset: float = 0.0) -> None:
        super().__init__(interval=interval, start_offset=start_offset)
        self._last_summaries: list[ClusterSummary] = []
        self._last_sorted_rows: list[ClusterSummary] = []
        self._last_render_fp: tuple = ()
        self._current_cols: list[tuple[str, int]] = []
        self._rebuild_cache_width: int = -1
        self._rebuild_cache_names: list[str] = []
        cfg_all = config.load()
        view_state = cfg_all.get("view_state", {})
        saved_sort = str(view_state.get("partitions_sort_col", ""))
        if saved_sort in {"partition", "nodes"}:
            self._sort_col = saved_sort
            self._sort_reversed = bool(view_state.get("partitions_sort_reversed", False))
        self._hidden_cols: set[str] = set(cfg_all.get("columns", {}).get("partitions_hidden", []))

    def compose(self) -> ComposeResult:
        yield Label("", id="partitions-header")
        yield CyclicDataTable(id="partitions-table", cursor_type="row", zebra_stripes=True)

    def _visible_cols_filtered(self, width: int | None = None) -> list[tuple[str, int]]:
        """Return budget-allocated columns. width defaults to cached or 80."""
        w = width if width is not None else (self._rebuild_cache_width if self._rebuild_cache_width > 0 else 80)
        budget = max(0, w - CHROME_OVERHEAD)
        cols = [col for col in COLUMNS if col.name not in self._hidden_cols]
        return allocate_columns(budget, cols, current_tier=tier_for(w))

    def _rebuild_columns(self, width: int | None = None, *, force: bool = False) -> bool:
        """Rebuild using budget allocation. Returns True if layout changed."""
        w = width if width is not None else (self._rebuild_cache_width if self._rebuild_cache_width > 0 else 80)
        new_cols = self._visible_cols_filtered(w)
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
        changed = self._rebuild_columns(event.width)
        if changed and self._last_sorted_rows:
            self._render_rows(self._last_sorted_rows)
            self._restore_table_state(state, self._last_sorted_rows)

    def _reload_column_visibility(self) -> None:
        cfg = config.load()
        self._hidden_cols = set(cfg.get("columns", {}).get("partitions_hidden", []))
        self._rebuild_columns(self._rebuild_cache_width if self._rebuild_cache_width > 0 else None, force=True)
        self._render_rows(self._last_sorted_rows)

    def on_mount(self) -> None:
        self._rebuild_columns(force=True)
        self.start_refresh_loop()

    def _fetch_data(self) -> list[ClusterSummary]:
        return fetch_cluster_summary()

    def _get_anchor_key(self, item: ClusterSummary) -> str:
        return item.partition

    def _set_sort(self, col: str) -> None:
        super()._set_sort(col)
        config.update({"view_state": {"partitions_sort_col": self._sort_col or "", "partitions_sort_reversed": self._sort_reversed}})
        self._last_sorted_rows = self._sorted_rows(self._last_summaries)
        self._render_rows(self._last_sorted_rows)

    def action_sort_partition(self) -> None:
        self._set_sort("partition")

    def action_sort_nodes(self) -> None:
        self._set_sort("nodes")

    def action_yank(self) -> None:
        """Visual yank when in visual mode; no-op otherwise."""
        if self._visual_active:
            self.action_visual_yank()

    def _update_table(self, summaries: list[ClusterSummary]) -> None:
        self._last_summaries = summaries
        self._last_sorted_rows = self._sorted_rows(summaries)

        now = datetime.now().strftime("%H:%M:%S")
        up = sum(1 for s in summaries if s.avail.lower() == "up")
        self.query_one("#partitions-header", Label).update(
            f"[b]sinfo[/b]  [green]{up} up[/]  "
            f"[dim]{len(summaries)} partitions  updated {now}[/]"
        )

        new_fp = tuple((s.partition, s.state, s.nodes) for s in self._last_sorted_rows)
        if new_fp == self._last_render_fp:
            return
        self._last_render_fp = new_fp

        state = self._capture_table_state()
        self._render_rows(self._last_sorted_rows)
        self._restore_table_state(state, self._last_sorted_rows)

    def _sorted_rows(self, summaries: list[ClusterSummary]) -> list[ClusterSummary]:
        rows = list(summaries)
        if self._sort_col == "partition":
            rows = sorted(rows, key=lambda s: s.partition, reverse=self._sort_reversed)
        elif self._sort_col == "nodes":
            rows = sorted(
                rows,
                key=lambda s: int(s.nodes) if s.nodes.isdigit() else 0,
                reverse=self._sort_reversed,
            )
        return rows

    def _capture_table_state(self) -> tuple[int, float, str | None]:
        table = self.query_one(CyclicDataTable)
        row = table.cursor_row
        scroll_y = float(table.scroll_offset.y)
        anchor: str | None = None
        if 0 <= row < len(self._last_sorted_rows):
            anchor = self._last_sorted_rows[row].partition
        return row, scroll_y, anchor

    def _restore_table_state(
        self, state: tuple[int, float, str | None], rows: list[ClusterSummary]
    ) -> None:
        if not rows:
            return
        saved_row, scroll_y, anchor = state
        table = self.query_one(CyclicDataTable)
        row = None
        if anchor:
            for i, summary in enumerate(rows):
                if summary.partition == anchor:
                    row = i
                    break
        if row is None:
            row = min(saved_row, len(rows) - 1)
        table.move_cursor(row=row)
        table.scroll_to(y=scroll_y, animate=False)

    def _cell_for_col(self, s: ClusterSummary, name: str, width: int | None = None) -> str:
        avail_color = AVAIL_COLORS.get(s.avail.lower(), "white")
        state_lower = s.state.lower().split("*")[0].rstrip("-")
        state_color = STATE_COLORS.get(state_lower, "white")
        plain = self._plain_cell(s, name)
        text = truncate_cell(plain, width) if width is not None else plain
        if name == "PARTITION":
            return f"[bold]{text}[/bold]"
        if name == "AVAIL":
            return f"[{avail_color}]{text}[/]"
        if name == "TIMELIMIT":
            return text
        if name == "NODES":
            return text
        if name == "STATE":
            return f"[{state_color}]{text}[/]"
        return text

    def _plain_cell(self, s: ClusterSummary, name: str) -> str:
        """Return plain (markup-free) cell text for a partition column."""
        if name == "PARTITION":
            return s.partition
        if name == "AVAIL":
            return s.avail
        if name == "TIMELIMIT":
            return s.timelimit
        if name == "NODES":
            return s.nodes
        if name == "STATE":
            return s.state
        return s.nodelist

    # ── Copy-pane interface ───────────────────────────────────────────────────

    def _pane_label(self) -> str:
        return "Partitions"

    def _current_items(self) -> list[ClusterSummary]:
        return list(self._last_sorted_rows)

    def _row_tsv(self, item: ClusterSummary) -> str:
        return "\t".join(self._plain_cell(item, name) for name, _ in self._current_cols)

    def copy_pane(self) -> tuple[str, str, int]:
        """Return (label, tsv_payload, row_count) for the partitions pane."""
        header = "\t".join(name for name, _ in self._current_cols)
        items = self._current_items()
        rows = [self._row_tsv(item) for item in items]
        payload = "\n".join([header, *rows]) + "\n"
        return self._pane_label(), payload, len(rows)

    def _render_rows(self, sorted_rows: list[ClusterSummary]) -> None:
        table = self.query_one(CyclicDataTable)
        visual_set = self.visual_rows()
        table.clear()
        for idx, s in enumerate(sorted_rows):
            visual_prefix = "» " if idx in visual_set else ""
            row = []
            for name, w in self._current_cols:
                cell = self._cell_for_col(s, name, w)
                if name == "PARTITION":
                    plain = truncate_cell(s.partition, w)
                    cell = f"[bold]{visual_prefix}{plain}[/bold]"
                row.append(cell)
            table.add_row(*row)
        if sorted_rows and table.cursor_row < 0:
            table.move_cursor(row=0)

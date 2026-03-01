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

COLUMNS: list[tuple[str, int]] = [
    ("PARTITION",  14),
    ("AVAIL",       7),
    ("TIMELIMIT",  12),
    ("NODES",       7),
    ("STATE",      12),
    ("NODELIST",   30),
]


class PartitionsView(BaseDataTableView[ClusterSummary]):
    """Displays a live sinfo-style partition summary table."""

    BINDINGS = [
        Binding("s", "sort_partition", show=False),
        Binding("n", "sort_nodes", show=False),
    ]

    def __init__(self, interval: float = 5.0, start_offset: float = 0.0) -> None:
        super().__init__(interval=interval, start_offset=start_offset)
        self._last_summaries: list[ClusterSummary] = []
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

    def _visible_cols_filtered(self) -> list[tuple[str, int]]:
        return [(name, w) for name, w in COLUMNS if name not in self._hidden_cols]

    def _rebuild_columns(self) -> None:
        table = self.query_one(CyclicDataTable)
        table.clear(columns=True)
        for name, width in self._visible_cols_filtered():
            table.add_column(name, width=width)

    def _reload_column_visibility(self) -> None:
        cfg = config.load()
        self._hidden_cols = set(cfg.get("columns", {}).get("partitions_hidden", []))
        self._rebuild_columns()
        self._render_rows(self._last_summaries)

    def on_mount(self) -> None:
        self._rebuild_columns()
        self.refresh_data()
        if self._start_offset > 0:
            self.set_timer(self._start_offset, self._begin_interval)
        else:
            self._begin_interval()

    def _fetch_data(self) -> list[ClusterSummary]:
        return fetch_cluster_summary()

    def _get_anchor_key(self, item: ClusterSummary) -> str:
        return item.partition

    def _set_sort(self, col: str) -> None:
        super()._set_sort(col)
        config.update({"view_state": {"partitions_sort_col": self._sort_col or "", "partitions_sort_reversed": self._sort_reversed}})
        self._render_rows(self._last_summaries)

    def action_sort_partition(self) -> None:
        self._set_sort("partition")

    def action_sort_nodes(self) -> None:
        self._set_sort("nodes")

    def _update_table(self, summaries: list[ClusterSummary]) -> None:
        state = self._capture_table_state()
        self._last_summaries = summaries
        self._render_rows(summaries)
        self._restore_table_state(state, self._sorted_rows(summaries))
        now = datetime.now().strftime("%H:%M:%S")
        up = sum(1 for s in summaries if s.avail.lower() == "up")
        self.query_one("#partitions-header", Label).update(
            f"[b]sinfo[/b]  [green]{up} up[/]  "
            f"[dim]{len(summaries)} partitions  updated {now}[/]"
        )

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
        rows = self._sorted_rows(self._last_summaries)
        if 0 <= row < len(rows):
            anchor = rows[row].partition
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

    def _cell_for_col(self, s: ClusterSummary, name: str) -> str:
        avail_color = AVAIL_COLORS.get(s.avail.lower(), "white")
        state_lower = s.state.lower().split("*")[0].rstrip("-")
        state_color = STATE_COLORS.get(state_lower, "white")
        if name == "PARTITION":
            return f"[bold]{s.partition}[/bold]"
        if name == "AVAIL":
            return f"[{avail_color}]{s.avail}[/]"
        if name == "TIMELIMIT":
            return s.timelimit
        if name == "NODES":
            return s.nodes
        if name == "STATE":
            return f"[{state_color}]{s.state}[/]"
        return s.nodelist

    def _render_rows(self, summaries: list[ClusterSummary]) -> None:
        rows = self._sorted_rows(summaries)
        visible = self._visible_cols_filtered()

        table = self.query_one(CyclicDataTable)
        table.clear()
        for s in rows:
            table.add_row(*[self._cell_for_col(s, name) for name, _ in visible])
        if rows and table.cursor_row < 0:
            table.move_cursor(row=0)

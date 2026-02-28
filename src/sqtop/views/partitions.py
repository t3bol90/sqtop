"""Partitions view — sinfo summary table with per-partition availability."""

from __future__ import annotations

from datetime import datetime

from textual.app import ComposeResult
from textual.binding import Binding
from textual.widgets import DataTable, Label, Static
from textual import work

from ..slurm import ClusterSummary, fetch_cluster_summary
from .widgets import CyclicDataTable

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


class PartitionsView(Static):
    """Displays a live sinfo-style partition summary table."""

    BINDINGS = [
        Binding("s", "sort_partition", show=False),
        Binding("n", "sort_nodes", show=False),
    ]

    def __init__(self, interval: float = 5.0) -> None:
        super().__init__()
        self._interval = interval
        self._last_summaries: list[ClusterSummary] = []
        self._fetching = False
        self._timer = None
        self._sort_col: str | None = None
        self._sort_reversed: bool = False

    def compose(self) -> ComposeResult:
        yield Label("", id="partitions-header")
        yield CyclicDataTable(id="partitions-table", cursor_type="row", zebra_stripes=True)

    def on_mount(self) -> None:
        table = self.query_one(CyclicDataTable)
        for name, width in COLUMNS:
            table.add_column(name, width=width)
        self.refresh_data()
        self._timer = self.set_interval(self._interval, self.refresh_data)

    def set_interval_rate(self, interval: float) -> None:
        self._interval = interval
        if self._timer:
            self._timer.stop()
        self._timer = self.set_interval(self._interval, self.refresh_data)

    @work(thread=True)
    def refresh_data(self) -> None:
        if self._fetching:
            return
        self._fetching = True
        try:
            summaries = fetch_cluster_summary()
            self.app.call_from_thread(self._update_table, summaries)
        finally:
            self._fetching = False

    def _set_sort(self, col: str) -> None:
        if self._sort_col == col:
            self._sort_reversed = not self._sort_reversed
        else:
            self._sort_col = col
            self._sort_reversed = False
        self._render_rows(self._last_summaries)

    def action_sort_partition(self) -> None:
        self._set_sort("partition")

    def action_sort_nodes(self) -> None:
        self._set_sort("nodes")

    def _update_table(self, summaries: list[ClusterSummary]) -> None:
        self._last_summaries = summaries
        self._render_rows(summaries)
        now = datetime.now().strftime("%H:%M:%S")
        up = sum(1 for s in summaries if s.avail.lower() == "up")
        self.query_one("#partitions-header", Label).update(
            f"[b]sinfo[/b]  [green]{up} up[/]  "
            f"[dim]{len(summaries)} partitions  updated {now}[/]"
        )

    def _render_rows(self, summaries: list[ClusterSummary]) -> None:
        rows = list(summaries)
        if self._sort_col == "partition":
            rows = sorted(rows, key=lambda s: s.partition, reverse=self._sort_reversed)
        elif self._sort_col == "nodes":
            rows = sorted(
                rows,
                key=lambda s: int(s.nodes) if s.nodes.isdigit() else 0,
                reverse=self._sort_reversed,
            )

        table = self.query_one(CyclicDataTable)
        saved_row = table.cursor_row
        table.clear()
        for s in rows:
            avail_color = AVAIL_COLORS.get(s.avail.lower(), "white")
            state_lower = s.state.lower().split("*")[0].rstrip("-")
            state_color = STATE_COLORS.get(state_lower, "white")
            table.add_row(
                f"[bold]{s.partition}[/bold]",
                f"[{avail_color}]{s.avail}[/]",
                s.timelimit,
                s.nodes,
                f"[{state_color}]{s.state}[/]",
                s.nodelist,
            )
        if rows:
            table.move_cursor(row=min(saved_row, len(rows) - 1))

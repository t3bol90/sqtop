"""Nodes view — sinfo-style table with utilization bars."""

from __future__ import annotations

from datetime import datetime

from textual.app import ComposeResult
from textual.binding import Binding
from textual.widgets import DataTable, Label, Static

from .widgets import CyclicDataTable
from .node_detail import NodeDetailScreen
from textual import work

from ..slurm import Node, fetch_nodes

STATE_COLORS = {
    "idle":      "green",
    "allocated": "cyan",
    "mixed":     "yellow",
    "down":      "red",
    "drain":     "red",
    "draining":  "magenta",
    "unknown":   "dim",
}

# (header, col_width, min_terminal_width_to_show)
COLUMNS: list[tuple[str, int, int]] = [
    ("NODE",       12,   0),
    ("STATE",      12,   0),
    ("CPU%",       14,   0),
    ("GPU%",       14,  60),
    ("CPUS A/T",   10,  75),
    ("GPU A/T",     9,  75),
    ("MEM FREE",   10,  90),
    ("PARTITION",  12, 105),
    ("MEM TOTAL",  10, 120),
    ("LOAD",        8, 120),
]


def _cpu_bar(alloc: str, total: str, bar_width: int = 8) -> str:
    try:
        a, t = int(alloc), int(total)
        pct = round(a / t * 100) if t else 0
        filled = round(pct / 100 * bar_width)
        bar = "█" * filled + "░" * (bar_width - filled)
        color = "green" if pct < 60 else ("yellow" if pct < 90 else "red")
        return f"[{color}]{bar}[/] {pct:3}%"
    except (ValueError, ZeroDivisionError):
        return "─" * bar_width


def _gpu_bar(alloc: int, total: int, bar_width: int = 8) -> str:
    if total == 0:
        return "[dim]—[/]"
    try:
        pct = round(alloc / total * 100)
        filled = round(pct / 100 * bar_width)
        bar = "█" * filled + "░" * (bar_width - filled)
        color = "green" if pct < 60 else ("yellow" if pct < 90 else "red")
        return f"[{color}]{bar}[/] {pct:3}%"
    except ZeroDivisionError:
        return "─" * bar_width


def _visible_cols(width: int) -> list[tuple[str, int]]:
    return [(name, w) for name, w, min_w in COLUMNS if min_w <= width]


def _cpu_pct(n: Node) -> float:
    try:
        return int(n.cpus_alloc) / int(n.cpus_total)
    except (ValueError, ZeroDivisionError):
        return 0.0


def _free_mem(n: Node) -> int:
    try:
        return int(n.memory_free)
    except ValueError:
        return 0


class NodesView(Static):
    """Displays a live sinfo-style node table."""

    BINDINGS = [
        Binding("enter", "open_node", "Open node", show=True),
        Binding("s", "sort_state", show=False),
        Binding("p", "sort_cpu", show=False),
        Binding("m", "sort_mem", show=False),
    ]

    def __init__(self, interval: float = 2.0) -> None:
        super().__init__()
        self._interval = interval
        self._last_nodes: list[Node] = []
        self._current_cols: list[tuple[str, int]] = []
        self._fetching = False
        self._timer = None
        self._sort_col: str | None = None
        self._sort_reversed: bool = False

    def compose(self) -> ComposeResult:
        yield Label("", id="nodes-header")
        yield CyclicDataTable(id="nodes-table", cursor_type="row", zebra_stripes=True)

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
            state = self._capture_table_state()
            self._rebuild_columns(event.size.width)
            self._render_rows(self._last_nodes)
            self._restore_table_state(state, self._sorted_visible(self._last_nodes))

    def _rebuild_columns(self, width: int) -> None:
        self._current_cols = _visible_cols(width)
        table = self.query_one(CyclicDataTable)
        table.clear(columns=True)
        for name, col_width in self._current_cols:
            table.add_column(name, width=col_width)

    def _capture_table_state(self) -> tuple[int, float, str | None]:
        table = self.query_one(CyclicDataTable)
        row = table.cursor_row
        scroll_y = float(table.scroll_offset.y)
        anchor: str | None = None
        rows = self._sorted_visible(self._last_nodes)
        if 0 <= row < len(rows):
            anchor = rows[row].name
        return row, scroll_y, anchor

    def _restore_table_state(self, state: tuple[int, float, str | None], rows: list[Node]) -> None:
        if not rows:
            return
        saved_row, scroll_y, anchor = state
        table = self.query_one(CyclicDataTable)
        row = None
        if anchor:
            for i, node in enumerate(rows):
                if node.name == anchor:
                    row = i
                    break
        if row is None:
            row = min(saved_row, len(rows) - 1)
        table.move_cursor(row=row)
        table.scroll_to(y=scroll_y, animate=False)

    @work(thread=True)
    def refresh_data(self) -> None:
        if self._fetching:
            return
        self._fetching = True
        try:
            nodes = fetch_nodes()
            self.app.call_from_thread(self._update_table, nodes)
        finally:
            self._fetching = False

    def _set_sort(self, col: str) -> None:
        if self._sort_col == col:
            self._sort_reversed = not self._sort_reversed
        else:
            self._sort_col = col
            self._sort_reversed = False
        self._render_rows(self._last_nodes)

    def action_sort_state(self) -> None:
        self._set_sort("state")

    def action_sort_cpu(self) -> None:
        self._set_sort("cpu")

    def action_sort_mem(self) -> None:
        self._set_sort("mem")

    def _sorted_visible(self, nodes: list[Node]) -> list[Node]:
        visible = [n for n in nodes if n.name]
        if self._sort_col == "state":
            return sorted(visible, key=lambda n: n.state, reverse=self._sort_reversed)
        elif self._sort_col == "cpu":
            return sorted(visible, key=_cpu_pct, reverse=self._sort_reversed)
        elif self._sort_col == "mem":
            return sorted(visible, key=_free_mem, reverse=self._sort_reversed)
        return visible

    def _update_table(self, nodes: list[Node]) -> None:
        state = self._capture_table_state()
        self._last_nodes = nodes
        self._render_rows(nodes)
        self._restore_table_state(state, self._sorted_visible(nodes))
        now = datetime.now().strftime("%H:%M:%S")
        visible = [n for n in nodes if n.name]
        idle  = sum(1 for n in visible if "idle"  in n.state.lower())
        alloc = sum(1 for n in visible if "alloc" in n.state.lower())
        mixed = sum(1 for n in visible if "mixed" in n.state.lower())
        down  = sum(1 for n in visible if "down"  in n.state.lower() or "drain" in n.state.lower())
        sort_tag = ""
        if self._sort_col:
            arrow = "↑" if self._sort_reversed else "↓"
            sort_tag = f"  [dim]sort:{self._sort_col}{arrow}[/]"
        self.query_one("#nodes-header", Label).update(
            f"[b]sinfo[/b]  [green]{idle} idle[/]  "
            f"[cyan]{alloc} alloc[/]  [yellow]{mixed} mixed[/]  "
            f"[red]{down} down[/]  "
            f"[dim]{len(visible)} total  updated {now}[/]"
            f"{sort_tag}"
        )

    def _render_rows(self, nodes: list[Node]) -> None:
        rows = self._sorted_visible(nodes)
        table = self.query_one(CyclicDataTable)
        table.clear()
        for node in rows:
            state_lower = node.state.lower().split("*")[0].rstrip("-")
            color = STATE_COLORS.get(state_lower, "white")
            row = []
            for name, _ in self._current_cols:
                if name == "NODE":
                    row.append(f"[bold]{node.name}[/bold]")
                elif name == "STATE":
                    row.append(f"[{color}]{node.state}[/]")
                elif name == "CPU%":
                    row.append(_cpu_bar(node.cpus_alloc, node.cpus_total))
                elif name == "GPU%":
                    row.append(_gpu_bar(node.gpu_alloc, node.gpu_total))
                elif name == "CPUS A/T":
                    row.append(f"{node.cpus_alloc}/{node.cpus_total}")
                elif name == "GPU A/T":
                    if node.gpu_total > 0:
                        free = node.gpu_total - node.gpu_alloc
                        gpu_color = "green" if free > 0 else "red"
                        row.append(f"[{gpu_color}]{node.gpu_alloc}/{node.gpu_total}[/]")
                    else:
                        row.append("[dim]—[/]")
                elif name == "MEM FREE":
                    row.append(f"{node.memory_free}M")
                elif name == "PARTITION":
                    row.append(node.partition)
                elif name == "MEM TOTAL":
                    row.append(f"{node.memory_total}M")
                elif name == "LOAD":
                    row.append(node.load)
            table.add_row(*row)
        if rows and table.cursor_row < 0:
            table.move_cursor(row=0)

    def on_data_table_row_selected(self, event: DataTable.RowSelected) -> None:
        rows = self._sorted_visible(self._last_nodes)
        row_idx = event.cursor_row
        if row_idx >= len(rows):
            return
        node = rows[row_idx]
        self.app.push_screen(NodeDetailScreen(node))

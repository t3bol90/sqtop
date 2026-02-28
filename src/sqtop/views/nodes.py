"""Nodes view — sinfo-style table with utilization bars."""

from __future__ import annotations

from datetime import datetime

from textual.app import ComposeResult
from textual.widgets import DataTable, Label, Static
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
    ("CPUS A/T",   10,  60),
    ("GPU A/T",     9,  60),
    ("MEM FREE",   10,  75),
    ("PARTITION",  12,  90),
    ("MEM TOTAL",  10, 105),
    ("LOAD",        8, 105),
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


def _visible_cols(width: int) -> list[tuple[str, int]]:
    return [(name, w) for name, w, min_w in COLUMNS if min_w <= width]


class NodesView(Static):
    """Displays a live sinfo-style node table."""

    def __init__(self, interval: float = 2.0) -> None:
        super().__init__()
        self._interval = interval
        self._last_nodes: list[Node] = []
        self._current_cols: list[tuple[str, int]] = []
        self._fetching = False
        self._timer = None

    def compose(self) -> ComposeResult:
        yield Label("", id="nodes-header")
        yield DataTable(id="nodes-table", cursor_type="row", zebra_stripes=True)

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
            self._render_rows(self._last_nodes)

    def _rebuild_columns(self, width: int) -> None:
        self._current_cols = _visible_cols(width)
        table = self.query_one(DataTable)
        table.clear(columns=True)
        for name, col_width in self._current_cols:
            table.add_column(name, width=col_width)

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

    def _update_table(self, nodes: list[Node]) -> None:
        self._last_nodes = nodes
        self._render_rows(nodes)
        now = datetime.now().strftime("%H:%M:%S")
        visible = [n for n in nodes if n.name]
        idle  = sum(1 for n in visible if "idle"  in n.state.lower())
        alloc = sum(1 for n in visible if "alloc" in n.state.lower())
        mixed = sum(1 for n in visible if "mixed" in n.state.lower())
        down  = sum(1 for n in visible if "down"  in n.state.lower() or "drain" in n.state.lower())
        self.query_one("#nodes-header", Label).update(
            f"[b]sinfo[/b]  [green]{idle} idle[/]  "
            f"[cyan]{alloc} alloc[/]  [yellow]{mixed} mixed[/]  "
            f"[red]{down} down[/]  "
            f"[dim]{len(visible)} total  updated {now}[/]"
        )

    def _render_rows(self, nodes: list[Node]) -> None:
        table = self.query_one(DataTable)
        table.clear()
        for node in nodes:
            if not node.name:
                continue
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
                elif name == "CPUS A/T":
                    row.append(f"{node.cpus_alloc}/{node.cpus_total}")
                elif name == "GPU A/T":
                    if node.gpu_total > 0:
                        free = node.gpu_total - node.gpu_alloc
                        color = "green" if free > 0 else "red"
                        row.append(f"[{color}]{node.gpu_alloc}/{node.gpu_total}[/]")
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

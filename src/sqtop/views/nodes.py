"""Nodes view — sinfo-style table with utilization bars."""

from __future__ import annotations

from datetime import datetime

from textual.app import ComposeResult
from textual.binding import Binding
from textual.widgets import DataTable, Label

from .base import BaseDataTableView
from .widgets import CyclicDataTable
from .node_detail import NodeDetailScreen

from ..slurm import Node, fetch_nodes
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
    ColumnSpec("NODE",       12, 20, 100, "xs"),
    ColumnSpec("STATE",      12, 16,  95, "xs"),
    ColumnSpec("CPU%",       14, 18,  90, "xs"),
    ColumnSpec("GPU%",       14, 18,  80, "sm"),
    ColumnSpec("CPUS A/T",  10, 12,  75, "sm"),
    ColumnSpec("GPU A/T",    9, 12,  70, "sm"),
    ColumnSpec("MEM FREE",  10, 12,  60, "md"),
    ColumnSpec("PARTITION", 12, 20,  55, "md"),
    ColumnSpec("MEM TOTAL", 10, 12,  45, "lg"),
    ColumnSpec("LOAD",       8, 10,  40, "lg"),
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


class NodesView(BaseDataTableView[Node]):
    """Displays a live sinfo-style node table."""

    BINDINGS = [
        Binding("enter", "open_node", "Open node", show=True),
        Binding("s", "sort_state", show=False),
        Binding("p", "sort_cpu", show=False),
        Binding("m", "sort_mem", show=False),
        Binding("v", "visual_enter", "Visual", show=False),
        Binding("V", "visual_enter", "Visual", show=False),
        Binding("escape", "visual_exit", "Exit visual", show=False),
        Binding("y", "yank", "Copy", show=False),
        Binding("left_square_bracket", "shift_column_left", show=False),
        Binding("right_square_bracket", "shift_column_right", show=False),
    ]

    def __init__(self, interval: float = 2.0, start_offset: float = 0.0) -> None:
        super().__init__(interval=interval, start_offset=start_offset)
        self._last_nodes: list[Node] = []
        self._last_nodes_index: dict[str, int] = {}
        self._last_sorted_nodes: list[Node] = []
        self._last_render_fp: tuple = ()
        self._current_cols: list[tuple[str, int]] = []
        self._rebuild_cache_width: int = -1
        self._rebuild_cache_names: list[str] = []
        cfg_all = config.load()
        view_state = cfg_all.get("view_state", {})
        saved_sort = str(view_state.get("nodes_sort_col", ""))
        if saved_sort in {"state", "cpu", "mem"}:
            self._sort_col = saved_sort
            self._sort_reversed = bool(view_state.get("nodes_sort_reversed", False))
        self._hidden_cols: set[str] = set(cfg_all.get("columns", {}).get("nodes_hidden", []))
        saved_order = list(cfg_all.get("columns", {}).get("nodes_order", []))
        default_order = [c.name for c in COLUMNS]
        self._column_order: list[str] = _reconcile_order(saved_order, default_order)
        self._warn_down_nodes = int(cfg_all.get("health", {}).get("warn_down_nodes", 1))

    def compose(self) -> ComposeResult:
        yield Label("", id="nodes-header")
        yield CyclicDataTable(id="nodes-table", cursor_type="row", zebra_stripes=True)

    def on_mount(self) -> None:
        self._rebuild_columns(self.size.width)
        self.start_refresh_loop()

    def _fetch_data(self) -> list[Node]:
        return fetch_nodes()

    def _get_anchor_key(self, item: Node) -> str:
        return item.name

    def on_resize(self, event) -> None:
        state = self._capture_table_state()
        self._rebuild_columns(event.size.width, force=True)
        self._render_rows(self._last_sorted_nodes)
        self._restore_table_state(state, self._last_sorted_nodes)

    def on_width_changed(self, event: WidthChanged) -> None:
        """Recompute column budget on every resize (spec §4.2)."""
        state = self._capture_table_state()
        changed = self._rebuild_columns(event.width)
        if changed and self._last_sorted_nodes:
            self._render_rows(self._last_sorted_nodes)
            self._restore_table_state(state, self._last_sorted_nodes)

    def _visible_cols_filtered(self, width: int) -> list[tuple[str, int]]:
        """Return budget-allocated columns for the given terminal width, in user-defined order."""
        budget = max(0, width - CHROME_OVERHEAD)
        col_map = {c.name: c for c in COLUMNS}
        cols = [
            col_map[name]
            for name in self._column_order
            if name not in self._hidden_cols and name in col_map
        ]
        return allocate_columns(budget, cols, current_tier=tier_for(width))

    def _rebuild_columns(self, width: int, *, force: bool = False) -> bool:
        """Rebuild column layout using budget allocation. Returns True if layout changed."""
        new_cols = self._visible_cols_filtered(width)
        visible_names = [n for n, _ in new_cols]

        if (
            not force
            and width == self._rebuild_cache_width
            and visible_names == self._rebuild_cache_names
        ):
            return False

        self._rebuild_cache_width = width
        self._rebuild_cache_names = visible_names

        if new_cols == self._current_cols:
            return False

        self._current_cols = new_cols
        table = self.query_one(CyclicDataTable)
        table.clear(columns=True)
        for name, col_width in self._current_cols:
            table.add_column(name, width=col_width)
        return True

    def _reload_column_visibility(self) -> None:
        cfg = config.load()
        self._hidden_cols = set(cfg.get("columns", {}).get("nodes_hidden", []))
        saved_order = list(cfg.get("columns", {}).get("nodes_order", []))
        default_order = [c.name for c in COLUMNS]
        self._column_order = _reconcile_order(saved_order, default_order)
        self._rebuild_columns(self.size.width, force=True)
        self._render_rows(self._last_sorted_nodes)

    def _capture_table_state(self) -> tuple[int, float, str | None]:
        table = self.query_one(CyclicDataTable)
        row = table.cursor_row
        scroll_y = float(table.scroll_offset.y)
        anchor: str | None = None
        if 0 <= row < len(self._last_sorted_nodes):
            anchor = self._last_sorted_nodes[row].name
        return row, scroll_y, anchor

    def _restore_table_state(self, state: tuple[int, float, str | None], rows: list[Node]) -> None:
        if not rows:
            return
        saved_row, scroll_y, anchor = state
        table = self.query_one(CyclicDataTable)
        row = self._last_nodes_index.get(anchor) if anchor else None
        if row is None:
            row = min(saved_row, len(rows) - 1)
        table.move_cursor(row=row)
        table.scroll_to(y=scroll_y, animate=False)

    def _set_sort(self, col: str) -> None:
        super()._set_sort(col)
        config.update({"view_state": {"nodes_sort_col": self._sort_col or "", "nodes_sort_reversed": self._sort_reversed}})
        self._last_sorted_nodes = self._sorted_visible(self._last_nodes)
        self._render_rows(self._last_sorted_nodes)

    def action_sort_state(self) -> None:
        self._set_sort("state")

    def action_sort_cpu(self) -> None:
        self._set_sort("cpu")

    def action_sort_mem(self) -> None:
        self._set_sort("mem")

    def action_yank(self) -> None:
        """Visual yank when in visual mode; no-op otherwise."""
        if self._visual_active:
            self.action_visual_yank()

    # ── Column reorder ────────────────────────────────────────────────────────

    def _persist_column_order(self) -> None:
        """Write current column order to config."""
        config.update({"columns": {"nodes_order": list(self._column_order)}})

    def _shift_visible_column(self, direction: int) -> None:
        """Shift the column under the cursor left (direction=-1) or right (+1).

        Works in visible-column space: finds the cursor column name, locates it
        in ``_column_order``, swaps with the neighbour in the same direction
        (skipping hidden columns), persists, rebuilds and re-renders.
        """
        table = self.query_one(CyclicDataTable)
        visible_names = [name for name, _ in self._current_cols]
        if not visible_names:
            return
        vis_idx = table.cursor_column
        if vis_idx < 0 or vis_idx >= len(visible_names):
            return
        col_name = visible_names[vis_idx]

        # Boundary guard
        if direction < 0 and vis_idx == 0:
            return
        if direction > 0 and vis_idx >= len(visible_names) - 1:
            return

        # Find absolute positions in _column_order for the two visible columns to swap
        abs_idx = self._column_order.index(col_name)
        neighbour_name = visible_names[vis_idx + direction]
        neighbour_abs = self._column_order.index(neighbour_name)

        # Swap
        self._column_order[abs_idx], self._column_order[neighbour_abs] = (
            self._column_order[neighbour_abs],
            self._column_order[abs_idx],
        )
        self._persist_column_order()

        state = self._capture_table_state()
        self._rebuild_columns(self.size.width, force=True)
        self._render_rows(self._last_sorted_nodes)
        # Move cursor to the moved column's new visible position
        new_vis_idx = vis_idx + direction
        table.move_cursor(column=new_vis_idx)
        self._restore_table_state(state, self._last_sorted_nodes)

    def action_shift_column_left(self) -> None:
        """Move the focused column one step to the left."""
        self._shift_visible_column(-1)

    def action_shift_column_right(self) -> None:
        """Move the focused column one step to the right."""
        self._shift_visible_column(1)

    def on_cyclic_data_table_column_reordered(self, event) -> None:
        """Handle mouse-drag column reorder from CyclicDataTable.ColumnReordered."""
        from_vis = event.from_index
        to_vis = event.to_index

        visible_names = [name for name, _ in self._current_cols]
        if not visible_names:
            return
        if from_vis < 0 or from_vis >= len(visible_names):
            return
        # Clamp to_vis into [0, len(visible_names)] — past-rightmost means append.
        to_vis = max(0, min(to_vis, len(visible_names)))

        moved_name = visible_names[from_vis]
        self._column_order.remove(moved_name)

        if to_vis >= len(visible_names):
            self._column_order.append(moved_name)
        else:
            updated_visible = [n for n in visible_names if n != moved_name]
            if to_vis >= len(updated_visible):
                self._column_order.append(moved_name)
            else:
                anchor_name = updated_visible[to_vis]
                anchor_abs = self._column_order.index(anchor_name)
                self._column_order.insert(anchor_abs, moved_name)

        self._persist_column_order()
        state = self._capture_table_state()
        self._rebuild_columns(self.size.width, force=True)
        self._render_rows(self._last_sorted_nodes)
        self._restore_table_state(state, self._last_sorted_nodes)

    def _sorted_visible(self, nodes: list[Node]) -> list[Node]:
        visible = [n for n in nodes if n.name]
        if self._sort_col == "state":
            return sorted(visible, key=lambda n: n.state, reverse=self._sort_reversed)
        elif self._sort_col == "cpu":
            return sorted(visible, key=_cpu_pct, reverse=self._sort_reversed)
        elif self._sort_col == "mem":
            return sorted(visible, key=_free_mem, reverse=self._sort_reversed)
        return visible

    def _update_nodes_header(self, nodes: list[Node]) -> None:
        visible = [n for n in nodes if n.name]
        idle = alloc = mixed = down = 0
        for n in visible:
            s = n.state.lower()
            if "idle" in s:
                idle += 1
            elif "alloc" in s:
                alloc += 1
            elif "mixed" in s:
                mixed += 1
            if "down" in s or "drain" in s:
                down += 1

        tier = getattr(getattr(self, "app", None), "tier", "sm")

        if tier == "xs":
            # xs: compact — most signal-bearing pair: idle / down
            warn = f"  [red bold]! {down} DOWN[/]" if down >= self._warn_down_nodes else ""
            self.query_one("#nodes-header", Label).update(
                f"[b]sinfo[/b]  [green]{idle} idle[/]  [red]{down} down[/]{warn}"
            )
            return

        now = datetime.now().strftime("%H:%M:%S")
        sort_tag = ""
        if self._sort_col:
            arrow = "↑" if self._sort_reversed else "↓"
            sort_tag = f"  [dim]sort:{self._sort_col}{arrow}[/]"
        warn_tag = f"  [red bold]! {down} DOWN/DRAIN[/]" if down >= self._warn_down_nodes else ""
        self.query_one("#nodes-header", Label).update(
            f"[b]sinfo[/b]  [green]{idle} idle[/]  "
            f"[cyan]{alloc} alloc[/]  [yellow]{mixed} mixed[/]  "
            f"[red]{down} down[/]  "
            f"[dim]{len(visible)} total  updated {now}[/]"
            f"{sort_tag}{warn_tag}"
        )

    def _update_table(self, nodes: list[Node]) -> None:
        state = self._capture_table_state()
        self._last_nodes = nodes
        self._last_sorted_nodes = self._sorted_visible(nodes)
        self._last_nodes_index = {n.name: i for i, n in enumerate(self._last_sorted_nodes)}

        new_fp = tuple((n.name, n.state, n.cpus_alloc, n.gpu_alloc) for n in self._last_sorted_nodes)
        if new_fp == self._last_render_fp:
            self._update_nodes_header(nodes)
            return
        self._last_render_fp = new_fp

        self._render_rows(self._last_sorted_nodes)
        self._restore_table_state(state, self._last_sorted_nodes)
        self._update_nodes_header(nodes)

    def _render_rows(self, sorted_rows: list[Node] | None = None) -> None:
        rows = sorted_rows if sorted_rows is not None else self._sorted_visible(self._last_nodes)
        table = self.query_one(CyclicDataTable)
        visual_set = self.visual_rows()
        table.clear()
        for idx, node in enumerate(rows):
            state_lower = node.state.lower().split("*")[0].rstrip("-")
            color = STATE_COLORS.get(state_lower, "white")
            row = []
            visual_prefix = "» " if idx in visual_set else ""
            for name, _ in self._current_cols:
                if name == "NODE":
                    row.append(f"[bold]{visual_prefix}{node.name}[/bold]")
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

    def _plain_cell(self, node: Node, col_name: str) -> str:
        """Return plain (markup-free) cell text for a node column."""
        if col_name == "NODE":
            return node.name
        if col_name == "STATE":
            return node.state
        if col_name == "CPU%":
            try:
                a, t = int(node.cpus_alloc), int(node.cpus_total)
                pct = round(a / t * 100) if t else 0
                return f"{pct}%"
            except (ValueError, ZeroDivisionError):
                return "N/A"
        if col_name == "GPU%":
            if node.gpu_total == 0:
                return "—"
            try:
                pct = round(node.gpu_alloc / node.gpu_total * 100)
                return f"{pct}%"
            except ZeroDivisionError:
                return "N/A"
        if col_name == "CPUS A/T":
            return f"{node.cpus_alloc}/{node.cpus_total}"
        if col_name == "GPU A/T":
            if node.gpu_total > 0:
                return f"{node.gpu_alloc}/{node.gpu_total}"
            return "—"
        if col_name == "MEM FREE":
            return f"{node.memory_free}M"
        if col_name == "PARTITION":
            return node.partition
        if col_name == "MEM TOTAL":
            return f"{node.memory_total}M"
        if col_name == "LOAD":
            return node.load
        return ""

    # ── Copy-pane interface ───────────────────────────────────────────────────

    def _pane_label(self) -> str:
        return "Nodes"

    def _current_items(self) -> list[Node]:
        return list(self._last_sorted_nodes)

    def _row_tsv(self, item: Node) -> str:
        return "\t".join(self._plain_cell(item, name) for name, _ in self._current_cols)

    def on_data_table_row_selected(self, event: DataTable.RowSelected) -> None:
        rows = self._last_sorted_nodes
        row_idx = event.cursor_row
        if row_idx >= len(rows):
            return
        node = rows[row_idx]
        self.app.push_screen(NodeDetailScreen(node))

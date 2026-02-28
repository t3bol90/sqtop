"""Health view — command latency and failure diagnostics."""
from __future__ import annotations

from datetime import datetime

from textual.app import ComposeResult
from textual.widgets import Label, Static
from textual import work

from ..slurm import CommandStat, fetch_command_health
from .widgets import CyclicDataTable


class HealthView(Static):
    """Displays lightweight command health telemetry."""

    def __init__(self, interval: float = 2.0) -> None:
        super().__init__()
        self._interval = interval
        self._timer = None
        self._fetching = False
        self._last_stats: list[CommandStat] = []

    def compose(self) -> ComposeResult:
        yield Label("", id="health-header")
        table = CyclicDataTable(id="health-table", cursor_type="row", zebra_stripes=True)
        table.add_column("COMMAND", width=22)
        table.add_column("OK", width=6)
        table.add_column("LATENCY", width=10)
        table.add_column("ERROR", width=42)
        yield table

    def on_mount(self) -> None:
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
            stats = fetch_command_health(100)
            self.app.call_from_thread(self._update_table, stats)
        finally:
            self._fetching = False

    def _update_table(self, stats: list[CommandStat]) -> None:
        self._last_stats = stats
        table = self.query_one("#health-table", CyclicDataTable)
        saved_row = table.cursor_row
        table.clear()
        for item in reversed(stats[-100:]):
            err = item.stderr[:40] + ("..." if len(item.stderr) > 40 else "")
            table.add_row(
                item.command.split(" ", 1)[0],
                "[green]yes[/]" if item.ok else "[red]no[/]",
                f"{item.latency_ms} ms",
                err,
            )
        if stats:
            table.move_cursor(row=min(saved_row, len(stats) - 1))

        failures = sum(1 for s in stats if not s.ok)
        avg_ms = int(sum(s.latency_ms for s in stats) / len(stats)) if stats else 0
        now = datetime.now().strftime("%H:%M:%S")
        self.query_one("#health-header", Label).update(
            f"[b]health[/b]  [red]{failures} failures[/]  [cyan]{avg_ms}ms avg[/]  "
            f"[dim]{len(stats)} samples  updated {now}[/]"
        )

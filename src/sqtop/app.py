"""sqtop TUI application — main app definition."""

from __future__ import annotations

from pathlib import Path
from textual.app import App, ComposeResult
from textual.binding import Binding
from textual.widgets import Footer, Header, TabbedContent, TabPane

from .views.jobs import JobsView
from .views.nodes import NodesView
from .views.partitions import PartitionsView
from .views.settings import SettingsScreen
from . import config


class SqtopApp(App):
    """Slurm TUI dashboard."""

    CSS_PATH = Path(__file__).parent / "styles" / "app.tcss"

    BINDINGS = [
        Binding("q", "quit", "Quit"),
        Binding("1", "switch_tab('jobs')", "Jobs"),
        Binding("2", "switch_tab('nodes')", "Nodes"),
        Binding("3", "switch_tab('partitions')", "Partitions"),
        Binding("r", "refresh", "Refresh"),
        Binding("S", "settings", "Settings"),
    ]

    TITLE = "sqtop"

    def __init__(self) -> None:
        super().__init__()
        cfg = config.load()
        self.interval = cfg["interval"]
        self._saved_theme = cfg["theme"]

    def on_mount(self) -> None:
        self.theme = self._saved_theme
        self.sub_title = "Slurm Dashboard"
        self.call_after_refresh(self._focus_table_for_tab, "jobs")

    def compose(self) -> ComposeResult:
        yield Header()
        with TabbedContent(initial="jobs"):
            with TabPane("Jobs [1]", id="jobs"):
                yield JobsView(self.interval)
            with TabPane("Nodes [2]", id="nodes"):
                yield NodesView(self.interval)
            with TabPane("Partitions [3]", id="partitions"):
                yield PartitionsView(self.interval)
        yield Footer()

    def action_switch_tab(self, tab_id: str) -> None:
        self.query_one(TabbedContent).active = tab_id
        self.call_after_refresh(self._focus_table_for_tab, tab_id)

    def _focus_table_for_tab(self, tab_id: str) -> None:
        table_id = {
            "jobs": "#jobs-table",
            "nodes": "#nodes-table",
            "partitions": "#partitions-table",
        }.get(tab_id)
        if not table_id:
            return
        try:
            self.query_one(table_id).focus()
        except Exception:
            # Ignore focus races during startup/resizes.
            return

    def action_refresh(self) -> None:
        for view in self.query("JobsView, NodesView, PartitionsView"):
            view.refresh_data()  # type: ignore[union-attr]

    def action_settings(self) -> None:
        self.push_screen(SettingsScreen(self.theme, self.interval))

    def set_refresh_interval(self, interval: float) -> None:
        self.interval = interval
        for view in self.query("JobsView, NodesView, PartitionsView"):
            view.set_interval_rate(interval)  # type: ignore[union-attr]

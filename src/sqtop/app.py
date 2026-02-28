"""sqtop TUI application — main app definition."""

from __future__ import annotations

from pathlib import Path
from textual.app import App, ComposeResult
from textual.binding import Binding
from textual.widgets import Footer, Header, TabbedContent, TabPane

from .views.jobs import JobsView
from .views.nodes import NodesView
from .views.settings import SettingsScreen

DEFAULT_THEME = "dracula"
DEFAULT_INTERVAL = 2.0


class SqtopApp(App):
    """Slurm TUI dashboard."""

    CSS_PATH = Path(__file__).parent / "styles" / "app.tcss"

    BINDINGS = [
        Binding("q", "quit", "Quit"),
        Binding("1", "switch_tab('jobs')", "Jobs"),
        Binding("2", "switch_tab('nodes')", "Nodes"),
        Binding("r", "refresh", "Refresh"),
        Binding("s", "settings", "Settings"),
    ]

    TITLE = "sqtop"

    def __init__(self) -> None:
        super().__init__()
        self.interval = DEFAULT_INTERVAL

    def on_mount(self) -> None:
        self.theme = DEFAULT_THEME
        self.sub_title = "Slurm Dashboard"

    def compose(self) -> ComposeResult:
        yield Header()
        with TabbedContent(initial="jobs"):
            with TabPane("Jobs [1]", id="jobs"):
                yield JobsView(self.interval)
            with TabPane("Nodes [2]", id="nodes"):
                yield NodesView(self.interval)
        yield Footer()

    def action_switch_tab(self, tab_id: str) -> None:
        self.query_one(TabbedContent).active = tab_id

    def action_refresh(self) -> None:
        for view in self.query("JobsView, NodesView"):
            view.refresh_data()  # type: ignore[union-attr]

    def action_settings(self) -> None:
        self.push_screen(SettingsScreen(self.theme, self.interval))

    def set_refresh_interval(self, interval: float) -> None:
        self.interval = interval
        for view in self.query("JobsView, NodesView"):
            view.set_interval_rate(interval)  # type: ignore[union-attr]

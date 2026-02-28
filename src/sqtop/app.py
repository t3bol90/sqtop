"""sqtop TUI application — main app definition."""

from __future__ import annotations

from pathlib import Path
from textual.app import App, ComposeResult
from textual.binding import Binding
from textual.widgets import Footer, Header, TabbedContent, TabPane

from .views.command_palette import CommandPaletteScreen
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
        Binding("ctrl+p", "command_palette", "Palette"),
        Binding("r", "refresh", "Refresh"),
        Binding("P", "save_screenshot", "Screenshot", show=False),
        Binding("S", "settings", "Settings", show=False),
    ]

    TITLE = "sqtop"

    def __init__(self) -> None:
        super().__init__()
        cfg = config.load()
        self.interval = cfg["interval"]
        self._saved_theme = cfg["theme"]
        self.expert_mode = bool(cfg.get("ui", {}).get("expert_mode", False))
        self.confirm_cancel_single = bool(cfg.get("safety", {}).get("confirm_cancel_single", True))
        self.confirm_bulk_actions = bool(cfg.get("safety", {}).get("confirm_bulk_actions", True))

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
        self.push_screen(
            SettingsScreen(
                self.theme,
                self.interval,
                self.expert_mode,
                self.confirm_cancel_single,
                self.confirm_bulk_actions,
            )
        )

    def action_save_screenshot(self) -> None:
        screenshot_dir = Path.home() / ".cache" / "sqtop" / "screenshots"
        screenshot_dir.mkdir(parents=True, exist_ok=True)
        try:
            path = self.save_screenshot(path=str(screenshot_dir))
            self.notify(f"Saved screenshot: {path}", title="Screenshot")
        except Exception as exc:
            self.notify(f"Screenshot failed: {exc}", title="Screenshot", severity="error")

    def action_command_palette(self) -> None:
        def handle(action: str | None) -> None:
            if action is None:
                return
            if action in {"jobs", "nodes", "partitions"}:
                self.action_switch_tab(action)
            elif action == "refresh":
                self.action_refresh()
            elif action == "settings":
                self.action_settings()
            elif action == "screenshot":
                self.action_save_screenshot()

        self.push_screen(CommandPaletteScreen(), handle)

    def set_refresh_interval(self, interval: float) -> None:
        self.interval = interval
        for view in self.query("JobsView, NodesView, PartitionsView"):
            view.set_interval_rate(interval)  # type: ignore[union-attr]

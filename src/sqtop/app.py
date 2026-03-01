"""sqtop TUI application — main app definition."""

from __future__ import annotations

from collections.abc import Iterable
from pathlib import Path
from textual.app import App, ComposeResult, SystemCommand
from textual.binding import Binding
from textual.screen import Screen
from textual.widgets import Footer, Header, TabbedContent, TabPane

from .views.jobs import JobsView, COLUMNS as JOBS_COLUMNS
from .views.nodes import NodesView, COLUMNS as NODES_COLUMNS
from .views.partitions import PartitionsView, COLUMNS as PARTITIONS_COLUMNS
from .views.column_toggle import ColumnToggleScreen
from . import config, slurm


class SqtopApp(App):
    """Slurm TUI dashboard."""

    CSS_PATH = Path(__file__).parent / "styles" / "app.tcss"

    BINDINGS = [
        Binding("q", "quit", "Quit"),
        Binding("ctrl+c", "quit", "Quit", show=False),
        Binding("1", "switch_tab('jobs')", "Jobs"),
        Binding("2", "switch_tab('nodes')", "Nodes"),
        Binding("3", "switch_tab('partitions')", "Partitions"),
        Binding("r", "refresh", "Refresh"),
        Binding("P", "save_screenshot", "Screenshot", show=False),
        Binding("S", "command_palette", "Commands", show=False),
        Binding("ctrl+p", "command_palette", "Commands", show=False),
        Binding("C", "column_toggle", "Columns", show=False),
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

    def watch_theme(self, theme: str) -> None:
        config.save(theme, self.interval)

    def on_mount(self) -> None:
        self.theme = self._saved_theme
        if slurm._SSH_HOST:
            self.sub_title = f"Slurm Dashboard — {slurm._SSH_HOST}"
        else:
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

    def action_column_toggle(self) -> None:
        active = self.query_one(TabbedContent).active
        cfg = config.load()
        if active == "jobs":
            view = self.query_one(JobsView)
            all_cols = [name for name, _, _ in JOBS_COLUMNS]
            hidden = list(cfg.get("columns", {}).get("jobs_hidden", []))
        elif active == "nodes":
            view = self.query_one(NodesView)
            all_cols = [name for name, _, _ in NODES_COLUMNS]
            hidden = list(cfg.get("columns", {}).get("nodes_hidden", []))
        elif active == "partitions":
            view = self.query_one(PartitionsView)
            all_cols = [name for name, _ in PARTITIONS_COLUMNS]
            hidden = list(cfg.get("columns", {}).get("partitions_hidden", []))
        else:
            return

        def _make_callback(v):
            return lambda _: v._reload_column_visibility()

        self.push_screen(ColumnToggleScreen(active, all_cols, hidden), _make_callback(view))

    def get_system_commands(self, screen: Screen) -> Iterable[SystemCommand]:
        yield from super().get_system_commands(screen)
        yield SystemCommand("Refresh data", "Refresh all views now", self.action_refresh)
        for secs in [1.0, 2.0, 5.0, 10.0, 30.0]:
            label = f"{secs:.0f}s"
            yield SystemCommand(
                f"Set refresh: {label}",
                f"Set auto-refresh interval to {label}",
                lambda s=secs: self._set_interval_and_save(s),
                discover=False,
            )
        mode = "on" if self.expert_mode else "off"
        yield SystemCommand(
            f"Expert mode: {mode} → toggle",
            "Toggle expert mode (fewer confirmation dialogs)",
            self._toggle_expert_mode,
        )
        ccs = "on" if self.confirm_cancel_single else "off"
        yield SystemCommand(
            f"Confirm single cancel: {ccs} → toggle",
            "Toggle confirmation dialog for single job cancel",
            self._toggle_confirm_cancel_single,
        )
        cba = "on" if self.confirm_bulk_actions else "off"
        yield SystemCommand(
            f"Confirm bulk actions: {cba} → toggle",
            "Toggle confirmation for bulk operations",
            self._toggle_confirm_bulk_actions,
        )
        yield SystemCommand(
            "Column visibility",
            "Show/hide columns for the current view",
            self.action_column_toggle,
        )
        yield SystemCommand(
            "Save screenshot",
            "Save a screenshot of sqtop",
            self.action_save_screenshot,
            discover=False,
        )

    def _set_interval_and_save(self, secs: float) -> None:
        self.set_refresh_interval(secs)
        config.save(self.theme, secs)

    def _toggle_expert_mode(self) -> None:
        self.expert_mode = not self.expert_mode
        config.update({"ui": {"expert_mode": self.expert_mode}})
        self.notify(f"Expert mode: {'on' if self.expert_mode else 'off'}", title="Settings")

    def _toggle_confirm_cancel_single(self) -> None:
        self.confirm_cancel_single = not self.confirm_cancel_single
        config.update({"safety": {"confirm_cancel_single": self.confirm_cancel_single}})
        self.notify(
            f"Confirm single cancel: {'on' if self.confirm_cancel_single else 'off'}",
            title="Settings",
        )

    def _toggle_confirm_bulk_actions(self) -> None:
        self.confirm_bulk_actions = not self.confirm_bulk_actions
        config.update({"safety": {"confirm_bulk_actions": self.confirm_bulk_actions}})
        self.notify(
            f"Confirm bulk actions: {'on' if self.confirm_bulk_actions else 'off'}",
            title="Settings",
        )

    def action_save_screenshot(self) -> None:
        screenshot_dir = Path.home() / ".cache" / "sqtop" / "screenshots"
        screenshot_dir.mkdir(parents=True, exist_ok=True)
        try:
            path = self.save_screenshot(path=str(screenshot_dir))
            self.notify(f"Saved screenshot: {path}", title="Screenshot")
        except Exception as exc:
            self.notify(f"Screenshot failed: {exc}", title="Screenshot", severity="error")

    def action_show_help_panel(self) -> None:
        """Open Textual help panel; fail gracefully if optional deps are missing."""
        try:
            super().action_show_help_panel()
        except Exception as exc:
            self.notify(
                f"Help panel unavailable: {exc}",
                title="Help",
                severity="warning",
                timeout=8,
            )

    def set_refresh_interval(self, interval: float) -> None:
        self.interval = interval
        for view in self.query("JobsView, NodesView, PartitionsView"):
            view.set_interval_rate(interval)  # type: ignore[union-attr]

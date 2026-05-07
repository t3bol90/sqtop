"""sqtop TUI application — main app definition."""

from __future__ import annotations

import dataclasses
import shutil
from collections.abc import Iterable
from pathlib import Path
from textual.app import App, ComposeResult, SystemCommand
from textual.binding import Binding
from textual.css.query import NoMatches
from textual.reactive import reactive
from textual.screen import Screen
from textual.widgets import Footer, Header, Static, TabbedContent, TabPane

from .views.base import BaseDataTableView
from .views.jobs import JobsView, COLUMNS as JOBS_COLUMNS
from .views.nodes import NodesView, COLUMNS as NODES_COLUMNS
from .views.partitions import PartitionsView, COLUMNS as PARTITIONS_COLUMNS
from .views.history import HistoryView
from .views.column_toggle import ColumnToggleScreen
from .views.keybindings_help import KeybindingHelpScreen
from . import config, slurm
from .clipboard import app_copy
from .responsive import Tier, TIER_WIDTH, TOO_SMALL_WIDTH, TOO_SMALL_HEIGHT, WidthChanged, tier_for

# (sort_key, human-readable label) — order determines palette display order
_JOBS_SORT_OPTIONS: list[tuple[str, str]] = [
    ("", "State priority (default)"),
    ("state", "State"),
    ("time", "Time used"),
    ("cpus", "CPUs"),
    ("qos", "QOS"),
]

# Tab labels: (short_label, full_label) keyed by TabPane id.
# short_label is used at xs tier; full_label at sm+.
_TAB_LABELS: dict[str, tuple[str, str]] = {
    "jobs":       ("Jobs",       "Jobs [1]"),
    "nodes":      ("Nodes",      "Nodes [2]"),
    "partitions": ("Partitions", "Partitions [3]"),
    "history":    ("History",    "History [4]"),
}

# Minimum tier for each action's binding to be shown in the Footer.
# Actions not listed here inherit their original show=True/False from BINDINGS.
# "xs" means always show (when BINDINGS has show=True).
# "sm" means hide at xs, show at sm+.
# "md" means hide at xs/sm, show at md+.
_BINDING_SHOW_AT: dict[str, Tier] = {
    # Always visible (xs+)
    "quit":             "xs",
    "show_keybindings": "xs",
    # sm+ bindings
    "refresh":                      "sm",
    "switch_tab('jobs')":           "sm",
    "switch_tab('nodes')":          "sm",
    "switch_tab('partitions')":     "sm",
    "switch_tab('history')":        "sm",
}


class SqtopApp(App):
    """Slurm TUI dashboard."""

    CSS_PATH = Path(__file__).parent / "styles" / "app.tcss"

    BINDINGS = [
        Binding("q", "quit", "Quit"),
        Binding("ctrl+c", "quit", "Quit", show=False),
        Binding("1", "switch_tab('jobs')", "Jobs"),
        Binding("2", "switch_tab('nodes')", "Nodes"),
        Binding("3", "switch_tab('partitions')", "Partitions"),
        Binding("4", "switch_tab('history')", "History"),
        Binding("r", "refresh", "Refresh"),
        Binding("P", "toggle_pause", "Pause", show=False),
        Binding("S", "command_palette", "Commands", show=False),
        Binding("ctrl+p", "command_palette", "Commands", show=False),
        Binding("C", "column_toggle", "Columns", show=False),
        Binding("question_mark", "show_keybindings", "Keys", show=True),
        Binding("ctrl+shift+y", "copy_pane", "Copy pane", show=False),
    ]

    TITLE = "sqtop"

    # Responsive tier reactive — initialized from terminal size before first paint.
    tier: reactive[Tier] = reactive("sm")

    # Too-small floor — True when terminal is below 40×10.
    too_small: reactive[bool] = reactive(False)

    def __init__(self) -> None:
        super().__init__()
        cfg = config.load()
        self.interval = cfg["interval"]
        self._saved_theme = cfg["theme"]
        self._paused: bool = False
        self.expert_mode = bool(cfg.get("ui", {}).get("expert_mode", False))
        self.confirm_cancel_single = bool(cfg.get("safety", {}).get("confirm_cancel_single", True))
        self.confirm_bulk_actions = bool(cfg.get("safety", {}).get("confirm_bulk_actions", True))
        # Synchronously read real terminal size before Textual mounts anything so
        # first-paint uses the correct tier (spec §4.1).
        size = shutil.get_terminal_size(fallback=(80, 24))
        self._initial_width: int = size.columns
        self._initial_height: int = size.lines
        self.tier = tier_for(self._initial_width)
        self.too_small = (
            self._initial_width < TOO_SMALL_WIDTH
            or self._initial_height < TOO_SMALL_HEIGHT
        )

    def watch_tier(self, old: str | None, new: str) -> None:
        """Swap tier-* CSS class on self.screen when tier changes."""
        for t in ("xs", "sm", "md", "lg"):
            screen.remove_class(f"tier-{t}")
        screen.add_class(f"tier-{new}")
        self._apply_tier_to_tabs(new)
        self._apply_sub_title(new)
        self._apply_tier_to_bindings(new)

    def watch_too_small(self, value: bool) -> None:
        """Toggle app-too-small CSS class and update the message widget."""
        try:
            screen = self.screen
        except Exception:
            return
        if value:
            screen.add_class("app-too-small")
        else:
            screen.remove_class("app-too-small")
        self._update_too_small_message()

    def _update_too_small_message(self) -> None:
        """Refresh the dimensions shown in the too-small overlay."""
        try:
            widget = self.query_one("#too-small-message", Static)
        except Exception:
            return
        w = self.size.width or self._initial_width
        h = self.size.height or self._initial_height
        widget.update(
            f"Terminal too small.\n"
            f"Resize to at least {TOO_SMALL_WIDTH}×{TOO_SMALL_HEIGHT}.\n"
            f"Current: {w}×{h}"
        )

    def watch_theme(self, theme: str) -> None:
        config.save(theme, self.interval)

    def on_mount(self) -> None:
        self.theme = self._saved_theme
        # Compute the base sub_title string once; _apply_sub_title will truncate it.
        if slurm._SSH_HOST:
            self._base_sub_title = f"Slurm Dashboard — {slurm._SSH_HOST}"
        else:
            self._base_sub_title = "Slurm Dashboard"
        # Ensure correct tier class is applied on first paint (spec §4.1).
        for t in ("xs", "sm", "md", "lg"):
            self.screen.remove_class(f"tier-{t}")
        self.screen.add_class(f"tier-{self.tier}")
        self._apply_tier_to_tabs(self.tier)
        self._apply_sub_title(self.tier)
        self._apply_tier_to_bindings(self.tier)
        # Apply too-small class if needed on first paint.
        if self.too_small:
            self.screen.add_class("app-too-small")
        self._update_too_small_message()
        self.call_after_refresh(self._focus_table_for_tab, "jobs")

    # ── Tier-driven chrome helpers ────────────────────────────────────────────

    def _apply_tier_to_tabs(self, tier: str) -> None:
        """Update tab labels: short at xs, full at sm+."""
        try:
            tc = self.query_one(TabbedContent)
        except Exception:
            return
        for pane_id, (short, full) in _TAB_LABELS.items():
            try:
                tab = tc.get_tab(pane_id)
                tab.label = short if tier == "xs" else full
            except Exception:
                pass

    def _apply_sub_title(self, tier: str) -> None:
        """Set sub_title per tier: empty at xs, truncated at sm+."""
        base = getattr(self, "_base_sub_title", "Slurm Dashboard")
        if tier == "xs":
            self.sub_title = ""
            return
        # Truncate to ≤ width // 2 - 10 cells at sm+.
        max_width = max(0, self._initial_width // 2 - 10)
        if len(base) <= max_width or max_width <= 0:
            self.sub_title = base
        elif max_width < 2:
            self.sub_title = ""
        else:
            self.sub_title = base[: max_width - 1] + "…"

    def _apply_tier_to_bindings(self, tier: str) -> None:
        """Adjust Footer show flags based on current tier.

        Bindings whose action is not listed in _BINDING_SHOW_AT keep their
        original show value.  Listed actions are shown only when the current
        tier is >= their minimum tier AND the binding was originally show=True
        in the class-level BINDINGS list.
        """
        _TIER_RANK: dict[str, int] = {"xs": 0, "sm": 1, "md": 2, "lg": 3}
        current_rank = _TIER_RANK.get(tier, 0)

        # Build a lookup of original show values from the class BINDINGS list.
        # Use (key, action) as the identity since one key may have multiple bindings.
        original_show: dict[tuple[str, str], bool] = {}
        for b in self.BINDINGS:
            if isinstance(b, Binding):
                original_show[(b.key, b.action)] = b.show
            elif isinstance(b, tuple) and len(b) >= 2:
                # tuple form: (key, action) or (key, action, description)
                key, action = b[0], b[1]
                original_show[(key, action)] = True  # tuples default show=True

        new_keys: dict[str, list[Binding]] = {}
        for key, bindings in self._bindings.key_to_bindings.items():
            new_list: list[Binding] = []
            for binding in bindings:
                min_tier = _BINDING_SHOW_AT.get(binding.action)
                if min_tier is None:
                    # Not in our responsiveness map — preserve binding as-is.
                    new_list.append(binding)
                else:
                    orig = original_show.get((binding.key, binding.action), binding.show)
                    min_rank = _TIER_RANK.get(min_tier, 0)
                    # Show only if originally show=True AND current tier qualifies.
                    desired_show = orig and current_rank >= min_rank
                    if binding.show != desired_show:
                        new_list.append(dataclasses.replace(binding, show=desired_show))
                    else:
                        new_list.append(binding)
            new_keys[key] = new_list
        self._bindings.key_to_bindings = new_keys
        self.refresh_bindings()

    def on_resize(self, event) -> None:
        """Update tier, too_small, and broadcast WidthChanged on every resize."""
        width: int = event.size.width
        height: int = event.size.height
        new_tier = tier_for(width)
        if new_tier != self.tier:
            self.tier = new_tier
        new_too_small = width < TOO_SMALL_WIDTH or height < TOO_SMALL_HEIGHT
        if new_too_small != self.too_small:
            self.too_small = new_too_small
        elif new_too_small:
            self._update_too_small_message()
        self.post_message(WidthChanged(width, height, new_tier))

    def push_screen(self, screen, callback=None, wait_for_dismiss: bool = False, **kwargs):
        """Wrap push_screen to call responsive_clamp before mount (spec §5.5)."""
        if hasattr(screen, "responsive_clamp"):
            try:
                screen.responsive_clamp(self.tier)
            except Exception:
                pass  # never let clamp failure block a modal
        return super().push_screen(screen, callback, wait_for_dismiss=wait_for_dismiss, **kwargs)

    def compose(self) -> ComposeResult:
        yield Static("", id="too-small-message")
        yield Header()
        with TabbedContent(initial="jobs"):
            with TabPane("Jobs [1]", id="jobs"):
                yield JobsView(self.interval)
            with TabPane("Nodes [2]", id="nodes"):
                yield NodesView(self.interval, start_offset=0.7)
            with TabPane("Partitions [3]", id="partitions"):
                yield PartitionsView(self.interval, start_offset=1.4)
            with TabPane("History [4]", id="history"):
                yield HistoryView(interval=30.0, start_offset=2.1)
        yield Footer()

    def action_switch_tab(self, tab_id: str) -> None:
        self.query_one(TabbedContent).active = tab_id
        self.call_after_refresh(self._focus_table_for_tab, tab_id)

    def _focus_table_for_tab(self, tab_id: str) -> None:
        table_id = {
            "jobs": "#jobs-table",
            "nodes": "#nodes-table",
            "partitions": "#partitions-table",
            "history": "#history-table",
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

    def action_toggle_pause(self) -> None:
        self._paused = not self._paused
        for view in self.query(BaseDataTableView):
            if self._paused:
                view.pause()
            else:
                view.resume()
        self.notify("Paused" if self._paused else "Resumed", title="Refresh")

    def action_column_toggle(self) -> None:
        active = self.query_one(TabbedContent).active
        cfg = config.load()
        if active == "jobs":
            view = self.query_one(JobsView)
            all_cols = [col.name for col in JOBS_COLUMNS]
            hidden = list(cfg.get("columns", {}).get("jobs_hidden", []))
        elif active == "nodes":
            view = self.query_one(NodesView)
            all_cols = [col.name for col in NODES_COLUMNS]
            hidden = list(cfg.get("columns", {}).get("nodes_hidden", []))
        elif active == "partitions":
            view = self.query_one(PartitionsView)
            all_cols = [col.name for col in PARTITIONS_COLUMNS]
            hidden = list(cfg.get("columns", {}).get("partitions_hidden", []))
        else:
            return

        def _make_callback(v):
            return lambda _: v._reload_column_visibility()

        self.push_screen(ColumnToggleScreen(active, all_cols, hidden), _make_callback(view))

    def action_show_keybindings(self) -> None:
        pane_name = "Jobs"
        pane_bindings = list(JobsView.BINDINGS)
        try:
            active = self.query_one(TabbedContent).active or "jobs"
        except Exception:
            active = "jobs"
        if active == "nodes":
            pane_name = "Nodes"
            pane_bindings = list(NodesView.BINDINGS)
        elif active == "partitions":
            pane_name = "Partitions"
            pane_bindings = list(PartitionsView.BINDINGS)
        self.push_screen(KeybindingHelpScreen(pane_name, list(self.BINDINGS), pane_bindings))

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
        for sort_val, sort_label in _JOBS_SORT_OPTIONS:
            yield SystemCommand(
                f"Jobs default sort: {sort_label}",
                f"Set jobs default sort to '{sort_label}' and persist",
                lambda v=sort_val: self._set_jobs_default_sort(v),
                discover=False,
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

    def _set_jobs_default_sort(self, col: str) -> None:
        label = next((lbl for key, lbl in _JOBS_SORT_OPTIONS if key == col), col)
        try:
            self.query_one(JobsView)._set_sort(col)
        except NoMatches:
            config.update({"view_state": {"jobs_sort_col": col, "jobs_sort_reversed": False}})
        self.notify(f"Jobs sort: {label}", title="Settings")

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

    def action_copy_pane(self) -> None:
        """Copy the active pane's full contents to clipboard."""
        # If a modal is on top of the screen stack, copy from it.
        screen = self.screen
        if hasattr(screen, "copy_pane"):
            label, payload, count = screen.copy_pane()
            app_copy(self, payload, label=f"Copied pane: {label}", count=count)
            return

        # Otherwise resolve the active tab view.
        try:
            active = self.query_one(TabbedContent).active
        except Exception:
            self.notify("No active pane to copy", severity="warning")
            return

        view_map = {
            "jobs": JobsView,
            "nodes": NodesView,
            "partitions": PartitionsView,
            "history": HistoryView,
        }
        view_cls = view_map.get(active)
        if view_cls is None:
            self.notify("No active pane to copy", severity="warning")
            return
        try:
            view = self.query_one(view_cls)
        except Exception:
            self.notify("No active pane to copy", severity="warning")
            return

        label, payload, count = view.copy_pane()
        app_copy(self, payload, label=f"Copied pane: {label}", count=count)

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

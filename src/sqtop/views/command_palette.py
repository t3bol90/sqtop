"""Simple command palette modal."""
from __future__ import annotations

from textual.app import ComposeResult
from textual.binding import Binding
from textual.screen import ModalScreen
from textual.widgets import Button, Label, Static


class CommandPaletteScreen(ModalScreen[str | None]):
    """Pick a global app command."""

    BINDINGS = [
        Binding("escape", "dismiss(None)", show=False),
        Binding("up", "focus_previous", show=False),
        Binding("down", "focus_next", show=False),
    ]

    CSS = """
    CommandPaletteScreen { align: center middle; }
    #palette-dialog {
        width: 56; height: auto;
        border: double $accent;
        background: $surface;
        padding: 1 2;
    }
    #btn-refresh, #btn-jobs, #btn-nodes, #btn-partitions, #btn-settings, #btn-shot, #btn-close {
        width: 100%; margin-top: 1;
    }
    """

    def compose(self) -> ComposeResult:
        with Static(id="palette-dialog"):
            yield Label("Command Palette", id="palette-title")
            yield Button("Refresh now", id="btn-refresh", variant="primary")
            yield Button("Switch to Jobs", id="btn-jobs")
            yield Button("Switch to Nodes", id="btn-nodes")
            yield Button("Switch to Partitions", id="btn-partitions")
            yield Button("Open Settings", id="btn-settings")
            yield Button("Save Screenshot", id="btn-shot")
            yield Button("Close  [dim]esc[/]", id="btn-close")

    def _focused_button_index(self) -> int:
        buttons = list(self.query(Button))
        focused = self.focused
        try:
            return buttons.index(focused)
        except ValueError:
            return 0

    def action_focus_next(self) -> None:
        buttons = list(self.query(Button))
        if buttons:
            buttons[(self._focused_button_index() + 1) % len(buttons)].focus()

    def action_focus_previous(self) -> None:
        buttons = list(self.query(Button))
        if buttons:
            buttons[(self._focused_button_index() - 1) % len(buttons)].focus()

    def on_button_pressed(self, event: Button.Pressed) -> None:
        mapping = {
            "btn-refresh": "refresh",
            "btn-jobs": "jobs",
            "btn-nodes": "nodes",
            "btn-partitions": "partitions",
            "btn-settings": "settings",
            "btn-shot": "screenshot",
        }
        self.dismiss(mapping.get(event.button.id))

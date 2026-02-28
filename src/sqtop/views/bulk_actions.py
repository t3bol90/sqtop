"""Bulk job action modal."""
from __future__ import annotations

from textual.app import ComposeResult
from textual.binding import Binding
from textual.screen import ModalScreen
from textual.widgets import Button, Label, Static


class BulkActionScreen(ModalScreen[str | None]):
    """Choose a bulk action for selected jobs."""

    BINDINGS = [
        Binding("escape", "dismiss(None)", show=False),
        Binding("up", "focus_previous", show=False),
        Binding("down", "focus_next", show=False),
    ]

    CSS = """
    BulkActionScreen { align: center middle; }
    #bulk-dialog {
        width: 52; height: auto;
        border: double $primary;
        background: $surface;
        padding: 1 2;
    }
    #btn-cancel, #btn-hold, #btn-release, #btn-requeue, #btn-close {
        width: 100%; margin-top: 1;
    }
    """

    def __init__(self, selected_count: int) -> None:
        super().__init__()
        self._selected_count = selected_count

    def compose(self) -> ComposeResult:
        with Static(id="bulk-dialog"):
            yield Label(f"Bulk actions for {self._selected_count} selected jobs")
            yield Button("Cancel selected", id="btn-cancel", variant="error")
            yield Button("Hold selected", id="btn-hold", variant="warning")
            yield Button("Release selected", id="btn-release", variant="default")
            yield Button("Requeue selected", id="btn-requeue", variant="primary")
            yield Button("Close  [dim]esc[/]", id="btn-close", variant="default")

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
            "btn-cancel": "cancel",
            "btn-hold": "hold",
            "btn-release": "release",
            "btn-requeue": "requeue",
        }
        self.dismiss(mapping.get(event.button.id))

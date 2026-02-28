"""Generic Yes/No confirmation modal."""
from __future__ import annotations

from textual.app import ComposeResult
from textual.binding import Binding
from textual.screen import ModalScreen
from textual.widgets import Button, Label, Static


class ConfirmScreen(ModalScreen[bool]):
    """Ask user to confirm an action. Returns True (yes) or False (no/esc)."""

    BINDINGS = [
        Binding("y", "confirm_yes", show=False),
        Binding("n", "confirm_no", show=False),
        Binding("escape", "confirm_no", show=False),
    ]

    CSS = """
    ConfirmScreen { align: center middle; }
    #confirm-dialog {
        width: 50; height: auto;
        border: double $warning;
        background: $surface;
        padding: 1 2;
    }
    #confirm-message { margin-bottom: 1; }
    #btn-yes { width: 100%; margin-top: 1; }
    #btn-no  { width: 100%; margin-top: 1; }
    """

    def __init__(self, message: str) -> None:
        super().__init__()
        self._message = message

    def compose(self) -> ComposeResult:
        with Static(id="confirm-dialog"):
            yield Label(self._message, id="confirm-message")
            yield Button("Yes  [dim]y[/]", id="btn-yes", variant="error")
            yield Button("No   [dim]n / esc[/]", id="btn-no", variant="default")

    def action_confirm_yes(self) -> None:
        self.dismiss(True)

    def action_confirm_no(self) -> None:
        self.dismiss(False)

    def on_button_pressed(self, event: Button.Pressed) -> None:
        self.dismiss(event.button.id == "btn-yes")

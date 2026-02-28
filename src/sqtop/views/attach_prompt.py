"""Prompt for optional node override when attaching to a job."""
from __future__ import annotations

from textual.app import ComposeResult
from textual.binding import Binding
from textual.screen import ModalScreen
from textual.widgets import Button, Input, Label, Static


class AttachNodePromptScreen(ModalScreen[str | None]):
    """Collect node expression for attach override."""

    BINDINGS = [
        Binding("escape", "dismiss(None)", show=False),
    ]

    CSS = """
    AttachNodePromptScreen { align: center middle; }
    #attach-dialog {
        width: 60; height: auto;
        border: double $primary;
        background: $surface;
        padding: 1 2;
    }
    #attach-node-input { margin-top: 1; }
    #btn-attach-ok, #btn-attach-cancel { width: 100%; margin-top: 1; }
    """

    def __init__(self, default_node: str) -> None:
        super().__init__()
        self._default_node = default_node

    def compose(self) -> ComposeResult:
        with Static(id="attach-dialog"):
            yield Label("Attach with node override", id="attach-title")
            yield Input(
                value=self._default_node,
                placeholder="node name/expression (empty to skip -w)",
                id="attach-node-input",
            )
            yield Button("Attach", id="btn-attach-ok", variant="primary")
            yield Button("Cancel", id="btn-attach-cancel", variant="default")

    def on_mount(self) -> None:
        self.query_one("#attach-node-input", Input).focus()

    def on_input_submitted(self, event: Input.Submitted) -> None:
        if event.input.id == "attach-node-input":
            self.dismiss(event.value.strip())

    def on_button_pressed(self, event: Button.Pressed) -> None:
        if event.button.id == "btn-attach-ok":
            value = self.query_one("#attach-node-input", Input).value.strip()
            self.dismiss(value)
        else:
            self.dismiss(None)

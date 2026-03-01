"""Column visibility toggle modal — per-view column show/hide."""
from __future__ import annotations

from textual.app import ComposeResult
from textual.binding import Binding
from textual.screen import ModalScreen
from textual.widgets import Button, Checkbox, Label, Static

from .. import config


class ColumnToggleScreen(ModalScreen[None]):
    """Modal with checkboxes to toggle column visibility."""

    BINDINGS = [
        Binding("escape", "dismiss(None)", show=False),
        Binding("C", "dismiss(None)", show=False),
    ]

    CSS = """
    ColumnToggleScreen { align: center middle; }
    #col-dialog {
        width: 40; height: auto;
        border: double $primary;
        background: $surface;
        padding: 1 2;
    }
    #col-title { text-style: bold; color: $primary; margin-bottom: 1; }
    Checkbox { margin-top: 0; }
    #btn-col-close { width: 100%; margin-top: 1; }
    """

    def __init__(self, view_name: str, all_columns: list[str], hidden_columns: list[str]) -> None:
        super().__init__()
        self._view_name = view_name
        self._all_columns = all_columns
        self._hidden: set[str] = set(hidden_columns)

    def compose(self) -> ComposeResult:
        with Static(id="col-dialog"):
            yield Label(f"Column visibility — {self._view_name}", id="col-title")
            for col in self._all_columns:
                yield Checkbox(col, value=(col not in self._hidden), id=f"col-{self._all_columns.index(col)}")
            yield Button("Close  [dim]esc[/]", id="btn-col-close", variant="default")

    def on_checkbox_changed(self, event: Checkbox.Changed) -> None:
        # Read column name from label text, not from ID (avoids sanitization issues)
        col_name = str(event.checkbox.label)
        if event.value:
            self._hidden.discard(col_name)
        else:
            self._hidden.add(col_name)
        config.update({"columns": {f"{self._view_name}_hidden": list(self._hidden)}})

    def on_button_pressed(self, event: Button.Pressed) -> None:
        self.dismiss(None)

"""Settings modal screen."""

from __future__ import annotations

from textual.app import ComposeResult
from textual.binding import Binding
from textual.screen import ModalScreen
from textual.widgets import Button, Label, RadioButton, RadioSet, Static


THEMES = [
    "dracula",
    "monokai",
    "tokyo-night",
    "nord",
    "gruvbox",
]

INTERVALS = [1.0, 2.0, 5.0, 10.0, 30.0]


class SettingsScreen(ModalScreen[None]):
    """Settings overlay — theme and refresh rate."""

    BINDINGS = [
        Binding("escape", "dismiss", show=False),
        Binding("s", "dismiss", show=False),
    ]

    CSS = """
    SettingsScreen {
        align: center middle;
    }

    #dialog {
        width: 44;
        height: auto;
        border: double $primary;
        background: $surface;
        padding: 1 2;
    }

    #dialog .section-title {
        text-style: bold;
        color: $primary;
        margin-top: 1;
    }

    #dialog RadioSet {
        border: none;
        height: auto;
        background: transparent;
        padding: 0;
    }

    #close {
        margin-top: 1;
        width: 100%;
    }
    """

    def __init__(self, current_theme: str, current_interval: float) -> None:
        super().__init__()
        self._current_theme = current_theme
        self._current_interval = current_interval

    def compose(self) -> ComposeResult:
        with Static(id="dialog"):
            yield Label("⚙  Settings", id="title")

            yield Label("Theme", classes="section-title")
            with RadioSet(id="theme-picker"):
                for theme in THEMES:
                    yield RadioButton(theme, value=(theme == self._current_theme))

            yield Label("Refresh rate", classes="section-title")
            with RadioSet(id="interval-picker"):
                for secs in INTERVALS:
                    label = f"{secs:.0f}s"
                    yield RadioButton(label, value=(secs == self._current_interval))

            yield Button("Close  [dim]esc[/]", id="close", variant="primary")

    def on_radio_set_changed(self, event: RadioSet.Changed) -> None:
        radio_set_id = event.radio_set.id
        label = str(event.pressed.label)

        if radio_set_id == "theme-picker":
            self.app.theme = label  # type: ignore[attr-defined]

        elif radio_set_id == "interval-picker":
            secs = float(label.removesuffix("s"))
            self.app.set_refresh_interval(secs)  # type: ignore[attr-defined]

    def on_button_pressed(self, event: Button.Pressed) -> None:
        self.dismiss()

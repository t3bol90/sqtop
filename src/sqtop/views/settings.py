"""Settings modal screen."""

from __future__ import annotations

from textual.app import ComposeResult
from textual.binding import Binding
from textual.screen import ModalScreen
from textual.widgets import Button, Checkbox, Label, RadioButton, RadioSet, Static

from .. import config


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

    def __init__(
        self,
        current_theme: str,
        current_interval: float,
        expert_mode: bool,
        confirm_cancel_single: bool,
        confirm_bulk_actions: bool,
    ) -> None:
        super().__init__()
        self._current_theme = current_theme
        self._current_interval = current_interval
        self._expert_mode = expert_mode
        self._confirm_cancel_single = confirm_cancel_single
        self._confirm_bulk_actions = confirm_bulk_actions

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

            yield Label("Workflow", classes="section-title")
            yield Checkbox("Expert mode (fewer confirmations)", value=self._expert_mode, id="expert-mode")
            yield Checkbox(
                "Confirm single cancel",
                value=self._confirm_cancel_single,
                id="confirm-cancel-single",
            )
            yield Checkbox(
                "Confirm bulk actions",
                value=self._confirm_bulk_actions,
                id="confirm-bulk-actions",
            )

            yield Button("Close  [dim]esc[/]", id="close", variant="primary")

    def on_radio_set_changed(self, event: RadioSet.Changed) -> None:
        radio_set_id = event.radio_set.id
        label = str(event.pressed.label)
        app = self.app  # type: ignore[attr-defined]

        if radio_set_id == "theme-picker":
            app.theme = label
        elif radio_set_id == "interval-picker":
            app.set_refresh_interval(float(label.removesuffix("s")))

        config.save(app.theme, app.interval)

    def on_checkbox_changed(self, event: Checkbox.Changed) -> None:
        app = self.app  # type: ignore[attr-defined]
        checkbox_id = event.checkbox.id
        value = bool(event.checkbox.value)
        if checkbox_id == "expert-mode":
            app.expert_mode = value
            config.update({"ui": {"expert_mode": value}})
        elif checkbox_id == "confirm-cancel-single":
            app.confirm_cancel_single = value
            config.update({"safety": {"confirm_cancel_single": value}})
        elif checkbox_id == "confirm-bulk-actions":
            app.confirm_bulk_actions = value
            config.update({"safety": {"confirm_bulk_actions": value}})

    def on_button_pressed(self, event: Button.Pressed) -> None:
        self.dismiss()

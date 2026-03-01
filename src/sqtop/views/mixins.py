"""Shared mixins for Textual modal screens."""
from __future__ import annotations

from textual.binding import Binding
from textual.widgets import Button


class ModalButtonNavMixin:
    """Cyclic up/down keyboard navigation between buttons in a modal screen."""

    BINDINGS = [
        Binding("up", "focus_previous", show=False),
        Binding("down", "focus_next", show=False),
    ]

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

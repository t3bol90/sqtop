"""Shared widgets used across sqtop views."""
from __future__ import annotations

from textual.widgets import DataTable


class CyclicDataTable(DataTable):
    """DataTable whose cursor wraps from last row to first and vice versa."""

    def action_cursor_up(self) -> None:
        if self.row_count and self.cursor_row == 0:
            self.move_cursor(row=self.row_count - 1)
        else:
            super().action_cursor_up()

    def action_cursor_down(self) -> None:
        if self.row_count and self.cursor_row >= self.row_count - 1:
            self.move_cursor(row=0)
        else:
            super().action_cursor_down()

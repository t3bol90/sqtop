"""Shared mixins for Textual modal screens and views."""
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


class VisualSelectMixin:
    """Vim-like visual row-selection mode for data-table views.

    Provides enter/exit/yank actions and movement-key overrides that extend
    the selection range while visual mode is active.

    Subclasses (or their mix-in companions) must implement:
        _current_items() -> list  -- returns the currently visible filtered list
        _row_tsv(item) -> str     -- returns one TSV line for an item (no newline)
        _render_rows(items)       -- redraws the table (already present on views)
        query_one(CyclicDataTable) -- standard Textual query

    The mixin stores three pieces of state:
        _visual_active  : bool         -- whether visual mode is on
        _visual_anchor  : int | None   -- the row index where v was pressed
        _visual_cursor  : int | None   -- the row index of the moving end
    """

    # Subclasses must include these in their own BINDINGS list:
    #   Binding("v", "visual_enter", "Visual", show=False)
    #   Binding("V", "visual_enter", "Visual", show=False)
    #   Binding("escape", "visual_exit", "Exit visual", show=False)
    #   Binding("y", "yank", "Yank", show=False)

    def __init_visual__(self) -> None:
        """Call from __init__ to set up visual state. Alternatively the attrs
        are created lazily, but calling this makes the intent explicit."""
        self._visual_active: bool = False
        self._visual_anchor: int | None = None
        self._visual_cursor: int | None = None

    # ── Properties with lazy init ─────────────────────────────────────────────

    @property
    def _visual_active(self) -> bool:
        return getattr(self, "_visual_active_", False)

    @_visual_active.setter
    def _visual_active(self, value: bool) -> None:
        self._visual_active_ = value

    @property
    def _visual_anchor(self) -> int | None:
        return getattr(self, "_visual_anchor_", None)

    @_visual_anchor.setter
    def _visual_anchor(self, value: int | None) -> None:
        self._visual_anchor_ = value

    @property
    def _visual_cursor(self) -> int | None:
        return getattr(self, "_visual_cursor_", None)

    @_visual_cursor.setter
    def _visual_cursor(self, value: int | None) -> None:
        self._visual_cursor_ = value

    # ── Visual range helpers ──────────────────────────────────────────────────

    def _visual_range(self) -> tuple[int, int] | None:
        """Return (start, end) inclusive row indices, or None if not active."""
        if not self._visual_active:
            return None
        anchor = self._visual_anchor
        cursor = self._visual_cursor
        if anchor is None:
            return None
        if cursor is None:
            cursor = anchor
        return (min(anchor, cursor), max(anchor, cursor))

    def visual_rows(self) -> set[int]:
        """Return the set of row indices currently in the visual selection."""
        r = self._visual_range()
        if r is None:
            return set()
        return set(range(r[0], r[1] + 1))

    # ── Actions ───────────────────────────────────────────────────────────────

    def action_visual_enter(self) -> None:
        """Enter visual mode anchored at the current cursor row."""
        from .widgets import CyclicDataTable
        table = self.query_one(CyclicDataTable)
        row = table.cursor_row
        if row < 0:
            return
        self._visual_active = True
        self._visual_anchor = row
        self._visual_cursor = row
        self.app.sub_title = "-- VISUAL --"
        self._render_rows(self._current_items())

    def action_visual_exit(self) -> None:
        """Exit visual mode without copying, restoring prior state."""
        if not self._visual_active:
            return
        self._visual_active = False
        self._visual_anchor = None
        self._visual_cursor = None
        # Restore app sub_title only if we set it
        try:
            if self.app.sub_title == "-- VISUAL --":
                self.app.sub_title = ""
        except Exception:
            pass
        self._render_rows(self._current_items())

    def action_visual_yank(self) -> None:
        """Yank the visual selection to clipboard, then exit visual mode."""
        if not self._visual_active:
            return
        r = self._visual_range()
        if r is None:
            self.action_visual_exit()
            return
        start, end = r[0], r[1] + 1  # end is exclusive
        text, count = self._visual_yank_payload(start, end)
        from ..clipboard import app_copy
        app_copy(self.app, text, label=f"Copied {count} rows", count=None)
        self.action_visual_exit()

    def _visual_yank_payload(self, start: int, end: int) -> tuple[str, int]:
        """Return (tsv_text, row_count) for rows [start:end].

        Default implementation uses _current_items() and _row_tsv(item).
        Subclasses may override for custom column projections.
        """
        items = self._current_items()
        selected = items[start:end]
        lines = [self._row_tsv(item) for item in selected]
        text = "\n".join(lines) + "\n" if lines else "\n"
        return text, len(selected)

    # ── Movement overrides that extend the visual range ───────────────────────

    def _move_visual_cursor(self, delta: int | None = None, absolute: int | None = None) -> None:
        """Move the visual cursor by delta rows (or to absolute row), updating range."""
        from .widgets import CyclicDataTable
        table = self.query_one(CyclicDataTable)
        row_count = table.row_count
        if row_count == 0:
            return

        if absolute is not None:
            new_cursor = max(0, min(absolute, row_count - 1))
        elif delta is not None:
            current = self._visual_cursor if self._visual_cursor is not None else table.cursor_row
            new_cursor = max(0, min(current + delta, row_count - 1))
        else:
            return

        self._visual_cursor = new_cursor
        table.move_cursor(row=new_cursor)
        self._render_rows(self._current_items())

    def action_cursor_up(self) -> None:
        if self._visual_active:
            self._move_visual_cursor(delta=-1)
        else:
            from .widgets import CyclicDataTable
            table = self.query_one(CyclicDataTable)
            table.action_cursor_up()

    def action_cursor_down(self) -> None:
        if self._visual_active:
            self._move_visual_cursor(delta=1)
        else:
            from .widgets import CyclicDataTable
            table = self.query_one(CyclicDataTable)
            table.action_cursor_down()

    def action_scroll_cursor_up(self) -> None:
        if self._visual_active:
            self._move_visual_cursor(delta=-1)
        else:
            from .widgets import CyclicDataTable
            super().action_scroll_cursor_up()  # type: ignore[misc]

    def action_scroll_cursor_down(self) -> None:
        if self._visual_active:
            self._move_visual_cursor(delta=1)
        else:
            super().action_scroll_cursor_down()  # type: ignore[misc]

    def action_visual_top(self) -> None:
        """Extend visual selection to top of table."""
        if self._visual_active:
            self._move_visual_cursor(absolute=0)

    def action_visual_bottom(self) -> None:
        """Extend visual selection to bottom of table."""
        if self._visual_active:
            from .widgets import CyclicDataTable
            table = self.query_one(CyclicDataTable)
            self._move_visual_cursor(absolute=table.row_count - 1)

    # ── Protocol methods (subclasses must provide) ────────────────────────────

    def _current_items(self) -> list:
        """Return the current visible filtered list of items. Override in subclasses."""
        raise NotImplementedError("VisualSelectMixin: subclass must implement _current_items()")

    def _row_tsv(self, item) -> str:
        """Return one TSV line for an item (no newline). Override in subclasses."""
        raise NotImplementedError("VisualSelectMixin: subclass must implement _row_tsv(item)")

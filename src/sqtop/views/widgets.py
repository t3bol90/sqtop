"""Shared widgets used across sqtop views."""
from __future__ import annotations

from rich.segment import Segment
from rich.style import Style as RichStyle
from textual import events
from textual.message import Message
from textual.strip import Strip
from textual.widgets import DataTable

# Minimum horizontal displacement (in cells) to trigger a drag vs a click.
DRAG_THRESHOLD_CELLS = 2


class CyclicDataTable(DataTable):
    """DataTable whose cursor wraps from last row to first and vice versa.

    Also supports mouse drag to reorder columns.  When the user presses the
    mouse button on the header row, moves at least DRAG_THRESHOLD_CELLS cells
    horizontally, and releases, a :class:`ColumnReordered` message is posted.
    Pressing Escape while dragging cancels without posting.
    """

    # ------------------------------------------------------------------
    # Nested message
    # ------------------------------------------------------------------

    class ColumnReordered(Message):
        """Posted when the user drags a column header to a new position.

        Handle with ``on_cyclic_data_table_column_reordered`` in a parent widget.
        ``from_index`` and ``to_index`` are both positions within the currently
        visible column set (0-based).  ``to_index`` is the insertion point, so
        it may equal ``num_visible_columns`` when dropping after the last column.
        """

        def __init__(self, from_index: int, to_index: int) -> None:
            self.from_index = from_index
            """The column index the drag started from."""
            self.to_index = to_index
            """The insertion index the column was dropped at."""
            super().__init__()

    # ------------------------------------------------------------------
    # Cyclic cursor overrides
    # ------------------------------------------------------------------

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

    # ------------------------------------------------------------------
    # Drag state
    # ------------------------------------------------------------------

    def __init__(self, *args, **kwargs) -> None:
        super().__init__(*args, **kwargs)
        self._drag_col_index: int | None = None  # column being dragged
        self._drag_press_x: int = 0              # widget-local x at mouse-down
        self._drag_press_y: int = 0              # widget-local y at mouse-down
        self._dragging: bool = False             # threshold crossed yet?
        self._drag_marker_x: int | None = None  # x of insertion marker (widget-local)

    # ------------------------------------------------------------------
    # Column boundary helpers
    # ------------------------------------------------------------------

    def _column_boundaries(self) -> list[int]:
        """Return a list of x positions for each column boundary.

        The list has *num_columns + 1* entries: boundary[0] is the left edge
        of the first column (after row-label column), boundary[n] is the right
        edge of the last column.  All values are in widget-local coordinates
        (i.e. assuming scroll_x == 0).
        """
        left_offset = self._row_label_column_width
        boundaries: list[int] = [left_offset]
        x = left_offset
        for col in self.ordered_columns:
            x += col.get_render_width(self)
            boundaries.append(x)
        return boundaries

    def _boundary_to_col_index(self, x: int) -> int:
        """Return the insertion index closest to widget-local x.

        Clamps to ``[0, num_visible_columns]``.
        """
        boundaries = self._column_boundaries()
        if not boundaries:
            return 0
        n = len(boundaries)
        # Find the closest boundary
        best_idx = 0
        best_dist = abs(x - boundaries[0])
        for i in range(1, n):
            d = abs(x - boundaries[i])
            if d < best_dist:
                best_dist = d
                best_idx = i
        # Clamp to valid insertion range [0, num_cols]
        return max(0, min(best_idx, len(self.ordered_columns)))

    def _col_index_from_x(self, x: int) -> int:
        """Return which column (0-based) is under widget-local x."""
        left_offset = self._row_label_column_width
        if x < left_offset:
            return 0
        pos = left_offset
        for i, col in enumerate(self.ordered_columns):
            pos += col.get_render_width(self)
            if x < pos:
                return i
        return max(0, len(self.ordered_columns) - 1)

    # ------------------------------------------------------------------
    # Mouse handlers
    # ------------------------------------------------------------------

    def on_mouse_down(self, event: events.MouseDown) -> None:
        """Record drag start if the press is on the header row."""
        if not self.show_header:
            return
        # The header row occupies y in [0, header_height).
        if event.y < self.header_height:
            col_index = self._col_index_from_x(event.x)
            self._drag_col_index = col_index
            self._drag_press_x = event.x
            self._drag_press_y = event.y
            self._dragging = False
            self._drag_marker_x = None

    def on_mouse_move(self, event: events.MouseMove) -> None:
        """Activate drag mode once threshold is crossed; update insertion marker."""
        if self._drag_col_index is None:
            return
        delta = abs(event.x - self._drag_press_x)
        if delta >= DRAG_THRESHOLD_CELLS:
            self._dragging = True
            new_marker = self._nearest_boundary_x(event.x)
            if new_marker != self._drag_marker_x:
                self._drag_marker_x = new_marker
                self.refresh()

    def _nearest_boundary_x(self, x: int) -> int:
        """Return the x coordinate of the column boundary nearest to x."""
        boundaries = self._column_boundaries()
        if not boundaries:
            return 0
        best = boundaries[0]
        best_dist = abs(x - best)
        for b in boundaries[1:]:
            d = abs(x - b)
            if d < best_dist:
                best_dist = d
                best = b
        return best

    def on_mouse_up(self, event: events.MouseUp) -> None:
        """Complete or cancel the drag on mouse release."""
        if self._drag_col_index is None:
            return
        if self._dragging:
            to_index = self._boundary_to_col_index(event.x)
            from_index = self._drag_col_index
            self._reset_drag_state()
            self.post_message(self.ColumnReordered(from_index, to_index))
        else:
            # Motion below threshold — reset state; let parent DataTable handle it.
            self._reset_drag_state()

    def on_key(self, event: events.Key) -> None:
        """Cancel an in-progress drag when Escape is pressed."""
        if event.key == "escape" and self._dragging:
            self._reset_drag_state()
            event.prevent_default()

    def _reset_drag_state(self) -> None:
        """Clear all drag state and remove the marker."""
        had_marker = self._drag_marker_x is not None
        self._drag_col_index = None
        self._drag_press_x = 0
        self._drag_press_y = 0
        self._dragging = False
        self._drag_marker_x = None
        if had_marker:
            self.refresh()

    # ------------------------------------------------------------------
    # Insertion marker rendering
    # ------------------------------------------------------------------

    def render_line(self, y: int) -> Strip:
        """Render a single horizontal strip, injecting the drag marker if active."""
        strip = super().render_line(y)
        if self._drag_marker_x is None or not self._dragging:
            return strip
        # Only render the marker on visible rows (all rows, full height).
        marker_x = self._drag_marker_x
        width = self.size.width
        if marker_x < 0 or marker_x >= width:
            return strip
        # Build a new strip with the marker inserted at marker_x.
        marker_style = RichStyle.parse("bold bright_yellow")
        left = strip.crop(0, marker_x)
        right_start = min(marker_x + 1, strip.cell_length)
        right = strip.crop_extend(right_start, strip.cell_length, None)
        marker_seg = Strip([Segment("▌", marker_style)])
        return Strip.join([left, marker_seg, right])

from __future__ import annotations

from textual.binding import Binding

from sqtop.views.keybindings_help import format_bindings
from sqtop.views.base import BaseDataTableView


def test_format_bindings_pretty_keys_and_desc():
    rows = format_bindings(
        [
            Binding("question_mark", "show_keybindings", "Keybindings"),
            Binding("ctrl+p", "command_palette", "Commands"),
            Binding("Y", "yank_row", show=False),
            Binding("slash", "activate_search", "Search"),
        ]
    )
    assert rows[0] == ("?", "Keybindings")
    assert rows[1] == ("Ctrl+P", "Commands")
    assert rows[2] == ("Shift+Y", "yank row")
    assert rows[3] == ("/", "Search")


class _DummyView(BaseDataTableView[int]):
    def _fetch_data(self) -> list[int]:
        return []

    def _get_anchor_key(self, item: int) -> str:
        return str(item)

    def _update_table(self, data: list[int]) -> None:
        return None


def test_start_refresh_loop_uses_small_default_delay():
    view = _DummyView()
    calls: list[tuple[float, object]] = []
    view.set_timer = lambda delay, callback: calls.append((delay, callback))  # type: ignore[method-assign]
    view.start_refresh_loop()
    assert calls
    assert calls[0][0] == 0.05

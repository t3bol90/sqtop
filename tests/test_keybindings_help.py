from __future__ import annotations

from textual.binding import Binding

from sqtop.views.keybindings_help import format_bindings


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

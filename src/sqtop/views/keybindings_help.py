"""Modal screen to show keybindings for the active pane."""
from __future__ import annotations

from textual.app import ComposeResult
from textual.binding import Binding
from textual.screen import ModalScreen
from textual.widgets import Label, RichLog, Static


_KEY_LABELS = {
    "enter": "Enter",
    "escape": "Esc",
    "space": "Space",
    "slash": "/",
    "asterisk": "*",
    "question_mark": "?",
}


def _pretty_key(key: str) -> str:
    if key in _KEY_LABELS:
        return _KEY_LABELS[key]
    if key.startswith("ctrl+"):
        return f"Ctrl+{key.split('+', 1)[1].upper()}"
    if len(key) == 1 and key.isalpha():
        return key if key.islower() else f"Shift+{key}"
    return key


def _binding_desc(binding: Binding) -> str:
    if binding.description:
        return str(binding.description)
    action = str(binding.action)
    if action.startswith("switch_tab("):
        return "Switch tab"
    return action.replace("_", " ")


def format_bindings(bindings: list[Binding]) -> list[tuple[str, str]]:
    return [(_pretty_key(str(b.key)), _binding_desc(b)) for b in bindings]


class KeybindingHelpScreen(ModalScreen[None]):
    """Show global + current-pane keybindings."""

    BINDINGS = [
        Binding("escape", "dismiss(None)", show=False),
        Binding("q", "dismiss(None)", show=False),
        Binding("question_mark", "dismiss(None)", show=False),
    ]

    CSS = """
    KeybindingHelpScreen { align: center middle; }
    #keys-dialog {
        width: 74; height: auto;
        border: double $primary;
        background: $surface;
        padding: 1 2;
    }
    #keys-title { text-style: bold; color: $primary; margin-bottom: 1; }
    #keys-output { height: auto; max-height: 24; }
    """

    def __init__(
        self,
        pane_name: str,
        global_bindings: list[Binding],
        pane_bindings: list[Binding],
    ) -> None:
        super().__init__()
        self._pane_name = pane_name
        self._global_bindings = global_bindings
        self._pane_bindings = pane_bindings

    def compose(self) -> ComposeResult:
        with Static(id="keys-dialog"):
            yield Label(f"Keybindings — {self._pane_name} pane", id="keys-title")
            yield RichLog(id="keys-output", highlight=True, markup=True)

    def on_mount(self) -> None:
        log = self.query_one("#keys-output", RichLog)
        log.write("[b]Global[/b]")
        for key, desc in format_bindings(self._global_bindings):
            log.write(f"  [cyan]{key:<12}[/] {desc}")
        log.write("")
        log.write(f"[b]{self._pane_name}[/b]")
        pane_rows = format_bindings(self._pane_bindings)
        if pane_rows:
            for key, desc in pane_rows:
                log.write(f"  [cyan]{key:<12}[/] {desc}")
        else:
            log.write("  [dim]Pane keybindings are still loading...[/]")
        log.write("")
        log.write("[dim]Press Esc, q, or ? to close[/]")

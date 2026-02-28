"""Job actions modal — inspect stdout/stderr logs, view details, cancel."""
from __future__ import annotations

from textual.app import ComposeResult
from textual.binding import Binding
from textual.screen import ModalScreen
from textual.widgets import Button, Label, Static

from ..slurm import Job


class JobActionScreen(ModalScreen[str | None]):
    """Show job summary + actions."""

    BINDINGS = [
        Binding("escape", "dismiss(None)", show=False),
        Binding("up", "focus_previous", show=False),
        Binding("down", "focus_next", show=False),
    ]

    CSS = """
    JobActionScreen { align: center middle; }
    #dialog {
        width: 50; height: auto;
        border: double $primary;
        background: $surface;
        padding: 1 2;
    }
    #dialog .section-title { text-style: bold; color: $primary; margin-top: 1; }
    #btn-attach-first, #btn-attach-custom,
    #btn-stdout, #btn-stderr, #btn-detail, #btn-cancel { width: 100%; margin-top: 1; }
    #btn-close { width: 100%; margin-top: 1; }
    """

    def __init__(self, job: Job) -> None:
        super().__init__()
        self._job = job

    def compose(self) -> ComposeResult:
        can_attach = self._job.state == "RUNNING"
        with Static(id="dialog"):
            yield Label(f"Job {self._job.job_id} — {self._job.name}", id="title")
            yield Label(f"State: {self._job.state}  User: {self._job.user}", classes="section-title")
            yield Button(
                "Attach shell (first node)",
                id="btn-attach-first",
                variant="primary",
                disabled=not can_attach,
            )
            yield Button(
                "Attach with node override...",
                id="btn-attach-custom",
                variant="default",
                disabled=not can_attach,
            )
            yield Button("View stdout log", id="btn-stdout", variant="primary")
            yield Button("View stderr log", id="btn-stderr", variant="default")
            yield Button("Show details", id="btn-detail", variant="default")
            yield Button("Cancel job [dim]scancel[/]", id="btn-cancel", variant="error")
            yield Button("Close  [dim]esc[/]", id="btn-close", variant="default")

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

    def on_button_pressed(self, event: Button.Pressed) -> None:
        if event.button.id == "btn-attach-first":
            self.dismiss("attach_first")
        elif event.button.id == "btn-attach-custom":
            self.dismiss("attach_custom")
        elif event.button.id == "btn-stdout":
            self.dismiss("stdout")
        elif event.button.id == "btn-stderr":
            self.dismiss("stderr")
        elif event.button.id == "btn-detail":
            self.dismiss("detail")
        elif event.button.id == "btn-cancel":
            self.dismiss("cancel")
        else:
            self.dismiss(None)

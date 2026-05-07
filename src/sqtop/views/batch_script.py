"""Batch script viewer modal."""
from __future__ import annotations

from textual.app import ComposeResult
from textual.binding import Binding
from textual.screen import ModalScreen
from textual.containers import Vertical
from textual.widgets import Label, TextArea
from textual import work

from ..slurm import fetch_batch_script
from ..clipboard import app_copy
from ..responsive import Tier


class BatchScriptScreen(ModalScreen[None]):
    BINDINGS = [
        Binding("escape", "dismiss", show=False),
        Binding("q", "dismiss", show=False),
        Binding("y", "copy_selection_or_all", show=False),
        Binding("ctrl+c", "copy_selection_or_all", show=False),
        Binding("v", "noop", show=False),
    ]
    CSS = """
    BatchScriptScreen { align: center middle; }
    #batch-dialog {
        width: 90%; height: 85%;
        min-width: 60; max-width: 140;
        min-height: 20; max-height: 50;
        border: double $primary;
        background: $surface;
        padding: 0 1;
    }
    BatchScriptScreen.clamp-xs #batch-dialog {
        width: 100%; height: 100%;
        min-width: 0; min-height: 0;
    }
    #batch-header { height: 1; background: $panel; padding: 0 1; }
    #batch-output { height: 1fr; }
    """

    def __init__(self, job_id: str) -> None:
        super().__init__()
        self._job_id = job_id
        self._script = ""

    def responsive_clamp(self, tier: Tier) -> None:
        self.add_class(f"clamp-{tier}")

    def compose(self) -> ComposeResult:
        with Vertical(id="batch-dialog"):
            yield Label(
                f"[b]batch script[/b]  job {self._job_id}  [dim]esc=close[/]",
                id="batch-header",
            )
            yield TextArea("", id="batch-output", read_only=True)

    def on_mount(self) -> None:
        self.call_after_refresh(self.fetch_script)

    @work(thread=True)
    def fetch_script(self) -> None:
        content = fetch_batch_script(self._job_id)
        self.app.call_from_thread(self._display, content)

    def _display(self, content: str) -> None:
        self._script = content
        self.query_one("#batch-output", TextArea).load_text(content)

    def action_copy_selection_or_all(self) -> None:
        ta = self.query_one(TextArea)
        text = ta.selected_text or ta.text
        app_copy(self.app, text, label="BatchScript", count=len(text.splitlines()))

    def action_noop(self) -> None:
        pass

    def copy_pane(self) -> tuple[str, str, int]:
        """Return (label, payload, line_count) for ctrl+shift+y."""
        text = self._script
        label = f"Batch Script job {self._job_id}"
        return label, text, len(text.splitlines())

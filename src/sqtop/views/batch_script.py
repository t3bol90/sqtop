"""Batch script viewer modal."""
from __future__ import annotations

from textual.app import ComposeResult
from textual.binding import Binding
from textual.screen import ModalScreen
from textual.containers import Vertical
from textual.widgets import Label, RichLog
from textual import work

from ..slurm import fetch_batch_script


class BatchScriptScreen(ModalScreen[None]):
    BINDINGS = [
        Binding("escape", "dismiss", show=False),
        Binding("q", "dismiss", show=False),
    ]
    CSS = """
    BatchScriptScreen { align: center middle; }
    #batch-dialog {
        width: 90%; height: 80%;
        border: double $primary;
        background: $surface;
        padding: 0 1;
    }
    #batch-header { height: 1; background: $panel; padding: 0 1; }
    #batch-output { height: 1fr; }
    """

    def __init__(self, job_id: str) -> None:
        super().__init__()
        self._job_id = job_id
        self._script = ""

    def compose(self) -> ComposeResult:
        with Vertical(id="batch-dialog"):
            yield Label(
                f"[b]batch script[/b]  job {self._job_id}  [dim]esc=close[/]",
                id="batch-header",
            )
            yield RichLog(id="batch-output", highlight=True, markup=False, wrap=False)

    def on_mount(self) -> None:
        self.call_after_refresh(self.fetch_script)

    @work(thread=True)
    def fetch_script(self) -> None:
        content = fetch_batch_script(self._job_id)
        self.app.call_from_thread(self._display, content)

    def _display(self, content: str) -> None:
        self._script = content
        self.call_after_refresh(self._write)

    def _write(self) -> None:
        log = self.query_one("#batch-output", RichLog)
        log.clear()
        log.write(self._script)
        self.refresh(layout=True)

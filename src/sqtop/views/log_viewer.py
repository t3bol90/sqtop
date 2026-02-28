"""Log viewer modal — auto-tailing log file display."""
from __future__ import annotations

from textual.app import ComposeResult
from textual.binding import Binding
from textual.screen import ModalScreen
from textual.widgets import Label, RichLog, Static
from textual import work

from ..slurm import tail_log_file


class LogViewerScreen(ModalScreen[None]):
    """Full-screen log viewer with 2s auto-refresh."""

    BINDINGS = [
        Binding("escape", "dismiss", show=False),
        Binding("q", "dismiss", show=False),
        Binding("f", "toggle_follow", "Follow"),
    ]

    CSS = """
    LogViewerScreen { align: center middle; }
    #log-dialog {
        width: 90%; height: 80%;
        border: double $primary;
        background: $surface;
        padding: 0 1;
    }
    #log-header { height: 1; background: $panel; padding: 0 1; }
    #log-output { height: 1fr; }
    """

    def __init__(self, job_id: str, log_path: str, log_type: str) -> None:
        super().__init__()
        self._job_id = job_id
        self._log_path = log_path
        self._log_type = log_type  # "stdout" or "stderr"
        self._follow = True
        self._timer = None

    def compose(self) -> ComposeResult:
        with Static(id="log-dialog"):
            yield Label("", id="log-header")
            yield RichLog(id="log-output", highlight=True, markup=False, wrap=True)

    def on_mount(self) -> None:
        self._update_header()
        self.fetch_log()
        self._timer = self.set_interval(2.0, self.fetch_log)

    def _update_header(self) -> None:
        follow_status = "[green]following[/]" if self._follow else "[dim]paused[/]"
        self.query_one("#log-header", Label).update(
            f"[b]{self._log_type}[/b]  {self._log_path}  {follow_status}  [dim]esc=close  f=toggle follow[/]"
        )

    def action_toggle_follow(self) -> None:
        self._follow = not self._follow
        self._update_header()

    @work(thread=True)
    def fetch_log(self) -> None:
        content = tail_log_file(self._log_path)
        self.app.call_from_thread(self._render_log, content)

    def _render_log(self, content: str) -> None:
        if not self._follow:
            return
        log = self.query_one("#log-output", RichLog)
        log.clear()
        log.write(content)

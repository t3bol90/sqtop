"""Job detail modal — shows scontrol show job output."""
from __future__ import annotations

from textual.app import ComposeResult
from textual.binding import Binding
from textual.screen import ModalScreen
from textual.containers import ScrollableContainer
from textual.widgets import Label, Static

from .detail import DetailView


class JobDetailScreen(ModalScreen[None]):
    """Modal that displays pre-fetched job detail via DetailView."""

    BINDINGS = [
        Binding("escape", "dismiss(None)", show=False),
        Binding("q", "dismiss(None)", "Close", show=True),
    ]

    CSS = """
    JobDetailScreen { align: center middle; }
    #job-detail-dialog {
        width: 90%; height: 80%;
        border: double $primary;
        background: $surface;
        padding: 0;
    }
    #job-detail-title {
        text-style: bold;
        padding: 0 2;
        margin-bottom: 1;
        background: $primary;
        color: $background;
        width: 100%;
    }
    #job-detail-scroll {
        height: 1fr;
        padding: 0 2;
    }
    """

    def __init__(self, job_id: str, data: dict[str, str]) -> None:
        super().__init__()
        self._job_id = job_id
        self._data = data

    def compose(self) -> ComposeResult:
        name = self._data.get("JobName", "")
        header = f"Job {self._job_id}" + (f" — {name}" if name else "")
        with Static(id="job-detail-dialog"):
            yield Label(header, id="job-detail-title")
            with ScrollableContainer(id="job-detail-scroll"):
                yield DetailView(id="job-detail-view")

    def on_mount(self) -> None:
        self.query_one("#job-detail-view", DetailView).show_job(self._data)

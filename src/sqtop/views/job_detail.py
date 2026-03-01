"""Job detail modal — shows scontrol show job output."""
from __future__ import annotations

from textual import work
from textual.app import ComposeResult
from textual.binding import Binding
from textual.screen import ModalScreen
from textual.containers import ScrollableContainer
from textual.widgets import Label, Static

from .detail import DetailView
from ..slurm import fetch_job_efficiency

_TERMINAL_STATES = {"COMPLETED", "FAILED", "CANCELLED", "TIMEOUT", "NODE_FAIL", "OUT_OF_MEMORY"}
_BAR_WIDTH = 10


def _eff_bar(fraction: float, bar_width: int = _BAR_WIDTH) -> str:
    """Return a Rich-markup efficiency bar string like '[green]██████░░░░[/] 62%'."""
    pct = round(min(max(fraction, 0.0), 1.0) * 100)
    filled = round(pct / 100 * bar_width)
    bar = "█" * filled + "░" * (bar_width - filled)
    color = "green" if pct >= 70 else ("yellow" if pct >= 40 else "red")
    return f"[{color}]{bar}[/]  {pct:3}%"


def _build_efficiency_text(eff: dict) -> str:
    """Build a two-line Rich markup string for CPU and memory efficiency."""
    cpu_bar = _eff_bar(eff["cpu_eff"])
    mem_bar = _eff_bar(eff["mem_eff"])
    cpu_line = (
        f"  [bold]CPU efficiency:[/]  {cpu_bar}"
        f"   [dim](used {eff['cpu_used_str']} of {eff['cpu_alloc_str']} CPU-time)[/]"
    )
    mem_line = (
        f"  [bold]Mem efficiency:[/]  {mem_bar}"
        f"   [dim](peak {eff['mem_peak_mb']} MB of {eff['mem_alloc_mb']} MB allocated)[/]"
    )
    return cpu_line + "\n" + mem_line


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
    #job-detail-efficiency {
        padding: 1 2 0 2;
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
            yield Static("", id="job-detail-efficiency")
            with ScrollableContainer(id="job-detail-scroll"):
                yield DetailView(id="job-detail-view")

    def on_mount(self) -> None:
        self.call_after_refresh(self._show_content)

    def _show_content(self) -> None:
        self.query_one("#job-detail-view", DetailView).show_job(self._data)
        state = self._data.get("JobState", "").upper()
        if state in _TERMINAL_STATES:
            self._load_efficiency()
        else:
            self.query_one("#job-detail-efficiency", Static).display = False

    @work(thread=True)
    def _load_efficiency(self) -> None:
        """Fetch and display efficiency bars for terminal-state jobs (background thread)."""
        eff = fetch_job_efficiency(self._job_id)
        if not eff.get("available") or (eff["mem_peak_mb"] == 0 and eff["cpu_eff"] == 0.0):
            self.app.call_from_thread(self._hide_efficiency)
            return
        text = _build_efficiency_text(eff)
        self.app.call_from_thread(self.query_one("#job-detail-efficiency", Static).update, text)

    def _hide_efficiency(self) -> None:
        self.query_one("#job-detail-efficiency", Static).display = False

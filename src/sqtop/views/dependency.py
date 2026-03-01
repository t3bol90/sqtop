"""Job dependency graph modal."""
from __future__ import annotations

from textual import work
from textual.app import ComposeResult
from textual.binding import Binding
from textual.screen import ModalScreen
from textual.widgets import Label, RichLog, Static

from ..slurm import Job, JobDependency, fetch_job_dependencies

_STATE_COLORS = {
    "RUNNING":   "green",
    "PENDING":   "yellow",
    "FAILED":    "red",
    "CANCELLED": "red",
    "COMPLETED": "dim",
}


def _state_color(state: str) -> str:
    return _STATE_COLORS.get(state.upper(), "white")


def _fulfilled_icon(state: str) -> str:
    upper = state.upper()
    if upper in {"COMPLETED"} or not upper:
        return "[green]✓[/]"
    if upper in {"FAILED", "CANCELLED"}:
        return "[red]✗[/]"
    return "[yellow]…[/]"


class JobDependencyScreen(ModalScreen[None]):
    """Shows the immediate dependency graph for a job."""

    BINDINGS = [
        Binding("escape", "dismiss(None)", show=False),
        Binding("q", "dismiss(None)", show=False),
        Binding("D", "dismiss(None)", show=False),
    ]

    CSS = """
    JobDependencyScreen { align: center middle; }
    #dep-dialog {
        width: 60; height: auto;
        border: double $primary;
        background: $surface;
        padding: 1 2;
    }
    #dep-title { text-style: bold; color: $primary; margin-bottom: 1; }
    #dep-output { height: auto; max-height: 20; }
    """

    def __init__(self, job: Job) -> None:
        super().__init__()
        self._job = job

    def compose(self) -> ComposeResult:
        with Static(id="dep-dialog"):
            yield Label(f"Dependencies — Job {self._job.job_id} ({self._job.name})", id="dep-title")
            yield RichLog(id="dep-output", highlight=True, markup=True)

    def on_mount(self) -> None:
        self.query_one("#dep-output", RichLog).write("[dim]Loading…[/]")
        self.fetch_deps()

    @work(thread=True)
    def fetch_deps(self) -> None:
        deps = fetch_job_dependencies(self._job.job_id)
        self.app.call_from_thread(self._render_deps, deps)

    def _render_deps(self, deps: list[JobDependency]) -> None:
        log = self.query_one("#dep-output", RichLog)
        log.clear()
        color = "green" if self._job.state == "RUNNING" else "yellow"
        log.write(f"[bold][{color}]{self._job.job_id}[/] {self._job.name}  [{color}]{self._job.state}[/][/bold]")
        if not deps:
            log.write("[dim]  (no dependencies)[/]")
            return
        for dep in deps:
            sc = _state_color(dep.state)
            icon = _fulfilled_icon(dep.state)
            log.write(
                f"  {icon} [{sc}]{dep.job_id}[/]  [dim]{dep.dep_type}[/]  [{sc}]{dep.state or 'COMPLETED'}[/]"
            )

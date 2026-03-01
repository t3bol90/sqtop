"""Array task expansion modal — shows individual tasks of a job array."""
from __future__ import annotations

from textual.app import ComposeResult
from textual.binding import Binding
from textual.screen import ModalScreen
from textual.widgets import Label, Static
from textual.worker import Worker, WorkerState

from ..slurm import Job, fetch_array_tasks
from .widgets import CyclicDataTable

STATE_COLORS = {
    "RUNNING":   "green",
    "PENDING":   "yellow",
    "FAILED":    "red",
    "CANCELLED": "red",
    "COMPLETED": "dim",
    "TIMEOUT":   "magenta",
    "NODE_FAIL": "red",
    "PREEMPTED": "yellow",
}


class ArrayTaskScreen(ModalScreen[None]):
    """Modal that lists individual tasks of a job array."""

    BINDINGS = [
        Binding("escape", "dismiss(None)", show=False),
        Binding("q", "dismiss(None)", "Close", show=True),
    ]

    CSS = """
    ArrayTaskScreen { align: center middle; }
    #array-task-dialog {
        width: 90%; height: 80%;
        border: double $primary;
        background: $surface;
        padding: 0;
    }
    #array-task-title {
        text-style: bold;
        padding: 0 2;
        background: $primary;
        color: $background;
        width: 100%;
    }
    #array-task-status {
        padding: 0 2;
        color: $text-muted;
        height: 1;
    }
    #array-task-table {
        height: 1fr;
    }
    """

    def __init__(self, job: Job) -> None:
        super().__init__()
        self._job = job

    def compose(self) -> ComposeResult:
        with Static(id="array-task-dialog"):
            yield Label(
                f"Array {self._job.job_id} — {self._job.name}",
                id="array-task-title",
            )
            yield Label("Loading…", id="array-task-status")
            yield CyclicDataTable(
                id="array-task-table",
                cursor_type="row",
                zebra_stripes=True,
            )

    def on_mount(self) -> None:
        table = self.query_one("#array-task-table", CyclicDataTable)
        table.add_column("TASK_ID", width=12)
        table.add_column("STATE", width=12)
        table.add_column("TIME", width=12)
        table.add_column("NODELIST", width=24)
        self.run_worker(self._load_tasks, thread=True)

    def _load_tasks(self) -> list[Job]:
        return fetch_array_tasks(self._job.job_id)

    def on_worker_state_changed(self, event: Worker.StateChanged) -> None:
        if event.state == WorkerState.SUCCESS:
            tasks: list[Job] = event.worker.result
            self.app.call_from_thread(self._render_tasks, tasks)

    def _render_tasks(self, tasks: list[Job]) -> None:
        table = self.query_one("#array-task-table", CyclicDataTable)
        table.clear()

        running = sum(1 for t in tasks if t.state == "RUNNING")
        pending = sum(1 for t in tasks if t.state == "PENDING")
        done = sum(
            1 for t in tasks
            if t.state in {"COMPLETED", "FAILED", "CANCELLED", "TIMEOUT", "NODE_FAIL", "PREEMPTED"}
        )
        self.query_one("#array-task-status", Label).update(
            f"[green]{running} running[/]  "
            f"[yellow]{pending} pending[/]  "
            f"[dim]{done} done  {len(tasks)} total[/]"
        )

        for task in tasks:
            # Extract the task suffix: "12345_3" → "3", "12345" → "12345"
            task_id = task.job_id
            if "_" in task_id:
                task_id = task_id.split("_", 1)[1]

            color = STATE_COLORS.get(task.state, "white")
            nodelist = task.nodelist if task.nodelist else task.reason
            table.add_row(
                f"[{color}]{task_id}[/]",
                f"[{color}]{task.state}[/]",
                task.time_used,
                nodelist,
            )

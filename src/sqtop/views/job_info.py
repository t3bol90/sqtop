"""Job info popup — shows rich job information for a selected job."""

from __future__ import annotations

from textual.app import ComposeResult
from textual.binding import Binding
from textual.screen import ModalScreen
from textual.containers import ScrollableContainer
from textual.widgets import Label, Static
from textual import work

from ..slurm import Job, fetch_job_detail, fetch_job_dependencies


class JobInfoScreen(ModalScreen[None]):
    """Modal that displays rich job information when pressing 'i' on a selected job."""

    BINDINGS = [
        Binding("escape", "dismiss(None)", show=False),
        Binding("q", "dismiss(None)", "Close", show=True),
    ]

    CSS = """
    JobInfoScreen { align: center middle; }
    #job-info-dialog {
        width: 80%; height: 85%;
        border: double $primary;
        background: $surface;
        padding: 0;
    }
    #job-info-title {
        text-style: bold;
        padding: 0 2;
        margin-bottom: 1;
        background: $primary;
        color: $background;
        width: 100%;
    }
    #job-info-scroll {
        height: 1fr;
        padding: 1 2;
    }
    #job-info-content {
        width: 100%;
    }
    """

    def __init__(self, job: Job) -> None:
        super().__init__()
        self._job = job

    def compose(self) -> ComposeResult:
        header = f"Job {self._job.job_id}"
        if self._job.name:
            header += f" — {self._job.name}"
        with Static(id="job-info-dialog"):
            yield Label(header, id="job-info-title")
            with ScrollableContainer(id="job-info-scroll"):
                yield Static("Loading...", id="job-info-content")

    def on_mount(self) -> None:
        self._load_job_info()

    @work(thread=True)
    def _load_job_info(self) -> None:
        job = self._job
        detail = fetch_job_detail(job.job_id)

        # Fetch dependencies if reason starts with "Dependency"
        deps = []
        if job.reason.startswith("Dependency") or detail.get("Reason", "").startswith("Dependency"):
            deps = fetch_job_dependencies(job.job_id)

        markup = self._build_markup(job, detail, deps)
        self.app.call_from_thread(self._update_content, markup)

    def _build_markup(self, job: Job, detail: dict[str, str], deps: list) -> str:
        lines: list[str] = []

        # Identity section
        lines.append("[bold cyan]── Identity ──────────────────────────────[/bold cyan]")
        lines.append(f"  [bold]Job ID:[/bold]     {job.job_id}")
        lines.append(f"  [bold]Name:[/bold]       {job.name or '(none)'}")
        lines.append(f"  [bold]User:[/bold]       {job.user}")
        lines.append(f"  [bold]Partition:[/bold]  {job.partition}")
        lines.append(f"  [bold]State:[/bold]      {self._colorize_state(job.state)}")
        lines.append("")

        # Full reason (untruncated)
        reason = detail.get("Reason", job.reason) or job.reason
        reason_color = "yellow" if reason and reason != "None" else "dim"
        lines.append("[bold cyan]── Reason ────────────────────────────────[/bold cyan]")
        lines.append(f"  [{reason_color}]{reason or '(none)'}[/{reason_color}]")
        lines.append("")

        # Timing section
        submit_time = detail.get("SubmitTime", "")
        start_time = detail.get("StartTime", "")
        end_time = detail.get("EndTime", "")
        if submit_time or start_time or end_time:
            lines.append("[bold cyan]── Timing ────────────────────────────────[/bold cyan]")
            if submit_time:
                lines.append(f"  [bold]Submitted:[/bold]  {submit_time}")
            if start_time and start_time not in {"N/A", "Unknown"}:
                lines.append(f"  [bold]Started:[/bold]    {start_time}")
            if end_time and end_time not in {"N/A", "Unknown"}:
                lines.append(f"  [bold]End:[/bold]        {end_time}")
            time_used = job.time_used or detail.get("RunTime", "")
            time_limit = job.time_limit or detail.get("TimeLimit", "")
            if time_used:
                lines.append(f"  [bold]Time used:[/bold]  {time_used}")
            if time_limit:
                lines.append(f"  [bold]Time limit:[/bold] {time_limit}")
            lines.append("")

        # Resources section
        lines.append("[bold cyan]── Resources ─────────────────────────────[/bold cyan]")
        num_nodes = detail.get("NumNodes", job.num_nodes or job.nodes)
        num_cpus = detail.get("NumCPUs", job.num_cpus)
        mem = detail.get("MinMemoryNode", "") or detail.get("mem", "")
        nodelist = detail.get("NodeList", job.nodelist or "")
        tres = detail.get("TRES", "")

        lines.append(f"  [bold]Nodes:[/bold]      {num_nodes}")
        lines.append(f"  [bold]CPUs:[/bold]       {num_cpus}")
        if mem:
            lines.append(f"  [bold]Memory:[/bold]     {mem}")
        if nodelist and nodelist not in {"(null)", "N/A"}:
            lines.append(f"  [bold]Nodelist:[/bold]   {nodelist}")
        if tres:
            lines.append(f"  [bold]TRES:[/bold]       {tres}")
        lines.append("")

        # Working directory / scripts
        work_dir = detail.get("WorkDir", "")
        stdout = detail.get("StdOut", "")
        stderr = detail.get("StdErr", "")
        command = detail.get("Command", "")
        if work_dir or stdout or stderr or command:
            lines.append("[bold cyan]── Paths ──────────────────────────────────[/bold cyan]")
            if work_dir:
                lines.append(f"  [bold]WorkDir:[/bold]    {work_dir}")
            if command:
                lines.append(f"  [bold]Script:[/bold]     {command}")
            if stdout:
                lines.append(f"  [bold]StdOut:[/bold]     {stdout}")
            if stderr:
                lines.append(f"  [bold]StdErr:[/bold]     {stderr}")
            lines.append("")

        # Dependencies section
        if deps:
            lines.append("[bold cyan]── Dependencies ──────────────────────────[/bold cyan]")
            for dep in deps:
                dep_color = "green" if dep.state == "COMPLETED" else "yellow"
                lines.append(
                    f"  [{dep_color}]{dep.dep_type}:{dep.job_id}  [{dep.state}][/{dep_color}]"
                )
            lines.append("")
        elif detail.get("Dependency", "") and detail.get("Dependency", "") not in {"None", "(null)"}:
            lines.append("[bold cyan]── Dependencies ──────────────────────────[/bold cyan]")
            lines.append(f"  [dim]{detail['Dependency']}[/dim]")
            lines.append("")

        lines.append("[dim]  Press q or Esc to close[/dim]")
        return "\n".join(lines)

    def _colorize_state(self, state: str) -> str:
        colors = {
            "RUNNING":   "green",
            "PENDING":   "yellow",
            "FAILED":    "red",
            "CANCELLED": "red",
            "COMPLETED": "dim",
            "TIMEOUT":   "magenta",
            "NODE_FAIL": "red",
            "PREEMPTED": "yellow",
            "COMPLETING": "cyan",
        }
        color = colors.get(state, "white")
        return f"[{color}]{state}[/{color}]"

    def _update_content(self, markup: str) -> None:
        self.query_one("#job-info-content", Static).update(markup)

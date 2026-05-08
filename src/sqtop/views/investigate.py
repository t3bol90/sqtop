"""Investigation screen — evidence-based per-job report (SPEC sec. 8)."""
from __future__ import annotations

from textual import work
from textual.app import ComposeResult
from textual.binding import Binding
from textual.containers import ScrollableContainer
from textual.screen import ModalScreen
from textual.widgets import Label, Static, TextArea

from ..clipboard import app_copy
from ..investigation import render_report
from ..responsive import Tier
from ..slurm import investigate_job, investigate_node


class JobInvestigationScreen(ModalScreen[None]):
    """Modal that shows the rendered InvestigationReport for a job.

    The Slurm I/O happens inside a worker thread (``@work(thread=True)``)
    so the UI never blocks on subprocess calls. The TextArea content is
    plain ASCII produced by ``investigation.render_report``; the screen
    does not re-implement formatting.
    """

    BINDINGS = [
        Binding("escape", "dismiss(None)", show=False),
        Binding("q", "dismiss(None)", "Close", show=True),
        Binding("y", "copy_report", show=False),
        Binding("ctrl+c", "copy_report", show=False),
        Binding("v", "noop", show=False),
    ]

    CSS = """
    JobInvestigationScreen { align: center middle; }
    #investigate-dialog {
        width: 90%; height: 85%;
        min-width: 60; max-width: 140;
        min-height: 20; max-height: 50;
        border: double $primary;
        background: $surface;
        padding: 0;
    }
    JobInvestigationScreen.clamp-xs #investigate-dialog {
        width: 100%; height: 100%;
        min-width: 0; min-height: 0;
    }
    #investigate-title {
        text-style: bold;
        padding: 0 2;
        margin-bottom: 1;
        background: $primary;
        color: $background;
        width: 100%;
    }
    #investigate-scroll {
        height: 1fr;
        padding: 1 2;
    }
    #investigate-content {
        width: 100%;
        height: 1fr;
        background: $surface;
        border: none;
    }
    """

    def __init__(self, job_id: str, *, job_name: str | None = None) -> None:
        super().__init__()
        self._job_id = job_id
        self._job_name = job_name
        self._plain_text: str = ""

    def responsive_clamp(self, tier: Tier) -> None:
        self.add_class(f"clamp-{tier}")

    def compose(self) -> ComposeResult:
        header = f"Investigate Job {self._job_id}"
        if self._job_name:
            header += f" — {self._job_name}"
        with Static(id="investigate-dialog"):
            yield Label(header, id="investigate-title")
            with ScrollableContainer(id="investigate-scroll"):
                yield TextArea("Loading…", id="investigate-content", read_only=True)

    def on_mount(self) -> None:
        self._load_report()

    @work(thread=True)
    def _load_report(self) -> None:
        report = investigate_job(self._job_id)
        text = render_report(report)
        self.app.call_from_thread(self._update_content, text)

    def _update_content(self, text: str) -> None:
        self._plain_text = text
        self.query_one("#investigate-content", TextArea).load_text(text)

    def action_copy_report(self) -> None:
        ta = self.query_one(TextArea)
        text = ta.selected_text or ta.text or self._plain_text
        app_copy(
            self.app,
            text,
            label=f"Investigation Job {self._job_id}",
            count=len(text.splitlines()),
        )

    def action_noop(self) -> None:
        pass

    def copy_pane(self) -> tuple[str, str, int]:
        """Return (label, payload, line_count) for ctrl+shift+y."""
        text = self._plain_text or self.query_one(TextArea).text
        label = f"Investigation Job {self._job_id}"
        return label, text, len(text.splitlines())


def push_for_job_id(app, job_id: str, *, job_name: str | None = None) -> None:
    """Push a JobInvestigationScreen for the given job id.

    Convenience entry-point used by the palette command in app.py.
    """
    app.push_screen(JobInvestigationScreen(job_id, job_name=job_name))


class NodeInvestigationScreen(ModalScreen[None]):
    """Modal that shows the rendered InvestigationReport for a node.

    Mirrors :class:`JobInvestigationScreen`. The Slurm I/O happens inside
    a worker thread (``@work(thread=True)``) so the UI never blocks on
    subprocess calls. The TextArea content is plain ASCII produced by
    ``investigation.render_report``; the screen does not re-implement
    formatting.
    """

    BINDINGS = [
        Binding("escape", "dismiss(None)", show=False),
        Binding("q", "dismiss(None)", "Close", show=True),
        Binding("y", "copy_report", show=False),
        Binding("ctrl+c", "copy_report", show=False),
        Binding("v", "noop", show=False),
    ]

    CSS = """
    NodeInvestigationScreen { align: center middle; }
    #investigate-dialog {
        width: 90%; height: 85%;
        min-width: 60; max-width: 140;
        min-height: 20; max-height: 50;
        border: double $primary;
        background: $surface;
        padding: 0;
    }
    NodeInvestigationScreen.clamp-xs #investigate-dialog {
        width: 100%; height: 100%;
        min-width: 0; min-height: 0;
    }
    #investigate-title {
        text-style: bold;
        padding: 0 2;
        margin-bottom: 1;
        background: $primary;
        color: $background;
        width: 100%;
    }
    #investigate-scroll {
        height: 1fr;
        padding: 1 2;
    }
    #investigate-content {
        width: 100%;
        height: 1fr;
        background: $surface;
        border: none;
    }
    """

    def __init__(self, node_name: str) -> None:
        super().__init__()
        self._node_name = node_name
        self._plain_text: str = ""

    def responsive_clamp(self, tier: Tier) -> None:
        self.add_class(f"clamp-{tier}")

    def compose(self) -> ComposeResult:
        header = f"Investigate Node {self._node_name}"
        with Static(id="investigate-dialog"):
            yield Label(header, id="investigate-title")
            with ScrollableContainer(id="investigate-scroll"):
                yield TextArea("Loading…", id="investigate-content", read_only=True)

    def on_mount(self) -> None:
        self._load_report()

    @work(thread=True)
    def _load_report(self) -> None:
        report = investigate_node(self._node_name)
        text = render_report(report)
        self.app.call_from_thread(self._update_content, text)

    def _update_content(self, text: str) -> None:
        self._plain_text = text
        self.query_one("#investigate-content", TextArea).load_text(text)

    def action_copy_report(self) -> None:
        ta = self.query_one(TextArea)
        text = ta.selected_text or ta.text or self._plain_text
        app_copy(
            self.app,
            text,
            label=f"Investigation Node {self._node_name}",
            count=len(text.splitlines()),
        )

    def action_noop(self) -> None:
        pass

    def copy_pane(self) -> tuple[str, str, int]:
        """Return (label, payload, line_count) for ctrl+shift+y."""
        text = self._plain_text or self.query_one(TextArea).text
        label = f"Investigation Node {self._node_name}"
        return label, text, len(text.splitlines())


def push_for_node_name(app, node_name: str) -> None:
    """Push a NodeInvestigationScreen for the given node name.

    Convenience entry-point mirroring :func:`push_for_job_id`.
    """
    app.push_screen(NodeInvestigationScreen(node_name))

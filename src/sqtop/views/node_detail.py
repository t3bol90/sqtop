"""Node detail modal — shows scontrol show node output."""
from __future__ import annotations

from textual.app import ComposeResult
from textual.binding import Binding
from textual.screen import ModalScreen
from textual.containers import ScrollableContainer
from textual.widgets import Label, Static, TextArea
from textual import work

from ..slurm import Node, fetch_node_detail
from ..clipboard import app_copy
from .detail import DetailView
from ..responsive import Tier


class NodeDetailScreen(ModalScreen[None]):
    """Modal that fetches and displays full node detail via scontrol."""

    BINDINGS = [
        Binding("escape", "dismiss(None)", show=False),
        Binding("q", "dismiss(None)", "Close", show=True),
        Binding("y", "copy_selection_or_all", show=False),
        Binding("ctrl+c", "copy_selection_or_all", show=False),
        Binding("v", "noop", show=False),
    ]

    CSS = """
    NodeDetailScreen { align: center middle; }
    #node-detail-dialog {
        width: 90%; height: 85%;
        min-width: 60; max-width: 140;
        min-height: 20; max-height: 50;
        border: double $primary;
        background: $surface;
        padding: 0;
    }
    NodeDetailScreen.clamp-xs #node-detail-dialog {
        width: 100%; height: 100%;
        min-width: 0; min-height: 0;
    }
    #node-detail-title {
        text-style: bold;
        padding: 0 2;
        margin-bottom: 1;
        background: $primary;
        color: $background;
        width: 100%;
    }
    #node-detail-scroll {
        height: 1fr;
        padding: 0 2;
    }
    """

    def __init__(self, node: Node) -> None:
        super().__init__()
        self._node = node
        self._detail_data: dict[str, str] = {}

    def responsive_clamp(self, tier: Tier) -> None:
        self.add_class(f"clamp-{tier}")

    def compose(self) -> ComposeResult:
        with Static(id="node-detail-dialog"):
            yield Label(
                f"Node {self._node.name}  [{self._node.state}]",
                id="node-detail-title",
            )
            with ScrollableContainer(id="node-detail-scroll"):
                yield DetailView(id="node-detail-view")

    def on_mount(self) -> None:
        self._fetch_detail()

    @work(thread=True)
    def _fetch_detail(self) -> None:
        data = fetch_node_detail(self._node.name)
        self.app.call_from_thread(self._show_detail, data)

    def _show_detail(self, data: dict[str, str]) -> None:
        self._detail_data = data
        self.query_one("#node-detail-view", DetailView).show_node(data)

    def action_copy_selection_or_all(self) -> None:
        ta = self.query_one(DetailView).query_one(TextArea)
        text = ta.selected_text or ta.text
        app_copy(self.app, text, label="NodeDetail", count=len(text.splitlines()))

    def action_noop(self) -> None:
        pass

    def copy_pane(self) -> tuple[str, str, int]:
        """Return (label, payload, line_count) for ctrl+shift+y."""
        try:
            text = self.query_one("#node-detail-view", DetailView).plain_text()
        except Exception:
            text = "\n".join(f"{k}: {v}" for k, v in self._detail_data.items())
        label = f"Node {self._node.name} Detail"
        return label, text, len(text.splitlines())

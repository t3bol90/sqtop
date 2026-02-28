"""Node detail modal — shows scontrol show node output."""
from __future__ import annotations

from textual.app import ComposeResult
from textual.binding import Binding
from textual.screen import ModalScreen
from textual.widgets import Label, ScrollableContainer, Static
from textual import work

from ..slurm import Node, fetch_node_detail
from .detail import DetailView


class NodeDetailScreen(ModalScreen[None]):
    """Modal that fetches and displays full node detail via scontrol."""

    BINDINGS = [
        Binding("escape", "dismiss(None)", show=False),
        Binding("q", "dismiss(None)", "Close", show=True),
    ]

    CSS = """
    NodeDetailScreen { align: center middle; }
    #node-detail-dialog {
        width: 90%; height: 80%;
        border: double $primary;
        background: $surface;
        padding: 0;
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
        self.query_one("#node-detail-view", DetailView).show_node(data)

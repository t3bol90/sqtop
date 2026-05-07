"""Detail panel — shows scontrol show job/node output."""

from __future__ import annotations

import re

from textual.app import ComposeResult
from textual.widget import Widget
from textual.widgets import TextArea


def _strip_rich(markup: str) -> str:
    """Strip Rich markup tags to produce plain text."""
    return re.sub(r"\[/?[^\[\]]*\]", "", markup)


class DetailView(Widget):
    """Renders key=value pairs from scontrol in a read-only TextArea."""

    DEFAULT_CSS = """
    DetailView {
        height: 1fr;
        width: 100%;
    }
    DetailView TextArea {
        height: 1fr;
        width: 100%;
        background: $surface;
        border: none;
    }
    """

    def __init__(self, *args, **kwargs) -> None:
        super().__init__(*args, **kwargs)
        self._plain_text: str = ""

    def compose(self) -> ComposeResult:
        yield TextArea("", read_only=True)

    def show_job(self, data: dict[str, str]) -> None:
        self._render_kv("Job Detail", data, highlight_keys={
            "JobId", "JobName", "UserId", "JobState",
            "NumNodes", "NumCPUs", "TimeLimit", "SubmitTime",
            "StartTime", "EndTime", "Partition", "NodeList",
            "Reason", "Priority",
        })

    def show_node(self, data: dict[str, str]) -> None:
        self._render_kv("Node Detail", data, highlight_keys={
            "NodeName", "State", "CPUTot", "CPUAlloc",
            "RealMemory", "FreeMem", "OS", "Arch",
            "CfgTRES", "AllocTRES", "Reason",
        })

    def _render_kv(
        self,
        title: str,
        data: dict[str, str],
        highlight_keys: set[str],
    ) -> None:
        lines = [f"{title}\n"]
        for k, v in data.items():
            lines.append(f"  {k}: {v}")
        text = "\n".join(lines)
        self._plain_text = text
        if self.is_mounted:
            self.query_one(TextArea).load_text(text)

    def plain_text(self) -> str:
        """Return the current plain-text content."""
        return self._plain_text

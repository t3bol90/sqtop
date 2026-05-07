"""Detail panel — shows scontrol show job/node output."""

from __future__ import annotations

from textual.app import ComposeResult
from textual.widgets import Static
from rich.table import Table
from rich.console import Console
from rich.text import Text


class DetailView(Static):
    """Renders key=value pairs from scontrol in a formatted panel."""

    def __init__(self, *args, **kwargs) -> None:
        super().__init__(*args, **kwargs)
        self._plain_data: dict[str, str] = {}
        self._plain_title: str = ""

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
        self._plain_title = title
        self._plain_data = dict(data)
        lines = [f"[bold underline]{title}[/]\n"]
        for k, v in data.items():
            key_style = "bold cyan" if k in highlight_keys else "dim"
            lines.append(f"  [{key_style}]{k}[/]: {v}")
        self.update("\n".join(lines))

    def plain_text(self) -> str:
        """Return the detail content as plain text (no markup)."""
        lines = [self._plain_title, ""]
        for k, v in self._plain_data.items():
            lines.append(f"  {k}: {v}")
        return "\n".join(lines)

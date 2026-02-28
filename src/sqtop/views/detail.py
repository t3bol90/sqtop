"""Detail panel — shows scontrol show job/node output."""

from __future__ import annotations

from textual.app import ComposeResult
from textual.widgets import Static
from rich.table import Table
from rich.console import Console
from rich.text import Text


class DetailView(Static):
    """Renders key=value pairs from scontrol in a formatted panel."""

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
        lines = [f"[bold underline]{title}[/]\n"]
        for k, v in data.items():
            key_style = "bold cyan" if k in highlight_keys else "dim"
            lines.append(f"  [{key_style}]{k}[/]: {v}")
        self.update("\n".join(lines))

"""Desktop notification helper for sqtop."""
from __future__ import annotations

import subprocess
import sys


def desktop_notify(title: str, message: str) -> None:
    """Fire desktop notification. Silent failure — terminal bell is primary signal."""
    try:
        if sys.platform == "darwin":
            subprocess.run(
                ["osascript", "-e",
                 f'display notification "{_esc(message)}" with title "{_esc(title)}"'],
                timeout=2,
                capture_output=True,
            )
        elif sys.platform.startswith("linux"):
            subprocess.run(
                ["notify-send", "--expire-time=8000", title, message],
                timeout=2,
                capture_output=True,
            )
    except Exception:
        pass


def _esc(s: str) -> str:
    return s.replace('"', '\\"').replace("'", "\\'")

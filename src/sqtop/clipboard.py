"""Clipboard helper with OSC 52 transport (works over SSH) and subprocess fallback."""
from __future__ import annotations

import subprocess
import sys
from dataclasses import dataclass, field
from typing import Literal

from . import config
from . import slurm

Transport = Literal["osc52", "pbcopy", "xclip", "xsel", "clip", "none"]

OSC52_MAX_BYTES = 74_000


@dataclass
class CopyResult:
    ok: bool
    transport: Transport
    truncated: bool = field(default=False)


def _truncate_utf8(text: str, max_bytes: int) -> tuple[str, bool]:
    """Truncate *text* to the largest valid UTF-8 prefix that fits in *max_bytes*.

    Returns (truncated_text, was_truncated).
    """
    encoded = text.encode("utf-8")
    if len(encoded) <= max_bytes:
        return text, False
    # Slice raw bytes then decode with errors="ignore" to drop any partial multi-byte char.
    truncated = encoded[:max_bytes].decode("utf-8", errors="ignore")
    return truncated, True


def _try_subprocess(text: str) -> CopyResult:
    """Attempt subprocess clipboard write using platform-appropriate tool."""
    encoded = text.encode("utf-8")
    if sys.platform == "darwin":
        try:
            subprocess.run(["pbcopy"], input=encoded, check=True, timeout=2)
            return CopyResult(ok=True, transport="pbcopy")
        except Exception:
            return CopyResult(ok=False, transport="none")
    elif sys.platform == "win32":
        try:
            subprocess.run(["clip"], input=encoded, check=True, timeout=2)
            return CopyResult(ok=True, transport="clip")
        except Exception:
            return CopyResult(ok=False, transport="none")
    else:
        # Linux — try xclip then xsel
        try:
            subprocess.run(
                ["xclip", "-selection", "clipboard"],
                input=encoded, check=True, timeout=2,
            )
            return CopyResult(ok=True, transport="xclip")
        except (FileNotFoundError, subprocess.CalledProcessError, subprocess.TimeoutExpired):
            pass
        try:
            subprocess.run(
                ["xsel", "--clipboard", "--input"],
                input=encoded, check=True, timeout=2,
            )
            return CopyResult(ok=True, transport="xsel")
        except Exception:
            pass
        return CopyResult(ok=False, transport="none")


def copy_to_clipboard(app, text: str) -> CopyResult:
    """Copy *text* to the clipboard.

    Transport selection is controlled by ``cfg["clipboard"]["transport"]``:
    - ``"auto"``       — OSC 52 first, subprocess fallback if local run.
    - ``"osc52"``      — OSC 52 only; no subprocess fallback.
    - ``"subprocess"`` — subprocess only (skips OSC 52).

    OSC 52 payloads > ``OSC52_MAX_BYTES`` are truncated; ``CopyResult.truncated``
    is set accordingly.
    """
    cfg = config.load()
    mode: str = cfg.get("clipboard", {}).get("transport", "auto")

    # --- OSC 52 path ---
    if mode in ("auto", "osc52"):
        payload, truncated = _truncate_utf8(text, OSC52_MAX_BYTES)
        try:
            app.copy_to_clipboard(payload)
            return CopyResult(ok=True, transport="osc52", truncated=truncated)
        except Exception:
            pass  # fall through to subprocess or give up

    # --- Subprocess fallback ---
    # Only allowed on local runs (no SSH host) and when mode permits.
    if mode in ("auto", "subprocess") and slurm._SSH_HOST is None:
        return _try_subprocess(text)

    return CopyResult(ok=False, transport="none")


def app_copy(app, text: str, *, label: str, count: int | None = None) -> CopyResult:
    """Copy *text* and notify the user via *app*.

    *label* names what was copied (e.g. ``"Job 12345"`` or ``"3 rows"``).
    *count* is appended in parentheses when provided.
    """
    result = copy_to_clipboard(app, text)

    if result.ok:
        msg = label
        if count is not None:
            msg += f" ({count})"
        msg += f" · {result.transport}"
        if result.truncated:
            msg += " · truncated"
        severity = "warning" if result.truncated else "information"
    else:
        msg = "Clipboard unavailable"
        severity = "warning"

    app.notify(msg, title="Clipboard", severity=severity)
    return result

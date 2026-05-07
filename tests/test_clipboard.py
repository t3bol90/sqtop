"""Tests for sqtop.clipboard — transport selection and size guard."""
from __future__ import annotations

import subprocess
from unittest.mock import MagicMock, patch

import pytest

from sqtop import slurm
from sqtop.clipboard import (
    OSC52_MAX_BYTES,
    CopyResult,
    Transport,
    app_copy,
    copy_to_clipboard,
)


# ---------------------------------------------------------------------------
# Helpers
# ---------------------------------------------------------------------------

def _make_app(*, osc52_raises: bool = False) -> MagicMock:
    """Return a minimal mock that looks like a Textual App."""
    app = MagicMock()
    if osc52_raises:
        app.copy_to_clipboard.side_effect = Exception("OSC52 not supported")
    else:
        app.copy_to_clipboard.return_value = None  # sync, returns None
    app.notify = MagicMock()
    return app


# ---------------------------------------------------------------------------
# copy_to_clipboard — OSC 52 path
# ---------------------------------------------------------------------------

class TestOsc52Preferred:
    """OSC 52 is tried first and returns transport='osc52'."""

    def test_osc52_used_when_available(self, temp_config):
        app = _make_app()
        result = copy_to_clipboard(app, "hello")
        app.copy_to_clipboard.assert_called_once_with("hello")
        assert result.ok is True
        assert result.transport == "osc52"
        assert result.truncated is False

    def test_osc52_used_in_auto_mode(self, temp_config):
        app = _make_app()
        result = copy_to_clipboard(app, "world")
        assert result.transport == "osc52"

    def test_osc52_used_in_osc52_mode(self, temp_config, monkeypatch):
        import sqtop.config as cfg_mod
        monkeypatch.setattr(cfg_mod, "load", lambda: {"clipboard": {"transport": "osc52"}})
        app = _make_app()
        result = copy_to_clipboard(app, "text")
        assert result.ok is True
        assert result.transport == "osc52"


# ---------------------------------------------------------------------------
# copy_to_clipboard — subprocess fallback
# ---------------------------------------------------------------------------

class TestSubprocessFallback:
    """Subprocess is tried only when SSH host is unset and config allows."""

    def test_subprocess_fires_when_osc52_fails_locally(self, temp_config, monkeypatch):
        # Ensure no SSH host
        monkeypatch.setattr(slurm, "_SSH_HOST", None)
        app = _make_app(osc52_raises=True)
        with patch("sqtop.clipboard.subprocess.run") as mock_run:
            mock_run.return_value = MagicMock(returncode=0)
            result = copy_to_clipboard(app, "data")
        # subprocess was tried
        assert mock_run.called
        assert result.ok is True

    def test_subprocess_skipped_when_ssh_host_set(self, temp_config, monkeypatch):
        monkeypatch.setattr(slurm, "_SSH_HOST", "login01.cluster.example.com")
        app = _make_app(osc52_raises=True)
        with patch("sqtop.clipboard.subprocess.run") as mock_run:
            result = copy_to_clipboard(app, "data")
        assert not mock_run.called
        assert result.ok is False
        assert result.transport == "none"

    def test_subprocess_only_mode_local(self, temp_config, monkeypatch):
        monkeypatch.setattr(slurm, "_SSH_HOST", None)
        import sqtop.config as cfg_mod
        monkeypatch.setattr(cfg_mod, "load", lambda: {"clipboard": {"transport": "subprocess"}})
        app = _make_app()
        with patch("sqtop.clipboard.subprocess.run") as mock_run:
            mock_run.return_value = MagicMock(returncode=0)
            result = copy_to_clipboard(app, "data")
        # OSC52 should NOT have been called (subprocess mode)
        app.copy_to_clipboard.assert_not_called()
        assert mock_run.called

    def test_subprocess_only_mode_remote_returns_none(self, temp_config, monkeypatch):
        monkeypatch.setattr(slurm, "_SSH_HOST", "remote.host")
        import sqtop.config as cfg_mod
        monkeypatch.setattr(cfg_mod, "load", lambda: {"clipboard": {"transport": "subprocess"}})
        app = _make_app()
        with patch("sqtop.clipboard.subprocess.run") as mock_run:
            result = copy_to_clipboard(app, "data")
        assert not mock_run.called
        assert result.ok is False
        assert result.transport == "none"


# ---------------------------------------------------------------------------
# copy_to_clipboard — remote SSH, OSC 52 fails → no subprocess
# ---------------------------------------------------------------------------

class TestRemoteNoSubprocess:
    """When _SSH_HOST is set and OSC 52 raises, result is ok=False, transport='none'."""

    def test_no_fallback_on_remote(self, temp_config, monkeypatch):
        monkeypatch.setattr(slurm, "_SSH_HOST", "hpc-login01")
        app = _make_app(osc52_raises=True)
        with patch("sqtop.clipboard.subprocess.run") as mock_run:
            result = copy_to_clipboard(app, "sensitive data")
        assert not mock_run.called, "subprocess must NOT run when on a remote host"
        assert result.ok is False
        assert result.transport == "none"


# ---------------------------------------------------------------------------
# copy_to_clipboard — OSC 52 size guard / truncation
# ---------------------------------------------------------------------------

class TestSizeGuard:
    """Payloads > OSC52_MAX_BYTES are truncated; result.truncated=True."""

    def test_large_payload_truncated(self, temp_config):
        # Build a text > 74 KB (all ASCII for simplicity)
        big_text = "x" * (OSC52_MAX_BYTES + 5_000)
        app = _make_app()
        result = copy_to_clipboard(app, big_text)
        assert result.ok is True
        assert result.transport == "osc52"
        assert result.truncated is True
        # The argument passed to copy_to_clipboard must be ≤ 74 KB in UTF-8
        sent = app.copy_to_clipboard.call_args[0][0]
        assert len(sent.encode("utf-8")) <= OSC52_MAX_BYTES

    def test_small_payload_not_truncated(self, temp_config):
        text = "a" * 100
        app = _make_app()
        result = copy_to_clipboard(app, text)
        assert result.truncated is False

    def test_truncated_payload_is_valid_utf8(self, temp_config):
        # Construct a string with multi-byte chars near the boundary
        # Each emoji is 4 bytes; just over the limit
        emoji = "\U0001F600"  # 4 bytes each
        n = (OSC52_MAX_BYTES // 4) + 10
        big_text = emoji * n
        app = _make_app()
        result = copy_to_clipboard(app, big_text)
        assert result.truncated is True
        sent = app.copy_to_clipboard.call_args[0][0]
        # Must be valid UTF-8 (encode/decode round-trip without error)
        encoded = sent.encode("utf-8")
        assert len(encoded) <= OSC52_MAX_BYTES
        decoded = encoded.decode("utf-8")  # should not raise
        assert len(decoded) > 0


# ---------------------------------------------------------------------------
# copy_to_clipboard — all clipboard tools missing
# ---------------------------------------------------------------------------

class TestAllToolsMissing:
    """When all subprocess tools are absent, return ok=False, no exception."""

    def test_no_tools_ok_false(self, temp_config, monkeypatch):
        monkeypatch.setattr(slurm, "_SSH_HOST", None)
        app = _make_app(osc52_raises=True)

        def raise_file_not_found(*args, **kwargs):
            raise FileNotFoundError("no such file")

        with patch("sqtop.clipboard.subprocess.run", side_effect=raise_file_not_found):
            result = copy_to_clipboard(app, "text")
        assert result.ok is False
        assert result.transport == "none"

    def test_no_exception_on_missing_tools(self, temp_config, monkeypatch):
        monkeypatch.setattr(slurm, "_SSH_HOST", None)
        app = _make_app(osc52_raises=True)
        with patch("sqtop.clipboard.subprocess.run", side_effect=FileNotFoundError):
            # Must not raise
            result = copy_to_clipboard(app, "text")
        assert isinstance(result, CopyResult)


# ---------------------------------------------------------------------------
# app_copy — notify message formatting
# ---------------------------------------------------------------------------

class TestAppCopy:
    """app_copy emits correct notify messages."""

    def test_notify_on_success(self, temp_config):
        app = _make_app()
        result = app_copy(app, "hello", label="Job 42", count=1)
        assert result.ok is True
        app.notify.assert_called_once()
        call_kwargs = app.notify.call_args
        msg = call_kwargs[0][0]
        assert "Job 42" in msg
        assert "(1)" in msg
        assert "osc52" in msg
        assert call_kwargs[1].get("severity") == "information"

    def test_notify_on_failure(self, temp_config, monkeypatch):
        monkeypatch.setattr(slurm, "_SSH_HOST", "remote")
        app = _make_app(osc52_raises=True)
        result = app_copy(app, "hello", label="Job 42")
        assert result.ok is False
        call_kwargs = app.notify.call_args
        msg = call_kwargs[0][0]
        assert "unavailable" in msg.lower()
        assert call_kwargs[1].get("severity") == "warning"

    def test_notify_on_truncation(self, temp_config):
        big_text = "y" * (OSC52_MAX_BYTES + 1_000)
        app = _make_app()
        result = app_copy(app, big_text, label="Pane", count=5000)
        assert result.truncated is True
        msg = app.notify.call_args[0][0]
        assert "truncated" in msg
        assert app.notify.call_args[1].get("severity") == "warning"

    def test_count_none_omits_parentheses(self, temp_config):
        app = _make_app()
        app_copy(app, "text", label="MyLabel")
        msg = app.notify.call_args[0][0]
        assert "(" not in msg

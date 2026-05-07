"""Tests for copy support in text-pane modals (PR 4)."""
from __future__ import annotations

import pytest
from unittest.mock import MagicMock, patch

from sqtop.clipboard import app_copy, copy_to_clipboard, CopyResult
from sqtop.views.detail import DetailView, _strip_rich
from sqtop.views.job_info import _strip_rich as job_info_strip_rich


# ---------------------------------------------------------------------------
# clipboard.py unit tests
# ---------------------------------------------------------------------------

def test_copy_to_clipboard_ok():
    app = MagicMock()
    result = copy_to_clipboard(app, "hello")
    app.copy_to_clipboard.assert_called_once_with("hello")
    assert result.ok is True
    assert result.transport == "osc52"


def test_copy_to_clipboard_failure():
    app = MagicMock()
    app.copy_to_clipboard.side_effect = Exception("no clipboard")
    result = copy_to_clipboard(app, "hello")
    assert result.ok is False
    assert result.transport == "none"


def test_app_copy_ok_message():
    app = MagicMock()
    r = app_copy(app, "some text", label="Test", count=3)
    assert r.ok is True
    app.notify.assert_called_once()
    call_kwargs = app.notify.call_args
    # message contains label, count, and transport
    msg = call_kwargs[0][0]
    assert "Test" in msg
    assert "3" in msg
    assert "osc52" in msg
    assert call_kwargs[1]["severity"] == "information"


def test_app_copy_failure_message():
    app = MagicMock()
    app.copy_to_clipboard.side_effect = Exception("fail")
    r = app_copy(app, "some text", label="Test")
    assert r.ok is False
    call_kwargs = app.notify.call_args
    msg = call_kwargs[0][0]
    assert "Clipboard unavailable" in msg
    assert call_kwargs[1]["severity"] == "warning"


def test_app_copy_no_count():
    app = MagicMock()
    r = app_copy(app, "text", label="MyLabel")
    call_kwargs = app.notify.call_args
    msg = call_kwargs[0][0]
    assert "MyLabel" in msg
    assert "osc52" in msg


# ---------------------------------------------------------------------------
# _strip_rich helper
# ---------------------------------------------------------------------------

def test_strip_rich_removes_tags():
    markup = "[bold cyan]── Title ──[/bold cyan]\n  [bold]Key:[/bold] value"
    plain = _strip_rich(markup)
    assert "[" not in plain
    assert "]" not in plain
    assert "── Title ──" in plain
    assert "Key:" in plain
    assert "value" in plain


def test_strip_rich_no_tags():
    text = "plain text with no markup"
    assert _strip_rich(text) == text


# ---------------------------------------------------------------------------
# DetailView widget tests
# ---------------------------------------------------------------------------

def _make_detail_view() -> DetailView:
    """Create a DetailView and mount it in a minimal app for testing."""
    return DetailView()


def test_detail_view_show_job_populates_text():
    """DetailView.show_job should load text into the TextArea."""
    from textual.widgets import TextArea

    dv = DetailView()
    # We can test the text building logic without a full app:
    # _render_kv builds lines and calls load_text on the TextArea.
    # Simulate by accessing the internal method with a mock TextArea.
    mock_ta = MagicMock()
    with patch.object(dv, "query_one", return_value=mock_ta):
        dv.show_job({"JobId": "123", "JobName": "test"})
    mock_ta.load_text.assert_called_once()
    text = mock_ta.load_text.call_args[0][0]
    assert "Job Detail" in text
    assert "JobId" in text
    assert "123" in text
    assert "JobName" in text
    assert "test" in text


def test_detail_view_show_node_populates_text():
    dv = DetailView()
    mock_ta = MagicMock()
    with patch.object(dv, "query_one", return_value=mock_ta):
        dv.show_node({"NodeName": "node01", "State": "idle"})
    mock_ta.load_text.assert_called_once()
    text = mock_ta.load_text.call_args[0][0]
    assert "Node Detail" in text
    assert "NodeName" in text
    assert "node01" in text


def test_detail_view_get_text():
    dv = DetailView()
    mock_ta = MagicMock()
    mock_ta.text = "hello world"
    with patch.object(dv, "query_one", return_value=mock_ta):
        result = dv.get_text()
    assert result == "hello world"


# ---------------------------------------------------------------------------
# BINDINGS presence checks — no full app needed
# ---------------------------------------------------------------------------

def _binding_keys(cls) -> set[str]:
    return {b.key for b in cls.BINDINGS}


def test_job_info_screen_has_copy_bindings():
    from sqtop.views.job_info import JobInfoScreen
    keys = _binding_keys(JobInfoScreen)
    assert "y" in keys
    assert "ctrl+c" in keys
    assert "v" in keys  # reserved no-op


def test_batch_script_screen_has_copy_bindings():
    from sqtop.views.batch_script import BatchScriptScreen
    keys = _binding_keys(BatchScriptScreen)
    assert "y" in keys
    assert "ctrl+c" in keys
    assert "v" in keys


def test_job_detail_screen_has_copy_bindings():
    from sqtop.views.job_detail import JobDetailScreen
    keys = _binding_keys(JobDetailScreen)
    assert "y" in keys
    assert "ctrl+c" in keys
    assert "v" in keys


def test_node_detail_screen_has_copy_bindings():
    from sqtop.views.node_detail import NodeDetailScreen
    keys = _binding_keys(NodeDetailScreen)
    assert "y" in keys
    assert "ctrl+c" in keys
    assert "v" in keys


def test_log_viewer_screen_has_copy_bindings():
    from sqtop.views.log_viewer import LogViewerScreen
    keys = _binding_keys(LogViewerScreen)
    assert "y" in keys
    assert "ctrl+c" in keys


# ---------------------------------------------------------------------------
# copy action logic (mocked TextArea)
# ---------------------------------------------------------------------------

def test_batch_script_copy_selection():
    """copy_selection_or_all copies selected_text when selection exists."""
    from sqtop.views.batch_script import BatchScriptScreen
    screen = BatchScriptScreen.__new__(BatchScriptScreen)
    screen._job_id = "99"
    screen._script = "#!/bin/bash\necho hi"

    mock_ta = MagicMock()
    mock_ta.selected_text = "echo hi"
    mock_ta.text = "#!/bin/bash\necho hi"

    mock_app = MagicMock()

    with patch.object(screen, "query_one", return_value=mock_ta):
        with patch("sqtop.views.batch_script.app_copy") as mock_app_copy:
            with patch.object(type(screen), "app", new_callable=lambda: property(lambda self: mock_app)):
                screen.action_copy_selection_or_all()
            mock_app_copy.assert_called_once_with(
                mock_app, "echo hi", label="BatchScript", count=1
            )


def test_batch_script_copy_all_when_no_selection():
    """copy_selection_or_all falls back to full text when no selection."""
    from sqtop.views.batch_script import BatchScriptScreen
    screen = BatchScriptScreen.__new__(BatchScriptScreen)
    screen._job_id = "99"
    screen._script = "#!/bin/bash\necho hi"

    mock_ta = MagicMock()
    mock_ta.selected_text = ""
    mock_ta.text = "#!/bin/bash\necho hi"

    mock_app = MagicMock()

    with patch.object(screen, "query_one", return_value=mock_ta):
        with patch("sqtop.views.batch_script.app_copy") as mock_app_copy:
            with patch.object(type(screen), "app", new_callable=lambda: property(lambda self: mock_app)):
                screen.action_copy_selection_or_all()
            mock_app_copy.assert_called_once_with(
                mock_app, "#!/bin/bash\necho hi", label="BatchScript", count=2
            )


def test_log_viewer_copy_uses_last_content():
    """LogViewerScreen.action_copy_selection_or_all copies _last_content."""
    from sqtop.views.log_viewer import LogViewerScreen
    screen = LogViewerScreen.__new__(LogViewerScreen)
    screen._job_id = "42"
    screen._log_path = "/path/to/log"
    screen._log_type = "stdout"
    screen._follow = True
    screen._last_content = "line1\nline2\nline3"

    mock_app = MagicMock()

    with patch("sqtop.views.log_viewer.app_copy") as mock_app_copy:
        with patch.object(type(screen), "app", new_callable=lambda: property(lambda self: mock_app)):
            screen.action_copy_selection_or_all()
        mock_app_copy.assert_called_once_with(
            mock_app, "line1\nline2\nline3", label="Log", count=3
        )


def test_job_info_strip_rich_consistent():
    """Both modules should have consistent _strip_rich behavior."""
    markup = "[bold]hello[/bold] [dim]world[/dim]"
    assert job_info_strip_rich(markup) == _strip_rich(markup)
    assert _strip_rich(markup) == "hello world"

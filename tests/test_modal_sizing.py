"""Tests for responsive modal sizing and responsive_clamp hook.

Strategy: instantiate each big modal via __new__ (skipping mount/compose),
call responsive_clamp, and assert the marker class is set.  No CSS-parsing —
visual confirmation covers the CSS rules.
"""
from __future__ import annotations

import pytest


# ---------------------------------------------------------------------------
# Helpers
# ---------------------------------------------------------------------------

def _new(cls, *args, **kwargs):
    """Create an instance without invoking __init__ (skips Textual mount)."""
    obj = cls.__new__(cls)
    # Minimally seed the _classes attribute that add_class/has_class rely on.
    # Textual stores CSS classes on _css_classes (a set), but add_class uses
    # the public API which may not exist pre-mount.  We patch it here.
    object.__setattr__(obj, "_css_classes", set())
    return obj


# Minimal stub for add_class so we can call responsive_clamp without mounting.
class _ClassStub:
    """Mixin that records add_class calls for assertion."""

    def __init_subclass__(cls, **kw):
        super().__init_subclass__(**kw)

    def add_class(self, *names: str) -> None:
        if not hasattr(self, "_test_classes"):
            self._test_classes: set[str] = set()
        self._test_classes.update(names)

    def has_test_class(self, name: str) -> bool:
        return name in getattr(self, "_test_classes", set())


# ---------------------------------------------------------------------------
# Fixtures — create stub instances without Textual wiring
# ---------------------------------------------------------------------------

def _make_stub(cls):
    """Return a lightweight stub of *cls* that supports responsive_clamp."""
    instance = object.__new__(cls)
    # Inject add_class shim so responsive_clamp can call it without a live App.
    instance._test_classes = set()

    def _add_class(self_or_name, *names):
        # Handle both bound and unbound calls.
        if isinstance(self_or_name, str):
            instance._test_classes.add(self_or_name)
            instance._test_classes.update(names)
        else:
            instance._test_classes.update(names)

    import types
    instance.add_class = types.MethodType(
        lambda self, *args: instance._test_classes.update(args),
        instance,
    )
    return instance


# ---------------------------------------------------------------------------
# Tests
# ---------------------------------------------------------------------------

def test_batch_script_responsive_clamp_xs():
    from sqtop.views.batch_script import BatchScriptScreen
    screen = _make_stub(BatchScriptScreen)
    screen.responsive_clamp("xs")
    assert "clamp-xs" in screen._test_classes


def test_batch_script_responsive_clamp_sm():
    from sqtop.views.batch_script import BatchScriptScreen
    screen = _make_stub(BatchScriptScreen)
    screen.responsive_clamp("sm")
    assert "clamp-sm" in screen._test_classes


def test_array_task_responsive_clamp_xs():
    from sqtop.views.array_tasks import ArrayTaskScreen
    screen = _make_stub(ArrayTaskScreen)
    screen.responsive_clamp("xs")
    assert "clamp-xs" in screen._test_classes


def test_job_info_responsive_clamp_xs():
    from sqtop.views.job_info import JobInfoScreen
    screen = _make_stub(JobInfoScreen)
    screen.responsive_clamp("xs")
    assert "clamp-xs" in screen._test_classes


def test_log_viewer_responsive_clamp_xs():
    from sqtop.views.log_viewer import LogViewerScreen
    screen = _make_stub(LogViewerScreen)
    screen.responsive_clamp("xs")
    assert "clamp-xs" in screen._test_classes


def test_job_detail_responsive_clamp_xs():
    from sqtop.views.job_detail import JobDetailScreen
    screen = _make_stub(JobDetailScreen)
    screen.responsive_clamp("xs")
    assert "clamp-xs" in screen._test_classes


def test_node_detail_responsive_clamp_xs():
    from sqtop.views.node_detail import NodeDetailScreen
    screen = _make_stub(NodeDetailScreen)
    screen.responsive_clamp("xs")
    assert "clamp-xs" in screen._test_classes


def test_responsive_clamp_lg_sets_correct_class():
    """responsive_clamp stores the tier as passed, not always xs."""
    from sqtop.views.batch_script import BatchScriptScreen
    screen = _make_stub(BatchScriptScreen)
    screen.responsive_clamp("lg")
    assert "clamp-lg" in screen._test_classes
    assert "clamp-xs" not in screen._test_classes


def test_big_modals_have_responsive_clamp():
    """All big modals expose the responsive_clamp hook."""
    from sqtop.views.batch_script import BatchScriptScreen
    from sqtop.views.array_tasks import ArrayTaskScreen
    from sqtop.views.job_info import JobInfoScreen
    from sqtop.views.log_viewer import LogViewerScreen
    from sqtop.views.job_detail import JobDetailScreen
    from sqtop.views.node_detail import NodeDetailScreen

    for cls in (
        BatchScriptScreen,
        ArrayTaskScreen,
        JobInfoScreen,
        LogViewerScreen,
        JobDetailScreen,
        NodeDetailScreen,
    ):
        assert callable(getattr(cls, "responsive_clamp", None)), (
            f"{cls.__name__} is missing responsive_clamp"
        )

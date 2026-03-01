from __future__ import annotations

from sqtop.views.bulk_actions import BulkActionScreen
from sqtop.views.job_actions import JobActionScreen


def _binding_map(bindings) -> dict[str, str]:
    return {binding.key: binding.action for binding in bindings}


def test_job_action_screen_has_arrow_navigation_bindings():
    bindings = _binding_map(JobActionScreen.BINDINGS)
    assert bindings["up"] == "focus_previous"
    assert bindings["down"] == "focus_next"
    assert bindings["escape"] == "dismiss(None)"


def test_bulk_action_screen_has_arrow_navigation_bindings():
    bindings = _binding_map(BulkActionScreen.BINDINGS)
    assert bindings["up"] == "focus_previous"
    assert bindings["down"] == "focus_next"
    assert bindings["escape"] == "dismiss(None)"

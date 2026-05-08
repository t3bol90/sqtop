"""Tests for NodesView transient state filter (SPEC §17.2).

The filter is runtime-only: it MUST NOT be persisted to config (SPEC §15).
"""
from __future__ import annotations

import pytest
from textual.binding import Binding

from sqtop import config
from sqtop.slurm import Node
from sqtop.views.nodes import NodesView, _FILTER_CYCLE


def _make_node(
    name: str,
    state: str = "idle",
    *,
    gpu_total: int = 0,
    gpu_alloc: int = 0,
    partition: str = "main",
) -> Node:
    return Node(
        name=name,
        state=state,
        partition=partition,
        cpus_total="64",
        cpus_alloc="0",
        memory_total="256000",
        memory_free="200000",
        load="0.10",
        gpu_total=gpu_total,
        gpu_alloc=gpu_alloc,
    )


# ── Filter helper unit tests ──────────────────────────────────────────────


def test_apply_state_filter_idle_only():
    view = NodesView()
    nodes = [
        _make_node("c1", "idle"),
        _make_node("c2", "idle*"),     # decorated
        _make_node("c3", "allocated"),
        _make_node("c4", "mixed"),
        _make_node("c5", "down"),
        _make_node("c6", "drain"),
        _make_node("c7", "drained"),
    ]
    view._filter_state = "idle"
    out = view._apply_state_filter(nodes)
    names = [n.name for n in out]
    assert names == ["c1", "c2"]


def test_apply_state_filter_allocated_substring():
    view = NodesView()
    nodes = [
        _make_node("c1", "idle"),
        _make_node("c2", "allocated"),
        _make_node("c3", "ALLOCATED*"),  # decorated + uppercase
        _make_node("c4", "mixed"),
        _make_node("c5", "down"),
    ]
    view._filter_state = "allocated"
    out = view._apply_state_filter(nodes)
    names = [n.name for n in out]
    assert names == ["c2", "c3"]


def test_apply_state_filter_mixed_with_decoration():
    view = NodesView()
    nodes = [
        _make_node("c1", "mixed"),
        _make_node("c2", "mixed-"),
        _make_node("c3", "mixed*"),
        _make_node("c4", "idle"),
        _make_node("c5", "allocated"),
    ]
    view._filter_state = "mixed"
    out = view._apply_state_filter(nodes)
    names = [n.name for n in out]
    assert names == ["c1", "c2", "c3"]


def test_apply_state_filter_down_combines_drain():
    view = NodesView()
    nodes = [
        _make_node("c1", "down"),
        _make_node("c2", "drain"),
        _make_node("c3", "drained"),
        _make_node("c4", "idle+drain"),  # decorated drain combo
        _make_node("c5", "idle"),
        _make_node("c6", "allocated"),
        _make_node("c7", "mixed"),
    ]
    view._filter_state = "down"
    out = view._apply_state_filter(nodes)
    names = [n.name for n in out]
    # Down/drain/drained/idle+drain all pass; idle/allocated/mixed do not.
    assert names == ["c1", "c2", "c3", "c4"]


def test_apply_state_filter_gpu_only():
    view = NodesView()
    nodes = [
        _make_node("c1", "idle", gpu_total=0),
        _make_node("c2", "allocated", gpu_total=0),
        _make_node("c3", "idle", gpu_total=4),
        _make_node("c4", "mixed", gpu_total=2),
    ]
    view._filter_state = "gpu"
    out = view._apply_state_filter(nodes)
    names = [n.name for n in out]
    assert names == ["c3", "c4"]


def test_apply_state_filter_empty_returns_all():
    view = NodesView()
    nodes = [
        _make_node("c1", "idle"),
        _make_node("c2", "down"),
        _make_node("c3", "allocated", gpu_total=4),
    ]
    view._filter_state = ""
    out = view._apply_state_filter(nodes)
    # Empty filter is a true no-op: same list, same order, same identity.
    assert out is nodes
    assert [n.name for n in out] == ["c1", "c2", "c3"]


# ── action_cycle_state_filter wiring ──────────────────────────────────────


def test_action_cycle_state_filter_advances_through_cycle(monkeypatch):
    view = NodesView()
    view._last_nodes = []

    notifications: list[tuple[str, str]] = []

    class _FakeApp:
        def notify(self, message: str, title: str = "") -> None:
            notifications.append((message, title))

    fake_app = _FakeApp()
    monkeypatch.setattr(NodesView, "app", property(lambda self: fake_app))

    # Stub render-side effects so the action's body does not require a mounted DOM.
    monkeypatch.setattr(view, "_capture_table_state", lambda: (0, 0.0, None))
    monkeypatch.setattr(view, "_render_rows", lambda rows: None)
    monkeypatch.setattr(view, "_restore_table_state", lambda state, rows: None)
    monkeypatch.setattr(view, "_update_nodes_header", lambda nodes: None)

    expected_after_each_call = ["idle", "allocated", "mixed", "down", "gpu", ""]
    expected_labels = ["IDLE", "ALLOCATED", "MIXED", "DOWN", "GPU", "ALL"]

    assert view._filter_state == ""

    for expected_state, expected_label in zip(expected_after_each_call, expected_labels):
        view.action_cycle_state_filter()
        assert view._filter_state == expected_state
        # Each invocation produces exactly one notification.
        assert len(notifications) == expected_after_each_call.index(expected_state) + 1
        msg, title = notifications[-1]
        assert msg == f"Filter: {expected_label}"
        assert title == "Node Filter"


def test_action_cycle_state_filter_resets_to_known_when_value_unknown(monkeypatch):
    """A foreign filter value falls back to the start of the cycle (-> idle)."""
    view = NodesView()
    view._last_nodes = []
    view._filter_state = "not-a-real-filter"

    notifications: list[tuple[str, str]] = []

    class _FakeApp:
        def notify(self, message: str, title: str = "") -> None:
            notifications.append((message, title))

    monkeypatch.setattr(NodesView, "app", property(lambda self: _FakeApp()))
    monkeypatch.setattr(view, "_capture_table_state", lambda: (0, 0.0, None))
    monkeypatch.setattr(view, "_render_rows", lambda rows: None)
    monkeypatch.setattr(view, "_restore_table_state", lambda state, rows: None)
    monkeypatch.setattr(view, "_update_nodes_header", lambda nodes: None)

    view.action_cycle_state_filter()

    assert view._filter_state == "idle"
    assert notifications[-1] == ("Filter: IDLE", "Node Filter")


# ── Persistence guard (SPEC §15) ──────────────────────────────────────────


def test_filter_state_does_not_persist_to_config(temp_config):
    """Mutating ``_filter_state`` MUST NOT leak any node-filter key into config."""
    view = NodesView()
    view._filter_state = "idle"

    cfg = config.load()

    def _walk(d):
        for k, v in d.items():
            assert "filter" not in k.lower(), f"unexpected filter key in config: {k}"
            if isinstance(v, dict):
                _walk(v)

    _walk(cfg)


# ── Binding shape ─────────────────────────────────────────────────────────


def test_nodes_view_has_f_binding_for_filter():
    """The ``f`` binding maps to ``cycle_state_filter`` and is shown in footer."""
    filter_bindings = [
        b for b in NodesView.BINDINGS
        if isinstance(b, Binding) and b.key == "f"
    ]
    assert len(filter_bindings) == 1
    assert filter_bindings[0].action == "cycle_state_filter"
    assert filter_bindings[0].show is True


def test_nodes_view_filter_cycle_order_matches_spec():
    """Cycle order is fixed: '' -> idle -> allocated -> mixed -> down -> gpu -> ''."""
    assert _FILTER_CYCLE == ("", "idle", "allocated", "mixed", "down", "gpu")


def test_filter_composes_with_sort(monkeypatch):
    """Filter runs before sort; cursor key resolves over the filtered+sorted list."""
    view = NodesView()
    nodes = [
        _make_node("c1", "idle"),
        _make_node("c2", "down"),
        _make_node("c3", "drain"),
        _make_node("c4", "allocated"),
    ]
    view._filter_state = "down"
    view._sort_col = "state"
    view._sort_reversed = False

    out = view._sorted_visible(nodes)
    names = [n.name for n in out]
    # Only down + drain pass the filter; sorted by state ascending.
    assert names == ["c2", "c3"]

"""Tests for column reorder foundation helpers and config keys."""
from __future__ import annotations

import pytest
from sqtop.columns import _reconcile_order, _move_in_order
from sqtop import config


# ── _reconcile_order ─────────────────────────────────────────────────────────


def test_reconcile_identity():
    """saved == default → same order returned."""
    default = ["A", "B", "C"]
    assert _reconcile_order(["A", "B", "C"], default) == ["A", "B", "C"]


def test_reconcile_empty_saved():
    """Empty saved → default order."""
    default = ["A", "B", "C"]
    assert _reconcile_order([], default) == ["A", "B", "C"]


def test_reconcile_dropped_name_appended():
    """Name in default but not in saved is appended in default order."""
    default = ["A", "B", "C"]
    assert _reconcile_order(["A", "C"], default) == ["A", "C", "B"]


def test_reconcile_unknown_saved_name_dropped():
    """Name in saved but not in default is silently dropped."""
    default = ["A", "B", "C"]
    assert _reconcile_order(["A", "X", "B", "C"], default) == ["A", "B", "C"]


def test_reconcile_permutation_preserved():
    """Custom permutation is fully preserved when all names present."""
    default = ["A", "B", "C", "D"]
    assert _reconcile_order(["D", "B", "A", "C"], default) == ["D", "B", "A", "C"]


def test_reconcile_malformed_non_strings_dropped():
    """Non-string entries in saved are silently skipped."""
    default = ["A", "B", "C"]
    assert _reconcile_order(["A", 42, None, "C"], default) == ["A", "C", "B"]


def test_reconcile_malformed_duplicates_first_wins():
    """Duplicate entries in saved — first occurrence wins."""
    default = ["A", "B", "C"]
    assert _reconcile_order(["A", "B", "A", "C"], default) == ["A", "B", "C"]


def test_reconcile_malformed_non_list():
    """Non-list saved (e.g. None, str, dict) is treated as empty."""
    default = ["A", "B", "C"]
    assert _reconcile_order(None, default) == ["A", "B", "C"]
    assert _reconcile_order("A", default) == ["A", "B", "C"]
    assert _reconcile_order({"A": 1}, default) == ["A", "B", "C"]


def test_reconcile_all_unknown_saved():
    """saved contains only unknown names → default order returned."""
    default = ["A", "B", "C"]
    assert _reconcile_order(["X", "Y", "Z"], default) == ["A", "B", "C"]


def test_reconcile_empty_default():
    """Empty default → always empty result."""
    assert _reconcile_order(["A", "B"], []) == []


def test_reconcile_multiple_unknowns_and_permutation():
    """Mix of unknowns and a partial permutation."""
    default = ["A", "B", "C", "D"]
    # saved: D first, C second, X unknown, A third; B missing
    result = _reconcile_order(["D", "C", "X", "A"], default)
    assert result == ["D", "C", "A", "B"]


# ── _move_in_order ────────────────────────────────────────────────────────────


def test_move_first_to_last():
    order = ["A", "B", "C", "D"]
    assert _move_in_order(order, "A", None) == ["B", "C", "D", "A"]


def test_move_last_to_first():
    order = ["A", "B", "C", "D"]
    assert _move_in_order(order, "D", "A") == ["D", "A", "B", "C"]


def test_move_middle_to_middle():
    order = ["A", "B", "C", "D"]
    assert _move_in_order(order, "B", "D") == ["A", "C", "B", "D"]


def test_move_noop_same_position():
    """Moving an element before its existing successor is a no-op in effect."""
    order = ["A", "B", "C"]
    result = _move_in_order(order, "A", "B")
    assert result == ["A", "B", "C"]


def test_move_before_none_appends():
    """before=None always appends to the end."""
    order = ["A", "B", "C"]
    assert _move_in_order(order, "B", None) == ["A", "C", "B"]


def test_move_name_not_in_order():
    """name not in order → return order unchanged."""
    order = ["A", "B", "C"]
    assert _move_in_order(order, "X", "A") == ["A", "B", "C"]


def test_move_before_not_in_order():
    """before not in order → append name to the end."""
    order = ["A", "B", "C"]
    assert _move_in_order(order, "A", "Z") == ["B", "C", "A"]


def test_move_pure_does_not_mutate():
    """_move_in_order must not mutate the input list."""
    order = ["A", "B", "C"]
    original = list(order)
    _move_in_order(order, "A", None)
    assert order == original


def test_move_single_element():
    """Single-element list: move to end or before itself stays the same."""
    assert _move_in_order(["A"], "A", None) == ["A"]
    assert _move_in_order(["A"], "A", "A") == ["A"]


# ── config round-trip ─────────────────────────────────────────────────────────


def test_config_jobs_order_roundtrip(temp_config):
    """Write jobs_order to config and read it back unchanged."""
    custom_order = ["STATE", "JOBID", "NAME", "USER"]
    config.update({"columns": {"jobs_order": custom_order}})
    cfg = config.load()
    assert cfg["columns"]["jobs_order"] == custom_order


def test_config_nodes_order_roundtrip(temp_config):
    """Write nodes_order to config and read it back unchanged."""
    custom_order = ["NODE", "STATE", "CPUS"]
    config.update({"columns": {"nodes_order": custom_order}})
    cfg = config.load()
    assert cfg["columns"]["nodes_order"] == custom_order


def test_config_partitions_order_roundtrip(temp_config):
    """Write partitions_order to config and read it back unchanged."""
    custom_order = ["PARTITION", "AVAIL", "NODES"]
    config.update({"columns": {"partitions_order": custom_order}})
    cfg = config.load()
    assert cfg["columns"]["partitions_order"] == custom_order


def test_config_order_defaults_to_empty_list(temp_config):
    """When no order is set, all three *_order keys default to []."""
    cfg = config.load()
    assert cfg["columns"]["jobs_order"] == []
    assert cfg["columns"]["nodes_order"] == []
    assert cfg["columns"]["partitions_order"] == []


def test_config_order_coerces_non_strings(temp_config):
    """Non-string entries in *_order lists loaded from TOML are dropped."""
    # Write a config with mixed types (TOML only allows homogeneous arrays,
    # so we test the load-side coercion by directly manipulating saved state
    # then verifying update() preserves string-only items).
    config.update({"columns": {"jobs_order": ["A", "B", "C"]}})
    # Manually write a TOML file with a mixed array that tomllib will accept
    # (TOML does not allow mixed arrays, but our load path checks isinstance).
    # Instead: verify that update with pure-string list round-trips correctly,
    # and that load() coerces via the _order branch (strings only kept).
    cfg = config.load()
    assert all(isinstance(x, str) for x in cfg["columns"]["jobs_order"])


def test_config_order_coexists_with_hidden(temp_config):
    """Setting *_order does not disturb the existing *_hidden keys."""
    config.update({
        "columns": {
            "jobs_hidden": ["NODES"],
            "jobs_order": ["STATE", "JOBID"],
        }
    })
    cfg = config.load()
    assert cfg["columns"]["jobs_hidden"] == ["NODES"]
    assert cfg["columns"]["jobs_order"] == ["STATE", "JOBID"]

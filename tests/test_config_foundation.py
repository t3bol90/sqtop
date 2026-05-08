"""Foundation tests for the [interval] table reshape and config invariants.

These tests cover SPEC §15, §16.4, §16.8, §16.9 — they pin the per-view
interval shape introduced in PR 1, plus the back-compat path that accepts a
legacy bare `interval = X.Y` top-level float without crashing.
"""
from __future__ import annotations

import pytest

from sqtop import config


# ── load returns full default tree ────────────────────────────────────────────

def test_load_returns_full_default_tree_when_no_file(temp_config):
    cfg = config.load()
    assert cfg["theme"] == "dracula"
    assert cfg["interval"] == {"jobs": 2.0, "nodes": 2.0, "partitions": 5.0}
    for section in config._DEFAULTS:
        assert section in cfg, f"missing section: {section}"


def test_load_empty_file_returns_defaults(temp_config):
    cfg_file = temp_config / "config.toml"
    cfg_file.write_text("", encoding="utf-8")
    cfg = config.load()
    assert cfg["theme"] == "dracula"
    assert cfg["interval"] == {"jobs": 2.0, "nodes": 2.0, "partitions": 5.0}
    assert cfg["jobs"]["name_max"] == 24
    assert cfg["attach"]["enabled"] is True
    assert cfg["safety"]["confirm_cancel_single"] is True


def test_load_preserves_user_set_values_per_section(temp_config):
    cfg_file = temp_config / "config.toml"
    cfg_file.write_text(
        'theme = "tokyo-night"\n'
        "\n"
        "[interval]\n"
        "jobs = 1.0\n"
        "nodes = 3.0\n"
        "partitions = 7.0\n"
        "\n"
        "[jobs]\n"
        "name_max = 40\n"
        "\n"
        "[attach]\n"
        "enabled = false\n"
        "\n"
        "[ui]\n"
        "expert_mode = true\n"
        "\n"
        "[safety]\n"
        "confirm_cancel_single = false\n"
        "\n"
        "[health]\n"
        "history_size = 250\n"
        "\n"
        "[view_state]\n"
        'jobs_sort_col = "JOBID"\n'
        "\n"
        "[columns]\n"
        'jobs_hidden = ["JOBID"]\n'
        "\n"
        "[notifications]\n"
        "desktop_enabled = false\n"
        "\n"
        "[remote]\n"
        'host = "login.example.org"\n'
        "\n"
        "[clipboard]\n"
        'transport = "osc52"\n',
        encoding="utf-8",
    )
    cfg = config.load()
    assert cfg["theme"] == "tokyo-night"
    assert cfg["interval"] == {"jobs": 1.0, "nodes": 3.0, "partitions": 7.0}
    assert cfg["jobs"]["name_max"] == 40
    assert cfg["attach"]["enabled"] is False
    assert cfg["ui"]["expert_mode"] is True
    assert cfg["safety"]["confirm_cancel_single"] is False
    assert cfg["health"]["history_size"] == 250
    assert cfg["view_state"]["jobs_sort_col"] == "JOBID"
    assert cfg["columns"]["jobs_hidden"] == ["JOBID"]
    assert cfg["notifications"]["desktop_enabled"] is False
    assert cfg["remote"]["host"] == "login.example.org"
    assert cfg["clipboard"]["transport"] == "osc52"


# ── interval back-compat & merge ──────────────────────────────────────────────

def test_load_legacy_interval_top_level_float_back_compat(temp_config):
    cfg_file = temp_config / "config.toml"
    cfg_file.write_text("interval = 3.5\n", encoding="utf-8")
    cfg = config.load()
    assert cfg["interval"] == {"jobs": 3.5, "nodes": 3.5, "partitions": 3.5}


def test_load_interval_table_overrides_legacy_float(temp_config):
    """Table values win when present; remaining keys come from defaults.

    Note: TOML 1.0 forbids `interval = 3.5` AND `[interval]` in the same file
    (TOMLDecodeError), so we cannot literally write both shapes to disk.
    We instead exercise both arms of the loader: first the table-only path,
    then the legacy-only path. Both arms must agree that the [interval] table
    wins on a per-key basis when supplied.
    """
    cfg_file = temp_config / "config.toml"

    # Table-only path: jobs honored, others default.
    cfg_file.write_text("[interval]\njobs = 1.0\n", encoding="utf-8")
    cfg = config.load()
    assert cfg["interval"]["jobs"] == 1.0
    assert cfg["interval"]["nodes"] == 2.0
    assert cfg["interval"]["partitions"] == 5.0

    # Legacy-only path: bare float fans out.
    cfg_file.write_text("interval = 3.5\n", encoding="utf-8")
    cfg = config.load()
    assert cfg["interval"] == {"jobs": 3.5, "nodes": 3.5, "partitions": 3.5}


def test_load_interval_table_partial_fills_remaining_with_defaults(temp_config):
    cfg_file = temp_config / "config.toml"
    cfg_file.write_text("[interval]\njobs = 1.0\n", encoding="utf-8")
    cfg = config.load()
    assert cfg["interval"] == {"jobs": 1.0, "nodes": 2.0, "partitions": 5.0}


# ── columns coercion (existing behavior, pinned here) ─────────────────────────

def test_load_coerces_non_list_columns_order_to_default(temp_config):
    cfg_file = temp_config / "config.toml"
    cfg_file.write_text('[columns]\njobs_order = "not a list"\n', encoding="utf-8")
    cfg = config.load()
    assert cfg["columns"]["jobs_order"] == []


def test_load_drops_non_string_entries_in_columns_order(temp_config):
    cfg_file = temp_config / "config.toml"
    cfg_file.write_text(
        '[columns]\njobs_order = ["JOBID", 5, true]\n',
        encoding="utf-8",
    )
    cfg = config.load()
    assert cfg["columns"]["jobs_order"] == ["JOBID"]


# ── save / update ─────────────────────────────────────────────────────────────

def test_save_broadcasts_interval_float_to_all_three_keys(temp_config):
    config.save("dracula", 7.0)
    cfg = config.load()
    assert cfg["interval"] == {"jobs": 7.0, "nodes": 7.0, "partitions": 7.0}


def test_update_partial_interval_does_not_clobber_other_keys(temp_config):
    config.update({"interval": {"jobs": 1.5}})
    cfg = config.load()
    assert cfg["interval"]["jobs"] == 1.5
    assert cfg["interval"]["nodes"] == 2.0
    assert cfg["interval"]["partitions"] == 5.0


def test_update_idempotent(temp_config):
    config.update({"jobs": {"name_max": 30}})
    config.update({"jobs": {"name_max": 30}})
    cfg = config.load()
    assert cfg["jobs"]["name_max"] == 30
    # Other jobs keys remain at defaults
    assert cfg["jobs"]["user_max"] == 12
    assert cfg["jobs"]["partition_max"] == 14
    assert cfg["jobs"]["nodelist_reason_max"] == 40
    assert cfg["jobs"]["qos_max"] == 12


def test_save_leaves_non_theme_non_interval_sections_intact(temp_config):
    config.update({"safety": {"confirm_cancel_single": False}})
    config.save("nord", 4.0)
    cfg = config.load()
    assert cfg["theme"] == "nord"
    assert cfg["interval"] == {"jobs": 4.0, "nodes": 4.0, "partitions": 4.0}
    assert cfg["safety"]["confirm_cancel_single"] is False


# ── malformed values do not crash ─────────────────────────────────────────────

def test_load_malformed_health_history_size_falls_back_to_default(temp_config):
    cfg_file = temp_config / "config.toml"
    cfg_file.write_text('[health]\nhistory_size = "bad"\n', encoding="utf-8")
    # load() must not raise; current behavior is to pass the raw value through
    # (read-time coercion is intentionally minimal in PR 1).
    cfg = config.load()
    assert "health" in cfg
    # _write must also tolerate the bad value without crashing.
    config.update({"theme": "dracula"})
    cfg2 = config.load()
    assert cfg2["health"]["history_size"] == 100


# ── round-trip preservation: PR 1.5 target ────────────────────────────────────

@pytest.mark.xfail(strict=True, reason="round-trip preservation not implemented yet (PR 1.5)")
def test_xfail_round_trip_preserves_unknown_section_and_comments(temp_config):
    cfg_file = temp_config / "config.toml"
    original = (
        "# user comment line\n"
        'theme = "tokyo-night"\n'
        "\n"
        "[ui]\n"
        "show_palette_hints = false\n"
        'my_unknown_ui_key = "x"\n'
        "\n"
        "[my_custom]\n"
        'foo = "bar"\n'
    )
    cfg_file.write_text(original, encoding="utf-8")

    config.update({"safety": {"confirm_cancel_single": False}})

    rewritten = cfg_file.read_text(encoding="utf-8")
    assert "# user comment line" in rewritten
    assert "[my_custom]" in rewritten
    assert 'foo = "bar"' in rewritten
    assert "my_unknown_ui_key" in rewritten

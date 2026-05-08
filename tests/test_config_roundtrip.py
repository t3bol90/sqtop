"""Round-trip writer tests for config.py (PR 1.5).

Pin the SPEC §16.5 (atomic write), §16.6 (round-trip preservation), §16.10
(unknown-key preservation, removed-setting safety), and §18.12 (use tomlkit)
contracts. These tests exercise the tomlkit-backed writer that mutates only
the keys requested by save() / update() and writes via a same-directory
temp file plus os.replace().
"""
from __future__ import annotations

import os

import pytest

from sqtop import config


# ── atomic write ─────────────────────────────────────────────────────────────


def test_atomic_write_leaves_original_intact_on_failure(temp_config, monkeypatch):
    cfg_file = temp_config / "config.toml"
    original = (
        '# user comment\n'
        'theme = "dracula"\n'
        '\n'
        '[ui]\n'
        'expert_mode = false\n'
    )
    cfg_file.write_text(original, encoding="utf-8")

    real_replace = os.replace

    def boom(*_args, **_kwargs):
        raise OSError("boom")

    monkeypatch.setattr(config.os, "replace", boom)

    with pytest.raises(OSError):
        config.update({"theme": "tokyo-night"})

    # Original file content unchanged.
    assert cfg_file.read_text(encoding="utf-8") == original

    # No temp file leftovers in the config dir.
    leftovers = [
        p for p in temp_config.iterdir()
        if p.name.startswith(".config.") and p.name.endswith(".toml.tmp")
    ]
    assert leftovers == []

    # Restore os.replace explicitly so a follow-on test would still work
    # (monkeypatch teardown handles this; this assignment is just defensive).
    monkeypatch.setattr(config.os, "replace", real_replace)


# ── round-trip preservation ──────────────────────────────────────────────────


def test_round_trip_preserves_unknown_keys_inside_known_section(temp_config):
    cfg_file = temp_config / "config.toml"
    cfg_file.write_text(
        "[ui]\n"
        "show_palette_hints = false\n"
        'my_unknown_ui_key = "x"\n',
        encoding="utf-8",
    )

    config.update({"ui": {"expert_mode": True}})

    rewritten = cfg_file.read_text(encoding="utf-8")
    assert "my_unknown_ui_key" in rewritten
    assert 'my_unknown_ui_key = "x"' in rewritten
    assert "expert_mode = true" in rewritten
    # Pre-existing key not touched.
    assert "show_palette_hints = false" in rewritten


def test_round_trip_preserves_unknown_top_level_keys(temp_config):
    cfg_file = temp_config / "config.toml"
    cfg_file.write_text(
        'unknown_top = "yes"\n'
        "\n"
        "[safety]\n"
        "confirm_cancel_single = false\n",
        encoding="utf-8",
    )

    config.update({"safety": {"confirm_bulk_actions": False}})

    rewritten = cfg_file.read_text(encoding="utf-8")
    assert 'unknown_top = "yes"' in rewritten
    assert "confirm_cancel_single = false" in rewritten
    assert "confirm_bulk_actions = false" in rewritten


def test_update_only_modifies_requested_keys(temp_config):
    cfg_file = temp_config / "config.toml"
    cfg_file.write_text(
        "[jobs]\n"
        "name_max = 99\n"
        "user_max = 50\n",
        encoding="utf-8",
    )

    config.update({"jobs": {"name_max": 100}})

    rewritten = cfg_file.read_text(encoding="utf-8")
    assert "name_max = 100" in rewritten
    # The hand-edited user_max value MUST survive — not be silently rewritten
    # to the default (12).
    assert "user_max = 50" in rewritten
    assert "user_max = 12" not in rewritten


# ── legacy interval migration ────────────────────────────────────────────────


def test_legacy_top_level_interval_is_migrated_to_table_on_first_write(temp_config):
    cfg_file = temp_config / "config.toml"
    cfg_file.write_text(
        "interval = 3.0\n"
        'theme = "dracula"\n',
        encoding="utf-8",
    )

    config.update({"theme": "nord"})

    rewritten = cfg_file.read_text(encoding="utf-8")
    # Bare top-level scalar must be gone.
    assert "interval = 3.0" not in rewritten
    # Replaced by an [interval] table with broadcast values.
    assert "[interval]" in rewritten
    assert "jobs = 3.0" in rewritten
    assert "nodes = 3.0" in rewritten
    assert "partitions = 3.0" in rewritten
    assert 'theme = "nord"' in rewritten

    cfg = config.load()
    assert cfg["interval"] == {"jobs": 3.0, "nodes": 3.0, "partitions": 3.0}


# ── default document for new install ─────────────────────────────────────────


def test_default_document_for_new_install_loads_to_defaults(temp_config):
    cfg_file = temp_config / "config.toml"
    assert not cfg_file.exists()

    # Trigger the writer with a no-op-ish update; the default document is
    # seeded before the mutation is applied.
    config.update({"theme": "dracula"})

    cfg = config.load()
    expected = config._defaults()
    assert cfg == expected

    rewritten = cfg_file.read_text(encoding="utf-8")
    for section in [
        "[interval]",
        "[jobs]",
        "[attach]",
        "[ui]",
        "[safety]",
        "[health]",
        "[view_state]",
        "[columns]",
        "[notifications]",
        "[remote]",
        "[clipboard]",
    ]:
        assert section in rewritten, f"missing section header: {section}"


# ── removed-setting safety (SPEC §16.10) ─────────────────────────────────────


def test_save_then_update_does_not_resurrect_default_keys_user_removed(temp_config):
    """User hand-deletes confirm_bulk_actions; an unrelated update must NOT add it back."""
    cfg_file = temp_config / "config.toml"
    cfg_file.write_text(
        '# tuned by hand\n'
        'theme = "dracula"\n'
        '\n'
        '[safety]\n'
        'confirm_cancel_single = false\n',
        encoding="utf-8",
    )

    config.update({"theme": "nord"})

    rewritten = cfg_file.read_text(encoding="utf-8")
    # The user's removal MUST be respected.
    assert "confirm_bulk_actions" not in rewritten
    # Pre-existing keys still present.
    assert "confirm_cancel_single = false" in rewritten
    assert 'theme = "nord"' in rewritten
    # Comment preserved.
    assert "# tuned by hand" in rewritten

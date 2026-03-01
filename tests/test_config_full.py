"""Extended config.py tests — _toml_escape, malformed TOML, partial sections."""
from __future__ import annotations

import pytest
from sqtop import config


# ── _toml_escape ─────────────────────────────────────────────────────────────

def test_toml_escape_backslash():
    assert config._toml_escape("a\\b") == "a\\\\b"


def test_toml_escape_double_quote():
    assert config._toml_escape('a"b') == 'a\\"b'


def test_toml_escape_combined():
    """Input a\\\"b (a, backslash, quote, b) — correct TOML escaping (backslash first).

    Correct escaping (backslash first, then quote):
      step 1 — replace \\ with \\\\: a, \\\\, ", b  →  a + 2 backslashes + quote + b
      step 2 — replace " with \\": a, \\\\, \\", b  →  a + 2 backslashes + backslash-quote + b
    TOML decode: a + (\\\\ = \\) + (\\" = ") + b = a, \\, ", b  (original string restored)
    """
    result = config._toml_escape('a\\"b')  # input: a, \, ", b
    assert result == 'a\\\\\\"b'  # a + 2 backslashes + backslash-quote + b


def test_toml_escape_no_special_chars():
    assert config._toml_escape("hello world") == "hello world"


def test_toml_escape_only_backslash():
    assert config._toml_escape("\\") == "\\\\"


def test_toml_escape_only_quote():
    assert config._toml_escape('"') == '\\"'


# ── load with malformed TOML ──────────────────────────────────────────────────

def test_load_malformed_toml_returns_defaults(temp_config):
    cfg_file = temp_config / "config.toml"
    cfg_file.write_text("this is not valid toml = = =", encoding="utf-8")
    cfg = config.load()
    # Should return defaults without raising
    assert cfg["theme"] == "dracula"
    assert cfg["interval"] == 2.0
    assert "jobs" in cfg
    assert "attach" in cfg


# ── load with partial sections ────────────────────────────────────────────────

def test_load_partial_section_fills_missing_keys(temp_config):
    cfg_file = temp_config / "config.toml"
    cfg_file.write_text(
        "[jobs]\nname_max = 30\n",
        encoding="utf-8",
    )
    cfg = config.load()
    assert cfg["jobs"]["name_max"] == 30
    # Missing keys filled from defaults
    assert cfg["jobs"]["user_max"] == 12
    assert cfg["jobs"]["partition_max"] == 14
    assert cfg["jobs"]["nodelist_reason_max"] == 40


def test_load_missing_section_uses_defaults(temp_config):
    cfg_file = temp_config / "config.toml"
    cfg_file.write_text('theme = "nord"\n', encoding="utf-8")
    cfg = config.load()
    assert cfg["theme"] == "nord"
    # Missing sections filled with defaults
    assert cfg["attach"]["enabled"] is True
    assert cfg["safety"]["confirm_cancel_single"] is True


# ── update with nested override ───────────────────────────────────────────────

def test_update_nested_override(temp_config):
    config.update({"jobs": {"name_max": 50}, "theme": "tokyo-night"})
    cfg = config.load()
    assert cfg["jobs"]["name_max"] == 50
    assert cfg["theme"] == "tokyo-night"
    # Other job keys untouched
    assert cfg["jobs"]["user_max"] == 12


def test_update_top_level_key(temp_config):
    config.update({"interval": 10.0})
    cfg = config.load()
    assert cfg["interval"] == 10.0

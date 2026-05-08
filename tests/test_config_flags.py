"""Tests for SPEC §16.2 (--config flag, SQTOP_CONFIG env var) and §16.7
(Reload config palette command).
"""
from __future__ import annotations

import shutil
from pathlib import Path
from unittest.mock import patch

import pytest

from sqtop import __main__ as cli_main
from sqtop import config


# ── set_config_path() unit tests ─────────────────────────────────────────────


def test_set_config_path_redirects_load_and_write(tmp_path, monkeypatch):
    """set_config_path() must redirect both reads and writes to the new path."""
    # Save originals so we can restore them after the test even if assertions fail.
    original_dir = config._CONFIG_DIR
    original_file = config._CONFIG_FILE
    try:
        alt_path = tmp_path / "alt" / "alt-config.toml"
        config.set_config_path(alt_path)

        config.update({"theme": "tokyo-night"})

        assert alt_path.exists(), "set_config_path did not redirect writes"
        assert config.load()["theme"] == "tokyo-night"

        # Restore default and verify it points back to the XDG location.
        config.set_config_path(None)
        assert config._CONFIG_FILE == Path.home() / ".config" / "sqtop" / "config.toml"
        assert config._CONFIG_DIR == Path.home() / ".config" / "sqtop"
    finally:
        # Restore in case the test fails before the explicit reset above.
        monkeypatch.setattr(config, "_CONFIG_DIR", original_dir)
        monkeypatch.setattr(config, "_CONFIG_FILE", original_file)


def test_set_config_path_expands_user_and_resolves(monkeypatch):
    """A '~/...' path must be expanded to an absolute path."""
    original_dir = config._CONFIG_DIR
    original_file = config._CONFIG_FILE
    try:
        config.set_config_path("~/sqtop-test.toml")
        assert config._CONFIG_FILE.is_absolute()
        assert "~" not in str(config._CONFIG_FILE)
        # Expanded form should sit under the home directory.
        assert str(config._CONFIG_FILE).startswith(str(Path.home()))

        # Restore default.
        config.set_config_path(None)
        assert config._CONFIG_FILE == Path.home() / ".config" / "sqtop" / "config.toml"
    finally:
        monkeypatch.setattr(config, "_CONFIG_DIR", original_dir)
        monkeypatch.setattr(config, "_CONFIG_FILE", original_file)


# ── CLI / env precedence tests ────────────────────────────────────────────────


class _DummyApp:
    """Stand-in for SqtopApp so main() returns without launching a TUI."""

    def run(self) -> None:
        return None


def test_main_cli_config_flag_sets_path(monkeypatch):
    """--config /path causes config.set_config_path('/path')."""
    calls: list[object] = []

    monkeypatch.setattr(
        cli_main.config,
        "set_config_path",
        lambda p: calls.append(p),
    )
    monkeypatch.setattr(cli_main, "SqtopApp", _DummyApp)
    monkeypatch.setattr("sys.argv", ["sqtop", "--config", "/custom/path"])
    monkeypatch.delenv("SQTOP_CONFIG", raising=False)

    cli_main.main()
    assert calls == ["/custom/path"]


def test_main_env_var_sets_path_when_flag_absent(monkeypatch):
    """SQTOP_CONFIG=/env/path is honored when --config is not passed."""
    calls: list[object] = []

    monkeypatch.setattr(
        cli_main.config,
        "set_config_path",
        lambda p: calls.append(p),
    )
    monkeypatch.setattr(cli_main, "SqtopApp", _DummyApp)
    monkeypatch.setattr("sys.argv", ["sqtop"])
    monkeypatch.setenv("SQTOP_CONFIG", "/env/path")

    cli_main.main()
    assert calls == ["/env/path"]


def test_main_cli_flag_wins_over_env_var(monkeypatch):
    """--config beats SQTOP_CONFIG when both are present."""
    calls: list[object] = []

    monkeypatch.setattr(
        cli_main.config,
        "set_config_path",
        lambda p: calls.append(p),
    )
    monkeypatch.setattr(cli_main, "SqtopApp", _DummyApp)
    monkeypatch.setattr("sys.argv", ["sqtop", "--config", "/cli/path"])
    monkeypatch.setenv("SQTOP_CONFIG", "/env/path")

    cli_main.main()
    assert calls == ["/cli/path"]


def test_main_no_override_does_not_call_set_config_path(monkeypatch):
    """No --config and no SQTOP_CONFIG → set_config_path is never called."""
    calls: list[object] = []

    monkeypatch.setattr(
        cli_main.config,
        "set_config_path",
        lambda p: calls.append(p),
    )
    monkeypatch.setattr(cli_main, "SqtopApp", _DummyApp)
    monkeypatch.setattr("sys.argv", ["sqtop"])
    monkeypatch.delenv("SQTOP_CONFIG", raising=False)

    cli_main.main()
    assert calls == []


# ── Reload config palette command ────────────────────────────────────────────


def _make_app(width: int = 120, height: int = 30):
    """Instantiate SqtopApp with a mocked terminal size."""
    from sqtop.app import SqtopApp

    fake_size = shutil.os.terminal_size((width, height))
    with patch("sqtop.app.shutil.get_terminal_size", return_value=fake_size):
        return SqtopApp()


def _patch_notify(app):
    """Replace ``app.notify`` with a list-recording stub.

    Mirrors the signature pattern used by tests/test_investigate_screen.py.
    """
    captured: list[dict] = []

    def fake_notify(message, *, title="", severity="information", timeout=None):
        captured.append(
            {
                "message": message,
                "title": title,
                "severity": severity,
                "timeout": timeout,
            }
        )

    app.notify = fake_notify  # type: ignore[method-assign]
    return captured


@pytest.mark.asyncio
async def test_reload_config_applies_theme_and_safety_flags(temp_config):
    """_action_reload_config() must re-apply theme + ui/safety flags."""
    cfg_file = temp_config / "config.toml"
    cfg_file.write_text(
        'theme = "dracula"\n'
        "\n"
        "[ui]\n"
        "expert_mode = false\n"
        "\n"
        "[safety]\n"
        "confirm_cancel_single = true\n"
        "confirm_bulk_actions = true\n",
        encoding="utf-8",
    )

    app = _make_app(120, 30)
    async with app.run_test(size=(120, 30)) as pilot:
        await pilot.pause()
        assert pilot.app.expert_mode is False
        assert pilot.app.theme == "dracula"

        # Mutate the on-disk config; round-trip writer preserves other keys.
        config.update(
            {
                "theme": "nord",
                "ui": {"expert_mode": True},
                "safety": {
                    "confirm_cancel_single": False,
                    "confirm_bulk_actions": False,
                },
            }
        )
        await pilot.pause()

        pilot.app._action_reload_config()
        await pilot.pause()

        assert pilot.app.expert_mode is True
        assert pilot.app.confirm_cancel_single is False
        assert pilot.app.confirm_bulk_actions is False
        assert pilot.app.theme == "nord"


@pytest.mark.asyncio
async def test_reload_config_handles_load_failure_gracefully(monkeypatch, temp_config):
    """A config.load() raise must produce an error toast, not propagate."""
    app = _make_app(120, 30)
    async with app.run_test(size=(120, 30)) as pilot:
        await pilot.pause()
        captured = _patch_notify(pilot.app)

        # Patch the symbol that app.py imports as ``config``.
        from sqtop import app as app_mod

        def boom() -> dict:
            raise Exception("boom")

        monkeypatch.setattr(app_mod.config, "load", boom)

        # Must not raise.
        pilot.app._action_reload_config()
        await pilot.pause()

        errors = [c for c in captured if c["severity"] == "error"]
        assert errors, f"expected an error notification, got {captured!r}"
        assert "boom" in errors[0]["message"]
        assert errors[0]["title"] == "Config"


# ── Reload config: [interval] re-thread (PR 10) ──────────────────────────────


@pytest.mark.asyncio
async def test_reload_config_applies_new_intervals_to_all_views(temp_config):
    """_action_reload_config() must re-thread [interval] to all views."""
    cfg_file = temp_config / "config.toml"
    cfg_file.write_text(
        'theme = "dracula"\n'
        "\n"
        "[interval]\n"
        "jobs = 2.0\n"
        "nodes = 2.0\n"
        "partitions = 5.0\n",
        encoding="utf-8",
    )

    app = _make_app(120, 30)
    async with app.run_test(size=(120, 30)) as pilot:
        await pilot.pause()
        # Sanity: pre-reload intervals match what we wrote.
        assert pilot.app._intervals == {"jobs": 2.0, "nodes": 2.0, "partitions": 5.0}

        config.update({"interval": {"jobs": 1.5, "nodes": 4.0, "partitions": 8.0}})
        await pilot.pause()

        pilot.app._action_reload_config()
        await pilot.pause()

        assert pilot.app._intervals == {"jobs": 1.5, "nodes": 4.0, "partitions": 8.0}
        # App-wide default tracks the Jobs interval.
        assert pilot.app.interval == 1.5

        # Per-view private interval cache (BaseDataTableView._interval) must
        # also reflect the new values.
        from sqtop.views.jobs import JobsView
        from sqtop.views.nodes import NodesView
        from sqtop.views.partitions import PartitionsView

        assert pilot.app.query_one(JobsView)._interval == 1.5
        assert pilot.app.query_one(NodesView)._interval == 4.0
        assert pilot.app.query_one(PartitionsView)._interval == 8.0


@pytest.mark.asyncio
async def test_reload_config_skips_interval_apply_when_unchanged(monkeypatch, temp_config):
    """When the on-disk intervals match the cache, set_interval_rate must NOT be called."""
    cfg_file = temp_config / "config.toml"
    cfg_file.write_text(
        'theme = "dracula"\n'
        "\n"
        "[interval]\n"
        "jobs = 2.0\n"
        "nodes = 2.0\n"
        "partitions = 5.0\n",
        encoding="utf-8",
    )

    app = _make_app(120, 30)
    async with app.run_test(size=(120, 30)) as pilot:
        await pilot.pause()

        # Re-write the on-disk file post-mount: watch_theme fires during
        # mount and broadcasts a single interval via config.save(), which
        # overwrites partitions=5.0. We restore the intended on-disk state
        # AND pin the App-level cache to match before recording calls.
        cfg_file.write_text(
            'theme = "dracula"\n'
            "\n"
            "[interval]\n"
            "jobs = 2.0\n"
            "nodes = 2.0\n"
            "partitions = 5.0\n",
            encoding="utf-8",
        )
        pilot.app._intervals = {"jobs": 2.0, "nodes": 2.0, "partitions": 5.0}

        from sqtop.views.base import BaseDataTableView

        calls: list[float] = []
        original = BaseDataTableView.set_interval_rate

        def recording(self, interval: float) -> None:
            calls.append(interval)
            return original(self, interval)

        monkeypatch.setattr(BaseDataTableView, "set_interval_rate", recording)

        # On-disk intervals match the cache → reload must be a no-op for intervals.
        pilot.app._action_reload_config()
        await pilot.pause()

        assert calls == [], f"expected no set_interval_rate calls, got {calls!r}"


@pytest.mark.asyncio
async def test_reload_config_handles_missing_interval_section_gracefully(temp_config):
    """A config with no [interval] section must fall back to documented defaults."""
    cfg_file = temp_config / "config.toml"
    cfg_file.write_text('theme = "dracula"\n', encoding="utf-8")

    app = _make_app(120, 30)
    async with app.run_test(size=(120, 30)) as pilot:
        await pilot.pause()

        # Re-write the on-disk file post-mount to remove the [interval]
        # section that watch_theme injected during startup. This is the
        # condition we actually want to test: a hand-edited config with no
        # [interval] table at all.
        cfg_file.write_text('theme = "dracula"\n', encoding="utf-8")

        # Must not raise.
        pilot.app._action_reload_config()
        await pilot.pause()

        # Documented defaults: jobs=2.0, nodes=2.0, partitions=5.0.
        assert pilot.app._intervals == {"jobs": 2.0, "nodes": 2.0, "partitions": 5.0}


@pytest.mark.asyncio
async def test_reload_config_notify_mentions_intervals(temp_config):
    """The reload notify message must mention 'interval' when intervals changed."""
    cfg_file = temp_config / "config.toml"
    cfg_file.write_text(
        'theme = "dracula"\n'
        "\n"
        "[interval]\n"
        "jobs = 2.0\n"
        "nodes = 2.0\n"
        "partitions = 5.0\n",
        encoding="utf-8",
    )

    app = _make_app(120, 30)
    async with app.run_test(size=(120, 30)) as pilot:
        await pilot.pause()
        captured = _patch_notify(pilot.app)

        config.update({"interval": {"jobs": 1.5, "nodes": 4.0, "partitions": 8.0}})
        await pilot.pause()

        pilot.app._action_reload_config()
        await pilot.pause()

        assert any("interval" in c["message"].lower() for c in captured), (
            f"expected at least one notify with 'interval', got {captured!r}"
        )


@pytest.mark.asyncio
async def test_reload_config_with_partial_interval_dict_uses_defaults(temp_config):
    """Partial [interval] table → missing keys fall back to documented defaults."""
    cfg_file = temp_config / "config.toml"
    cfg_file.write_text(
        'theme = "dracula"\n'
        "\n"
        "[interval]\n"
        "jobs = 1.0\n",
        encoding="utf-8",
    )

    app = _make_app(120, 30)
    async with app.run_test(size=(120, 30)) as pilot:
        await pilot.pause()

        # watch_theme fires during mount and broadcasts the App's interval
        # back to disk via config.save(); rewrite the partial-section state
        # to test the actual scenario we care about.
        cfg_file.write_text(
            'theme = "dracula"\n'
            "\n"
            "[interval]\n"
            "jobs = 1.0\n",
            encoding="utf-8",
        )

        pilot.app._action_reload_config()
        await pilot.pause()

        assert pilot.app._intervals == {"jobs": 1.0, "nodes": 2.0, "partitions": 5.0}

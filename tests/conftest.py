"""Shared pytest fixtures for sqtop tests."""
from __future__ import annotations

import pytest
from sqtop import slurm, config


@pytest.fixture
def mock_run(monkeypatch):
    """Return a factory that monkeypatches slurm._run to return controlled output."""
    def factory(output: str) -> None:
        monkeypatch.setattr(slurm, "_run", lambda cmd: output)
    return factory


@pytest.fixture
def mock_run_result(monkeypatch):
    """Return a factory that monkeypatches slurm._run_result to return controlled output."""
    def factory(stdout: str = "", ok: bool = True, stderr: str = "") -> None:
        monkeypatch.setattr(slurm, "_run_result", lambda cmd: (stdout, ok, stderr))
    return factory


@pytest.fixture
def temp_config(monkeypatch, tmp_path):
    """Provide an isolated config directory so tests don't touch ~/.config/sqtop."""
    monkeypatch.setattr(config, "_CONFIG_FILE", tmp_path / "config.toml")
    monkeypatch.setattr(config, "_CONFIG_DIR", tmp_path)
    return tmp_path

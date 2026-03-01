from __future__ import annotations

from sqtop import __main__ as cli_main
from sqtop import config


def test_main_uses_remote_host_from_config(monkeypatch):
    calls: list[tuple[str, str]] = []

    monkeypatch.setattr(
        cli_main.config,
        "load",
        lambda: {"remote": {"host": "cluster-alias", "ssh_user": "ignored", "ssh_key": "ignored"}},
    )
    monkeypatch.setattr(cli_main.slurm, "set_remote", lambda host, key="": calls.append((host, key)))
    monkeypatch.setattr("sys.argv", ["sqtop"])

    class DummyApp:
        def run(self) -> None:
            return None

    monkeypatch.setattr(cli_main, "SqtopApp", DummyApp)

    cli_main.main()
    assert calls == [("cluster-alias", "")]


def test_main_cli_remote_overrides_config(monkeypatch):
    calls: list[tuple[str, str]] = []

    monkeypatch.setattr(cli_main.config, "load", lambda: {"remote": {"host": "cluster-alias"}})
    monkeypatch.setattr(cli_main.slurm, "set_remote", lambda host, key="": calls.append((host, key)))
    monkeypatch.setattr("sys.argv", ["sqtop", "--remote", "cli-alias", "--ssh-key", "~/.ssh/id_test"])

    class DummyApp:
        def run(self) -> None:
            return None

    monkeypatch.setattr(cli_main, "SqtopApp", DummyApp)

    cli_main.main()
    assert calls == [("cli-alias", "~/.ssh/id_test")]


def test_config_writer_persists_only_remote_host(temp_config):
    config.update({"remote": {"host": "cluster-alias", "ssh_user": "legacy", "ssh_key": "legacy-key"}})

    content = (temp_config / "config.toml").read_text(encoding="utf-8")
    assert '[remote]' in content
    assert 'host = "cluster-alias"' in content
    assert "ssh_user" not in content
    assert "ssh_key" not in content

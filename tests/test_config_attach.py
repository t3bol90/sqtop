from __future__ import annotations

from sqtop import config


def test_load_defaults_include_attach(monkeypatch, tmp_path):
    cfg_file = tmp_path / "config.toml"
    monkeypatch.setattr(config, "_CONFIG_FILE", cfg_file)
    monkeypatch.setattr(config, "_CONFIG_DIR", tmp_path)

    cfg = config.load()
    assert "attach" in cfg
    assert cfg["attach"]["enabled"] is True
    assert cfg["attach"]["default_command"] == "$SHELL -l"
    assert cfg["attach"]["extra_args"] == ""


def test_load_merges_partial_attach(monkeypatch, tmp_path):
    cfg_file = tmp_path / "config.toml"
    cfg_file.write_text(
        'theme = "nord"\n'
        "interval = 10.0\n\n"
        "[attach]\n"
        "enabled = false\n",
        encoding="utf-8",
    )
    monkeypatch.setattr(config, "_CONFIG_FILE", cfg_file)
    monkeypatch.setattr(config, "_CONFIG_DIR", tmp_path)

    cfg = config.load()
    assert cfg["theme"] == "nord"
    assert cfg["interval"] == 10.0
    assert cfg["attach"]["enabled"] is False
    assert cfg["attach"]["default_command"] == "$SHELL -l"
    assert cfg["attach"]["extra_args"] == ""


def test_save_preserves_attach_values(monkeypatch, tmp_path):
    cfg_file = tmp_path / "config.toml"
    cfg_file.write_text(
        'theme = "tokyo-night"\n'
        "interval = 2.0\n\n"
        "[attach]\n"
        "enabled = false\n"
        'default_command = "zsh -l"\n'
        'extra_args = "--mpi=none"\n',
        encoding="utf-8",
    )
    monkeypatch.setattr(config, "_CONFIG_FILE", cfg_file)
    monkeypatch.setattr(config, "_CONFIG_DIR", tmp_path)

    config.save("nord", 5.0)

    content = cfg_file.read_text(encoding="utf-8")
    assert 'theme = "nord"' in content
    assert "interval = 5.0" in content
    assert "[attach]" in content
    assert "enabled = false" in content
    assert 'default_command = "zsh -l"' in content
    assert 'extra_args = "--mpi=none"' in content


def test_update_merges_nested_sections(monkeypatch, tmp_path):
    cfg_file = tmp_path / "config.toml"
    monkeypatch.setattr(config, "_CONFIG_FILE", cfg_file)
    monkeypatch.setattr(config, "_CONFIG_DIR", tmp_path)

    config.update({"ui": {"expert_mode": True}, "safety": {"confirm_bulk_actions": False}})
    cfg = config.load()
    assert cfg["ui"]["expert_mode"] is True
    assert cfg["safety"]["confirm_bulk_actions"] is False
    # untouched defaults remain
    assert cfg["safety"]["confirm_cancel_single"] is True

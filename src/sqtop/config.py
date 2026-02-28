"""Persistent configuration stored in ~/.config/sqtop/config.toml."""
from __future__ import annotations

import tomllib
from pathlib import Path

_CONFIG_DIR = Path.home() / ".config" / "sqtop"
_CONFIG_FILE = _CONFIG_DIR / "config.toml"

_DEFAULTS: dict = {
    "theme": "dracula",
    "interval": 2.0,
    "jobs": {
        "name_max": 24,
        "user_max": 12,
        "partition_max": 14,
        "nodelist_reason_max": 40,
    },
    "attach": {
        "enabled": True,
        "default_command": "$SHELL -l",
        "extra_args": "",
    },
}


def _defaults() -> dict:
    return {
        "theme": _DEFAULTS["theme"],
        "interval": _DEFAULTS["interval"],
        "jobs": dict(_DEFAULTS["jobs"]),
        "attach": dict(_DEFAULTS["attach"]),
    }


def load() -> dict:
    """Return config dict, falling back to defaults on any error."""
    if not _CONFIG_FILE.exists():
        return _defaults()
    try:
        with _CONFIG_FILE.open("rb") as f:
            data = tomllib.load(f)
        cfg = _defaults()
        cfg.update({k: v for k, v in data.items() if k not in {"jobs", "attach"}})
        jobs = dict(_DEFAULTS["jobs"])
        if isinstance(data.get("jobs"), dict):
            jobs.update(data["jobs"])
        cfg["jobs"] = jobs
        attach = dict(_DEFAULTS["attach"])
        if isinstance(data.get("attach"), dict):
            attach.update(data["attach"])
        cfg["attach"] = attach
        return cfg
    except Exception:
        return _defaults()


def save(theme: str, interval: float) -> None:
    """Persist current settings to disk."""
    cfg = load()
    jobs = cfg.get("jobs", {})
    attach = cfg.get("attach", {})

    enabled = bool(attach.get("enabled", _DEFAULTS["attach"]["enabled"]))
    default_command = str(attach.get("default_command", _DEFAULTS["attach"]["default_command"]))
    extra_args = str(attach.get("extra_args", _DEFAULTS["attach"]["extra_args"]))

    _CONFIG_DIR.mkdir(parents=True, exist_ok=True)
    lines = [
        f'theme = "{theme}"',
        f"interval = {interval}",
        "",
        "[jobs]",
        f'name_max = {int(jobs.get("name_max", _DEFAULTS["jobs"]["name_max"]))}',
        f'user_max = {int(jobs.get("user_max", _DEFAULTS["jobs"]["user_max"]))}',
        f'partition_max = {int(jobs.get("partition_max", _DEFAULTS["jobs"]["partition_max"]))}',
        (
            "nodelist_reason_max = "
            f'{int(jobs.get("nodelist_reason_max", _DEFAULTS["jobs"]["nodelist_reason_max"]))}'
        ),
        "",
        "[attach]",
        f'enabled = {"true" if enabled else "false"}',
        f'default_command = "{default_command}"',
        f'extra_args = "{extra_args}"',
        "",
    ]
    _CONFIG_FILE.write_text("\n".join(lines), encoding="utf-8")

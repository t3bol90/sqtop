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
        "qos_max": 12,
    },
    "attach": {
        "enabled": True,
        "default_command": "$SHELL -l",
        "extra_args": "",
    },
    "ui": {
        "expert_mode": False,
        "show_palette_hints": True,
    },
    "safety": {
        "confirm_cancel_single": True,
        "confirm_bulk_actions": True,
    },
    "health": {
        "enabled": True,
        "history_size": 100,
        "warn_pending_ratio": 0.7,
        "warn_down_nodes": 1,
    },
    "view_state": {
        "jobs_sort_col": "",
        "jobs_sort_reversed": False,
        "nodes_sort_col": "",
        "nodes_sort_reversed": False,
        "partitions_sort_col": "",
        "partitions_sort_reversed": False,
    },
    "columns": {
        "jobs_hidden": [],
        "nodes_hidden": [],
        "partitions_hidden": [],
    },
    "notifications": {
        "desktop_enabled": True,
    },
    "remote": {
        "host": "",
    },
}


def _defaults() -> dict:
    return {
        "theme": _DEFAULTS["theme"],
        "interval": _DEFAULTS["interval"],
        "jobs": dict(_DEFAULTS["jobs"]),
        "attach": dict(_DEFAULTS["attach"]),
        "ui": dict(_DEFAULTS["ui"]),
        "safety": dict(_DEFAULTS["safety"]),
        "health": dict(_DEFAULTS["health"]),
        "view_state": dict(_DEFAULTS["view_state"]),
        "columns": {k: list(v) for k, v in _DEFAULTS["columns"].items()},
        "notifications": dict(_DEFAULTS["notifications"]),
        "remote": dict(_DEFAULTS["remote"]),
    }


def _toml_escape(value: str) -> str:
    return value.replace("\\", "\\\\").replace('"', '\\"')


def load() -> dict:
    """Return config dict, falling back to defaults on any error."""
    if not _CONFIG_FILE.exists():
        return _defaults()
    try:
        with _CONFIG_FILE.open("rb") as f:
            data = tomllib.load(f)
        cfg = _defaults()
        nested_keys = {"jobs", "attach", "ui", "safety", "health", "view_state", "columns", "notifications", "remote"}
        cfg.update({k: v for k, v in data.items() if k not in nested_keys})
        jobs = dict(_DEFAULTS["jobs"])
        if isinstance(data.get("jobs"), dict):
            jobs.update(data["jobs"])
        cfg["jobs"] = jobs
        attach = dict(_DEFAULTS["attach"])
        if isinstance(data.get("attach"), dict):
            attach.update(data["attach"])
        cfg["attach"] = attach
        ui = dict(_DEFAULTS["ui"])
        if isinstance(data.get("ui"), dict):
            ui.update(data["ui"])
        cfg["ui"] = ui
        safety = dict(_DEFAULTS["safety"])
        if isinstance(data.get("safety"), dict):
            safety.update(data["safety"])
        cfg["safety"] = safety
        health = dict(_DEFAULTS["health"])
        if isinstance(data.get("health"), dict):
            health.update(data["health"])
        cfg["health"] = health
        view_state = dict(_DEFAULTS["view_state"])
        if isinstance(data.get("view_state"), dict):
            view_state.update(data["view_state"])
        cfg["view_state"] = view_state
        columns = {k: list(v) for k, v in _DEFAULTS["columns"].items()}
        if isinstance(data.get("columns"), dict):
            for k, v in data["columns"].items():
                if isinstance(v, list):
                    columns[k] = v
        cfg["columns"] = columns
        notifications = dict(_DEFAULTS["notifications"])
        if isinstance(data.get("notifications"), dict):
            notifications.update(data["notifications"])
        cfg["notifications"] = notifications
        remote = dict(_DEFAULTS["remote"])
        if isinstance(data.get("remote"), dict):
            remote.update(data["remote"])
        cfg["remote"] = remote
        return cfg
    except Exception:
        return _defaults()


def save(theme: str, interval: float) -> None:
    """Persist theme/interval while preserving other settings."""
    cfg = load()
    cfg["theme"] = theme
    cfg["interval"] = interval
    _write(cfg)


def update(overrides: dict) -> None:
    """Update config with shallow+section merge and persist."""
    cfg = load()
    for key, value in overrides.items():
        if isinstance(value, dict) and isinstance(cfg.get(key), dict):
            cfg[key] = {**cfg[key], **value}
        else:
            cfg[key] = value
    _write(cfg)


def _toml_str_list(lst: list) -> str:
    return "[" + ", ".join(f'"{x}"' for x in lst) + "]"


def _write(cfg: dict) -> None:
    jobs = cfg.get("jobs", {})
    attach = cfg.get("attach", {})
    ui = cfg.get("ui", {})
    safety = cfg.get("safety", {})
    health = cfg.get("health", {})
    view_state = cfg.get("view_state", {})
    columns = cfg.get("columns", {})
    notifications = cfg.get("notifications", {})
    remote = cfg.get("remote", {})

    enabled = bool(attach.get("enabled", _DEFAULTS["attach"]["enabled"]))
    default_command = str(attach.get("default_command", _DEFAULTS["attach"]["default_command"]))
    extra_args = str(attach.get("extra_args", _DEFAULTS["attach"]["extra_args"]))
    expert_mode = bool(ui.get("expert_mode", _DEFAULTS["ui"]["expert_mode"]))
    show_palette_hints = bool(ui.get("show_palette_hints", _DEFAULTS["ui"]["show_palette_hints"]))
    confirm_cancel_single = bool(
        safety.get("confirm_cancel_single", _DEFAULTS["safety"]["confirm_cancel_single"])
    )
    confirm_bulk_actions = bool(
        safety.get("confirm_bulk_actions", _DEFAULTS["safety"]["confirm_bulk_actions"])
    )
    health_enabled = bool(health.get("enabled", _DEFAULTS["health"]["enabled"]))
    history_size = int(health.get("history_size", _DEFAULTS["health"]["history_size"]))
    warn_pending_ratio = float(
        health.get("warn_pending_ratio", _DEFAULTS["health"]["warn_pending_ratio"])
    )
    warn_down_nodes = int(health.get("warn_down_nodes", _DEFAULTS["health"]["warn_down_nodes"]))

    theme = str(cfg.get("theme", _DEFAULTS["theme"]))
    interval = float(cfg.get("interval", _DEFAULTS["interval"]))

    jobs_sort_col = str(view_state.get("jobs_sort_col", _DEFAULTS["view_state"]["jobs_sort_col"]))
    jobs_sort_reversed = bool(view_state.get("jobs_sort_reversed", _DEFAULTS["view_state"]["jobs_sort_reversed"]))
    nodes_sort_col = str(view_state.get("nodes_sort_col", _DEFAULTS["view_state"]["nodes_sort_col"]))
    nodes_sort_reversed = bool(view_state.get("nodes_sort_reversed", _DEFAULTS["view_state"]["nodes_sort_reversed"]))
    partitions_sort_col = str(view_state.get("partitions_sort_col", _DEFAULTS["view_state"]["partitions_sort_col"]))
    partitions_sort_reversed = bool(view_state.get("partitions_sort_reversed", _DEFAULTS["view_state"]["partitions_sort_reversed"]))

    jobs_hidden = list(columns.get("jobs_hidden", []))
    nodes_hidden = list(columns.get("nodes_hidden", []))
    partitions_hidden = list(columns.get("partitions_hidden", []))

    desktop_enabled = bool(notifications.get("desktop_enabled", _DEFAULTS["notifications"]["desktop_enabled"]))

    remote_host = str(remote.get("host", _DEFAULTS["remote"]["host"]))

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
        f'qos_max = {int(jobs.get("qos_max", _DEFAULTS["jobs"]["qos_max"]))}',
        "",
        "[attach]",
        f'enabled = {"true" if enabled else "false"}',
        f'default_command = "{_toml_escape(default_command)}"',
        f'extra_args = "{_toml_escape(extra_args)}"',
        "",
        "[ui]",
        f'expert_mode = {"true" if expert_mode else "false"}',
        f'show_palette_hints = {"true" if show_palette_hints else "false"}',
        "",
        "[safety]",
        f'confirm_cancel_single = {"true" if confirm_cancel_single else "false"}',
        f'confirm_bulk_actions = {"true" if confirm_bulk_actions else "false"}',
        "",
        "[health]",
        f'enabled = {"true" if health_enabled else "false"}',
        f"history_size = {history_size}",
        f"warn_pending_ratio = {warn_pending_ratio}",
        f"warn_down_nodes = {warn_down_nodes}",
        "",
        "[view_state]",
        f'jobs_sort_col = "{_toml_escape(jobs_sort_col)}"',
        f'jobs_sort_reversed = {"true" if jobs_sort_reversed else "false"}',
        f'nodes_sort_col = "{_toml_escape(nodes_sort_col)}"',
        f'nodes_sort_reversed = {"true" if nodes_sort_reversed else "false"}',
        f'partitions_sort_col = "{_toml_escape(partitions_sort_col)}"',
        f'partitions_sort_reversed = {"true" if partitions_sort_reversed else "false"}',
        "",
        "[columns]",
        f"jobs_hidden = {_toml_str_list(jobs_hidden)}",
        f"nodes_hidden = {_toml_str_list(nodes_hidden)}",
        f"partitions_hidden = {_toml_str_list(partitions_hidden)}",
        "",
        "[notifications]",
        f'desktop_enabled = {"true" if desktop_enabled else "false"}',
        "",
        "[remote]",
        f'host = "{_toml_escape(remote_host)}"',
        "",
    ]
    _CONFIG_FILE.write_text("\n".join(lines), encoding="utf-8")

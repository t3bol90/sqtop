"""Persistent configuration for sqtop.

Stored at ~/.config/sqtop/config.toml. The config file is the single
persistent source of truth for user preferences (SPEC §15, §18.11).

Schema:

theme: str — Textual theme name applied at startup.

[interval]
  jobs: float — auto-refresh seconds for the Jobs view.
  nodes: float — auto-refresh seconds for the Nodes view.
  partitions: float — auto-refresh seconds for the Partitions view.

[jobs]
  name_max: int — max column width for the job name.
  user_max: int — max column width for the user name.
  partition_max: int — max column width for the partition name.
  nodelist_reason_max: int — max column width for nodelist/reason.
  qos_max: int — max column width for QoS.

[attach]
  enabled: bool — whether the attach-via-srun action is offered.
  default_command: str — default command shell when attaching to a job.
  extra_args: str — extra arguments appended to the srun attach command.

[ui]
  expert_mode: bool — when true, suppress confirmation dialogs for
    destructive actions.
  show_palette_hints: bool — when true, show command palette hints.

[safety]
  confirm_cancel_single: bool — confirm before cancelling a single job.
  confirm_bulk_actions: bool — confirm before bulk actions on selections.

[health]
  enabled: bool — enable the Health view diagnostics.
  history_size: int — number of recent Slurm command records to retain.
  warn_pending_ratio: float — pending/total threshold that triggers a warn.
  warn_down_nodes: int — DOWN node count that triggers a warn.

[view_state]
  jobs_sort_col: str — last sort column for Jobs view (empty = default).
  jobs_sort_reversed: bool — last sort direction for Jobs view.
  nodes_sort_col: str — last sort column for Nodes view.
  nodes_sort_reversed: bool — last sort direction for Nodes view.
  partitions_sort_col: str — last sort column for Partitions view.
  partitions_sort_reversed: bool — last sort direction for Partitions view.

[columns]
  jobs_hidden: list[str] — hidden column names for Jobs view.
  nodes_hidden: list[str] — hidden column names for Nodes view.
  partitions_hidden: list[str] — hidden column names for Partitions view.
  jobs_order: list[str] — explicit column order for Jobs view (empty = default).
  nodes_order: list[str] — explicit column order for Nodes view.
  partitions_order: list[str] — explicit column order for Partitions view.

[notifications]
  desktop_enabled: bool — enable desktop notifications when supported.

[remote]
  host: str — default SSH host for remote mode (empty = local).

[clipboard]
  transport: str — clipboard transport: "auto", "osc52", or "subprocess".

Writes are round-trip preserving: comments, key order, unknown sections, and
unknown keys present in the on-disk file are retained when only specific keys
are mutated by save() / update(). Persisted writes are atomic via a same-
directory temp file plus os.replace().
"""
from __future__ import annotations

import os
import tempfile
import tomllib
from pathlib import Path

import tomlkit
from tomlkit import TOMLDocument
from tomlkit.items import Table

_CONFIG_DIR = Path.home() / ".config" / "sqtop"
_CONFIG_FILE = _CONFIG_DIR / "config.toml"

_DEFAULTS: dict = {
    "theme": "dracula",
    "interval": {"jobs": 2.0, "nodes": 2.0, "partitions": 5.0},
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
        "jobs_order": [],
        "nodes_order": [],
        "partitions_order": [],
    },
    "notifications": {
        "desktop_enabled": True,
    },
    "remote": {
        "host": "",
    },
    "clipboard": {
        "transport": "auto",
    },
}

# Documented section order (SPEC §16.9) plus the one-line section comments used
# when writing a fresh config for a brand-new install.
_SECTION_ORDER: list[str] = [
    "interval",
    "jobs",
    "attach",
    "ui",
    "safety",
    "health",
    "view_state",
    "columns",
    "notifications",
    "remote",
    "clipboard",
]

_SECTION_COMMENTS: dict[str, str] = {
    "interval": "Auto-refresh seconds per view.",
    "jobs": "Jobs view column width caps.",
    "attach": "Attach-via-srun behavior.",
    "ui": "UI visual behavior and confirmation toggles.",
    "safety": "Confirmation prompts for destructive actions.",
    "health": "Health view diagnostics and warning thresholds.",
    "view_state": "Persisted sort/filter state.",
    "columns": "Hidden columns and explicit column order.",
    "notifications": "Desktop notification behavior.",
    "remote": "Default SSH host for remote mode.",
    "clipboard": "Clipboard transport selection.",
}


def _defaults() -> dict:
    return {
        "theme": _DEFAULTS["theme"],
        "interval": dict(_DEFAULTS["interval"]),
        "jobs": dict(_DEFAULTS["jobs"]),
        "attach": dict(_DEFAULTS["attach"]),
        "ui": dict(_DEFAULTS["ui"]),
        "safety": dict(_DEFAULTS["safety"]),
        "health": dict(_DEFAULTS["health"]),
        "view_state": dict(_DEFAULTS["view_state"]),
        "columns": {k: list(v) for k, v in _DEFAULTS["columns"].items()},
        "notifications": dict(_DEFAULTS["notifications"]),
        "remote": dict(_DEFAULTS["remote"]),
        "clipboard": dict(_DEFAULTS["clipboard"]),
    }


def _toml_escape(value: str) -> str:
    """Escape a string for inclusion inside TOML basic-string literals.

    Retained for legacy callers and tests; tomlkit-based writes do not need
    this because tomlkit handles escaping internally.
    """
    return value.replace("\\", "\\\\").replace('"', '\\"')


def set_config_path(path: str | Path | None) -> None:
    """Override the config file path used by load/save/update.

    Pass an explicit Path to redirect; pass None to restore the default
    XDG location (~/.config/sqtop/config.toml).

    The change applies to every subsequent call. Existing in-memory state
    held by callers (e.g. SqtopApp.__init__'s cached values) is unaffected
    until they call load() again — the "Reload config" palette command
    is the documented way to re-apply state mid-session.
    """
    global _CONFIG_DIR, _CONFIG_FILE
    if path is None:
        _CONFIG_DIR = Path.home() / ".config" / "sqtop"
        _CONFIG_FILE = _CONFIG_DIR / "config.toml"
    else:
        p = Path(path).expanduser().resolve()
        _CONFIG_FILE = p
        _CONFIG_DIR = p.parent


def load() -> dict:
    """Return config dict, falling back to defaults on any error."""
    if not _CONFIG_FILE.exists():
        return _defaults()
    try:
        with _CONFIG_FILE.open("rb") as f:
            data = tomllib.load(f)
        cfg = _defaults()
        nested_keys = {
            "interval",
            "jobs",
            "attach",
            "ui",
            "safety",
            "health",
            "view_state",
            "columns",
            "notifications",
            "remote",
            "clipboard",
        }
        # Copy bare top-level keys (e.g. theme) but skip nested-table keys so
        # the legacy bare `interval = 2.0` does not overwrite the dict default.
        cfg.update({k: v for k, v in data.items() if k not in nested_keys})

        # Interval — legacy float at top level fans out to all three keys; a
        # [interval] table, when present, wins on a per-key basis.
        interval = dict(_DEFAULTS["interval"])
        legacy = data.get("interval")
        if isinstance(legacy, (int, float)) and not isinstance(legacy, bool):
            broadcast = float(legacy)
            interval = {k: broadcast for k in interval}
        if isinstance(legacy, dict):
            for k, default_v in _DEFAULTS["interval"].items():
                v = legacy.get(k, default_v)
                try:
                    interval[k] = float(v)
                except (TypeError, ValueError):
                    interval[k] = float(default_v)
        cfg["interval"] = interval

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
                    if k.endswith("_order"):
                        columns[k] = [x for x in v if isinstance(x, str)]
                    else:
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
        clipboard = dict(_DEFAULTS["clipboard"])
        if isinstance(data.get("clipboard"), dict):
            clipboard.update(data["clipboard"])
        cfg["clipboard"] = clipboard
        return cfg
    except Exception:
        return _defaults()


def save(theme: str, interval: float) -> None:
    """Persist theme and broadcast interval to all three view keys.

    The single-knob "Set refresh: Xs" UX writes the same value to jobs/nodes/
    partitions; per-view tuning happens via direct config edits or update().
    """
    secs = float(interval)
    _apply_updates_to_disk(
        {
            "theme": theme,
            "interval": {"jobs": secs, "nodes": secs, "partitions": secs},
        }
    )


def update(overrides: dict) -> None:
    """Update config with shallow+section merge and persist."""
    _apply_updates_to_disk(overrides)


# ── tomlkit round-trip writer ────────────────────────────────────────────────


def _default_document() -> TOMLDocument:
    """Build a fresh tomlkit document seeded with documented defaults."""
    doc = tomlkit.document()
    doc.add("theme", _DEFAULTS["theme"])
    for section in _SECTION_ORDER:
        comment = _SECTION_COMMENTS.get(section)
        if comment:
            doc.add(tomlkit.comment(comment))
        table = tomlkit.table()
        for key, value in _DEFAULTS[section].items():
            table.add(key, _to_tomlkit_value(value))
        doc.add(section, table)
    return doc


def _to_tomlkit_value(value):
    """Convert a plain Python value to its tomlkit equivalent.

    tomlkit accepts Python primitives directly when added to tables; this
    helper exists to normalize lists (which we want to stay inline) and to
    keep the call sites readable.
    """
    if isinstance(value, list):
        arr = tomlkit.array()
        for item in value:
            arr.append(item)
            arr.multiline(False)
        return arr
    return value


def _read_or_init_document() -> TOMLDocument:
    """Read the on-disk config as a tomlkit document, or build a default doc.

    On parse failure of an existing file, fall back to the default document
    rather than corrupting the file further. Callers about to write should
    overwrite atomically, so a malformed file becomes well-formed after the
    next save.
    """
    if _CONFIG_FILE.exists():
        try:
            text = _CONFIG_FILE.read_text(encoding="utf-8")
            return tomlkit.parse(text)
        except Exception:
            return _default_document()
    return _default_document()


def _ensure_table(doc: TOMLDocument, section: str) -> Table:
    """Return the named section as a tomlkit Table, creating it if absent."""
    existing = doc.get(section)
    if isinstance(existing, Table):
        return existing
    # Either missing or a non-table value (e.g. legacy bare scalar). Replace it
    # with a fresh table; the caller is responsible for filling in keys.
    if section in doc:
        del doc[section]
    table = tomlkit.table()
    doc.add(section, table)
    return table


def _migrate_legacy_interval(doc: TOMLDocument) -> None:
    """Promote a legacy bare top-level `interval = X` scalar into [interval].

    TOML 1.0 disallows mixing `interval = 3.0` and `[interval]` in the same
    document. When the existing file uses the legacy bare form, broadcast the
    value to all three view keys before any edit so the resulting document is
    valid TOML and matches the shape produced by save()/update().
    """
    legacy = doc.get("interval")
    if isinstance(legacy, bool) or not isinstance(legacy, (int, float)):
        return
    broadcast = float(legacy)
    del doc["interval"]
    table = tomlkit.table()
    for key in _DEFAULTS["interval"].keys():
        table.add(key, broadcast)
    doc.add("interval", table)


def _apply_section_updates(table: Table, updates: dict) -> None:
    """Merge updates into a tomlkit Table, preserving unrelated keys."""
    for key, value in updates.items():
        table[key] = _to_tomlkit_value(value)


def _apply_updates_to_disk(updates: dict) -> None:
    """Round-trip-preserving writer that mutates only the keys requested.

    Reads the existing config (or seeds a default document for new installs),
    applies the requested mutations into the matching tomlkit nodes, then
    writes the document atomically via a same-directory temp file plus
    os.replace().
    """
    doc = _read_or_init_document()
    _migrate_legacy_interval(doc)

    nested_sections = set(_SECTION_ORDER)

    for key, value in updates.items():
        if key in nested_sections and isinstance(value, dict):
            table = _ensure_table(doc, key)
            _apply_section_updates(table, value)
        else:
            # Top-level scalar (e.g. "theme") or unrecognized top-level key.
            doc[key] = _to_tomlkit_value(value)

    _atomic_write(tomlkit.dumps(doc))


def _atomic_write(text: str) -> None:
    """Write *text* to _CONFIG_FILE atomically.

    Uses a temp file in the same directory plus os.replace() so the on-disk
    file either reflects the previous contents or the new contents — never a
    partial write. On any failure the temp file is unlinked.
    """
    _CONFIG_DIR.mkdir(parents=True, exist_ok=True)
    fd, tmp_path = tempfile.mkstemp(
        prefix=".config.", suffix=".toml.tmp", dir=str(_CONFIG_DIR)
    )
    try:
        with os.fdopen(fd, "w", encoding="utf-8") as f:
            f.write(text)
            f.flush()
            os.fsync(f.fileno())
        os.replace(tmp_path, _CONFIG_FILE)
    except Exception:
        try:
            os.unlink(tmp_path)
        except OSError:
            pass
        raise

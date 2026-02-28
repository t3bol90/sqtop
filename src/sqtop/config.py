"""Persistent configuration stored in ~/.config/sqtop/config.toml."""
from __future__ import annotations

import tomllib
from pathlib import Path

_CONFIG_DIR = Path.home() / ".config" / "sqtop"
_CONFIG_FILE = _CONFIG_DIR / "config.toml"

_DEFAULTS: dict = {
    "theme": "dracula",
    "interval": 2.0,
}


def load() -> dict:
    """Return config dict, falling back to defaults on any error."""
    if not _CONFIG_FILE.exists():
        return dict(_DEFAULTS)
    try:
        with _CONFIG_FILE.open("rb") as f:
            data = tomllib.load(f)
        return {**_DEFAULTS, **data}
    except Exception:
        return dict(_DEFAULTS)


def save(theme: str, interval: float) -> None:
    """Persist current settings to disk."""
    _CONFIG_DIR.mkdir(parents=True, exist_ok=True)
    _CONFIG_FILE.write_text(
        f'theme = "{theme}"\ninterval = {interval}\n',
        encoding="utf-8",
    )

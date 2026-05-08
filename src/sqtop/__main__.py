"""Entry point for sqtop."""

import argparse
import os
from pathlib import Path

from .app import SqtopApp
from . import config, investigation, slurm


def main() -> None:
    parser = argparse.ArgumentParser(prog="sqtop", description="Slurm TUI dashboard")
    parser.add_argument(
        "--remote",
        default="",
        metavar="HOST_OR_ALIAS",
        help="Remote Slurm cluster via SSH host/alias from ~/.ssh/config",
    )
    parser.add_argument(
        "--ssh-key",
        default="",
        metavar="PATH",
        help="SSH identity file",
    )
    parser.add_argument(
        "--config",
        default="",
        metavar="PATH",
        help="Use this config file instead of ~/.config/sqtop/config.toml (also honors $SQTOP_CONFIG)",
    )
    args = parser.parse_args()

    # Config-path precedence per SPEC §16.2: --config > SQTOP_CONFIG > default.
    config_override = args.config.strip() or os.environ.get("SQTOP_CONFIG", "").strip()
    if config_override:
        config.set_config_path(config_override)

    host = args.remote.strip()
    key = args.ssh_key.strip()
    cfg: dict | None = None
    if not host:
        cfg = config.load()
        r = cfg.get("remote", {})
        host = str(r.get("host", "")).strip()
    if host:
        slurm.set_remote(host, key)

    # Site-specific pending-reason overrides (SPEC §20.3). Reuses the cfg
    # already loaded for remote resolution when possible to avoid a second
    # disk read.
    if cfg is None:
        cfg = config.load()
    reasons_path = str(cfg.get("investigation", {}).get("reasons_path", "")).strip()
    p: Path | None
    if reasons_path:
        p = Path(reasons_path).expanduser()
        if not p.is_absolute():
            p = (config._CONFIG_DIR / p).resolve()
    else:
        # Auto-discover: when no path is configured, fall back to a sibling
        # reasons.toml next to config.toml. Empty / missing = no-op. The
        # is_file() guard skips both "missing" and "is a directory" cases
        # so we never call register_user_reasons({}) on the auto-discover
        # path and clobber any state left by the explicit-path branch.
        candidate = (config._CONFIG_DIR / "reasons.toml").resolve()
        p = candidate if candidate.is_file() else None

    if p is not None:
        investigation.register_user_reasons(investigation.load_user_reasons(p))

    SqtopApp().run()


if __name__ == "__main__":
    main()

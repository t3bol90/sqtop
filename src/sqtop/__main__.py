"""Entry point for sqtop."""

import argparse
import os

from .app import SqtopApp
from . import config, slurm


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
    if not host:
        cfg = config.load()
        r = cfg.get("remote", {})
        host = str(r.get("host", "")).strip()
    if host:
        slurm.set_remote(host, key)

    SqtopApp().run()


if __name__ == "__main__":
    main()

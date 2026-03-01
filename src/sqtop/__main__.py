"""Entry point for sqtop."""

import argparse

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
    args = parser.parse_args()

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

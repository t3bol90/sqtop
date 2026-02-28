#!/bin/sh
# Run sqtop with Docker-backed Slurm shims in PATH
SQTOP_DIR="$(cd "$(dirname "$0")" && pwd)"
exec env PATH="$SQTOP_DIR/bin:$PATH" uv run sqtop "$@"

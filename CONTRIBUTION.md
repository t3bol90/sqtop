# Contribution Guide

This file describes how to run and develop `sqtop` locally.

## Prerequisites

- Python 3.11+
- `uv`
- Docker + Docker Compose

## 1. Clone and install dependencies

```bash
git clone https://github.com/t3bol90/sqtop.git
cd sqtop
uv sync
```

## 2. Start local Slurm cluster

This repo includes a local cluster helper in `slurm-cluster/`.

```bash
cd slurm-cluster
./cluster.sh up
./cluster.sh submit-test
./cluster.sh status
cd ..
```

Useful helper commands:

```bash
./slurm-cluster/cluster.sh help
./slurm-cluster/cluster.sh shell
./slurm-cluster/cluster.sh down
```

## 3. Run sqtop during development

### Option A: run against local Docker cluster shims

```bash
./run.sh
```

`run.sh` prepends `bin/` shims (`squeue`, `sinfo`, `scontrol`) that proxy into the `slurmctld` container.

### Option B: run in the project virtual environment

```bash
uv run sqtop
```

Use this if Slurm commands are already available in your host/container `PATH`.

## 4. Test and verify

Run tests:

```bash
uv run pytest
```

Basic manual verification checklist:

- Jobs tab refreshes and row selection works
- Node details open with `Enter`
- Job actions open with `Enter`
- Attach from job actions opens `srun --pty` and returns to sqtop on shell exit
- Log viewer opens for stdout/stderr
- Settings (`S`) updates theme and refresh interval
- Partitions tab (`3`) renders and sorts

## 5. Build/package checks

Install from local source to verify packaging:

```bash
uv tool install --force .
sqtop
```

## Commit conventions

Prefer small focused commits with conventional prefixes, e.g.:

- `feat: ...`
- `fix: ...`
- `docs: ...`
- `refactor: ...`

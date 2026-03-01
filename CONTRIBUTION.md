# Contributing to sqtop

This guide gets you from a fresh machine to a running development environment.
Pick the mode that matches your situation — **Docker simulation** (recommended for most contributors)
or **real Slurm cluster** (if you already have one).

---

## Prerequisites

| Tool | Install | Why |
|------|---------|-----|
| Python 3.11+ | `brew install python` or [python.org](https://python.org) | runtime |
| [uv](https://docs.astral.sh/uv/) | `brew install uv` | dependency & venv management |
| Docker Desktop | [docker.com/desktop](https://www.docker.com/products/docker-desktop/) | **Mode A only** — runs a local 4-node Slurm cluster |
| Git | built-in on macOS (or `brew install git`) | source control |

Verify everything is present:

```bash
python3 --version   # 3.11+
uv --version        # 0.x.x
docker info         # (Mode A only) should not error
git --version
```

---

## 1. Clone and install

```bash
git clone https://github.com/t3bol90/sqtop.git
cd sqtop
uv sync             # installs runtime + dev deps into .venv
```

---

## Mode A — Docker simulation (no real Slurm needed)

This is the recommended path for anyone developing on a MacBook.
A 4-node Slurm cluster runs entirely inside Docker containers.
The `bin/` directory contains shims (`squeue`, `sinfo`, `scontrol`, `scancel`, `srun`)
that transparently proxy every Slurm command into the `slurmctld` container via `docker exec`.

### A1. First-time setup

Build the Slurm image from source (one-time, ~10-15 min):

```bash
cd slurm-cluster
./cluster.sh build
cd ..
```

> Skip `build` on subsequent sessions — the image is cached by Docker.

### A2. Start the cluster

```bash
./slurm-cluster/cluster.sh up        # starts controller + 4 compute nodes
./slurm-cluster/cluster.sh status    # should show 4 nodes idle
```

### A3. Populate the queue with test jobs

```bash
./slurm-cluster/cluster.sh submit-test
# submits 5 long-running jobs (good for log-follow testing)
# and 3 quick jobs (complete in ~20s, good for efficiency/detail testing)
```

### A4. Run sqtop

```bash
./run.sh
```

`run.sh` prepends `bin/` to `PATH` so every Slurm call is silently redirected
to the Docker container — sqtop sees a real Slurm API.

### A5. Daily workflow

```bash
# Start work
./slurm-cluster/cluster.sh up
./slurm-cluster/cluster.sh submit-test
./run.sh

# During development — resubmit jobs when they finish
./slurm-cluster/cluster.sh submit-test

# Inspect the cluster directly
./slurm-cluster/cluster.sh shell       # bash into slurmctld
./slurm-cluster/cluster.sh status      # sinfo + squeue
./slurm-cluster/cluster.sh nodes       # scontrol show nodes
./slurm-cluster/cluster.sh jobs        # scontrol show jobs
./slurm-cluster/cluster.sh logs        # tail slurmctld logs

# End work
./slurm-cluster/cluster.sh down

# Full reset (clears all job history, starts fresh)
./slurm-cluster/cluster.sh clean
./slurm-cluster/cluster.sh up
```

---

## Mode B — Real Slurm cluster

Use this if `squeue`, `sinfo`, and `scontrol` are already in your `PATH`
(HPC login node, container with Slurm installed, etc.).

```bash
uv run sqtop
```

That's it. No shims, no Docker — sqtop calls the real Slurm binaries directly.

---

## 2. Run tests

Tests do not require Docker or a running cluster (they mock all subprocess calls).

```bash
uv run pytest                              # full suite
uv run pytest tests/test_slurm_actions.py  # single file
```

All tests must pass before committing.

---

## 3. Manual verification checklist

After making changes, run through this in sqtop to catch regressions:

- [ ] Jobs tab refreshes; row selection and cursor follow work
- [ ] `d` opens job detail modal instantly (no freeze for completed jobs)
- [ ] `l` opens log viewer; no visible blink every 2s on stable logs
- [ ] `Enter` on a node opens node detail
- [ ] Job actions open with `Enter`; cancel/hold/release work
- [ ] `S` opens Settings; theme and refresh interval update live
- [ ] Nodes tab (`2`) renders CPU/GPU bars
- [ ] Partitions tab (`3`) renders and sorts
- [ ] `?` shows key bindings help

---

## 4. Package sanity check

Before opening a PR, verify the package installs cleanly from source:

```bash
uv tool install --force .
sqtop --version 2>/dev/null || sqtop   # should launch
```

---

## 5. Code structure at a glance

```
src/sqtop/
├── app.py          # Textual App: tabs, bindings, interval control
├── slurm.py        # Data layer: ALL Slurm CLI calls go through here
├── config.py       # ~/.config/sqtop/config.toml load/save
└── views/
    ├── jobs.py         # Jobs tab (filter, sort, search)
    ├── nodes.py        # Nodes tab (CPU/GPU bars)
    ├── partitions.py   # Partitions tab
    ├── job_detail.py   # Job detail modal
    ├── node_detail.py  # Node detail modal
    ├── log_viewer.py   # Stdout/stderr log viewer
    ├── job_actions.py  # Job action modal
    ├── bulk_actions.py # Bulk cancel/hold
    ├── settings.py     # Settings screen
    ├── confirm.py      # Generic yes/no modal
    ├── widgets.py      # CyclicDataTable
    └── health.py       # Command-latency diagnostic view
```

Key rules when adding code:

- **All** Slurm subprocess calls go in `slurm.py`. Views never call `subprocess` directly.
- Use `CyclicDataTable` (not `DataTable`) for any new main view table.
- Long-running work (subprocess, file I/O) must run in `@work(thread=True)`;
  use `call_from_thread` to update the UI from a worker.
- New modals must follow the `ModalScreen[T]` + `push_screen(Screen, callback)` + `dismiss(value)` pattern.

---

## 6. Commit conventions

Small, focused commits with conventional prefixes:

```
feat: add GPU utilization sparklines to nodes tab
fix: prevent log viewer from clearing on identical content
refactor: extract _build_efficiency_text helper
docs: update contribution guide
chore: bump textual to 0.82
```

One concern per commit. Avoid "fix stuff" or "WIP".

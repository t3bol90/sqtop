# sqtop — Slurm TUI Dashboard

A rich, interactive TUI for monitoring Slurm clusters — like `htop`, but for SLURM.

## Features

- **Jobs tab** (`1`) — live `squeue`-style table, color-coded by job state
- **Nodes tab** (`2`) — live `sinfo`-style table with CPU utilization bars
- **Auto-refresh** every 30 seconds
- **Detail panel** — `scontrol show job/node` on selection

## Dev Setup

### 1. Start the local Slurm cluster

```bash
cd slurm-cluster
./cluster.sh up          # starts slurmctld + 4 compute nodes
./cluster.sh submit-test # submit 10 test jobs
./cluster.sh status      # sinfo + squeue
```

### 2. Install the app

```bash
uv sync
```

### 3. Run sqtop inside the controller container

```bash
# Option A: run sqtop on your host (if slurm binaries are in PATH)
uv run sqtop

# Option B: exec into the cluster and run there
docker exec -it slurm-slurmctld bash
# then inside: sqtop
```

## Keybindings

| Key | Action       |
|-----|--------------|
| `1` | Jobs tab     |
| `2` | Nodes tab    |
| `r` | Refresh now  |
| `q` | Quit         |
| `?` | Help         |

## Architecture

```
src/sqtop/
├── __main__.py      # Entry point
├── app.py           # Textual App, tabs, bindings
├── slurm.py         # Data layer: squeue/sinfo/scontrol wrappers
├── views/
│   ├── jobs.py      # Jobs DataTable view
│   ├── nodes.py     # Nodes DataTable view
│   └── detail.py    # scontrol detail panel
└── styles/
    └── app.tcss     # Textual CSS
```

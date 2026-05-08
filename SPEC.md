# sqtop — Specification

> Single-source reference for what sqtop does, how to use it, and how it works.
> Written for end users (everything up to §6) and contributors (§7 onward).

---

## Table of contents

1. [Overview](#1-overview)
2. [Install and run](#2-install-and-run)
3. [The three tabs](#3-the-three-tabs)
4. [Working with jobs](#4-working-with-jobs)
5. [Copying data](#5-copying-data)
6. [Configuration](#6-configuration)
7. [Keybindings — full reference](#7-keybindings--full-reference)
8. [Responsive design and terminal sizing](#8-responsive-design-and-terminal-sizing)
9. [Column visibility and reorder](#9-column-visibility-and-reorder)
10. [Architecture](#10-architecture)
11. [Source map](#11-source-map)
12. [Data layer (`slurm.py`)](#12-data-layer-slurmpy)
13. [View layer](#13-view-layer)
14. [Modals and screens](#14-modals-and-screens)
15. [Config internals](#15-config-internals)
16. [Testing and dev workflow](#16-testing-and-dev-workflow)

---

## 1. Overview

sqtop is a [Textual](https://textual.textualize.io/)-based TUI dashboard for Slurm clusters — "htop for SLURM." It refreshes every few seconds, lets you sort/filter/search jobs, drill into details, take actions (cancel, hold, release, requeue), and copy data into the clipboard via OSC 52 so it works over SSH.

Three tabs:

- **Jobs** — running and pending jobs, with state filter, search, multi-select, watch, and dependency view.
- **Nodes** — cluster nodes with CPU% / GPU% utilization bars and free-memory display.
- **Partitions** — partition summary (state, time limit, node count, nodelist).

Plus modals for job actions, batch script viewing, log following, job/node detail (`scontrol show ...` output), bulk operations, settings, and column visibility.

### What sqtop is not

- Not a Slurm replacement or scheduler. It calls the standard Slurm CLI tools and parses their output.
- Not a long-term monitoring system. There's no metrics database; everything is "what is true right now."
- Not a job-submission tool. Use `sbatch` from a normal shell.

---

## 2. Install and run

### Install

```bash
# from a published GitHub release
uv tool install git+https://github.com/t3bol90/sqtop.git

# from a local checkout
git clone https://github.com/t3bol90/sqtop.git
cd sqtop
uv tool install .

# upgrade later
uv tool upgrade sqtop
```

### Run

```bash
# Local — Slurm CLI in PATH
sqtop

# Remote cluster over SSH (uses ~/.ssh/config)
sqtop --remote my-cluster
sqtop --remote my-cluster --ssh-key ~/.ssh/id_ed25519

# Local Docker-backed dev cluster (this repo)
./run.sh
```

`my-cluster` is any SSH host that already works with `ssh my-cluster`. sqtop runs locally, calls Slurm commands over SSH, and renders the result locally.

### Prerequisites

- Python ≥ 3.11.
- `squeue`, `sinfo`, `scontrol`, `scancel`, `srun` available — locally in `PATH`, or remotely over SSH.
- For attach (`Enter` on a running job): `srun --pty` must work.

---

## 3. The three tabs

| Tab        | Source                            | What it shows                                        |
|------------|-----------------------------------|------------------------------------------------------|
| Jobs       | `squeue` parsed in `slurm.py`     | Live job queue: ID, state, name, user, partition, time, etc. |
| Nodes      | `sinfo` + `scontrol show node`    | Per-node state, CPU/GPU utilization bars, free memory |
| Partitions | `sinfo` summary                   | Partition name, availability, time limit, node count |

Switch with `1` / `2` / `3`. Each tab refreshes on its own interval (default 2 s, configurable via `[interval]`). Pressing `r` forces a refresh of the current tab; `P` pauses auto-refresh.

The **Jobs** tab is the primary surface and gets the most features (search, multi-select, bulk actions, watch, dependency tree, attach). Nodes and Partitions are lighter — sort and detail-drilldown only.

There is a fourth, undocumented-by-default tab — **Health** (`views/health.py`). It's a passive diagnostic readout of every Slurm CLI call sqtop has made (command, latency, ok/error). Wired out of the default `TabbedContent` but available if you re-enable it.

---

## 4. Working with jobs

### Selection model

There are **two** independent selection states on the Jobs tab:

- **Cursor row** — what's highlighted by `up`/`down`. Single-row, always one row. Used by `Enter` (open actions), `i` (info), `l` (log), etc.
- **Multi-select set** — a persistent set of job IDs marked with `Space`. Shown as a `✓` prefix. Used by bulk actions.

`Space` toggles the cursor row in the multi-select set. `*` adds all currently visible jobs. `x` clears the set. The set survives sort/filter/refresh.

Single-job actions (`Enter`, `h`, `R`, `e`, `w`) operate on the cursor row when the multi-select set is empty, and on the multi-select set when it isn't. So `h` always means "hold the relevant job(s)" without you having to choose between two keys.

### Search

`/` opens an inline search bar. Substring match across name, state, partition. Live filter — results update as you type. `Esc` closes search and keeps the filtered view; the filter clears when you submit empty or clear it.

### State filter

`f` cycles through `(none) → RUNNING → PENDING → FAILED → (none)`. Combines with search and the "my jobs" toggle.

### "My jobs" filter

`u` toggles a filter that keeps only jobs where `user == $USER`. Useful on shared clusters.

### Watch

`w` adds the cursor row to a "watched" set. Watched jobs get a `★` prefix. When a watched job transitions to a terminal state (COMPLETED / FAILED / CANCELLED / etc.), sqtop fires a desktop notification (controlled by `[notifications].desktop_enabled`).

### Visual mode

`v` enters visual selection mode (vim-style). Move with arrows or `j`/`k` to extend the range; `y` yanks the selection as TSV; `Esc` exits without yanking. `V` enters visual-line mode (same effect on data tables — selection is row-granular). See §5.

### Job-action modal

`Enter` on a job opens the actions modal:

- For **PENDING** jobs: Cancel, Hold/Release.
- For **RUNNING** jobs: Cancel, Suspend, Attach (interactive `srun --pty` into the job's compute node).
- All states: Requeue, View detail (`scontrol show job`), View batch script.

When you choose Attach, sqtop suspends, runs the attach command interactively, then resumes when you exit the shell.

### Job detail (`d`) and job info (`i`)

- `d` shows the full `scontrol show job <id>` output in a scrollable read-only pane.
- `i` shows a curated, formatted summary including job efficiency stats (when available) — CPU %, memory, exit code, etc., parsed from `sacct`.

### Log viewer (`l`)

Opens a follow-mode tail of the job's stdout (and stderr if separate). Toggle follow with `f`. Copy with `y` / `ctrl+c`.

### Dependencies (`Shift+D`)

Renders the job's dependency tree (parent/child) parsed from `scontrol show job` and `Dependency=` fields. Useful for chained workflows.

### Array tasks (`a`)

Expands a job array into its individual tasks. Each task is shown as an independent row with its own state.

---

## 5. Copying data

sqtop's primary deployment is over SSH, so the default copy mechanism is **OSC 52** — an escape sequence the local terminal emulator (on your laptop) intercepts and writes to your local clipboard. No `pbcopy`/`xclip` on the server, no X-forwarding.

### Two granularities

| Action                | Key                | What                                                      |
|-----------------------|--------------------|-----------------------------------------------------------|
| Copy job ID           | `y` (Jobs tab, no visual) | The cursor row's job ID                          |
| Copy current row      | `Shift+Y`          | Current row as TSV (Jobs tab)                            |
| Visual selection      | `v` then move then `y` | TSV of the selected rows (data tables) or the selected text (text panes) |
| Copy entire pane      | `Ctrl+Shift+Y`     | Full pane as TSV with header (data tables) or full content (text panes) |
| Copy selection (text panes) | `y` or `Ctrl+C` | Whatever is selected, or all if nothing is selected |

Pane copy uses the **post-filter, post-sort** view — what you see is what you copy. To copy unfiltered, clear filters first (`x` for selections, `Esc`/clear in search).

### tmux + SSH gotcha

If you `ssh login01` then `tmux attach`, tmux drops OSC 52 by default. Add to the **remote** `~/.tmux.conf`:

```tmux
set -g set-clipboard on
set -g allow-passthrough on   # tmux 3.3+
```

Verify end-to-end with:

```bash
printf '\e]52;c;%s\a' "$(printf 'sqtop test' | base64)"
```

If your local clipboard then contains `sqtop test`, OSC 52 is working.

### Terminal support

iTerm2, Kitty, WezTerm, Alacritty, Ghostty, GNOME Terminal (VTE ≥ 0.50), Windows Terminal — work out of the box. **Terminal.app does not support OSC 52** — switch to one of the above for SSH use. Mosh ≥ 1.4.

### Size limit

Payloads above ~74 KB are truncated to that size and a warning notification fires. The cap comes from terminal-side OSC 52 buffer limits (xterm ~75 KB, iTerm ~100 KB, tmux without `set-clipboard on` is much lower).

### Local fallback

When sqtop runs locally (no SSH) and OSC 52 fails, it falls back to `pbcopy` (macOS) → `xclip` → `xsel` → `clip` (Windows), each with a 2 s timeout. Configurable via `[clipboard].transport ∈ {"auto", "osc52", "subprocess"}`.

---

## 6. Configuration

Config lives at `~/.config/sqtop/config.toml`. It is loaded once at startup and re-loaded after the Settings screen / column-toggle modal commits changes. Direct edits while the app is running are **not** picked up — restart sqtop.

### Full default config (with explanations)

```toml
theme = "dracula"          # Textual theme name
interval = 2.0             # global refresh interval in seconds

[jobs]
# Caps for auto-sized columns. Content longer than the cap truncates with "…".
name_max = 24
user_max = 12
partition_max = 14
nodelist_reason_max = 40
qos_max = 12

[attach]
enabled = true                       # if false, the Attach option is hidden in the actions modal
default_command = "$SHELL -l"        # what runs inside srun --pty
extra_args = ""                      # extra args appended to srun

[ui]
expert_mode = false                  # if true, all confirmation dialogs are skipped
show_palette_hints = true            # show "S" hints in the footer

[safety]
confirm_cancel_single = true         # ask before scancel on a single job
confirm_bulk_actions = true          # always ask before bulk operations

[health]
enabled = true
history_size = 100                   # _COMMAND_HISTORY ring buffer size in slurm.py
warn_pending_ratio = 0.7             # show warn icon if pending/total > this
warn_down_nodes = 1                  # show warn icon if # down nodes >= this

[view_state]
# Persisted across restarts.
jobs_sort_col = ""                   # "" means default state-priority sort
jobs_sort_reversed = false
nodes_sort_col = ""
nodes_sort_reversed = false
partitions_sort_col = ""
partitions_sort_reversed = false

[columns]
# Column visibility — names dropped from the rendered tables.
jobs_hidden = []
nodes_hidden = []
partitions_hidden = []
# Column order — user-defined order overrides the default. Empty list = use default.
jobs_order = []
nodes_order = []
partitions_order = []

[notifications]
desktop_enabled = true               # macOS notifications for watched-job transitions

[remote]
host = ""                            # default remote SSH alias for sqtop --remote

[clipboard]
transport = "auto"                   # one of "auto", "osc52", "subprocess"
```

### How merge works

On load, sqtop:

1. Starts from `_DEFAULTS` (a deep copy).
2. Reads the TOML file if present.
3. **Section-merges**: every section dict is filled with defaults first, then user values overwrite per key. Unknown keys in user config are ignored. Unknown sections are dropped.
4. Coerces types defensively: lists are filtered to strings, numerics are bounded, booleans are parsed from strings ("true"/"yes"/"on" → `True`).

This means a malformed user config never crashes sqtop — it just loses the bad fields and falls back to defaults.

### Where settings come from at runtime

- `config.load()` returns the merged dict — full snapshot.
- `config.update(partial)` merges a partial dict into the file — used by Settings, ColumnToggle, sort persistence, column-order persistence.
- The `App` object exposes a few high-traffic settings as attributes (`expert_mode`, `confirm_cancel_single`, `confirm_bulk_actions`) so views read them via `getattr(self.app, ...)` rather than re-reading TOML on every render.

---

## 7. Keybindings — full reference

### Global (any tab, any view)

| Key            | Action                                                                |
|----------------|-----------------------------------------------------------------------|
| `1` / `2` / `3` | Switch to Jobs / Nodes / Partitions tab                              |
| `4`            | Switch to History tab (when present)                                  |
| `r`            | Force-refresh the current tab                                         |
| `P`            | Pause / resume auto-refresh                                           |
| `S` / `Ctrl+P` | Open command palette (refresh interval, default sort, expert mode, etc.) |
| `C`            | Open column visibility toggle for the current tab                     |
| `?`            | Show keybindings help for the current pane                            |
| `Ctrl+Shift+Y` | Copy entire current pane as TSV / full text                           |
| `q` / `Ctrl+C` | Quit                                                                  |

### Jobs tab

| Key       | Action                                                          |
|-----------|-----------------------------------------------------------------|
| `Enter`   | Open job-actions modal                                          |
| `u`       | Toggle "my jobs" filter (`user == $USER`)                       |
| `/`       | Open search bar (live substring match)                          |
| `Space`   | Toggle multi-select on cursor row                               |
| `*`       | Add all visible jobs to multi-select                            |
| `x`       | Clear multi-select                                              |
| `Shift+B` | Open bulk-actions menu                                          |
| `h`       | Hold (selection or cursor)                                      |
| `Shift+R` | Release (selection or cursor)                                   |
| `e`       | Requeue (selection or cursor)                                   |
| `s`       | Sort by state                                                   |
| `t`       | Sort by time                                                    |
| `c`       | Sort by CPUs                                                    |
| `f`       | Cycle state filter: none → RUNNING → PENDING → FAILED → none    |
| `i`       | Job info (formatted summary + efficiency)                       |
| `l`       | Log viewer (stdout/stderr tail with follow)                     |
| `d`       | Job detail (`scontrol show job` output)                         |
| `a`       | Expand array tasks                                              |
| `w`       | Toggle watch on cursor row                                      |
| `Shift+D` | Dependency tree                                                 |
| `y`       | Copy job ID (or yank visual selection)                          |
| `Shift+Y` | Copy current row as TSV                                         |
| `v` / `V` | Enter visual / visual-line mode                                 |
| `Esc`     | Exit visual mode / close search / cancel                        |
| `.`       | Cycle reorder-target column right (wraps)                       |
| `[`       | Move targeted column one slot left                              |
| `]`       | Move targeted column one slot right                             |

### Nodes tab

| Key     | Action                          |
|---------|---------------------------------|
| `Enter` | Open node detail                |
| `s`     | Sort by state                   |
| `p`     | Sort by CPU %                   |
| `m`     | Sort by free memory             |
| `v`/`V` | Visual mode                     |
| `y`     | Copy yank                       |
| `.`/`[`/`]` | Reorder-target / shift columns |

### Partitions tab

| Key | Action               |
|-----|----------------------|
| `s` | Sort by partition    |
| `n` | Sort by node count   |
| `v`/`V`/`y` | Visual / yank |

### Modals (universal)

| Key            | Action               |
|----------------|----------------------|
| `Esc`          | Close modal          |
| `q`            | Close modal (text panes / detail) |
| `y` / `Ctrl+C` | Copy selection or all (text panes) |

### Convention

App-level bindings use **uppercase** (`S`, `C`, `P`, `B`, `Y`, `R`, `D`) or `Ctrl+`. Lowercase keys are reserved for view-local actions inside the focused tab. This avoids collisions: `s` sorts the table, `S` opens settings.

`show=False` on a binding hides it from the footer (the bottom strip listing keys) but keeps it functional. `?` shows a complete list including hidden ones.

---

## 8. Responsive design and terminal sizing

sqtop adapts to terminal width across four named tiers, with a hard floor:

| Tier   | Width (cells) | Intent                                          |
|--------|---------------|-------------------------------------------------|
| floor  | < 40          | Refuse to render — show "Terminal too small"    |
| `xs`   | 40 – 79       | Narrow tmux pane, small laptop terminal         |
| `sm`   | 80 – 109      | Default 80-col terminal                         |
| `md`   | 110 – 159     | Comfortable single-monitor full-screen          |
| `lg`   | ≥ 160         | Wide-monitor full-screen                        |

### Hard requirements (none of these may break)

1. **No horizontal scrolling, ever.** Every column-width sum ≤ terminal width at all times.
2. **First-paint correctness.** No "flash of overflow" on launch — tier is computed before the first widget mounts.
3. **Continuous resize fidelity.** Slow drag-resize produces a correct frame at every intermediate width, not just at tier boundaries.
4. **All actions respect tier.** Modals, tabs, search overlay — none can produce overflowing layouts at the current width.

### Column gating per tier

| View       | Always (xs)              | + sm (≥ 80)                | + md (≥ 110)                              | + lg (≥ 160)        |
|------------|--------------------------|----------------------------|-------------------------------------------|---------------------|
| Jobs       | JOBID, NAME, STATE       | USER, TIME, TIME_LEFT      | PARTITION, QOS, NODES, CPUS, TIME_LIMIT   | NODELIST(REASON)    |
| Nodes      | NODE, STATE, CPU%        | GPU%, CPUS A/T, GPU A/T    | MEM FREE, PARTITION                       | MEM TOTAL, LOAD     |
| Partitions | PARTITION, AVAIL, STATE  | TIMELIMIT, NODES           | NODELIST                                  | —                   |
| History    | JOBID, STATE, ELAPSED    | NAME, USER, EXIT           | PARTITION                                 | —                   |

A column visible at a given tier may still be **dropped at runtime** by the budget allocator if its content overflows. See below.

### Width-budget allocator

Rather than "show all tier-eligible columns and hope they fit," sqtop runs a budget algorithm on every rebuild:

1. **Pass 1 — minimum widths.** Each visible column starts at its `min_width`. Reject if minimums alone overflow.
2. **Pass 2 — distribute slack by priority.** Remaining cells are handed out to columns in priority order (per-view, e.g. JOBID > STATE > NAME > ...), capped at each column's `content_max`.
3. **Pass 3 — drop overflow.** If the budget still doesn't fit (rare, only if `min_widths` themselves overflow), drop the lowest-priority column and retry.
4. **Pass 4 — truncate cells.** Cells longer than their assigned width get "…" truncation. Never wrap.

This guarantees `sum(column_widths) ≤ budget` at every render, on every resize event, at every tier.

### CPU% / GPU% bar shrinkage

The Nodes tab utilization bars adapt:

- `xs` — numeric only (`42%`)
- `sm` — narrow bar (5 cells)
- `md+` — full bar (10 cells)

### Modals respect the tier

Modal screens use `width: 90%; max-width: <tier-cap>` so a modal at `xs` (40 cells) is small but readable, and at `lg` (240 cells) doesn't sprawl into 200-cell whitespace.

---

## 9. Column visibility and reorder

### Visibility (`C`)

`C` opens the **Column Toggle** modal: a checkbox list of every column for the current tab. Uncheck to hide; recheck to show. Persists to `[columns].<view>_hidden` on close.

The modal also has a **"Reset to default order"** button — wipes the user-defined order back to the source declaration order. Visibility is unaffected.

### Reorder

Three mechanisms, all updating the same `_column_order` list:

| Method                  | How                                                              |
|-------------------------|------------------------------------------------------------------|
| Mouse drag-and-drop     | Press on a header, drag horizontally ≥ 2 cells, drop. Insertion marker `▌` shows the drop target. Click without drag falls through to its normal action (sort). `Esc` mid-drag cancels. |
| Keyboard                | `.` cycles a "reorder target" indicator across visible columns (header rendered in reverse video). `[` shifts the target one slot left, `]` one slot right. After a successful shift, the target follows the moved column so consecutive presses keep operating on it. |
| Reset                   | "Reset to default order" button in the column-toggle modal.      |

### Reorder semantics

- The order is **per view** (Jobs / Nodes / Partitions each have their own).
- Order persists to `[columns].<view>_order` in config.toml on every change.
- Hidden columns retain their slot in `_column_order` — show them again and they pop back into the same position.
- Keyboard shifts skip past hidden columns when locating the swap neighbor (so `[` always produces a visible change).
- Forward-compatible: a release that adds a new column appends it to the user's saved order in its source position; a release that removes a column drops it from saved order silently. No migration script.

---

## 10. Architecture

### Three-layer structure

```
slurm.py          ← data layer: every Slurm CLI call goes through here
views/*.py        ← UI layer: Textual widgets and modal screens
app.py            ← wiring: TabbedContent, app-level bindings, refresh interval
```

**Boundary rules:**

- Views never call `subprocess` directly. Every Slurm command goes through `slurm.py`.
- `slurm.py` knows nothing about Textual. It returns plain dataclasses and dicts.
- `app.py` doesn't know how a view fetches data, only how to mount and bind it.

### Data pipeline (every main view)

1. **Fetch on a worker thread.** `@work(thread=True) refresh_data()` runs the Slurm subprocess off the main thread.
2. **Hop back to main.** On completion, `self.app.call_from_thread(self._update_table, data)`.
3. **Filter, sort, render.** `_update_table(data)` applies filters/sort, updates `_last_*` cache, calls `_render_rows()`.
4. **State preservation.** `_capture_table_state()` records cursor row + scroll offset + an anchor (job_id / node name). `_restore_table_state()` restores after rebuild — anchored so the cursor tracks the same item across sort changes.

This is the same shape in `JobsView`, `NodesView`, `PartitionsView`. The duplication is intentional — three views with three subtly different filter pipelines were tried as a base class once and made things less clear, not more.

### Modal pattern

All modals inherit `ModalScreen[T]` and communicate back via `dismiss(value)` + a callback:

```python
def handle_action(action: str | None) -> None:
    if action == "cancel": ...

self.app.push_screen(JobActionScreen(job), handle_action)
```

The type parameter `T` documents what `dismiss()` should pass back. New modals **must** follow this pattern — it keeps the calling site sync and avoids leaky shared state.

### `CyclicDataTable`

A thin subclass of Textual's `DataTable` (in `views/widgets.py`) that:

1. Wraps cursor: pressing `up` on the first row jumps to the last; `down` on the last jumps to the first.
2. Adds mouse-drag column reorder — `MouseDown`/`MouseMove`/`MouseUp` handlers post a `ColumnReordered(from_index, to_index)` message when motion exceeds the drag threshold (2 cells).
3. Renders the insertion marker `▌` at the current drop boundary while dragging.

Always use `CyclicDataTable` instead of `DataTable` directly for main views.

### Command history (`_COMMAND_HISTORY`)

Every Slurm subprocess call records `(command, latency_ms, ok, stderr)` into a module-level deque (capped at `[health].history_size`). `fetch_command_health()` exposes it to the Health view. This is how the user (or operator) can spot a slow `sinfo`, a flaky `scontrol`, or sustained 10-second timeouts.

---

## 11. Source map

```
sqtop/
├── pyproject.toml                  # setuptools build, textual + rich deps
├── run.sh                          # prepends bin/ to PATH then `uv run sqtop`
├── bin/                            # Docker shims: squeue, sinfo, scontrol, scancel, srun
├── slurm-cluster/                  # 4-node Dockerized Slurm for local dev
│   ├── docker-compose.yml
│   └── cluster.sh                  # up/down/status/submit-test/shell helpers
├── tests/                          # pytest tests (no Docker required — all subprocess mocked)
├── docs/specs/                     # per-feature specs (responsive, copy, column-reorder)
├── CLAUDE.md                       # contributor agent guide
├── CONTRIBUTION.md                 # human contributor guide
├── README.md                       # short user-facing guide
├── SPEC.md                         # this document
└── src/sqtop/
    ├── app.py                      # Textual App: TabbedContent, app bindings, intervals
    ├── slurm.py                    # Data layer: ALL Slurm CLI calls
    ├── config.py                   # ~/.config/sqtop/config.toml load/merge/save
    ├── columns.py                  # Pure helpers: _reconcile_order, _move_in_order
    ├── responsive.py               # Tier definitions, allocate_columns, truncate_cell
    ├── clipboard.py                # OSC 52 + subprocess fallback
    ├── notify.py                   # Desktop notifications (macOS)
    ├── styles/app.tcss             # Textual CSS for layout
    └── views/
        ├── base.py                 # BaseDataTableView mixin (shared pipeline)
        ├── widgets.py              # CyclicDataTable
        ├── mixins.py               # VisualSelectMixin (visual mode)
        ├── jobs.py                 # Jobs tab
        ├── nodes.py                # Nodes tab
        ├── partitions.py           # Partitions tab
        ├── history.py              # History tab (sacct-backed)
        ├── health.py               # Command-latency diagnostics
        ├── job_actions.py          # JobActionScreen modal
        ├── job_detail.py           # `scontrol show job` modal
        ├── job_info.py             # Curated job summary modal
        ├── node_detail.py          # `scontrol show node` modal
        ├── batch_script.py         # Batch script viewer
        ├── log_viewer.py           # stdout/stderr tail
        ├── attach_prompt.py        # AttachNodePromptScreen modal
        ├── bulk_actions.py         # Bulk operations modal
        ├── confirm.py              # Generic Yes/No modal
        ├── column_toggle.py        # Column visibility + reset modal
        ├── dependency.py           # Dependency tree modal
        ├── array_tasks.py          # Array-task expansion modal
        ├── keybindings_help.py     # `?` overlay
        ├── settings.py             # Settings command palette
        └── detail.py               # Generic scrollable text modal
```

---

## 12. Data layer (`slurm.py`)

Every public function is callable from any view. They all return plain dataclasses (`Job`, `Node`, `ClusterSummary`, `SacctJob`, `JobDependency`) or dicts.

### Subprocess wrappers

- `_run(cmd: str) -> str` — runs a command with a 10 s timeout, returns stdout. Raises on failure.
- `_run_result(cmd: str) -> tuple[str, bool, str]` — `(stdout, ok, stderr)`. Used by everything that needs to surface errors to the user without raising.
- `_record_command(cmd, ok, latency_ms, stderr)` — appends to `_COMMAND_HISTORY` deque on every call.

### Fetchers (read-only)

- `fetch_jobs() -> list[Job]` — parses `squeue -o "<format>"`.
- `fetch_nodes() -> list[Node]` — parses `sinfo` + `scontrol show node` + GPU allocations.
- `fetch_cluster_summary() -> list[ClusterSummary]` — partition summary.
- `fetch_job_detail(job_id) -> dict[str, str]` — `scontrol show job` parsed into key/value.
- `fetch_node_detail(node_name) -> dict[str, str]` — `scontrol show node` parsed.
- `fetch_batch_script(job_id) -> str` — `scontrol write batch_script`.
- `fetch_log_paths(job_id) -> tuple[str, str]` — stdout, stderr paths from job detail.
- `tail_log_file(path, n=200) -> str` — file tail (uses subprocess `tail`).
- `fetch_job_efficiency(job_id) -> dict` — sacct-parsed efficiency stats.
- `fetch_array_tasks(job_id) -> list[Job]` — expand array job into tasks.
- `fetch_job_dependencies(job_id) -> list[JobDependency]` — parent/child tree.
- `fetch_sacct_jobs(hours=24) -> list[SacctJob]` — recent finished jobs.
- `fetch_command_health(limit=100) -> list[CommandStat]` — exposes the deque.

### Actions (mutating)

- `cancel_job(job_id) -> bool` — fire-and-forget.
- `cancel_job_result(job_id) -> tuple[bool, str]` — surfaces stderr.
- `hold_job_result`, `release_job_result`, `requeue_job_result` — same shape.
- `run_job_action(action, job_id) -> ActionResult` — dispatcher.
- `run_bulk_job_action(action, job_ids) -> list[ActionResult]` — fan-out, collects per-job results.

### Attach

- `resolve_first_node(nodelist_expr) -> str` — expands `node[01-04]` and picks the first.
- `build_attach_command(job_id, node, ...) -> list[str]` — assembles the `srun --pty` argv.
- `run_attach_command(cmd) -> int` — execv's into the shell. Used by the suspend-and-attach flow.

### Remote

- `set_remote(host, key="")` — switches every subsequent `_run` to prefix with `ssh <key-args> <host>`.

---

## 13. View layer

### `BaseDataTableView` (`views/base.py`)

Shared methods for the three main tabs:

- `start_refresh_loop()` / `pause()` / `resume()` — interval control.
- `@work(thread=True) refresh_data()` — calls `_fetch_data()` (subclass-implemented), then hops to main.
- `_capture_table_state()` / `_restore_table_state()` — cursor anchor logic.
- `_get_anchor_key(item)` — subclass returns the unique key (job_id, node name, partition name).
- `copy_pane()` — returns `(label, tsv_payload)` for `Ctrl+Shift+Y`.

### `JobsView` (`views/jobs.py`)

The richest view. Carries:

- `_last_jobs_raw`, `_last_jobs` — pre-filter and post-filter caches.
- `_last_jobs_index: dict[str, int]` — job_id → row index for fast cursor restore.
- `_filter_mine`, `_search_query`, `_filter_state` — filter state.
- `_sort_col`, `_sort_reversed` — sort state.
- `_selected_job_ids: set[str]` — multi-select.
- `_watched_states: dict[str, str]` — last seen state for each watched job (used to detect transitions).
- `_visual_active`, `_visual_anchor`, `_visual_cursor` — visual mode.
- `_column_order: list[str]`, `_hidden_cols: set[str]`, `_reorder_target_idx: int` — column UI state.

Filter pipeline order (in `_update_table`):

1. `_filter_mine` → `user == $USER`.
2. `_filter_state` → state matches the cycle-state filter.
3. `_search_query` → substring match on name / state / partition.
4. `_sort_col` + `_sort_reversed` → custom sort or the default state-priority sort (RUNNING > PENDING > others, then by job_id).

`_last_jobs_raw` always holds the unfiltered list; `_last_jobs` holds the post-filter result used for rendering and row-index lookups.

### `NodesView` (`views/nodes.py`)

Simpler. State sort, cpu% sort, mem sort. CPU and GPU bars rendered via `responsive._render_pct_bar(pct, tier)`. Free memory shown in human format ("123G", "1.2T").

### `PartitionsView` (`views/partitions.py`)

Read-only summary. Sort by partition name or node count.

### `HistoryView` (`views/history.py`)

Backed by `sacct` (instead of `squeue`). Shows finished jobs in a configurable time window (default 24 h). Has its own column set and visual mode but no actions.

---

## 14. Modals and screens

All inherit `ModalScreen[T]`. Most bind `Esc` and `q` to `dismiss(None)`.

| Screen                  | T (dismiss type)            | Purpose                                       |
|-------------------------|------------------------------|-----------------------------------------------|
| `JobActionScreen`       | `str \| None`               | "cancel" / "hold" / "release" / "requeue" / "attach" / "detail" / "info" / "log" / "script" |
| `BulkActionScreen`      | `str \| None`               | "cancel" / "hold" / "release" — applied to multi-select |
| `ConfirmScreen`         | `bool`                      | Generic yes/no                                |
| `ColumnToggleScreen`    | `tuple[str, str] \| None`   | `("reset", view_name)` on reset button, `None` on close. Visibility changes are persisted in `on_checkbox_changed`. |
| `JobInfoScreen`         | `None`                      | Curated job summary + efficiency              |
| `JobDetailScreen`       | `None`                      | `scontrol show job` text                      |
| `NodeDetailScreen`      | `None`                      | `scontrol show node` text                     |
| `BatchScriptScreen`     | `None`                      | Job batch script                              |
| `LogViewerScreen`       | `None`                      | stdout/stderr tail with follow                |
| `AttachNodePromptScreen`| `str \| None`               | Confirm + node choice for `srun --pty`        |
| `DependencyTreeScreen`  | `None`                      | Tree of parent/child jobs                     |
| `ArrayTasksScreen`      | `None`                      | Tasks of an array job                         |
| `KeybindingsHelpScreen` | `None`                      | `?` overlay                                   |
| `SettingsScreen`        | command-result tuple         | Command palette: theme, interval, default sort, expert mode, column toggle |
| `DetailView`            | `None`                      | Generic scrollable text (used by misc internals) |

### Modal sizing rules (responsive)

- `width: 90%; max-width: <tier cap>; min-width: 40` — readable at every tier without sprawl on `lg`.
- Body widgets that hold long text use `TextArea(read_only=True)` so users get free character-level selection + clipboard via `y` / `Ctrl+C`.

---

## 15. Config internals

### Layout (`src/sqtop/config.py`)

- `_DEFAULTS` — single dict with every default value. Source of truth.
- `load() -> dict` — merges file with defaults, returns the full snapshot.
- `update(partial)` — section-merges `partial` into the file. Used everywhere that needs to persist state.
- `_write(cfg)` — writes a TOML-formatted string back to disk (manual TOML emission, no third-party writer dependency — keeps the dep tree small).

### Coercion

- Booleans accept `True`, `"true"`, `"yes"`, `"on"`, `"1"`. Everything else is `False`.
- Lists are filtered to their expected element type (e.g. `*_order` keys must be lists of strings — non-strings are dropped).
- Numerics are coerced via `int()` / `float()` with bounds; out-of-bounds falls back to default.

### Sections

| Section          | Purpose                                                |
|------------------|--------------------------------------------------------|
| `[jobs]`         | Per-column max widths for auto-sized columns           |
| `[attach]`       | Attach-via-srun command and args                       |
| `[ui]`           | UI behavior flags (expert mode, palette hints)         |
| `[safety]`       | Confirmation dialog gates                              |
| `[health]`       | Command-history size, threshold for warn icons         |
| `[view_state]`   | Persisted sort column + direction per view             |
| `[columns]`      | Hidden lists + user order lists per view               |
| `[notifications]`| Desktop notification toggle                            |
| `[remote]`       | Default SSH host                                       |
| `[clipboard]`    | OSC 52 vs subprocess transport pin                     |

### Round-trip stability

Every key has a default and a coercion path. Reading a config, immediately writing it back, then re-reading is **idempotent** — covered by tests.

---

## 16. Testing and dev workflow

### Run tests

```bash
uv run pytest                                     # full suite (~1100 tests, ~2s)
uv run pytest tests/test_slurm_actions.py         # one file
uv run pytest -k "column_reorder"                 # match-name
```

Tests are entirely subprocess-mocked — they do **not** require Docker, a Slurm cluster, or network access. The test suite must be green before any commit lands.

### Test files (selected)

| File                                | Coverage                                              |
|-------------------------------------|-------------------------------------------------------|
| `test_slurm_parsing.py`             | `squeue`/`sinfo`/`scontrol` output parsing            |
| `test_slurm_actions.py`             | `cancel`/`hold`/`release`/`requeue` action plumbing   |
| `test_slurm_attach.py`              | Attach command construction, node resolution         |
| `test_config_*.py`                  | Config load, merge, round-trip                        |
| `test_responsive.py`                | Tier mapping, width budget                            |
| `test_width_budget.py`              | `allocate_columns` algorithm                          |
| `test_jobs_columns.py`              | Column gating + visibility                            |
| `test_chrome.py`                    | Footer, tab labels, header sub-title sizing           |
| `test_modal_sizing.py`              | Modal width clamps per tier                           |
| `test_too_small.py`                 | Floor handling                                        |
| `test_clipboard.py`                 | OSC 52 + subprocess transports                        |
| `test_pane_copy.py`, `test_text_pane_copy.py` | Pane-copy payloads                          |
| `test_visual_mode.py`               | Visual mode in data tables and text panes             |
| `test_modal_bindings.py`            | Esc/q close behavior across modals                    |
| `test_keybindings_help.py`          | `?` overlay contents                                   |
| `test_remote_config.py`             | `--remote` SSH plumbing                               |
| `test_column_reorder*.py`           | Column reorder helpers, JobsView, NodesView, mouse drag, ColumnToggle reset |
| `test_cyclic_table_drag.py`         | `CyclicDataTable` mouse drag                          |

### Two ways to run sqtop locally

**Mode A — Docker simulation (recommended).** A 4-node Slurm cluster runs in Docker; `bin/squeue` etc. are shims that `docker exec` into the controller.

```bash
cd slurm-cluster && ./cluster.sh build       # one-time (~10–15 min)
cd ..
./slurm-cluster/cluster.sh up                # start cluster
./slurm-cluster/cluster.sh submit-test       # populate queue
./run.sh                                      # launch sqtop with shims in PATH
```

`./slurm-cluster/cluster.sh` also has `down`, `status`, `shell`, `nodes`, `jobs`, `logs`, `clean`.

**Mode B — Real Slurm in PATH.**

```bash
uv run sqtop
```

### Manual verification checklist

After non-trivial UI changes, walk through this:

- [ ] Jobs tab refreshes; row selection and cursor follow work
- [ ] `d` opens job detail modal instantly (no freeze for completed jobs)
- [ ] `l` opens log viewer; no visible blink every 2 s on stable logs
- [ ] `Enter` on a node opens node detail
- [ ] `Enter` on a job opens job-actions; cancel/hold/release work
- [ ] `S` opens settings; theme and refresh interval update live
- [ ] Nodes tab (`2`) renders CPU/GPU bars
- [ ] Partitions tab (`3`) renders and sorts
- [ ] `?` shows key bindings help
- [ ] Resize the terminal — columns reflow without overflow at every width
- [ ] Drag a column header — insertion marker shows; drop reorders
- [ ] Press `.` then `[` / `]` — target highlight moves; column shifts

### Commit conventions

`feat:` / `fix:` / `docs:` / `refactor:` / `chore:` prefixes. Subject only; bodies optional. One concern per commit.

```
feat: add GPU utilization sparklines to nodes tab
fix: prevent log viewer from clearing on identical content
refactor: extract _build_efficiency_text helper
docs: update column-reorder spec
```

### Parallel implementation workflow

For features that decompose into independent slices, sqtop uses Claude Code agents in `isolation: "worktree"` mode. Each agent takes a slice end-to-end, commits in its own branch, and the human (or another reviewer agent) cherry-picks them onto `main` in dependency order. Conflicts are resolved by taking the better side. See `CLAUDE.md` for the full procedure.

---

## Appendix A — file paths to key behavior

When investigating a bug or planning a change, jumping to one of these is usually the fastest path:

| Behavior                                                | File:line (approx)                          |
|---------------------------------------------------------|---------------------------------------------|
| Subprocess wrappers + command history                   | `src/sqtop/slurm.py` `_run` / `_run_result` |
| Job parsing                                             | `src/sqtop/slurm.py` `fetch_jobs`           |
| Jobs filter pipeline                                    | `src/sqtop/views/jobs.py` `_update_table`   |
| Jobs render                                             | `src/sqtop/views/jobs.py` `_render_rows`    |
| Column allocation algorithm                             | `src/sqtop/responsive.py` `allocate_columns`|
| Tier definitions                                        | `src/sqtop/responsive.py` `tier_for`        |
| Mouse drag on headers                                   | `src/sqtop/views/widgets.py` `on_mouse_*`   |
| `_reconcile_order` / `_move_in_order`                   | `src/sqtop/columns.py`                      |
| Config defaults                                         | `src/sqtop/config.py` `_DEFAULTS`           |
| App-level keybindings                                   | `src/sqtop/app.py` `BINDINGS`               |
| Tab switching                                           | `src/sqtop/app.py` `action_switch_tab`      |
| Pane copy                                               | `src/sqtop/app.py` `action_copy_pane`       |
| Clipboard transports                                    | `src/sqtop/clipboard.py`                    |
| Watched-job state transitions                           | `src/sqtop/views/jobs.py` `_check_watched`  |

---

## Appendix B — design specs

The detailed design specs that drove individual features live at:

- `docs/specs/responsive.md` — tier system, width budget, modal sizing.
- `docs/specs/copy.md` — visual mode, pane copy, OSC 52, SSH/tmux details.
- `docs/specs/column-reorder.md` — column reorder mouse + keyboard, target highlight, reset button.

This SPEC.md is the integrated, user-friendly view; the per-feature specs are the design archaeology.

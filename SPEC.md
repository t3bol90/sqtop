
# sqtop — Specification

Status: Draft v2  
Audience: users, contributors, maintainers  
Purpose: Define what sqtop is, what user pain it solves, how it behaves, and how it should be implemented.

---

## Normative Language

The key words `MUST`, `MUST NOT`, `REQUIRED`, `SHOULD`, `SHOULD NOT`, `RECOMMENDED`, `MAY`, and `OPTIONAL` are to be interpreted as requirement levels.

`Implementation-defined` means the behavior is part of sqtop's implementation contract, but this specification does not prescribe one universal policy.

---

## 1. Problem Statement

sqtop is a live terminal dashboard and investigation tool for regular Slurm users.

Slurm users often need to answer simple operational questions:

- How are my jobs doing?
- Which nodes are free?
- Which GPUs, CPUs, memory, or partitions appear available?
- Why is my job pending?
- Who is using the resource I expected?
- Is my job blocked by resources, dependency, priority, partition, QoS, or node state?
- What can I safely do next?

Slurm exposes most of this information through command-line tools such as `squeue`, `sinfo`, `scontrol`, `sacct`, `scancel`, and `srun`, but the information is scattered. Users often need to manually chain commands, copy job IDs, inspect raw scheduler fields, parse pending reasons, check node state, inspect logs, and share evidence with admins.

sqtop consolidates these workflows into a responsive terminal UI.

sqtop has two primary modes:

1. **Dashboard mode**
   - Shows what is happening right now.
   - Helps users see jobs, nodes, partitions, resource availability, and their own job state.

2. **Investigation mode**
   - Helps users understand a specific job or node.
   - Answers: "Why does this job/node look like this?"
   - Combines Slurm evidence, derived observations, and safe next actions.

Important boundary:

- sqtop is for regular Slurm users, not privileged cluster admins.
- sqtop does not replace Slurm.
- sqtop does not simulate the full Slurm scheduler.
- sqtop does not bypass cluster permissions.
- sqtop surfaces what the current user can see and do through standard Slurm interfaces.

---

## 2. User Model and Permission Boundary

### 2.1 Primary User

sqtop is optimized for normal Slurm users who:

- run jobs on shared HPC, AI, research, or lab clusters
- usually connect through SSH or a login node
- have standard Slurm CLI access
- do not have sudo or Slurm admin privileges
- can inspect user-visible queue/resource state
- can act on their own jobs when Slurm permits
- may not fully understand all Slurm scheduler internals

### 2.2 User Questions

sqtop should help users answer:

- "How are my jobs?"
- "Which nodes are free?"
- "Which resources are busy?"
- "Why is my job stuck?"
- "Who is using the node/resource I expected?"
- "What does Slurm report as the reason?"
- "What evidence can I send to an admin?"
- "Can I safely cancel, hold, release, requeue, attach, or inspect logs?"

### 2.3 Permission Boundary

sqtop MUST NOT require admin privileges.

sqtop MUST NOT assume the user can:

- drain nodes
- resume nodes
- modify partitions
- modify QoS/account limits
- alter scheduler policy
- control other users' jobs
- inspect private data hidden by site policy
- bypass Slurm permissions

Permission failures are normal runtime outcomes. They MUST be displayed clearly and MUST NOT crash the app.

Examples:

- attempting to cancel another user's job may fail
- `sacct` may be disabled or restricted
- `scontrol show job` may hide fields
- job logs may be inaccessible
- SSH may fail
- node detail may be partially unavailable

---

## 3. Goals and Non-Goals

### 3.1 Goals

sqtop MUST:

- Provide live situational awareness for Slurm jobs, nodes, and partitions.
- Prioritize normal users checking their own jobs.
- Make common queue investigation faster than manually chaining Slurm commands.
- Help users understand pending/running/failed job behavior from visible Slurm evidence.
- Help users inspect resource availability: nodes, CPUs, GPUs, memory, partitions.
- Provide safe user-level actions for jobs.
- Work well over SSH.
- Copy useful data through terminal-friendly mechanisms such as OSC 52.
- Keep the UI responsive while Slurm commands run.
- Degrade gracefully when commands, fields, permissions, or Slurm versions differ.
- Centralize all Slurm command execution in the data layer.
- Preserve selection, cursor, filters, and scroll position across refresh when possible.
- Avoid horizontal overflow at supported terminal sizes.
- Treat the user config file as the single persistent source of truth for user preferences, UI settings, safety settings, view state, column state, investigation settings, remote defaults, and clipboard behavior.


### 3.2 Non-Goals

sqtop MUST NOT become:

- a Slurm replacement
- a scheduler
- a job submission system
- a privileged admin console
- a long-term metrics database
- a Grafana/Prometheus replacement
- a full Slurm scheduling simulator
- a tool for bypassing cluster permissions
- a workflow engine
- an accounting/reporting system for long-term usage analysis

sqtop MAY expose historical or accounting data from `sacct`, but it is not a durable monitoring system.

---

## 4. Product Pillars

sqtop has five product pillars.

### 4.1 Personal Job Awareness

User question:

> How are my jobs doing?

sqtop SHOULD make it easy to:

- filter to the current user's jobs
- see running, pending, failed, completed, or cancelled jobs
- search by job ID, name, state, partition, or user-visible fields
- watch jobs
- receive notification when watched jobs reach terminal states
- inspect job detail, logs, batch scripts, dependencies, and accounting data

### 4.2 Cluster Resource Visibility

User question:

> Which resources appear free or busy?

sqtop SHOULD make it easy to:

- see node states
- identify idle, allocated, mixed, down, drained, reserved, or unknown nodes
- inspect CPU/GPU/memory availability where visible
- inspect partition availability
- see whether a partition or node group appears full

### 4.3 Queue Investigation

User question:

> Why is my job waiting or behaving this way?

sqtop SHOULD make it easy to inspect:

- Slurm job state
- pending reason
- requested resources
- partition
- QoS/account fields when visible
- dependencies
- array task state
- related node state
- related partition state
- currently visible jobs using relevant nodes/resources
- raw Slurm evidence

### 4.4 Safe User-Level Action

User question:

> What can I safely do next?

sqtop SHOULD provide contextual actions such as:

- cancel
- hold
- release
- requeue
- attach
- inspect logs
- inspect batch script
- inspect raw detail
- copy/share evidence

Mutating actions MUST respect Slurm permissions and MUST display failures clearly.

### 4.5 Evidence Sharing

User question:

> What can I send to an admin or teammate?

sqtop SHOULD support:

- copy job ID
- copy current row
- copy selected rows
- copy full pane
- copy raw detail
- copy investigation report
- copy command/error evidence

Clipboard behavior SHOULD work over SSH.

---

## 5. System Overview

### 5.1 Main Surfaces

sqtop has these main user surfaces:

1. **Jobs**
   - Live job queue.
   - Primary surface for normal users.
   - Supports search, filter, sort, watch, multi-select, actions, logs, detail, dependencies, array tasks, and investigation.

2. **Nodes**
   - Live node/resource view.
   - Shows node state, CPU/GPU utilization, free memory, partition context, and node detail.
   - Supports node investigation.

3. **Partitions**
   - Partition summary.
   - Shows partition availability, state, time limit, node count, and nodelist.

4. **Investigation**
   - Contextual screen for a selected job or node.
   - Combines raw Slurm data, derived observations, likely explanations, related jobs/nodes, and suggested user-level actions.

5. **Health**
   - Diagnostic surface for Slurm command execution.
   - Shows command, latency, ok/error state, and stderr snippets.
   - Useful for identifying slow `sinfo`, failing `scontrol`, SSH issues, or command timeouts.

### 5.2 Main Components

1. `App Layer`
   - Owns Textual app lifecycle.
   - Mounts tabs and global bindings.
   - Manages refresh/pause behavior.
   - Exposes high-traffic settings to views.

2. `View Layer`
   - Owns Jobs, Nodes, Partitions, Health, modals, and investigation screens.
   - Handles rendering, filters, sorting, selection, and user interactions.
   - MUST NOT call Slurm commands directly.

3. `Data Layer`
   - Implemented primarily in `slurm.py`.
   - Owns all Slurm CLI command execution.
   - Owns local/remote command transport.
   - Owns parsing, normalization, command history, and error categorization.

4. `Domain Layer`
   - Defines normalized dataclasses and pure helpers.
   - Includes jobs, nodes, partitions, dependencies, accounting records, investigation reports, and command results.

5. `Configuration Layer`
   - Loads and writes `~/.config/sqtop/config.toml`.
   - Applies defaults.
   - Coerces malformed values defensively.
   - Persists view state and column state.

6. `Clipboard Layer`
   - Owns OSC 52 and local subprocess clipboard fallback.
   - MUST support SSH-first workflows.

7. `Responsive Layout Layer`
   - Owns terminal size tiers.
   - Owns column budget allocation.
   - MUST prevent horizontal overflow.

8. `Observability Layer`
   - Owns command history and Health view.
   - Makes slow/failing Slurm commands visible.

---

## 6. Core Domain Model

### 6.1 `Job`

A normalized Slurm job record.

Required or recommended fields:

```python
@dataclass
class Job:
    job_id: str
    name: str | None
    user: str | None
    state: str
    reason: str | None
    partition: str | None
    qos: str | None
    account: str | None
    nodes: int | None
    nodelist_or_reason: str | None
    cpus: int | None
    gpus: int | None
    memory: str | None
    time_used: str | None
    time_left: str | None
    time_limit: str | None
    submit_time: str | None
    start_time: str | None
    dependency: str | None
    priority: int | None
    array_job_id: str | None
    array_task_id: str | None
    raw: dict[str, str] | None
````

Normalization rules:

* `job_id` MUST be stable and used as the internal key.
* `state` SHOULD be normalized for comparison but displayed as Slurm reports it.
* Unknown or unavailable fields SHOULD be represented as `None`, not fake values.
* Parser failures SHOULD preserve raw command output where practical.

### 6.2 `Node`

A normalized Slurm node record.

```python
@dataclass
class Node:
    name: str
    state: str
    partitions: list[str]
    cpu_alloc: int | None
    cpu_total: int | None
    gpu_alloc: int | None
    gpu_total: int | None
    memory_free: str | None
    memory_total: str | None
    load: float | None
    features: list[str]
    gres: str | None
    reason: str | None
    raw: dict[str, str] | None
```

Normalization rules:

* Node state SHOULD be parsed into useful state categories: idle, allocated, mixed, down, drained, reserved, unknown.
* GPU/GRES parsing MUST be defensive because clusters encode GPUs differently.
* Missing GPU data MUST NOT imply zero GPUs unless Slurm explicitly reports zero.

### 6.3 `Partition`

```python
@dataclass
class Partition:
    name: str
    availability: str | None
    state: str | None
    time_limit: str | None
    node_count: int | None
    nodelist: str | None
    default: bool | None
    max_nodes: int | None
    max_time: str | None
```

### 6.4 `CommandResult`

Every Slurm command SHOULD normalize into a command result.

```python
@dataclass
class CommandResult:
    command: str
    stdout: str
    stderr: str
    exit_code: int | None
    ok: bool
    duration_ms: float
    started_at: datetime
    source: Literal["local", "remote"]
    error_category: str | None
```

This object supports:

* user-facing errors
* command history
* Health view
* tests
* partial investigation reports

### 6.5 `SelectionState`

Jobs selection has two independent states.

```python
@dataclass
class SelectionState:
    cursor_key: str | None
    selected_job_ids: set[str]
    visible_row_keys: list[str]
    anchor_key: str | None
    scroll_offset: int | None
```

Rules:

* Cursor row is the highlighted row.
* Multi-select set is persistent and independent.
* Actions operate on multi-select when non-empty.
* Actions operate on cursor row when multi-select is empty.
* Selection SHOULD survive refresh, sort, and filter when possible.

### 6.6 `WatchState`

```python
@dataclass
class WatchState:
    watched_job_ids: set[str]
    last_seen_state_by_job: dict[str, str]
    terminal_transition_events: list[WatchEvent]
```

Terminal transitions SHOULD trigger notifications when notifications are enabled.

### 6.7 `ViewState`

```python
@dataclass
class ViewState:
    current_tab: str
    search_query: str | None
    state_filter: str | None
    my_jobs_only: bool
    sort_column: str | None
    sort_reversed: bool
    hidden_columns: list[str]
    column_order: list[str]
    paused: bool
    refresh_interval_seconds: float
```

### 6.8 `InvestigationTarget`

```python
@dataclass
class InvestigationTarget:
    kind: Literal["job", "node"]
    identifier: str
    source: Literal["cursor", "typed", "related_link", "watch"]
```

### 6.9 `InvestigationReport`

```python
@dataclass
class InvestigationReport:
    target: InvestigationTarget
    generated_at: datetime
    summary: list[InvestigationItem]
    evidence: list[InvestigationEvidence]
    explanations: list[InvestigationExplanation]
    related_jobs: list[Job]
    related_nodes: list[Node]
    suggested_actions: list[InvestigationAction]
    raw_sections: dict[str, str]
    errors: list[InvestigationError]
```

### 6.10 `InvestigationEvidence`

```python
@dataclass
class InvestigationEvidence:
    id: str
    label: str
    value: str
    source: Literal["squeue", "sinfo", "scontrol", "sacct", "derived", "cache"]
    confidence: Literal["high", "medium", "low"]
```

### 6.11 `InvestigationExplanation`

```python
@dataclass
class InvestigationExplanation:
    title: str
    detail: str
    confidence: Literal["high", "medium", "low"]
    evidence_refs: list[str]
```

### 6.12 `InvestigationError`

```python
@dataclass
class InvestigationError:
    source: str
    category: str
    message: str
    stderr: str | None = None
```

---

## 7. User Workflows

### 7.1 Dashboard Workflow

User question:

> What is happening right now?

Required behavior:

* Jobs, Nodes, and Partitions refresh automatically.
* `r` forces refresh for the current view.
* `P` pauses/resumes auto-refresh.
* The UI MUST remain responsive while refresh is running.
* The user SHOULD be able to see high-level queue/resource state without opening modals.

### 7.2 Personal Job Workflow

User question:

> How are my jobs?

Required behavior:

* `u` toggles "my jobs" filter.
* Search and state filter MUST compose with "my jobs."
* Watched jobs SHOULD be visually marked.
* Watched terminal-state transitions SHOULD notify the user when enabled.
* Job detail, logs, batch script, dependency, array tasks, and investigation SHOULD be reachable from the selected job.

### 7.3 Resource Availability Workflow

User question:

> Which nodes/resources are free?

Required behavior:

* Nodes view MUST show node state.
* Nodes view SHOULD show CPU/GPU/memory availability where visible.
* Partitions view MUST show partition availability and state.
* Node detail MUST expose raw `scontrol show node`.
* Node investigation SHOULD explain whether a node appears idle, allocated, mixed, down, drained, reserved, or unknown.

### 7.4 Queue Investigation Workflow

User question:

> Why is my job stuck?

Required behavior:

* Job investigation MUST be available from a selected job.
* Job investigation SHOULD also be available by typed job ID.
* Investigation MUST show Slurm evidence before derived explanation.
* Investigation MUST NOT claim full scheduler certainty.
* Investigation SHOULD show likely explanations with confidence levels.
* Investigation SHOULD suggest safe user-level next actions.

### 7.5 Safe Action Workflow

User question:

> What can I safely do next?

Required behavior:

* Job actions MUST be contextual.
* Mutating actions MUST be explicit.
* Confirmation dialogs MUST gate dangerous actions unless disabled by config.
* Bulk actions MUST be confirmed unless explicitly disabled.
* Permission failures MUST be shown clearly.
* sqtop MUST NOT report success until the Slurm command succeeds.

### 7.6 Evidence Sharing Workflow

User question:

> What can I copy/share?

Required behavior:

* Copy job ID.
* Copy current row.
* Copy visual selection.
* Copy entire pane.
* Copy raw text pane.
* Copy investigation report.
* Use OSC 52 by default when appropriate.
* Fall back to local clipboard tools when configured and available.
* Warn on truncation.

---

## 8. Investigation Mode

Investigation Mode is a first-class workflow.

It is designed for normal Slurm users who care about a specific job ID or node state.

Investigation Mode answers:

* What does Slurm report?
* What visible evidence is relevant?
* What is likely happening?
* What remains unknown?
* What can the user safely do next?

Investigation Mode MUST be evidence-based.

It MUST NOT:

* pretend to fully simulate Slurm scheduling
* claim admin-only knowledge
* expose privileged data
* imply another user is responsible unless visible data directly supports it
* suggest admin-only actions as normal next steps

### 8.1 Entry Points

Investigation SHOULD be reachable from:

* Jobs tab selected row
* Nodes tab selected row
* command palette: "Investigate job by ID"
* command palette: "Investigate node by name"
* related links inside an investigation report

Recommended keybindings:

| Context         |          Key | Action                    |
| --------------- | -----------: | ------------------------- |
| Jobs            |          `I` | Investigate selected job  |
| Nodes           |          `I` | Investigate selected node |
| Command palette | text command | Investigate by ID/name    |

Existing `i` may remain job info. `I` is the deeper investigation workflow.

### 8.2 Investigation Screen Layout

The report SHOULD use these sections:

1. Summary
2. Slurm evidence
3. State explanation
4. Likely explanations
5. Related jobs
6. Related nodes
7. Suggested next actions
8. Raw details
9. Errors or missing evidence

The report MUST be copyable as plain text.

### 8.3 Confidence Levels

Explanations SHOULD use confidence levels:

* `high`: direct Slurm field gives a specific explanation, such as dependency or held job
* `medium`: Slurm reason plus visible resource/node/partition data supports an explanation
* `low`: sqtop lacks enough evidence; show observations only

### 8.4 Job Investigation

A job investigation starts from a selected job or explicit job ID.

Required data sources:

| Source                            | Purpose                                                        |
| --------------------------------- | -------------------------------------------------------------- |
| current Jobs table row            | live job state, reason, partition, user, basic resource fields |
| `scontrol show job <jobid>`       | raw scheduler-visible fields                                   |
| `sacct -j <jobid>` when available | accounting, exit code, elapsed time, efficiency                |
| dependency parser                 | parent/child dependency state                                  |
| Nodes snapshot                    | related node state and resource availability                   |
| Partitions snapshot               | partition context                                              |
| log paths when available          | link to logs                                                   |

Job investigation SHOULD show:

* job ID
* name
* user
* state
* pending reason
* partition
* QoS/account if visible
* requested nodes
* requested CPUs
* requested GPUs/GRES/TRES
* requested memory
* time used
* time limit
* submit time
* eligible/start time if visible
* dependency
* allocated nodes if running
* exit code if completed/failed and available

#### 8.4.1 Pending Reason Explanation

For pending jobs, investigation SHOULD explain common Slurm reasons.

Initial reason map:

| Reason                  | User-facing explanation                                                                                                    |
| ----------------------- | -------------------------------------------------------------------------------------------------------------------------- |
| `Resources`             | Slurm cannot currently find enough matching resources. Check requested CPUs/GPUs/memory, partition, and node availability. |
| `Priority`              | Job is eligible but lower priority than other queued jobs.                                                                 |
| `Dependency`            | Job is waiting for another job or condition.                                                                               |
| `ReqNodeNotAvail`       | Requested node is unavailable, drained, down, reserved, or otherwise not schedulable.                                      |
| `PartitionTimeLimit`    | Requested time exceeds partition limit.                                                                                    |
| `JobHeldUser`           | Job is held by the user.                                                                                                   |
| `JobHeldAdmin`          | Job is held by an administrator or policy.                                                                                 |
| `BeginTime`             | Job has a future begin time.                                                                                               |
| `Reservation`           | Job is waiting for reservation constraints.                                                                                |
| `Licenses`              | Required license resources are unavailable.                                                                                |
| `QOSMaxCpuPerUserLimit` | Visible QoS CPU-per-user limit may be blocking the job.                                                                    |
| `QOSMaxGRESPerUser`     | Visible QoS GRES/GPU-per-user limit may be blocking the job.                                                               |
| `AssocGrpCpuLimit`      | Association/group CPU limit may be blocking the job.                                                                       |
| `AssocGrpGRES`          | Association/group GRES/GPU limit may be blocking the job.                                                                  |

Unknown fallback:

```text
sqtop does not have a built-in explanation for this pending reason yet.
Raw Slurm reason: <reason>
```

#### 8.4.2 Resource Request Section

Investigation SHOULD display the job's requested shape.

Example:

```text
Requested resources:
- Nodes: 1
- CPUs: 16
- GPUs: 1 x a100
- Memory: 128G
- Time limit: 24:00:00
- Partition: gpu
```

If sqtop cannot parse a field, it SHOULD say unavailable rather than guessing.

#### 8.4.3 Related Nodes

For pending jobs:

* If `ReqNodeList` exists, show those nodes and state.
* If partition is known, summarize nodes in that partition.
* If GPU/GRES request is visible, highlight matching nodes where inferable.
* If nodes are unavailable, show visible reason: allocated, mixed, drained, down, reserved, unknown.

For running jobs:

* Show allocated nodes.
* Show current node state.
* Show other visible jobs on those nodes when available.

#### 8.4.4 Dependencies

If the job has dependencies, investigation SHOULD show:

```text
Dependency: afterok:12345

Dependency status:
- 12345: RUNNING

Explanation:
This job cannot start until the dependency condition is satisfied.
```

#### 8.4.5 Likely Explanation

Example:

```text
Likely explanation:
- Slurm reports reason: Resources.
- The job requests 1 GPU in partition gpu.
- No fully idle GPU node is currently visible in that partition.
- Several matching nodes are allocated or mixed.

Confidence: medium
```

#### 8.4.6 Suggested Next Actions

For jobs, suggested actions MAY include:

* watch this job
* inspect raw job detail
* inspect dependency tree
* inspect batch script
* inspect logs
* hold/release if owned by user and permitted
* cancel if owned by user and permitted
* requeue if permitted
* copy investigation report
* contact admin with copied evidence

Suggested actions MUST NOT include admin-only actions such as draining/resuming nodes or changing QoS.

### 8.5 Node Investigation

A node investigation starts from a selected node or explicit node name.

Required data sources:

| Source                      | Purpose                            |
| --------------------------- | ---------------------------------- |
| current Nodes table row     | node state, CPU/GPU/memory summary |
| `scontrol show node <node>` | raw node detail                    |
| current Jobs table          | visible jobs using the node        |
| Partitions snapshot         | partition context                  |
| command history             | diagnose stale or failed data      |

Node investigation SHOULD show:

* node name
* state
* partition(s)
* CPU allocated/total
* GPU allocated/total if visible
* memory free/total
* load
* features
* GRES
* drain/down reason if visible
* running jobs visible on that node
* related pending jobs where inferable

#### 8.5.1 Node State Explanation

Initial state map:

| State               | Explanation                                                               |
| ------------------- | ------------------------------------------------------------------------- |
| `IDLE`              | Node appears available for matching jobs.                                 |
| `ALLOCATED`         | Node is fully allocated to running jobs.                                  |
| `MIXED`             | Some resources are allocated; some may remain free.                       |
| `DOWN`              | Node is unavailable.                                                      |
| `DRAIN` / `DRAINED` | Node is being removed from scheduling or already drained.                 |
| `RESERVED`          | Node may be reserved for specific users, jobs, accounts, or reservations. |
| `UNKNOWN`           | sqtop cannot confidently classify this node state.                        |

#### 8.5.2 Jobs Using This Node

Node investigation SHOULD list visible jobs currently using the node.

Example:

```text
Jobs currently using this node:
- 12345 alice train-a100 RUNNING 1 GPU 8 CPUs
- 12346 bob preprocess RUNNING 0 GPU 16 CPUs
```

If no jobs are visible:

```text
No matching jobs are visible to sqtop. The node may still be unavailable due to reservations, drain state, hidden jobs, or cluster policy.
```

#### 8.5.3 Free Resource Estimate

sqtop SHOULD display visible free resources.

Example:

```text
Visible free resources:
- CPUs: 24 / 64
- GPUs: 2 / 4
- Memory: 320G / 512G
```

The UI SHOULD use "visible" or "reported" rather than "guaranteed schedulable."

#### 8.5.4 Related Pending Jobs

Node investigation MAY show pending jobs that appear related to the node's partition/resource shape.

Example:

```text
Pending jobs that may be waiting for this resource shape:
- 12350 you PENDING Resources gpu 1xa100
```

This feature is heuristic and MUST be labeled accordingly.

---

## 9. Slurm Integration Contract

### 9.1 Integration Boundary

sqtop integrates with Slurm through standard Slurm CLI tools.

Required or expected commands:

* `squeue`
* `sinfo`
* `scontrol`
* `sacct`
* `scancel`
* `srun`

sqtop SHOULD avoid direct Slurm database integration.

Rationale:

* Works for normal users.
* Does not require admin credentials.
* Matches commands users can run manually.
* Works across many clusters.
* Avoids Slurm database/schema coupling.

### 9.2 Data Layer Invariant

All Slurm command execution MUST go through `slurm.py` or the approved data-layer equivalent.

Views, modals, widgets, and investigation screens MUST NOT call:

* `subprocess`
* `ssh`
* Slurm commands directly

Rationale:

* centralizes parsing
* centralizes timeouts
* centralizes local/remote behavior
* centralizes command history
* centralizes error categorization
* makes tests easier

### 9.3 Required Data-Layer Operations

Read operations:

```python
fetch_jobs() -> list[Job]
fetch_nodes() -> list[Node]
fetch_cluster_summary() -> list[Partition]
fetch_job_detail(job_id: str) -> dict[str, str]
fetch_node_detail(node_name: str) -> dict[str, str]
fetch_batch_script(job_id: str) -> str
fetch_log_paths(job_id: str) -> tuple[str | None, str | None]
fetch_job_accounting(job_id: str) -> SacctJob | None
fetch_job_dependencies(job_id: str) -> list[JobDependency]
fetch_command_health() -> list[CommandResult]
```

Action operations:

```python
cancel_job(job_id: str) -> CommandResult
hold_job(job_id: str) -> CommandResult
release_job(job_id: str) -> CommandResult
requeue_job(job_id: str) -> CommandResult
suspend_job(job_id: str) -> CommandResult
attach_to_job(job_id: str, command: str) -> CommandResult
```

Investigation operations:

```python
investigate_job(job_id: str) -> InvestigationReport
investigate_node(node_name: str) -> InvestigationReport
fetch_jobs_on_node(node_name: str) -> list[Job]
```

### 9.4 Parsing Rules

Parsers MUST be defensive.

They SHOULD:

* tolerate missing fields
* tolerate extra fields
* tolerate site-specific Slurm formatting
* preserve raw output where useful
* avoid crashing on unknown state/reason values
* surface parse errors as partial data when possible

### 9.5 Command Timeout

Slurm command timeout is implementation-defined but MUST be documented.

Timeouts MUST:

* not freeze the UI
* create command history entries
* surface user-visible errors where relevant
* allow the app to continue running

---

## 10. Error Handling Contract

### 10.1 Error Categories

Recommended normalized categories:

* `slurm_command_not_found`
* `slurm_command_timeout`
* `slurm_command_failed`
* `slurm_parse_error`
* `slurm_permission_denied`
* `slurm_field_unavailable`
* `ssh_connection_failed`
* `ssh_auth_failed`
* `ssh_command_timeout`
* `job_not_found`
* `node_not_found`
* `batch_script_unavailable`
* `log_path_unavailable`
* `accounting_unavailable`
* `dependency_parse_error`
* `clipboard_unavailable`
* `clipboard_payload_truncated`
* `unsupported_terminal`
* `action_permission_denied`
* `action_failed`

### 10.2 User-Facing Error Behavior

Errors SHOULD show:

* short summary
* command/source where useful
* stderr snippet where safe/useful
* suggested next step when obvious

Errors MUST NOT crash the app during normal operation.

### 10.3 Partial Results

For dashboard and investigation workflows, partial data is better than no data.

Example:

```text
Could not fetch sacct data.
Reason: sacct is unavailable or restricted.
The rest of the investigation report is still usable.
```

---

## 11. Safety Model

### 11.1 Mutating Actions

Mutating actions include:

* cancel
* hold
* release
* requeue
* suspend
* attach when it launches an interactive command

These MUST be explicit user actions.

Passive navigation MUST NOT mutate Slurm state.

### 11.2 Confirmation Rules

By default:

* single-job cancel SHOULD require confirmation
* bulk actions MUST require confirmation
* dangerous actions SHOULD show target job IDs before execution

Config MAY allow expert users to reduce confirmations.

Bulk confirmation SHOULD remain independently configurable.

### 11.3 Cursor vs Multi-Select

Action target resolution MUST be deterministic:

1. If multi-select set is non-empty, action applies to selected jobs.
2. Otherwise action applies to cursor row.

The UI SHOULD clearly show selected jobs before bulk action execution.

### 11.4 Action Result

After a mutating command, sqtop MUST display whether Slurm accepted or rejected the action.

sqtop MUST NOT imply success until the Slurm command succeeds.

### 11.5 Other Users' Jobs

sqtop MAY show other users' jobs if Slurm exposes them.

sqtop MUST NOT assume the current user can mutate them.

Permission denied results MUST be treated as normal outcomes.

---

## 12. Remote and SSH Contract

### 12.1 Remote Mode

Remote mode means:

> sqtop UI runs locally; Slurm commands execute remotely over SSH; results are parsed and rendered locally.

This is not the same as running the full TUI on the remote host.

### 12.2 SSH Host

Remote host SHOULD use existing SSH configuration.

Examples:

```bash
sqtop --remote my-cluster
sqtop --remote my-cluster --ssh-key ~/.ssh/id_ed25519
```

### 12.3 Remote Command Behavior

Remote commands MUST:

* go through the data layer
* respect command timeout
* record command history
* surface SSH failures clearly
* not block the UI thread

### 12.4 Remote Transport Decision

Current implementation MAY use per-command SSH execution.

Future implementation MAY add persistent SSH transport.

If persistent transport is added, it MUST preserve:

* timeout behavior
* command history
* clean error categorization
* safe shutdown

---

## 13. Clipboard Contract

### 13.1 Primary Transport

sqtop SHOULD use OSC 52 by default because the primary deployment environment is SSH.

OSC 52 allows the terminal emulator on the user's local machine to receive clipboard content even when sqtop is running against a remote cluster.

### 13.2 Copy Granularity

sqtop SHOULD support:

| Action                    | Description                      |
| ------------------------- | -------------------------------- |
| Copy job ID               | selected/cursor job ID           |
| Copy current row          | row as TSV                       |
| Copy visual selection     | selected rows/text               |
| Copy full pane            | visible pane as TSV or text      |
| Copy raw detail           | raw text from detail/log screens |
| Copy investigation report | plain-text investigation report  |

### 13.3 Fallback

When local fallback is configured and OSC 52 fails, sqtop MAY try:

* `pbcopy`
* `xclip`
* `xsel`
* `clip`

Fallback commands MUST have timeouts.

### 13.4 Size Limit

Clipboard payloads above the configured/known safe size SHOULD be truncated with a warning.

The warning SHOULD tell the user that copied content was truncated.

---

## 14. Responsive UI Contract

sqtop MUST be usable in terminal environments.

### 14.1 Width Tiers

Recommended tiers:

| Tier  |     Width | Intent                    |
| ----- | --------: | ------------------------- |
| floor |    `< 40` | too small; show error     |
| `xs`  |   `40–79` | narrow terminal/tmux pane |
| `sm`  |  `80–109` | standard terminal         |
| `md`  | `110–159` | comfortable terminal      |
| `lg`  |   `>=160` | wide terminal             |

### 14.2 Hard Requirements

sqtop MUST satisfy:

1. No horizontal scrolling.
2. No first-paint overflow.
3. Correct rendering during continuous resize.
4. Modals respect current terminal width.
5. New columns must go through width-budget allocation.

### 14.3 Column Budget Algorithm

Recommended algorithm:

1. Start with tier-eligible columns.
2. Apply user-hidden columns.
3. Apply user-defined order.
4. Assign minimum widths.
5. Distribute slack by priority.
6. Drop lowest-priority columns if minimums overflow.
7. Truncate cells with ellipsis.
8. Never wrap table cells.

---

## 15. Configuration Philosophy

sqtop is config-first.

The user config file is the single persistent source of truth for sqtop behavior, similar in spirit to tools such as Vim, Bash, Zsh, and Ghostty.

All persistent user-facing settings MUST be representable in the config file. If a setting can be changed through the UI, the same setting MUST also be configurable by editing the config file directly.

The UI is allowed to edit settings, but the UI acts as a config editor, not as a separate settings store.

sqtop MUST NOT create hidden persistent preference state that cannot be inspected, edited, copied, versioned, or reset through the config file.

Examples of settings that belong in config:

- theme
- refresh interval
- my-jobs startup behavior
- column visibility
- column order
- sort preferences
- safety confirmations
- expert mode
- investigation mode behavior
- clipboard transport
- remote host defaults
- notification behavior
- health/history limits
- attach behavior

Transient runtime state does not need to be persisted in config.

Examples of transient state:

- currently highlighted row
- currently open modal
- in-flight refresh worker
- temporary command result
- current investigation report, unless explicit history/export is implemented
- current terminal size
- current scroll position, unless intentionally persisted as view state

---

## 16. Configuration Specification

### 16.1 Config-First Contract

sqtop is config-first.

The config file is the canonical persistent representation of user preferences and user-modifiable behavior.

Any setting exposed through sqtop's UI MUST map to a documented config key.

Any persistent UI modification MUST write back to the config file.

Examples:

- Changing theme in the UI updates `[ui].theme`.
- Changing refresh interval in the UI updates the relevant interval key.
- Hiding a column updates `[columns].<view>_hidden`.
- Reordering columns updates `[columns].<view>_order`.
- Changing expert mode updates `[ui].expert_mode`.
- Changing confirmation behavior updates `[safety]`.
- Changing investigation behavior updates `[investigation]`.

The UI MUST NOT persist settings only in memory, SQLite, cache files, opaque blobs, or platform-specific preference stores.

Temporary runtime state MAY remain in memory and does not need to be written to config.

### 16.2 Config Path

Default config path:

```text
~/.config/sqtop/config.toml
```

sqtop MAY support an explicit config path through a CLI flag:

```bash
sqtop --config /path/to/config.toml
```

sqtop MAY support an environment variable for config path override:

```bash
SQTOP_CONFIG=/path/to/config.toml sqtop
```

Precedence, if implemented:

```text
--config
SQTOP_CONFIG
default XDG path: ~/.config/sqtop/config.toml
```

16.3 Config File Format

The config file format is TOML.

Rationale:

readable by humans
easy to version
easy to copy between machines
familiar for modern CLI/TUI tools
supports sections clearly
works well for structured settings

The config file SHOULD remain hand-editable.

sqtop SHOULD avoid writing generated noise, unstable ordering, or unnecessary defaults into the user's config file.

16.4 Load Behavior

On startup, sqtop SHOULD:

Load built-in defaults.
Locate the config file.
Read TOML if present.
Section-merge known sections.
Ignore unknown sections and keys for forward compatibility.
Coerce values defensively.
Fall back to defaults for invalid values.
Surface config warnings without crashing.

Malformed config MUST NOT crash sqtop unless the malformed file prevents safe startup entirely.

When possible, sqtop SHOULD show config warnings in the Health view or startup notification.

16.5 Save Behavior

When sqtop writes config, it MUST preserve the config-first contract.

A UI commit SHOULD:

Load the current config file from disk.
Apply the specific setting changes.
Preserve unrelated user settings.
Preserve unknown keys where practical.
Preserve comments and formatting where practical.
Write the updated config atomically.

Atomic write means:

Write to a temporary file in the same directory.
Flush/sync where practical.
Replace the original config file.

This avoids partially written config files if sqtop crashes during save.

16.6 Round-Trip Preservation

Because the config file is user-owned, sqtop SHOULD preserve comments, ordering, and formatting when editing config from the UI.

Recommended implementation:

use a TOML library that supports round-trip editing, such as tomlkit
update only the keys that changed
avoid rewriting the entire file in a lossy way when possible

If comment-preserving writes are not implemented, sqtop MUST still preserve unknown sections and unknown keys where practical.

16.7 Runtime Reload

Direct file edits while sqtop is running SHOULD be reloadable through an explicit command.

Recommended command palette action:

Reload config

Optional future behavior:

watch the config file for changes
reload automatically when safe

If config reload fails, sqtop SHOULD keep the last known good effective config and show an error.

UI-driven settings changes MUST apply after commit and write back to the config file.

16.8 Config Schema

Recommended core sections:

Section	Purpose

[ui]	theme, hints, expert mode, visual behavior
[interval]	refresh intervals
[jobs]	job-view behavior and column width caps
[nodes]	node-view behavior
[partitions]	partition-view behavior
[investigation]	investigation mode settings
[attach]	attach-via-srun behavior
[safety]	confirmation behavior
[health]	command history and warning thresholds
[view_state]	persisted sort/filter/tab state
[columns]	hidden columns and column order
[notifications]	desktop notification behavior
[remote]	default SSH host and remote behavior
[clipboard]	OSC 52 / subprocess transport behavior

16.9 Example Config

```
theme = "dracula"

[interval]
jobs = 2.0
nodes = 2.0
partitions = 5.0

[jobs]
start_my_jobs = false
name_max = 24
user_max = 12
partition_max = 14
nodelist_reason_max = 40
qos_max = 12

[nodes]
show_gpu = true
show_memory = true

[investigation]
enabled = true
show_raw_sections = true
show_confidence = true
include_sacct = true
include_logs = true
include_related_nodes = true
include_related_jobs = true
max_related_jobs = 20

[attach]
enabled = true
default_command = "$SHELL -l"
extra_args = ""

[ui]
expert_mode = false
show_palette_hints = true

[safety]
confirm_cancel_single = true
confirm_bulk_actions = true

[health]
enabled = true
history_size = 100
warn_pending_ratio = 0.7
warn_down_nodes = 1

[view_state]
current_tab = "jobs"
jobs_sort_col = ""
jobs_sort_reversed = false
nodes_sort_col = ""
nodes_sort_reversed = false
partitions_sort_col = ""
partitions_sort_reversed = false

[columns]
jobs_hidden = []
nodes_hidden = []
partitions_hidden = []
jobs_order = []
nodes_order = []
partitions_order = []

[notifications]
desktop_enabled = true

[remote]
host = ""

[clipboard]
transport = "auto"
```
16.10 Config Compatibility

Unknown keys SHOULD be ignored.

Unknown sections SHOULD be preserved when writing config.

Removed settings SHOULD be ignored safely.

New settings SHOULD have defaults so old config files continue to work.

Config migrations MAY be added when necessary, but migrations SHOULD be conservative and SHOULD NOT destroy user comments or unknown keys.

16.11 Config and UI Relationship

The UI MUST reflect the effective config.

The UI MAY provide convenient controls for changing config values.

The UI MUST NOT invent settings that cannot be represented in config.

If a UI feature needs persistent state, the spec MUST define its config key before or during implementation.


---


## 17. View and Interaction Contract

### 17.1 Jobs View

Jobs view MUST support:

* refresh
* search
* state filter
* my-jobs filter
* sort
* cursor selection
* multi-select
* watch
* job actions
* job detail
* job info
* log viewer
* batch script viewer
* dependency view
* array task expansion
* investigation

### 17.2 Nodes View

Nodes view MUST support:

* refresh
* sort
* node detail
* node investigation
* CPU visibility where available
* GPU visibility where available
* memory visibility where available
* state visibility

Nodes view SHOULD support filtering in a future release:

* idle only
* allocated only
* mixed only
* down/drained only
* GPU nodes only

### 17.3 Partitions View

Partitions view MUST support:

* refresh
* sort
* partition state
* partition time limit
* node count
* nodelist where available

Partitions view MAY support future investigation:

* partition pressure summary
* pending jobs by partition
* node availability by partition

### 17.4 Investigation Screen

Investigation screen MUST:

* render partial reports
* show errors without crashing
* allow copy report
* expose raw Slurm evidence
* show confidence for derived explanations
* suggest safe user-level actions

---

## 18. Technical Decisions

### 18.1 Slurm CLI as Integration Boundary

Decision:

> sqtop uses standard Slurm CLI commands as its integration boundary.

Rationale:

* works for non-admin users
* avoids privileged dependencies
* mirrors manual user workflows
* avoids direct database coupling
* works over SSH

Consequence:

* parsing must be defensive
* cluster/site differences must be expected
* missing fields are normal

### 18.2 `slurm.py` Owns Slurm Commands

Decision:

> All Slurm command execution belongs in `slurm.py` or an equivalent data-layer module.

Rationale:

* keeps UI pure
* improves testability
* centralizes remote behavior
* centralizes command history
* centralizes error handling

### 18.3 Dashboard First, Investigation Second

Decision:

> sqtop opens as a dashboard, then lets users investigate specific jobs/nodes.

Rationale:

* users often first want situational awareness
* investigation usually starts after noticing a problem
* this preserves the htop-like mental model

### 18.4 Investigation Is Evidence-Based

Decision:

> Investigation Mode shows evidence and likely explanations, not absolute scheduler truth.

Rationale:

* normal users may lack full scheduler visibility
* Slurm scheduling can depend on hidden policy/account/QoS/reservation state
* overclaiming would reduce trust

### 18.5 Nodes Are Core, Not Secondary

Decision:

> Jobs remains the primary surface, but Nodes is core to the resource-availability workflow.

Rationale:

* users need to know which node is free
* users need to understand who is using resources
* node state is essential for job-pending investigation

### 18.6 My-Jobs Is a First-Class Mode

Decision:

> sqtop should bias toward the current user's jobs.

Rationale:

* target user is a normal Slurm user
* most actions apply only to the user's own jobs
* shared clusters can have noisy queues

Future option:

```toml
[jobs]
start_my_jobs = true
```

### 18.7 Permission Failures Are Normal UX

Decision:

> Permission failures are not app bugs.

Rationale:

* users are not admins
* Slurm site policy varies
* access differs by command, job, node, and accounting configuration

### 18.8 Remote Mode Is Command Transport

Decision:

> Remote mode transports Slurm commands over SSH but renders UI locally.

Rationale:

* better local terminal UX
* existing SSH config works
* local clipboard integration remains useful
* no need to run full TUI stack on the cluster

### 18.9 Refresh Must Not Block UI

Decision:

> Data fetch runs off the UI thread.

Rationale:

* Slurm commands can be slow
* SSH can hang or fail
* UI must remain responsive

### 18.10 No Horizontal Overflow

Decision:

> No horizontal table overflow is a hard product invariant.

Rationale:

* terminal UI must remain usable in tmux/SSH/narrow panes
* horizontal scrolling is painful for dashboard monitoring

### 18.11 Config File Is the Persistent Source of Truth

Decision:

> sqtop follows a config-first model. The config file is the single persistent source of truth for user preferences and user-modifiable behavior.

Rationale:

- normal CLI/TUI users expect config files they can inspect, edit, copy, version, and sync
- users may work across clusters and want reproducible behavior
- config files are easier to debug than hidden state
- settings changed in the UI should not become invisible magic
- this matches the culture of tools like Vim, Bash, Zsh, and Ghostty

Consequence:

- every persistent UI setting must map to a config key
- UI settings screens must write config
- config writes should preserve comments and unknown keys where practical
- hidden persistent preference stores are disallowed
- direct config editing remains a first-class workflow

### 18.12 Use `tomlkit` for Config Writes

Decision:

> Use `tomlkit` instead of a basic TOML writer if the config should feel like a Vim/Ghostty/Bash-style user-owned config.

Rationale:

- basic TOML writers often destroy comments and formatting
- for a config-first tool, that feels bad because the user's file becomes less personal and less maintainable after the UI writes to it
- preserving comments, key order, and whitespace keeps the file recognizable to its owner

Principle:

> sqtop may edit the config, but it should behave like a respectful editor of a user-owned file.

---

## 19. Testing and Validation Matrix

### 19.1 Core Tests

Required tests:

* parse `squeue` output
* parse `sinfo` output
* parse `scontrol show job`
* parse `scontrol show node`
* parse GPU/GRES fields defensively
* parse missing/unknown fields
* command timeout handling
* command failure handling
* permission-denied handling
* config load/merge/coercion
* config round-trip stability
* jobs filter/search/sort composition
* my-jobs filter
* state filter
* cursor/selection preservation
* bulk action target resolution
* job action command construction
* clipboard OSC 52 payload
* clipboard fallback
* responsive width budget
* modal sizing
* remote SSH command construction
* command history recording

### 19.2 Investigation Tests

Required tests for Investigation Mode:

* investigate pending job with `Resources`
* investigate pending job with `Dependency`
* investigate held job
* investigate running job with allocated nodes
* investigate failed job with `sacct` available
* investigate failed job with `sacct` unavailable
* investigate node `IDLE`
* investigate node `ALLOCATED`
* investigate node `MIXED`
* investigate node `DOWN`
* investigate node `DRAIN`
* investigate node with visible running jobs
* investigate node with no visible running jobs
* partial report when `scontrol` fails
* partial report when `sacct` fails
* copy investigation report
* confidence labels render correctly
* unknown pending reason fallback

### 19.3 Real Cluster Smoke Tests

Recommended, not required in CI:

* run sqtop on a real login node
* run remote mode over SSH
* verify `squeue`, `sinfo`, `scontrol`, `sacct`
* verify OSC 52 through SSH/tmux
* inspect pending job investigation
* inspect running job investigation
* inspect idle/mixed/allocated node investigation
* attempt permitted action on own test job
* verify denied action shows clean error

### 19.4 Config-First Tests

Required tests:

- UI setting changes write to config.
- Column visibility changes write to config.
- Column reorder changes write to config.
- Safety setting changes write to config.
- Investigation setting changes write to config.
- Unknown config keys are preserved where practical.
- Unknown config sections are preserved where practical.
- Invalid config values fall back to defaults.
- Config write is atomic.
- Config reload keeps last known good config on failure.
- UI reflects effective config after commit.
- Hand-edited config can be loaded without using the UI.

---

## 20. Implementation Checklist

### 20.1 Required for Core Conformance

* Jobs tab renders live `squeue` data.
* Nodes tab renders `sinfo` + node detail data.
* Partitions tab renders partition summary.
* All Slurm commands go through the data layer.
* Data fetch does not block the UI thread.
* Config load/merge/save is defensive.
* My-jobs filter works.
* Search and state filters compose.
* Cursor and selection survive refresh where possible.
* Job detail opens raw `scontrol show job`.
* Node detail opens raw `scontrol show node`.
* Copy works through OSC 52 or fallback.
* Mutating actions require configured confirmations.
* Command failures do not crash the app.
* Remote mode works through SSH.
* No horizontal overflow at supported terminal widths.
* Tests cover parsing and action command construction.

### 20.2 Required for Investigation Conformance

* `I` opens Job Investigation from Jobs view.
* `I` opens Node Investigation from Nodes view.
* Job investigation shows summary, evidence, explanation, related resources, suggested actions, and raw detail.
* Node investigation shows summary, state explanation, visible jobs, free resource estimate, and raw detail.
* Investigation supports partial reports.
* Investigation supports copy report.
* Unknown pending reasons have safe fallback.
* Confidence labels are shown for derived explanations.
* Suggested actions are user-level only.

### 20.3 Recommended Extensions

* Start in my-jobs mode by config.
* Add node filters.
* Add partition pressure summary.
* Add "jobs using this node" drilldown.
* Add resource-fit hints for pending jobs.
* Add better GPU/GRES normalization.
* Add persistent SSH transport.
* Add shareable investigation bundle.
* Add investigation history.
* Add command palette entry for typed job/node investigation.
* Add site-specific reason explanation extension file.

### 20.4 Required for Config-First Conformance

- All persistent user settings are documented in the config schema.
- Every UI-modifiable setting maps to a config key.
- UI setting changes write back to the config file.
- sqtop does not use hidden persistent preference stores.
- Unknown config keys/sections are ignored safely.
- Unknown config keys/sections are preserved where practical.
- Config writes are atomic.
- Config loading is defensive.
- Config reload is available through an explicit command or documented restart behavior.
  
---

## 21. Appendix A — Example Job Investigation Report

```text
Investigate Job 123456

Summary
- State: PENDING
- Reason: Resources
- User: you
- Partition: gpu
- Requested: 1 node, 16 CPUs, 1 GPU, 128G memory
- Time limit: 24:00:00
- Submitted: 2026-05-08 10:14

Slurm evidence
- squeue reason: Resources
- scontrol NumNodes: 1
- scontrol NumCPUs: 16
- scontrol TRES: cpu=16,mem=128G,gres/gpu=1
- partition: gpu

Likely explanation
- Slurm reports that matching resources are not currently available.
- The job requests GPU resources in partition gpu.
- No fully idle GPU node is currently visible in that partition.
- Several matching nodes are allocated or mixed.

Confidence: medium

Related nodes
- gpu-a100-01: ALLOCATED, 4/4 GPUs allocated
- gpu-a100-02: MIXED, 3/4 GPUs allocated
- gpu-a100-03: DRAIN, reason: maintenance

Suggested next actions
- Watch this job.
- Inspect dependency tree.
- Inspect batch script.
- Copy this report.
- Contact an admin if the drain state is unexpected.

Raw detail
- scontrol show job: available
- sacct: unavailable on this cluster
```

---

## 22. Appendix B — Example Node Investigation Report

```text
Investigate Node gpu-a100-02

Summary
- State: MIXED
- Partitions: gpu
- CPUs: 40 / 64 allocated
- GPUs: 3 / 4 allocated
- Memory free: 320G / 512G
- GRES: gpu:a100:4

State explanation
- MIXED means some resources are allocated and some may remain free.
- This node may be able to accept smaller jobs if remaining resources match the request.

Jobs currently using this node
- 12345 alice train-a100 RUNNING 1 GPU 8 CPUs
- 12346 bob preprocess RUNNING 2 GPUs 32 CPUs

Visible free resources
- CPUs: 24 / 64
- GPUs: 1 / 4
- Memory: 320G / 512G

Possible relevance
- Your pending job 12350 requests 1 GPU in partition gpu.
- This node appears to have 1 visible GPU free, but schedulability may also depend on memory, CPU, features, reservations, QoS, and policy.

Confidence: low

Suggested next actions
- Inspect your pending job.
- Watch your job.
- Copy this node report.
- Contact admin if the node state looks inconsistent with expected policy.

Raw detail
- scontrol show node: available


---

## 23. Appendix C — Suggested Source Map

```text
src/sqtop/
├── app.py                    # Textual app lifecycle, tabs, global bindings
├── slurm.py                  # ALL Slurm command execution and parsing
├── investigation.py          # Investigation report construction
├── config.py                 # Config load/merge/save
├── columns.py                # Column order helpers
├── responsive.py             # Width tiers and column allocator
├── clipboard.py              # OSC 52 and fallback clipboard
├── notify.py                 # Desktop notifications
├── styles/app.tcss           # Textual CSS
└── views/
    ├── jobs.py               # Jobs tab
    ├── nodes.py              # Nodes tab
    ├── partitions.py         # Partitions tab
    ├── health.py             # Command health
    ├── investigate.py        # Investigation screen
    ├── job_actions.py        # Job action modal
    ├── job_detail.py         # scontrol show job modal
    ├── job_info.py           # Curated job info modal
    ├── node_detail.py        # scontrol show node modal
    ├── batch_script.py       # Batch script viewer
    ├── log_viewer.py         # stdout/stderr tail
    ├── dependency.py         # Dependency tree
    ├── array_tasks.py        # Array task expansion
    ├── bulk_actions.py       # Bulk operation modal
    ├── confirm.py            # Confirmation modal
    ├── column_toggle.py      # Column visibility/order
    ├── settings.py           # Settings command palette
    └── widgets.py            # CyclicDataTable and shared widgets
```


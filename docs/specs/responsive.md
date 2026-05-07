# Spec: Responsive design (lazygit-style)

Status: Draft
Owner: t3bol90
Last updated: 2026-05-08

## 1. Goal

Make sqtop usable across the full spectrum of terminal sizes — from a tmux pane on a 13" laptop (≈ 60×20) to a maximized iTerm window on a 4K monitor (≈ 240×60) — without ugly overflow, broken layouts, or unreachable controls. Match the spirit of lazygit's responsive behavior: **always show what matters, hide what doesn't, never overflow**.

### 1.1 Hard requirements

These are non-negotiable. A change that violates any of them must not ship:

1. **No horizontal scrolling, ever.** At any terminal width ≥ 40 cells, every rendered frame fits. The user never has to scroll left/right to see content. This applies to first paint, every subsequent paint, and every transient state during a resize drag.
2. **First-paint correctness.** When the user runs `sqtop`, the very first frame is already at the right tier. There is no "flash of overflow" — no moment where columns/modals are sized for a default 80×24 terminal and then reflow. Tier is computed before the first widget is laid out.
3. **Continuous resize fidelity.** Every `Resize` event — including each step of a slow window-edge drag (a "squeeze") — produces a correct, non-overflowing frame within the same render tick. Latency between edge drag and visual update is one frame; correctness is guaranteed at every intermediate width, not just at tier boundaries.
4. **All actions respect current tier.** Opening modals, switching tabs, toggling search, pushing keybindings help, etc. must produce non-overflowing layouts at the current tier. A modal that would overflow at the current width is clamped before mount, not after.

These four requirements drive most of the design choices below — particularly the column-width budget (§5.1.1), the synchronous first-tier computation (§4.1), and the modal-clamp-on-push policy (§5.5).

## 2. Non-goals

- Ground-up redesign. The current `TabbedContent` layout stays.
- Mobile-friendly text density at < 40 cols. We accept that those terminals are unusable and just refuse to render gracefully (a clear "terminal too small" notice) rather than chasing infinite shrink.
- Theming / colors. Out of scope.
- Multi-pane splits like lazygit's main+files+stash layout. sqtop is tab-based by design; we adapt within each tab, not across them.

## 3. Background / current state

What works today:
- **`_visible_cols(width)` in `views/jobs.py:182` and `views/nodes.py:68`** filters table columns by terminal width. `COLUMNS` rows are `(header, min_col_width, min_terminal_width_to_show)`. Existing thresholds:
  - jobs: `0 / 65 / 90 / 105 / 120`
  - nodes: `0 / 60 / 75 / 90 / 105 / 120`
- Auto-sized job columns bounded by `[jobs].*_max` config values.
- `Header` widget shows `sub_title` (e.g. `"Slurm Dashboard — login01"`).
- `Footer` lists key bindings; Textual already collapses unshown bindings into a `?` overflow.

What breaks at narrow widths:
- **Confirm/option modals use fixed cell widths** (`width: 50` / `60` / `52` / `40`) — these overflow on terminals ≤ 50 cols. See `views/confirm.py:22`, `views/job_actions.py:25`, `views/bulk_actions.py:23`, `views/history.py:50`, `views/attach_prompt.py:20`, `views/dependency.py:46`, `views/column_toggle.py:23`.
- **Big modals use `width: 90%`** which is fine, but they have no `min-width` floor — at 50 cols, 90% = 45 cols, which is too narrow to render scontrol output usefully. They also have no `max-width` cap — on a 240-col terminal, a 90% modal is 216 cols of mostly-whitespace.
- **CPU/GPU bars** in `nodes.py` are fixed-width strings; at narrow widths they crowd out the state column.
- **Sub-title** (`"Slurm Dashboard — long-hostname.cluster.example.com"`) ungracefully truncates inside `Header`.
- **Tab labels** include `"[1]" / "[2]"` etc., which are useful at wide widths but consume cells.
- **Search bar / inline status strings** in `JobsView` (the `#search-bar`, `#jobs-header`) don't scale.
- **Footer key bindings** (`?`, `Y`, `B`, etc.) are all `show=True` for some — at narrow widths the footer wraps unpredictably.
- **No "terminal too small" guard** — at 30 cols the app renders garbled content rather than refusing.

Existing breakpoints are implicit and inconsistent across views. Formalizing them once and reusing the names everywhere is the main win.

## 4. Breakpoints

Three formal tiers, plus a hard floor:

| Tier   | Width (cells) | Intent                                                |
|--------|---------------|-------------------------------------------------------|
| `xs`   | 40 – 79       | Narrow tmux pane / small laptop terminal              |
| `sm`   | 80 – 109      | Default 80-col terminal                               |
| `md`   | 110 – 159     | Comfortable single-monitor full-screen                |
| `lg`   | ≥ 160         | Wide-monitor full-screen                              |
| floor  | < 40          | Refuse to render — show "Terminal too small (need ≥ 40)" |

A new `src/sqtop/responsive.py` module exposes:

```python
Tier = Literal["xs", "sm", "md", "lg"]

def tier_for(width: int) -> Tier: ...
def at_least(tier: Tier, width: int) -> bool: ...   # tier_for(width) >= tier
TOO_SMALL = 40
```

These constants replace the magic numbers currently scattered across `_visible_cols`. Existing column thresholds get re-pegged to the named tiers (see §5.1).

Width is the **app's** width (`self.app.size.width`), not the focused widget's — every responsive decision is global to the frame.

### 4.1 First-tier computation (first-paint correctness)

The first frame must render at the correct tier. Today, several views call `_rebuild_columns(self.size.width, ...)` from `compose()` / `on_mount()`, but `self.size.width` can be `0` or stale at that point and is only authoritative after the first `Resize` event. That race is the source of the "first-paint flash" the user sees on launch.

Fix:

- `SqtopApp.__init__` reads `os.get_terminal_size()` (or `shutil.get_terminal_size()` for Windows compatibility) and stores `self._initial_width` / `self._initial_height` synchronously, before Textual mounts anything.
- `tier_for_app(app)` returns `app.tier` if mounted, else `tier_for(self._initial_width)`. Views call this from `compose()` instead of touching `self.size.width`.
- The tier reactive on `SqtopApp` is initialised to `tier_for(self._initial_width)` so the very first `add_class("tier-xs")` call lands before any widget styles resolve.
- After mount, the first `Resize` event reconciles against the real terminal size; if Textual's measurement disagrees with `os.get_terminal_size()` (rare, but happens with some emulators that lie about size), the tier reactive snaps to the Textual-reported value and triggers a single re-render. This is the only allowed "flash" — and only if the terminal lied; in the common case the initial value is already correct and no re-render is needed.

### 4.2 Resize handling — squeeze fidelity

Every resize must produce a correct frame, including intermediate widths during a slow drag. We do **not** debounce or coalesce resize events; correctness beats render economy at the rates real terminals emit (≤ 60 Hz).

- `SqtopApp.on_resize` updates the tier reactive (which fires `watch_tier` only on actual flips — cheap class swap) AND broadcasts a `WidthChanged(width)` message that views listen to. Views recompute column widths on every event, not just tier flips, because intra-tier shrinking still needs column reallocation (see §5.1.1).
- The recompute path is O(visible_columns × cached_max_lengths) — already cheap. The existing `_rebuild_cache_*` short-circuit in `views/jobs.py:362` prevents redundant DOM mutations when nothing actually changes.
- No `set_timer` / debounce / animation. Lazygit doesn't, and the perceived smoothness comes from per-frame correctness, not from filtering events.

## 5. Design per element

### 5.1 Data tables (jobs / nodes / partitions / history)

- Re-peg `min_terminal_width_to_show` values in `COLUMNS` lists to the named tier breakpoints. The existing pattern stays (`(header, min_col_width, min_terminal_width_to_show)`); the third value just becomes one of `0 / 80 / 110 / 160` instead of arbitrary numbers.
- Concrete proposal — column gating per view:

  | View       | Always (xs) | + sm (≥ 80)              | + md (≥ 110)              | + lg (≥ 160)        |
  |------------|-------------|--------------------------|---------------------------|---------------------|
  | Jobs       | JOBID, NAME, STATE | USER, TIME, TIME_LEFT | PARTITION, QOS, NODES, CPUS, TIME_LIMIT | NODELIST(REASON) |
  | Nodes      | NODE, STATE, CPU% | GPU%, CPUS A/T, GPU A/T | MEM FREE, PARTITION  | MEM TOTAL, LOAD     |
  | Partitions | PARTITION, AVAIL, STATE | TIMELIMIT, NODES   | NODELIST                  | —                   |
  | History    | JOBID, STATE, ELAPSED | NAME, USER, EXIT     | PARTITION                 | —                   |

- **CPU%/GPU% bar shrinkage** in `nodes.py`: today the bar is a fixed glyph count. At `xs`, render numeric-only (`"42%"`); at `sm`, narrow bar (5 cells); at `md`+, full bar (10 cells). One helper `_render_pct_bar(pct: int, tier: Tier) -> str` keeps the logic in one place.

#### 5.1.1 Width budget allocation (the "no horizontal scroll" algorithm)

The current `_rebuild_columns` in `views/jobs.py:358` decides which columns to *show* based on tier, then sizes each shown column to its content. **It does not check that the sum of column widths fits the terminal.** With a wide job name or long node list, total width can exceed terminal width → the DataTable horizontally scrolls. This is the bug the §1.1 hard requirements forbid.

New algorithm — every view's column rebuild runs this:

```
budget = terminal_width - chrome_overhead   # chrome = borders, scrollbar reserve
visible = [(name, min_w, content_max_w, priority) for col in tier_visible_cols]
visible.sort(key=priority, descending)      # priority defined per view, see below

# Pass 1: assign each column its minimum
assigned = {name: min_w for name, min_w, _, _ in visible}
remaining = budget - sum(assigned.values())

# Pass 2: distribute the remainder by priority, capped at content_max
for name, min_w, content_max, _ in visible:
    extra = min(remaining, content_max - min_w)
    if extra <= 0: break
    assigned[name] += extra
    remaining -= extra

# Pass 3: if budget < sum(min_w), drop lowest-priority columns until it fits
while sum(assigned.values()) > budget and len(assigned) > 1:
    drop = min(visible, key=priority)
    del assigned[drop.name]
    visible.remove(drop)

# Pass 4: cell content gets truncated to its assigned column width with "…"
```

This guarantees `sum(assigned) ≤ budget` for every visible column at every width. Cells are truncated, never wrapped.

Per-view priorities (highest → lowest):

| View       | Priority order                                                                  |
|------------|---------------------------------------------------------------------------------|
| Jobs       | JOBID, STATE, NAME, USER, TIME, TIME_LEFT, PARTITION, NODES, CPUS, QOS, TIME_LIMIT, NODELIST(REASON) |
| Nodes      | NODE, STATE, CPU%, GPU%, CPUS A/T, GPU A/T, MEM FREE, PARTITION, MEM TOTAL, LOAD |
| Partitions | PARTITION, AVAIL, STATE, TIMELIMIT, NODES, NODELIST                             |
| History    | JOBID, STATE, ELAPSED, NAME, USER, EXIT, PARTITION                              |

Pass 3 (drop columns when even minimums don't fit) is what makes the algorithm robust at the `xs` floor: if the user opens a 42-col tmux pane, JOBID + STATE + NAME might still not fit; the algorithm drops NAME, leaving JOBID + STATE, both within budget. The tier-based hide table in §5.1 is just an upper bound on what's *eligible*; the budget algorithm makes the final call.

`COLUMNS` lists gain a fourth field (`priority: int`) instead of the implicit list ordering, so adding a column doesn't accidentally change priorities.

`chrome_overhead` accounts for the DataTable's left/right padding (typically 2 cells) and a one-cell scrollbar reserve. Measured empirically in PR 2 and pinned as a constant.

### 5.2 Header

- At `xs`: drop `sub_title`. The `Header` widget already truncates with `…` but at 60 cols the truncation eats the actual title — better to drop it entirely.
- At `sm`+: keep `sub_title` but apply manual truncation to `≤ width // 2 - 10` so it never collides with the title.
- Tab labels: at `xs` strip the `[1] / [2]` suffixes (`"Jobs"` not `"Jobs [1]"`). Number bindings still work; users learn them from `?`.

### 5.3 Per-view headers (jobs-header, nodes-header)

These are the colored single-line summary strips above each table (`"sinfo  3 idle  1 alloc  ..."`).

- At `xs`: collapse to the most important counter only (e.g. for nodes, just `"3 idle / 1 down"`; drop the timestamp).
- At `sm`+: current full content.
- Implementation: each view's `_update_header` accepts the current tier and assembles content accordingly.

### 5.4 Footer

Textual's `Footer` already auto-collapses bindings that don't fit. We tune what it shows:

- At `xs`: set `show=False` on every binding except `?` and `q`. The footer becomes `? help · q quit`.
- At `sm`: keep the top ~6 most-used bindings shown (refresh, switch tab, search, copy, info, log).
- At `md`+: current behavior.
- Implementation: the `BINDINGS` lists stay declarative; we add a `show_at: Tier = "sm"` field to a tiny `Binding`-wrapping helper and resolve `show` dynamically in a `watch_tier` reactive on the `App`. Avoids per-view churn.

### 5.5 Modals

Two classes of modal, two fixes:

#### 5.5a Big modals (job info, batch script, log viewer, scontrol detail)

Currently `width: 90%; height: 80%`. Add bounds in CSS:

```tcss
JobInfoScreen #job-info-dialog {
    width: 90%; height: 85%;
    min-width: 60; max-width: 140;
    min-height: 20; max-height: 50;
}
```

`min-width: 60` ensures readable content; `max-width: 140` prevents the dialog from spanning a 4K monitor with empty padding. Same shape applies to `BatchScriptScreen`, `LogViewerScreen`, `JobDetailScreen`, `NodeDetailScreen`, `ArrayTaskScreen`.

If actual terminal width < `min-width`: clamp to `width: 100%`. This is a `xs`-tier-only override using the `:tier-xs` pseudo-class introduced in §5.7.

**Clamp-on-push policy.** Modals must also be checked at the moment they are pushed onto the screen stack, not only after they mount. `SqtopApp.push_screen` is wrapped to call `_clamp_for_tier(screen)` — a no-op for screens that don't expose `responsive_clamp(tier)`, otherwise lets the screen adjust its CSS / content before mount. This prevents the failure mode where opening a modal at `xs` flashes overflow for one frame before CSS resolves.

#### 5.5b Small modals (confirm, job actions, bulk actions, attach, etc.)

Currently `width: 50` / `60` / `52` / `40` cells flat. Migrate every fixed-cell modal to:

```tcss
ConfirmScreen #confirm-dialog {
    width: 50;
    max-width: 90%;
}
```

This keeps the comfortable 50-cell width on big terminals and shrinks to fit on narrow ones. No code changes — pure CSS.

Files to update: `views/confirm.py:22`, `views/job_actions.py:25`, `views/bulk_actions.py:23`, `views/history.py:50` (history modal), `views/attach_prompt.py:20`, `views/dependency.py:46`, `views/column_toggle.py:23`.

### 5.6 Search bar / status strips

The `#search-bar` Input in `JobsView` and the `#jobs-header` Label both span 100% width and are fine. No change needed. Confirm during testing.

### 5.7 The "too small" floor

When `app.size.width < 40` or `app.size.height < 10`:

- Mount a single full-screen `Static` with the message:
  ```
  Terminal too small.
  Resize to at least 40×10.
  Current: 32×8
  ```
- All other widgets are hidden via a CSS class `.app-too-small`.
- On resize back above the floor, restore the normal layout.
- Implementation: a reactive `_too_small: bool` on `SqtopApp`, watched and toggled in a single `on_resize` handler; CSS does the visibility flip.

Tier as a CSS pseudo-class: alongside the message, the App sets a `tier-xs` / `tier-sm` / `tier-md` / `tier-lg` class on its root via `add_class` / `remove_class`. CSS can then write tier-conditional rules without per-widget Python:

```tcss
.tier-xs Header { display: none; }
.tier-xs JobsView #jobs-header { display: none; }
```

This is the lazygit pattern: declarative responsive rules in one place, not scattered ifs in render code.

## 6. Implementation plan

Five PRs in order. Each is independently reviewable and ships a behavior improvement.

### PR 1 — `responsive.py` module + first-paint tier + tier broadcast

- Add `src/sqtop/responsive.py` with `Tier`, `tier_for`, `at_least`, `TOO_SMALL`, and constants `TIER_WIDTH = {"xs": 40, "sm": 80, "md": 110, "lg": 160}`.
- In `SqtopApp.__init__`, read `shutil.get_terminal_size()` and store `_initial_width` / `_initial_height`. Initialize the `tier` reactive to `tier_for(_initial_width)` so first-paint is correct (§4.1).
- Watch `tier`: add/remove `tier-xs|sm|md|lg` classes on `self.screen`.
- Override `on_resize` to recompute tier AND broadcast a `WidthChanged(width)` message that views consume for column-budget recompute (§4.2).
- Wrap `push_screen` to call `screen.responsive_clamp(self.tier)` before mount when the screen exposes that hook (§5.5 clamp-on-push).
- No behavior change for existing views yet.
- Tests: `tier_for` at every boundary; `_initial_width` populated; `WidthChanged` fires on resize; `tier` reactive flips only on tier change.

### PR 2 — width-budget column allocation + re-peg thresholds

This is the load-bearing PR for the §1.1 "no horizontal overflow" requirement.

- Implement the width-budget algorithm from §5.1.1 as a shared helper in `views/base.py` or `responsive.py`: `allocate_columns(budget, columns_with_priority) -> dict[name, width]`.
- `COLUMNS` lists in `jobs.py`, `nodes.py`, `partitions.py`, `history.py` gain a `priority: int` field. Re-peg the existing `min_terminal_width_to_show` field to `TIER_WIDTH["sm"]` / `["md"]` / `["lg"]` and treat it as a "soft" eligibility filter; the budget algorithm is the hard guarantee.
- Replace each view's `_rebuild_columns` body with a call to `allocate_columns`. Existing per-job `_col_max` config caps become the `content_max_w` input to the algorithm.
- Cell rendering truncates to assigned column width with `"…"` (one-char ellipsis); use the existing `_truncate` helper in jobs.py and lift it to `responsive.py` so all views share it.
- Listen to `WidthChanged` (from PR 1) and recompute on every resize, not just tier flips.
- Tests:
  - Synthetic terminal widths from 40 to 240 in steps of 1: assert `sum(assigned column widths) ≤ width - chrome_overhead` for every view, every width. This is the explicit no-overflow regression net.
  - At width=42, jobs view drops NAME (Pass 3); only JOBID + STATE remain.
  - First-paint test: instantiate `JobsView` without firing a Resize event, assert columns are sized for `tier_for(_initial_width)`, not for default 80.

### PR 3 — modal sizing

- CSS-only change (no Python) to add `min-width` / `max-width` to big modals and `max-width: 90%` to small modals. File list in §5.5.
- Add `:tier-xs` override to clamp big modals to `width: 100%`.
- Manual smoke test: open every modal at three terminal widths (40, 80, 160) and confirm rendering.
- No new tests; visual confirmation suffices.

### PR 4 — header / footer / per-view header chrome

- `Header.sub_title` truncation policy by tier.
- Tab label `[N]` suffix stripping at `xs`.
- Per-view header content density per §5.3.
- Footer binding `show` rules per §5.4 (introduce `show_at` helper or a `App.watch_tier` that mutates `BINDINGS`).
- Tests: assert footer footer string at width=60 contains only `?` and `q`; at width=120 contains the full set.

### PR 5 — too-small floor + docs

- Implement the "too small" guard per §5.7.
- README "Terminal sizing" section: recommend ≥ 80 cols for normal use; document that bars/columns hide at narrow widths and that's intentional.
- Tests: resize event with `width=30, height=8` → `_too_small` is True; widgets hidden.

## 7. Edge cases & decisions

- **Resize storms** (drag-resize a window). `on_resize` fires per cell. We re-broadcast tier only when the tier value actually changes; views that listen ignore intra-tier resizes for tier-driven re-renders. Per-view column auto-sizing (jobs auto-width logic) still runs every resize as today — that's separate from tier.
- **`HealthView`** is currently disabled in `app.py`. The spec covers it as if it were active so when it's re-enabled it's already responsive.
- **CSS specificity of `.tier-*` classes**. Textual CSS resolves left-to-right; `.tier-xs Foo { ... }` overrides `Foo { ... }`. Confirmed in PR 3 manual testing.
- **Terminal that lies about size during startup** (some emulators report 80×24 then immediately resize). The reactive on `tier` recomputes on every resize, so this self-corrects within one tick.
- **SSH session resized after detach/reattach** (tmux). Same path: detached sessions get a `Resize` on attach.
- **Help screen (`?`) sizing**. Already a modal; covered by §5.5a (medium-modal class).
- **Bar narrowing accessibility**. At `xs`, "42%" alone is still legible; we don't drop the value. Color is preserved.
- **What about height?** Height matters for modals (covered) and for the data table — Textual's `DataTable` already paginates/scrolls, so height doesn't drive responsive decisions in the same way width does. We add a `min-height: 10` floor in §5.7 and otherwise leave height alone.
- **Why not pixel-equivalent breakpoints?** Cells aren't pixels; users care about column count. lazygit uses the same approach. Keeps the math simple.

## 8. Acceptance criteria

### 8.1 Hard requirements (§1.1) — explicit checks

These are the most important tests. Every PR must keep them green.

- **No horizontal scroll, any width.** Automated test: for `width in range(40, 241)`, instantiate each main view, run the column-budget allocation, assert `sum(column_widths) + chrome_overhead ≤ width`. Snapshot also any modal at three widths each (40, 80, 160). Total: ~4 views × 200 widths + ~8 modals × 3 widths = ~830 assertions.
- **First-paint correctness.** Test: launch the app under a pty harness with `COLUMNS=60`, capture the first frame, assert no row exceeds 60 cells; assert tier class on `screen` is `tier-xs` from frame 1. Repeat for `COLUMNS=100` (`tier-sm`), `120` (`tier-md`), `180` (`tier-lg`).
- **Resize squeeze fidelity.** Scripted test using `App.run_test()` that emits `Resize` events at every width from 200 → 50 → 200 in steps of 1. After each event, assert no horizontal overflow and tier class matches `tier_for(current_width)`.
- **Modal at narrow width.** Open every modal at width=50; assert dialog width ≤ 50; assert all interactive elements are reachable (visible in frame, not clipped).

### 8.2 Per-tier visual checks (manual smoke)

1. At width=60 (`xs`):
   - Header readable, no truncation garbage.
   - Tab labels show no `[N]` suffix.
   - Footer shows only `? help · q quit`.
   - Jobs table shows JOBID, NAME, STATE only (or fewer, if budget forces drop).
   - Confirm dialog fits without horizontal scroll.
2. At width=80 (`sm` boundary):
   - Jobs table gains USER, TIME, TIME_LEFT.
   - Footer expands to ~6 bindings.
3. At width=110 (`md` boundary):
   - Jobs table gains PARTITION, QOS, NODES, CPUS, TIME_LIMIT.
   - All current bindings visible.
4. At width=180 (`lg`):
   - All columns visible, including NODELIST(REASON).
   - Big modals capped at `max-width: 140` (don't span the whole screen).
5. At width=30:
   - "Terminal too small" message shown; main UI hidden.
6. Resizing 60 → 110 → 60 in quick succession does not crash, double-render, or leave stale UI.
7. `uv run pytest` is green, including the new automated checks in §8.1.

## 9. Open questions

- **Should compact mode be opt-in via config (`[ui].compact = "auto" | "always" | "never"`)** for users who want a dense UI on a wide terminal? Default in this spec is `"auto"`. Cheap to add; flagging for PR 1 or 2.
- **Per-view tier overrides**: a power user might want `[history].min_columns = ["JOBID","STATE","NAME"]` to force visibility regardless of tier. Defer unless someone asks.
- **Terminal type detection** (`TERM_PROGRAM=Apple_Terminal` etc.) for emoji / box-drawing fallbacks. Out of scope for responsive; flagging because it's adjacent.
- **Animation / transitions** between tiers. lazygit does instant switches; we should too (cheap, predictable). Flagged here so we don't accidentally add transitions in PR 3.

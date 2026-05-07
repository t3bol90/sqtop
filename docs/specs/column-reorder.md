# Spec: User-driven column reorder

Status: Draft
Owner: t3bol90
Last updated: 2026-05-08

## 1. Goal

Let the user reorder the columns in the data-table views (`JobsView`, `NodesView`, `PartitionsView`) so the layout matches the order they actually want to read — e.g. turning the default `JOBID, STATE, NAME, ...` into `NAME, JOBID, STATE, ...` without editing config by hand.

The interaction must feel native to both interaction modes the project already supports:

- **Mouse**: drag-and-drop a column header to a new position, with a visible drop-target indicator.
- **Keyboard**: shift the column under the data cursor one slot left or right with a single keystroke.

Both paths mutate the same underlying state and both persist across restarts.

### 1.1 Hard requirements

These constraints are non-negotiable; any change that violates them must not ship.

1. **Mouse and keyboard reach the same end state.** A drag from position 2 to position 0 and two keyboard shifts of the column at position 2 produce identical `_column_order` lists. There is no mouse-only or keyboard-only mode.
2. **Reorder must not conflict with sort.** Today `s` / `t` / `c` (and the planned header-click sort) sort by a column. A click that does *not* drag must still sort (or remain a no-op for non-sortable columns); a click that *does* drag must reorder and never trigger sort. Disambiguation is on motion distance, not on a modal "reorder mode" toggle.
3. **Hidden columns retain their position.** The existing `_hidden_cols` set (`views/jobs.py:284`, `views/nodes.py:120`, surfaced by `ColumnToggleScreen`) hides columns from render but must not drop them from `_column_order`. Re-showing a hidden column restores it to its previous slot, not the end.
4. **Persistence is per-view, in the existing config file.** The order survives restart, lives under `[columns]` in `~/.config/sqtop/config.toml` next to `jobs_hidden` / `nodes_hidden`, and uses the same load/save pipeline (`config.py:115`-ish). No new config file, no new section.
5. **Forward-compatible with future columns.** When a future sqtop release adds a new column to `COLUMNS`, the user's saved order must still load cleanly: unknown names in saved order are dropped; new names not in saved order are appended in their `COLUMNS` definition position. No crash, no data loss, no reset to default.
6. **Reorder respects the responsive tier.** A column hidden by the width budget (`allocate_columns`, `responsive.md` §5.1.1) is not reachable by mouse drag — there is no header to grab. Keyboard shift on a column not currently in `_current_cols` is a no-op (no error, no silent reorder of off-screen columns). The user reorders what they can see; widening the terminal reveals more columns in the order they were placed.

## 2. Non-goals

- Column **resizing** by drag. Widths are determined by the budget allocator (`responsive.md` §5.1.1) and `[jobs].*_max` config caps. Out of scope.
- Per-tab independent orders for the same view (e.g. one order at `xs`, another at `lg`). One order per view, applied at all tiers.
- Reordering inside modals (`JobInfoScreen`, `NodeDetailScreen`, etc.). Detail panes are read-only key/value views, not tabular.
- A "presets" system (saved column layouts the user can switch between). One layout per view.
- Cross-view drag (e.g. drag a column from Jobs into Nodes). Each view has its own `COLUMNS` definition.
- Animated transitions during reorder. A single re-render at the new position is sufficient.

## 3. Background / current state

What exists today:

- **Column definitions are positional, hard-coded in source.** `COLUMNS: list[ColumnSpec]` at `views/jobs.py:183` and `views/nodes.py:37` is the single source of truth for both presence and order. Every render path iterates `COLUMNS` (or a `_make_columns()` derivative) in declaration order.
- **Visibility toggling already works.** `_hidden_cols: set[str]` is loaded from `[columns].jobs_hidden` / `[columns].nodes_hidden` (`views/jobs.py:284`), filtered out by `_make_columns()` (`views/jobs.py:384`), and surfaced via `ColumnToggleScreen` (`views/column_toggle.py`). This proves the persistence pattern we'll reuse.
- **Sort persists similarly.** `[view_state].jobs_sort_col` / `jobs_sort_reversed` (`config.py:198`-`201`). Sort actions today are key-bound (`s` state, `t` time, `c` cpus at `views/jobs.py:229-231`) — there is no header-click sort yet.
- **`CyclicDataTable`** (`views/widgets.py:7`) is a thin subclass of Textual's `DataTable` that wraps cursor movement. It does not currently handle mouse events beyond what `DataTable` provides; it is the natural home for drag interception since both `JobsView` and `NodesView` already use it.
- **Render rebuild already drops and re-adds columns.** `_rebuild_columns()` at `views/jobs.py:393` calls `table.clear(columns=True)` then iterates `self._current_cols` and calls `table.add_column(...)` in order. Reordering is a single mutation of the source list followed by the existing rebuild — no new render machinery required.

What is missing:

- Any concept of a user-mutable column order. `_make_columns()` always returns columns in `COLUMNS` declaration order.
- Mouse event handling on header cells.
- Keyboard bindings for shifting a column.
- A persistence key under `[columns]` for the order.

## 4. UX design

### 4.1 Mouse: drag-and-drop on header row

Activation is implicit — there is no "enter reorder mode" key. Mouse-down on a header cell starts a potential drag.

#### Interaction states

| State              | Trigger                                         | Visual                                                  |
|--------------------|-------------------------------------------------|---------------------------------------------------------|
| Idle               | default                                         | normal header style                                     |
| Pressed            | `MouseDown` on a header cell                    | grabbed header gets `text-style: reverse`               |
| Dragging           | `MouseMove` while pressed, motion ≥ 2 cells     | grabbed header stays reversed; insertion marker `▌` rendered between two columns at the drop target |
| Drop               | `MouseUp` while in Dragging state               | column moves into the marked slot; insertion marker clears; row cursor stays on the same logical item |
| Click (no drag)    | `MouseUp` while in Pressed state, motion < 2 cells | falls through to existing `HeaderSelected` handler (sort, once §4.4 lands) or no-op  |
| Cancel             | `Esc` while in Dragging state                   | grab released, no reorder, no sort                      |

#### Drop target

The insertion marker `▌` (Unicode U+258C, Left Half Block) renders between two adjacent header cells, on the boundary closest to the mouse `x` coordinate within the header row's cell layout. Snap-to-boundary uses the column edges already known from `_current_cols` widths plus chrome offsets. Dragging past the leftmost column places the marker at position 0; past the rightmost places it at `len(_current_cols)`.

#### ASCII storyboard

```
Idle:
┌────────┬─────────┬──────┬──────────┐
│ JOBID  │  STATE  │ NAME │  PART    │
├────────┼─────────┼──────┼──────────┤
│ 12345  │ RUNNING │ foo  │  debug   │

Pressed on JOBID, no motion yet:
┌────────┬─────────┬──────┬──────────┐
│[JOBID] │  STATE  │ NAME │  PART    │     ← reverse style on JOBID

Dragging JOBID rightward, cursor between NAME and PART:
┌────────┬─────────┬──────▌─────────┐
│[JOBID] │  STATE  │ NAME ▌  PART    │     ← ▌ at the drop boundary

Drop:
┌─────────┬──────┬────────┬──────────┐
│  STATE  │ NAME │ JOBID  │  PART    │
├─────────┼──────┼────────┼──────────┤
│ RUNNING │ foo  │ 12345  │  debug   │
```

#### Click vs drag threshold

Threshold = **2 cells of horizontal motion** between `MouseDown` and `MouseUp`. Below threshold = click; at or above = drag. Vertical motion is ignored (the header row is one cell tall). The threshold value is a module-level constant in the `CyclicDataTable` subclass, not a config key — it is an interaction tuning, not a user preference.

### 4.2 Keyboard: shift the column at the cursor

Two new view-local bindings on `JobsView` and `NodesView`:

| Key   | Action                                                              |
|-------|---------------------------------------------------------------------|
| `[`   | Move the column under the data cursor one position left             |
| `]`   | Move the column under the data cursor one position right            |

Semantics:

- The "column under the data cursor" is determined from the table's current cursor column index, mapped back to the column name via `_current_cols[cursor_column].name`.
- A shift swaps that column with its left/right neighbor in `_column_order`, then triggers `_rebuild_columns(force=True)` and a `_render_rows(...)`.
- Cursor follows the moved column: after a shift, the cursor remains on the same column (now at the new index). The data row under the cursor does not change.
- Shifting the leftmost-visible column further left, or rightmost-visible further right, is a no-op (no wrap). Wrapping a column from one edge to the other is a deliberately bigger gesture and should be done with mouse drag.
- Hidden columns are skipped when locating the swap neighbor. `[` swaps with the *next visible* column to the left, even if hidden columns sit between them in `_column_order`. Rationale: a press that produced no visible movement (because the immediate neighbor was hidden) felt broken in early prototyping; users perceive hidden columns as not occupying a position.

Why `[` / `]` and not arrow-based bindings: unshifted arrows are cursor movement inside `DataTable`; `shift+arrow` is reserved for visual-mode selection extension (`copy.md` §4.1); and `ctrl+shift+arrow` is widely intercepted by terminals and OSes (tab/pane navigation, word-by-word selection) so it failed in practice. `[` / `]` are unbound at every layer, single-keystroke, and mnemonically borrow tmux's `{` / `}` swap-pane convention without the modifier.

Both bindings register with `show=False` to keep the footer uncluttered; they appear in the keybindings help overlay.

### 4.3 Settings / column-toggle integration

The existing `ColumnToggleScreen` (`views/column_toggle.py`) shows columns in `COLUMNS` declaration order. After this spec lands it shows them in **`_column_order`** order. Toggling a column off keeps it in the order list (just hidden). Toggling it back on restores the original slot.

A new button **"Reset to default order"** is added to `ColumnToggleScreen`. It writes `_column_order = [c.name for c in COLUMNS]`, clears the persisted order key, and rebuilds. Visibility is unaffected by reset.

### 4.4 Header-click sort (out of scope for this spec, but interaction must compose)

Header-click sort is its own follow-up. This spec only commits to: **a click on a header that does not exceed the drag threshold falls through to whatever click handler exists** (today: nothing, so it is a no-op; tomorrow: sort by that column). Drag does not trigger sort. The two features compose without further coupling.

## 5. Implementation

### 5.1 State

Per view (`JobsView`, `NodesView`, `PartitionsView`):

```python
self._column_order: list[str]  # all columns in user's chosen order, including hidden
```

Initialized in `__init__` from config:

```python
saved = list(cfg_all.get("columns", {}).get("jobs_order", []))
default = [c.name for c in COLUMNS]
self._column_order = _reconcile_order(saved, default)
```

`_reconcile_order(saved, default)` returns a list that:
1. Contains every name in `default`, exactly once.
2. Preserves the relative order of names that appear in `saved`.
3. Drops names in `saved` that are not in `default` (column removed in newer release).
4. Appends names in `default` that are not in `saved` (column added in newer release), in their `default` order.

This is the "forward-compatible" behavior from §1.1.5. Lives in a new module `src/sqtop/columns.py` so jobs/nodes/partitions share one implementation; covered by direct unit tests.

### 5.2 Render path

`_make_columns()` (`views/jobs.py:373`) changes from iterating `COLUMNS` to iterating `_column_order` mapped through a `{name: ColumnSpec}` lookup:

```python
def _make_columns(self) -> list[ColumnSpec]:
    by_name = {c.name: c for c in COLUMNS}
    return [
        ColumnSpec(name, c.min_width, self._col_max.get(name, c.content_max), c.priority, c.min_tier)
        for name in self._column_order
        if name not in self._hidden_cols
        and (c := by_name.get(name)) is not None
    ]
```

No other render code changes. `_rebuild_columns()` already iterates `self._current_cols` for `table.add_column(...)`, so it picks up the new order automatically.

### 5.3 Mouse drag

A new mixin or direct override in `CyclicDataTable` (`views/widgets.py`) adds three handlers:

```python
class CyclicDataTable(DataTable):
    DRAG_THRESHOLD_CELLS = 2

    class ColumnReordered(Message):
        def __init__(self, from_index: int, to_index: int) -> None:
            super().__init__()
            self.from_index = from_index
            self.to_index = to_index

    def on_mouse_down(self, event): ...   # record press cell + x; if not header row, ignore
    def on_mouse_move(self, event): ...   # if pressed and motion ≥ threshold, set _dragging, render marker
    def on_mouse_up(self, event):  ...    # if _dragging, post ColumnReordered; else fall through
```

The marker is drawn by toggling a CSS class on the table that triggers a one-cell-wide overlay; alternatively, a transient `Static` positioned above the header row at the boundary x. (Implementation detail; either works. The `Static` overlay is simpler and doesn't require subclassing the renderer.)

Each consuming view handles the message:

```python
def on_cyclic_data_table_column_reordered(self, msg: CyclicDataTable.ColumnReordered) -> None:
    src_name = self._current_cols[msg.from_index][0]
    # Translate to_index from "visible" coordinates to absolute _column_order coordinates.
    visible_names = [n for n, _ in self._current_cols]
    target_visible_name = visible_names[msg.to_index] if msg.to_index < len(visible_names) else None
    self._move_in_order(src_name, before=target_visible_name)
    self._persist_column_order()
    self._rebuild_columns(self.app.size.width, self._last_jobs, force=True)
    self._render_rows(self._last_jobs)
```

`_move_in_order(name, before)` mutates `self._column_order` in place, preserving hidden columns' positions: it removes `name`, then inserts it before the `before` name's index in the (full, including hidden) order list. If `before is None`, append.

### 5.4 Keyboard

Two new actions on each view:

```python
Binding("ctrl+shift+left",  "shift_column_left",  show=False),
Binding("ctrl+shift+right", "shift_column_right", show=False),

def action_shift_column_left(self) -> None:
    self._shift_visible_column(direction=-1)

def action_shift_column_right(self) -> None:
    self._shift_visible_column(direction=+1)

def _shift_visible_column(self, *, direction: int) -> None:
    table = self.query_one(CyclicDataTable)
    cur_visible_idx = table.cursor_column
    if not (0 <= cur_visible_idx < len(self._current_cols)):
        return
    name = self._current_cols[cur_visible_idx][0]
    abs_idx = self._column_order.index(name)
    new_abs_idx = abs_idx + direction
    if not (0 <= new_abs_idx < len(self._column_order)):
        return
    self._column_order[abs_idx], self._column_order[new_abs_idx] = (
        self._column_order[new_abs_idx], self._column_order[abs_idx],
    )
    self._persist_column_order()
    self._rebuild_columns(self.app.size.width, self._last_jobs, force=True)
    self._render_rows(self._last_jobs)
    # Reposition cursor onto the moved column at its new visible index.
    new_visible_idx = next(
        (i for i, (n, _) in enumerate(self._current_cols) if n == name),
        cur_visible_idx,
    )
    table.move_cursor(column=new_visible_idx)
```

Note that `_shift_visible_column` works in **absolute** order coordinates, not visible ones — that's how a swap with a hidden neighbor is the user-visible "jump over" semantics described in §4.2.

### 5.5 Persistence

Two new keys under `[columns]` in `~/.config/sqtop/config.toml`:

```toml
[columns]
jobs_hidden = []                         # existing
nodes_hidden = []                        # existing
jobs_order  = ["NAME", "JOBID", "STATE", ...]   # new
nodes_order = ["NODELIST", "STATE", ...]        # new
partitions_order = [...]                        # new (when PartitionsView gets it)
```

Loading: `__init__` reads as shown in §5.1. Saving: a new helper `_persist_column_order()` calls `config.update({"columns": {"jobs_order": self._column_order}})`. Pattern matches existing `_persist_sort()` (`views/jobs.py:484`).

`config.py` updates:
- Add `"jobs_order": []`, `"nodes_order": []`, `"partitions_order": []` to the `_DEFAULTS["columns"]` dict.
- Extend the merge logic in the load path to coerce these to `list[str]`.
- Extend the writer (`config.py:248` block) to emit them under `[columns]`.

### 5.6 Files touched

- **New**: `src/sqtop/columns.py` — `_reconcile_order(saved, default)` and any shared helpers.
- **Modified**: `src/sqtop/views/widgets.py` — drag handlers + `ColumnReordered` message.
- **Modified**: `src/sqtop/views/jobs.py`, `views/nodes.py`, `views/partitions.py` — `_column_order` state, `_make_columns()` change, message handler, two new actions, persistence.
- **Modified**: `src/sqtop/views/column_toggle.py` — render in `_column_order` order, add "Reset to default order" button.
- **Modified**: `src/sqtop/config.py` — defaults, merge, writer for `*_order` keys.

## 6. Edge cases

| Case                                                                 | Behavior                                                                                       |
|----------------------------------------------------------------------|------------------------------------------------------------------------------------------------|
| Drag starts on header, mouse leaves the table widget mid-drag        | Continue tracking via captured pointer; drop on `MouseUp` wherever it lands. If the up-event x is outside the header band, treat as drop at nearest edge. Do **not** cancel — silent cancellation surprises users mid-gesture. |
| Drag starts on a row cell (not header)                               | Ignored. Row clicks are for selection / sort row.                                              |
| Reorder triggered while a refresh is in flight (`@work` thread)      | The mutation is on the main thread; the next `_update_table()` callback uses the new `_column_order` automatically. No race. |
| Saved order references a removed column                              | Dropped silently by `_reconcile_order`. Logged at debug level for diagnostics.                |
| New column added by a release upgrade                                | Appended in `COLUMNS` declaration position (not always end — `_reconcile_order` inserts at the position where it'd be relative to surviving names from `saved`). |
| All columns hidden                                                   | Reorder bindings still work on `_column_order` but render is empty. No crash. The toggle modal remains reachable via `C`.  |
| Terminal too narrow to show even one column (`< TOO_SMALL`)          | Already handled by `responsive.md` §4 floor. Reorder bindings are no-ops because `_current_cols` is empty.                                       |
| User drags a column onto itself (very short drag, ≥ threshold)       | `from_index == to_index`: `_move_in_order` is a no-op, no persist write, no rebuild.           |
| Two `ctrl+shift+right` presses in rapid succession                   | Each persists synchronously. If config write is slow, debouncing is a follow-up; not in scope for v1. |
| User edits config.toml manually with a malformed `jobs_order`        | `_reconcile_order` is total: any input list (even `["nonsense", 42, None]`) reduces to a valid order. Invalid entries dropped.                  |

## 7. Tests

Follow the existing test layout (`tests/test_jobs_columns.py`, `tests/test_responsive.py`, etc.). New file: `tests/test_column_reorder.py`.

Coverage:

1. `_reconcile_order` unit tests
   - Saved equals default → identity.
   - Saved is empty → default.
   - Saved drops a default name → that name appended.
   - Saved contains an unknown name → dropped.
   - Saved is a permutation → preserved.
   - Saved is malformed (non-strings, duplicates) → coerced to a valid order.
2. `_move_in_order` unit tests
   - Move first to last, last to first, middle to middle, no-op same-position.
   - Move with hidden columns interleaved → hidden positions preserved.
3. Persistence round-trip (jobs + nodes + partitions)
   - Mutate order, write config, re-read, assert order survives.
4. Render integration (using Textual's `App.run_test`)
   - Default order renders columns in `COLUMNS` order.
   - After `ctrl+shift+right` on column 0, header row 0 is the previous column 1, etc.
   - After hiding a column then unhiding via `ColumnToggleScreen`, position is preserved.
   - "Reset to default order" button restores.
5. Drag interaction (`App.run_test` + simulated mouse events)
   - `MouseDown` on header + `MouseUp` at same position → no reorder, click falls through.
   - `MouseDown` on header + horizontal motion ≥ 2 cells + `MouseUp` → `ColumnReordered` posted with correct indices.
   - `Esc` mid-drag → no reorder.

## 8. Rollout

Single PR, behind no feature flag — the default behavior (saved order absent → matches `COLUMNS`) is identical to today. Existing users see no change until they reorder.

CHANGELOG entry under `feat:` mentioning both interaction paths and the new `[columns].*_order` config keys.

No migration script required: missing `*_order` keys produce the default behavior via `_reconcile_order(saved=[], default=...)`.

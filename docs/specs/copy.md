# Spec: Copy data from sqtop

Status: Draft
Owner: t3bol90
Last updated: 2026-05-08

## 1. Goal

Let users copy data out of sqtop into the system clipboard at two granularities:

- **A. Selected text** — an arbitrary user-selected region inside a pane (e.g. a partial column, a few characters of a job id, a substring of a log line).
- **B. A whole pane** — the full visible (or full filtered) contents of the current pane as a single, paste-friendly blob.

Both paths must work without leaving the TUI and without depending on terminal-emulator-specific shift-drag selection (which behaves inconsistently across iTerm2 / Terminal.app / Alacritty / GNOME Terminal / Windows Terminal and breaks across pane borders, padding, and scrollback).

**Primary deployment is over SSH.** Most users run sqtop on a login node and view it through an SSH session from their laptop. Any copy mechanism that writes to the *server's* clipboard (`pbcopy`/`xclip` running on the remote host) is therefore wrong by default — the user wants the bytes to land in the clipboard on the **machine running the terminal emulator**. OSC 52 is the only mechanism that does this reliably without extra infrastructure, so it is the default transport here, with subprocess fallbacks reserved for local-only runs.

## 2. Non-goals

- Replacing terminal-native shift-drag selection. Users who prefer it keep using it; we just no longer rely on it.
- Rich/HTML clipboard payloads. Plain text only (TSV for tabular content).
- Persistent kill-rings or named registers. One clipboard, one yank at a time.
- Cross-machine clipboard relay over SSH. Out of scope; documented as a known limitation.

## 3. Background / current state

- `_copy_to_clipboard(text)` already exists at `src/sqtop/views/jobs.py:131`. It dispatches to `pbcopy` (darwin) → `clip` (win32) → `xclip` → `xsel` (linux), each with a 2 s timeout. Returns `bool`.
  - **This is wrong for the SSH case.** When sqtop runs on a remote login node, `pbcopy`/`xclip` either (a) fails because no clipboard tool is installed on the server, or (b) silently writes to the *server's* clipboard, which the user never sees. Today both yanks (`y`, `Y` in `JobsView`) are effectively broken over SSH.
- Textual's `App.copy_to_clipboard(text)` (available in textual ≥0.50, our `pyproject.toml` pins ≥0.80) emits an OSC 52 escape sequence on the TTY. The terminal emulator on the **user's local machine** intercepts it and writes to the local clipboard. This is the standard fix for "copy from a remote TUI."
- `JobsView` already has two yanks (`src/sqtop/views/jobs.py:457`, `:468`):
  - `y` → `action_yank_job_id` (current row's job id)
  - `Y` → `action_yank_row` (current row as TSV)
- `NodesView`, `PartitionsView`, `HistoryView` have **no** copy bindings.
- Modal text panes (`JobInfoScreen`, `BatchScriptScreen`, `LogViewerScreen`, `DetailView`, `JobDetailScreen`, `NodeDetailScreen`) have **no** copy bindings.
- App-level binding convention: uppercase / `ctrl+` for app-wide, lowercase for view-local (`CLAUDE.md` §"Key binding conventions").

## 4. UX design

### 4.1 Feature A — selected text copy

We introduce an in-app **visual selection mode** modeled loosely on vim's character/line visual mode. It works inside the focused pane and is independent of the terminal's own selection.

#### Activation & keys (view-local, lowercase)

| Key      | Action                                                       |
|----------|--------------------------------------------------------------|
| `v`      | Enter visual mode anchored at the current cursor position    |
| `V`      | Enter visual-line mode (whole rows)                          |
| `Esc`    | Exit visual mode without copying                             |
| `y`      | Yank the current selection to the clipboard, exit visual mode |
| arrows / `h j k l` | Extend the selection                                |
| `g g` / `G` | Extend selection to top / bottom                          |

In data-table panes (`JobsView`, `NodesView`, `PartitionsView`, `HistoryView`):

- `v` selects whole rows (character-level selection inside a `DataTable` cell is not meaningful for our use cases). `v` and `V` collapse to the same row-range behavior here.
- The selected rows are highlighted with the existing "selected" row style (already used by `space` multi-select in `JobsView`). Visual mode reuses that styling but does **not** mutate the persistent multi-select set — exiting visual mode without yanking restores the prior selection.
- `y` emits TSV using `_current_cols` (the same column projection used for rendering and for the existing `Y` row-yank), one row per line, no header. To include the header, use `Y` (pane copy) — see 4.2.

In text panes (`JobInfoScreen`, `BatchScriptScreen`, `LogViewerScreen`, `DetailView`, `JobDetailScreen`, `NodeDetailScreen`):

- The body widget is migrated from `Static`/`RichLog` to a **read-only `TextArea`** (`TextArea(read_only=True)`) where it isn't already. Textual's `TextArea` ships with character-level selection, cursor movement, and a `selected_text` property — we wire `y` (and `ctrl+c` as a synonym, see 4.3) to `app.copy_to_clipboard(text_area.selected_text or text_area.text)`.
- `v` is unnecessary inside a `TextArea` — selection happens implicitly via cursor + shift-arrow / mouse-drag inside the widget. `v` is reserved as a no-op there to avoid a key-handling surprise.

#### Visual feedback

- Footer reports `-- VISUAL --` (or `-- VISUAL LINE --`) while active. Implemented as a reactive on the view that flips a footer status string.
- Selection range is rendered with the existing selected-row style; no new CSS class is required for tables. Text-pane selection uses `TextArea`'s built-in selection rendering.
- On successful yank: `app.notify(f"Copied {n} rows" | f"Copied {n} chars", title="Clipboard")`. On failure (clipboard tool missing): `severity="warning"`, message `"Clipboard unavailable — install xclip or xsel"` (linux) / `"Clipboard unavailable"` (other).

### 4.2 Feature B — pane copy

A single app-level binding copies the **entire current pane** in one shot.

#### Key

| Key            | Scope       | Action                                  |
|----------------|-------------|-----------------------------------------|
| `ctrl+shift+y` | App-wide    | Copy the active pane's full contents    |

Rationale for `ctrl+shift+y`: app-level (matches §"Key binding conventions"), free across all current views (`grep BINDINGS` confirms no collision), mnemonic with `y`/`Y`.

#### Per-pane payload

| Pane                  | Payload                                                                                          |
|-----------------------|--------------------------------------------------------------------------------------------------|
| `JobsView`            | TSV header + every row in `_last_jobs` (post-filter, post-sort), columns = `_current_cols`       |
| `NodesView`           | TSV header + every row in `_last_nodes`, columns = current visible columns                       |
| `PartitionsView`      | TSV header + every row in `_last_partitions`                                                     |
| `HistoryView`         | TSV header + every row currently shown                                                           |
| `JobInfoScreen`       | Full info text (already a single string)                                                         |
| `BatchScriptScreen`   | Full script body                                                                                 |
| `LogViewerScreen`     | Full log buffer (only what's loaded — same bounds as visible scrollback)                         |
| `DetailView` / `JobDetailScreen` / `NodeDetailScreen` | Full `scontrol show` text                                                |

Notes:

- We copy the **filtered** list, not `_last_jobs_raw`. Reason: what you see is what you copy. If a user wants the unfiltered set they clear filters first (`x` etc.). This matches the principle of least surprise and avoids accidentally pasting hundreds of hidden rows.
- TSV uses `\t` separator and `\n` line ending on all platforms (clipboard payload, not a file). No quoting; cell values already pass through `_plain_cell` which strips Rich markup.
- A trailing newline is appended after the last row.

#### Visual feedback

- `app.notify(f"Copied pane: {label} ({n} lines)", title="Clipboard")` where `label` is the active tab title or modal screen name.
- Same warning copy as 4.1 on failure.

### 4.3 Discoverability

- `y` and `Y` keep their current meaning **outside** visual mode in `JobsView` (yank id / yank row) — backwards compatible.
- Add `ctrl+shift+y` to the keybindings help screen (`KeybindingHelpScreen`) under a new "Clipboard" section.
- Add `v` / `V` / `y` (visual yank) to each view's `BINDINGS` with `show=False` to keep the footer clean but make them appear in `?`.
- README gets a short "Copying data" section.

## 5. Implementation plan

Five small PRs, in order. Each preserves behavior of the steps before it.

### PR 1 — extract clipboard helper, OSC 52 first

- Create `src/sqtop/clipboard.py` exposing `copy_to_clipboard(app, text) -> CopyResult` where `CopyResult` is a small dataclass: `ok: bool`, `transport: Literal["osc52", "pbcopy", "xclip", "xsel", "clip", "none"]`, `truncated: bool`.
- Transport order:
  1. **OSC 52** via `app.copy_to_clipboard(text)` — always tried first. This is the only path that works over SSH without extra infra.
  2. **Subprocess fallback** (`pbcopy`/`clip`/`xclip`/`xsel`, the existing `_copy_to_clipboard` logic) — only when `sqtop._SSH_HOST` is unset *and* the user has not opted out via config. Reason: on a local laptop with iTerm2 + tmux misconfigured, OSC 52 may silently drop bytes; pbcopy is a useful safety net there. On a remote host the subprocess path is meaningless, so we skip it.
- A config flag `[clipboard].transport` ∈ `{"auto", "osc52", "subprocess"}` (default `"auto"`) lets a user pin one transport. `"auto"` is the order above. Persisted via `config.update`.
- Size guard: OSC 52 has terminal-side caps (typically 75 KB for xterm, ~100 KB for iTerm2, lower through tmux without `set-clipboard on`). When `len(text.encode()) > 74_000`, log a warning notify (`"Payload truncated to 74 KB; configure tmux set-clipboard on for full copy"`), truncate, and set `CopyResult.truncated = True`. The pane-copy notify surfaces this.
- Move the existing subprocess logic from `views/jobs.py:131` into this module and delete the old function. Update `JobsView` call sites.
- Add a thin `app_copy(app, text, *, label, count=None)` wrapper that calls `copy_to_clipboard` and emits the standard notify message — every new call site uses this so messaging stays consistent. Notify includes the transport so users can tell at a glance whether OSC 52 fired (`"Copied 6 rows · osc52"`).
- Tests: `tests/test_clipboard.py` covers (a) OSC 52 path is preferred, (b) subprocess path used only when SSH is unset and config allows, (c) over-cap payloads are truncated, (d) graceful return on missing tools.

### PR 2 — pane copy (Feature B)

- Add `action_copy_pane` to `SqtopApp` in `app.py`. It dispatches to a new `copy_pane() -> tuple[str, str]` method (returns `(label, payload)`) on the focused view / top screen.
- Implement `copy_pane()` on each of the four data-table views and each of the six text-pane modals. Data-table version factored into a shared helper on `BaseDataTableView`:
  ```python
  def copy_pane(self) -> tuple[str, str]:
      header = "\t".join(name for name, _ in self._current_cols)
      rows   = [self._row_tsv(item) for item in self._current_items()]
      return self._pane_label(), "\n".join([header, *rows]) + "\n"
  ```
  Each subclass implements `_row_tsv` (already trivial — `JobsView` has the logic inline at `:473`) and `_current_items` (returns `_last_jobs` / `_last_nodes` / etc.).
- Add `Binding("ctrl+shift+y", "copy_pane", "Copy pane", show=False)` to `SqtopApp.BINDINGS`.
- Tests: snapshot-style — given a fixed list of fake `Job`s, assert the TSV payload is byte-identical.

### PR 3 — visual mode in data-table views (Feature A, tables)

- Introduce a `VisualSelectMixin` in `views/mixins.py` providing:
  - `_visual_active: bool`, `_visual_anchor: int | None`, `_visual_cursor: int | None`
  - `action_visual_enter`, `action_visual_exit`, `action_visual_yank`
  - Movement-key overrides that, while `_visual_active`, extend the range and call `_render_visual_overlay()`
- Apply mixin to `BaseDataTableView` (or each of the four views directly if `BaseDataTableView` doesn't already serve that role — verify in implementation).
- Footer status: add a `visual_status` reactive on the view; the `Footer` widget already updates from view bindings, so we surface this via a small `Static` element above the table or by setting `app.sub_title` while active.
- Reuse the existing selected-row style; do not introduce a new class.

### PR 4 — copy in text-pane modals (Feature A, text)

- Migrate body widgets in `JobInfoScreen`, `BatchScriptScreen`, `LogViewerScreen`, `DetailView`, `JobDetailScreen`, `NodeDetailScreen` to `TextArea(read_only=True)` (where not already). Behavior preserved: scrolling, syntax-free rendering, search.
  - For `LogViewerScreen` specifically: confirm `TextArea` performs acceptably with multi-MB log buffers. If not, keep `RichLog` and instead expose `copy_pane()` returning the in-memory buffer; visual selection in the log pane is then deferred (acceptable — `ctrl+shift+y` still copies the whole pane).
- Bind `y` and `ctrl+c` (the latter `show=False`) to copy `selected_text or text` via the new `app_copy` helper.

### PR 5 — discoverability

- Update `KeybindingHelpScreen` to list a "Clipboard" section.
- Add a "Copying data" section to `README.md` (3–5 lines: visual mode, `y`/`Y` row yanks, `ctrl+shift+y` pane copy, SSH limitation).

## 6. Edge cases & decisions

- **Empty pane**. Pane copy on a pane with zero data rows still copies the header line + `\n`. Notify says `"Copied pane: Jobs (0 rows)"`.
- **Pending refresh during yank**. `_last_*` is mutated only on the main thread inside `_update_table`. Both copy actions read it on the main thread, so there is no torn-read risk. No locking needed.
- **Very large panes**. A 50k-row pane at ~120 chars/row is ~6 MB — fine for `pbcopy`/`xclip` stdin but slow if the user holds the binding. Debounce is unnecessary; the 2 s subprocess timeout in `copy_to_clipboard` is the natural backstop.
- **SSH / remote** (primary case). Covered in detail in §6.1 below. tl;dr: OSC 52 is the default transport, works through SSH transparently, requires one-line tmux config when tmux is in the chain.
- **Headless CI**. Tests must mock `subprocess.run`. The clipboard helper returns `False` cleanly when no tool is installed, so behavior is graceful.
- **Multi-select interaction**. In `JobsView`, `space` toggles persistent multi-select for bulk actions. Visual mode is **separate state**: entering `v` does not clear multi-select, exiting `v` (with or without yank) does not modify it. Yank (`y`) operates on the visual range only, not the multi-select set. This keeps the two features composable.
- **Footer key conflicts**. `v`, `V`, `y`, `Y` are all currently free in `NodesView`, `PartitionsView`, `HistoryView`. `y`/`Y` in `JobsView` are reused as documented above (still yank-id / yank-row when not in visual mode; `y` doubles as visual-yank when visual mode is active).

### 6.1 SSH / remote — design detail

This is the dominant deployment, so it gets its own section.

#### How OSC 52 works end-to-end

1. sqtop (running on the login node) calls `app.copy_to_clipboard(text)`.
2. Textual writes `ESC ] 52 ; c ; <base64(text)> BEL` to stdout.
3. Bytes flow through `sshd` → SSH client → user's terminal emulator on the laptop.
4. The terminal emulator parses the escape and writes `text` to the **local** system clipboard.

No clipboard daemon, no X forwarding, no `xclip` on the server. Works for `~/.ssh/config`-based SSH and through jump hosts.

#### Terminal support matrix

| Terminal              | OSC 52 write | Notes                                                        |
|-----------------------|--------------|--------------------------------------------------------------|
| iTerm2 (macOS)        | ✅ default   | ~100 KB cap                                                  |
| Kitty                 | ✅ default   | `clipboard_control write-clipboard` (default on)             |
| WezTerm               | ✅ default   | No size cap by default                                       |
| Alacritty             | ✅ default   | Recent versions                                              |
| Ghostty               | ✅ default   | —                                                            |
| GNOME Terminal / VTE  | ✅ recent    | VTE ≥ 0.50                                                   |
| Windows Terminal      | ✅ default   | —                                                            |
| Konsole               | ✅           | "Allow programs to access clipboard" must be enabled         |
| **Terminal.app**      | ❌           | macOS built-in. Users on this terminal need the subprocess fallback (only useful locally) or to switch terminal emulator. Documented as a known gap. |
| tmux                  | ⚠️ requires config | See below                                              |
| screen                | ⚠️ partial   | Not officially supported; documented as a known gap.        |

#### tmux

If the user's session goes through tmux on the remote host (very common — `ssh login01` then `tmux attach`), tmux **drops OSC 52 by default**. The fix is one line in the user's remote `~/.tmux.conf`:

```tmux
set -g set-clipboard on
set -g allow-passthrough on    # tmux 3.3+; needed for some terminal/tmux combos
```

We document this in README under "Copying data → SSH + tmux" with the exact two lines, plus a verification snippet:

```bash
printf '\e]52;c;%s\a' "$(printf 'sqtop test' | base64)"
# If your local clipboard now contains "sqtop test", OSC 52 is working.
```

On a fresh sqtop install over SSH+tmux, the first `y` press notifies `"Copied 1 row · osc52"`. If the user reports nothing landed in the clipboard, the README pointer to this verification snippet should be enough to self-diagnose. We do **not** auto-detect tmux misconfiguration — it's noisy and false-positive-prone.

#### Mosh

Mosh strips OSC sequences it doesn't whitelist. OSC 52 is whitelisted in mosh ≥ 1.4. Users on older mosh need to upgrade; documented as a known limitation, not worked around.

#### Failure modes & user-visible behavior

| Scenario                                | What happens                                                  |
|-----------------------------------------|---------------------------------------------------------------|
| OSC 52 works                            | Notify: `"Copied N rows · osc52"`                             |
| OSC 52 emitted but terminal ignores it  | Silent on the wire — we cannot detect this. Notify still says `osc52`. README documents the verification snippet. |
| Payload > 74 KB                         | Truncated, notify: `"Copied N rows · osc52 · truncated to 74 KB"` with `severity="warning"` |
| OSC 52 disabled via config + remote     | Notify: `"Clipboard unavailable on remote host (OSC 52 disabled, no local fallback)"` `severity="warning"` |
| Local laptop run, OSC 52 fails silently | Subprocess fallback fires, notify: `"Copied N rows · pbcopy"` |

#### Why not alternatives

- **`xclip` over X11 forwarding (`ssh -Y`)** — works but requires X server on the laptop, latency per copy is bad, and most macOS users don't run XQuartz. Rejected.
- **`lemonade` / `clipper` / custom daemons** — extra software the user has to install on both ends. Rejected for the default path; users who want it can wrap sqtop themselves.
- **Writing to a sentinel file the user `scp`s back** — terrible UX, mentioned only to dismiss.

OSC 52 is the only reasonable default.

## 7. Acceptance criteria

1. From `JobsView`: pressing `v`, moving down 5 rows, pressing `y` puts 6 TSV lines on the clipboard. Footer shows `-- VISUAL --` while active.
2. From any of the four main tabs: `ctrl+shift+y` puts the full visible (filtered, sorted) table on the clipboard with a TSV header. Notify confirms count.
3. From `JobInfoScreen`: clicking-and-dragging inside the body, then pressing `y`, copies exactly the dragged substring. Pressing `ctrl+shift+y` copies the entire body.
4. With no clipboard tool installed: every copy path notifies `"Clipboard unavailable"` with `severity="warning"` and does not crash.
5. `?` shows the new bindings under a "Clipboard" section. README "Copying data" section exists, including the SSH+tmux setup snippet and verification command from §6.1.
6. **SSH acceptance**: from a `tmux` session on a remote login node with `set-clipboard on` configured, pressing `ctrl+shift+y` from `JobsView` lands the TSV payload on the user's **local** laptop clipboard. Notify shows `osc52`.
7. `uv run pytest` is green, including the new `tests/test_clipboard.py` and table-snapshot tests in PR 2 / PR 3.

## 8. Open questions

- Do we want a yank-without-header variant for pane copy (e.g. `ctrl+y` = with header, `ctrl+shift+y` = without)? Default in this spec is **header included** — most common case is pasting into a spreadsheet or markdown table.
- For `LogViewerScreen` specifically: keep `RichLog` (no in-pane visual mode) or move to `TextArea` (visual mode works, possible perf hit on huge logs)? Decide during PR 4 with a quick benchmark.
- Should we ship a one-liner `sqtop --check-clipboard` that emits the OSC 52 verification sequence and exits, so users can debug without copy-pasting from README? Cheap to add; flagging for the same PR as the README docs.

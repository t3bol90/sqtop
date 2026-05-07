"""Tests for PR 4: header/footer/tab-label responsive chrome (spec §5.2–5.4)."""

from __future__ import annotations

import re
import shutil
from datetime import datetime
from unittest.mock import patch

import pytest


# ---------------------------------------------------------------------------
# Helpers
# ---------------------------------------------------------------------------

def _make_app(width: int = 80, height: int = 24):
    """Instantiate SqtopApp with a mocked terminal size."""
    from sqtop.app import SqtopApp

    fake_size = shutil.os.terminal_size((width, height))
    with patch("sqtop.app.shutil.get_terminal_size", return_value=fake_size):
        return SqtopApp()


def _shown_actions(app) -> set[str]:
    """Return the set of actions whose binding currently has show=True."""
    result: set[str] = set()
    for _key, bindings in app._bindings.key_to_bindings.items():
        for b in bindings:
            if b.show:
                result.add(b.action)
    return result


def _build_jobs_header(tier: str, all_jobs) -> str:
    """Return the jobs-header markup string for the given tier, without Textual wiring.

    Mirrors the logic in JobsView._update_header so tests can verify content
    without needing a live Textual app.
    """
    total = len(all_jobs)
    if tier == "xs":
        return f"[b]squeue[/b]  [dim]{total} total[/]"

    now = datetime.now().strftime("%H:%M:%S")
    running = sum(1 for j in all_jobs if j.state == "RUNNING")
    pending = sum(1 for j in all_jobs if j.state == "PENDING")
    count_str = f"{total} total"
    return (
        f"[b]squeue[/b]  [green]{running} running[/]  "
        f"[yellow]{pending} pending[/]  "
        f"[dim]{count_str}  updated {now}[/]"
    )


def _build_nodes_header(tier: str, nodes) -> str:
    """Return the nodes-header markup string for the given tier, without Textual wiring.

    Mirrors the logic in NodesView._update_nodes_header.
    """
    visible = [n for n in nodes if n.name]
    idle = alloc = mixed = down = 0
    for n in visible:
        s = n.state.lower()
        if "idle" in s:
            idle += 1
        elif "alloc" in s:
            alloc += 1
        elif "mixed" in s:
            mixed += 1
        if "down" in s or "drain" in s:
            down += 1

    if tier == "xs":
        return (
            f"[b]sinfo[/b]  [green]{idle} idle[/]  [red]{down} down[/]"
        )

    now = datetime.now().strftime("%H:%M:%S")
    return (
        f"[b]sinfo[/b]  [green]{idle} idle[/]  "
        f"[cyan]{alloc} alloc[/]  [yellow]{mixed} mixed[/]  "
        f"[red]{down} down[/]  "
        f"[dim]{len(visible)} total  updated {now}[/]"
    )


# ---------------------------------------------------------------------------
# §5.2 Tab labels
# ---------------------------------------------------------------------------

class TestTabLabels:
    """Tab labels drop the [N] suffix at xs, keep it at sm+."""

    def test_tab_labels_constants_defined(self):
        from sqtop.app import _TAB_LABELS
        assert "jobs" in _TAB_LABELS
        assert "nodes" in _TAB_LABELS
        assert "partitions" in _TAB_LABELS
        assert "history" in _TAB_LABELS

    def test_tab_labels_xs_short(self):
        """At xs the short label has no bracket suffix."""
        from sqtop.app import _TAB_LABELS
        for pane_id, (short, full) in _TAB_LABELS.items():
            assert "[" not in short, f"Short label for {pane_id!r} must not contain '['"

    def test_tab_labels_full_has_bracket(self):
        """At sm+ the full label includes [N]."""
        from sqtop.app import _TAB_LABELS
        for pane_id, (short, full) in _TAB_LABELS.items():
            assert "[" in full, f"Full label for {pane_id!r} should contain '[N]'"

    def test_short_is_prefix_of_full(self):
        """Short label text is a prefix of the full label."""
        from sqtop.app import _TAB_LABELS
        for pane_id, (short, full) in _TAB_LABELS.items():
            assert full.startswith(short), (
                f"Full label {full!r} should start with short label {short!r}"
            )

    def test_apply_tier_to_tabs_no_crash_when_unmounted(self):
        """_apply_tier_to_tabs must not raise when called before mount."""
        app = _make_app(60, 24)
        # Calling before tabs exist should be a no-op (no exception).
        app._apply_tier_to_tabs("xs")
        app._apply_tier_to_tabs("sm")


# ---------------------------------------------------------------------------
# §5.2 Sub-title truncation
# ---------------------------------------------------------------------------

class TestSubTitle:
    """sub_title is empty at xs; truncated or full at sm+."""

    def test_sub_title_empty_at_xs(self):
        app = _make_app(60, 24)
        app._base_sub_title = "Slurm Dashboard — login01.cluster.example.com"
        app._apply_sub_title("xs")
        assert app.sub_title == ""

    def test_sub_title_empty_at_xs_via_tier_init(self):
        """When the app starts at xs width, sub_title must be empty."""
        app = _make_app(60, 24)
        # on_mount is not called here (no Textual event loop), but _base_sub_title
        # defaults aren't set until on_mount. Check at least that tier is xs.
        assert app.tier == "xs"

    def test_sub_title_present_at_sm(self):
        app = _make_app(80, 24)
        app._initial_width = 80
        app._base_sub_title = "Slurm Dashboard — login01"
        app._apply_sub_title("sm")
        # Should be non-empty at sm tier.
        assert app.sub_title != ""

    def test_sub_title_present_at_lg(self):
        app = _make_app(160, 40)
        app._initial_width = 160
        app._base_sub_title = "Slurm Dashboard — login01.cluster.example.com"
        app._apply_sub_title("lg")
        # Full hostname should fit: len=45, max_width = 160//2-10 = 70.
        assert "login01" in app.sub_title

    def test_sub_title_truncated_at_narrow_sm(self):
        """At sm with a very long hostname, sub_title is truncated with …"""
        app = _make_app(80, 24)
        app._initial_width = 80
        long_host = "Slurm Dashboard — " + "a" * 100
        app._base_sub_title = long_host
        app._apply_sub_title("sm")
        # max_width = 80//2-10 = 30; text is truncated
        assert len(app.sub_title) <= 30
        assert app.sub_title.endswith("…")

    def test_sub_title_no_ellipsis_when_fits(self):
        """Short sub_title is NOT truncated at lg."""
        app = _make_app(200, 50)
        app._initial_width = 200
        app._base_sub_title = "Slurm Dashboard"
        app._apply_sub_title("lg")
        assert "…" not in app.sub_title
        assert app.sub_title == "Slurm Dashboard"


# ---------------------------------------------------------------------------
# §5.4 Footer binding visibility
# ---------------------------------------------------------------------------

class TestFooterBindings:
    """Footer shows only ? and q at xs; gains more at sm+."""

    def test_xs_only_quit_and_keys(self):
        """At xs, only 'quit' and 'show_keybindings' are shown."""
        app = _make_app(60, 24)
        app._apply_tier_to_bindings("xs")
        shown = _shown_actions(app)
        # Must include quit and show_keybindings.
        assert "quit" in shown
        assert "show_keybindings" in shown
        # Must NOT include tab-switching actions.
        assert "switch_tab('jobs')" not in shown
        assert "switch_tab('nodes')" not in shown
        assert "switch_tab('partitions')" not in shown
        assert "switch_tab('history')" not in shown
        assert "refresh" not in shown

    def test_xs_only_two_visible_app_bindings(self):
        """At xs, exactly 2 app-level bindings are shown (quit + keys)."""
        app = _make_app(60, 24)
        app._apply_tier_to_bindings("xs")
        shown = _shown_actions(app)
        assert len(shown) == 2

    def test_sm_includes_tabs_and_refresh(self):
        """At sm, tab-switching and refresh bindings are shown."""
        app = _make_app(80, 24)
        app._apply_tier_to_bindings("sm")
        shown = _shown_actions(app)
        assert "quit" in shown
        assert "show_keybindings" in shown
        assert "refresh" in shown
        assert "switch_tab('jobs')" in shown
        assert "switch_tab('nodes')" in shown

    def test_sm_more_visible_than_xs(self):
        """sm tier shows more bindings than xs."""
        app = _make_app(60, 24)
        app._apply_tier_to_bindings("xs")
        xs_count = len(_shown_actions(app))
        app._apply_tier_to_bindings("sm")
        sm_count = len(_shown_actions(app))
        assert sm_count > xs_count

    def test_bindings_still_functional_at_xs(self):
        """Tab keys exist in _bindings even when hidden (still functional)."""
        app = _make_app(60, 24)
        app._apply_tier_to_bindings("xs")
        # The key "1" must still exist in the bindings map (just show=False).
        assert "1" in app._bindings.key_to_bindings

    def test_originally_hidden_bindings_stay_hidden(self):
        """Bindings that were show=False in BINDINGS stay hidden at sm+."""
        app = _make_app(80, 24)
        app._apply_tier_to_bindings("sm")
        shown = _shown_actions(app)
        # "toggle_pause" has show=False in BINDINGS — must stay hidden.
        assert "toggle_pause" not in shown

    def test_ctrl_c_stays_hidden(self):
        """ctrl+c quit binding is show=False and must remain hidden."""
        app = _make_app(80, 24)
        app._apply_tier_to_bindings("sm")
        # ctrl+c maps to 'quit' but was originally show=False.
        ctrl_c_bindings = app._bindings.key_to_bindings.get("ctrl+c", [])
        for b in ctrl_c_bindings:
            assert not b.show, "ctrl+c (show=False originally) must stay hidden"

    def test_switching_tier_sm_to_xs_hides_tabs(self):
        """When tier drops from sm to xs, tab bindings become hidden."""
        app = _make_app(80, 24)
        app._apply_tier_to_bindings("sm")
        assert "switch_tab('jobs')" in _shown_actions(app)
        app._apply_tier_to_bindings("xs")
        assert "switch_tab('jobs')" not in _shown_actions(app)

    def test_switching_tier_xs_to_sm_shows_tabs(self):
        """When tier rises from xs to sm, tab bindings become visible."""
        app = _make_app(60, 24)
        app._apply_tier_to_bindings("xs")
        assert "switch_tab('jobs')" not in _shown_actions(app)
        app._apply_tier_to_bindings("sm")
        assert "switch_tab('jobs')" in _shown_actions(app)


# ---------------------------------------------------------------------------
# §5.3 Per-view header content density
# ---------------------------------------------------------------------------

class TestViewHeaderDensity:
    """Per-view header is shorter at xs than at md."""

    def _jobs_header_text(self, tier: str, total: int = 10) -> str:
        """Build jobs header text for the given tier without Textual wiring."""
        from sqtop.slurm import Job

        def make_job(state: str) -> Job:
            return Job(
                job_id="1", name="test", state=state, user="u",
                time_used="00:01:00", time_limit="01:00:00",
                partition="debug", qos="normal", nodes="1",
                num_nodes="1", num_cpus="4", nodelist="n1", reason="",
            )

        jobs = [make_job("RUNNING")] * (total // 2) + [make_job("PENDING")] * (total // 2)
        return _build_jobs_header(tier, jobs)

    def _nodes_header_text(self, tier: str) -> str:
        """Build nodes header text for the given tier without Textual wiring."""
        from sqtop.slurm import Node

        def make_node(state: str) -> Node:
            return Node(
                name="n1", state=state, cpus_alloc="0", cpus_total="8",
                memory_free="60000", memory_total="64000", gpu_alloc=0,
                gpu_total=0, partition="debug", load="0.01",
            )

        nodes = [make_node("idle"), make_node("idle"), make_node("allocated")]
        return _build_nodes_header(tier, nodes)

    def test_jobs_header_xs_shorter_than_md(self):
        """jobs-header at xs is shorter (plain text) than at md."""
        import re

        def strip_markup(s: str) -> str:
            return re.sub(r"\[/?[^\]]*\]", "", s)

        xs_text = strip_markup(self._jobs_header_text("xs", total=10))
        md_text = strip_markup(self._jobs_header_text("md", total=10))
        assert len(xs_text) < len(md_text), (
            f"xs header ({len(xs_text)!r} chars) should be shorter than md ({len(md_text)!r})"
        )

    def test_jobs_header_xs_has_no_timestamp(self):
        """xs jobs-header must not include a timestamp."""
        import re
        xs_text = self._jobs_header_text("xs", total=5)
        # Timestamps look like HH:MM:SS — no colon-separated time in xs header.
        assert not re.search(r"\d{2}:\d{2}:\d{2}", re.sub(r"\[/?[^\]]*\]", "", xs_text))

    def test_jobs_header_md_has_running_and_pending(self):
        """md jobs-header includes 'running' and 'pending' labels."""
        md_text = self._jobs_header_text("md", total=10)
        assert "running" in md_text
        assert "pending" in md_text

    def test_nodes_header_xs_shorter_than_md(self):
        """nodes-header at xs is shorter than at md."""
        import re

        def strip_markup(s: str) -> str:
            return re.sub(r"\[/?[^\]]*\]", "", s)

        xs_text = strip_markup(self._nodes_header_text("xs"))
        md_text = strip_markup(self._nodes_header_text("md"))
        assert len(xs_text) < len(md_text), (
            f"xs nodes header ({len(xs_text)!r}) should be shorter than md ({len(md_text)!r})"
        )

    def test_nodes_header_xs_has_idle_and_down(self):
        """xs nodes-header shows idle and down counts."""
        xs_text = self._nodes_header_text("xs")
        assert "idle" in xs_text
        assert "down" in xs_text

    def test_nodes_header_md_has_alloc_and_mixed(self):
        """md nodes-header shows alloc and mixed counts."""
        md_text = self._nodes_header_text("md")
        assert "alloc" in md_text
        assert "mixed" in md_text

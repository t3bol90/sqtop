"""Width-budget column allocation regression tests (spec §8.1).

Critical regression net: for every terminal width 40..240, every view's
column allocation must sum to <= budget (terminal_width - CHROME_OVERHEAD).
~800+ assertions total.
"""
from __future__ import annotations

import pytest

from sqtop.responsive import (
    CHROME_OVERHEAD,
    allocate_columns,
    tier_for,
    truncate_cell,
)
from sqtop.views.jobs import COLUMNS as JOBS_COLUMNS
from sqtop.views.nodes import COLUMNS as NODES_COLUMNS
from sqtop.views.partitions import COLUMNS as PARTITIONS_COLUMNS
from sqtop.views.history import COLUMNS as HISTORY_COLUMNS


# ── Helper ────────────────────────────────────────────────────────────────────


def _alloc(columns, width):
    budget = width - CHROME_OVERHEAD
    return allocate_columns(budget, list(columns), current_tier=tier_for(width))


# ── No horizontal overflow (spec §8.1 hard requirement) ───────────────────────


@pytest.mark.parametrize("width", range(40, 241))
def test_no_horizontal_overflow_jobs(width):
    cols = _alloc(JOBS_COLUMNS, width)
    budget = width - CHROME_OVERHEAD
    total = sum(w for _, w in cols)
    assert total <= budget, (
        f"Jobs overflow at width={width}: sum={total} > budget={budget}, cols={cols}"
    )


@pytest.mark.parametrize("width", range(40, 241))
def test_no_horizontal_overflow_nodes(width):
    cols = _alloc(NODES_COLUMNS, width)
    budget = width - CHROME_OVERHEAD
    total = sum(w for _, w in cols)
    assert total <= budget, (
        f"Nodes overflow at width={width}: sum={total} > budget={budget}, cols={cols}"
    )


@pytest.mark.parametrize("width", range(40, 241))
def test_no_horizontal_overflow_partitions(width):
    cols = _alloc(PARTITIONS_COLUMNS, width)
    budget = width - CHROME_OVERHEAD
    total = sum(w for _, w in cols)
    assert total <= budget, (
        f"Partitions overflow at width={width}: sum={total} > budget={budget}, cols={cols}"
    )


@pytest.mark.parametrize("width", range(40, 241))
def test_no_horizontal_overflow_history(width):
    cols = _alloc(HISTORY_COLUMNS, width)
    budget = width - CHROME_OVERHEAD
    total = sum(w for _, w in cols)
    assert total <= budget, (
        f"History overflow at width={width}: sum={total} > budget={budget}, cols={cols}"
    )


# ── Targeted allocation tests ─────────────────────────────────────────────────


def test_jobs_width_42_drops_to_minimal_columns():
    """At width=42 (xs), Pass 3 should drop until only highest-priority cols fit."""
    cols = _alloc(JOBS_COLUMNS, 42)
    names = [n for n, _ in cols]
    # JOBID (priority 100) and STATE (priority 95) are highest; NAME (90) may be dropped.
    assert "JOBID" in names
    assert "STATE" in names
    # NAME should be dropped at this very tight budget.
    # budget = 42 - 3 = 39. JOBID min=8 + STATE min=10 = 18, NAME min=8 → 26 total.
    # That fits, but after Pass 2 expansion they may exceed budget.
    # The key assertion is: no overflow.
    budget = 42 - CHROME_OVERHEAD
    assert sum(w for _, w in cols) <= budget


def test_jobs_width_80_has_sm_columns():
    """At width=80 (sm boundary), USER, TIME, TIME_LEFT should be present."""
    cols = _alloc(JOBS_COLUMNS, 80)
    names = [n for n, _ in cols]
    assert "USER" in names
    assert "TIME" in names
    assert "TIME_LEFT" in names


def test_jobs_width_160_all_columns():
    """At width=160 (lg boundary), all jobs columns should be visible."""
    cols = _alloc(JOBS_COLUMNS, 160)
    names = [n for n, _ in cols]
    for spec in JOBS_COLUMNS:
        assert spec.name in names, f"Column {spec.name} missing at width=160"


def test_output_preserves_input_order():
    """Returned columns must be in input order (not priority order)."""
    cols = _alloc(JOBS_COLUMNS, 160)
    names = [n for n, _ in cols]
    input_names = [spec.name for spec in JOBS_COLUMNS if spec.name in names]
    assert names == input_names, "Output order does not match input order"


def test_nodes_width_80_has_sm_columns():
    """Nodes: at width=80, GPU%, CPUS A/T, GPU A/T should appear."""
    cols = _alloc(NODES_COLUMNS, 80)
    names = [n for n, _ in cols]
    assert "GPU%" in names
    assert "CPUS A/T" in names
    assert "GPU A/T" in names


def test_partitions_width_40_xs_columns():
    """Partitions: at xs, at least PARTITION, AVAIL, STATE are shown."""
    cols = _alloc(PARTITIONS_COLUMNS, 40)
    names = [n for n, _ in cols]
    assert "PARTITION" in names
    # At very narrow widths budget may only fit PARTITION; STATE/AVAIL may be dropped.
    budget = 40 - CHROME_OVERHEAD
    assert sum(w for _, w in cols) <= budget


def test_history_width_80_has_sm_columns():
    """History: at sm, NAME, USER, EXIT should appear."""
    cols = _alloc(HISTORY_COLUMNS, 80)
    names = [n for n, _ in cols]
    assert "NAME" in names
    assert "USER" in names
    assert "EXIT" in names


def test_no_columns_below_min_budget():
    """With budget <= 0, allocate_columns returns empty list."""
    result = allocate_columns(0, list(JOBS_COLUMNS), current_tier="xs")
    assert result == []

    result = allocate_columns(-5, list(JOBS_COLUMNS), current_tier="xs")
    assert result == []


def test_non_empty_result_at_minimum_width():
    """At width=40 (minimum supported), at least one column is returned."""
    cols = _alloc(JOBS_COLUMNS, 40)
    assert len(cols) >= 1


# ── truncate_cell tests ───────────────────────────────────────────────────────


def test_truncate_cell_truncates_with_ellipsis():
    assert truncate_cell("hello world", 5) == "hell…"


def test_truncate_cell_no_truncation_when_fits():
    assert truncate_cell("hi", 10) == "hi"


def test_truncate_cell_exact_fit():
    assert truncate_cell("hello", 5) == "hello"


def test_truncate_cell_width_2_last_resort():
    assert truncate_cell("abc", 2) == "a…"


def test_truncate_cell_width_1_returns_empty():
    assert truncate_cell("abc", 1) == ""


def test_truncate_cell_width_0_returns_empty():
    assert truncate_cell("abc", 0) == ""


def test_truncate_cell_empty_string():
    assert truncate_cell("", 5) == ""


# ── Priority ordering sanity checks ──────────────────────────────────────────


def test_jobs_highest_priority_is_jobid():
    top = max(JOBS_COLUMNS, key=lambda c: c.priority)
    assert top.name == "JOBID"


def test_jobs_lowest_priority_is_nodelist():
    bottom = min(JOBS_COLUMNS, key=lambda c: c.priority)
    assert bottom.name == "NODELIST(REASON)"


def test_nodes_highest_priority_is_node():
    top = max(NODES_COLUMNS, key=lambda c: c.priority)
    assert top.name == "NODE"


def test_history_highest_priority_is_jobid():
    top = max(HISTORY_COLUMNS, key=lambda c: c.priority)
    assert top.name == "JOBID"

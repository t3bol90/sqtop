"""SPEC §14.2 regression net: no horizontal overflow at supported widths.

These tests pin the "No horizontal scrolling. No first-paint overflow."
guarantee from SPEC §14.2 across every current view's column set and at every
representative width in the supported tier ladder (xs/sm/md/lg). They also
pin the chrome-overhead reservation contract and the ``min_tier`` filter
that gates wide-tier-only columns out of narrow terminals.

Distinction from existing coverage:
- ``tests/test_width_budget.py`` already asserts ``sum(widths) <= budget``
  for every width 40..240 on each view. These tests add the complementary
  invariants that future regressions could miss: every survivor's name
  must come from the original column list, the result MUST be non-empty
  at sm+ widths, the ``min_tier`` filter MUST exclude lg-only columns at
  md, and the ``CHROME_OVERHEAD = 3`` reservation MUST be honored.
- ``tests/test_modal_sizing.py`` and ``tests/test_investigate_screen.py``
  already cover ``responsive_clamp("xs")`` for both investigation screens
  individually; here we add a single parametrized check that pins both
  together and documents the xs invariant in one place.
"""
from __future__ import annotations

import pytest

from sqtop.responsive import (
    CHROME_OVERHEAD,
    ColumnSpec,
    allocate_columns,
    tier_for,
    truncate_cell,
)
from sqtop.views.history import COLUMNS as HISTORY_COLUMNS
from sqtop.views.investigate import (
    JobInvestigationScreen,
    NodeInvestigationScreen,
)
from sqtop.views.jobs import COLUMNS as JOBS_COLUMNS
from sqtop.views.nodes import COLUMNS as NODES_COLUMNS
from sqtop.views.partitions import COLUMNS as PARTITIONS_COLUMNS


# ---------------------------------------------------------------------------
# Helpers
# ---------------------------------------------------------------------------


def _alloc(columns, width):
    """Mirror what every view does: budget = width - CHROME_OVERHEAD."""
    return allocate_columns(
        width - CHROME_OVERHEAD, list(columns), current_tier=tier_for(width),
    )


# View column sets keyed by short name so parametrize IDs are readable.
_VIEW_SETS = {
    "jobs": JOBS_COLUMNS,
    "nodes": NODES_COLUMNS,
    "partitions": PARTITIONS_COLUMNS,
    "history": HISTORY_COLUMNS,
}


# Representative widths from each tier:
# - 40 = xs floor (TOO_SMALL_WIDTH); only the very tightest budget.
# - 60 = mid xs.
# - 80 = sm floor.
# - 100 = mid sm.
# - 110 = md floor.
# - 130 = mid md.
# - 160 = lg floor.
# - 200 = wide lg.
_WIDTHS = (40, 60, 80, 100, 110, 130, 160, 200)


# ---------------------------------------------------------------------------
# 1. View column sets — overflow + invariant matrix
# ---------------------------------------------------------------------------


@pytest.mark.parametrize(
    ("view", "width"),
    [(view, w) for view in _VIEW_SETS for w in _WIDTHS],
)
def test_view_columns_fit_within_budget_at_width(view: str, width: int) -> None:
    """SPEC §14.2: no first-paint overflow + every survivor is a real column.

    For every (view, supported width) combination, allocate_columns must:
      - return a result whose summed widths fit inside the budget,
      - return only column names that exist in the input set,
      - return at least one column for any width >= sm floor (80) — the
        xs floor (40) is the only tier where dropping every column is
        legitimate behaviour.
    """
    columns = _VIEW_SETS[view]
    result = _alloc(columns, width)
    budget = width - CHROME_OVERHEAD

    # No first-paint overflow: every cell width fits within the chrome-net budget.
    total = sum(w for _, w in result)
    assert total <= budget, (
        f"{view} overflow at width={width}: sum={total} > budget={budget}, "
        f"cols={result}"
    )

    # Every surviving column name is one we actually defined for the view.
    input_names = {col.name for col in columns}
    for name, _ in result:
        assert name in input_names, (
            f"{view} produced phantom column {name!r} at width={width}; "
            f"expected one of {input_names}"
        )

    # At sm and wider, at least one column must survive — only the xs floor
    # is allowed to drop everything to fit the terminal.
    if width >= 80:
        assert len(result) >= 1, (
            f"{view} returned zero columns at width={width}; sm+ tiers must "
            f"always render at least one column"
        )


# ---------------------------------------------------------------------------
# 2. min_tier eligibility filter
# ---------------------------------------------------------------------------


def test_columns_with_higher_min_tier_dropped_at_lower_widths() -> None:
    """SPEC §14.3 step 1: tier-ineligible columns MUST be filtered out.

    A column whose ``min_tier`` is ``lg`` must not appear at md (width=100).
    The same column must appear at lg (width=200) where the budget supports
    its minimum width.
    """
    columns = [
        ColumnSpec("ALWAYS",   8, 12, 100, "xs"),
        ColumnSpec("LG_ONLY", 10, 14,  50, "lg"),
    ]

    md_names = [n for n, _ in _alloc(columns, 100)]
    assert "ALWAYS" in md_names
    assert "LG_ONLY" not in md_names, (
        "LG-only column appeared at md tier (width=100); min_tier filter is broken"
    )

    lg_names = [n for n, _ in _alloc(columns, 200)]
    assert "ALWAYS" in lg_names
    assert "LG_ONLY" in lg_names, (
        "LG-only column missing at lg tier (width=200) despite ample budget"
    )


# ---------------------------------------------------------------------------
# 3. truncate_cell never exceeds target width
# ---------------------------------------------------------------------------


@pytest.mark.parametrize("text_len", [0, 1, 3, 5, 8, 12, 30])
@pytest.mark.parametrize("width", [0, 1, 2, 3, 5, 8, 12])
def test_truncate_cell_never_exceeds_target_width(text_len: int, width: int) -> None:
    """truncate_cell MUST never return a string longer than the target width.

    Off-by-one bugs in the ellipsis branch would let a cell render past its
    column boundary and cause first-paint overflow despite a correct
    allocate_columns sum. This grid covers the boundary widths (0, 1, 2)
    and the ellipsis-active region (text_len > width).
    """
    text = "x" * text_len
    result = truncate_cell(text, width)
    assert len(result) <= width, (
        f"truncate_cell({text!r}, {width}) -> {result!r} exceeds width {width}"
    )


# ---------------------------------------------------------------------------
# 4. Investigation modal CSS clamp at xs
# ---------------------------------------------------------------------------


@pytest.mark.parametrize(
    ("cls", "args"),
    [
        (JobInvestigationScreen, ("12345",)),
        (NodeInvestigationScreen, ("nodeX",)),
    ],
    ids=["job", "node"],
)
def test_investigation_modal_clamp_xs_class_present(cls, args: tuple) -> None:
    """SPEC §14.2.4: modals respect terminal width via the clamp-xs hook.

    Both investigation screens expose ``responsive_clamp(tier)``; calling it
    with ``"xs"`` must add the ``clamp-xs`` CSS class so the matching CSS
    rule (``#investigate-dialog { width: 100%; height: 100%; ... }``) takes
    effect at the xs tier. We instantiate the screens via ``__new__`` to
    avoid touching the Textual mount lifecycle.
    """
    instance = object.__new__(cls)
    classes: set[str] = set()

    def _add_class(*names: str) -> None:
        classes.update(names)

    # Patch add_class to a plain recorder so we can drive responsive_clamp
    # without standing up a full app.
    instance.add_class = _add_class  # type: ignore[method-assign]
    cls.responsive_clamp(instance, "xs")

    assert "clamp-xs" in classes, (
        f"{cls.__name__}.responsive_clamp('xs') did not add clamp-xs class; "
        f"got classes={classes!r}"
    )


# ---------------------------------------------------------------------------
# 5. CHROME_OVERHEAD reservation invariant
# ---------------------------------------------------------------------------


def test_chrome_overhead_constant_is_three() -> None:
    """The chrome reservation must stay at exactly 3 cells.

    DataTable left pad (1) + right pad (1) + scrollbar reserve (1) = 3.
    Changing this without auditing every column min_width would silently
    eat one column at the boundary widths (40, 80, 110, 160).
    """
    assert CHROME_OVERHEAD == 3


def test_allocate_columns_reserves_chrome_overhead() -> None:
    """The dropping pass MUST account for CHROME_OVERHEAD when minimums overflow.

    At terminal width 80 the budget is 80 - CHROME_OVERHEAD = 77. With two
    columns whose combined minimums exceed 77, the lower-priority column
    MUST be dropped so the survivor fits inside 77 cells. The high-priority
    column's min_width is set to exactly 77 so any drift in the chrome
    reservation (e.g. a refactor accidentally raising CHROME_OVERHEAD to 4
    or eliminating the budget = width - CHROME_OVERHEAD step) would push
    the survivor over budget and break this assertion.
    """
    columns = [
        ColumnSpec("KEEP", 77, 77, 100, "xs"),
        ColumnSpec("DROP", 10, 10,  50, "xs"),
    ]
    result = _alloc(columns, 80)
    names = [n for n, _ in result]
    assert names == ["KEEP"], (
        f"Expected only KEEP to survive at width=80 (budget=77); got {result}"
    )
    assert sum(w for _, w in result) <= 80 - CHROME_OVERHEAD, (
        f"Survivor exceeds chrome-net budget: {result}"
    )

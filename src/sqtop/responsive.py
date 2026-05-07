"""Responsive tier infrastructure for sqtop.

Defines terminal-width breakpoints and helpers used across all views
to make layout decisions without magic numbers scattered everywhere.
"""

from __future__ import annotations

from typing import Literal, NamedTuple

from textual.message import Message

Tier = Literal["xs", "sm", "md", "lg"]

# Minimum width (inclusive) to enter each tier.
TIER_WIDTH: dict[Tier, int] = {"xs": 40, "sm": 80, "md": 110, "lg": 160}

# Ordered list for comparison; index = rank.
_TIER_ORDER: tuple[Tier, ...] = ("xs", "sm", "md", "lg")

# Terminal dimensions below which sqtop refuses to render.
TOO_SMALL_WIDTH = 40
TOO_SMALL_HEIGHT = 10

# Chrome overhead: DataTable left/right padding (2 cells) + scrollbar reserve (1 cell).
# Measured empirically: Textual DataTable uses 1-cell left pad, 1-cell right pad,
# and reserves 1 cell for the scrollbar when content overflows.
CHROME_OVERHEAD = 3


def tier_for(width: int) -> Tier:
    """Return the responsive tier for the given terminal width."""
    if width < TIER_WIDTH["sm"]:
        return "xs"
    if width < TIER_WIDTH["md"]:
        return "sm"
    if width < TIER_WIDTH["lg"]:
        return "md"
    return "lg"


def at_least(target: Tier, width: int) -> bool:
    """Return True if ``width`` qualifies for at least ``target`` tier.

    Examples::

        at_least("sm", 80)  -> True
        at_least("sm", 79)  -> False
        at_least("md", 110) -> True
    """
    current_rank = _TIER_ORDER.index(tier_for(width))
    target_rank = _TIER_ORDER.index(target)
    return current_rank >= target_rank


class ColumnSpec(NamedTuple):
    """Specification for a single table column used by allocate_columns."""

    name: str
    min_width: int      # smallest readable width including 1-char padding
    content_max: int    # cap for auto-sizing (from per-view config or sensible default)
    priority: int       # higher = kept longer when budget shrinks
    min_tier: Tier      # eligibility filter: column only shown at this tier or wider


def allocate_columns(
    budget: int,
    columns: list[ColumnSpec],
    *,
    current_tier: Tier,
) -> list[tuple[str, int]]:
    """Return list of (name, width) such that sum(width) <= budget.

    Algorithm (spec §5.1.1):
      1. Filter to columns where at_least(min_tier, budget+CHROME_OVERHEAD) is true
         (using the full terminal width implied by budget + CHROME_OVERHEAD).
      2. Sort by priority desc.
      3. Pass 1: assign min_width to each.
      4. Pass 2: distribute remaining budget by priority, capped at content_max.
      5. Pass 3: while sum > budget and len > 1, drop lowest-priority column.
      6. Return preserving the input ordering of survivors.
    """
    if budget <= 0:
        return []

    # Reconstruct the full terminal width so at_least() works on tier breakpoints.
    terminal_width = budget + CHROME_OVERHEAD

    # Step 1: filter by tier eligibility.
    eligible: list[ColumnSpec] = [
        col for col in columns
        if at_least(col.min_tier, terminal_width)
    ]

    if not eligible:
        return []

    # Step 2: work in priority-descending order.
    by_priority = sorted(eligible, key=lambda c: c.priority, reverse=True)

    # Step 3 (Pass 1): assign minimum widths.
    assigned: dict[str, int] = {col.name: col.min_width for col in by_priority}

    # Check if even the minimums exceed budget — if so go straight to Pass 3.
    remaining = budget - sum(assigned.values())

    # Step 4 (Pass 2): distribute surplus by priority order, capped at content_max.
    if remaining > 0:
        for col in by_priority:
            if remaining <= 0:
                break
            extra = min(remaining, col.content_max - col.min_width)
            if extra > 0:
                assigned[col.name] += extra
                remaining -= extra

    # Step 5 (Pass 3): drop lowest-priority columns until we fit within budget.
    while sum(assigned.values()) > budget and len(assigned) > 1:
        # Find the lowest-priority *still-assigned* column.
        drop = min(
            (col for col in by_priority if col.name in assigned),
            key=lambda c: c.priority,
        )
        del assigned[drop.name]

    # Step 6: return in input order (not priority order).
    result: list[tuple[str, int]] = []
    for col in columns:
        if col.name in assigned:
            result.append((col.name, assigned[col.name]))

    return result


def truncate_cell(text: str, width: int) -> str:
    """Truncate ``text`` to fit within ``width`` cells, appending ``…`` if needed.

    If ``width`` < 2, returns an empty string (no room for any visible character).
    If ``text`` already fits, it is returned unchanged.
    """
    if width < 2:
        return ""
    if len(text) <= width:
        return text
    return text[: width - 1] + "…"


class WidthChanged(Message):
    """Fired by SqtopApp on every Resize event so views can recompute layout."""

    def __init__(self, width: int, height: int, tier: Tier) -> None:
        super().__init__()
        self.width = width
        self.height = height
        self.tier = tier

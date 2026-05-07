"""Responsive tier infrastructure for sqtop.

Defines terminal-width breakpoints and helpers used across all views
to make layout decisions without magic numbers scattered everywhere.
"""

from __future__ import annotations

from typing import Literal

from textual.message import Message

Tier = Literal["xs", "sm", "md", "lg"]

# Minimum width (inclusive) to enter each tier.
TIER_WIDTH: dict[Tier, int] = {"xs": 40, "sm": 80, "md": 110, "lg": 160}

# Ordered list for comparison; index = rank.
_TIER_ORDER: tuple[Tier, ...] = ("xs", "sm", "md", "lg")

# Terminal dimensions below which sqtop refuses to render.
TOO_SMALL_WIDTH = 40
TOO_SMALL_HEIGHT = 10


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


class WidthChanged(Message):
    """Fired by SqtopApp on every Resize event so views can recompute layout."""

    def __init__(self, width: int, height: int, tier: Tier) -> None:
        super().__init__()
        self.width = width
        self.height = height
        self.tier = tier

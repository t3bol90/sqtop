"""Tests for the responsive tier infrastructure (PR 1)."""

from __future__ import annotations

from unittest.mock import patch

import pytest

from sqtop.responsive import (
    TIER_WIDTH,
    TOO_SMALL_HEIGHT,
    TOO_SMALL_WIDTH,
    Tier,
    WidthChanged,
    at_least,
    tier_for,
)


# ---------------------------------------------------------------------------
# TIER_WIDTH constants
# ---------------------------------------------------------------------------

class TestTierWidthConstants:
    def test_xs(self):
        assert TIER_WIDTH["xs"] == 40

    def test_sm(self):
        assert TIER_WIDTH["sm"] == 80

    def test_md(self):
        assert TIER_WIDTH["md"] == 110

    def test_lg(self):
        assert TIER_WIDTH["lg"] == 160

    def test_too_small_width(self):
        assert TOO_SMALL_WIDTH == 40

    def test_too_small_height(self):
        assert TOO_SMALL_HEIGHT == 10


# ---------------------------------------------------------------------------
# tier_for boundaries
# ---------------------------------------------------------------------------

class TestTierFor:
    @pytest.mark.parametrize("width,expected", [
        (39, "xs"),   # below xs floor → still xs
        (40, "xs"),   # xs floor (= TOO_SMALL_WIDTH, but still xs tier)
        (79, "xs"),   # just below sm
        (80, "sm"),   # sm floor
        (109, "sm"),  # just below md
        (110, "md"),  # md floor
        (159, "md"),  # just below lg
        (160, "lg"),  # lg floor
        (9999, "lg"), # very wide
    ])
    def test_boundary(self, width: int, expected: str):
        assert tier_for(width) == expected, f"tier_for({width}) should be {expected!r}"

    def test_return_type_is_literal(self):
        result = tier_for(100)
        assert result in ("xs", "sm", "md", "lg")


# ---------------------------------------------------------------------------
# at_least
# ---------------------------------------------------------------------------

class TestAtLeast:
    # at_least("xs", *) — xs is the minimum; everything qualifies
    @pytest.mark.parametrize("width", [40, 79, 80, 109, 110, 159, 160, 9999])
    def test_xs_always_true_at_or_above_floor(self, width: int):
        assert at_least("xs", width) is True

    # at_least("sm", *)
    def test_sm_false_below_80(self):
        assert at_least("sm", 79) is False

    def test_sm_true_at_80(self):
        assert at_least("sm", 80) is True

    def test_sm_true_at_109(self):
        assert at_least("sm", 109) is True

    def test_sm_true_at_160(self):
        assert at_least("sm", 160) is True

    # at_least("md", *)
    def test_md_false_below_110(self):
        assert at_least("md", 109) is False

    def test_md_true_at_110(self):
        assert at_least("md", 110) is True

    def test_md_true_at_159(self):
        assert at_least("md", 159) is True

    def test_md_true_at_160(self):
        assert at_least("md", 160) is True

    # at_least("lg", *)
    def test_lg_false_below_160(self):
        assert at_least("lg", 159) is False

    def test_lg_true_at_160(self):
        assert at_least("lg", 160) is True

    def test_lg_true_at_9999(self):
        assert at_least("lg", 9999) is True

    # Explicit spec examples
    def test_spec_example_sm_80(self):
        assert at_least("sm", 80) is True

    def test_spec_example_sm_79(self):
        assert at_least("sm", 79) is False

    def test_spec_example_md_110(self):
        assert at_least("md", 110) is True


# ---------------------------------------------------------------------------
# WidthChanged message
# ---------------------------------------------------------------------------

class TestWidthChanged:
    def test_is_message_subclass(self):
        from textual.message import Message
        assert issubclass(WidthChanged, Message)

    def test_attributes(self):
        msg = WidthChanged(width=120, height=40, tier="md")
        assert msg.width == 120
        assert msg.height == 40
        assert msg.tier == "md"

    def test_xs_tier(self):
        msg = WidthChanged(width=60, height=20, tier="xs")
        assert msg.tier == "xs"


# ---------------------------------------------------------------------------
# SqtopApp — _initial_width population and tier reactive
# ---------------------------------------------------------------------------

class TestSqtopAppInit:
    def test_initial_width_populated(self):
        """_initial_width must be set from shutil.get_terminal_size in __init__."""
        from sqtop.app import SqtopApp
        import shutil
        fake_size = shutil.os.terminal_size((120, 35))
        with patch("sqtop.app.shutil.get_terminal_size", return_value=fake_size):
            app = SqtopApp()
        assert app._initial_width == 120
        assert app._initial_height == 35

    def test_initial_height_populated(self):
        from sqtop.app import SqtopApp
        import shutil
        fake_size = shutil.os.terminal_size((80, 24))
        with patch("sqtop.app.shutil.get_terminal_size", return_value=fake_size):
            app = SqtopApp()
        assert app._initial_height == 24

    def test_tier_matches_initial_width(self):
        """tier reactive must be initialized to tier_for(_initial_width)."""
        from sqtop.app import SqtopApp
        import shutil
        fake_size = shutil.os.terminal_size((120, 35))
        with patch("sqtop.app.shutil.get_terminal_size", return_value=fake_size):
            app = SqtopApp()
        assert app.tier == "md"

    def test_tier_sm_at_80(self):
        from sqtop.app import SqtopApp
        import shutil
        fake_size = shutil.os.terminal_size((80, 24))
        with patch("sqtop.app.shutil.get_terminal_size", return_value=fake_size):
            app = SqtopApp()
        assert app.tier == "sm"

    def test_tier_lg_at_180(self):
        from sqtop.app import SqtopApp
        import shutil
        fake_size = shutil.os.terminal_size((180, 50))
        with patch("sqtop.app.shutil.get_terminal_size", return_value=fake_size):
            app = SqtopApp()
        assert app.tier == "lg"

    def test_smoke_tier_md(self):
        """Smoke: instantiate SqtopApp with mocked size=120; assert tier == 'md'."""
        from sqtop.app import SqtopApp
        import shutil
        fake_size = shutil.os.terminal_size((120, 40))
        with patch("sqtop.app.shutil.get_terminal_size", return_value=fake_size):
            app = SqtopApp()
        assert app.tier == "md"
        assert app._initial_width == 120

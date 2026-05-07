"""Tests for the too-small terminal floor (spec §5.7)."""
from __future__ import annotations

import shutil
from collections import namedtuple
from unittest.mock import patch

import pytest

from sqtop.app import SqtopApp
from sqtop.responsive import TOO_SMALL_WIDTH, TOO_SMALL_HEIGHT


_FakeSize = namedtuple("_FakeSize", ["columns", "lines"])


def _make_app(columns: int, lines: int) -> SqtopApp:
    """Instantiate SqtopApp with a mocked terminal size."""
    with patch("shutil.get_terminal_size", return_value=_FakeSize(columns, lines)):
        app = SqtopApp()
    return app


# ---------------------------------------------------------------------------
# too_small flag — pure unit tests, no Textual event loop needed
# ---------------------------------------------------------------------------


def test_too_small_narrow_width():
    """Width below floor → too_small is True."""
    app = _make_app(30, 20)
    assert app.too_small is True
    assert app._initial_width == 30
    assert app._initial_height == 20


def test_too_small_short_height():
    """Height below floor → too_small is True even at wide width."""
    app = _make_app(80, 8)
    assert app.too_small is True
    assert app._initial_width == 80
    assert app._initial_height == 8


def test_not_too_small_normal():
    """80×24 is above both floors → too_small is False."""
    app = _make_app(80, 24)
    assert app.too_small is False


def test_not_too_small_at_exact_floor():
    """Exactly at the floor (40×10) → not too small."""
    app = _make_app(TOO_SMALL_WIDTH, TOO_SMALL_HEIGHT)
    assert app.too_small is False


def test_too_small_one_below_width_floor():
    """One column below width floor → too small."""
    app = _make_app(TOO_SMALL_WIDTH - 1, TOO_SMALL_HEIGHT)
    assert app.too_small is True


def test_too_small_one_below_height_floor():
    """One row below height floor → too small."""
    app = _make_app(TOO_SMALL_WIDTH, TOO_SMALL_HEIGHT - 1)
    assert app.too_small is True


# ---------------------------------------------------------------------------
# CSS class integration — requires Textual run_test()
# ---------------------------------------------------------------------------


@pytest.mark.asyncio
async def test_screen_class_too_small():
    """screen has 'app-too-small' class when terminal is below floor."""
    app = _make_app(30, 20)
    async with app.run_test(size=(30, 20)) as pilot:
        assert app.too_small is True
        assert pilot.app.screen.has_class("app-too-small")


@pytest.mark.asyncio
async def test_screen_class_normal():
    """screen does NOT have 'app-too-small' class at normal size."""
    app = _make_app(80, 24)
    async with app.run_test(size=(80, 24)) as pilot:
        assert app.too_small is False
        assert not pilot.app.screen.has_class("app-too-small")


@pytest.mark.asyncio
async def test_resize_to_too_small_adds_class():
    """Resizing into sub-floor territory adds 'app-too-small' class."""
    app = _make_app(80, 24)
    async with app.run_test(size=(80, 24)) as pilot:
        assert not pilot.app.screen.has_class("app-too-small")
        await pilot.resize_terminal(30, 8)
        await pilot.pause()
        assert pilot.app.too_small is True
        assert pilot.app.screen.has_class("app-too-small")


@pytest.mark.asyncio
async def test_resize_back_above_floor_removes_class():
    """Resizing back above the floor removes 'app-too-small' class."""
    app = _make_app(30, 8)
    async with app.run_test(size=(30, 8)) as pilot:
        assert pilot.app.screen.has_class("app-too-small")
        await pilot.resize_terminal(80, 24)
        await pilot.pause()
        assert pilot.app.too_small is False
        assert not pilot.app.screen.has_class("app-too-small")

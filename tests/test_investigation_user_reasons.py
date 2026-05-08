"""Tests for site-supplied pending-reason overrides (SPEC §20.3, §8.4.1).

These tests cover ``load_user_reasons()`` and ``register_user_reasons()``
plus the override path through ``explain_pending_reason()``. The autouse
fixture below resets ``_USER_REASONS`` after every test so module-level
state cannot leak across tests in this file or into other test files.
"""
from __future__ import annotations

import pytest

from sqtop import investigation
from sqtop.investigation import (
    InvestigationExplanation,
    explain_pending_reason,
    load_user_reasons,
    register_user_reasons,
)


# ---------------------------------------------------------------------------
# Test isolation: pin invariant that _USER_REASONS is always reset to {}.
#
# Invariants:
#   1. _USER_REASONS starts each test as {} (no cross-test contamination).
#   2. After every test, _USER_REASONS is again {} (no leakage to other
#      test files like test_investigation_domain.py).
# ---------------------------------------------------------------------------


@pytest.fixture(autouse=True)
def _reset_user_reasons():
    """Ensure _USER_REASONS is empty before and after each test."""
    register_user_reasons({})
    yield
    register_user_reasons({})


# ---------------------------------------------------------------------------
# load_user_reasons — empty / missing inputs
# ---------------------------------------------------------------------------


def test_load_user_reasons_empty_path_returns_empty():
    assert load_user_reasons("") == {}


def test_load_user_reasons_none_returns_empty():
    assert load_user_reasons(None) == {}


def test_load_user_reasons_missing_file_returns_empty(tmp_path):
    missing = tmp_path / "does_not_exist.toml"
    # Must not raise; missing file is a documented degraded-mode return.
    assert load_user_reasons(missing) == {}


# ---------------------------------------------------------------------------
# load_user_reasons — happy path and partial-result tolerance
# ---------------------------------------------------------------------------


def test_load_user_reasons_happy_path(tmp_path):
    p = tmp_path / "reasons.toml"
    p.write_text(
        "[SiteSpecificFoo]\n"
        'title = "Site-specific foo"\n'
        'detail = "Foo is unavailable due to local cluster policy."\n'
        'confidence = "medium"\n'
        "\n"
        "[AnotherReason]\n"
        'title = "Another reason title"\n'
        'detail = "Some other detail."\n'
        'confidence = "high"\n',
        encoding="utf-8",
    )
    out = load_user_reasons(p)
    assert set(out.keys()) == {"SiteSpecificFoo", "AnotherReason"}

    foo = out["SiteSpecificFoo"]
    assert isinstance(foo, InvestigationExplanation)
    assert foo.title == "Site-specific foo"
    assert foo.detail == "Foo is unavailable due to local cluster policy."
    assert foo.confidence == "medium"
    assert foo.evidence_refs == ()

    other = out["AnotherReason"]
    assert other.title == "Another reason title"
    assert other.detail == "Some other detail."
    assert other.confidence == "high"


def test_load_user_reasons_skips_missing_fields(tmp_path):
    p = tmp_path / "reasons.toml"
    p.write_text(
        "[Good]\n"
        'title = "Good title"\n'
        'detail = "Good detail."\n'
        'confidence = "low"\n'
        "\n"
        "[Bad]\n"
        'title = "Bad — missing detail"\n'
        'confidence = "high"\n',
        encoding="utf-8",
    )
    out = load_user_reasons(p)
    assert "Good" in out
    assert "Bad" not in out


def test_load_user_reasons_skips_missing_title(tmp_path):
    p = tmp_path / "reasons.toml"
    p.write_text(
        "[Good]\n"
        'title = "ok"\n'
        'detail = "ok detail"\n'
        'confidence = "high"\n'
        "\n"
        "[NoTitle]\n"
        'detail = "no title here"\n'
        'confidence = "high"\n',
        encoding="utf-8",
    )
    out = load_user_reasons(p)
    assert "Good" in out
    assert "NoTitle" not in out


def test_load_user_reasons_skips_invalid_confidence(tmp_path):
    p = tmp_path / "reasons.toml"
    p.write_text(
        "[BadConfidence]\n"
        'title = "x"\n'
        'detail = "y"\n'
        'confidence = "very_high"\n'
        "\n"
        "[GoodHigh]\n"
        'title = "ok-h"\n'
        'detail = "d-h"\n'
        'confidence = "high"\n'
        "\n"
        "[GoodMedium]\n"
        'title = "ok-m"\n'
        'detail = "d-m"\n'
        'confidence = "medium"\n'
        "\n"
        "[GoodLow]\n"
        'title = "ok-l"\n'
        'detail = "d-l"\n'
        'confidence = "low"\n',
        encoding="utf-8",
    )
    out = load_user_reasons(p)
    assert "BadConfidence" not in out
    assert {"GoodHigh", "GoodMedium", "GoodLow"} <= set(out.keys())


def test_load_user_reasons_skips_non_dict_value(tmp_path):
    """Top-level scalar entries (e.g. ``Foo = "bar"``) are skipped, not crashed on.

    A bare top-level scalar above any [table] header parses as a top-level
    key/value pair, so the loaded dict has ``{"Foo": "bar", "Good": {...}}``.
    The non-dict ``"Foo"`` value must be skipped.
    """
    p = tmp_path / "reasons.toml"
    p.write_text(
        'Foo = "bar"\n'
        "\n"
        "[Good]\n"
        'title = "ok"\n'
        'detail = "ok detail"\n'
        'confidence = "high"\n',
        encoding="utf-8",
    )
    out = load_user_reasons(p)
    assert "Foo" not in out
    assert "Good" in out


def test_load_user_reasons_malformed_toml_returns_empty(tmp_path):
    p = tmp_path / "reasons.toml"
    p.write_text("= = =\n", encoding="utf-8")
    # Must not raise.
    assert load_user_reasons(p) == {}


def test_load_user_reasons_skips_non_string_title_or_detail(tmp_path):
    p = tmp_path / "reasons.toml"
    p.write_text(
        "[BadTitle]\n"
        "title = 42\n"
        'detail = "ok"\n'
        'confidence = "high"\n'
        "\n"
        "[BadDetail]\n"
        'title = "ok"\n'
        "detail = 3.14\n"
        'confidence = "high"\n'
        "\n"
        "[Good]\n"
        'title = "ok-t"\n'
        'detail = "ok-d"\n'
        'confidence = "high"\n',
        encoding="utf-8",
    )
    out = load_user_reasons(p)
    assert "BadTitle" not in out
    assert "BadDetail" not in out
    assert "Good" in out


# ---------------------------------------------------------------------------
# register_user_reasons + explain_pending_reason override path
# ---------------------------------------------------------------------------


def _make_explanation(title: str = "site-foo", detail: str = "site-detail") -> InvestigationExplanation:
    return InvestigationExplanation(
        title=title,
        detail=detail,
        confidence="medium",
        evidence_refs=(),
    )


def test_register_user_reasons_replaces_state():
    register_user_reasons({"FooReason": _make_explanation()})
    exp = explain_pending_reason("FooReason")
    assert exp.title == "site-foo"
    assert exp.detail == "site-detail"

    # Replace with empty -> falls through to unknown-reason fallback.
    register_user_reasons({})
    exp2 = explain_pending_reason("FooReason")
    assert "unrecognized" in exp2.title.lower()
    assert exp2.confidence == "low"


def test_register_user_reasons_copies_input_dict():
    """Mutating the dict passed to register_user_reasons() must not leak in."""
    src: dict[str, InvestigationExplanation] = {"FooReason": _make_explanation()}
    register_user_reasons(src)
    src["BarReason"] = _make_explanation(title="leaked", detail="leaked")
    exp = explain_pending_reason("BarReason")
    assert "unrecognized" in exp.title.lower()


def test_explain_pending_reason_user_wins_over_builtin():
    """Site-supplied 'Resources' override beats the built-in entry."""
    override = InvestigationExplanation(
        title="site-Resources-title",
        detail="site-Resources-detail",
        confidence="high",
        evidence_refs=(),
    )
    register_user_reasons({"Resources": override})
    exp = explain_pending_reason("Resources")
    assert exp is override
    assert exp.title == "site-Resources-title"
    assert exp.confidence == "high"


def test_explain_pending_reason_user_does_not_break_unknown_path():
    register_user_reasons({"Foo": _make_explanation()})
    exp = explain_pending_reason("BarUnknown")
    assert exp.confidence == "low"
    assert "unrecognized" in exp.title.lower()
    assert "BarUnknown" in exp.detail


def test_explain_pending_reason_user_does_not_break_null_path():
    """Null/empty inputs always return the documented 'no reason' explanation,
    even when user overrides exist."""
    register_user_reasons({"Resources": _make_explanation(title="x", detail="y")})
    for null_input in ("", "(null)", None):
        exp = explain_pending_reason(null_input)  # type: ignore[arg-type]
        assert "no pending reason" in exp.title.lower()
        assert exp.confidence == "low"


def test_explain_pending_reason_falls_back_to_builtin_when_user_has_no_match():
    """When user reasons are populated but lack the queried key, built-in wins."""
    register_user_reasons({"SomeOtherKey": _make_explanation()})
    exp = explain_pending_reason("Resources")
    # Built-in 'Resources' is medium confidence; site map missed it.
    assert exp.confidence == "medium"
    assert "resource" in (exp.title + " " + exp.detail).lower()


# ---------------------------------------------------------------------------
# End-to-end: load TOML -> register -> explain
# ---------------------------------------------------------------------------


def test_load_then_register_then_explain_chain(tmp_path):
    p = tmp_path / "reasons.toml"
    p.write_text(
        "[CustomReason]\n"
        'title = "Custom title"\n'
        'detail = "Custom detail."\n'
        'confidence = "low"\n',
        encoding="utf-8",
    )
    loaded = load_user_reasons(p)
    register_user_reasons(loaded)

    exp = explain_pending_reason("CustomReason")
    assert exp.title == "Custom title"
    assert exp.detail == "Custom detail."
    assert exp.confidence == "low"


def test_load_user_reasons_does_not_mutate_module_state(tmp_path):
    """Loading does not, by itself, register; it returns a dict the caller
    must pass to register_user_reasons()."""
    p = tmp_path / "reasons.toml"
    p.write_text(
        "[Foo]\n"
        'title = "t"\n'
        'detail = "d"\n'
        'confidence = "high"\n',
        encoding="utf-8",
    )
    _ = load_user_reasons(p)
    # Module state untouched.
    assert investigation._USER_REASONS == {}
    exp = explain_pending_reason("Foo")
    assert "unrecognized" in exp.title.lower()

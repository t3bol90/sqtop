"""Schema-coherence regression tests for config.py.

These tests pin the internal consistency of the three module-level constants
that together describe the config schema:

* ``_DEFAULTS`` — source of truth for documented sections and their default
  key/value pairs.
* ``_SECTION_ORDER`` — emit order used by the fresh-install document writer.
* ``_SECTION_COMMENTS`` — one-line section comments seeded for new installs.

If a future PR adds a new dict section to ``_DEFAULTS`` but forgets to update
``_SECTION_ORDER`` or ``_SECTION_COMMENTS`` (or vice versa, or omits a section
from the deep-copy block in ``_defaults()``), at least one of these tests
fails loudly. They are intentionally cheap, deterministic, and free of any
filesystem/network side effects except where ``temp_config`` is required.
"""
from __future__ import annotations

import tomlkit

from sqtop import config


# ── helpers ──────────────────────────────────────────────────────────────────


def _dict_sections() -> dict[str, dict]:
    """Return only the dict-valued entries of ``_DEFAULTS``.

    ``_DEFAULTS`` mixes a bare top-level scalar (``theme``) with nested
    section tables. The schema-coherence invariants apply only to the latter.
    """
    return {k: v for k, v in config._DEFAULTS.items() if isinstance(v, dict)}


# ── (a) every dict section in _DEFAULTS appears in _SECTION_ORDER ────────────


def test_every_dict_section_in_defaults_appears_in_section_order():
    missing = [name for name in _dict_sections() if name not in config._SECTION_ORDER]
    assert missing == [], (
        f"Sections present in _DEFAULTS but missing from _SECTION_ORDER: {missing}. "
        "Add them to _SECTION_ORDER so fresh installs emit them in a predictable order."
    )


# ── (b) every section in _SECTION_ORDER appears in _DEFAULTS as a dict ───────


def test_every_section_in_section_order_appears_in_defaults_as_dict():
    for name in config._SECTION_ORDER:
        assert name in config._DEFAULTS, (
            f"_SECTION_ORDER references {name!r} but _DEFAULTS has no entry for it."
        )
        assert isinstance(config._DEFAULTS[name], dict), (
            f"_DEFAULTS[{name!r}] must be a dict (a TOML section), "
            f"got {type(config._DEFAULTS[name]).__name__}."
        )


# ── (c) every section in _SECTION_ORDER has a non-empty comment ──────────────


def test_every_section_has_a_section_comment():
    for name in config._SECTION_ORDER:
        assert name in config._SECTION_COMMENTS, (
            f"Missing section comment for {name!r}; add an entry to _SECTION_COMMENTS."
        )
        comment = config._SECTION_COMMENTS[name]
        assert isinstance(comment, str), (
            f"_SECTION_COMMENTS[{name!r}] must be a str, got {type(comment).__name__}."
        )
        assert comment.strip() != "", (
            f"_SECTION_COMMENTS[{name!r}] must be a non-empty string."
        )


# ── (d) section comments have no orphans ─────────────────────────────────────


def test_section_comments_have_no_orphan_keys():
    orphans = [name for name in config._SECTION_COMMENTS if name not in config._SECTION_ORDER]
    assert orphans == [], (
        f"Stale entries in _SECTION_COMMENTS not present in _SECTION_ORDER: {orphans}. "
        "Remove them when a section is removed."
    )


# ── (e) _defaults() includes every section with isolated dict identity ───────


def test_defaults_helper_includes_every_section_with_isolated_dict():
    cfg = config._defaults()
    for name in config._SECTION_ORDER:
        assert name in cfg, (
            f"_defaults() did not return section {name!r}; "
            "extend the deep-copy block in _defaults()."
        )
        assert isinstance(cfg[name], dict), (
            f"_defaults()[{name!r}] must be a dict, got {type(cfg[name]).__name__}."
        )
        assert cfg[name] is not config._DEFAULTS[name], (
            f"_defaults()[{name!r}] must be a fresh dict, not a shared reference "
            f"to _DEFAULTS[{name!r}]. Mutating the loaded config could otherwise "
            "leak into the module-level defaults."
        )


# ── (f) _default_document() emits every section header ───────────────────────


def test_default_document_serializes_every_section_header():
    doc = config._default_document()
    rendered = tomlkit.dumps(doc)
    for name in config._SECTION_ORDER:
        header = f"[{name}]"
        assert header in rendered, (
            f"_default_document() output is missing the {header} section header."
        )


# ── (g) section default values are flat scalars/lists ────────────────────────


def test_section_default_keys_are_all_simple_scalars_or_lists():
    allowed = (bool, int, float, str, list)
    for section, body in _dict_sections().items():
        for key, value in body.items():
            assert isinstance(value, allowed), (
                f"_DEFAULTS[{section!r}][{key!r}] has unsupported type "
                f"{type(value).__name__}; nested dicts inside a section break the "
                "writer's flat-table assumption."
            )


# ── (h) load() returns a dict for every documented section ───────────────────


def test_load_returns_dict_for_every_documented_section(temp_config):
    cfg = config.load()
    for name in config._SECTION_ORDER:
        assert name in cfg, f"load() omitted section {name!r}."
        assert isinstance(cfg[name], dict), (
            f"load()[{name!r}] must be a dict on a fresh install, "
            f"got {type(cfg[name]).__name__}."
        )


# ── (i) _SECTION_ORDER is duplicate-free ─────────────────────────────────────


def test_section_order_no_duplicates():
    assert len(config._SECTION_ORDER) == len(set(config._SECTION_ORDER)), (
        f"_SECTION_ORDER contains duplicates: {config._SECTION_ORDER}."
    )

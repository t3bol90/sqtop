"""Pure helpers for user-driven column reordering."""
from __future__ import annotations


def _reconcile_order(saved: object, default: list[str]) -> list[str]:
    """Return a column-name list that reconciles saved order with default columns.

    Rules:
      1. Result contains every name in ``default`` exactly once.
      2. Relative order of string entries in ``saved`` that exist in ``default``
         is preserved.
      3. Names in ``saved`` not present in ``default`` are dropped.
      4. Names in ``default`` not present in ``saved`` are appended in their
         default order.

    Malformed-input coercions:
      - Non-list ``saved`` is treated as empty.
      - Non-string entries inside ``saved`` are skipped.
      - Duplicate entries in ``saved`` are de-duplicated (first occurrence wins).
    """
    if not isinstance(saved, list):
        return list(default)

    default_set = set(default)
    seen: set[str] = set()
    ordered: list[str] = []

    for entry in saved:
        if not isinstance(entry, str):
            continue
        if entry in seen:
            continue
        seen.add(entry)
        if entry in default_set:
            ordered.append(entry)

    # Append default names not present in saved, in their original default order.
    for name in default:
        if name not in seen:
            ordered.append(name)

    return ordered


def _move_in_order(order: list[str], name: str, before: str | None) -> list[str]:
    """Return a new list with ``name`` repositioned.

    - If ``before`` is ``None``, ``name`` is moved to the end.
    - If ``name`` is not in ``order``, return ``order`` unchanged.
    - If ``before`` is not in ``order``, ``name`` is moved to the end.
    - Pure: does not mutate the input list.
    """
    if name not in order:
        return list(order)

    result = [x for x in order if x != name]

    if before is None or before not in result:
        result.append(name)
    else:
        idx = result.index(before)
        result.insert(idx, name)

    return result

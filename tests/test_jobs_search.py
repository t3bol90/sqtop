"""Tests for the JobsView search filter (`_job_matches_search`).

These exercise the pure helper in isolation so they don't need a mounted
Textual app. The filter logic in `JobsView._update_table` delegates to this
helper, so coverage here implicitly covers the filter pipeline.
"""
from __future__ import annotations

import pytest

from sqtop.slurm import Job
from sqtop.views.jobs import _job_matches_search


def _make_job(
    *,
    job_id: str = "12345",
    name: str = "training-run",
    user: str = "alice",
    state: str = "RUNNING",
    partition: str = "compute",
    reason: str = "",
    nodelist: str = "node001",
    qos: str = "normal",
) -> Job:
    return Job(
        job_id=job_id,
        name=name,
        user=user,
        state=state,
        partition=partition,
        nodes="1",
        num_nodes="1",
        num_cpus="8",
        time_used="00:02:10",
        time_limit="01:00:00",
        reason=reason,
        nodelist=nodelist,
        qos=qos,
    )


@pytest.fixture
def sample_job() -> Job:
    return _make_job(
        job_id="987654",
        name="train-resnet",
        user="bob",
        state="RUNNING",
        partition="gpu",
        reason="None",
        nodelist="node03",
        qos="gpu_high",
    )


def test_match_by_name(sample_job: Job) -> None:
    assert _job_matches_search(sample_job, "resnet") is True


def test_match_by_user(sample_job: Job) -> None:
    # The bug we are fixing: 'bob' (a username) should match.
    assert _job_matches_search(sample_job, "bob") is True


def test_match_by_qos(sample_job: Job) -> None:
    # The bug we are fixing: a qos string should match.
    assert _job_matches_search(sample_job, "gpu_high") is True


def test_match_by_reason() -> None:
    job = _make_job(reason="Resources", state="PENDING")
    # The bug we are fixing: pending reason should match.
    assert _job_matches_search(job, "Resources") is True


def test_match_by_nodelist(sample_job: Job) -> None:
    # The bug we are fixing: a nodelist entry should match.
    assert _job_matches_search(sample_job, "node03") is True


def test_match_by_state(sample_job: Job) -> None:
    assert _job_matches_search(sample_job, "running") is True


def test_match_by_partition(sample_job: Job) -> None:
    assert _job_matches_search(sample_job, "gpu") is True


def test_match_by_job_id(sample_job: Job) -> None:
    assert _job_matches_search(sample_job, "987654") is True
    # Substring match on job_id should also work.
    assert _job_matches_search(sample_job, "9876") is True


def test_case_insensitive(sample_job: Job) -> None:
    # 'BoB' must match user 'bob' (the new behavior is case-insensitive).
    assert _job_matches_search(sample_job, "BoB") is True
    assert _job_matches_search(sample_job, "RESNET") is True
    assert _job_matches_search(sample_job, "GPU_HIGH") is True


def test_empty_query_matches_everything(sample_job: Job) -> None:
    assert _job_matches_search(sample_job, "") is True
    # Also verify with a wholly different job.
    assert _job_matches_search(_make_job(), "") is True


def test_no_match_returns_false(sample_job: Job) -> None:
    assert _job_matches_search(sample_job, "definitely-not-present-xyzzy") is False


def test_handles_empty_optional_fields() -> None:
    # Job with all three optional fields empty must not crash and must still
    # match on the always-populated fields (e.g. name).
    job = _make_job(reason="", nodelist="", qos="")
    assert _job_matches_search(job, "training-run") is True
    # And a query that only existed in the cleared fields should miss.
    assert _job_matches_search(job, "node001") is False  # nodelist was cleared
    assert _job_matches_search(job, "normal") is False   # qos was cleared


def test_handles_none_optional_fields() -> None:
    # The Job dataclass declares reason/nodelist/qos as `str = ""`, so the
    # type system prevents these from being None in practice. The defensive
    # `(field or "")` guard in `_job_matches_search` still tolerates None
    # if an upstream parser ever returned it; we exercise that path here by
    # forcibly assigning None on a constructed instance.
    job = _make_job()
    job.reason = None  # type: ignore[assignment]
    job.nodelist = None  # type: ignore[assignment]
    job.qos = None  # type: ignore[assignment]
    # Must not raise AttributeError.
    assert _job_matches_search(job, "training-run") is True
    assert _job_matches_search(job, "no-such-text") is False


@pytest.mark.parametrize(
    "query,expected",
    [
        ("train", True),       # name substring
        ("alice", True),       # user
        ("compute", True),     # partition
        ("running", True),     # state
        ("normal", True),      # qos
        ("node001", True),     # nodelist
        ("12345", True),       # job_id
        ("missing", False),    # no field contains this
    ],
)
def test_match_matrix(query: str, expected: bool) -> None:
    job = _make_job()
    assert _job_matches_search(job, query) is expected

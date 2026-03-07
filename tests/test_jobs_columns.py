"""Regression tests for jobs table column sizing."""
from __future__ import annotations

from sqtop.slurm import Job
from sqtop.views.jobs import JobsView


class _FakeTable:
    def clear(self, columns: bool = False) -> None:
        return

    def add_column(self, name: str, width: int) -> None:
        return


def _job(name: str) -> Job:
    return Job(
        job_id="12345",
        name=name,
        user="alice",
        state="RUNNING",
        partition="compute",
        nodes="1",
        num_nodes="1",
        num_cpus="8",
        time_used="00:02:10",
        time_limit="01:00:00",
        reason="",
        nodelist="node001",
        qos="normal",
    )


def test_jobs_columns_rebuild_with_data_after_empty_startup(monkeypatch, temp_config):
    view = JobsView()
    monkeypatch.setattr(view, "query_one", lambda *args, **kwargs: _FakeTable())

    width = 200
    view._rebuild_columns(width, [], force=True)
    initial_name_width = dict(view._current_cols)["NAME"]

    view._rebuild_columns(width, [_job("very-long-job-name-001")])
    updated_name_width = dict(view._current_cols)["NAME"]

    assert updated_name_width > initial_name_width


def test_jobs_columns_rebuild_after_empty_transition(monkeypatch, temp_config):
    view = JobsView()
    monkeypatch.setattr(view, "query_one", lambda *args, **kwargs: _FakeTable())

    width = 200
    view._rebuild_columns(width, [_job("short")], force=True)
    first_name_width = dict(view._current_cols)["NAME"]

    # Queue goes empty for a while.
    view._rebuild_columns(width, [])

    # New data appears with longer names at same width; should rebuild.
    view._rebuild_columns(width, [_job("very-long-job-name-after-empty-transition")])
    second_name_width = dict(view._current_cols)["NAME"]

    assert second_name_width > first_name_width

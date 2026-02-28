from __future__ import annotations

from sqtop import slurm


def test_run_job_action_unsupported():
    result = slurm.run_job_action("noop", "100")
    assert result.ok is False
    assert "unsupported" in result.message


def test_run_bulk_job_action_aggregates(monkeypatch):
    def fake_run_job_action(action: str, job_id: str):
        return slurm.ActionResult(job_id=job_id, action=action, ok=(job_id != "2"), message="")

    monkeypatch.setattr(slurm, "run_job_action", fake_run_job_action)
    results = slurm.run_bulk_job_action("cancel", ["1", "2", "3"])
    assert len(results) == 3
    assert sum(1 for r in results if r.ok) == 2

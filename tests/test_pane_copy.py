"""Tests for copy_pane() on data-table views and modal screens."""
from __future__ import annotations

import types
from unittest.mock import MagicMock

import pytest

from sqtop.slurm import Job, Node, ClusterSummary, SacctJob


# ── helpers ────────────────────────────────────────────────────────────────────

def _make_job(**kwargs) -> Job:
    defaults = dict(
        job_id="1001",
        name="test_job",
        user="alice",
        state="RUNNING",
        partition="gpu",
        nodes="1",
        num_nodes="1",
        num_cpus="8",
        time_used="01:00:00",
        time_limit="08:00:00",
        reason="None",
        nodelist="node01",
        qos="normal",
    )
    defaults.update(kwargs)
    return Job(**defaults)


def _make_node(**kwargs) -> Node:
    defaults = dict(
        name="node01",
        state="idle",
        partition="gpu",
        cpus_total="32",
        cpus_alloc="0",
        memory_total="128000",
        memory_free="128000",
        load="0.00",
        gpu_total=4,
        gpu_alloc=0,
    )
    defaults.update(kwargs)
    return Node(**defaults)


def _make_summary(**kwargs) -> ClusterSummary:
    defaults = dict(
        partition="gpu",
        avail="up",
        timelimit="7-00:00:00",
        nodes="4",
        state="idle",
        nodelist="node[01-04]",
    )
    defaults.update(kwargs)
    return ClusterSummary(**defaults)


def _make_sacct_job(**kwargs) -> SacctJob:
    defaults = dict(
        job_id="2001",
        name="done_job",
        user="bob",
        state="COMPLETED",
        num_cpus="4",
        elapsed="00:30:00",
        exit_code="0:0",
        partition="cpu",
    )
    defaults.update(kwargs)
    return SacctJob(**defaults)


# ── JobsView ───────────────────────────────────────────────────────────────────

class _FakeJobsView:
    """Minimal stand-in for JobsView that provides copy_pane() without Textual wiring."""

    def __init__(self, jobs, cols):
        self._last_jobs = jobs
        self._current_cols = cols

    def _plain_cell(self, job: Job, col_name: str) -> str:
        mapping = {
            "JOBID": job.job_id,
            "NAME": job.name,
            "STATE": job.state,
            "USER": job.user,
            "TIME": job.time_used,
            "PARTITION": job.partition,
            "QOS": job.qos or "",
            "NODES": job.nodes,
            "CPUS": job.num_cpus,
            "TIME_LIMIT": job.time_limit,
        }
        return mapping.get(col_name, job.nodelist or job.reason)

    def _pane_label(self) -> str:
        return "Jobs"

    def _current_items(self):
        return list(self._last_jobs)

    def _row_tsv(self, item: Job) -> str:
        return "\t".join(self._plain_cell(item, name) for name, _ in self._current_cols)

    def copy_pane(self):
        header = "\t".join(name for name, _ in self._current_cols)
        items = self._current_items()
        rows = [self._row_tsv(item) for item in items]
        payload = "\n".join([header, *rows]) + "\n"
        return self._pane_label(), payload, len(rows)


def test_jobs_copy_pane_empty():
    """Empty pane still copies header + trailing newline, 0 rows."""
    cols = [("JOBID", 8), ("NAME", 8), ("STATE", 10)]
    view = _FakeJobsView([], cols)
    label, payload, count = view.copy_pane()
    assert label == "Jobs"
    assert count == 0
    assert payload == "JOBID\tNAME\tSTATE\n"


def test_jobs_copy_pane_two_rows():
    """Two jobs produce header + 2 TSV rows + trailing newline."""
    cols = [("JOBID", 8), ("NAME", 8), ("STATE", 10), ("USER", 8)]
    jobs = [
        _make_job(job_id="1001", name="jobA", state="RUNNING", user="alice"),
        _make_job(job_id="1002", name="jobB", state="PENDING", user="bob"),
    ]
    view = _FakeJobsView(jobs, cols)
    label, payload, count = view.copy_pane()
    assert count == 2
    lines = payload.rstrip("\n").splitlines()
    assert lines[0] == "JOBID\tNAME\tSTATE\tUSER"
    assert lines[1] == "1001\tjobA\tRUNNING\talice"
    assert lines[2] == "1002\tjobB\tPENDING\tbob"
    assert payload.endswith("\n")


def test_jobs_copy_pane_uses_filtered_list():
    """copy_pane reads _last_jobs (filtered), not a raw list."""
    cols = [("JOBID", 8), ("STATE", 10)]
    all_jobs = [
        _make_job(job_id="1001", state="RUNNING"),
        _make_job(job_id="1002", state="PENDING"),
    ]
    filtered = all_jobs[:1]  # simulate "mine" filter
    view = _FakeJobsView(filtered, cols)
    label, payload, count = view.copy_pane()
    assert count == 1
    assert "1001" in payload
    assert "1002" not in payload


# ── NodesView ─────────────────────────────────────────────────────────────────

class _FakeNodesView:
    def __init__(self, nodes, cols):
        self._last_sorted_nodes = nodes
        self._current_cols = cols

    def _plain_cell(self, node: Node, col_name: str) -> str:
        if col_name == "NODE":
            return node.name
        if col_name == "STATE":
            return node.state
        if col_name == "CPUS A/T":
            return f"{node.cpus_alloc}/{node.cpus_total}"
        if col_name == "MEM FREE":
            return f"{node.memory_free}M"
        if col_name == "PARTITION":
            return node.partition
        return ""

    def _pane_label(self):
        return "Nodes"

    def _current_items(self):
        return list(self._last_sorted_nodes)

    def _row_tsv(self, item):
        return "\t".join(self._plain_cell(item, name) for name, _ in self._current_cols)

    def copy_pane(self):
        header = "\t".join(name for name, _ in self._current_cols)
        items = self._current_items()
        rows = [self._row_tsv(item) for item in items]
        payload = "\n".join([header, *rows]) + "\n"
        return self._pane_label(), payload, len(rows)


def test_nodes_copy_pane_basic():
    cols = [("NODE", 12), ("STATE", 12), ("CPUS A/T", 10)]
    nodes = [
        _make_node(name="node01", state="idle", cpus_alloc="0", cpus_total="32"),
        _make_node(name="node02", state="allocated", cpus_alloc="32", cpus_total="32"),
    ]
    view = _FakeNodesView(nodes, cols)
    label, payload, count = view.copy_pane()
    assert label == "Nodes"
    assert count == 2
    lines = payload.rstrip("\n").splitlines()
    assert lines[0] == "NODE\tSTATE\tCPUS A/T"
    assert lines[1] == "node01\tidle\t0/32"
    assert lines[2] == "node02\tallocated\t32/32"
    assert payload.endswith("\n")


def test_nodes_copy_pane_empty():
    cols = [("NODE", 12), ("STATE", 12)]
    view = _FakeNodesView([], cols)
    label, payload, count = view.copy_pane()
    assert count == 0
    assert payload == "NODE\tSTATE\n"


# ── PartitionsView ─────────────────────────────────────────────────────────────

class _FakePartitionsView:
    def __init__(self, summaries):
        self._last_sorted_rows = summaries

    def _visible_cols_filtered(self):
        # Use all COLUMNS
        from sqtop.views.partitions import COLUMNS
        return list(COLUMNS)

    def _plain_cell(self, s: ClusterSummary, name: str) -> str:
        if name == "PARTITION":
            return s.partition
        if name == "AVAIL":
            return s.avail
        if name == "TIMELIMIT":
            return s.timelimit
        if name == "NODES":
            return s.nodes
        if name == "STATE":
            return s.state
        return s.nodelist

    def _pane_label(self):
        return "Partitions"

    def _current_items(self):
        return list(self._last_sorted_rows)

    def _row_tsv(self, item):
        visible = self._visible_cols_filtered()
        return "\t".join(self._plain_cell(item, name) for name, _ in visible)

    def copy_pane(self):
        visible = self._visible_cols_filtered()
        header = "\t".join(name for name, _ in visible)
        items = self._current_items()
        rows = [self._row_tsv(item) for item in items]
        payload = "\n".join([header, *rows]) + "\n"
        return self._pane_label(), payload, len(rows)


def test_partitions_copy_pane_basic():
    summaries = [
        _make_summary(partition="gpu", avail="up", state="idle", nodes="4"),
        _make_summary(partition="cpu", avail="up", state="mixed", nodes="8"),
    ]
    view = _FakePartitionsView(summaries)
    label, payload, count = view.copy_pane()
    assert label == "Partitions"
    assert count == 2
    lines = payload.rstrip("\n").splitlines()
    assert lines[0].startswith("PARTITION\t")
    assert "gpu" in lines[1]
    assert "cpu" in lines[2]
    assert payload.endswith("\n")


# ── HistoryView ────────────────────────────────────────────────────────────────

class _FakeHistoryView:
    def __init__(self, jobs):
        self._last_jobs = jobs

    def _plain_cell(self, job: SacctJob, col_name: str) -> str:
        mapping = {
            "JOBID": job.job_id,
            "NAME": job.name,
            "USER": job.user,
            "STATE": job.state,
            "ELAPSED": job.elapsed,
            "EXIT": job.exit_code,
            "PARTITION": job.partition,
        }
        return mapping.get(col_name, "")

    def _pane_label(self):
        return "History"

    def _current_items(self):
        return list(self._last_jobs)

    def _row_tsv(self, item):
        from sqtop.views.history import COLUMNS
        return "\t".join(self._plain_cell(item, name) for name, _ in COLUMNS)

    def copy_pane(self):
        from sqtop.views.history import COLUMNS
        header = "\t".join(name for name, _ in COLUMNS)
        items = self._current_items()
        rows = [self._row_tsv(item) for item in items]
        payload = "\n".join([header, *rows]) + "\n"
        return self._pane_label(), payload, len(rows)


def test_history_copy_pane_basic():
    jobs = [
        _make_sacct_job(job_id="2001", name="done_job", user="bob", state="COMPLETED"),
        _make_sacct_job(job_id="2002", name="fail_job", user="alice", state="FAILED"),
    ]
    view = _FakeHistoryView(jobs)
    label, payload, count = view.copy_pane()
    assert label == "History"
    assert count == 2
    lines = payload.rstrip("\n").splitlines()
    assert lines[0].startswith("JOBID\t")
    assert "2001" in lines[1]
    assert "2002" in lines[2]
    assert payload.endswith("\n")


def test_history_copy_pane_empty():
    view = _FakeHistoryView([])
    label, payload, count = view.copy_pane()
    assert count == 0
    assert payload.endswith("\n")
    assert "JOBID" in payload


# ── Modal screens ─────────────────────────────────────────────────────────────

def test_batch_script_copy_pane():
    """BatchScriptScreen.copy_pane() returns the script body."""
    from sqtop.views.batch_script import BatchScriptScreen

    screen = BatchScriptScreen.__new__(BatchScriptScreen)
    screen._job_id = "999"
    screen._script = "#!/bin/bash\nsrun python train.py\n"

    label, payload, count = screen.copy_pane()
    assert label == "Batch Script job 999"
    assert payload == "#!/bin/bash\nsrun python train.py\n"
    assert count == 2


def test_log_viewer_copy_pane():
    """LogViewerScreen.copy_pane() returns the buffered log content."""
    from sqtop.views.log_viewer import LogViewerScreen

    screen = LogViewerScreen.__new__(LogViewerScreen)
    screen._job_id = "42"
    screen._log_path = "/var/log/slurm/42.out"
    screen._log_type = "stdout"
    screen._last_content = "line1\nline2\nline3\n"

    label, payload, count = screen.copy_pane()
    assert "42" in label
    assert payload == "line1\nline2\nline3\n"
    assert count == 3


def test_job_info_copy_pane():
    """JobInfoScreen.copy_pane() strips Rich markup and returns plain text."""
    from sqtop.views.job_info import JobInfoScreen

    job = _make_job(job_id="777", name="my_job")
    screen = JobInfoScreen.__new__(JobInfoScreen)
    screen._job = job
    screen._markup_text = "[bold cyan]── Identity ──[/bold cyan]\n  [bold]Job ID:[/bold]     777\n"

    label, payload, count = screen.copy_pane()
    assert "777" in label
    assert "[bold" not in payload
    assert "777" in payload
    assert count > 0


def test_detail_view_plain_text():
    """DetailView.plain_text() returns markup-free key=value lines."""
    from sqtop.views.detail import DetailView

    # Instantiate without Textual app context
    view = DetailView.__new__(DetailView)
    view._plain_title = "Job Detail"
    view._plain_data = {"JobId": "123", "JobName": "test"}

    text = view.plain_text()
    assert "Job Detail" in text
    assert "JobId: 123" in text
    assert "JobName: test" in text
    assert "[" not in text  # no markup


def test_job_detail_copy_pane_fallback():
    """JobDetailScreen.copy_pane() falls back to _data when DetailView is not mounted."""
    from sqtop.views.job_detail import JobDetailScreen

    screen = JobDetailScreen.__new__(JobDetailScreen)
    screen._job_id = "555"
    screen._data = {"JobId": "555", "JobName": "my_job", "JobState": "RUNNING"}

    # Simulate query_one failing (widget not mounted)
    def _raise(*args, **kwargs):
        raise Exception("not mounted")

    screen.query_one = _raise

    label, payload, count = screen.copy_pane()
    assert "555" in label
    assert "JobId: 555" in payload
    assert count > 0


def test_node_detail_copy_pane_fallback():
    """NodeDetailScreen.copy_pane() falls back to _detail_data when not mounted."""
    from sqtop.views.node_detail import NodeDetailScreen

    node = _make_node(name="node01")
    screen = NodeDetailScreen.__new__(NodeDetailScreen)
    screen._node = node
    screen._detail_data = {"NodeName": "node01", "State": "idle", "CPUTot": "32"}

    def _raise(*args, **kwargs):
        raise Exception("not mounted")

    screen.query_one = _raise

    label, payload, count = screen.copy_pane()
    assert "node01" in label
    assert "NodeName: node01" in payload
    assert count > 0


# ── TSV exact snapshot ────────────────────────────────────────────────────────

def test_jobs_tsv_exact_snapshot():
    """Exact byte-identical TSV snapshot for reproducibility."""
    cols = [("JOBID", 8), ("NAME", 12), ("STATE", 10), ("USER", 8)]
    jobs = [_make_job(job_id="100", name="train", state="RUNNING", user="alice")]
    view = _FakeJobsView(jobs, cols)
    _, payload, _ = view.copy_pane()
    expected = "JOBID\tNAME\tSTATE\tUSER\n100\ttrain\tRUNNING\talice\n"
    assert payload == expected

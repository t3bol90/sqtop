from __future__ import annotations

from sqtop import slurm


def test_resolve_first_node_uses_scontrol_hostnames(monkeypatch):
    monkeypatch.setattr(slurm, "_run", lambda cmd: "c1\nc2\n")
    assert slurm.resolve_first_node("c[1-2]") == "c1"


def test_resolve_first_node_falls_back_when_scontrol_fails(monkeypatch):
    monkeypatch.setattr(slurm, "_run", lambda cmd: "")
    assert slurm.resolve_first_node("c1,c2,c3") == "c1"


def test_build_attach_command_with_node_and_extra_args():
    cmd = slurm.build_attach_command(
        job_id="12345",
        node="c2",
        default_command="bash -l",
        extra_args="--mpi=none",
    )
    assert cmd == [
        "srun",
        "--pty",
        "--overlap",
        "--mpi=none",
        "--jobid",
        "12345",
        "-w",
        "c2",
        "bash",
        "-l",
    ]


def test_build_attach_command_without_node():
    cmd = slurm.build_attach_command(
        job_id="12345",
        node=None,
        default_command="bash -l",
        extra_args="",
    )
    assert "-w" not in cmd

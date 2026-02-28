#!/usr/bin/env bash
# Helper script for managing the local Slurm dev cluster

set -e

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
UPSTREAM_DIR="$SCRIPT_DIR/../slurm-docker-upstream"

CMD="${1:-help}"

run_compose() {
  docker compose -f "$UPSTREAM_DIR/docker-compose.yml" "$@"
}

case "$CMD" in
  build)
    echo "Building Slurm image from source (takes ~10-15 min)..."
    docker compose -f "$UPSTREAM_DIR/docker-compose.yml" build
    ;;

  up)
    echo "Starting Slurm cluster (c1-c4)..."
    run_compose up -d mysql slurmdbd slurmctld slurmrestd c1 c2 c3 c4
    echo ""
    echo "Waiting for cluster to be ready..."
    sleep 15
    docker exec slurmctld sinfo
    ;;

  down)
    echo "Stopping Slurm cluster..."
    run_compose down
    ;;

  clean)
    echo "Removing cluster and all volumes..."
    run_compose down -v
    ;;

  status)
    docker exec slurmctld sinfo
    echo ""
    docker exec slurmctld squeue
    ;;

  shell)
    docker exec -it slurmctld bash
    ;;

  submit-test)
    echo "Submitting test jobs (with live log output)..."
    docker exec slurmctld bash -c '
      mkdir -p /tmp/slurm-logs

      # Long-running jobs that write a timestamped line every second (good for follow mode)
      for i in $(seq 1 5); do
        sbatch --job-name="tail-$i" \
               --ntasks=1 \
               --time=01:00:00 \
               --output=/tmp/slurm-logs/%j.out \
               --error=/tmp/slurm-logs/%j.err \
               --wrap="
                 echo \"=== Job \$SLURM_JOB_ID started at \$(date) on \$SLURMD_NODENAME ===\"
                 step=0
                 while true; do
                   step=\$((step + 1))
                   echo \"[\$(date +%H:%M:%S)] step \$step  job=\$SLURM_JOB_ID  node=\$SLURMD_NODENAME\"
                   if [ \$((step % 10)) -eq 0 ]; then
                     echo \"[WARN] heartbeat at step \$step\" >&2
                   fi
                   sleep 1
                 done
               "
      done

      # A few quick jobs that complete (to test COMPLETED state log viewing)
      for i in $(seq 1 3); do
        sbatch --job-name="quick-$i" \
               --ntasks=1 \
               --time=00:05:00 \
               --output=/tmp/slurm-logs/%j.out \
               --error=/tmp/slurm-logs/%j.err \
               --wrap="
                 echo \"=== quick-$i started at \$(date) ===\"
                 for step in \$(seq 1 20); do
                   echo \"[\$(date +%H:%M:%S)] step \$step / 20\"
                   sleep 1
                 done
                 echo \"=== done ===\"
               "
      done
    '
    echo "Submitted 8 jobs (5 long-running + 3 quick). Logs go to /tmp/slurm-logs/ inside slurmctld."
    docker exec slurmctld squeue
    ;;

  logs)
    SERVICE="${2:-slurmctld}"
    run_compose logs -f "$SERVICE"
    ;;

  nodes)
    docker exec slurmctld scontrol show nodes
    ;;

  jobs)
    docker exec slurmctld scontrol show jobs
    ;;

  help|*)
    cat <<EOF
Usage: ./cluster.sh <command> [args]

Commands:
  build        Build the Slurm Docker image from source
  up           Start the cluster (c1-c4 + controller)
  down         Stop the cluster
  clean        Stop and remove all volumes (fresh start)
  status       Show sinfo + squeue
  shell        Open bash on slurmctld (controller)
  submit-test  Submit 8 jobs that write live output to /tmp/slurm-logs/
  logs [svc]   Tail logs (default: slurmctld)
  nodes        scontrol show nodes
  jobs         scontrol show jobs
  help         This message
EOF
    ;;
esac

#!/usr/bin/env bash
# Size Cargo's parallel compiles to the memory the job may use, not to the CPUs it sees: inside a
# sandbox (gVisor on the shared untrusted boxes) nproc reports the host's cores while the memory
# limit is the sandbox's, and one rustc per core on the big crates runs out of memory (rustc
# killed, "exit status: 2" with no diagnostic). Sets CARGO_BUILD_JOBS for the rest of the job.
# Usage: build-jobs.sh [GB_PER_JOB] (default 3)
set -euo pipefail
per_job=${1:-3}
kb=$(awk '/MemAvailable/{print $2}' /proc/meminfo)
# A cgroup limit, where there is one, can be lower than what /proc/meminfo reports.
for f in /sys/fs/cgroup/memory.max /sys/fs/cgroup/memory/memory.limit_in_bytes; do
  if [ -r "$f" ]; then
    limit=$(cat "$f")
    if [ "$limit" != max ] && [ "$((limit / 1024))" -lt "$kb" ]; then
      kb=$((limit / 1024))
    fi
    break
  fi
done
cpus=$(nproc)
jobs=$((kb / 1048576 / per_job))
[ "$jobs" -lt 1 ] && jobs=1
[ "$jobs" -gt "$cpus" ] && jobs=$cpus
echo "build-jobs: $((kb / 1048576)) GB available, $cpus CPUs -> CARGO_BUILD_JOBS=$jobs"
echo "CARGO_BUILD_JOBS=$jobs" >> "$GITHUB_ENV"

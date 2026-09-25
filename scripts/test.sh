#!/usr/bin/env bash
# Run the suite with the temp root emptied first, and again after.
#
# `.cargo/config.toml` points TMPDIR at `scratch/`, so every fixture directory
# in the workspace lands inside the repository instead of a shared /tmp that
# once filled a disk. That moved the pile; it did not collect it. A run killed
# by a signal, by the OOM killer, or by Ctrl-C leaves its `tempfile` guards
# undropped, and their directories stay behind until something removes them —
# which is what this script is. The sources are bounded on their own (fixed
# names and counters, enforced by `no_crate_names_a_temp_path_that_grows`), so
# what remains here is only what an interrupted run abandoned.
#
# Usage:
#   ./scripts/test.sh                       # the whole workspace
#   ./scripts/test.sh -p aria-ui --lib      # anything cargo test accepts
#
# JOBS caps compiler parallelism. The default leaves half the machine to its
# owner: a full-width build of this workspace saturates every core for minutes,
# and the editor being built is also the editor being used.

set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

SCRATCH="$ROOT/scratch"
JOBS="${JOBS:-$(( $(nproc) / 2 ))}"

sweep() {
    [[ -d "$SCRATCH" ]] || return 0
    local before after
    before="$(find "$SCRATCH" -mindepth 1 -maxdepth 1 | wc -l)"
    find "$SCRATCH" -mindepth 1 -maxdepth 1 ! -name .gitkeep -exec rm -rf {} +
    after="$(find "$SCRATCH" -mindepth 1 -maxdepth 1 | wc -l)"
    if (( before != after )); then
        echo "==> swept $(( before - after )) leftover entries from scratch/"
    fi
}

# Fail closed: without the redirect, fixtures write to a shared temp directory
# and this sweep would be cleaning a directory nothing writes to.
if [[ ! -f "$ROOT/.cargo/config.toml" ]] || ! grep -q 'TMPDIR' "$ROOT/.cargo/config.toml"; then
    echo "error: .cargo/config.toml no longer redirects TMPDIR into the workspace" >&2
    exit 1
fi

mkdir -p "$SCRATCH"
sweep
trap sweep EXIT

echo "==> cargo test -j $JOBS ${*:-(whole workspace)}"
cargo test -j "$JOBS" "$@"

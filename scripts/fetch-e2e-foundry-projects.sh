#!/usr/bin/env bash
# Download external Foundry projects for edb's e2e integration tests.
# Idempotent: skips repos already at the pinned ref.

set -euo pipefail

DEST="${EDB_E2E_FIXTURES_DIR:-testdata/foundry-e2e}"
mkdir -p "$DEST"

# Clone or update a repo to the tip of the given branch.
# Uses the local remote-tracking ref for idempotency; no network call when up-to-date.
clone_at() {
    local url=$1 branch=$2 dir=$3
    local target="$DEST/$dir"
    if [[ -d "$target/.git" ]]; then
        # Fetch the branch to get the latest remote-tracking ref.
        git -C "$target" fetch --depth 1 origin "$branch" 2>/dev/null
        local current desired
        current=$(git -C "$target" rev-parse HEAD 2>/dev/null || echo "")
        desired=$(git -C "$target" rev-parse FETCH_HEAD 2>/dev/null || echo "")
        if [[ -n "$current" && -n "$desired" && "$current" == "$desired" ]]; then
            echo "[skip] $dir already at $branch ($current)"
            return 0
        fi
        echo "[update] $dir -> $branch"
        git -C "$target" checkout --detach FETCH_HEAD
    else
        echo "[clone] $dir at $branch"
        rm -rf "$target"
        git clone --depth 1 --branch "$branch" "$url" "$target"
    fi
    # Init submodules shallowly (forge-template/solady each rely on forge-std):
    git -C "$target" submodule update --init --recursive --depth 1 2>/dev/null || true
}

clone_at "https://github.com/foundry-rs/forge-template" "master" "forge-template"
clone_at "https://github.com/Vectorized/solady"           "main"   "solady"
clone_at "https://github.com/Uniswap/v4-core"             "main"   "uniswap-v4-core"
clone_at "https://github.com/transmissions11/solmate"      "main"   "solmate"
clone_at "https://github.com/PaulRBerg/prb-math"           "main"   "prb-math"

echo "Done. Projects in: $DEST"

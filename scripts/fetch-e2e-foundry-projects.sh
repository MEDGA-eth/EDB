#!/usr/bin/env bash
# Download external Foundry projects for edb's e2e integration tests.
# Idempotent: skips repos already at the pinned commit.
#
# Each project is pinned to a specific commit hash so that the test suite is
# reproducible and CI cache keys (hashFiles on this script) invalidate exactly
# when a pinned commit is bumped.

set -euo pipefail

DEST="${EDB_E2E_FIXTURES_DIR:-testdata/foundry-e2e}"
mkdir -p "$DEST"

# Clone or update a repo to a specific pinned commit.
# ref_hint is the branch name used only for the initial shallow clone when the
# commit is not yet available locally; the checkout always targets $commit.
clone_at_commit() {
    local url=$1 ref_hint=$2 commit=$3 dir=$4
    local target="$DEST/$dir"
    if [[ -d "$target/.git" ]]; then
        local current
        current=$(git -C "$target" rev-parse HEAD 2>/dev/null || echo "")
        if [[ "$current" == "$commit" ]]; then
            echo "[skip] $dir already at $commit"
            return 0
        fi
        echo "[update] $dir -> $commit"
        git -C "$target" fetch --depth 1 origin "$commit" 2>/dev/null \
            || git -C "$target" fetch origin "$ref_hint"
        git -C "$target" checkout --detach "$commit" 2>/dev/null \
            || git -C "$target" checkout --detach FETCH_HEAD
    else
        echo "[clone] $dir at $commit"
        rm -rf "$target"
        git clone --depth 1 --branch "$ref_hint" "$url" "$target" 2>/dev/null \
            || git clone --depth 1 "$url" "$target"
        git -C "$target" fetch --depth 1 origin "$commit" 2>/dev/null || true
        git -C "$target" checkout --detach "$commit" 2>/dev/null || true
    fi
    git -C "$target" submodule update --init --recursive --depth 1 2>/dev/null || true
}

# Pinned commits — tested and known-good as of the commits in this repo.
# To update a project: run `git -C testdata/foundry-e2e/<dir> rev-parse HEAD`
# after fetching the desired version, then update the commit hash below and
# commit this script (which also invalidates the CI cache automatically).
clone_at_commit "https://github.com/foundry-rs/forge-template" "master" \
    "f5db6aeeff588c8a789b6f7da83313950fd97178" "forge-template"
clone_at_commit "https://github.com/Vectorized/solady" "main" \
    "90db92ce173856605d24a554969f2c67cadbc7e9" "solady"
clone_at_commit "https://github.com/Uniswap/v4-core" "main" \
    "46c6834698c48bc4a463a86d8420f4eb1d7f3b75" "uniswap-v4-core"
clone_at_commit "https://github.com/transmissions11/solmate" "main" \
    "89365b880c4f3c786bdd453d4b8e8fe410344a69" "solmate"
clone_at_commit "https://github.com/PaulRBerg/prb-math" "main" \
    "82e5ed5561d0a1c43a3a59edbf4291c8de26479e" "prb-math"

# prb-math fetches forge-std via npm (see package.json), not git submodules.
# Materialize `node_modules/` so foundry-compilers can resolve
# `forge-std/src/StdAssertions.sol`. We prefer `bun install` (already on CI)
# and fall back to `npm install --omit=dev` so devs without bun can still
# run this script.
if [[ -f "$DEST/prb-math/package.json" && ! -d "$DEST/prb-math/node_modules/forge-std" ]]; then
    echo "[npm] populating prb-math node_modules (forge-std dep)"
    if command -v bun >/dev/null 2>&1; then
        (cd "$DEST/prb-math" && bun install --frozen-lockfile 2>/dev/null \
            || bun install)
    elif command -v npm >/dev/null 2>&1; then
        (cd "$DEST/prb-math" && npm install --omit=dev --silent)
    else
        echo "[warn] neither bun nor npm available — prb-math tests will fail to compile"
    fi
fi

echo "Done. Projects in: $DEST"

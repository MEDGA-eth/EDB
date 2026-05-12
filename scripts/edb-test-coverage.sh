#!/usr/bin/env bash
# Run edb test --no-ui against every test in the vendored real-world Foundry
# projects and report a coverage tally.
#
# Usage:
#   ./scripts/edb-test-coverage.sh            # run all tests
#   ./scripts/edb-test-coverage.sh --limit 5  # run at most 5 tests per project
#
# Prerequisites:
#   ./scripts/fetch-e2e-foundry-projects.sh   # populate testdata/foundry-e2e/
#
# Output: one JSON line per test to stdout, summary table to stderr.

set -uo pipefail   # intentionally NOT -e: failures are recorded, not fatal

# ── argument parsing ────────────────────────────────────────────────────────
LIMIT=0  # 0 = unlimited
while [[ $# -gt 0 ]]; do
    case "$1" in
        --limit)
            LIMIT="${2:?--limit requires a value}"
            shift 2
            ;;
        --limit=*)
            LIMIT="${1#*=}"
            shift
            ;;
        *)
            echo "Unknown argument: $1" >&2
            echo "Usage: $0 [--limit N]" >&2
            exit 1
            ;;
    esac
done

# ── paths ────────────────────────────────────────────────────────────────────
REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
EDB="$REPO_ROOT/target/release/edb"
DEST="${EDB_E2E_FIXTURES_DIR:-$REPO_ROOT/testdata/foundry-e2e}"

# ── preflight checks ─────────────────────────────────────────────────────────
if [[ ! -d "$DEST/forge-template/test" ]] || [[ ! -d "$DEST/solady/test" ]]; then
    echo "ERROR: e2e fixtures not found at $DEST" >&2
    echo "Run: ./scripts/fetch-e2e-foundry-projects.sh" >&2
    exit 2
fi

# ── build edb in release mode ─────────────────────────────────────────────────
echo "Building edb (release)..." >&2
cargo build --release --bin edb --manifest-path "$REPO_ROOT/Cargo.toml" 2>&1 \
    | tail -5 >&2
if [[ ! -x "$EDB" ]]; then
    echo "ERROR: $EDB not found after build" >&2
    exit 3
fi

# ── counters ─────────────────────────────────────────────────────────────────
declare -A counts
counts=(
    [ok]=0
    [edb-rejected]=0
    [test-revert]=0
    [engine-panic]=0
    [timeout]=0
    [unknown]=0
)

# ── per-test runner ───────────────────────────────────────────────────────────
run_test() {
    local project="$1" contract="$2" testfn="$3"
    local target="${contract}::${testfn}"
    printf "  %-60s " "$target" >&2

    local out rc
    out=$(timeout 60 "$EDB" test "$target" --root "$DEST/$project" --no-ui 2>/dev/null)
    rc=$?

    if [[ $rc -eq 124 ]]; then
        echo "TIMEOUT" >&2
        counts[timeout]=$((counts[timeout] + 1))
        printf '{"project":"%s","target":"%s","status":"timeout"}\n' "$project" "$target"
        return
    fi

    if [[ $rc -ne 0 ]]; then
        echo "PANIC(rc=$rc)" >&2
        counts[engine-panic]=$((counts[engine-panic] + 1))
        printf '{"project":"%s","target":"%s","status":"engine-panic","rc":%d}\n' \
            "$project" "$target" "$rc"
        return
    fi

    # Last non-empty line of stdout is the JSON summary from --no-ui.
    local summary
    summary=$(printf '%s\n' "$out" | grep -v '^$' | tail -1)

    local status
    status=$(printf '%s' "$summary" \
        | python3 -c "import json,sys; d=json.load(sys.stdin); print(d.get('status','unknown'))" \
        2>/dev/null || echo "unknown")

    echo "$status" >&2

    # Increment counter; fall back to unknown for any unrecognised status.
    if [[ -v "counts[$status]" ]]; then
        counts[$status]=$((counts[$status] + 1))
    else
        counts[unknown]=$((counts[unknown] + 1))
    fi

    # Emit the raw JSON line (with project added) or a fallback.
    if [[ -n "$summary" ]]; then
        # Inject "project" field into the existing JSON object.
        printf '%s' "$summary" \
            | python3 -c "
import json, sys
d = json.load(sys.stdin)
d['project'] = '$project'
print(json.dumps(d))
" 2>/dev/null || printf '{"project":"%s","target":"%s","status":"%s"}\n' \
            "$project" "$target" "$status"
    else
        printf '{"project":"%s","target":"%s","status":"unknown","note":"empty output"}\n' \
            "$project" "$target"
    fi
}

# ── project walker ────────────────────────────────────────────────────────────
walk_project() {
    local project="$1"
    echo "" >&2
    echo "## $project" >&2
    local count=0

    for sol in "$DEST/$project"/test/*.t.sol; do
        [[ -f "$sol" ]] || continue
        local contract
        contract=$(basename "$sol" .t.sol)

        while IFS= read -r testfn; do
            [[ -z "$testfn" ]] && continue
            if [[ "$LIMIT" -gt 0 && "$count" -ge "$LIMIT" ]]; then
                echo "  (limit $LIMIT reached, stopping)" >&2
                return
            fi
            run_test "$project" "$contract" "$testfn"
            count=$((count + 1))
        done < <(grep -oE 'function (test|testFuzz|testFork|invariant_)[A-Za-z0-9_]+' "$sol" \
                 | sed -E 's/^function //' \
                 | sort -u)
    done
}

# ── main ──────────────────────────────────────────────────────────────────────
echo "Coverage run started: $(date)" >&2
[[ "$LIMIT" -gt 0 ]] && echo "(limit: $LIMIT tests per project)" >&2

walk_project "forge-template"
walk_project "solady"

echo "" >&2
echo "=== Coverage summary ===" >&2
total=0
for k in ok edb-rejected test-revert engine-panic timeout unknown; do
    v="${counts[$k]}"
    printf "  %-15s %d\n" "$k" "$v" >&2
    total=$((total + v))
done
printf "  %-15s %d\n" "TOTAL" "$total" >&2
echo "Coverage run finished: $(date)" >&2

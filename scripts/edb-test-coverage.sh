#!/usr/bin/env bash
# Run edb test --no-ui against every test in the vendored real-world Foundry
# projects and report a coverage tally.
#
# Usage:
#   ./scripts/edb-test-coverage.sh            # run all tests
#   ./scripts/edb-test-coverage.sh --limit 5  # run at most N tests per project
#   ./scripts/edb-test-coverage.sh --sample N # run every Nth test per project (sampling)
#
# Prerequisites:
#   ./scripts/fetch-e2e-foundry-projects.sh   # populate testdata/foundry-e2e/
#
# Output: one JSON line per test to stdout, summary table to stderr.

set -uo pipefail   # intentionally NOT -e: failures are recorded, not fatal

# ── argument parsing ────────────────────────────────────────────────────────
LIMIT=0       # 0 = unlimited
SAMPLE=1      # 1 = no sampling (run every test), N = run every Nth test
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
        --sample)
            SAMPLE="${2:?--sample requires a value}"
            shift 2
            ;;
        --sample=*)
            SAMPLE="${1#*=}"
            shift
            ;;
        *)
            echo "Unknown argument: $1" >&2
            echo "Usage: $0 [--limit N] [--sample N]" >&2
            exit 1
            ;;
    esac
done

# ── paths ────────────────────────────────────────────────────────────────────
REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
EDB="$REPO_ROOT/target/release/edb"
DEST="${EDB_E2E_FIXTURES_DIR:-$REPO_ROOT/testdata/foundry-e2e}"

# ── preflight checks ─────────────────────────────────────────────────────────
if [[ ! -d "$DEST/forge-template" ]] || [[ ! -d "$DEST/solady" ]]; then
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

# ── per-project counters (plain variables, bash 3.2 compatible) ──────────────
# forge-template
ft_ok=0; ft_rejected=0; ft_revert=0; ft_panic=0; ft_timeout=0; ft_unknown=0; ft_total=0
# solady
sl_ok=0; sl_rejected=0; sl_revert=0; sl_panic=0; sl_timeout=0; sl_unknown=0; sl_total=0
# uniswap-v4-core
uv_ok=0; uv_rejected=0; uv_revert=0; uv_panic=0; uv_timeout=0; uv_unknown=0; uv_total=0
# solmate
sm_ok=0; sm_rejected=0; sm_revert=0; sm_panic=0; sm_timeout=0; sm_unknown=0; sm_total=0
# prb-math
pm_ok=0; pm_rejected=0; pm_revert=0; pm_panic=0; pm_timeout=0; pm_unknown=0; pm_total=0
# grand totals
grand_ok=0; grand_rejected=0; grand_revert=0; grand_panic=0
grand_timeout=0; grand_unknown=0; grand_total=0

# ── increment project counters ────────────────────────────────────────────────
# Usage: inc_counters <project_prefix> <status>
inc_counters() {
    local pfx="$1"
    local status="$2"
    eval "${pfx}_total=\$(( \${${pfx}_total} + 1 ))"
    grand_total=$((grand_total + 1))
    case "$status" in
        ok)
            eval "${pfx}_ok=\$(( \${${pfx}_ok} + 1 ))"
            grand_ok=$((grand_ok + 1))
            ;;
        edb-rejected)
            eval "${pfx}_rejected=\$(( \${${pfx}_rejected} + 1 ))"
            grand_rejected=$((grand_rejected + 1))
            ;;
        test-revert)
            eval "${pfx}_revert=\$(( \${${pfx}_revert} + 1 ))"
            grand_revert=$((grand_revert + 1))
            ;;
        engine-panic)
            eval "${pfx}_panic=\$(( \${${pfx}_panic} + 1 ))"
            grand_panic=$((grand_panic + 1))
            ;;
        timeout)
            eval "${pfx}_timeout=\$(( \${${pfx}_timeout} + 1 ))"
            grand_timeout=$((grand_timeout + 1))
            ;;
        *)
            eval "${pfx}_unknown=\$(( \${${pfx}_unknown} + 1 ))"
            grand_unknown=$((grand_unknown + 1))
            ;;
    esac
}

# ── per-test runner ───────────────────────────────────────────────────────────
# Usage: run_test <project_name> <project_prefix> <contract> <testfn> [path_hint]
#   path_hint: when non-empty, emits "path_hint::contract::testfn" to resolve
#              duplicate contract names (e.g. prb-math sd59x18/ud60x18).
run_test() {
    local project="$1" pfx="$2" contract="$3" testfn="$4" path_hint="${5:-}"
    local target
    if [[ -n "$path_hint" ]]; then
        target="${path_hint}::${contract}::${testfn}"
    else
        target="${contract}::${testfn}"
    fi
    printf "  %-70s " "$target" >&2

    local out rc
    local tmpstderr tmpstdout
    tmpstderr=$(mktemp)
    tmpstdout=$(mktemp)

    # macOS ships without GNU `timeout`; use a background-process approach.
    "$EDB" test "$target" --root "$DEST/$project" --no-ui >"$tmpstdout" 2>"$tmpstderr" &
    local bgpid=$!
    (
        sleep 60
        kill -TERM "$bgpid" 2>/dev/null
        sleep 2
        kill -KILL "$bgpid" 2>/dev/null
    ) &
    local killerpid=$!
    wait "$bgpid"
    rc=$?
    kill "$killerpid" 2>/dev/null
    wait "$killerpid" 2>/dev/null
    out=$(cat "$tmpstdout")
    rm -f "$tmpstdout"

    # SIGTERM → exit 143, SIGKILL → exit 137 on bash
    if [[ $rc -eq 143 ]] || [[ $rc -eq 137 ]]; then
        rc=124
    fi

    if [[ $rc -eq 124 ]]; then
        echo "TIMEOUT" >&2
        inc_counters "$pfx" "timeout"
        printf '{"project":"%s","target":"%s","status":"timeout"}\n' "$project" "$target"
        rm -f "$tmpstderr"
        return
    fi

    if [[ $rc -ne 0 ]]; then
        local errmsg
        errmsg=$(sed 's/\x1b\[[0-9;]*m//g' "$tmpstderr" | grep -v '^$' | tail -3 | tr '\n' '|')
        echo "PANIC(rc=$rc)" >&2
        inc_counters "$pfx" "engine-panic"
        errmsg="${errmsg//\"/\'}"
        printf '{"project":"%s","target":"%s","status":"engine-panic","rc":%d,"error":"%s"}\n' \
            "$project" "$target" "$rc" "$errmsg"
        rm -f "$tmpstderr"
        return
    fi
    rm -f "$tmpstderr"

    # Strip ANSI codes, find the JSON summary line.
    local summary
    summary=$(printf '%s\n' "$out" \
        | sed 's/\x1b\[[0-9;]*m//g' \
        | grep -E '^\{"' \
        | tail -1)

    local status
    status=$(printf '%s' "$summary" \
        | python3 -c "import json,sys; d=json.load(sys.stdin); print(d.get('status','unknown'))" \
        2>/dev/null || echo "unknown")

    echo "$status" >&2
    inc_counters "$pfx" "$status"

    if [[ -n "$summary" ]]; then
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

# ── extract test functions (no-arg only) from a .sol file ─────────────────────
# Returns lines of the form "ContractName::testFunctionName"
# Uses python3 because macOS awk does not support 3-arg match().
extract_tests() {
    local sol="$1"
    python3 - "$sol" <<'PYEOF'
import re, sys

sol_path = sys.argv[1]
current = ""
seen = set()
try:
    with open(sol_path) as f:
        for line in f:
            m = re.match(r'^contract ([A-Za-z0-9_]+)', line)
            if m:
                current = m.group(1)
            m = re.search(
                r'function ((?:test|testFuzz|testFork|invariant_)[A-Za-z0-9_]+)\s*\(\s*\)',
                line
            )
            if m and current:
                key = f"{current}::{m.group(1)}"
                if key not in seen:
                    seen.add(key)
                    print(key)
except Exception:
    pass
PYEOF
}

# ── derive path hint from a .sol file path ────────────────────────────────────
# Scans for known prb-math sub-type directory components in the file's path.
# Returns the first match: sd59x18, ud60x18, sd21x18, ud21x18, ud2x18, sd1x18.
# Prints nothing if no known component is found (no path hint needed).
derive_path_hint() {
    local sol="$1"
    python3 -c "
import sys, os
sol = sys.argv[1]
hints = ['sd59x18', 'ud60x18', 'sd21x18', 'ud21x18', 'ud2x18', 'sd1x18']
parts = sol.replace('\\\\', '/').split('/')
for h in hints:
    if h in parts:
        print(h)
        sys.exit(0)
" "$sol"
}

# ── project walker ────────────────────────────────────────────────────────────
# Usage: walk_project <name> <prefix> <test_dir> <recursive> [path_hint_mode]
#   recursive:      "yes" = find .t.sol recursively, "no" = flat only
#   path_hint_mode: "auto" = derive path hint from each file's directory
#                   components (used for prb-math to resolve duplicate
#                   contract names via the [[path::]contract::]testFn syntax).
#                   Absent or empty = no path hint (default behaviour).
walk_project() {
    local project="$1" pfx="$2" test_dir="$3" recursive="$4" path_hint_mode="${5:-}"

    echo "" >&2
    echo "## $project  (test dir: $test_dir, recursive=$recursive)" >&2
    local count=0
    local sample_idx=0

    # Collect .t.sol files
    local tmplist
    tmplist=$(mktemp)
    if [[ "$recursive" == "yes" ]]; then
        find "$test_dir" -name "*.t.sol" | sort > "$tmplist"
    else
        ls -1 "$test_dir"/*.t.sol 2>/dev/null | sort > "$tmplist"
    fi

    while IFS= read -r sol; do
        [[ -f "$sol" ]] || continue

        # Derive path hint for this file (empty when path_hint_mode != "auto")
        local file_path_hint=""
        if [[ "$path_hint_mode" == "auto" ]]; then
            file_path_hint=$(derive_path_hint "$sol")
        fi

        while IFS= read -r pair; do
            [[ -z "$pair" ]] && continue
            local contract testfn
            contract="${pair%%::*}"
            testfn="${pair##*::}"

            # Sampling
            if [[ "$SAMPLE" -gt 1 ]]; then
                if (( sample_idx % SAMPLE != 0 )); then
                    sample_idx=$((sample_idx + 1))
                    continue
                fi
            fi
            sample_idx=$((sample_idx + 1))

            # Hard limit
            if [[ "$LIMIT" -gt 0 && "$count" -ge "$LIMIT" ]]; then
                echo "  (limit $LIMIT reached, stopping)" >&2
                rm -f "$tmplist"
                return
            fi

            run_test "$project" "$pfx" "$contract" "$testfn" "$file_path_hint"
            count=$((count + 1))
        done < <(extract_tests "$sol")
    done < "$tmplist"
    rm -f "$tmplist"
}

# ── main ──────────────────────────────────────────────────────────────────────
echo "Coverage run started: $(date)" >&2
[[ "$LIMIT" -gt 0 ]] && echo "(limit: $LIMIT tests per project)" >&2
[[ "$SAMPLE" -gt 1 ]] && echo "(sampling: every ${SAMPLE}th test per project)" >&2

walk_project "forge-template"  "ft" "$DEST/forge-template/test"    "no"
walk_project "solady"          "sl" "$DEST/solady/test"             "no"
walk_project "uniswap-v4-core" "uv" "$DEST/uniswap-v4-core/test"   "no"
walk_project "solmate"         "sm" "$DEST/solmate/src/test"        "no"
# prb-math: "auto" path hints resolve duplicate contract names (sd59x18 / ud60x18)
# via the [[path::]contract::]testFn syntax introduced in 84fcf61.
walk_project "prb-math"        "pm" "$DEST/prb-math/test"           "yes"  "auto"

echo "" >&2
echo "=== Coverage summary by project ===" >&2
printf "%-20s %6s %6s %6s %6s %6s %6s %6s\n" \
    "Project" "Total" "OK" "Rejected" "Revert" "Panic" "Timeout" "Unknown" >&2
echo "────────────────────────────────────────────────────────────────────────────────────" >&2

print_proj_row() {
    local label="$1" t="$2" o="$3" r="$4" rv="$5" p="$6" to="$7" u="$8"
    local pct=0
    [[ "$t" -gt 0 ]] && pct=$(( o * 100 / t ))
    printf "%-20s %6d %6d %6d %6d %6d %6d %6d  (%d%%)\n" \
        "$label" "$t" "$o" "$r" "$rv" "$p" "$to" "$u" "$pct" >&2
}

print_proj_row "forge-template"  "$ft_total" "$ft_ok" "$ft_rejected" "$ft_revert" "$ft_panic" "$ft_timeout" "$ft_unknown"
print_proj_row "solady"          "$sl_total" "$sl_ok" "$sl_rejected" "$sl_revert" "$sl_panic" "$sl_timeout" "$sl_unknown"
print_proj_row "uniswap-v4-core" "$uv_total" "$uv_ok" "$uv_rejected" "$uv_revert" "$uv_panic" "$uv_timeout" "$uv_unknown"
print_proj_row "solmate"         "$sm_total" "$sm_ok" "$sm_rejected" "$sm_revert" "$sm_panic" "$sm_timeout" "$sm_unknown"
print_proj_row "prb-math"        "$pm_total" "$pm_ok" "$pm_rejected" "$pm_revert" "$pm_panic" "$pm_timeout" "$pm_unknown"
echo "────────────────────────────────────────────────────────────────────────────────────" >&2
print_proj_row "TOTAL"           "$grand_total" "$grand_ok" "$grand_rejected" "$grand_revert" "$grand_panic" "$grand_timeout" "$grand_unknown"

echo "" >&2
echo "Coverage run finished: $(date)" >&2

#!/usr/bin/env python3
# Static cheatcode-coverage estimator. For each test function in every
# vendored real-world Foundry project, determine the set of vm.* cheatcodes
# it (directly or transitively via setUp / base contracts in the same file)
# invokes, and compute whether EDB's catalog covers them all.
#
# This is a STATIC pre-filter — we don't actually run the tests. It answers:
# "how many tests are *eligible* for `edb test` based on cheatcode support?"
#
# Usage: ./scripts/edb-static-cheatcode-coverage.py [--json]
# Output: human-readable table by default; --json emits a machine-readable
# summary.

from __future__ import annotations
import argparse
import collections
import json
import os
import re
import sys
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parent.parent
E2E_DIR = REPO_ROOT / "testdata" / "foundry-e2e"
CHEATS_RS = REPO_ROOT / "crates" / "edb" / "src" / "cmd" / "test" / "cheats.rs"

# Projects to scan + their test root + whether to recurse + path-hint mode for
# duplicate contract names.
PROJECTS = [
    ("forge-template", "test", False),
    ("solady", "test", False),
    ("uniswap-v4-core", "test", True),
    ("solmate", "src/test", True),
    ("prb-math", "test", True),
]

# ---------------------------------------------------------------------------
# Parse EDB's KNOWN_CHEATCODES catalog. The catalog has three sections:
#   1. Implemented (supported) — listed first, before the "Not yet implemented"
#      or "Explicitly rejected" comment markers.
#   2. Rejected — under "--- Explicitly rejected in EDB v1 ---".
#   3. Not yet implemented — under "--- Not yet implemented ---".
# We need only the *implemented* names. The dispatch code is the ground truth
# (a selector in dispatch but not in KNOWN_CHEATCODES would still work) but the
# catalog name overlap is good enough for a static estimate.
# ---------------------------------------------------------------------------
def parse_supported_cheatcodes() -> set[str]:
    text = CHEATS_RS.read_text()
    # Find the KNOWN_CHEATCODES const body (between `&[` and the matching `];`).
    start = text.find("const KNOWN_CHEATCODES")
    assert start != -1, "couldn't find KNOWN_CHEATCODES in cheats.rs"
    body = text[start:]
    body = body[: body.find("];")]

    # The catalog is partitioned by `// --- <section-name> ---` comments. Walk
    # line-by-line and track whether the current section is supported. Section
    # names that mean "supported": anything not containing "rejected" /
    # "not yet implemented" / "Dynamic / array / decimal / approx".
    names = set()
    in_supported = True  # rows before the first `// ---` marker are supported.
    for line in body.splitlines():
        stripped = line.strip()
        # Section marker: `// --- something ---`.
        m_sec = re.match(r"//\s*---\s*(.*?)\s*---", stripped)
        if m_sec:
            label = m_sec.group(1).lower()
            in_supported = not any(
                bad in label
                for bad in (
                    "rejected",
                    "not yet implemented",
                    "dynamic / array / decimal",
                    "approx assertion",
                )
            )
            continue
        # Inline "Gas snapshot stubs" / similar comment: still in supported.
        # Match catalog rows: (&[0x.., 0x.., 0x.., 0x..], "name"),
        m_row = re.search(r'\],\s*"([a-zA-Z_][a-zA-Z0-9_]*)"\)', stripped)
        if m_row and in_supported:
            names.add(m_row.group(1))
    return names


# ---------------------------------------------------------------------------
# Walk one .sol file: list test functions + the cheatcodes each one invokes
# (including those invoked by any non-test function it can reach within the
# same file, e.g. setUp or internal helpers — we conservatively unionize all
# vm.* calls in the same contract).
# ---------------------------------------------------------------------------
TEST_FN_RE = re.compile(
    r"function\s+((?:test|testFuzz|testFork|invariant_)[A-Za-z0-9_]*)\s*\(\s*\)",
    re.MULTILINE,
)
# Legacy `testFail*` pattern: forge dropped support and so did EDB. Excluding
# matches what `edb test` will actually run.
TEST_FAIL_RE = re.compile(r"^testFail")
CHEATCODE_RE = re.compile(r"\bvm\.([a-zA-Z_][a-zA-Z0-9_]*)\s*\(")


def cheatcodes_in_file(path: Path) -> tuple[set[str], list[str]]:
    """Return (per_file_cheatcodes, test_function_names) for this file.

    Conservative: union of every `vm.<name>(` call in the entire file. This
    overestimates the cheatcode set per individual test (one test might not
    actually invoke every cheatcode the file uses), but it's the right
    behavior for `setUp` cheatcodes that every test inherits anyway, and the
    overestimate is a *safer* lower bound for "is this test eligible?".
    """
    try:
        src = path.read_text(errors="replace")
    except OSError:
        return set(), []
    cheats = set(CHEATCODE_RE.findall(src))
    tests = TEST_FN_RE.findall(src)
    # Drop `testFail*` legacy names — forge dropped them, so will edb test.
    tests = [t for t in tests if not TEST_FAIL_RE.match(t)]
    # De-dup tests (a file can technically define the same function name in
    # two contracts; both pass for our count).
    seen = []
    seen_set = set()
    for t in tests:
        if t not in seen_set:
            seen.append(t)
            seen_set.add(t)
    return cheats, seen


def walk_project(project_dir: Path, test_subdir: str, recurse: bool):
    """Yield (file_path, test_fn, cheatcodes_used) tuples."""
    root = project_dir / test_subdir
    if not root.is_dir():
        return
    pattern = "**/*.t.sol" if recurse else "*.t.sol"
    for sol in sorted(root.glob(pattern)):
        cheats, tests = cheatcodes_in_file(sol)
        for t in tests:
            yield (sol, t, cheats)


# ---------------------------------------------------------------------------
# Main: tally per-project + overall, list dominant unsupported cheatcodes.
# ---------------------------------------------------------------------------
def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--json", action="store_true", help="emit machine-readable JSON")
    args = ap.parse_args()

    supported = parse_supported_cheatcodes()

    proj_stats = {}  # project -> {total, eligible, ineligible}
    blocking = collections.Counter()  # unsupported cheatcode -> # blocked tests

    for name, sub, recurse in PROJECTS:
        pdir = E2E_DIR / name
        if not pdir.is_dir():
            proj_stats[name] = {"total": 0, "eligible": 0, "missing": True}
            continue
        total = 0
        eligible = 0
        for (_, fn, cheats) in walk_project(pdir, sub, recurse):
            total += 1
            unsupported = cheats - supported
            if not unsupported:
                eligible += 1
            else:
                for c in unsupported:
                    blocking[c] += 1
        proj_stats[name] = {"total": total, "eligible": eligible, "missing": False}

    overall_total = sum(p["total"] for p in proj_stats.values())
    overall_eligible = sum(p["eligible"] for p in proj_stats.values())
    pct = (100 * overall_eligible / overall_total) if overall_total else 0.0

    if args.json:
        out = {
            "supported_count": len(supported),
            "per_project": {
                name: {
                    "total": p["total"],
                    "eligible": p["eligible"],
                    "pct": (100 * p["eligible"] / p["total"]) if p["total"] else 0.0,
                }
                for name, p in proj_stats.items()
            },
            "overall": {"total": overall_total, "eligible": overall_eligible, "pct": pct},
            "blocking_cheatcodes": dict(blocking.most_common()),
        }
        json.dump(out, sys.stdout, indent=2)
        sys.stdout.write("\n")
        return 0

    print(f"EDB supports {len(supported)} cheatcode names (per KNOWN_CHEATCODES, supported section).")
    print()
    print(f"{'project':18s} {'total':>7s} {'eligible':>10s} {'pct':>6s}")
    print("-" * 50)
    for name, p in proj_stats.items():
        if p.get("missing"):
            print(f"{name:18s} (project directory missing — run scripts/fetch-e2e-foundry-projects.sh)")
            continue
        t, e = p["total"], p["eligible"]
        per_proj_pct = (100 * e / t) if t else 0.0
        print(f"{name:18s} {t:>7d} {e:>10d} {per_proj_pct:>5.0f}%")
    print("-" * 50)
    print(f"{'overall':18s} {overall_total:>7d} {overall_eligible:>10d} {pct:>5.0f}%")
    print()
    if blocking:
        print(f"Top blocking unsupported cheatcodes (# tests they block):")
        for c, n in blocking.most_common(15):
            print(f"  vm.{c:30s} {n:>5d}")

    return 0


if __name__ == "__main__":
    sys.exit(main())

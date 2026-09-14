#!/usr/bin/env python3
"""Compare corpus identities and statuses without rerunning either compiler.

Accept the checked-in three-column baseline or a full six-column ledger.
Exit 1 on a lost pass or newly skipped test, 2 on malformed/incomparable
ledgers. A valid comparison always emits structured JSON on stdout, including
when it exits 1. Exit 0 is only a status check, not proof of correct
diagnostics.
"""

import argparse
from collections import Counter
import json
from pathlib import Path
import sys


KINDS = frozenset(("pos", "neg", "run"))
STATUSES = frozenset(("pass", "fail", "skip"))


def read_ledger(path):
    records = {}
    width = None
    # Diagnostic records contain ASCII RS (0x1e), which str.splitlines treats
    # as a newline. Only LF separates corpus rows.
    for line_number, line in enumerate(Path(path).read_bytes().split(b"\n"), 1):
        if not line or line.startswith(b"#"):
            continue
        fields = line.split(b"\t")
        if len(fields) not in (3, 6) or (width is not None and len(fields) != width):
            raise ValueError(f"{path}:{line_number}: expected consistently 3 or 6 fields")
        width = len(fields)
        kind, name, status = (field.decode("utf-8") for field in fields[:3])
        if kind not in ("pos", "neg", "run") or not name or status not in ("pass", "fail", "skip"):
            raise ValueError(f"{path}:{line_number}: invalid identity or status")
        key = (kind, name)
        if key in records:
            raise ValueError(f"{path}:{line_number}: duplicate identity {key}")
        records[key] = status
    if not records:
        raise ValueError(f"{path}: empty ledger")
    return records


def compare(baseline, candidate):
    missing = sorted(baseline.keys() - candidate.keys())
    added = sorted(candidate.keys() - baseline.keys())
    if missing or added:
        raise ValueError(f"different test identities: missing={missing}, added={added}")
    changes = []
    for (kind, name), before in sorted(baseline.items()):
        after = candidate[(kind, name)]
        if before != after:
            changes.append({"kind": kind, "test": name, "before": before, "after": after,
                            "loss": before == "pass" or after == "skip"})
    counts = lambda ledger: {
        kind: dict(Counter(status for (k, _), status in ledger.items() if k == kind))
        for kind in ("pos", "neg", "run")
    }
    return {"rows": len(candidate), "baseline": counts(baseline),
            "candidate": counts(candidate), "changes": changes,
            "losses": sum(change["loss"] for change in changes)}


def validate_result(result):
    """Validate the JSON object emitted by :func:`compare`.

    The comparator deliberately returns exit status 1 for a valid comparison
    that contains losses.  Callers must therefore validate the structured
    output independently of the process status; treating every non-zero
    status as malformed loses the useful loss/change counts.
    """

    if not isinstance(result, dict):
        raise ValueError("comparison result must be an object")
    fields = {"rows", "baseline", "candidate", "changes", "losses"}
    if set(result) != fields:
        raise ValueError("comparison result fields mismatch")

    rows = result["rows"]
    if isinstance(rows, bool) or not isinstance(rows, int) or rows <= 0:
        raise ValueError("comparison result rows must be a positive integer")
    for side in ("baseline", "candidate"):
        counts = result[side]
        if not isinstance(counts, dict) or set(counts) != KINDS:
            raise ValueError(f"comparison result {side} counts are malformed")
        total = 0
        for kind in KINDS:
            per_kind = counts[kind]
            if not isinstance(per_kind, dict):
                raise ValueError(f"comparison result {side}/{kind} counts are malformed")
            for status, count in per_kind.items():
                if status not in STATUSES or isinstance(count, bool) or not isinstance(count, int) or count < 0:
                    raise ValueError(f"comparison result {side}/{kind} counts are malformed")
                total += count
        if total != rows:
            raise ValueError(f"comparison result {side} counts do not sum to rows")

    changes = result["changes"]
    if not isinstance(changes, list):
        raise ValueError("comparison result changes must be a list")
    losses = result["losses"]
    if isinstance(losses, bool) or not isinstance(losses, int) or losses < 0:
        raise ValueError("comparison result losses must be a non-negative integer")
    loss_count = 0
    identities = set()
    for change in changes:
        if not isinstance(change, dict) or set(change) != {"kind", "test", "before", "after", "loss"}:
            raise ValueError("comparison result change is malformed")
        if change["kind"] not in KINDS or not isinstance(change["test"], str) or not change["test"]:
            raise ValueError("comparison result change identity is malformed")
        identity = (change["kind"], change["test"])
        if identity in identities:
            raise ValueError("comparison result contains duplicate change identity")
        identities.add(identity)
        if change["before"] not in STATUSES or change["after"] not in STATUSES:
            raise ValueError("comparison result change status is malformed")
        if change["before"] == change["after"]:
            raise ValueError("comparison result change has no status change")
        if not isinstance(change["loss"], bool):
            raise ValueError("comparison result change loss is malformed")
        if change["loss"] != (change["before"] == "pass" or change["after"] == "skip"):
            raise ValueError("comparison result change loss is inconsistent")
        loss_count += change["loss"]
    if losses != loss_count:
        raise ValueError("comparison result losses do not match changes")
    return result


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("baseline", type=Path)
    parser.add_argument("candidate", type=Path)
    args = parser.parse_args()
    try:
        result = compare(read_ledger(args.baseline), read_ledger(args.candidate))
        validate_result(result)
    except (OSError, TypeError, ValueError) as error:
        print(str(error), file=sys.stderr)
        return 2
    print(json.dumps(result, indent=2, ensure_ascii=True))
    return int(result["losses"] != 0)


if __name__ == "__main__":
    sys.exit(main())

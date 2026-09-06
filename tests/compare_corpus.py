#!/usr/bin/env python3
"""Compare corpus identities and statuses without rerunning either compiler.

Accept the checked-in three-column baseline or a full six-column ledger.
Exit 1 on a lost pass or newly skipped test, 2 on malformed/incomparable
ledgers. Exit 0 is only a status check, not proof of correct diagnostics.
"""

import argparse
from collections import Counter
import json
from pathlib import Path
import sys


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


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("baseline", type=Path)
    parser.add_argument("candidate", type=Path)
    args = parser.parse_args()
    try:
        result = compare(read_ledger(args.baseline), read_ledger(args.candidate))
    except (OSError, ValueError) as error:
        print(str(error), file=sys.stderr)
        return 2
    print(json.dumps(result, indent=2, ensure_ascii=True))
    return int(result["losses"] != 0)


if __name__ == "__main__":
    sys.exit(main())

#!/usr/bin/env python3
"""Read and write the small, fixed-schema records used by verify_merge.sh.

The gate consumes output from several independent shell processes.  Human
readable summaries are useful in the log, but are not a safe protocol: a
missing line, an extra field, or a command whose status was lost in a pipeline
can all look like success.  This module is deliberately boring and strict so
that a malformed record fails closed.
"""

from __future__ import annotations

import argparse
import base64
import json
import os
import re
import sys
import tempfile
from pathlib import Path
from typing import Any


SCHEMA = 1
FIELDS = ("schema", "step", "status", "exit_code", "summary")
STATUSES = frozenset(("pass", "fail", "skip", "error"))
STEP_RE = re.compile(r"^[A-Za-z0-9_]+$")


class ResultError(ValueError):
    """A record does not conform to the gate result schema."""


def validate(record: Any, expected_step: str | None = None) -> dict[str, Any]:
    if not isinstance(record, dict):
        raise ResultError("result must be an object")
    if set(record) != set(FIELDS):
        missing = sorted(set(FIELDS) - set(record))
        extra = sorted(set(record) - set(FIELDS))
        detail = []
        if missing:
            detail.append("missing=" + ",".join(missing))
        if extra:
            detail.append("extra=" + ",".join(extra))
        raise ResultError("result fields mismatch (" + " ".join(detail) + ")")
    if record["schema"] != SCHEMA or isinstance(record["schema"], bool):
        raise ResultError(f"unsupported schema: {record['schema']!r}")
    step = record["step"]
    if not isinstance(step, str) or not STEP_RE.fullmatch(step):
        raise ResultError("step must contain only letters, digits, and underscores")
    if expected_step is not None and step != expected_step:
        raise ResultError(f"unexpected step: {step!r} (expected {expected_step!r})")
    status = record["status"]
    if not isinstance(status, str) or status not in STATUSES:
        raise ResultError(f"invalid status: {status!r}")
    code = record["exit_code"]
    if isinstance(code, bool) or not isinstance(code, int) or not 0 <= code <= 255:
        raise ResultError("exit_code must be an integer from 0 through 255")
    summary = record["summary"]
    if not isinstance(summary, str) or "\n" in summary or "\r" in summary:
        raise ResultError("summary must be a single line of text")
    return dict(record)


def read(path: str | os.PathLike[str], expected_step: str | None = None) -> dict[str, Any]:
    try:
        with open(path, "r", encoding="utf-8") as stream:
            record = json.load(stream)
    except (OSError, UnicodeError, json.JSONDecodeError) as exc:
        raise ResultError(f"cannot read result {path}: {exc}") from exc
    return validate(record, expected_step)


def write(
    path: str | os.PathLike[str],
    step: str,
    status: str,
    exit_code: int | str,
    summary: str,
) -> dict[str, Any]:
    try:
        code = int(exit_code)
    except (TypeError, ValueError) as exc:
        raise ResultError("exit_code must be an integer from 0 through 255") from exc
    record = validate(
        {
            "schema": SCHEMA,
            "step": step,
            "status": status,
            "exit_code": code,
            "summary": summary,
        }
    )
    destination = Path(path)
    destination.parent.mkdir(parents=True, exist_ok=True)
    fd, temporary = tempfile.mkstemp(
        prefix=f".{destination.name}.", suffix=".tmp", dir=destination.parent
    )
    try:
        with os.fdopen(fd, "w", encoding="utf-8") as stream:
            json.dump(record, stream, ensure_ascii=False, sort_keys=True, separators=(",", ":"))
            stream.write("\n")
            stream.flush()
            os.fsync(stream.fileno())
        os.replace(temporary, destination)
    except BaseException:
        try:
            os.unlink(temporary)
        except OSError:
            pass
        raise
    return record


def read_row(path: str, expected_step: str | None = None) -> str:
    record = read(path, expected_step)
    encoded = base64.b64encode(record["summary"].encode("utf-8")).decode("ascii") or "-"
    return "\t".join(
        (record["step"], record["status"], str(record["exit_code"]), encoded)
    )


def main(argv: list[str]) -> int:
    parser = argparse.ArgumentParser()
    sub = parser.add_subparsers(dest="command", required=True)

    write_parser = sub.add_parser("write")
    write_parser.add_argument("path")
    write_parser.add_argument("step")
    write_parser.add_argument("status")
    write_parser.add_argument("exit_code")
    write_parser.add_argument("summary")

    read_parser = sub.add_parser("read")
    read_parser.add_argument("path")
    read_parser.add_argument("expected_step", nargs="?")

    args = parser.parse_args(argv)
    try:
        if args.command == "write":
            record = write(args.path, args.step, args.status, args.exit_code, args.summary)
            print(json.dumps(record, ensure_ascii=False, sort_keys=True, separators=(",", ":")))
        else:
            print(read_row(args.path, args.expected_step))
    except ResultError as exc:
        print(f"gate result: {exc}", file=sys.stderr)
        return 2
    return 0


if __name__ == "__main__":
    raise SystemExit(main(sys.argv[1:]))

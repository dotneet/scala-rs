#!/usr/bin/env python3
"""Guard the release profile used by compiler performance benchmarks."""

import tomllib
from pathlib import Path


ROOT = Path(__file__).resolve().parents[3]
profile = tomllib.loads((ROOT / "Cargo.toml").read_text())["profile"]["release"]
assert profile["lto"] == "thin", profile
assert profile["codegen-units"] == 1, profile

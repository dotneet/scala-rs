# Flat macro Block chain

This dependency-free case makes each macro expansion append one local
definition to a flat `Block`. The next call passes the entire previous Block
as its receiver. The provider is compiled outside the timed region; both
compilers compile the same generated 300-call client in resident processes.

From the repository root:

```sh
python3 tests/perf/macro-block-chain/check.py \
  target/release/scala-rs /tmp/scala-2.13.16 /path/to/jdk
```

The interim ratio gate detects regression in the compact Block-child and
shared-prefix transport. It does not claim parity with scalac on this case;
continue reducing the gap before tightening it to 1.0.

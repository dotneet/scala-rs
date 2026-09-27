# Witnessless extension candidates

This fixture uses only the Scala 2.13.16 standard library. A binary provider
exposes one usable extension conversion and 1,000 conversions whose distinct
implicit prerequisites have no instances. The client compiles one `.result`
selection through an inherited API. The provider is compiled once with
scalac; only the client compilations are timed. Both compilers remain resident,
run in both orders, and the program output is checked.

The regression was warming the result hierarchy of every explicit-parameter
conversion for each failed prerequisite search. Such conversions cannot be
implicit values. Before filtering them, scala-rs took about 4.0 times as long
as scalac; after filtering, it took about 0.8 times as long. The gate requires
scala-rs to be faster.

```sh
python3 tests/perf/view-witness-warming/check.py \
  target/release/scala-rs /tmp/scala-2.13.16 /path/to/jdk
```

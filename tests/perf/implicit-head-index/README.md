# Implicit candidate head indexing

This fixture uses only Scala 2.13.16's standard library. It compiles a
provider of one recursive type-class rule and 1,000 unrelated rules before
timing a client that requests a 12-level nested witness. Each unrelated rule
has a distinct, uninhabited prerequisite. The client has no third-party
dependencies, and its result is checked after every compiler comparison.

The regression is repeated scanning of all lexical candidates for each
uninhabited prerequisite. The provider is compiled once with scalac; only
the client compilations are timed. Both compilers remain resident and run in
both orders. Before candidate-head indexing, scala-rs took about 1.7 times
as long as scalac on this fixture. The gate requires scala-rs to be faster.

```sh
python3 tests/perf/implicit-head-index/check.py \
  target/release/scala-rs /tmp/scala-2.13.16 /path/to/jdk
```

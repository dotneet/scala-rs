# Large signature import regression

The check generates one Scala class with many generic implicit members and a
separate source that imports those members. It uses only the Scala standard
library. The provider is compiled once before timing, so the measured portion
is classpath loading and compilation of the importing source.

From the repository root, after building the release compiler:

```sh
python3 tests/perf/large-signature/check.py \
  target/release/scala-rs /path/to/scala-2.13.16 /path/to/jdk
```

Both compilers are resident and warmed before the timed repetitions. The
check runs in both orders and fails when the native/scalac ratio reaches 2.0.
The original linear lookup exceeds this limit; the indexed lookup stays below
it. This is a performance regression gate, not a claim that this isolated
workload or every larger build is faster than scalac.

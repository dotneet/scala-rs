# Early rejection of witnessless views

This standard-library-only fixture compiles a binary provider with 300
extension conversions. Each has an absent first implicit prerequisite and a
second prerequisite whose companion contains 300 derivation rules. The result
type retains an undetermined type parameter, so extension lookup considers
the candidate before its witnesses are resolved.

The regression was warming every candidate's entire witness chain before
checking the first prerequisite. The provider is compiled once with scalac;
only the client is timed. Both compilers remain resident, run in both orders,
and the program output is checked. Before the early rejection, scala-rs took
about 5.4 times as long as scalac. The gate requires scala-rs to be faster.

```sh
python3 tests/perf/view-witness-order/check.py \
  target/release/scala-rs /tmp/scala-2.13.16 /path/to/jdk
```

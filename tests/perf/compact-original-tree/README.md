# Compact original macro trees

This dependency-free fixture returns a growing receiver tree through a macro.
The first occurrence of an original typed tree should travel back by its splice
index alone. A second occurrence must still reconstruct an independent tree.
The four independent chains keep each generated method below the JVM code-size
limit while exposing repeated serialization across macro expansions.

Run `python3 tests/perf/compact-original-tree/check.py target/release/scala-rs
/tmp/scala-2.13.16 /path/to/jdk`. The script warms both resident compilers,
checks program output, and compares the combined compile times in both orders.
Before compact original-tree replies, the native/scalac ratio was 2.86-2.90;
the default gate is 2.0 to allow modest machine noise.

# Inherited override-check regression

This fixture uses only the Scala standard library. It generates one trait
with 3,000 abstract methods and 50 empty abstract subclasses. A subclass with
one parent and no declared members cannot introduce an override conflict, so
its body pass should not revisit every inherited method.

The check compiles the generated source 20 times per compiler in each run
order. It compares the median of the last 10 iterations, after the resident
Scala compiler has warmed up, and checks the program output. The unoptimized
override scan is slower than scalac; the guarded scan must be faster.

From the repository root:

```sh
python3 tests/perf/inherited-headers/check.py \
  target/release/scala-rs /path/to/scala-2.13.16 /path/to/jdk
```

# Large unused macro arguments

`check.py` generates independent calls whose typed arguments each contain many
local values. The macro returns a constant without inspecting its argument.
The example uses only Scala 2.13.16's standard library and reflection API.

```sh
python3 tests/perf/lazy-macro-arguments/check.py \
  target/release/scala-rs /tmp/scala-2.13.16 /path/to/jdk
```

Both compilers run resident after a warm-up, using the same generated source,
provider, classpath, and JDK. On JDK 21.0.2, eager rebuilding of every
argument and application tree took about 2.2 times as long as scalac. The
default 1.6 ratio limit detects that regression without demanding an exact
timing on a loaded machine.

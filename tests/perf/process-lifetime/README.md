# Compiler process-lifetime reproduction

`Tiny.scala` is one line and uses only the Scala standard library. The driver
compiles identical copies into fresh output directories. The native compiler
starts a process per compilation; the reference creates a new compiler instance
per compilation inside one resident JVM. The first reference compilation
includes compiler warm-up; JVM launch is outside the measured interval.

From the repository root, run the two orders and a fresh-process scalac control:

```sh
python3 tests/perf/process-lifetime/check.py \
  /path/to/scala-rs /path/to/scala-2.13.16 /path/to/jdk \
  --maximum-ratio 1
```

The check compiles and runs both outputs, reports the native/resident ratio in
each order, and prints the median of three fresh-process scalac compilations.
Use `--maximum-ratio 1` to require native to be faster in both orders, or
`--minimum-ratio` to reproduce an older slow baseline.

On JDK 21.0.2 with scala-rs base commit `520556c5`, the original four-line
fixture took 9.31/4.18 and 9.42/4.16 seconds for 40 compilations
(native/resident scalac). With the current one-line fixture, after deferring
binary-parent indexing, avoiding impossible Scala/JDK class lookups, and
linking the Product hierarchy without eagerly completing its members, two
validation runs took 2.116/2.286 and 1.913/2.255 seconds, then 1.924/2.251
and 1.889/2.220 seconds. The largest observed native/resident ratio was 0.93;
fresh-process scalac took about 0.93 seconds per compilation. These numbers
vary with machine load. The comparison deliberately uses a fresh native
process against a resident JVM with a new compiler instance per compilation;
the process models are not identical. The compiler and reflect jars used by
the Java driver are part of Scala 2.13.16; the source has no third-party
dependencies. This result applies to the minimal fixture, not to implicit,
macro, or multi-module workloads.

## One macro expansion

`Marker.scala` defines a blackbox macro and `MacroUse.scala` calls it once.
They use only the Scala 2.13 distribution's library and reflect jars, with no
third-party dependency. The provider is compiled once, outside the timed
interval. The comparison keeps the same process model as above and checks the
output of both compilers:

```sh
python3 tests/perf/process-lifetime/macro_check.py \
  /path/to/scala-rs /path/to/scala-2.13.16 /path/to/jdk \
  --maximum-ratio 1
```

On JDK 21.0.2 at commit `7b061e12`, 20 fresh native compilations took about
10.6 seconds against 1.9-2.0 seconds for 20 new scalac instances in a
resident JVM. With `SCALA_RS_MACRO_TIMING=1`, the one-call native compilation
spent about 0.43 seconds starting its JVM macro engine and about 0.024
seconds exchanging the expansion. This fixture isolates the process-lifetime
cost that the non-macro `Tiny.scala` does not exercise. The regression gate
`--maximum-ratio 1` intentionally fails until that cost is eliminated.

To compare steady-state resident compilers, add `--resident-native`. It
requires a working local macro daemon rather than silently falling back to a
fresh JVM, and compiles one untimed warm-up source with each compiler in its
own resident process before the measured repetitions. Report this separately
from the fresh-native result because the process lifetimes differ.

Add `--resident-compiler` to keep the native compiler process alive as well.
It uses `__compile_batch` for sequential, isolated compiler runs and reuses
archive indexes only while their file identity, size, modification time, and
change time remain unchanged. Both compilers then exclude process startup
from the measured repetitions. This is a third comparison mode, not a
replacement for the fresh-process or macro-daemon-only measurements.
On Unix, batch compilation reuses a local macro daemon by default when one
can start. Set `SCALA_RS_MACRO_DAEMON=0` to disable it; if it cannot start,
compilation falls back to the ordinary macro engine.
On JDK 17 and later, the native batch process connects to the macro daemon
through a private Unix-domain socket. Set `SCALA_RS_MACRO_FORCE_TCP=1` for a
same-binary TCP control; runtimes without Unix-domain socket support use TCP.

The one-expansion fixture also isolates the resident compiler's default macro
engine policy without third-party libraries:

```sh
python3 tests/perf/process-lifetime/macro_check.py \
  /path/to/scala-rs /path/to/scala-2.13.16 /path/to/jdk \
  --resident-compiler --daemon-policy default --repeats 12 --maximum-ratio 1
```

The provider is compiled outside the timed interval; the measured source is
the same for both compilers. On JDK 21.0.2, 12 compilations took 0.23 seconds
with the default daemon policy versus 0.77-0.79 seconds for resident scalac.
Setting `--daemon-policy disabled` took 5.77-5.80 seconds for native on the
same source and run order. The check requires the default daemon to be
available, so it does not silently accept a fallback measurement.


## Many expansions in one small source

`Fanout.scala` emits 256 calls to a second macro from one call site, and
`FanoutUse.scala` prints the result. The second macro asks for one standard
library implicit. Both files are dependency-free apart from the Scala
distribution's library and reflect jars. Run the same process comparison with
the provider compiled outside the timed interval:

```sh
python3 tests/perf/process-lifetime/macro_check.py \
  /path/to/scala-rs /path/to/scala-2.13.16 /path/to/jdk \
  --provider tests/perf/process-lifetime/Fanout.scala \
  --source tests/perf/process-lifetime/FanoutUse.scala \
  --main-class FanoutUse --expected-output 1 --repeats 20
```

On JDK 21.0.2 at commit `7b061e12` with local compiler changes, 20 fresh
native compilations took 12.54/12.50 seconds versus 2.67/2.67 seconds for
resident scalac, in opposite orders. One native compilation performed 257
macro expansions and 256 distinct implicit searches. Two repetitions are too
few for this comparison: the reference JVM's first compiler instance includes
warm-up, while the native side pays its macro JVM startup on every run.

With `--resident-compiler --repeats 40` on JDK 21.0.2, Unix-domain transport
completed the native runs in 2.226 and 1.780 seconds, against 2.363 and 2.362
seconds for resident scalac in opposite orders. The same native binary forced
to TCP took 2.479 and 2.229 seconds. Both compiler runs use one untimed warm-up
compilation; these figures are specific to this small source and machine load.

## Repeated independent macro implicit searches

`ReflectFanout.scala` emits 384 calls to a macro that requests three standard
library implicits and typechecks one expression. The caller and provider use
only the Scala distribution's library and reflect jars. This reproduces a
cache miss in independent macro implicit queries without external dependencies:

```sh
python3 tests/perf/process-lifetime/macro_check.py \
  /path/to/scala-rs /path/to/scala-2.13.16 /path/to/jdk \
  --provider tests/perf/process-lifetime/ReflectFanout.scala \
  --source tests/perf/process-lifetime/ReflectFanoutUse.scala \
  --main-class ReflectFanoutUse --expected-output 1 \
  --resident-compiler --repeats 80 --maximum-ratio 1
```

On JDK 21.0.2, before caching independent depth-one macro searches, the
native/resident-scalac totals were 6.773/5.647 and 6.492/5.518 seconds in
opposite orders. With those searches eligible for the existing scope-fingerprint
cache, they were 5.383/5.595 and 5.160/5.642 seconds. The check compiles both
outputs and runs the resulting program. Timings vary with machine load; use
both orders and keep the compiler lifetimes identical when comparing changes.

## Reused macro engine correctness

`MirrorProvider.scala`, `MirrorA.scala`, and `MirrorB.scala` reproduce a
separate-compilation classpath transition without third-party libraries. The
engine first sees `MirrorA` as source, then as a binary dependency of
`MirrorB`, then as source again. The check compares both `MirrorA` class-file
sets and runs the resulting program for both compilers:

```sh
python3 tests/perf/process-lifetime/mirror_reload_check.py \
  /path/to/scala-rs /path/to/scala-2.13.16 /path/to/jdk
```

The native check enables the optional macro daemon. Without replacing a
binary package-scope symbol when the same name becomes source again, the
second native `MirrorA` compilation fails at `PackageScope.enter`. This is a
correctness gate for daemon reuse, not a speed measurement.

`ReloadProviderOne.scala` and `ReloadProviderTwo.scala` define the same macro
class names but return different values. The check compiles the caller with
provider A, B, and A again, first from distinct classpath directories and
then by replacing class files in the same directory:

```sh
python3 tests/perf/process-lifetime/provider_reload_check.py \
  /path/to/scala-rs /path/to/scala-2.13.16 /path/to/jdk
```

It verifies the program output and first/last output class hashes for scalac
and the required native macro daemon. A daemon that keeps a previously loaded
provider class incorrectly prints A's value during the B compilation. This
check must pass before resident macro performance results are meaningful.

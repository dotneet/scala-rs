# Using scala-rs

This guide is for people who compile code with scala-rs: what it needs, how a
build tool should drive it, how macros are run, and what to check when a build
is slower than expected. The language boundary is in
[language support](language-support.md) and [known gaps](not-implemented.md);
the details of macro expansion are in [macros](macros.md).

## What a compilation needs

- **scala-library 2.13.** `--scala-library <jar>` links the output against the
  real standard library. Without a path, scala-rs looks for a 2.13 jar in
  `SCALA_LIBRARY_JAR`, `/tmp/scala-rs-lib` and the working directory, and
  `compile` and `run` use one they find. `--no-scala-library` emits the
  project's private runtime classes instead; such output does not interoperate
  with code compiled by scalac.
- **The dependency classpath**, as `-cp` (or `--class-path`): jars and class
  directories separated by `:`, in the order scalac would get them.
- **A JDK, for macros only.** Expanding a Scala 2 def macro runs its
  implementation on a JVM. scala-rs uses `$JAVA_HOME/bin/java` (and `javac`
  for its own engine class) when `JAVA_HOME` is set, and `java` on `PATH`
  otherwise. Set `JAVA_HOME` to the JDK the project builds with: a macro
  implementation compiled for a newer class-file version cannot load on an
  older JVM.
- **scala-reflect on `-cp`** when the code uses macros, as for scalac.

Options mirror scalac's where they exist: `-d`, `-cp`, `-deprecation`,
`-feature`, `-nowarn`, `-Xfatal-warnings`, `-language:<feature>`,
`-Xsource:3`, `-Xsource-features:<features>` and `-Xasync`.
`scala-rs --help` lists all of them. `--diagnostics=scalac` prints errors and
warnings exactly in scalac's console format, which is what tools that parse
compiler output expect.

## One compilation, or a whole build

`scala-rs compile` is a process per compilation. That is right for a single
project. A build of many modules should instead keep one compiler process and
send it every module in dependency order: the process keeps validated archive
indexes, class files read from dependency directories and decoded library
signatures between modules, and only the first module pays for reading them.

The resident compiler is `scala-rs __compile_batch`. Its protocol is small:

- A request is the arguments of `scala-rs compile` (without the word
  `compile`), each followed by a NUL byte, and then one more NUL byte.
- For each request the process writes one status byte to stdout once the
  module's class files are complete: `0` for success, `1` for failure.
  Diagnostics go to stderr.
- Closing stdin ends the process.
- `__compile_batch --no-archive-cache` keeps no state between requests.

```python
import subprocess

compiler = subprocess.Popen(["scala-rs", "__compile_batch"],
                            stdin=subprocess.PIPE, stdout=subprocess.PIPE)

def compile_module(args):
    compiler.stdin.write(("\0".join(args) + "\0\0").encode())
    compiler.stdin.flush()
    return compiler.stdout.read(1) == b"\0"

compile_module(["-cp", classpath, "-d", "out/core", "-nowarn",
                "--scala-library", library_jar, "--diagnostics=scalac", *sources])
```

The name starts with two underscores because the protocol is meant for build
integrations and benchmarks rather than for typing at a shell. Requests run
one at a time; source files are always read afresh.

Everything kept between requests is checked before it is used. A jar is
reused while its size, inode, modification and change times are unchanged; a
class file or a directory listing of a dependency directory likewise. A
module's output directory can therefore be rebuilt in place between requests,
and the next module sees the new classes. The kept class bytes are bounded
(768 MB) and dropped as a whole when full.

## Macros

### The engine

Macro implementations run in a JVM, the *macro engine*. scala-rs compiles the
engine class once with `javac` into `$TMPDIR/scala-rs-macro-engine-<hash>`,
starts the JVM in the background as soon as scala-reflect is seen on the
classpath, and talks to it over a pipe or a socket. The first expansion of a
run waits only for whatever is left of that start-up.

### The shared daemon

A JVM per compilation costs its start-up (around 0.5--0.8 s) plus the time its
first expansions run interpreted, on every module that uses a macro (logging
and JSON libraries make that most modules). By default a compilation therefore
uses a **macro daemon**: one engine JVM per user, runtime jars, working
directory and `java`, which every compiler on the machine shares.

- The daemon serves one compilation at a time. A compiler that finds it busy
  starts an engine of its own instead of waiting, so parallel builds are not
  serialised.
- It keeps the class loaders of recent runs, and their runtime mirrors. A
  later run reuses one while every class it already loaded still resolves to
  the same, unchanged class file on the new classpath; a changed or shadowed
  class gets a new loader. This is what nsc's
  `-Ycache-macro-class-loader:last-modified` does, and it has the same
  consequence: state that a macro implementation keeps in a static field can
  survive into the next run.
- It exits after three idle minutes. A session that timed out or broke the
  protocol shuts it down, and a watchdog ends a daemon whose macro has run
  for ten minutes without a word to the compiler. The next compiler starts a
  new one.
- It needs a private directory under the engine cache, a socket directory
  under `/tmp`, and a loopback or Unix-domain socket.

### When the daemon cannot run

A sandbox without local sockets, a read-only `/tmp`, or a daemon busy with
another build makes a compiler fall back to an engine of its own:

- `scala-rs compile` then starts a JVM for that compilation. In a build of
  many small modules that is the 0.5--0.8 s per module above.
- `scala-rs __compile_batch` starts one engine JVM over pipes and keeps it for
  all its later requests, with the same loader reuse as the daemon. After the
  daemon has failed once, the process stops asking for it. The engine exits
  with the batch process.

So a build that cannot use the daemon should use `__compile_batch`. A build
that is much slower than expected, by roughly a constant per module, is
usually paying a JVM start per module; `SCALA_RS_MACRO_TIMING=1` shows
`engine start-up` per module.

### Settings

| Variable | Effect |
|---|---|
| `SCALA_RS_MACRO_DAEMON=0` | Never use the shared daemon. |
| `SCALA_RS_MACRO_DAEMON_REQUIRE=1` | Fail instead of falling back when the daemon cannot be used. |
| `SCALA_RS_MACRO_PRESTART=0` | Start the engine at the first expansion rather than in the background. |
| `SCALA_RS_MACRO_TIMEOUT_SECS=<n>` | The time one expansion's implementation may run (default 20; `0` for none, when debugging an implementation). |
| `SCALA_RS_MACRO_FORCE_TCP=1` | Reach the daemon over TCP even where a Unix-domain socket exists. |
| `SCALA_RS_MACRO_TIMING=1` | Print, per module, the engine start-up and each expansion's time by stage. |
| `SCALA_RS_MACRO_IMPLICIT_STATS=1` | Count repeated `c.inferImplicitValue` searches. |

## Measuring and comparing

- Compare resident with resident: `__compile_batch` against one long-lived
  scalac JVM that creates a fresh `Global` per module. A cold `scala-rs
  compile` per module against a warm scalac, or the reverse, measures process
  and JVM start-up rather than the compilers.
- Discard the first run of a fresh daemon or batch process. Its macro engine
  is still interpreting the implementations.
- Check that the macro daemon (or the batch engine) was used: a constant
  0.5 s or more on every small module means it was not.
- Use the same flags for both compilers (`-nowarn` changes how much work
  either does), the same JDK, and fresh output directories each round.
- `SCALA_RS_PHASE_TIMING=1`, `SCALA_RS_TYPER_PHASE_TIMING=1` and
  `SCALA_RS_UNIT_TIMING=1` print where one compilation's time goes, by
  compiler phase, by typer phase and by source file.

[Performance](performance.md) records the project's own measurements and how
they were taken.

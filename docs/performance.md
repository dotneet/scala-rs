## Speed

### What is measured

The standard workload is a **full compile** of slick's 184 sources (the file
list `tests/bench.sh` pins, including the seven expanded FreeMarker
templates), with `-Xsource:3`, and scala-library 2.13.16, slick's dependency
jars and scala-reflect on the classpath: type checking → erasure → code
generation → writing the class files. The same compile with real scalac
2.13.16 took 12.0 s wall and 68.6 s of CPU when it was last compared.

### Where we stand

The latest recorded figures, each a same-machine before/after comparison
rather than an absolute benchmark:

* **slick, 184 sources**: 3.06 s wall, 2.80 s user CPU, 1504 class files
  (medians of eight alternating pairs, 2026-09-12).
* **slick, 2026-09-24**: main had drifted to 1.24e11 instructions and 6.4 s
  user CPU by then, and it stops at two type errors (JdbcModelBuilder,
  Compiled) before code generation, so these figures cover type checking
  only. The ancestry caches, the function-parent walk and the shared
  conversion memo below took it to 6.37e10 instructions (-48%) and about
  3.7 s user, with the same diagnostics.
* **gitbucket, 354 sources**: about 20 s on a quiet machine, 2.86e11
  instructions retired, after the linearization / base-type caches of
  2026-09-12 (from 154 s and 2.57e12). Macro expansion is under 1 s of that.

The merge gate's wall time, per step, is printed in each gate's summary and
recorded per gate in `tests/BASELINE.md`.

### How to measure

```bash
tests/bench.sh            # full compile, twice; reports real and user
tests/bench.sh --parse    # parse only (this one really does stop early)
REPS=3 tests/bench.sh     # change the repeat count
```

To compare two binaries, use `tests/bench_compare.py`. It alternates the two
binaries, writes each run to a fresh directory, and fails on a failed compile,
empty output, different diagnostics or any class file whose SHA-256 differs,
instead of reporting that as a speedup:

```sh
python3 tests/bench_compare.py /path/to/before /path/to/after \
  --sources-file /path/to/files.txt \
  --scala-library /path/to/scala-library-2.13.16.jar \
  --classpath "$(cat /path/to/deps.cp)" \
  --compiler-arg=-Xsource:3 --reps 4
```

The source manifest holds one path per line; use `tests/bench.sh`'s file list.
The runner does not download dependencies or build either binary, and prints
per-run timings and medians as JSON lines.

`tests/battery_cost.sh [check ...]` times every check in the verification
battery. Run it every few changes: a check that suddenly doubles is a bug
report, not a fact of life.

### Measuring on a loaded machine

This machine usually has several agents on it. The same binary has produced
1.87 s and 2.78 s of `user` inside one minute, so a 2% change is invisible in
`user` and a 6% change is a coin flip.

* **Compare two binaries, alternating, back to back.** Never measure one
  commit and then the other. Build the old one into a scratch tree
  (`git archive <rev> | tar -x -C <dir>` and `cargo build --release` there)
  rather than switching the working tree.
* **`/usr/bin/time -l` reports `instructions retired`** on Apple silicon, and
  it is stable to about 1% regardless of load. It is the right metric for
  deciding whether a change did anything:

  ```
  /usr/bin/time -l ./scala-rs compile … 2>&1 | grep 'instructions retired'
  ```

  Drive the compiler directly: timing a wrapper script counts only the shell.
* **Instructions understate a cache-miss win.** A change that removes memory
  traffic rather than arithmetic can move instructions by 1% and CPU time by
  8%. Take instructions as a lower bound and confirm with CPU time at a quiet
  moment, or with `cycles elapsed` from the same output.
* **Report the minimum, not the median, when the load is high.** Contention
  only ever makes a run slower, so over enough alternating pairs the fastest
  run of each binary is the least-disturbed estimate of both.
* Tests that compare JVM output (`names_match_released_scala…`, `numtower`)
  must run on JDK 17; see [testing.md](testing.md).

### Profiling

Profiles are taken with macOS `sample` (`sample <pid> <seconds> -f out.txt`).
Read the **call graph** (the tree at the top), not only the "Sort by top of
stack" summary at the end: the per-phase breakdown appears only in the call
graph. Three traps:

* **`--typer` does not stop after type checking.** It is a flag that dumps the
  typed tree; the compile runs to the end regardless. Neither "time under
  `--typer`" nor "full minus `--typer`" measures a phase.
* **A blocked thread is sampled exactly like a working one.** The class-file
  writer threads spend most samples in `__psynch_mutexwait` because the pool's
  shared receiver mutex is held across a blocking `recv`; they are asleep, not
  contended. The macro engine's stderr collector sits in `read` for the whole
  run by design.
* **Recursive functions break naive inclusive-time arithmetic.** `sample`
  prints a recursive call under itself, so summing every occurrence of a
  recursive function counts the same samples once per frame. Count a symbol
  once per root-to-leaf path, or read the entry points into it from outside.

`SCALA_RS_MACRO_TIMING=1` makes `crates/typer/src/expand_timing.rs` print, per
macro expansion and in total, request serialisation, wall time on the pipe,
the engine's own compute, the implementation's run, the cost of answering the
engine's questions, tree rebuilding and re-typing at the call site, as a
Markdown table on stderr. It costs one extra round trip per expansion and is
off unless the variable is set.

### Ancestry caches and conversion memo (2026-09-24)

Four changes, each measured on slick in instructions retired (1.236e11 at
the start, 6.37e10 at the end):

* **The implicit memo spans `conversion_result`** (-27%). It asked
  `conv_param_matches` and then `instantiate_conv_type`, and both solve the
  conversion's type arguments from its implicit clause with the same
  searches; the memo died between them, so every search ran twice. The
  same pairing in `warm_conversion_witnesses` (`conv_param_matches`, then
  `conv_implicit_params`) now shares one memo too.
* **`class_reaches`, `is_ancestor_of` and `inherits_from` are cached**
  (`ReachCache`, same `mutation_gen` rule as `LinCache`; -13%). The two that
  resolve parents through `class_sym_of` keep an answer only when every
  parent they followed named its class outright, and never under an ambient
  expansion guard, for the reason `lin.rs`'s `parent_names_its_class` gives.
* **The subtype walk's symbol-level "no" walks through function parents**
  (`subtype_class_reaches`; -14%). `class_reaches` gives up at a function
  parent, and `Seq` reaches `Function1` through `PartialFunction`, so every
  `Seq`/`List`/`Map` asked about an unrelated class took the full
  substituting walk: 4.1 million parent walks, down to 2.4 million.
* **That "no" is taken before `hk_alias_sub_type` and the path-member and
  projection traversals** (`classes_unrelated`; -3%), which only rewrite a
  proper class's arguments.

What remains on the same profile is the extension search itself. For every
select that falls back on a view, every conversion in scope is tried, and
cats' syntax conversions (`toComposeOps[F[_, _], A, B](fab: F[A, B])(implicit
F: Compose[F])`) each search their witness with `F` undetermined, which fits
every instance and comes out ambiguous. The answer does not depend on the
receiver, but it does depend on the position, so it cannot outlive the memo
without keying on the lexical context. `warm_conversion_witnesses` is the
same loop (about 105 calls, 5 ms each, almost none of which change a symbol).

### What is left

From the last profiles (slick and gitbucket, 2026-09-12):

* **Type checking dominates** (well over half of a slick compile), and within
  it member selection and the implicit-conversion search it falls back on.
  Pruning conversion candidates by "could the result have a member of this
  name?" is not obviously safe: the member may exist only in the result
  class's pickle, and asking for it is the expensive, mutating call the prune
  was meant to avoid.
* **`Type` is deep-cloned and structurally compared everywhere.** `Type::clone`,
  its drop glue, `memmove` and the allocator are spread over the whole
  compile, and gitbucket's remaining profile is flat with no dominator.
  Interning types (an index, or `Rc` for shared subtrees and argument vectors)
  is the next real step, and it reaches every part of the compiler.
* **Memoisation keyed on "the symbol table has not changed" only pays in big
  compilations.** A corpus test is a few lines; its time is process start-up
  and reading the library jar.
* The compile is single-threaded. Parsing is trivially parallel; the typer
  shares a mutable symbol table and is not.

### History

Earlier versions of this page recorded each optimisation pass in detail (the
first pass from 217 s to 3.5 s on slick, the class-file writer, implicit-search
memoisation, the `agent/macroperf` caches that took gitbucket from 154 s to
20 s, and the merge gate's parallelisation), with their tables and
measurements. Read them with `git log -p -- docs/performance.md`.

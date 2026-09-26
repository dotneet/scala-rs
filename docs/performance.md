## Speed

### What is measured

The standard workload is a **full compile** of slick's 184 sources (the file
list `tests/bench.sh` pins, including the seven expanded FreeMarker
templates), with scala-library 2.13.16, Slick's dependency jars and
scala-reflect on the classpath: type checking → erasure → code generation →
writing the class files. Historical scala-rs-only runs used `-Xsource:3`;
the direct scala-rs/scalac comparison below gives both compilers
`-Xsource:3-cross`, matching Slick's Scala 2.13 build setting. Its medians
supersede the older, differently configured scalac observation of 12.0 s
wall and 68.6 s CPU.

### Where we stand

The latest recorded figures, each a same-machine before/after comparison
rather than an absolute benchmark:

* **slick, 184 sources**: 3.06 s wall, 2.80 s user CPU, 1504 class files
  (medians of eight alternating pairs, 2026-09-12).
* **slick, 2026-09-24, full-compilation comparison**: the two remaining type
  errors in `JdbcModelBuilder` and `Compiled` have been fixed. On the same 184
  sources, dependency classpath, JDK 21.0.2 and `-Xsource:3-cross` (Slick's
  Scala 2.13 setting), three alternating fresh-output runs took 3.98, 4.01,
  4.04 s wall with scala-rs and 10.59, 10.95, 10.83 s with scalac 2.13.16.
  Both exited successfully and emitted 1504 and 1498 class files respectively.
  The wall-time median is 4.01 vs 10.83 s, or 2.70x in favor of scala-rs for
  this compiler-only workload. The twelve differential client programs,
  compiled against scalac's Slick output, all produced identical output with
  both libraries in three runs each. Reverse interoperability remains red:
  when those clients are compiled against scala-rs's Slick output, scalac
  rejects eleven of twelve because methods such as `createStatements` and
  `statements` are missing from its view of the emitted API. The JVM class
  sweep found no verification failures but could not initialize two classes.
  This timing is not evidence that Slick is a drop-in binary replacement.
* **slick, earlier 2026-09-24 type-check-only comparison**: main had drifted
  to 1.24e11 instructions and 6.4 s user CPU and stopped at the two type
  errors before code generation. The ancestry caches, function-parent walk
  and shared conversion memo reduced that to 6.37e10 instructions (-48%) and
  about 3.7 s user, with the same diagnostics. Those figures are not
  comparable to the full-compilation wall times above.
* **gitbucket, 354 sources**: about 20 s on a quiet machine, 2.86e11
  instructions retired, after the linearization / base-type caches of
  2026-09-12 (from 154 s and 2.57e12). Macro expansion is under 1 s of that.
* **2026-09-24, second pass** (below): slick 5.83e10 -> 4.64e10, gitbucket
  3.27e11 -> 1.03e11 instructions (19 s -> about 11 s), a one-table slick
  client with two macro expansions 1.10 s -> 0.63 s wall, a hundred-table
  one 3.9 s -> 2.6 s. Diagnostics of slick, gitbucket, cats and the library
  unchanged throughout.

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

For an optional profile-guided compiler build, see
[Experimental profile-guided builds](performance-pgo.md). Keep a matched
non-PGO control and validate held-out workloads before adopting the result.

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
  (`ReachCache`; -13% in the original measurement). Its current epoch is
  `graph_gen`, which excludes method, term and type-parameter mutations,
  rather than `LinCache`'s broader `mutation_gen`. The two that
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

### View pruning, seen-from caches and the macro engine (2026-09-24)

Measured against the `main` of the morning (5.83e10 slick, 3.27e11
gitbucket), each step on top of the last:

| change | slick | gitbucket |
|---|---:|---:|
| drop a view whose result class lacks the member first (`view_result_may_have_member`) | -9% | -34% |
| same-named declarations only, value types once (`shadow_inherited_implicits`) | 0 | -14% |
| import-prefix views kept across searches (`SeenCache`) | 0 | -25% |
| `SeenCache` keyed by symbol, `ReachCache` on `graph_gen`, self types kept | -1% | -17% |
| clone and walk trimming (`subst_tparams_cow`, `lub_at`, warm dedupe, one mentions walk) | -9% | -10% |

* **The view prune is nsc's** (`ImplicitComputation.survives`, which checks
  `?{def name: ?}` against the result before typing the view). It runs the
  exact test the extension loop already made after `conversion_result`, so
  it cannot drop a view the loop would have kept -- with one exception it
  had to learn: asking a hand-written prelude class (`RichInt`) its pickle
  for every view in scope completes members nothing else would, and made
  `intWrapper` a view to `Ordered[Int]` (a latent bug, reachable by writing
  `new RichInt(1) < 2`; the prelude is left out of the prune).
* **`mutation_gen` moves at nearly every statement.** Typing a body sets the
  type of every local and method it meets through `get_mut`, so a cache keyed
  on it is thrown away constantly; gitbucket hands out about 200,000 methods,
  130,000 terms and 90,000 type parameters against 40,000 classes.
  `graph_gen` counts only classes, modules, packages and type members, and
  is the right key for anything that reads parent lists alone.
* **What `SeenCache` may read.** The substitutions it keeps
  (`subst_as_seen_from`, `expand_in_type`) read symbols, `abs_projection_of`,
  the ambient guards and -- through `lookup_type` -- the scopes, for a
  `Type::Named` and for a `Type::Tuple`'s `TupleN`. Neither of the last two is
  kept, and nothing on those paths reads `this_class`.

**Macros.** A macro run pays for the engine's JVM and runtime universe once
(about 0.43 s wall and 0.9 s of CPU); an expansion after that costs 0.3-3 ms,
about what scalac spends on it. Two changes:

* The engine starts on its own thread when the typer does, whenever
  scala-reflect.jar is on the classpath (`PendingEngine`); the first
  expansion waits only for what is left. `SCALA_RS_MACRO_PRESTART=0` turns it
  off. A run that expands nothing pays for a JVM on another core.
* Each file's text goes to the engine once (`fill_source_text`); before, every
  request carried it, and the engine re-read and re-parsed it per call.

AppCDS for the engine would save a further 0.2 s and 0.5 s of CPU per run,
but it needs a jar-only classpath prefix, and putting scala-library and
scala-reflect first changes which class wins when the user's classpath
shadows one.

### Shared types, per-context implicits and shapeless (2026-09-24)

**Types.** `Type`'s `Vec<Type>` fields are `TyList` and its `Box<Type>`
fields `TyBox`: both share their contents behind an `Rc`, so a clone is a
reference count, and both record the `TypeFlags` of what they hold when they
are built (rustc's `TypeFlags`). "Does this type mention a type parameter /
a type member / a name?" is a bit test on the node, `any_type_masked` skips
subtrees without the kinds it looks for, and `subst_map` hands back a subtree
with nothing to substitute as it is. A flag is "may contain", but a lost bit
costs the caches keyed on it, so the flags are otherwise exact: summarising
a refinement's declarations as "everything" cost `SeenCache` all its hits on
gitbucket and made it 30% slower. slick -7%, gitbucket -7%.

**Implicits in scope.** `SymbolTable::scopes` is a `Scopes`, stamped with a
version every write moves (`DerefMut`), and `InScopeCache` keeps the list
per scope version, `this_class`, the class `this` means and the
super-constructor flag, while `mutation_gen` and the symbol count stand
still. Four calls in five hit on gitbucket (-5%).

**shapeless.** A `Generic` + `Lazy` derivation over forty nested case
classes (`Show` for `C0` .. `C39`, each holding the previous one) expands
`Generic.materialize` 10,000 times and `LazyMacros.mkLazyImpl` 8,300 times,
with 34,000 round trips to the engine. It took 64 s, scalac 12 s -- and
scalac expands *more* (42,000 times, `-Ystatistics:typer`), so the gap was
the cost of a round trip, not their number. What it went to, and what
changed:

| where | cost | change |
|---|---|---|
| engine: `c.openImplicits`, asked at every level | 3/4 of the engine: the whole open-implicit stack re-sent, re-parsed and every type rebuilt by reflection | entries sent once and named by handle after; runtime types kept per wire text |
| engine: every reflective `call` | a `class#name/arity` string built and hashed, parameter types copied | lookup by class, arity and name; parameter types kept |
| typer: `_root_` and `inst$macro$N` | the whole unqualified-name search, probing every jar for each fresh name | answered at once |
| typer: `package_object_of` | a package object refolded into its package per unknown name | skipped while neither has gained a member |
| typer: `openImplicits` answers | every wanted type rewritten per answer | wire of a context-free type kept (`graph_gen`) |
| bridge | a thread spawned per timed read | one reader thread per engine |

The derivation now takes about 13 s (scalac 12 s); a fifteen-class one 3.3 s
(scalac 7.0 s). The fixture is generated; see `crates/cli/tests/
macrotransportbatch.rs`'s `shapeless_lazy_derivation_matches_scalac` for
the shape. The engine is profiled with JFR through `JAVA_TOOL_OPTIONS`
(`-Xlog:jfr+startup=error` keeps JFR's banner off the protocol's stdout); it
is killed at the end of a run, so the recording has to be dumped before
that.

### What is left

From the profiles of 2026-09-24:

* **Type checking still dominates**, within it the implicit search: views
  whose result class does declare the member still solve their type
  arguments against every receiver (`conv_param_matches`); in a shapeless
  derivation, the search each `c.inferImplicitValue` answers starts from
  scratch, where nsc's derivation context shares it.
* **`mutation_gen` moves at nearly every statement**, which bounds every
  cache keyed on it; `graph_gen` covers the class graph only.
* **`Method.paramss` is still `Vec<Vec<Type>>`**, and `Named.name` a
  `String`: the remaining deep copies are there and in trees.
* The compile is single-threaded. Parsing is trivially parallel; the typer
  shares a mutable symbol table and is not.

### History

Earlier versions of this page recorded each optimisation pass in detail (the
first pass from 217 s to 3.5 s on slick, the class-file writer, implicit-search
memoisation, the `agent/macroperf` caches that took gitbucket from 154 s to
20 s, and the merge gate's parallelisation), with their tables and
measurements. Read them with `git log -p -- docs/performance.md`.

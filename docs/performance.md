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
* **circe/shapeless derivations, 2026-09-26** (below): 19.5 s -> 13.3 s wall,
  1.88e11 -> 1.28e11 instructions; with sealed-trait decoders, which did not
  compile before, 14.5 s against scalac's 17.1 s.
* **44 synthetic workloads against scalac, 2026-09-27** (below): every one
  now compiles in 0.01--0.54 of scalac's wall time; before, scalac won five
  and two did not compile.
* **2026-09-27, later** (below): the large synthetic kinds another 13--24%
  fewer instructions; gitbucket 4.3 s against scalac's 10.9 s and cats 3.3 s
  against 14.2 s, both with no errors.

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

### Macro-heavy derivations against scalac (2026-09-26)

**Workload.** `tests/macro_bench_gen.py` writes 30 files of ten case classes
each, every one deriving circe's `Encoder` and `Decoder` with `semiauto`
(shapeless `Lazy` and `LabelledGeneric` underneath, later classes holding
earlier ones), a four-case sealed trait per file, and values derived by
`generic.auto`; `Main` prints every round trip. The classpath is in the
script's header. JDK 17.0.20, 12 cores, fresh processes, alternating runs.
Give scalac a heap: its launcher's default `-Xmx256M` ends this workload in
`OutOfMemoryError` (`JAVA_OPTS="-Xmx2g -Xss4m"`, which is also nearer what
sbt runs it with).

| sources | before | after | scalac 2.13.16 |
|---|---:|---:|---:|
| `--no-coproduct-decoder` | 19.5 s, 1.88e11 instr. | 13.3 s, 1.28e11 instr. | 15.5 -- 21 s |
| full (sealed-trait decoders) | does not compile | 14.4 -- 14.5 s | 16.9 -- 17.3 s |

Both compilers' programs print the same. Before and after emit the same
classes except for the numbers in fresh macro names (`exportDecoder$macro$N`,
`inst$macro$N`): fewer expansions draw fewer names. Comparing `javap -p -c`
per class with those numbers normalised gives equal sets of 2,553 classes.
scalac uses 52--55 s of CPU for its 17 s, scala-rs 19.5 s; on a loaded
machine scalac's wall time moves far more than scala-rs's. A direct
`Generic` + `Lazy` derivation (`Show` over 15 and 25 nested case classes, the
shape of `shapeless_lazy_derivation_matches_scalac`) was already faster than
scalac and still is: 0.90 s and 1.10 s against 1.76 s and 2.31 s.

**Where the time went.** Before the changes the typer thread spent 36% of the
compile waiting for the engine, 25% answering the engine's
`c.inferImplicitValue` and 26% typing expansions; 215 MB crossed the pipe in
92,000 round trips for 14,640 expansions. Each change adopts what makes nsc's
side cheap:

| nsc | scala-rs before | change | effect |
|---|---|---|---|
| expands a whitebox implicit candidate once, while typing it (`typedImplicit`) | a fitted expansion was reused only in the very open-implicit context it was fitted in, so `mkDefaultSymbolicLabelling` and `Generic.materialize` ran four times per derivation; one expansion per search pass, each pass a full search | an expansion that took no argument, read no implicit scope and asked the engine nothing context-dependent (`implicit_scope_reads`, `macro_context_queries`) is reused in any context at the call site, after the divergence check a new entry would get; every pending expansion runs before the next pass (`expand_next_whitebox_entry`) | 14,640 -> 9,421 expansions; 1.63e11 -> 1.36e11 instr., 16.3 -> 14.1 s |
| builds an implicit argument while searching for it | fitting a candidate searched its implicit arguments, building its tree searched for each again (46% of 181,000 searches) | the fit's answers, memo entries, live until the implicit operation ends (`with_implicit_decisions`) and building takes them under the memo's own open-stack and depth rules, those found with undetermined type variables under the solved type too | 181,000 -> 140,000 searches, -5% instr. |
| parses a class file once per class (`ClassfileParser`) | overload checks and forwarder probes re-inflated and re-parsed the same library class files: 27,500 parses in a 10-file compile | `Typer::parsed_classfile` keeps them for the run | with the two below, 1.88e11 -> 1.72e11 instr. |
| runs macros in-process, reading fields directly | 62.9 million reflective calls from the engine, most for the universe's constants (`NoSymbol`, `EmptyTree`, `definitions`, companions) and `Iterator`/`Product`/`Tree`/`Symbol` accessors | constants kept (`stableMember`), accessors through `static final` method handles (`Fast`) | 6.8 million reflective calls left |
| shares types | every refinement got a fresh wire label, so no type containing one -- every labelled `HList` field -- was ever found in the engine's type cache | labels by content (`refined_label`), the engine caches such types | engine+pipe -0.6 s, rebuilding replies -0.26 s |
| -- | a timed reply read went through a reader thread and a channel, two thread wake-ups per line | `poll(2)` on the pipe before each blocking read (Unix; Windows keeps the thread) | -0.5 s |
| -- | the engine asked for the same companion several times per expansion | companions it has found are kept | 16,000 fewer round trips |
| -- | `expects_function_value` built the full SAM signature to answer yes or no | `SymbolTable::is_sam_type` stops before the as-seen-from substitution | -1% instr. |

A round trip on the pipe costs about 12 us (measured by doubling the
`symbol` questions), so the remaining 77,000 cost about 1 s; the engine's
own work, not the pipe, is what the typer waits for.

**A bug on the way.** A by-name argument inside an answer tree went to the
engine as the typer's internal `() => e` thunk; shapeless's `Lazy`
untypechecks what it collects, and the thunk came back as a function
literal. circe's coproduct decoder passes `c.history` by name to the
overloaded `DecodingFailure.apply`, so `deriveDecoder` of any sealed trait
failed with `no matching overload`. Answers now carry the expression, as
nsc's typed tree does (`byname_thunk_body`); see `docs/macros.md` §4.2.

**Still behind nsc here.** 140,000 implicit searches against nsc's 41,800
(`-Ystatistics:typer`): a labelled-generic derivation still takes five search
passes, one per whitebox output it discovers (0.7 s), and every candidate's
arguments are fitted again in each. The engine spends most of a `Lazy`
expansion (1.7 ms) parsing, building and writing trees. A derivation nested
more than about 32 levels deep stops at `MAX_QUERY_DEPTH` (64 queries, two
per level), where scalac has no such limit.

### Across the board against scalac (2026-09-27)

**Workload.** `tests/scalac_bench_gen.py` writes 44 synthetic programs, one
per language area (collections, for-comprehensions, pattern matching and
exhaustivity, implicits and type classes, Java interop and SAM lambdas,
literals, case classes, value classes, futures, higher-kinded types,
`BigDecimal` arithmetic, stackable traits, ...), 20 files each unless noted;
`tests/scalac_bench.sh` compiles each with both compilers (`-nowarn`, scalac
with `-Xmx2g`), runs both programs and compares their output. One fresh
process each, JDK 17.0.20, 12 cores, 2026-09-27. Wall seconds:

| kind | scalac | before | after | after / scalac |
|---|---:|---:|---:|---:|
| `bigenum` (300-case sealed matches) | 58.7 | 165.6 | 2.5 | 0.04 |
| `patmat_big` (100 files) | 45.4 | 20.0 | 6.1 | 0.13 |
| `bignum` (`Int op BigDecimal`) | 4.3 | fails | 1.7 | 0.39 |
| `literals` (800-entry tables) | 3.9 | fails | 0.9 | 0.24 |
| `futures` | 7.6 | 7.9 | 2.1 | 0.28 |
| `hkt` | 4.9 | 4.8 | 1.0 | 0.20 |
| `javainterop` | 5.1 | 3.4 | 1.3 | 0.26 |
| `javainterop_big` (100 files) | 13.5 | 16.8 | 6.6 | 0.49 |
| `typeclass_big` (100 files) | 5.5 | 5.1 | 2.2 | 0.40 |
| `typeclass_n200` (200 files) | 8.1 | 10.2 | 4.4 | 0.54 |
| `lambdas_big` (100 files) | 21.9 | 15.1 | 8.1 | 0.37 |
| `coll_big` (100 files) | 21.9 | 11.9 | 8.3 | 0.38 |
| `forcomp_big` (100 files) | 20.9 | 10.8 | 9.2 | 0.44 |
| `chains_big` (100 files) | 14.0 | 8.0 | 6.5 | 0.46 |

The other 30 kinds went from 0.01--0.43 to 0.01--0.31 of scalac's time, and
every program prints what scalac's does (three print a lambda, whose class
name differs). On the real code bases: 538 sources of the standard library
type-check in 2.3 s instead of 11.9 s (same 18 diagnostics); slick compiles
in 3.7 s instead of 4.0 s.

**What was slow, and what nsc does instead.**

* *Exhaustivity analysis was re-solved for every match.* Code written from
  a template repeats one match shape hundreds of times. Whether a case is
  reachable and whether a match is exhaustive are satisfiability questions,
  so their answers are now kept per translated match, positions left out
  (`warn_patmat::AnalysisMemo`); a hit advances the symbol-id counter as
  solving would. Counter-examples of a non-exhaustive match can follow
  hash-set order and are solved every time.
* *A lub per argument, from scratch, per argument.* Each argument's
  prototype re-inferred the callee's type parameters from all earlier
  arguments, so `Map(k0 -> v0, ..., k799 -> v799)` was a million joins. An
  argument repeating an earlier (formal, type) pair now reuses the last
  solution. Within one outermost `lub`, repeated sub-lubs are memoised, as
  nsc's `lubResults` are.
* *Caches that never hit.* The SAM check erasure asks of every tree's type
  cached nothing for `Integer` (an `AnyVal` parent) or any `Seq` (a
  function-typed parent above it), and scanned constructors before the
  cheap answer; the emitter looked classes up by internal name with a scan
  of the whole symbol table. `member_graph_gen` (`graph_gen` plus members
  entered by `alloc`, which `graph_gen` did not see) now keys caches that
  read member lists: companion implicits (nsc's `implicitsCache`), SAM
  walks, as-seen-from walks per receiver.
* *Work that changed nothing, repeated.* Warming implicit witnesses and
  completing a member from the pickles are additive; a request that changed
  no symbol is remembered until the class graph moves.
* *A class per SAM literal.* nsc implements a SAM type through
  `LambdaMetafactory` when it compiles to a pure interface (a Java
  interface, or traits with nothing but abstract members); scala-rs emitted
  an anonymous class for every one, 20,000 class files where scalac wrote
  200. The closure classes per enclosing class now match scalac's
  (`sam_literals_use_lambda_metafactory_where_scalac_does`).
* *Dead branches emitted.* nsc folds `0 % 2 == 0` and never keeps the branch
  that cannot run; a table of such entries overflowed the 64 KB method
  limit here and not in scalac.

**Bugs found on the way.** `Stream.collect(Collector[T, A, R])` came back a
`Stream[A]` (a `collect(pf)` heuristic read any class's second type
argument); `2 / y` with `y: BigDecimal` did not find
`BigDecimal.int2bigDecimal`, which nsc's view search takes from the
argument's implicit scope. Both have scalac-compared tests.

**Still behind.** At 100--200 files scalac's JIT has warmed up, and its
marginal cost per file is 0.15--0.2 s against scala-rs's 0.06--0.1 s: the
large kinds sit at 0.4--0.55 of scalac where their 20-file versions are at
0.2--0.3. Everything runs on one core. A SAM literal of a trait read from
the classpath still gets a class, since its initializer is not known here.

### Shared parameter lists, stable caches and real code bases (2026-09-27)

**Where the time went.** After the pass above, type checking was still two
thirds of every large workload, and within it copies and caches that did
not survive a statement: `Type::Method`'s parameter lists were deep-copied
at every as-seen-from, member lookups and outer implicit scopes were
recomputed per selection because the counters keying them moved at every
local definition, and the class files were written only after the last
unit was generated.

**What changed.**

* `Type::Method { paramss }` is a `ParamClauses`: one reference-counted
  `Vec<Vec<Type>>` with its type flags computed once, so cloning a method
  type is a pointer copy and "does this mention a type parameter" is a bit
  test. Template checks borrow the body instead of cloning it; argument
  flattening in the backend borrows unless clauses really are joined.
* Each `Scope` carries a stamp renewed only when a binding is entered or
  replaced. The implicits of the scopes outside the innermost template are
  cached per (stamps, class, `member_graph_gen`), so a statement no longer
  invalidates what the enclosing class and package offer. `lookup_member`
  keeps nominal answers until `member_graph_gen` moves.
* The typer reads the node-id bound the parser already knows instead of
  walking every tree for it; the local-object checks run only when some
  class is owned by a method or value.
* Class files are written by the pool while later units are generated.
  A backend error in a later unit still leaves nothing of the run behind:
  the writer deletes what it wrote (`backend_error_takes_back_classes_already_written`).
  Staging into a directory and renaming cost as much as the writes when a
  run's classes share one directory.

Instructions and wall time, one fresh process each, base = `dd44c8a2`:

| workload | base | now | wall base | wall now | scalac wall |
|---|---:|---:|---:|---:|---:|
| `coll_big` | 102.1G | 79.2G | 8.5 s | 6.7 s | 21.9 s |
| `forcomp_big` | 110.6G | 84.4G | 9.3 s | 7.4 s | 20.9 s |
| `chains_big` | 79.9G | 60.2G | 6.4 s | 5.1 s | 14.0 s |
| `javainterop_big` | 85.3G | 63.6G | 6.6 s | 5.2 s | 13.5 s |
| `typeclass_n200` | 65.3G | 58.8G | 4.4 s | 4.0 s | 8.1 s |
| `lambdas_big` | 114.6G | 93.0G | 8.2 s | 6.2 s | 21.9 s |
| `patmat_big` | 82.0G | 69.7G | 6.2 s | 5.1 s | 45.4 s |
| slick, 184 sources | 28.2G | 26.9G | 2.2 s | 2.0 s | |

`lambdas_big`'s write phase went from 0.55 s to 0.006 s; the classes are on
disk by the time the last unit is generated.

**Real code bases against scalac.** gitbucket (354 sources, `-Xsource:3-cross`)
and cats kernel+core (340 sources, `-Xsource:3`, with the kind-projector and
better-monadic-for plugins on scalac's side) had both regressed to three
type errors each; they compile again with none:

| code base | scalac | scala-rs | classes (scalac / scala-rs) |
|---|---:|---:|---:|
| gitbucket | 10.9 s | 4.3 s | 1314 / 1314 |
| cats kernel+core | 14.2 s | 3.3 s | 3553 / 2948 |

cats' class count differs partly by the specialised classes
`-no-specialization` leaves out. The regressions found on the way, each with
a scalac-compared test: a `toSet[B >: A]` stand-in without its type parameter
(`lower_bound_widening`), an expected `Unit` deciding a generic call's type
parameters where nsc's
`isWeaklyCompatible` lets it decide nothing (`unit_discard_generic`), and a
newtype's abstract `Type` read off its declaration instead of the owning
object's path, which lost the implicit ops conversion
(`newtype_alias_prefix`). Partial-function literals now compile to nsc's
classes, names and package included (`pk/M$$anonfun$m0$1`, extending
`AbstractPartialFunction`); they had been written to the default package.

**Not done.** Code generation reads the symbol table through `Rc`-shared
types, so it cannot run on threads without making `Type` `Send`; forking
the process after typing was considered and left out, since the classpath
and writer threads are running by then.

### What is left

From the profiles of 2026-09-24:

* **Type checking still dominates**, within it the implicit search: views
  whose result class does declare the member still solve their type
  arguments against every receiver (`conv_param_matches`); in a shapeless
  derivation, the search each `c.inferImplicitValue` answers starts from
  scratch, where nsc's derivation context shares it.
* **`mutation_gen` moves at nearly every statement**, which bounds every
  cache keyed on it; scope stamps and `member_graph_gen` now key the
  implicit-scope and member-lookup caches instead, but the SAM and
  conversion caches still read it.
* **`Named.name` is a `String`**: the remaining deep copies are there and in
  trees.
* The compile is single-threaded apart from class-file writing. Parsing is
  trivially parallel; the typer shares a mutable symbol table and is not,
  and the backend reads `Rc`-shared types.

### History

Earlier versions of this page recorded each optimisation pass in detail (the
first pass from 217 s to 3.5 s on slick, the class-file writer, implicit-search
memoisation, the `agent/macroperf` caches that took gitbucket from 154 s to
20 s, and the merge gate's parallelisation), with their tables and
measurements. Read them with `git log -p -- docs/performance.md`.

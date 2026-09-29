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
* **Library-heavy code, 2026-09-27** (below): code over cats, cats-effect and
  monad transformers had been 1.8--5.3 times slower than scalac; it is now
  0.3--0.5 of scalac's time.
* **A private 97-module application, 2026-09-29** (below): 175 s -> 121 s for
  every module in turn; its largest module 22.1 s -> 15.3 s against scalac's
  27.2 s, with identical class files. With the macro daemon by default and
  the compatibility fixes that let every module compile: 92.5 s against
  scalac's 339.5 s.

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

### Library-heavy code against scalac (2026-09-27)

**Where scalac still won.** The synthetic kinds above lean on the standard
library. `tests/library_bench_gen.py` adds code written against libraries,
compiled against their jars from the Coursier cache (cats 2.13.0, cats-effect
3.7.1, fs2 3.13.0, circe 0.14.7), and `tests/library_bench.sh` compares both
compilers on it, running both programs. Of twenty candidate shapes, five
were slower than scalac, by 1.7 to 5.3 times, all written over
`cats.syntax.all._`; one more, very large literal collections, was even:

| kind | scalac | before | after |
|---|---:|---:|---:|
| `catsyntax` (`traverse`, `mapN`, `foldMap`, `\|+\|`, `Validated`) | 9.1 s | 49.3 s | 4.2 s |
| `catsdata` (`Validated`, `NonEmptyList`, `Ior`, `Kleisli`) | 8.0 s | 27.6 s | 2.5 s |
| `monadtrans` (`EitherT` / `OptionT` / `StateT` over `IO`) | 5.4 s | 17.0 s | 1.8 s |
| `catseffect` (`IO`, `Ref`, `parTraverse`, `Resource`) | 5.8 s | 12.6 s | 1.7 s |
| `tagless` (`F[_]: Monad` programs run in `State`) | 6.3 s | 10.9 s, rejected | 2.4 s |
| `biglits` (`List(...)` of 2000, `Map(...)` of 1000 elements) | 4.4 s | 4.3 s | 2.1 s |
| `fs2s` (fs2 `Stream` pipelines) | 4.1 s | 0.9 s | 0.9 s |
| `circeprod` (circe `forProduct5` codecs) | 5.1 s | 1.5 s | 1.5 s |

The other shapes -- type-level `Nat` induction, deep trait linearisation,
heavy overloading, long method chains, 22-field case classes, tuple
`Ordering`s, Java streams, source-defined syntax and instances -- run in
0.1--0.75 of scalac's time before and after.

**What was slow.** `import cats.syntax.all._` puts about two hundred views
in scope, each returning a refinement of an `Ops` trait
(`Traverse.Ops[F, A] { type TypeClassType = Traverse[F] }`). Every filter
that asks "can this view's result have member `m`" or "can it be a `T`"
answered *maybe* for a refinement, so each `xs.traverse(f)` solved the type
arguments and searched the witnesses of all two hundred views, and so did
each lambda a for-comprehension over `EitherT` retypes. Now:

* A view's refined result is read at its class parents, both when looking
  for an extension member and when warming a conversion's witnesses (where
  a wanted function type is read as its `FunctionN`).
* A view is rejected as an implicit *value* before it is read through its
  import prefix, which the fit did anyway, only later.
* Top-level searches keep their answers across implicit operations
  (`ScopeSearchCache`): at depth 0, with no caller type variables and
  nothing open, an answer is a function of the wanted type, the candidates
  in scope and the symbol graph, and is kept under a fingerprint of those
  (member graph generation, `this`, the lexical candidates, the declared
  types of candidates defined in source). Sixty `xs.combineAll` in a file
  were sixty identical searches over cats' instance scopes.
* `unify_tparam_all` joins an argument type once: `List(A, B, Cc(1), ...)`
  of 1500 elements has three element types and made 1500 compound lubs.

Each has a unit test that counts the work (`implicits::memo_tests`,
`check_infer::direct_result_tests::repeated_argument_types_are_joined_once`).
slick went from 26.9G to 25.4G instructions; gitbucket, cats and the
standard library produce the same diagnostics as before.

**Found on the way.** `tagless` was rejected: `cats.data.State.modify(f)`
written out in full reached the type alias `State` of `package object data`
instead of the object beside it, and selected the aliased class's instance
`modify` (`pkgobj_term`). A shapeless `Generic`-based derivation was "could
not find implicit value" with only shapeless on the classpath: its macros
need scala-compiler, which nsc always has and the macro engine now finds by
itself (`docs/macros.md` §2.4, `macro_runtime`).
The macro-heavy circe workload above is now the closest to scalac, at 13.0 s
against 17.8 s; most of it is the macro engine answering `inferImplicitValue`.

### Test frameworks, parsers, XML and derivation against scalac (2026-09-27)

A further twelve shapes: ScalaTest suites (`AnyFunSuite`, `AnyWordSpec` with
`should` matchers), scala-parser-combinators grammars, XML literals, circe's
`generic.auto` and `semiauto` derivation, and language shapes -- a trait of
300 abstract members implemented six times per file, `Enumeration`s,
custom and regex extractors, objects of 1500 `val`s, eight-deep generic
types. One was far behind, one about even:

| kind | scalac | before | after |
|---|---:|---:|---:|
| `xmllits` (60 XML literals per file, 20 files) | 4.4 s | 71.7 s | 2.3 s |
| `circeauto` (`io.circe.generic.auto._`, 20 files) | 17.5 s | 18.1 s | 18.1 s |
| `overloads` (earlier round) | 4.8 s | 2.5 s | 1.1 s |

The rest ran in 0.05--0.68 of scalac's time before and after
(`xmllits` is in `tests/library_bench_gen.py` now).

**What was slow.** A file of XML literals spent its type checking in
subtype questions with obvious answers. The view search tries every
primitive, array and string view of `Predef` against each receiver, and
each `List[Node] <: Short` walked `List`'s forty ancestors to say no --
220,000 times; `Text <: B` for a bare type parameter walked `Text`'s the
same way. And `::[B >: A]` joined `Atom` and `Elem` at every child, each
join comparing two deep hierarchies from scratch. Now:

* A class type is not below a primitive, an array or `String`, and not below
  a type parameter except through its lower bound; both are answered before
  any parent walk (`class_never_below`).
* Joins of two ground types -- no parameter, member, path or wildcard -- are
  kept across calls while the class graph stands still (`LubMemo::settled`).

**Still even.** `circeauto` is bound by the macro engine: 17,000 expansions
and 139,000 round trips to the JVM for 20 files, with scala-rs answering
the engine's questions in about 3 s of that. Engine flags (GC, JIT tiers)
change nothing.

**Found on the way.** A parser-combinator grammar's `~` was "not found"
as a type, a value and an extractor; fixed (`parser_combinators.rs`).
ScalaTest's `should have length` and `should not be empty` were "ambiguous
implicit" (the `Length` / `Emptiness` instances over structural and
higher-kinded bounds); fixed (`implicit_structural.rs`). Not yet fixed: an
XML program's `\\` / `\` query prints `List()` where scalac's prints the
items.

### Matchers, Free monads, circe parsing and more language shapes (2026-09-27)

Eleven further shapes, 20 files each: ScalaTest's `AnyFlatSpec` with
`must` matchers and inspectors, a `cats.free.Free` key-value DSL, circe
parsing and cursor navigation, and language shapes -- long
`for`-comprehensions, a 60-case sealed hierarchy matched with nested
patterns, `f`/`s`/`raw` interpolation, `Future` chains, generic `Numeric`
code, long collection pipelines, curried and partial functions, and
default and named arguments.

| kind | scalac | scala-rs | ratio |
|---|---:|---:|---:|
| `st_flatspec` | 5.39 s | 2.90 s | 0.54 |
| `catsfree` | 3.78 s | 0.40 s | 0.11 |
| `circeparse` | 3.07 s | 0.43 s | 0.14 |
| `forcomp` | 5.86 s | 0.71 s | 0.12 |
| `sealedmatch` | 8099 s | 953 s | 0.12 |
| `interp` | 2.49 s | 0.13 s | 0.05 |
| `futures` | 5.78 s | 0.74 s | 0.13 |
| `numeric` | 3.97 s | 0.53 s | 0.13 |
| `collchains` | 6.93 s | 0.76 s | 0.11 |
| `closures` | 5.56 s | 0.40 s | 0.07 |
| `defaults` | 4.29 s | 0.48 s | 0.11 |

`st_flatspec` did not compile at first ("no matching overload" for every
`forAll (xs) { ... }` and `contain allOf (a, b)`); it is measured after the
fixes. None is slower than scalac. `sealedmatch` is slow in both compilers
for the same reason: the exhaustivity and reachability analysis is nsc's
DPLL solver, run over a formula per case. Two of its costs were ours
alone -- the pure-literal scan rebuilt two ordered sets per step, and unit
propagation rewrote every clause whether or not it held the literal --
and one file of it now takes 29.6 s rather than 47.2 s (scalac: about
400 s), with the same warnings and class files.

`circeauto` remains the one workload behind scalac (18.8 s against
18.0 s). Its 139,000 round trips to the macro engine cost about 76 µs each
on the pipe; the reads already block on the pipe itself, so what is left
is the number of trips, which the engine's lazily completed symbols set.

### Scale: one large object, many files, deep implicits (2026-09-27)

Eleven more shapes: one `object` of thousands of members, 400 tiny files,
deep source-defined type-class derivation, shapeless `HList` operations,
F-bounded hierarchies, 40-deep alias chains, a 20-component cake,
anonymous classes, 22-element tuples, `@specialized` classes and 500
implicits imported at once. All but one ran in 0.06--0.39 of scalac's time.
Three did not compile at first and do now: the cake pattern
(`inner_class_type_members.rs`), the tuple `zipped` of `Predef`
(`predef_library_implicits.rs`) and shapeless's `take`/`reverse`
(`shapeless_hlist_ops.rs`).

The exception grew faster than the file: `bigfile` (one `object`, each
member a case class, a method and a value) took 0.45, 1.14, 2.07 and 5.0 s
for 500, 1000, 1500 and 2500 members. Three scans of the object's member
list ran once per member:

* completing a member's signature lazily re-created the scope stack it was
  declared in, and copied the object's scope -- thousands of names -- each
  time. A scope's table is now shared until it is written;
* every implicit search in a body collected the enclosing classes' members
  named like a candidate, reading each member of the object; the result is
  kept while the member lists stand still;
* entering each case class looked for a prelude symbol to shadow among the
  object's members (there are none in a class this run declares), and for
  its companion without the per-class cache `companion_module` keeps.

| members | typecheck before | after | scalac (whole compile) |
|---:|---:|---:|---:|
| 1000 | 0.37 s | 0.29 s | 6.6 s |
| 2500 | 1.36 s | 0.74 s | 11.0 s |
| 5000 | 4.55 s | 1.76 s | (class too large) |

What is left grows slowly: a member looked up by a name nobody asked for
before still reads the owner's member list once.

### Deep branches, literal lists and more shapes (2026-09-28)

Two more rounds, 22 shapes: long methods, 700 locals, wide hierarchies,
thousands of case objects, mixins, low-priority implicits, Java overloads,
`BigDecimal` arithmetic, generic inference, tail recursion, sealed matches
with nested patterns, closures, for-comprehensions, nested `try`/`finally`,
literal collections, lazy vals, value classes, Peano arithmetic in
implicits, extension methods and interpolators. Nineteen ran in 0.01--0.32
of scalac's time; scalac spent 274 s on the case objects and 426 s on a
60-case match with nested patterns, against our 2.0 and 2.5 s. circe's
automatic derivation stays at 1.04, bound by macro round trips. The other
two were slower than they should be.

**Deeply nested branches.** Twelve levels of `if { val y = …; … } else
(x match …)` took 1828 s against scalac's 460 s (both then reject the
method as too large). Every `val` of every branch kept a slot of its own to
the end of the method, so each branch target's stack map frame was as long
as the method's locals, and emitting was quadratic. A block's locals now go
out of scope at its end and their slots are reused, as javac does
(`local_slot_reuse.rs`); the assembler forgets the released slots too, or a
loop head records a type a later reuse contradicts. Each level doubles the
source:

| depth | source | before | after | scalac |
|---:|---:|---:|---:|---:|
| 7 | 0.3 MB | 0.26 s | 0.20 s | 2.3 s |
| 8 | 0.6 MB | 0.70 s | 0.44 s | 3.5 s |
| 9 | 1.3 MB | 2.04 s | 0.94 s | 4.3 s |

**Long literal lists.** `List("s0", …, "s1499")` was quadratic in its
length. Arguments are typed left to right, and each takes a prototype
solved from the ones before it; repeated (formal, type) pairs reuse the
last solution, but every literal has a constant type of its own, so every
pair was new. Only a formal that mentions the callee's type variables and
is not one itself uses that prototype, and `A*` is one itself: the solution
is now computed only for formals that can use it, and the earlier formals
are no longer rebuilt at every argument. 20 objects of 1500 elements each:

| literal | before | after | scalac |
|---|---:|---:|---:|
| `List` of strings | 0.96 s | 0.06 s | 6.6 s |
| `List` of ints | 0.58 s | 0.04 s | 1.2 s |
| `Array` of ints | 0.60 s | 0.06 s | |
| `Map` of `k -> v` | 0.79 s | 0.65 s | 4.2 s |

A string list of 8000 elements type-checks in 0.026 s instead of 1.26 s.
`Map` remains linear: it pays one extension search for `->` per element,
since each literal receiver is a type of its own.

The differential run of cats found an initialization-order bug on the way: an
implicit member of an enclosing class was selected through a companion
object that inherits it, whose `MODULE$` is still null while its parent's
constructor runs (`outer_this_implicit.rs`).

### Macros against scalac (2026-09-28)

Macro-heavy code is where scala-rs stays closest to scalac. Every expansion
runs on the JVM engine and every question it asks is a round trip, so the
ratios sit at 0.2--0.6 where plain code reaches 0.05:

| workload | scalac | scala-rs | ratio |
|---|---:|---:|---:|
| circe `generic.auto`, 20 files | 17.8 s | 16.9 s | 0.95 |
| circe semi-automatic derivation | 11.1 s | 6.9 s | 0.62 |
| ScalaTest `FunSuite` (`Position`, `assert`) | 4.9 s | 2.7 s | 0.55 |
| shapeless `Generic` derivation | 7.3 s | 3.2 s | 0.44 |
| user macro calling `c.typecheck` | 2.3 s | 1.3 s | 0.59 |
| user blackbox macro, 4800 calls | 2.8 s | 1.1 s | 0.38 |
| user implicit materializer (`Show[T]`) | 7.3 s | 2.7 s | 0.36 |
| user whitebox macro | 5.3 s | 1.2 s | 0.22 |

The automatic derivation is the slow one. scalac expands 56520 macros in
10.5 s; scala-rs reuses context-free expansions and runs 17171, but each
costs more: 5.3 s of the type checking waited for the engine and about 9 s
was our own work -- typing the expansions (4.3 s), answering the engine's
implicit searches (3.3 s) and rebuilding its replies. Measured changes:

* **Symbol descriptions in answers are fetched together.** The trees the
  engine gets back from `c.typecheck` and `c.inferImplicitValue` are mostly
  earlier expansions, full of locals the mirror has not seen, and each was
  described in a round trip of its own: 79648 of 131114 round trips. They
  are now batched as a request's are (57089 round trips).
* **`adapt` asks whether a function is expected only of a method value.**
  The question is a SAM walk over the expected type's class, and its cache
  is dropped whenever an expansion enters a class; it was asked of every
  tree.
* **Replies are parsed as bytes**, sliced rather than pushed character by
  character, and each list allocated once.
* **The implicit instance maps hash with Fx** instead of SipHash.

Together: 17.1 s → 15.9 s of type checking. JVM options for the engine
(C1 only, other collectors, a larger young generation) changed nothing or
made it slower; the engine process spends about 13 s of CPU, much of it
compiling scala-reflect and the macro implementations.

The user-defined macros found two correctness gaps on the way. A type class
with a derivation macro (`implicit def derive[T]: Show[T]` beside
`implicit def opt[T](implicit s: Show[T]): Show[Option[T]]`) was
"ambiguous implicit": a candidate whose only clause is implicit counted as
a view, and a view never beats a value on type alone
(`implicit_clause_specificity.rs`). The typed trees sent to a macro also
differed from nsc's in shape (`s.length` for `s.length()`, no `TypeApply`
for inferred type arguments, `_root_.scala.List` for
``scala.`package`.List``), which `showCode` makes visible; they now match
(`macro_tree_shape.rs`), explicit type arguments aside: nsc prints those
fully qualified, through aliases scala-rs does not keep (`scala.Predef.String`).

### Four slower-than-expected shapes, and macros again (2026-09-28)

Measured again against scalac, four non-macro shapes were well behind:

| kind | scalac | before | after |
|---|---:|---:|---:|
| `xmllits` (XML literals) | 4.7 s | 14.5 s | 2.5 s |
| `bigexpr` (a 1000-term `+` chain per method) | fails | 4.1 s | 0.5 s |
| `srcsyntax` (600 source implicit classes) | 6.0 s | 5.5 s | 2.0 s |
| `deepinherit` (`super.v` along 150 traits) | 6.4 s | 4.7 s | 1.6 s |

* `xmllits` had regressed: `Predef`'s tuple views made every view search ask
  whether the receiver is below a tuple, walking its ancestors. `Tuple1` to
  `Tuple22` are final, so only the tuple class itself is. `adapt` also asked
  the SAM question of trees that are neither functions nor methods.
* Each application of a `+` chain copied the whole chain before it to ask
  whether its receiver is `Dynamic`; the receiver is now probed in place.
* An extension search compared each source implicit class with every
  pickled candidate, and re-read `scala.AnyVal`'s (absent) pickle at every
  ancestor walk; comparisons are by name and a class without a pickle is
  recorded once.
* `drop_overridden` ordered 150 owners pairwise, each pair a walk up the
  chain; each owner's ancestors are now walked once per selection.

**Macros.** A simple macro's expansion costs about 50 µs of round trip,
which fewer round trips could save: sending the infos of settled symbols
with their descriptions cut circe's round trips from 57,000 to 31,000 and
saved no time, and batching symbol descriptions (the earlier pass) saved
0.6 s. The engine's JVM computes about half of the time it is asked and
waits for scala-rs the other half; JFR's samples under-count its compute
several times over (`sample` on the process shows it). What saved time:

* A tree scala-rs answered `c.typecheck` or `c.inferImplicitValue` with,
  returned by the implementation unchanged, is spliced back typed instead of
  typed again, as nsc leaves an attributed tree alone (circe `generic.auto`
  17.5 s -> 15.6 s; every one of a `c.typecheck` macro's 11,000 answers).
  A tree that still holds an unexpanded implicit macro is typed again, so
  the macro is expanded only if it is used.
* The engine is prestarted for a library that depends on scala-reflect, not
  only when scala-reflect is named; the first expansion then waits 0.35 s
  instead of 0.41 s. The JVM's own start-up is the rest.

### A private multi-module application (2026-09-29)

The workload is a private sbt build of 97 modules with sources (2,940 files),
each compiled on its own with the classpath sbt exports for it: 350--480
entries, most of them jars, and up to 16 directories that do not exist. Its
macros are circe's and shapeless's derivations, airframe's DI and logging.
Every module compiled one after another, fresh processes, JDK 21.0.2:

| | before | after |
|---|---:|---:|
| all 97 modules | 175.4 s | 120--122 s |
| largest module (351 files, 10,705 expansions) | 22.1 s | 15.3 s |
| its instructions | 2.20e11 | 1.52e11 |

scalac 2.13.16 takes 27.2 s wall (81.9 s user) for that module. Every
module's class files and diagnostics are byte-identical before and after.
On slick's 184 sources (`tests/bench_compare.py`, four alternating pairs,
`-Xsource:3-cross`) the medians are 1.60 s -> 1.37 s wall, with identical
output.

* The compiler settings, `-classpath` included, went with every `(expand …)`
  request: about 40 KB of a 57 KB average request, 612 MB for the module.
  They now go with an engine's first request only (`(settingsSame)` after).
* A shapeless labelled representation is a refinement at every field
  (`FieldType[K, V] :: …`), and refinements were left out of the kept wire
  spellings of context-free types. A refinement's label is fixed for the
  run, so they are kept too: -18% instructions on the largest module.
* A pickle holds every class nested in its top-level one, and each class
  asked for decoded the whole pickle again; a slick `Tables` with hundreds of
  nested row classes was decoded once per class. `SigCache` keeps each file's
  decoded signatures, and `Pickle::sym_full_name` is memoized per entry
  (a one-line use of such a `Tables` went from 1.5 s to 0.25 s).
* Package probes on an archive were a binary search over its sorted entry
  names, comparing long shared prefixes; they are a set lookup of directory
  prefixes. A directory classpath entry's package check uses the kind its
  parent's listing already recorded instead of a `stat`.
* `drop_overridden` walked each owner's ancestors once per selection; a
  wildcard import asks once per imported name, so the sets are kept while the
  class graph stands still.
* The engine's classpath starts with the Scala distribution's jars, which is
  where nsc's parent-first macro class loader finds them: loading
  scala-reflect's classes probed every entry ahead of it (about 0.2 s of
  every engine start on these classpaths; 5--7% of a mid-sized module).
* Smaller: macro positions count UTF-16 units from the previous position in
  the file, the cached-prefix search keys on a short slice of the needle,
  `quote_into` copies unescaped runs whole, the symbol table reserves room
  for half a million symbols (a symbol is 1.2 KB, and each doubling copied the
  table), and lambda bodies are no longer copied once per enclosing lambda.

Measured and not kept: mapping jars instead of reading them (3% fewer
instructions, no wall-clock change: they are in the page cache), and JVM
flags for the engine (C1 only is 7% faster on small modules, 13% slower on
the largest; a dynamic CDS archive is slower, since the classpath differs per
module).

**Later the same day**, two behaviours changed for these modules, and every
module now compiles:

* `compile` uses the macro daemon by default, as the resident batch
  compiler already did. Small modules had paid a cold JVM each -- its start
  and its first, interpreted, expansions. A daemon serves one compiler at a
  time (a busy one sends `(busy)` and the compiler starts an engine of its
  own, so parallel builds are not serialised), a poisoned session shuts the
  daemon down so a looping macro cannot hold it, and a watchdog ends a daemon
  whose macro has computed for ten minutes without a word to scala-rs.
  `SCALA_RS_MACRO_DAEMON=0` restores a JVM per compilation.
* A classpath directory that does not exist when the run starts is nothing
  for the run; it had been probed again for every new class name, 1--5% of
  each module (their build tool lists the output directories of modules not
  built yet).

With five compatibility fixes (an `Option` view reached through `Some` and
`None`, one inherited implicit named through its module, a module entered
from its header alone, a type projection through an alias parameter, named
then positional arguments choosing an overload) and Java sources compiled by
javac, all 97 modules compile. The four modules that did not compile before
are included from here on:

| | scalac | scala-rs |
|---|---:|---:|
| all 97 modules | 339.5 s | 92.5 s |
| the 64 of 12 files or fewer | 148.5 s | 32.5 s |
| largest module | 27.5 s | 13.6 s |

scalac runs with the application's own 67 options, lints included; scala-rs
with the five of them it accepts. No module is slower than with scalac. The
fixes changed 176 class files of seven modules that already compiled; in
every one the methods called and fields read are now closer to scalac's
(counted over the `javap` references), and none moved further away.

What remains is about a quarter of the largest module's time in the engine's
JVM, even warm.

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

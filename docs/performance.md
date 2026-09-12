## Speed

### 2026-09-12: recognize reflection universes through their ancestors

On the pinned Slick 184-source full compile, against commit
`1fc8f69247db3979e96884ed1f0bb344931b9277`:

| | wall | user CPU | system CPU | class files |
| --- | --- | --- | --- | --- |
| before | 10.09 s | 9.71 s | 0.31 s | 1504 |
| after | 3.55 s | 3.30 s | 0.23 s | 1504 |

Medians of four runs per binary, reversing order on alternate pairs, each
writing to a fresh directory. This is **65% less wall time (2.84x faster)**
and **66% less user CPU** for this workload. Every run produced byte-identical
class files and identical stdout/stderr after normalizing the output directory.
The environment was macOS arm64, Rust 1.98.1 release defaults, Temurin
21.0.12.1, Scala library/reflect 2.13.16, and the dependencies and Slick revision
listed in `tests/bench.sh`. These numbers are a same-machine before/after
comparison, not a comparison with the historical tables below.

`sample` over the first five seconds of the baseline found
`Typer::is_reflect_universe` in about half the compiler thread's samples.
It scanned every symbol for the two Universe JVM names, then walked the
receiver's ancestors separately for each matching symbol. This predicate is
also called on ordinary imported receivers, so unrelated symbols made even
negative answers expensive.

The predicate now walks the receiver's ancestors once and checks their JVM
names directly. `pickle_supply::inherits_matching` shares the original
`inherits_from` traversal, including parent resolution, traversal order,
cycle handling, and the 256-node guard (including its original boundary
behavior). No answers are cached: lazy parent completion and JVM-name changes
remain visible. All matching ancestors count, including duplicate JVM names;
looking up only the first symbol in the existing JVM-name index would not
preserve that property.

The three `universe_tests` cover direct and inherited universes, duplicate
binary names, unrelated names, late parent/name changes, and cycles. The
comparison runner checks successful compilation, nonempty output, diagnostics,
and SHA-256 hashes of every class file on every timed run:

```sh
python3 tests/bench_compare.py /path/to/before /path/to/after \
  --sources-file /path/to/files.txt \
  --scala-library /path/to/scala-library-2.13.16.jar \
  --classpath "$(cat /path/to/deps.cp)" \
  --compiler-arg=-Xsource:3 --reps 4
```

The source manifest contains one path per line. Use `tests/bench.sh`'s fixed
file list, including the seven expanded FreeMarker templates. The runner does
not download dependencies or build either binary. It prints per-run timings
and medians as JSON lines, and fails on differing output or a failed compile
instead of reporting that as a speedup.

Validation:

```sh
cargo test --release --offline --workspace --lib --no-fail-fast
cargo test --release --offline -p scala-rs-cli \
  --test quasi --test reify --test reify2 --test rf_reify \
  --test linearization --test implicitmemo --no-fail-fast
```

All **350 library tests** passed with JDK 17, and all **58 integration tests**
passed with JDK 21 and Scala 2.13.16 available, including JVM execution and
scalac comparisons. The library suite initially passed 349/350 on JDK 21:
`names_match_released_scala_for_bmp_and_escape_sequences` compares the checked-in
JDK 17 / Unicode 13 name table with the running JVM, so JDK 21's newer Unicode
classification disagrees at U+0870. Re-running with its reference JDK 17
passed; no name-encoding code was changed.

### Follow-up: three isolated experiments

Each candidate below was built separately on top of the Universe change,
using the same Slick workload and output-checking runner. User CPU medians
are from four runs per binary; compare each candidate only with its own
paired baseline, since machine load varies between experiments.

| candidate | baseline user CPU | candidate user CPU | reduction | decision |
| --- | --- | --- | --- | --- |
| borrow ordinary parent types in `parents_of` and `base_type_args` | 3.358 s | 3.311 s | 1.4% | keep |
| cache complete root linearizations during backend emission | 3.392 s | 3.365 s | 0.8% | discard |
| reuse the resolved implicit candidate signature during conversion inference | 3.359 s | 3.327 s | 0.9% | keep |

The parent-type change removes two unconditional deep copies. Structural
function parents still use `function_class_form`; all other parents are
borrowed through the same class lookup and substitution as before.

The backend experiment reused a class's complete linearization across 17
mixin/initializer call sites in one `Gen`. Its lifetime was restricted to an
immutable symbol-table borrow, so no invalidation was needed. Its small
end-to-end saving did not justify the new cache state and call-site changes;
the backend remains unchanged. This does not rule out a larger benefit from
reusing linearizations in the typer, where safely delimiting mutations would
require a separate design.

For implicit conversions, `conv_targs` already resolves the candidate's type
at its import/owner prefix. `solve_conv_targs_from_implicits` now borrows that
snapshot instead of resolving it again; the intervening base-type lookup and
structural unification perform no implicit searches. Non-polymorphic
conversions return an empty type-argument list immediately, while callers
continue to check their implicit clauses. Candidate selection, applicability,
search bounds, and witness resolution are unchanged, and no new cache is
introduced.

With the two retained changes together, eight alternating pairs against the
Universe-only binary measured **3.630 s → 3.542 s wall (−2.4%)**, and
**3.370 s → 3.276 s user CPU (−2.8%)**, at the medians. System CPU was
0.234 s → 0.232 s. Every run passed the class-byte and diagnostic comparison
(1504 classes). This is the incremental saving over the previous optimization;
the different absolute timings between sections are why each comparison uses
back-to-back runs of both binaries.

The retained combination passed all **350 library tests** and **73 targeted
integration tests**, using JDK 17 and Scala 2.13.16. The integration selection
covers base-type meets, function parents, inherited members, implicit
conversion inference (including dependent and bounded arguments), required
witnesses, local/by-name conversions, and JVM execution in both runtime modes:

```sh
cargo test --release --offline --workspace --lib --no-fail-fast
cargo test --release --offline -p scala-rs-cli \
  --test conversion_inference --test convimpl --test implicitmemo \
  --test implicit_misc --test localconv --test btmeet --test bparent \
  --test linearization --test traitextends --test ifacebridge --test javanest \
  --test function_pattern --test byname_followup --no-fail-fast
```

### Reuse import-prefix resolution within one immutable search

Eight alternating pairs against the preceding combined version, on the same
Slick 184-source workload, measured:

| | wall | user CPU | system CPU |
| --- | --- | --- | --- |
| previous combined version | 3.574 s | 3.306 s | 0.242 s |
| scoped import-prefix lookup | 3.057 s | 2.800 s | 0.230 s |

These medians are an additional **14.5% wall-time reduction** and **15.3%
user-CPU reduction**. Every run produced the same 1504 class files, identical
byte for byte, with identical diagnostics. This uses `bench_compare.py` with
`--reps 8`, fresh output directories, JDK 21 and the same dependencies as the
earlier timing runs. No test suite or build ran alongside this comparison.

The next profile of the combined binary pointed at
`implicit_candidate_ty` → `at_import_prefix_of`. A substantial part of that
work precedes substitution: `term_import_prefix_for` asks whether the current
or enclosing class already inherits the member's owner, then scans remembered
imports in reverse order, walking their ancestors and checking whether their
qualifiers are still writable. The list spans compilation units. Several
candidates with the same owner, and repeated reads of one candidate during
inference, were paying for the same positive or negative lookup repeatedly.

`ImportPrefixMemo` caches that lookup by owner only for the lifetime of an
immutable implicit search or conversion-result calculation. Nested operations
share the map; the last guard clears it. The guard holds `&Typer`, so callers
must drop it before changing scopes, imports, enclosing classes or parents.
This also means `search_extension` cannot carry it across Java/pickle loading.
It is separate from `ImplicitMemo`'s companion-prefix and route bookkeeping,
and does not extend the lifetime of those search results.

An immutable symbol table is not sufficient by itself: class lookup can see
an already-open type expansion and truncate an alias or bound. Prefix lookups
under any ambient bound/alias/chase, written-type, subtype-walk or qualified
type-parameter context bypass the cache entirely. Standalone lookups still
execute the original lookup order and return owned qualifier trees, so neither
the chosen import nor ownership of the returned tree changes.

Three new tests exercise late parent completion, disappearance/shadowing of
an import root between searches, and both cache-hit and cache-miss behavior
under an ambient bound chase.

All **353 library tests** and **126 integration tests** passed with JDK 17
and Scala 2.13.16. The integration selection includes imports, lexical
contexts, path-dependent types, implicit conversions, quasiquotes and reify,
including reference-scalac comparisons and JVM execution:

```sh
cargo test --release --offline --workspace --lib --no-fail-fast
cargo test --release --offline -p scala-rs-cli \
  --test imports --test quasi --test reify --test reify2 --test rf_reify \
  --test lexicalcontext --test contextualinference --test nameamb --test pfx \
  --test pathdep --test implicitmemo --test implicit_misc \
  --test conversion_inference --test convimpl --test localconv \
  --test bparent --test btmeet --test byname_followup --no-fail-fast
```

### Historical measurements

The following tables and pass descriptions record earlier compiler versions.

What is measured is slick's 184 files (the file list `tests/bench.sh` pins),
with `-Xsource:3`, and scala-library 2.13.16 + slick's 12 dependency jars +
scala-reflect on the classpath: a **full compile** (type checking → erasure →
code generation → writing the class files).

|                                            | wall time | CPU time (`user`) | class files |
| ------------------------------------------ | --------- | ----------------- | ----------- |
| nsc (scalac 2.13.16, including JVM startup) | 12.0 s    | 68.6 s            | 1498        |
| scala-rs, before any optimisation           | 217.3 s   | 209.6 s           | 4552        |
| scala-rs, after the first pass              | 3.5 s     | 3.0 s             | 4552        |
| scala-rs, after the second pass and indy    | 2.0 s     | 1.8 s             | 2127        |
| scala-rs, after the class-file writing pass | 1.8 s     | 1.6 s             | 1596        |
| scala-rs, after the redone-work pass        | **1.5 s** | **1.3 s**         | 1596        |

Medians of three runs each, alternating between the two compilers so both see
the same machine; the last two rows are medians of eight alternating runs taken
back to back on a quiet machine (`user` 1.61 s → 1.33 s, `sys` 0.21 s in both).
The last row is the current state; the earlier rows are kept because the
optimisation passes are described below and the numbers are what each one
moved.

The class-file counts in the middle rows are what those passes measured. The
count today is **1596**, not the 2127 the `invokedynamic` row records; the
paragraphs below that quote 2127 are left as the record of what was measured
at the time.

The CPU column is `user` only, which is the right comparison for the first two
passes because they were arithmetic. It hides the third: writing the class
files spends its CPU in the kernel, and that pass took **`sys` from 0.83 s to
0.22 s**. Total CPU on the last row is 1.89 s against the previous 2.50 s.

That is **136x less CPU than where it started**, and against nsc **8x faster in
wall time, 45x in CPU**. nsc's wall time is carried by several threads; the
compile in scala-rs is still **entirely single-threaded**, and only writing the
class files is parallel.

Peak resident set is **566 MB** (`/usr/bin/time -l`, `maximum resident set
size`), of which 516 MB is `peak memory footprint`. An earlier note put it at
1.4 GB; that is not what this binary does.

The class file counts still differ. scala-rs lowers plain `FunctionN` literals
to `invokedynamic` as nsc does, but `PartialFunction` literals remain anonymous
classes. So scala-rs reaches this time while writing more class files than nsc.
(The other half of the old gap, one `T$class` helper per trait with a concrete
member, is gone: `agent/traitclass` moved trait bodies onto the interface, as
nsc does.)

nsc reports 3 Scala 3 migration errors under `-Xsource:3`, so the runs above
silence them with `-Wconf:cat=scala3-migration:s` (scala-rs does not implement
that migration check and passes the sources straight through).

### Where the time went: the first pass

Four of the seven roots were quadratic in files times symbols.

| change | CPU |
| --- | --- |
| baseline | 213.1 s |
| parse the jar's central directory once, not per class lookup | 13.1 s |
| a reverse index for `find_by_jvm` instead of a linear walk | 11.8 s |
| borrow instead of cloning `Vec<Type>` in subtyping; cache `find_class` | 10.6 s |
| stop erasure's all-symbol sweep at a fixpoint (184 passes → 2) | 5.5 s |
| test before taking in uncurry's `flatten_one_method` | 4.4 s |
| share `trait_members` / `pickles` through `Rc` | 3.1 s |
| build the emitter's JVM-name index once; `mkdir` once per directory | 3.11 s |
| borrow the names in the implicit-scope walk | 3.07 s |

The first line was 94% of the whole problem: every single class lookup rebuilt
the central directory of every jar on the classpath — around ten thousand
entries, times two candidate names, times fourteen jars.

### Where the time went: the second pass

| change | CPU |
| --- | --- |
| mimalloc as the global allocator | −18% |
| `rustc-hash` for the typer's internal maps and the constant pool | −6% |
| `implicit_candidate_ty` returns `Cow<Type>` instead of a deep clone | −11% |
| capacity hints and borrows in `Scope::entries` / `implicits_in_scope` | −7% |
| write the class files on eight threads | −5% wall, no CPU change |

Two things about this pass are worth keeping in mind.

The profile said 42% of the time was in malloc and free. That was read as "the
`Type` tree is cloned too much", and it was half wrong: most of it was macOS's
own allocator, and swapping in mimalloc took the whole category from 42% to 8%.
The clone that did matter was not the tree structure but one function deep-
cloning a declaration for every implicit candidate it only wanted to read.

The first pass had rejected a fast hasher because "changing `HashMap` iteration
order is risky". That reasoning was wrong: `std`'s `RandomState` is seeded per
process, so no output can ever have depended on a particular iteration order —
if it did, the compiler would produce different results on consecutive runs. A
fixed hasher only makes the order reproducible.

Measured and discarded: thin LTO with `codegen-units=1` (within noise, and the
build went from 14 s to 46 s), and a `Cow` fast path in `subst_tparams_slice`
(the types on that path really do mention type parameters).

### Where the time went: the redone-work pass

Four more things were being redone once per compilation unit that are a
function of the whole run, and two parent-DAG walks were re-deriving an answer
that needs no type arguments.

| change | insns |
| --- | --- |
| borrow the parent lists instead of cloning them; skip an identity substitution | −1.6% |
| `flatten_method_symbols` starts where the last call stopped | −1.3% |
| `is_sub_type`: is the target class up there at all? | −4.3% |
| `collect_boxed_vars` and `find_class_named` once per run, not per unit | −1.4% |
| `base_type_instance`: the same reachability question | −3.1% |
| `find_overridden_method` walks symbols, not cloned parent types | −1.5% |
| read `SCALA_RS_*_DEBUG` once; don't build a type to answer a predicate | −0.4% |

Measured end to end against `main`, eight runs each alternating the two
binaries on a quiet machine: **23.05 G instructions → 20.02 G (−13.2%)**,
**1.75 s → 1.47 s wall (−16%)**, **1.61 s → 1.33 s `user` (−17%)**, `sys`
unchanged at 0.21 s. Every class file slick produces is byte-identical to the
old binary's (`diff -r` over both output trees) and the diagnostics are the
same text.

**The first pass's headline was still true a year later.** Four of its seven
roots were "quadratic in files × symbols", and four more of exactly that shape
were still here:

- `uncurry` swept every symbol looking for a method with more than one
  parameter list — 184 passes over ~100k symbols, one random read of a large
  `Symbol` each. It is safe to resume from a mark: the driver types *every*
  unit before it lowers any of them, so the lazy class-file loading that
  installs a curried signature has finished before the first sweep, and no
  later phase writes more than one parameter list (`lambda_lift` splices its
  captures into `paramss[0]`, `lazy_local` writes `vec![vec![cell]]`).
- `collect_boxed_vars` read every symbol's `captures` once per unit, and
  `find_class_named` (the case-class companion in `emit_module`) was a linear
  search of the symbol table once per module. Both are pure functions of the
  frozen table, so the driver builds them once and hands them to each unit —
  the same treatment `trait_members` and the JVM-name index already had.
- `find_overridden_method` runs for every method symbol during erasure and
  cloned each node's parent list — a deep copy of every type in it — to walk
  it. It only ever asks a parent for its class, so the worklist holds symbols.

**The subtype walk was answering a harder question than it was asked.**
`is_sub_type` and `base_type_instance` walk the parent DAG with the type
arguments substituted at every edge and with no visited set, so a diamond is
re-entered once per path and a *miss* costs the whole graph — and a miss is
what implicit search asks for, over and over. Whether one class is under
another needs no type arguments at all, so `SymbolTable::class_reaches` answers
it first by walking symbols with a visited set, linearly.

It is deliberately an over-approximation of what the real walk visits, so it
can only ever say "run the real walk". `Some(false)` — the promise that the
walk cannot succeed — is returned only when every parent in the closure is an
ordinary class or `AnyRef` / `Any` / `AnyVal`, which are the two shapes whose
behaviour it models exactly. Anything else (a `FunctionN` in class clothing,
which `is_sub_type` rewrites to the structural function type; a refinement; an
abstract type; a module or singleton parent) answers `None` and nothing is
concluded. That is what makes it safe against a symbol table that is still
being filled in: it reads the same `parents` the walk itself would read, at the
same moment, and caches nothing.

Two smaller constants, both worth remembering as a species:

- `trace` asked `var_os("SCALA_RS_PICKLE_DEBUG")` on *every call*, from the
  middle of member completion; the lambda lowering did the same for
  `SCALA_RS_LAMBDA_TRACE` on every lambda. `var_os` walks the process
  environment. Both read once into a `OnceLock` now.
- `function_class_shape` is asked "is this class a `FunctionN`?" at every node
  of every parent walk, and it answered by *building* the structural function
  type — a `Vec` and a `Box` — which the caller threw away.

**Measured and discarded.** Nothing was reverted this pass; four ideas were
costed and turned down, which is worth as much as the ones that landed.

- **Restricting `pickle_all` to the classes the run emits.** It pickles every
  `Class`/`ModuleClass` in the table before erasure, and only ~1600 of them are
  emitted -- but the table holds just **2855** classes for slick's 113,959
  symbols, so the waste is bounded at 45% of a 7% phase. Against that,
  `attach_scala_sig` *falls back* to pickling at emit time for any class the
  map lacks, which is after erasure: a class the filter missed would silently
  get an erased signature rather than fail. Bad trade.
- **Memoising `is_sub_type` or `implicits_in_scope`.** Both need an
  invalidation epoch, and `parents` alone is written from more than fifty
  places; a missed one is a wrong answer, not a slow one. `class_reaches`
  exists precisely because it needs no cache.
- **Hoisting the type snapshot out of `warm_implicit_candidates`** (4%: it
  deep-clones the candidate type of every implicit in scope). The loop body
  mutates the symbol table, and what the *next* iteration would see is
  deliberately the pre-loop snapshot. Every way of avoiding the copy also
  changes which table the later iterations read.
- **Shrinking `Type` from 56 bytes** by boxing `Named` / `Refined` /
  `Constant`. This is the one worth doing: `memmove`, the allocator and
  `Type::clone` are together still ~15%. It reaches every `match` in the
  compiler, so it wants a slice of its own.

### What is left

From a profile of the current binary (`sample`, per-thread, reading the call
graph rather than the self-time summary):

| phase | share |
| --- | --- |
| type checking | 58% |
| code generation (`gen`) | 17% |
| erasure | 10% |
| pickling | 7% |
| everything else (uncurry, lambda-lift, parsing, drops) | 8% |

- **`type_select` is a third of the whole compile**, and `search_extension`
  — the implicit-conversion search a selection falls back on when the name is
  not a member — is 14% of it. For each candidate conversion in scope it runs
  `conversion_result`, which unifies and then searches for the conversion's
  own implicit arguments. Pruning candidates by "could this conversion's result
  even have a member of this name?" is the obvious idea and it is not
  obviously safe: the member may exist only in the result class's pickle, and
  asking for that is the expensive, mutating call the prune was meant to avoid.
- `Type::clone`, its drop glue, `memmove` and the allocator are together about
  15%, spread over everything. The fix is interning — a `TypeId` index, or
  `Rc` for shared subtrees — or simply making `Type` smaller than 56 bytes.
  Either reaches every part of the compiler that touches a type.
- ~~`implicits_in_scope` runs on every implicit search and rebuilds a set of
  every name in every enclosing scope (4%).~~ **Done** — see *Memoizing
  implicit search* below. The key it needed turned out to be no key at all:
  the whole search runs under `&self`, so nothing it reads can change while it
  is in flight, and the answer is cached for the length of one implicit
  operation. (Class-file loading does add members mid-typing, but only from
  `&mut self`, which is exactly what cannot happen inside a search.)
- Writing the class files is **7% of wall time**, not the 45% an earlier
  reading of `sample` claimed — see *Writing the class files* below for why
  those two numbers are not the same measurement. 2127 files is 2127 creates.
  579 of them are closure classes scalac does not emit: every one implements
  `scala.PartialFunction`, and scalac has 137 such classes to our 716. scalac
  only builds a `PartialFunction` class when the expected type really is one;
  a `{ case … }` passed where a `Function1` is wanted becomes an ordinary
  `invokedynamic` lambda whose body is the match. So the count is a typing
  question, not a code-generation one. (A further 106 were `T$class` helpers,
  which are gone: trait bodies are interface default methods now, as in nsc.)
- The compile is single-threaded. Parsing is trivially parallel; the typer
  shares a mutable symbol table and is not.

### Memoizing implicit search

`ImplicitMemo` in `crates/typer/src/implicits.rs`. It answers
`search_implicit_undet` from a table keyed by the wanted type, the
undetermined call-site parameters and the depth, and caches two other things
along the way: `implicits_in_scope` and `strictly_more_specific` (nsc's
`improvesCache`).

**What made a key possible.** Every earlier attempt foundered on "the answer
depends on the context, and the context is the whole scope stack". It does —
but the whole search runs under `&self`, so none of it can change while a
search is in flight. The memo is created when the outermost implicit operation
starts and thrown away when it returns, which turns the context into a constant
instead of a key. The borrow checker is what enforces this: a caller that needs
`&mut self` cannot hold the memo alive, and `warm_implicit_candidates` — the
one thing that really does add symbols mid-typing — is `&mut self`.

Three pieces of state a search reads or writes are not covered by that
argument, and each is handled separately:

* `open_implicits` (nsc's `openImplicits`, the divergence cut-off) **is**
  mutable during a search. An entry carries a 64-bit signature of the
  candidates its subtree asked the divergence check about, and is stored and
  reused only where that signature is disjoint from the open stack's. A
  collision costs a miss, never a wrong answer.
* `MAX_IMPLICIT_DEPTH` makes `depth` matter, but only where the limit actually
  bit. An entry records whether it did; one that never reached the limit is
  reused at any *shallower* depth, which is what stops the same wanted type
  being re-derived once per depth it is reached at.
* Both of those travel **upwards through cache hits**. A search that took an
  answer from the memo has that answer's subtree in its own, so a hit ORs the
  entry's signature and depth-limit flag into the search that read it.
  Forgetting this is the subtle way to get it wrong: the parent would be
  recorded as depending on neither and then reused where it does not hold.

`diverged_implicit` keeps only the *first* divergence and is monotone
inside one top-level search. `implicit_via_module` needs explicit replay:
two companions can inherit the same member symbol, and discovering the second
companion overwrites the route recorded for the first. Each memo entry stores
the final route writes of its subtree and replays them on a hit, including
writes made while examining candidates for a failed search. Those writes also
propagate into an enclosing entry, just like divergence and depth dependence.

The resumption review reproduced the missing replay with a focused regression:
searching companion A, then B, then A again returned A's witness with B's
receiver route. The cached and uncached searches must agree about both the
witness and this code-generation metadata. `memo_replays_inherited_companion_route`
checks found and failed entries; the existing `im_memo` fixture still compares
executed output with real scalac.

**What it is worth.** On the four compile measures as they stand, nothing
measurable: today's implicit scope is small enough that the searches are
shallow. The reason to have it is the family it unblocks. Guarding
`Typer::import_wildcard` with `PickleSupply::pickle_readable` — one line, and
the fix for gitbucket's largest remaining family — multiplies the number of
candidates in scope, and with that guard applied:

* the memo answers **99.3%** of all `search_implicit_undet` calls (counted over
  gitbucket's 213 hand-written sources: 3.98M calls, 3.98M hits, 18k entries
  refused by the divergence signature);
* `most_specific` was **73% of one ten-second profile** taken inside the
  deepest search, all of it in `is_as_specific_type`, and it does not appear in
  the profile at all once `strictly_more_specific` is cached. (Two `sample`
  runs at different points of a long compile are not the same measurement —
  this says the quadratic comparison is gone from that search, not that the
  run is 73% shorter.)

**And what it is not enough for.** That guard is still not affordable, and the
bisection is sharp: with the guard, gitbucket's first **190** hand-written
sources compile in 6 s, and adding the 191st — `util/DatabaseConfig.scala` —
takes it past 400 s. That is true of both the memoized and the unmemoized
compiler, so the memo does not reach this.

What is left is not repeated searches — it is the ~0.7% that miss, each of
which fits *every* candidate in scope to the wanted type before it can answer.
The profile under the guard is `implicit_fit_open`'s own substitution work
(`subst_tparams_slice`, `type_mentions_tparam`, `Type::clone` and its drop
glue) at those misses. nsc has a cheap structural pre-test
(`isPlausiblyCompatible`) that rejects most candidates before unification is
attempted, and that — not more caching — is what the next slice on this needs.
See `docs/gitbucket.md`.

**That slice ran, and the pre-test was not the answer either.**
`Typer::plausibly_inhabits` is now in the tree: two `Type::Class` heads that
cannot reach each other reject the candidate before `Unify` touches it, and the
"no" is sound because substitution replaces `TypeParam` *leaves* and so leaves
both head symbols exactly where the final `is_sub_type` will find them. It
rejects **58% of all fits** under the guard (11.5M of 20M on the 191-file
reproduction) and, on its own, that run still did not finish in 600 s — because
the deep tree is made entirely of *same-head* nodes (`Shape` against `Shape`),
which no test on head symbols can cut.

What cut it was an applicability rule, not a filter: a candidate parameter that
unification "solved" only to a **wildcard** was being counted as pinned down,
so slick's 22 `tupleNShape` rules all looked applicable to a wanted
`Shape[_ <: L, ?M, ?U, ?P]` and each searched all of its own `Shape` clauses at
every level down to `MAX_IMPLICIT_DEPTH`. Discounting a wildcard solution took
the reproduction from **over 600 s to 14.4 s** and the whole 353-file measure
to 6.3 s — with the guard on. Both changes are diagnostic-neutral on all three
measures. The guard is still unmerged, now for a diagnostic reason
(`Session` / `JdbcBackend#SessionDef`) rather than a timing one;
`docs/gitbucket.md` has the thirteen-line reproduction.

The general lesson is worth keeping: **the profile named the work, and cutting
the work named in the profile was not the same as cutting the tree that
generated it.** `sample` showed `subst_tparams_slice` and `Type::clone`, and
removing 58% of those calls changed the wall clock by nothing at all, because
the surviving 42% were the recursive ones. A histogram of *which candidate at
which depth* — 102k entries apiece for every one of ~30 `Shape` witnesses at
depth 7 — is what pointed at the rule that admitted them.

### Writing the class files

Two changes, both in `crates/driver/src/lib.rs`. slick, 184 files, 2127 class
files, medians of 16 runs alternating the two binaries so both see the same
load:

| | wall | user | sys | CPU (user+sys) |
| --- | --- | --- | --- | --- |
| eight writer threads, all writes after the last unit | 1.87 s | 1.67 s | 0.83 s | 2.50 s |
| four writer threads, overlapped with code generation | **1.78 s** | 1.67 s | **0.22 s** | **1.89 s** |

−5% wall and **−24% CPU**, with the class files byte-for-byte identical
(`diff -r` over both output trees).

**First: the profile did not say what it was read as saying.** The claim was
"`open` is 45% of the non-waiting samples, so writing dominates". `sample`
counts *thread* time, and a thread blocked in `open` is sampled exactly like a
thread doing arithmetic. Adding up eight blocked writer threads and comparing
that total against one working main thread inflates I/O by the width of the
pool. Read `sample`'s per-thread totals instead: the writer threads are alive
for **86 ms of a 1231 ms process**, and timing `write_emitted` directly agreed
(65–100 ms). Writing was 7% of the compile, and 45% was never available to win.

**Second: more threads made it worse, not better.** "It is I/O bound, so add
threads" is the wrong model for creating files. `write_emitted` timed on its
own, slick's 2127 files, fresh output directory, APFS:

| threads | 1 | 2 | 3 | 4 | 5 | 6 | 8 | 12 | 16 | 24 | 32 |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| ms | 110 | 85 | 60 | **55** | 62 | 68 | 95 | 180 | 190 | 195 | 200 |

Creating a file takes an exclusive lock on its directory, and these 2127 files
land in 19 directories — 716 of them in one. Past about four threads they queue
on each other, and the queueing is kernel CPU: `sys` was 0.83 s at eight
threads and 0.28 s at four, for the same syscalls. That single constant is
most of the CPU saving above.

Overwriting an existing file is cheaper than creating one (65 ms against 90 ms
for the same 2127 files), so a repeated build into the same `-d` is measuring
something slightly different from a first build.

**Third: the writes did not have to be at the end.** They are now streamed —
each unit's classes go to the pool as soon as `emit_opts` returns
(`ClassWriter`), so the file system latency overlaps with the code generation
that follows instead of being appended to it. Each chunk is *moved* to a writer
thread and moved back when written, so nothing is copied and nothing is shared;
`compile_paths` still returns every class, in emit order. This is the −4% wall
that is left once the thread count is fixed. Runs under 64 classes write on the
calling thread — starting a pool costs more than it saves.

Measured and discarded: **streaming does not address the 1.4 GB peak**. All
2127 class files together are 9.2 MB. Whatever the peak is, it is not the
emitted bytes being held.

Not attempted, and worth knowing before someone tries: `openat` against a
cached directory fd would save resolving the parent path per file, but path
resolution is not what the writers are blocked on — the directory lock is.

### How to reproduce

```bash
tests/bench.sh            # full compile, twice; reports real and user
tests/bench.sh --parse    # parse only
REPS=3 tests/bench.sh     # change the repeat count
```

**Wall time swings wildly** when other jobs are running on the same machine.
Compare `user` (CPU time) across commits. Even that moves by 20–30% through
contention for memory bandwidth, so **always take the before and after
measurements back to back** (under the same load).

### Measuring on a loaded machine

This machine usually has several agents on it. The same binary produced 1.87 s
and 2.78 s of `user` inside one minute, so a 2% change is invisible in `user`
and a 6% change is a coin flip. Two things make it readable.

**`/usr/bin/time -l` reports `instructions retired`** on Apple silicon, and it
is stable to about 1% across runs regardless of load — the counter does not
tick while the process is descheduled or stalled on someone else's memory
traffic. It is the right metric for deciding whether a change did anything:

```
/usr/bin/time -l ./scala-rs compile … 2>&1 | grep 'instructions retired'
```

**Instructions understate a cache-miss win.** Two of the changes above delete
strided reads over a large `Vec<Symbol>`: `flatten_method_symbols` moved
−1.3% of instructions but about −8% of CPU time, and `collect_boxed_vars`
−0.6% against a clear win in `sample`. When a change removes memory traffic
rather than arithmetic, take instructions as a *lower bound* and confirm with
CPU time on a quiet moment, or with `cycles elapsed` from the same output.

**Report the minimum, not the median, when the load is high.** Contention only
ever makes a run slower, so over enough alternating pairs the fastest run of
each binary is the least-disturbed estimate of both. The two agree when the
machine is quiet: at load 8 the redone-work pass measured −15% CPU by median
and −16% by minimum; at load 26, the same pair of binaries gave −22% by median
and −3% by minimum, both from noise.

Two binaries, alternating, is still the only way to compare: build the old one
into a scratch tree (`git archive main | tar -x -C <dir>` and `cargo build
--release` there — a git worktree is not available to a worktree-isolated
agent) rather than measuring one commit and then the other.

### The first pass in detail

Profiles were taken with macOS `sample` (`sample <pid> <seconds> -f out.txt`).
Read the **call graph itself** (the tree at the top), not just the "Sort by top
of stack" (self time) summary at the end of `sample`'s output: the per-phase
breakdown appears only in the call graph.

Seven roots. Every one of them was work being redone from scratch, and **four
were quadratic in "number of files × number of symbols"**.

1. **The jar's central directory was re-read for every single class lookup**
   (`javaclass.rs`). `ZipArchive::new` builds the entry table for the whole
   archive. That is about 10,000 entries for scala-library alone, with 15 jars
   on the classpath and 2 candidate names per lookup. This was **94% of the
   entire compile**. Fixed by opening each jar once and holding it (221.8 →
   13.8 s). While there, `find_class` now remembers its answers, including the
   misses — a lookup that finds nothing scans every jar and jmod, so it is the
   most expensive kind.
2. **Erasure re-walked the whole symbol table once per source file**
   (`erasure.rs`): 184 times over roughly 100,000 symbols. **55% of the
   total.** It is a fixpoint iteration, so the pass after one that rewrote
   nothing is guaranteed to rewrite nothing (`SymbolTable::erasure_settled`).
   On slick it converges in effectively 2 rounds and the remaining 182 return
   immediately (10.8 → 6.0 s).
3. **Uncurry had the same shape** (`flatten_method_symbols`) — and it cloned
   `paramss` and the whole method type before even making the decision (6.0 →
   4.9 s).
4. **The "whole-run maps" handed to code generation were deep-copied per file**
   (`EmitOpts::trait_members` at 9%, `pickles` at 3%). They are shared through
   `Rc` now (4.9 → 3.5 s).
5. **`classpath::find_by_jvm` was a linear scan of the symbol table.** There is
   now a reverse index from `jvm_name` (`SymbolTable::find_class_by_jvm`).
6. **`&self` methods cloned `Vec<Type>`s they only read** (the parent-DAG walk
   in `is_sub_type`, `base_type_instance`, `subst_tparams`,
   `implicits_in_scope`). They just borrow now.
7. **Code generation rebuilt `build_jvm_index`** (the JVM-name index over the
   whole symbol table) **once per file**, and `write_emitted` called
   `create_dir_all` once per class file.

### Measurement pitfalls (walked into, all of them)

**`--typer` does not stop after type checking.** It is a flag that dumps the
typed tree; the compile runs to the end regardless. Both "time under `--typer`
= time spent type checking" and "full minus `--typer` = time spent generating
code" are **wrong**. The actual breakdown (after optimisation, from `sample`'s
call graph) is

| phase                                                 | share |
| ----------------------------------------------------- | ----- |
| type checking                                         | 53%   |
| code generation (`gen`)                               | 22%   |
| writing class files (one `open`/`write`/`close` each)  | 11%   |
| erasure / uncurry / pickle                            | 9%    |

and parsing is 0.05 s (1.5% of the total). That breakdown is from the
first pass; the second pass and `invokedynamic` have since moved the balance
towards writing files (see **What is left** above for the current one).

**`__psynch_mutexwait` in the writer threads is not contention.** It is three
quarters of their samples, and it is the pool's shared `Mutex<Receiver>` being
held across a *blocking* `recv`: three writers wait on the mutex while the
fourth waits on the channel. All four are asleep. Only about 8% of their
samples are in `open` / `write` / `close`, which is the number that matters.
This is the same trap as the 45% reading described above — a blocked thread is
sampled exactly like a working one.

**Recursive functions break naive inclusive-time arithmetic.** `sample`'s
tree prints a recursive call under itself, so adding up every occurrence of
`is_sub_type` or `base_type_instance` counted the same samples once per stack
frame — 92% and 26% of a thread that only spent 11% and 5% in them. Count a
symbol once per root-to-leaf path (skip it when it is already on the path from
the root), or read the *entry points* into it from outside instead.

## gitbucket's measure was *not* macro-bound (2026-09-12, `agent/macroperf`)

`tests/gitbucket_measure.sh` took 13 s before Slick's `mapTo` could expand and
**154 s** after (`agent/gbmacro`; 142 s when that slice measured it). The
obvious reading -- 31 new macro expansions, so about 4 s each inside the engine
-- was wrong, and believing it would have optimised the wrong process. It is
**20 s** now, and the macro bridge was a tenth of what was removed.

### What the measurement actually said

Two instruments, and they agreed.

**`sample` on the compiler, for the whole run.** The main thread does nothing
(it joins the worker), and reading the call graph of the *worker* thread -- each
symbol counted once per root-to-leaf path, per the warning above -- gave, out of
135 s:

| inclusive                                               | seconds |
| ------------------------------------------------------- | ------- |
| `Typer::expand_macro_application` (the whole macro path) | **2.7** |
| `Typer::search_extension` → implicit search              | 110     |
| `Typer::at_import_prefix_of`                             | 77.7    |
| `Typer::most_specific`                                   | 45      |

Waiting for the engine was 1.3 s of the window; what looked like "blocked on the
JVM" in a first reading was the *stderr collector* thread, which sits in `read`
for the whole run by design. A blocked thread is sampled exactly like a working
one -- the same trap as the writer pool above.

**Counters, for one run.** `linearize` was called **44,539,771** times (812
million `Lin::lin` node visits) and `base_type_args` **44,390,698** times,
against 405,801 mutations of the symbol table in the whole compile. Both are
pure functions of the symbol graph, and both were recomputed from scratch every
time: `Lin`'s memo was built and thrown away once per call.

### The four changes, each measured on its own

All four are caches or memoisations of deterministic lookups. None changes what
the compiler accepts or emits: `tests/bench_compare.py` compiles slick and cats
with both binaries, alternating, and reports `identical_output: true` (slick
1504 class files, cats 2976, byte for byte, with identical diagnostics), and
gitbucket's error set is the same line for line.

**Report instructions, not seconds, for the headline.** This machine carries
several agents; the same pair of binaries on gitbucket gave 154 s/20 s on a
quiet moment and 366 s/43 s under load an hour later, while *instructions
retired* varied by 0.06% between runs. Driving the compiler directly (not
through `tests/gitbucket_measure.sh`, whose `/usr/bin/time -l` would count only
the shell):

| gitbucket, 354 sources | instructions retired | wall, quiet machine |
| ---------------------- | -------------------- | ------------------- |
| base (`batch/w3`)      | 2,566,780,000,000    | 154 s |
| after                  | **286,490,000,000**  | **20 s** |

which is **9.0x fewer instructions**, with the same one error reported at the
same place.

The per-step figures below are wall-clock readings taken as each change landed,
so they are good for the *ordering* and not for three digits:

| after                                                     | gitbucket | what it removed |
| --------------------------------------------------------- | --------- | --------------- |
| base (`batch/w3`)                                          | 154 s     | -- |
| linearization cache (`crates/typer/src/lin.rs`)            | 120 s     | 812 M `lin` node visits → 400 K; 99.9% hit rate |
| `base_type_args` cache (`symbol.rs`)                       | 69 s      | 44.4 M walks → 44 K; 99.9% hit rate |
| `implicit_candidate_ty` memo (`implicits.rs`)              | 34 s      | `most_specific`'s O(n²) re-substitution of candidate types |
| engine-side reflection memo + one reused pipe placeholder  | **20 s**  | macro expansion 8.5 s → 0.4 s |

1. **Linearization was recomputed per caller.** `Lin`'s memo lived for one
   `linearize` call, so a diamond was re-expanded once per caller. It is now
   also kept on the symbol table (`SymbolTable::lin_cache`), keyed by a
   **mutation generation** that `SymbolTable::get_mut` bumps: an entry is valid
   until *any* symbol is handed out for mutation, at which point the whole cache
   is dropped rather than reasoned about. 405 K mutations against 44.5 M
   queries, and the hit rate is 99.9%, so the conservative rule costs nothing.
   A truncated subtree is still never published, exactly as `Lin`'s own memo
   never kept one. Nor is a class whose parent clause does not *name* its class
   outright (`lin::parent_names_its_class`): a `Type::Named`, a type parameter,
   a projection or a tuple is resolved by `class_sym_of` through the **scopes**,
   through `abs_projections` / `path_members`, or under the thread-local chase
   guard, and all three move as the typer walks the program with no symbol being
   mutated at all. An applied type, an annotated one and an inner class behind a
   prefix (a `Refined` around the class it names) do count as naming it, which
   matters: gitbucket's every table is `extends profile.Table[…]`, and excluding
   those would have left the cache idle exactly where it pays.
2. **`base_type_args` likewise** (`SymbolTable::bta_cache`), keyed by the pair
   it is a function of: the class and the arguments it is applied at. Buckets
   are per class and scanned with `==`, because `Type` cannot be hashed (it
   holds an `f64`); a class asked about at more than 16 instantiations stops
   being cached, and the whole cache is dropped past 100,000 entries. The answer
   is handed out as an `Rc`, so the hot path neither rebuilds nor clones it.
   The walk reads the parent clauses of every class in the linearization, so it
   honours the same verdict: where the linearization may not be kept, neither
   may this.
3. **`most_specific` re-derived every candidate's type once per pair.** nsc's
   `improvesCache` equivalent (`ImplicitMemo::improves`) already caches the
   *verdict* for a candidate pair; the types those verdicts are computed from
   were not cached, so for n candidates the `subst_as_seen_from` behind
   `implicit_candidate_ty` ran O(n²) times. It is memoised for the life of one
   search now (`ImplicitMemo::candidate_tys`) -- the same validity window
   `improves` already relies on, which is why this is not a new assumption. The
   one branch that reads `companion_prefixes`, a map the search itself writes,
   is deliberately left uncached -- and not only when an entry is already
   there: the search fills that map as it meets wanted types, so a candidate
   whose answer that branch *could* decide is never kept, or the first question
   would answer for the second.
4. **The macro bridge.** With the above in, `SCALA_RS_MACRO_TIMING=1` (below)
   said the engine was 8.5 s of a 27 s build, of which the macro
   implementations' own run was **0.064 s**. Two causes, both outside the
   implementations:
   * `ScalaRsMacroEngine.call` did `recv.getClass().getMethods()` on every
     reflective call. That builds and copies a fresh array each time, and a
     scala-reflect universe class has thousands of public methods -- and every
     node of every tree built or serialised goes through it. `getMethods`,
     the (class, name, arity) overload list and `Class.forName` are all
     remembered now, in `getMethods` order, so the overload picked is the one
     that was picked before.
   * `MacroEngine::read_reply` called `dead_pipe()` -- which **spawns a `true`
     process** -- on every single read, to hold `stdout`'s place while the
     reader thread has it. A gitbucket build reads 600 lines off that pipe:
     1.2 s of the 1.5 s the conversation cost was `posix_spawn`. The
     placeholder is made once per engine and swapped back after each read.

### `SCALA_RS_MACRO_TIMING=1`

`crates/typer/src/expand_timing.rs` reports, per expansion and as a total:
request serialisation, wall time on the pipe, the engine's own compute (its
whole exchange minus the time it was blocked on us), the implementation's run
inside `Method.invoke`, what answering its questions cost us, tree rebuild,
and re-typing the expansion at the call site -- plus the number of round trips
and a breakdown of the questions by kind. The engine reports its own two
numbers through a `(timing)` request, which is the only way to tell "the JVM was
computing" from "the JVM was waiting for us". The instrumentation is off unless
the variable is set (it costs one extra round trip per expansion).

gitbucket, after all four changes (68 expansions: 37 `TableQuery`, 29 `mapTo`,
2 `sql`):

| stage                                             | seconds |
| ------------------------------------------------- | ------- |
| engine start-up (`javac` cached, JVM to `(ready)`) | 0.51 |
| request serialisation                             | 0.003 |
| wall on the pipe                                  | 0.274 |
| &nbsp;&nbsp;of which the engine computed           | 0.241 |
| &nbsp;&nbsp;of which the implementations ran       | 0.090 |
| answering the engine's 572 questions              | 0.003 |
| rebuilding the returned trees                     | 0.002 |
| re-typing the expansions at their call sites      | 0.114 |
| **total**                                         | **0.91** |

The questions, by kind: `viewInfo` 197, `symbol` 138, `companion` 137,
`symbolInfo` 65, `modulePair` 35 -- 0.003 s all together. So the two things the
coordinator expected to matter do not: the engine-side symbol cache already
exists (`ScalaRsMacroEngine.sourceSymbols`, never cleared between expansions),
and **an expansion cache on the Rust side would save at most 0.27 s**, since
that is all the pipe now costs for the whole build. Likewise the JVM warm-up
ideas: start-up is 0.5 s, and `-XX:TieredStopAtLevel=1` would make the
serialiser's own reflection *slower*, not faster. None of them were done, and
the numbers are the reason.

### Where these caches do *not* help

The corpus is the other end of the scale and it barely moves: `CORPUS_SIZE=sample`
(250 per kind, 577 tests) alternating the two binaries gave 44 s / 50 s before
and 42 s / 48 s after, with the same 417 pass and 160 fail. Each of those tests
is a handful of lines, so there is no symbol graph to amortise a cache over and
the time is process start-up and reading the library jar. A cache keyed on "the
symbol table has not changed" pays in proportion to how long the table stands
still, which is a property of *big* compilations.

### What is left on gitbucket (2026-09-12)

16.5 s of worker time with no dominator: the largest self-time entry is
`subst_as_seen_from_walk_at` at 1.2 s, then `class_reaches` 0.7 s,
`mentions_abs_projection` 0.6 s, `Type::clone` 0.7 s, `Type::eq` 0.5 s,
`subst_tparams_cow` 0.5 s. The distribution is flat, which is where to stop
with caches and start with the shape of the data: `Type` is deep-cloned and
structurally compared everywhere, and interning it (or at least `Rc`-ing the
argument vectors) is the next real step.
**142 s** after (`agent/gbmacro`): every one of the 31 call sites now starts the
JVM macro engine, describes the case class being compiled as a lazy mirror and
runs Slick's real `mapToImpl`. The other three measures are unchanged (cats
7.6 s for 340 files, the library 3.3 s, slick 4.7 s), so this is macro
expansion, not a general slowdown. Worth attacking when macro-heavy builds
matter: the engine is started per run, the mirror is rebuilt per call site, and
nothing caches an expansion whose inputs repeat.

## The merge gate's wall time (2026-09-13)

The gate (`tests/verify_merge.sh`) was serial from end to end. Derived from log
mtimes on the last accepted run, and then measured directly once the gate
learned to time itself:

| step | serial (batch/w3 tip) | concurrent, final |
| --- | --- | --- |
| build | 9 s | 0.1 s (already built) |
| slick measure | 7-16 s | 16.0 s |
| cats measure | 8-15 s | 14.9 s |
| gitbucket measure | 3:01 - 5:34 | 5:34 |
| library measure | 5-7 s | 6.2 s |
| `slick_run.sh` | ~2:00 | 1:59 |
| `slick_subset.sh` | ~0:40 | 0:31 |
| workspace tests | 31:00 - 37:45 | 19:05 |
| full corpus | 4:16 - 9:17 | 18:53 |
| fmt | instant | 2.5 s |
| **total** | **29 min idle / 52 min under load** | **27:19 under load** |

The ranges are real: every number on this machine moves by a factor of two or
more with how many slices are running, and the two columns were not taken under
the same load — the serial column is the accepted 631b238d gate (load average
~28 on 16 cores), the concurrent one was measured with three other slices
active (~52). So the concurrent column is a ceiling, not a best case: it did
the same work at twice the load in half the time. It is the per-step table, not
the total, that says where the time goes — and the steps that are still serial
show the load difference plainly (the gitbucket measure alone reads 2:41, 3:20
and 5:34 across three runs of the same tree).

### What the time actually was

**`slick_subset.sh`, the workspace suite and the full corpus share no state.**
Each already had a private work area (`$SP/subset-$$`, `$CORPUS_WORK=$SP/work-$$`,
its own log under `$GATE_DIR`), so they now run as three concurrent jobs. Two
things had to be separated first:

* **the compiler binary.** `cargo test --workspace` can relink
  `target/release/scala-rs`, because workspace feature unification is not the
  same as `-p scala-rs-cli`'s, and a corpus worker that execs it mid-relink
  fails for a reason that has nothing to do with the tree. The heavy steps get
  an immutable copy as `$SCALA_RS`, and the corpus runs with
  `SCALA_RS_PREBUILT=1` so it never wants the cargo target lock.
* **the corpus per-test timeout.** A compile over `CORPUS_TIMEOUT` is recorded
  as `skip`, and `compare_corpus.py` counts a pass that became a skip as a
  **loss**. Sharing the machine makes every compile slower, so the gate raises
  its own limits to 120 s / 60 s. This is not cosmetic: at the default 40 s, a
  corpus sharing the machine with 24 test threads would have invented losses.
  It cannot hide a regression either -- a higher limit only turns a `skip` into
  a `pass` or a `fail`, never a `pass` into a `skip` -- and the gate now prints
  how many rows were skipped on a timeout (0 in both measured runs), so a run
  that was racing for the machine is visible rather than inferred.

**`RUST_TEST_THREADS` was never the problem.** `.cargo/config.toml` pins it to
6 so a slice's `cargo test` leaves the machine usable. Raising it for the gate
bought nothing measurable: the per-binary times libtest reports sum to 1862 s
at 6 and 1850 s at 12, and in both cases that sum *was* the step's wall time.
The reason is that **`cargo test` runs one test binary at a time** and this
workspace has 329 of them, most holding a handful of tests -- so the fan-out
inside a binary has nothing to work with while the machine idles between
binaries.

`tests/workspace_tests.sh` therefore builds once with `--no-run` and runs the
binaries themselves in parallel (`WT_JOBS` x `WT_THREADS`, 6 x 4 by default),
longest-first by test count from `--list` so the 460-test binary is not
scheduled last and left as the tail. Measured standalone against the same tree
`cargo test` had just spent 37:45 on: **16:09**, with identical results (336
rows, 3106 passed, 0 failed, 7 rows from doc tests). The sum of per-binary
times divided by the six lanes matches the wall time, i.e. the lanes were busy
from start to finish, and the slowest single binary (230 s) is nowhere near the
critical path -- so more lanes only help on a machine with spare capacity.

`classfile_lint.py` takes `LINT_JOBS` (the gate passes 6): its `javap` chunks
are independent, and 1498 slick classes went from 16.0 s to 5.5 s with
byte-identical output.

### What is still serial, and why

* **the four compile measures.** Seconds each, and a measure that competes with
  another measure -- or with itself -- is not a measurement of anything. The
  one expensive member, gitbucket at 3-5.5 minutes, is macro expansion (see the
  section above), not scheduling.
* **`slick_run.sh`.** It is a differential *execution* test: it runs each client
  program three times per side and compares stdout byte for byte. Timing and
  machine load are part of what it measures, and its own header records how a
  shared work directory once turned a harness collision into something that
  read exactly like a compiler bug.

### Two checks the gate did not have

* **per-step wall times**, as a table in the summary. They had to be derived
  from log mtimes, which is how the first column of the table above was
  obtained.
* **a step that died without failing a check.** Every heavy step was consumed
  through a `| tail` pipeline, which discards the exit status, so a script that
  exited non-zero while still printing a satisfying summary line read as a
  pass. Each step's status is recorded and named if nothing else failed. The
  same hole existed inside the workspace step: summing libtest's `test result:`
  lines cannot notice a binary that printed none because it died on a signal,
  so the runner counts the binaries it launched and reports `missing=`.

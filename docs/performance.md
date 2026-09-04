## Speed

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
classes, and traits get `T$class` helpers that nsc 2.13 does not emit at all
(it uses interface default methods). So scala-rs reaches this time while
writing about 40% more class files than nsc.

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
- `implicits_in_scope` runs on every implicit search and rebuilds a set of
  every name in every enclosing scope (4%). Caching it needs a sound key, and
  its answer depends on the scope stack *and* on class members that class-file
  loading adds as the search runs.
- Writing the class files is **7% of wall time**, not the 45% an earlier
  reading of `sample` claimed — see *Writing the class files* below for why
  those two numbers are not the same measurement. 2127 files is 2127 creates.
  579 of them are closure classes scalac does not emit: every one implements
  `scala.PartialFunction`, and scalac has 137 such classes to our 716. scalac
  only builds a `PartialFunction` class when the expected type really is one;
  a `{ case … }` passed where a `Function1` is wanted becomes an ordinary
  `invokedynamic` lambda whose body is the match. So the count is a typing
  question, not a code-generation one. A further 106 are `T$class` helpers that
  scalac 2.13 replaces with interface default methods.
- The compile is single-threaded. Parsing is trivially parallel; the typer
  shares a mutable symbol table and is not.

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

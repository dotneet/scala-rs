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
| scala-rs, after the class-file writing pass | **1.8 s** | **1.7 s**         | 2127        |

Medians of three runs each, alternating between the two compilers so both see
the same machine. The last row is the current state; the earlier rows are kept
because the optimisation passes are described below and the numbers are
what each one moved.

The CPU column is `user` only, which is the right comparison for the first two
passes because they were arithmetic. It hides the third: writing the class
files spends its CPU in the kernel, and that pass took **`sys` from 0.83 s to
0.22 s**. Total CPU on the last row is 1.89 s against the previous 2.50 s.

That is **116x less CPU than where it started**, and against nsc **6x faster in
wall time, 38x in CPU**. nsc's wall time is carried by several threads; the
compile in scala-rs is still **entirely single-threaded**, and only writing the
class files is parallel.

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

### What is left

From a profile of the current binary (`sample`, excluding threads parked in
`__ulock_wait`):

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
- `Type::clone` and its drop glue are about 11%. The fix is interning — a
  `TypeId` index, or `Rc` for shared subtrees — which reaches every part of the
  compiler that touches a type.
- `is_sub_type` re-walks the parent DAG on every question and is about 27% of
  type checking.
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
towards writing files (see **What is left** above).

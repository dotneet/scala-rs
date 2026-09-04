## Speed

> Note: the numbers in this document were measured before lambdas moved to
> `invokedynamic`; that change dropped slick's output from 4552 class files to
> 2127. The README carries the current figures. The methodology, the phase
> breakdown and the pitfalls below are unchanged.

What is measured is slick's 184 files (the file list `tests/bench.sh` pins),
with `-Xsource:3`, and scala-library 2.13.16 + slick's 12 dependency jars +
scala-reflect on the classpath: a **full compile** (type checking → erasure →
code generation → writing 4552 class files).

|                                                 | wall time  | CPU time (`user`) | class files emitted |
| ----------------------------------------------- | ---------- | ----------------- | ------------------- |
| nsc (scalac 2.13.16, including JVM startup)      | 11.9 s     | 68.6 s            | 1498                |
| scala-rs (`34c78ba`, before optimisation)        | 217.3 s    | 209.6 s           | 4552                |
| scala-rs (current)                               | **3.5 s**  | **3.0 s**         | 4552                |

All three were taken on the same machine within a few minutes of each other
(load 9–14).

That is **69x faster in CPU time and 62x faster in wall time**. Against nsc it
is 3.4x faster in wall time and 23x in CPU time (nsc uses several threads, so
the CPU-time gap is the larger one). scala-rs is still **entirely
single-threaded**; nothing has been parallelised.

The class file counts differ because **scala-rs emits lambdas as anonymous
classes** (nsc uses `invokedynamic` + `LambdaMetaFactory`). So scala-rs reaches
this time while writing three times as many class files. nsc reports 3 Scala 3
migration errors under `-Xsource:3`, so the number above is with
`-Wconf:cat=scala3-migration:s` silencing them so the run finishes (scala-rs
does not implement that migration check and passes the sources straight
through).

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

### What was slow

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
| writing class files (4552 × `open`/`write`/`close`)   | 11%   |
| erasure / uncurry / pickle                            | 9%    |

and parsing is 0.05 s (1.5% of the total).

### What is still there

- **Hashing is about 6%.** `std`'s SipHash could be swapped for a fast hash on
  internal keys, but that **changes `HashMap` iteration order**, so anywhere
  output is built by walking a map in order, the contents of class files or the
  order of diagnostics would change. Only worth doing together with that audit.
- **Peak RSS is 1.4 GB** (184 files). It does not cost time, but it is a lot.
- **Still single-threaded.** Type checking at 53% is hard because of the
  dependencies; writing class files at 11% parallelises straightforwardly.

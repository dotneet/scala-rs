# Nested profile return types: an investigation probe

Run from the worktree after building its binary:

```sh
cargo build -p scala-rs-cli --release
python3 tests/probes/returning-family/probe.py target/release/scala-rs
```

The probe uses scalac 2.13.16 to build the family library, then compares source
and separate compilation. Each positive program that compiles is executed with
`java -Xverify:all`. When both execute, stdout is compared as bytes. No stubs or
external service are involved. Outputs go to a fresh temporary directory and
are retained, including the JSON comparison report. Disagreement is a reported
result, not a nonzero exit: this pins an open compiler defect.

Measured on `c8104b12`, with the worktree binary freshly rebuilt:

| Case | Scala-rs source | Scala-rs classpath library | scalac both modes |
| --- | --- | --- | --- |
| Explicit conversion, then `returning` | accepts, runs, stdout `4\n` matches | rejects `Root.Out[Int]` member selection | accepts and runs |
| Implicit conversion to reach `returning` | rejects extension lookup | rejects extension lookup | accepts and runs |
| Explicit result ascribed to String | rejects | rejects | rejects |

Rejection agreement does not prove diagnostic agreement: the binary negative
case can fail at the already-broken member selection before the String mismatch.
`Client.scala` combines both positive cases for manual inspection; the script
uses `Explicit.scala` and `Implicit.scala` to keep the two failures independent.

## Connection to gitbucket

A minimal `val profile: BlockingJdbcProfile; import profile.blockingApi._`
with `q: TableQuery[Rows]` reproduces gitbucket's `q.returning(q.map(_.id))`
error. scalac's `-Xprint:typer` selects `queryInsertActionExtensionMethods`, not
`queryToInsertInvoker`. The latter's `BlockingInsertInvoker` has no `returning`
member in `javap`, and scalac rejects that explicit route too.

Calling `queryInsertActionExtensionMethods(q).returning(...)` explicitly works
in scalac but fails here with
`RelationalActionComponent.InsertActionExtensionMethods[Int]`. Slick's JDBC
component defines that abstract family as `CountingInsertActionComposer[T]`,
which does declare `returning`. The independent family in this directory keeps
that outer type-member / inner API structure without Slick or its macros.

## Next checks, not established causes

* `implicit_candidate_ty` calls `at_import_prefix_of`; the latter requires owner
  type parameters and only calls `subst_as_seen_from`. A non-generic inner API
  can still refer to the outer object's abstract type family. The source
  explicit/implicit split is evidence to inspect this path first.
* Explicit source selection already works. In separate compilation, the result
  is represented as `Root.Out[Int]` instead of the overriding `Rich[Int]`.
  Inspect pickle type-member representation and the outer receiver's concrete
  alias before changing implicit search itself.
* The original `Root.api` inherited directly by `Derived` also lost `convert`
  itself during import. The present `DerivedApi extends Api` matches Slick's
  nested API more closely and removes that earlier wall; it does not fix it.
* No evidence yet joins this defect to gitbucket's separate `Shape` cluster.

This probe is not a fix, a merge gate, or a claim that gitbucket compiles.

## Rejected gate and overload investigation

`78cc806f` finished the full gate with FAIL: four workspace test failures
and two corpus losses. See `tests/BASELINE.md`; main remains `c8104b12`.
The next worktree is `.worktrees/codex-returning-overloads`.

The current changes are experimental, not a merge candidate:

- Preserve different generic arities and remove the one-function-overload
  limit. This makes `MapCollect.scala` compile and execute with stdout `2\n`,
  byte-identical to real scalac 2.13.16 under `java -Xverify:all`.
  `78cc806f` rejected that same program. Evidence is in
  `/tmp/returning-overloads-collect/`.
- Trying declaration-site JVM descriptors has not fixed `Map.map` execution.
  `buildfrom` still has three failures, including `ClassCastException`.
  Logs: `/tmp/returning-overloads-descriptor.log`.
- Vector's ambiguous alternatives are distinct declarations:
  `IndexedSeqOps.map` and `IterableOps.map`, copied onto unrelated receiver
  symbols. The original declaration owner's hierarchy must survive copying;
  deduplicating identical `pickled_origin` strings cannot handle overrides.
  Evidence: `/tmp/returning-vector-probe/newdebug.log`.
- Both corpus losses (`spec-asseenfrom`, `t3774`) are collection overloads
  expecting pairs when the argument is not a pair. They must be retested
  alongside `ambigmap`, `buildfrom` and the required supply-boundary suites.

No test has been weakened. Do not merge or push this experimental branch.
Do not rerun the full gate until these focused regressions are resolved.

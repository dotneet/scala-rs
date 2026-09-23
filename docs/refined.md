# Building refined's coreJVM

## Target and scope

[fthomas/refined](https://github.com/fthomas/refined/tree/11560e094e8cb4ea5b6c09a9f72572f179cc80b9)
at commit `11560e094e8cb4ea5b6c09a9f72572f179cc80b9` was checked with Scala
2.13.18 and shapeless 2.3.13. The target is the 45 sources of
`coreJVM / Compile` (44 hand-written ones plus the `BuildInfo.scala` generated
by sbt-buildinfo). No source was modified, excluded, or stubbed.

Given the same sources and the 6 dependency jars that the official
`sbt ++2.13.18 coreJVM/compile` selects, scala-rs generates 360 class files
into an empty output directory. The verification script packs them into
`refined_2.13-scala-rs.jar`. Before this work, scala-rs `ee2e3869` produced 222
errors under the same conditions.

Against the generated refined JAR,
[Smoke.scala](../tests/refinedrun/Smoke.scala) is compiled with both scala-rs
and official scalac, and the following is checked under `java -Xverify:all`.
The consumer class path does not include the officially built refined classes.

- `Month.from` accepting in-range and rejecting out-of-range values.
- `NonEmptyString.from` rejecting the empty string.
- `MD5.from` checking length and hexadecimal characters.
- The adjacent values of `Adjacent` for Double / Float.

This script checks the coreJVM source compilation and a representative JVM run.
All 528 coreJVM tests were checked in a separate validation at the end of this
document. Additional integration modules, Scala.js, Scala Native, and Scala 3
were out of scope.

## Compiler features added

1. Declaring macro bundles of the form `class Impl(val c: Context)`, their
   ScalaSignature, and instantiating them at run time. Mutual separate
   compilation with official scalac is also checked.
2. Type aliases on a refined receiver, abstract type members from binaries,
   inference from implicit arguments, and preserving literal singleton types.
   The members used by shapeless's `Witness.Aux` and `ToList` are handled
   without replacing them with erased JVM types.
3. Macro expansion of type prefixes through `Dynamic`, and well-formed source
   fragments for `c.parse`. Refinements that include constant types and type
   members returned by `c.typecheck` are passed to macros, and attributed type
   carriers are received back. Forward references to higher-kinded type
   aliases are supported too.
4. reify of `this` with type arguments, type-checking a reify body against the
   expected type, and typed Tree patterns of the form `q"${lit: Literal}"`.
5. Owners of binary classes whose names contain Scala operator names, Regex's
   actual constructor, and converting fully qualified Java static methods into
   function values.

Recovering from the error case of `c.parse` by catching the checked
`ParseException` is diagnosed as unsupported by the current Context proxy.
This does not mean arbitrary Context APIs, or arbitrary types and Trees built by
macros, are supported.

## Reproduction

Prepare a JDK, sbt, Python 3.9 or later, and a release build of scala-rs.
The first sbt run needs to download dependencies.

```sh
git clone https://github.com/fthomas/refined.git /tmp/refined-source
git -C /tmp/refined-source checkout 11560e094e8cb4ea5b6c09a9f72572f179cc80b9
cd /tmp/refined-source
sbt -batch -Dsbt.supershell=false '++2.13.18' 'coreJVM/compile' \
  'show coreJVM/Compile/sources' 'show coreJVM/Compile/dependencyClasspath' \
  > /tmp/refined-sbt.log 2>&1
```

Run the following from the root of the scala-rs repository. `--output` must
name a directory that does not exist yet: the script refuses an existing output
directory so that stale class files are never reused.

```sh
cargo build --release -p scala-rs-cli
python3 tests/refined_check.py \
  --checkout /tmp/refined-source \
  --sbt-log /tmp/refined-sbt.log \
  --output /tmp/refined-result
```

`result.json` records the number of sources and classes, and each command with
its exit code. `verdict: PASS` is recorded only when both JVM runs print
`REFINED_SMOKE_PASS`. This check is separate from the full compatibility gate
in `tests/verify_merge.sh`.

scala-rs is given `-feature`, `-unchecked`, the required `-language` flags, and
`-Xfatal-warnings`. The nsc-only lint / unused options and `-release 8` are not
passed, so this does not verify that the full warning configuration of the
official sbt build is reproduced.

Related small regression tests live in `aliaslookup`, `macrotransportbatch`,
`reify2`, and `czero`. They compare acceptance, rejection, and run results with
official Scala 2.13.16. Bounds of inherited type aliases, types prefixed by
abstract types, HLists, and case class extractor patterns are also checked in
`erascg`, `gzero`, `slickshape`, and `warn`.

To check the whole scala-rs workspace, put the same JDK 17 as the baseline on
`JAVA_HOME` and `PATH`, then run the following. The environment used here was
Temurin 17.0.3. Tests that compare string representations of numbers or
Unicode identifier classification depend on the JDK version.

```sh
WT_DIR=/tmp/scala-rs-workspace-result WT_JOBS=6 WT_THREADS=4 tests/workspace_tests.sh
cargo fmt --all --check
cargo clippy --workspace --release --all-targets
```

Confirm that the workspace runner ends with `missing=0 failed_bins=0`.

## Validation results (2026-09-16)

- coreJVM: 45 sources, 360 classes, JAR generated. No upstream source changes.
- Consumer compilation with official scalac / scala-rs against the generated
  JAR, and running under `java -Xverify:all`: both `REFINED_SMOKE_PASS`.
- Whole workspace: 3,436 passed, 0 failed and 0 ignored.
  `binaries=24 rows=31 missing=0 failed_bins=0 doc_rows=7` confirmed.
  After the test-suite fixes below, everything was rerun with the same result.
- `cargo fmt --all --check` passed. Clippy passed with no new warnings from this
  change (existing warnings remain).

## refined's own test suite (after the 2026-09-16 fixes)

The 3,436 tests above are scala-rs's own regression tests, not refined's tests.
Results of rerunning refined's `coreJVM/test` (Scala 2.13.18, all 35 test
sources):

| Condition | Before the fixes | After the fixes |
|---|---|---|
| Official scalac / sbt (control) | 528 passed | Same control result reused |
| Officially compiled tests run on the scala-rs JAR | 525 passed, 3 errors | 528 passed |
| Tests recompiled by official scalac against the scala-rs JAR | 34 errors | 0 errors, 0 warnings |
| The recompiled tests above run on the scala-rs JAR | Not runnable (compilation failed) | 528 passed, 0 failures, 0 errors |
| Tests compiled by scala-rs against the scala-rs JAR | 1,078 errors | 1,084 errors, not runnable |

The 3 runtime errors were `Regex.isValid`, `Regex.showExpr`, and
`Regex.showResult`. The following causes were fixed. No upstream source or test
was changed or excluded.

1. The counterpart of an explicit case class companion was looked up by simple
   name only. `Regex` picked a class from another package, so `apply()` /
   `unapply` were not generated. The class is now resolved by owner and name
   together.
2. Nested wildcards in ScalaSignature were hoisted outward. Existentials inside
   the type arguments of `T => Iterable[_]` and `ToList[RT, Result[_]]` are now
   kept, which restored implicit search for string `Validate` and `OneOf`.
3. shapeless's `::` was stored as the `::` of Scala's List. The symbol's actual
   owner is now used, fixing the type information of `AllOf` / `AnyOf` and
   similar.
4. The type information of objects was missing the enclosing package prefix.
   Restoring the fully qualified name fixed the mismatch with the diagnostic
   strings that `illTyped` expects.
5. The `equals` / `hashCode` generated for value classes and their `$extension`
   methods were not recorded in the type information. Declaring both prevents
   official scalac's spurious equality-comparison warning and the failure to
   resolve the extension methods.

The tests use all 35 sources that official sbt selects (including 7 generated
doctest sources). Direct compilation uses the same dependency JARs as Scala
2.13.18, with `-feature`, `-unchecked`, the required `-language` flags, and
`-Xfatal-warnings` enabled.
The run was done inside an sbt session by replacing, in
`coreJVM / Test / fullClasspath`, `Compile / classDirectory` with the scala-rs
JAR and `Test / classDirectory` with the recompilation output, and then running
`coreJVM/test`. It was confirmed that the displayed class path contained neither
the official refined classes nor the old test output directory, and that
`Passed: Total 528, Failed 0, Errors 0, Passed 528` was printed.

Compiling the tests themselves with scala-rs still hits unsupported parts.
Diagnostics come from implicit conversions to ScalaCheck operators,
dependent-type inference, passing macro types and Trees, macros that depend
directly on `scala.tools.nsc.Global`, and so on.
The 1,084 include cascading diagnostics and are not a count of independent
bugs. Once the type information of `::` was fixed and HList implicit search got
further, the diagnostics beyond it increased.
The 528 passes are therefore the result of "the main code generated by scala-rs,
the tests by official scalac"; they do not mean that the tests themselves could
be generated by scala-rs.


## Additional validation before push (2026-09-16)

`tests/verify_merge.sh` was run to the final `DONE` on `07e69831`.
The workspace's 3,436 tests passed, but the overall verdict was `VERDICT=FAIL`.
To tell the old corpus baseline `ff08907d` apart from the preceding main
`ee2e3869`, the latter was built into a separate output directory and the
failing cases were compared with the same sources and dependencies.

The following regressions introduced by this change were fixed.

- When compiling the standard library's own List / Queue, the `::` pattern
  selected the prelude symbol of the same name. The source class now takes
  precedence.
- Macro bundles that add unrelated type members to Context are rejected.
  Bundles with a well-formed `PrefixType` refinement still compile and run.
- The parent class's type arguments are passed to the type projections of
  inherited methods. `staticModule` / `staticPackage`, which use `U#ModuleSymbol`
  of `Mirror[U]`, resolve again, and type-checking and running through ToolBox
  succeed too.
- `apply` kept being inserted on the result of an `applyDynamic` call with a
  missing argument list. The recursion is stopped and the invalid call is
  diagnosed.

Four regression tests comparing the accepting and rejecting cases with official
scalac were added. After the fixes, refined's 45 sources and the consumer smoke
test were rerun successfully. Every entry of the generated JAR was byte-for-byte
identical to the JAR from the 528-pass run in the previous section (ZIP
container timestamps excluded).

The following are existing failures that also reproduce on `ee2e3869`; they were
not in scope for this push. The overall gate is not treated as passing.

| Target | Control result on main before the change |
|---|---|
| Scala standard library | 1 error at `TailCalls.scala:63`. The 3 List / Queue errors added by this change were fixed |
| GitBucket | Mismatch between `Format[Html]` and `XmlFormat` at Twirl's `xml/feed.template.scala:11` |
| Slick run | 10 of 12 passed. 2 fail with `AbstractMethodError` on `Take.withInferredType(Map, boolean): Node` |
| Cats run | JVM `VerifyError` in `Monoids` at `NonEmptySetOps.contains`. The other 8 passed in the `07e69831` gate |

Validation used JDK 17 and the 5,324-case corpus pinned to Scala 2.13.16 by the
fixture manifest. No baseline was changed to hide existing failures.


## Integrating with main updates (2026-09-16)

origin/main advanced to `25a5d270` during validation, so it was merged with
`2416ef99` to create `639a2e1c`. Both sides' commits were kept, and the
following interactions were fixed.

- For Float extension methods, a conversion taking Float is preferred over the
  implicit conversion that widens to Double. The accepting case of `isNaN` was
  compared with official scalac.
- When reading the binary type alias `Out = M[E]`, the owner's type arguments
  are substituted. The accepting and rejecting cases of inherited `ToList` were
  compared with official scalac.
- Descriptor selection for binary methods also uses the existing erasure logic.
  `Array[A <: Int]` erases to Object and `Array[Int]` to int[].
  The 12 existing erascg cross-call tests passed.

After the merge, the 45 unmodified refined sources still produced 360 classes,
and the consumer smoke test passed with both compilers. Recompiling all 35 test
sources with official scalac gave 0 errors and 0 warnings, and `coreJVM/test`
using the generated JAR passed 528 with 0 failures and 0 errors.
On the other hand, compiling the tests with scala-rs as well still leaves 1,007
diagnostics.

Before the merge, `2416ef99` passed 3,440 workspace tests, with Clippy at the
existing 117 warnings and 0 new ones.
The overall gate on the merged `639a2e1c` ran to `DONE` and gave
`VERDICT=FAIL`. The workspace passed 3,446 of 3,462 with 16 failures.

The 3 failures introduced by the merge
(`mapto2::mt2_reference_targs_expand_and_run`,
`typeidentitybatch::macro_bindings_round_trip_without_selecting_inherited_fallback`,
`gbmapto::source_type_arguments_and_weak_parameters_match_scalac`) were fixed in
`ed6b9d4f`. The explicit argument of `c.WeakTypeTag[R]` was being replaced by
the type alias's own parameter. Type projections with arguments are now handed
to the ordinary argument substitution. All 3 passed on rerun.
Rerunning the 5 type- and macro-related modules gave 66 passes and the 1
existing failure.

The remaining 13 were confirmed to fail, or to produce mismatched output, in the
same way on the incoming `25a5d270` alone. No test was deleted and no
expectation was relaxed.

| Area | Workspace failures that also reproduce on remote main |
|---|---|
| Dependent types / type inference | `compiledquery` 1, `fs2_io` 1, `memberbatch` 1 |
| Constructor default arguments | `mismatch10` 2, `verify_sql` 1 |
| Macro diagnostics | `mapto2` 1, `gbmac` 1 |
| Numeric / character ranges | typer 1, `lsurf` 1, `e2e` 3 |

On the full 5,324-case corpus, the merged version passed pos 1,276 / 1,859,
neg 809 / 1,405, and run 1,043 / 2,060. Compared with `2416ef99`, 24 improved
and 1 regressed. The regressed `run/array-charSeq` fails with the same error on
`25a5d270` as well. Compared with the old baseline `ff08907d`, 10 failures
remain, so the baseline was not updated.

The Slick main code (184 sources) and the Cats main code (340 sources) compile
with 0 errors. GitBucket has 2 errors, in `Regex.Match` and Twirl, the same as
`25a5d270` alone. Cats runs: 2 of 9 passed and 7 failed, also the same as
`25a5d270` alone. The 2 known Slick run failures and the 1 `TailCalls` error in
the standard library remain as well.

In `ed6b9d4f`, refined's 45 sources and the consumer were revalidated
successfully. Every entry of the generated JAR is byte-for-byte identical to the
JAR from the merged version's 528-pass run.
Clippy's 120 warnings are the same as `25a5d270`, with 0 new ones.
Neither a passing overall gate nor generating all tests with scala-rs is
claimed.

After the final type-argument fix, the 27 tests in `qualifier_retry` / `reify2`
/ `erascg` also passed. Rerunning the 343 related macro, reify, and ToolBox
corpus cases gave the same state for every case as at the time of the overall
gate (0 new regressions). This is distinct from a rerun of the overall gate
itself.

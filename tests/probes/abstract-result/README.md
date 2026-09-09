# Abstract implementation result inference

Baseline main a900fb05, compiler 172a6525. No baseline remeasurement.
Probe.scala compiles on the saved accepted binary, then throws ClassCastException
from Raw to Wrapped at Child.value. Real scalac 2.13.16 compiles and executes it,
printing 8. Logs are `/tmp/scala-rs-abstract-result/{before,ours,nsc}.*`.

The inherited result helper was gated on a written override modifier. An
implementation of an abstract method need not write one. The candidate also
uses known abstract results in that case, without forcing pending source
signatures. Matching checks parameter sections and exact parameter types for
these abstract implementations; unrelated overloads must not inherit the result.
Inherited method type parameters are renamed to the implementing method's ones.

The first candidate fixed the cast but made Child.value return Any when Base
said Any and the body returned String. NarrowResult.scala is accepted by nsc
and rejected by that candidate; this disproved the assumption that the inherited
result should remain the final method type. The final candidate uses it as the
body's expected type, then infers the actual adapted result. Unannotated methods
remain lazy, and body completion locks allow recursive use of a known inherited
result. NarrowForward.scala reverses declaration order and also runs correctly.

The absresult fixtures cover source parents and scalac-produced binary parents,
generic classes, renamed method type parameters, unrelated overloads, narrower
results, forward references and recursive implementations. Expected stdout is
8/7/11/13/ok/done, one item per line. Both compilers reject an incompatible Int
implementation of the Wrapped result. The saved pre-fix binary rejects the
fixture's recursive method; fixture-before.compile records it. The original
minimal probe separately preserves the pre-fix runtime failure.

The unannotated Slick Join.scala probe now compiles and executes under JVM
verification with 139 SQL bytes identical to scalac. Logs:
`/tmp/scala-rs-abstract-result/slick-final.{compile,stdout,stderr}`. This is the
query probe only, not a full gitbucket or cats compilation claim. Full merge-gate
validation is pending.


Focused validation: absresult 1/1, the nine mandatory member-supply boundary
suites 534/534, and override/lazysig2/lazysig_impl2/tail1/tail3 52/52 pass
(587 total). Formatting passes. Clippy exits zero with 59 existing warnings and
no additions against the saved singleton-clippy log. The final Slick rerun still
matches scalac; no full gate has been run for this candidate yet.

### Fatal self-reference warnings

The rejected gate's `neg/t6276` loss exposed a missing warning, rather than an
invalid inherited-result acceptance. Scala 2.13.16 RefChecks warns when a
parameterless definition's entire body refers to itself. The candidate now
reports the same seven self-reference sites in t6276 and rejects it with
`-Xfatal-warnings`. Scalac also reports an unrelated auto-application deprecation.

`absresult_selfwarn_bad.scala` covers four direct self-reference forms. The saved
accepted 172a6525 binary accepts it even with fatal warnings; the changed compiler
and scalac both reject it with four self-reference diagnostics.
`absresult_selfwarn.scala` verifies parameterized recursion and another receiver:
both compilers accept it with fatal warnings, and JVM verification succeeds with
identical `7\n8\n` output. The focused absresult suite passes both tests.
Logs are in `/tmp/scala-rs-abstract-regression/warnings`.

This is a focused repair only. The remaining rejected-gate corpus losses and
Slick/library regressions still require investigation before a new merge gate.

### Dependent inherited results

The rejected gate's `pos/t7668` failure was a term-path identity mismatch:
`Extractor.extract`'s result still referred to its own parameter symbol after
being borrowed by `Sub.extract`. Method type-parameter substitution cannot rename
that term path. The inherited-result lookup now reuses dependent-call path
substitution to map ancestor parameters to implementation parameters, after the
signature match. The original corpus source now compiles.

`absresult_dependent.scala` executes with JVM verification on scala-rs and scalac,
with identical `dependent\n` output. The negative counterpart returns the other
parameter's abstract member; both compilers reject it. All three absresult tests
pass. The prior gate provides the pre-fix rejection of the original t7668 source;
the new runtime fixture has not separately been run on the pre-fix candidate.
Logs: `/tmp/scala-rs-abstract-regression/dependent-test.log`.
This does not yet resolve the remaining full-input generic-owner regressions.

### Universal-method covariant bridges

The `run/t7912` regression was missing JVM dispatch adaptation. Inferred Nothing
is a valid result of the user-defined toString. Scalac emits both the narrow
Nothing-returning method and a String-returning bridge. The candidate emitted
only the former, so MatchError called Object.toString and obtained an identity
string instead of encountering the user's exception.

The shared linearization deliberately omits universal roots. Erasure-bridge
emission now includes AnyRef and Any after the ordinary ancestors. The runtime
fixture `absresult_nothing_bridge.scala` fails on the saved pre-fix candidate
(binary and logs in `/tmp/scala-rs-abstract-regression/t7912`) and succeeds on the
changed compiler and scalac with identical output under JVM verification.
All four focused absresult tests pass. This is a backend change and therefore
requires the full class validation and execution stages at the eventual gate.

The first broad root scan failed the existing overload test by bridging the
final JVM method Object.wait. Checking the symbol FINAL flag alone was also
incorrect: prelude intrinsics mark even Any.toString FINAL. Universal-root
candidates are therefore restricted to the overridable Object methods
(toString, equals, hashCode, clone, finalize). Ordinary ancestor lookup remains
unchanged. The final absresult/anonbridge/ifacebridge/override/vcbridge run passes
52 tests, zero failures; log: `t7912/related-final2.log` under the directory above.

### Generic aliases exposed by inherited-result lookup

Full-input tracing corrected the single-root hypothesis. createBuilder's inherited
result is substituted correctly, while its body retains a raw ancestor R.
Separately, own_type_members expands concrete aliases without substituting their
declaring owner's parameters. The four-line absresult_alias fixture reproduces
this: the saved pre-fix binary rejects B as A. Alias expansion now substitutes
only the newly exposed alias as seen from the implementation owner. Substituting
the whole result a second time was rejected during investigation because it
turned an existing Try[R] into Try[Try[R]] in DBIOAction.

The final focused suite passes five tests, including byte-identical alias runtime
output against scalac under JVM verification. A diagnostic full Slick compile
now reports one error (createBuilder), down from the rejected candidate's four;
this is not a merge gate and the accepted main baseline remains zero errors.
Logs: `/tmp/scala-rs-abstract-regression/slick-alias-final.log` and
`/tmp/scala-rs-abstract-regression/alias-final-test.log`. Temporary trace code has
been removed. Library and remaining corpus cases have not been remeasured.

### Inherited type parameters in a self-aliased anonymous class

Adding a self alias to the anonymous-class control reproduces createBuilder's
remaining failure. The unqualified R resolves to Base's parameter, but previously
returned that raw symbol instead of its instance in the anonymous receiver.
`absresult_inherited_param.scala` is rejected by the saved pre-fix compiler and
accepted by scalac. The changed compiler and scalac execute it with identical
`prefix\n` output under JVM verification.

The initial substitution was too broad: a lexical outer owner may also be an
ancestor, and replacing its parameters caused double expansion in DBIOAction
and Type.scala. Lexically enclosing parameters now retain their identity; only
inherited parameters are instantiated. The final full Slick diagnostic compile
reports 184 files, zero errors and 1492 classes. The class count differs from the
accepted baseline of 1490 and requires class-level investigation/validation;
this is not a gate result. All six focused absresult tests pass.
Logs: `/tmp/scala-rs-abstract-regression/inherited-param/` and
`/tmp/scala-rs-abstract-regression/slick-param-final-summary.log`.

### Infix type associativity

The remaining t8146b result displayed a left-associated HCons chain, unlike its
right-associated value expression. Scala 2.13.16 Parsers.infixTypeRest groups
colon-suffixed type operators right and rejects mixed associativity. The parser
now follows that rule. The runtime fixture fails on the saved pre-fix binary;
scala-rs and scalac now execute it with identical output under JVM verification.
Both reject the mixed-associativity negative fixture. Seven focused tests pass.

This was only one root: t8146b still fails implicit Shape adaptation after its
expected HCons chain is corrected. It must not be counted as recovered. Logs:
`/tmp/scala-rs-abstract-regression/infix-test.log`, `infix-parser-test.log`, and
`remaining/t8146b-infix.log` under the same directory. t7212 also still fails and
requires actual infer-override support, which remains explicitly unimplemented.

### HList implicit completion investigation

After the associativity fix, reductions with one, two, and four elements all
failed; real scalac accepts the one-element reduction. An implicit-fit trace
showed hnilShape and hconsShape still had NoType results when considered.
warm_implicit_candidates previously loaded binary parents only. The experimental
change also completes unknown source candidate signatures, excluding signatures
already being completed, and requests a retry when a type changed.

The one-element reduction now compiles, but two/four elements and the original
27-element corpus source still fail. Therefore neither the corpus regression nor
recursive Shape derivation is fixed yet. The next investigation should examine
the nested/annotated HCons argument fit rather than assuming a depth limit.
Temporary tracing has been removed. Evidence is under
`/tmp/scala-rs-abstract-regression/hlist` (probe logs, trace.log, complete logs).
No new runtime regression test yet covers this experimental completion change;
it must be added and compared before any gate/merge claim.

### Nested derivation variable identity

Removing uncheckedVariance annotations does not recover the two-element case.
The trace reaches hconsShape recursively with its own declaration's P2 symbol
as the caller's still-open packed tail. Unify keys both candidate variables and
caller variables by SymbolId; implicit_fit_open already documents this recursive
instantiation limitation. The trace is in `hlist/nested-trace.log`. A fresh
variable identity per candidate application is the next implementation direction;
it must preserve bounds, caller bindings, divergence detection and memo validity.
Increasing the depth limit cannot fix this depth-two collision.

The candidate-completion change now has a runtime regression fixture,
absresult_completion.scala. The saved pre-fix binary rejects it, while the current
compiler and scalac run with JVM verification and byte-identical `completed\n`
output. It uses a real TC implementation, not a placeholder body. Logs and stdout
are in `hlist/completion-*` under the same temporary evidence directory. The
recursive HList regression remains unresolved. All temporary tracing is removed.

### Executable recursive inference oracle

`recursive-pack/RecursivePack.scala` is a non-stub two-level derivation. Scalac
infers its output and runs with JVM verification, printing the saved expected.txt.
The current compiler rejects the open output parameter. Constructor type
arguments are explicit so unrelated singleton widening is excluded.
`recursive-pack/ExplicitResult.scala` changes only the pack call's explicit type
arguments; the compiler then derives the same nested witnesses and runs with
byte-identical output. Compile these separately because they define the same
classes. This is a known-failing probe, not a passing fixture-suite claim.

The explicit HList witness chain also compiles with both compilers. Together
these controls isolate solving an open recursive result from construction,
ordinary nested search and runtime dispatch. Logs are `hlist/recursivepack-*`
and `hlist/explicitnested-*` in the evidence directory above. A proper solution
must freshen candidate variables per application while mapping inferred results
back to caller variables and preserving bounds and divergence/memo semantics.

### Fresh candidate signature prototype

Candidate preparation now creates detached signatures with distinct type-parameter
symbols at each supported search depth. Bounds and nested higher-kinded parameter
symbols are copied with substitution. The selected method remains the original
declaration; divergence checks and memo dependency masks normalize instance IDs
back to it. The warm-up retry refreshes instances when the source signature changes.
This prototype is not yet a validated general solution: allocation cost, cached
signature invalidation, deep derivation and broader regressions need review.

The formerly failing RecursivePack fixture now compiles and executes with the
scalac output, and the two-element HList compiles. Four-element HList still fails;
the original corpus regression is not recovered. A new negative fixture removes
one required implicit leaf instead of weakening its type. Evidence lives under
`/tmp/scala-rs-abstract-regression/hlist/fresh-*`.

### Annotation complexity and newly detected termination regression

After freshening variables, annotation-free four-element HList compiles while
annotated three/four-element cases fail. Divergence complexity treated every
Annotated type as a single node, hiding the tail's size. Counting the underlying
type recovers both annotated reductions. A unit test confirms that a shrinking
annotated tail is allowed while equal/increasing targets still dominate.
The original 27-element corpus source still fails.

Validation found a critical separate regression in the fresh-signature prototype:
implicitmemo's im_memo_bad compile did not terminate. Its owned process was
confirmed live (PID 52799, child of test PID 52794) and terminated individually;
the test session then finished with exit 101. Do not interpret this as a passing
memo/termination check. absresult passed 9, implicit_misc passed 8, and the new
complexity unit test passed 1, but implicitmemo did not pass. Logs:
`hlist/complexity-related.log` and `hlist/complexity-test.log` in the evidence
directory. No test process remains running. Investigating the non-terminating
loop fixture takes precedence over deeper HList support or a merge gate.

### Search-signature registration regression fixed

A bounded reproduction was sampled and its child reaped. The source inspection
then found that SymbolTable::alloc automatically appends to the supplied owner's
members. The purportedly detached instances were ordinary candidates, each
eligible for further instance preparation. Allocating with owner NONE and setting
the copied symbol's owner afterward keeps them out of declaration member lists.
The same applies to the fresh type parameters.

The previously non-terminating im_memo_bad now exits in about 0.8 seconds with
both expected divergence and ambiguity diagnostics. A new unit test verifies
that repeated preparation leaves class and method members unchanged. It passes,
as do absresult (9), implicit_misc (8), and implicitmemo (4): 22 tests total
including the unit test. RecursivePack and annotated four-element HList still
compile. Evidence: `hlist/termination.sample`, `detached-negative.log`,
`detached-unit.log`, and `detached-related.log`. All owned processes completed.
This corrects the earlier assumption that alloc did not register ownership; no
full-gate safety claim is made.

### Deep shrinking derivations and complete witness trees

Depth tracing showed the original 27-element t8146b stopped at search depth 8,
while its HList argument was still shrinking; the unifier's depth guard was not
hit. Search now retains the ordinary depth guard for other paths but allows a
repeat of the same declaration with strictly smaller target complexity. Prepared
instance capacity is based on the wanted type's complexity, extending existing
instances rather than replacing them. The corpus source now compiles (about
0.8 seconds in the diagnostic run); loop/ambiguity controls still reject.

Runtime validation caught another independent cutoff: implicit_tree returned a
bare method at depth 8, omitting required implicit arguments and producing a JVM
operand-stack-underflow VerifyError. Tree construction now tracks derivation
frames and diagnoses non-decreasing recursive expansion instead of truncating
successful evidence. Each construction step solves its own concrete target.

The 12-level absresult_recursive_deep fixture now runs under JVM verification
and matches scalac byte-for-byte. The saved pre-fix compiler rejects it.
absresult (10), implicit_misc (8), and implicitmemo (4) pass, 22 total. Logs:
`hlist/shrinking-runtime-test.log`, `shrinking-full.log`, `shrinking-loop.log`.
This recovers the individual t8146b probe, not a new full-corpus gate; remaining
infer-override and library validation, performance, and class differences are
still outstanding before integration.

### infer-override semantic oracle

Scala 2.13.16 Namers.assignTypeToTree chooses the inherited expected type as the
inferred result under Xsource:3 plus infer-override, excluding final constant vals
and whitebox expansions. This applies to vals and vars as well as methods; merely
retaining a method's inherited result would not implement the feature.

`infer-override/Probe.scala` is accepted and executed by scalac with the flag,
printing method/field/42/constant. The unannotated mutable field has inherited Any
and accepts 42; the final constant still has String type. Without the flag scalac
rejects the missing Any setter. Current scala-rs with the flag rejects assigning
42 to the inferred String field. `NarrowResult.scala` tests the other direction:
with the feature, method and val have inherited Any and cannot be assigned to
String; without it scalac accepts both. Compile the probes separately. Logs:
`/tmp/scala-rs-abstract-regression/infer-override/`.
Implementation remains pending, including macro exceptions and binding the feature
to source mode. Do not mark infer-override implemented based only on t7212.

### Ordinary infer-override implementation

Methods now retain their inherited result when the feature is enabled. Unannotated
vals/vars look up the inherited expectation, type/adapt their initializer, and
store that type for getter/setter signatures. Final constant initializers retain
the legacy narrower result. The CLI already clears source features when source3
is absent; nested macro typing is excluded, but expansion-related exceptions
remain unverified. CLI help and documentation therefore say partial support,
rather than incorrectly claiming the flag changes nothing or is fully complete.

The original t7212 probe compiles. The new fixture executes and prints
method/field/42/constant with both compilers under JVM verification; the negative
method/val narrowing fixture is rejected by both. The saved pre-change probe
rejection and oracle outputs are under the infer-override evidence directory.
All eleven absresult tests pass. Macro exceptions, source-mode boundaries and
broader regression checks remain required before a gate/feature-completion claim.

### Source-mode and blackbox macro validation

Existing xsource3, override, lazysig2 and lazysig_impl2 suites pass 53 tests.
The infer-override source-mode boundary now has a dual-compiler regression test:
without Xsource:3 both compilers ignore the feature with a warning and accept
the narrow String results.

A real blackbox macro compiled by scalac supplies method and val initializers.
With infer-override both compilers execute the client under JVM verification and
print macro/macro. javap confirms both method and field getter use Object and
there are no String-returning alternatives. This matters because a bridge alone
would make an Object-only presence assertion too weak. The 13-test absresult
suite passes; the strengthened signature test was rerun separately and passes.
Logs: `infer-override/macro-and-mode-test.log` and `macro-signature-test.log`
under the temporary evidence directory. MacroLib and MacroUse probes are saved.

Whitebox support is explicitly rejected by macros.rs before macro binding; it
is a compiler-wide unsupported feature, not implemented by this flag. The source
feature remains advertised conservatively as partial. Full gate, library counts,
and generated-class differences still have not been verified on this composite.

### Remaining library regression: parameterless generic receivers

After refreshing inherited expected results at body typing, the candidate
library diagnostic run reports 549 errors in 124 files (accepted reference:
541 in 123). The focused absresult, override, lazysig2 and lazysig_impl2
suites pass all 52 tests. This is not a merge-gate acceptance.

`empty-next/Probe.scala` reduces one remaining regression without library
sources: `Cursor.empty.next()` retains the method's open `T` and is rejected
against the inherited expected `A`. The saved accepted compiler at
`codex-proven-shape/target/release/scala-rs` accepts it, as does real scalac
2.13.16. Both accepted outputs execute with `java -Xverify:all` and have
byte-identical stdout (`value\n`). The current candidate rejects it.
Evidence is under `/tmp/scala-rs-abstract-regression/empty-next/`.
The current hypothesis is an undetermined receiver type argument escaping
selection; `instantiate_parameterless` deliberately postpones solving when
there is no expected receiver type. A repair must preserve inference from
subsequent method arguments rather than prematurely fixing every receiver
argument to its bound. No repair for this reduced case is included yet.

The receiver hypothesis was confirmed: `type_select` now registers the open
parameters of a completed parameterless receiver in `undet_tvars`. No bound
is chosen at selection time. Existing call-argument and expected-result
solvers can then constrain them. `absresult_receiver.scala` covers both the
inherited expected result and a later value argument; the negative fixture
keeps explicit type arguments and enclosing type parameters rigid. All 14
absresult tests passed with JVM execution and scalac output comparison.
The resulting library diagnostic measure is 480 errors in 121 files, with
65 removed diagnostics and four added against accepted 172a6525. Logs:
`/tmp/scala-rs-abstract-regression/library-receiver{,-summary}.log`.

The four remaining added diagnostics are overloaded `compose`/`andThen`
implementations in `typeConstraints.scala`. `absresult_overload.scala`
reproduces the cause: inherited-result lookup accepted either direction of
parameter subtyping for explicit overrides, so a `Broad` parameter received
the `Narrow` overload's expected result. The pre-fix candidate rejects that
fixture (`overload-result/before.log`). Matching dealiased parameter identity
instead preserves distinct overload signatures. This repair still requires
broader regression checks and a clean composed merge gate.

After exact overload matching, all 54 focused tests pass (absresult 15,
override 27, lazysig2 9, lazysig_impl2 3), including scalac/JVM comparison of
the formerly rejected overload fixture. The candidate library diagnostic
measure is now 475 errors in 120 files; detailed logs are
`/tmp/scala-rs-abstract-regression/library-overload{,-summary}.log`.
This remains an intermediate diagnostic run, not a composed merge gate.

### Supply-boundary validation after the receiver and overload repairs

The mandatory release suites all pass: overloadshadow, ambigmap, setapply,
uniteq, integral, ordsummon, mutcoll, conform and e2e (534 tests total).
Log: `/tmp/scala-rs-abstract-regression/seam-tests.log`.
Clippy exposed new box replacement and explicit dereference warnings in
`check_member.rs`; these were corrected without suppressing warnings.

A fresh candidate Slick compilation completes with zero errors and 1492
classes. Output is retained at
`/tmp/scala-rs-abstract-regression/slick-current-classes` for investigating
the two-class increase over the accepted reference. Structural lint reports
`lint_classes=1492 lint_problems=0`. The stronger `verify_all.sh` sweep reports
`verify_classes=1492 verify_failures=0 verify_loaded=1490 verify_incomplete=2`
and exits 2: `slick.jdbc.PGUtils$` and `slick.jdbc.TimestamptzConverter$` fail
initialization with `ExceptionInInitializerError`. Thus this is not a fully
clean load sweep. Logs use the `slick-current-{lint,verify}.log` names in the
same evidence directory. Runtime programs and the full composed gate have
not been rerun after these repairs; no acceptance is claimed.

### The two extra Slick classes repair an existing runtime failure

A class inventory generated from clean accepted commit 172a6525 (rebuilt
immediately before use) was compared with the retained candidate output.
This investigates a dimension absent from BASELINE, not a replacement for
its accepted aggregate measurements. Normalizing generated numeric suffixes
leaves precisely two added classes and no removed classes:
`slick$ast$ScalaBaseType$$anonfun$21` and
`slick$ast$ScalaOptionType$$anonfun$31`. Both implement `scala.math.Ordering`.
They implement the lambdas returned by `scalaOrderingFor` in Type.scala.
The accepted implementation instead returns Function2 and casts it in its
Ordering bridge, causing ClassCastException when called through ScalaType.

`slick-ordering/Probe.scala` is compiled by real scalac 2.13.16 against the
published Slick 3.6.1 jar. The same client runs with `java -Xverify:all`
against candidate classes, accepted classes, and the published jar. The
candidate and published jar return byte-identical five-line output stored in
`expected.txt` (ascending, descending, empty Option and populated Option).
Accepted classes fail at the first comparison with ClassCastException.
Evidence: `/tmp/scala-rs-abstract-regression/slick-ordering-probe/`.
The 1492 class count is therefore expected for this repaired candidate;
merely requiring the old 1490 count would reject the corrected SAM emission.

Verbose initialization verification also identifies both incomplete loads:
PGUtils lacks `org.postgresql.util.PGobject`, and TimestamptzConverter lacks
`oracle.sql.TIMESTAMPTZ`. These are absent JDBC drivers, not verifier failures.
The sweep remains explicitly incomplete with its current classpath.

The standalone `absresult_ordering.scala` now locks this behavior into the
absresult test suite. Its real-scalac/candidate JVM output comparison passes.
The freshly built accepted compiler compiles this reduced fixture but its
JVM execution fails with the same Function2-to-Ordering ClassCastException
(`slick-ordering-probe/reduced-before.log`). The merge gate's exact Slick
class-count expectations are updated from 1490 to 1492 for the two verified
SAM classes; all stages and failure checks remain enabled.

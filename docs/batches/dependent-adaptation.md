# Dependent adaptation batch inventory

Base: local main f86e3b97, accepted compiler 913fb1c0. The previous turn
completed and pushed its gate; this dedicated worktree contains the next batch.
The following prior inventory was checked before implementation. No root or
reproduction is inferred from error wording. No subagents. All mechanisms below
are hypotheses; correcting them by measurement is the primary value of a probe.
The complete remaining diagnostics and five-line source windows are retained in
remaining-diagnostic-inventory.json. Its gitbucket input is expanded a1443e0e;
refresh locations from the accepted next gate before assigning work. Multiline
errors are not all captured by its single-line parser, so its record count is
not the aggregate error total.

| Candidate family | Source evidence | Hypothesis and next bidirectional probe | Difficulty |
| --- | --- | --- | --- |
| Generic appendAll builder result | cats list/queue/seq/vector combineAllOptionK reports Some[To] | Reduce appendAll signature plus concrete builder; compare valid result, wrong element, alone and preloaded builder symbols; execute primitive/reference lists | Medium, repeated-variable solver may raise it |
| Context-bound evidence inside anonymous class | cats Eval algebra vals report ambiguous algebra vs evidence | Compare self-reference eligibility, explicit member types, outer evidence and initialization behavior; nsc may intentionally prefer outer parameter | Medium/high |
| Deferred function result | cats function0/function1 Deferred[Any] | Trace inference through by-name/lazy locals and constructors; valid Function0/1 and independent wrong result probes | Medium/high |
| Inherited generic constructor arguments | cats ApplicativeMonoid/InvariantMonoidalMonoid/Future/Try | Reduce actual subclass-to-base F/A alignment and explicitly requested implicit value; isolate inference from evidence search | High if solver shared |
| Generic IterableOnce.reduceOption | cats kernel Semigroup still missing member although concrete runtime defect repaired | Compare concrete and abstract element conversion with method-value combine; test compilation and exact runtime, with and without symbol warming | Medium |
| Java wildcard Consumer SAM | gitbucket JGitUtil cache.keys.forEach | Probe real Java Map keySet/Consumer[_ >: String], inferred lambda and explicitly typed lambda, invalid parameter/body; execute mutations | Medium |
| Exception.catching overload | gitbucket Implicits.toIntOpt sees only PartialFunction overload | Read actual jar declarations, compare class varargs and PF overload with nsc, execute success/failure paths | Medium |
| Slick Rep[Option[T]] operations | isEmpty/isDefined at several service sites | Trace extension conversion arguments and evidence on real Slick, Int/Date controls, missing lifted evidence and wrong output probes | Medium/high |
| Slick joinLeft OptionLift and Shape | six sites retain O2 and U2 variables | Trace actual constraints/evidence, compare generic and concrete joins plus wrong row, use real emitted SQL output | High, isolate deeply interacting solver |
| Explicit self type intersection | abstract DefaultGitCommand falsely rejects own self type | Reduce Scala 2.13 accepted intersection syntax and inheritance; include genuine missing requirement rejection and runtime mixed implementation | Medium/high |
| Polymorphic method value on FunctionK/self | cats compose and Parallel | Reduce polymorphic apply, self alias, owner substitution, invalid type arguments, runtime composition | Medium/high |
| Dependent SemigroupalBuilder projection | cats repeated |@| reports F[B] vs F[Z] | Probe chained inner results and owner type parameters with generic method; runtime distinct element types | High |
| Slick source-type macro reflection | mapTo still sees placeholder symbol | Real macro declaration/body reflection and identity require a separate substantial slice; no manufactured class/source stub | High, separate |

Silent-miscompile controls must accompany the selected family: wrong generic
argument acceptance, primitive/reference boxing, null/empty collections,
receiver evaluation count, eager versus lazy function evaluation, and
multi-source load order. Error counts cannot discover these. Reuse immutable
accepted binary and real scalac2.13.16; every valid new behavior executes with
-Xverify:all and stdout byte comparison, and every claimed repair must fail
on the before compiler. Batch as many supported low/medium roots as practical,
then existing/historical prerequisites once and a composed full gate once.


## Initial independent probes while gate 913fb1c0 ran

Evidence: next-probes/results.json, with real scalac2.13.16, immutable accepted
9cc and immutable candidate913 binaries. Source and per-compiler output retained.
Exception.catching(classOf[NumberFormatException]), Java keySet.forEach with an
inferred String Consumer, and appendAll(...): bldr.type each execute under nsc
but both scala-rs binaries reject. The generic IterableOnce reduction executes
under candidate913 with byte-identical nsc output; accepted9cc compiles then
fails at runtime. Thus the remaining full-cats missing-member diagnostic is
not reproduced by the generic trait reduction and requires loading-path tracing.
Wrong Consumer element, wrong reduceOption result and wrong builder result
reject with all three compilers.

An attempted catching rejection using classOf[String] was a false hypothesis:
nsc accepts it. Preserve that acceptance as a control, never tighten the API to
Throwable subclasses without evidence. A runnable control and separately invalid
String value argument are checked in extra-catch-probes/results.json.


## Current evidence and composed implementation

Additional independent probes in /tmp/scala-rs-dependent-adaptation reproduce
Deferred[Any], non-implicit algebra overriding an inherited implicit def,
explicit self type inheritance, polymorphic FunctionK/self application,
generic parent constructors, and inner generic result chains. Every valid
oracle program executes using actual scalac2.13.16 / Temurin17, and the
immutable 913fb1c0 binary rejects those seven programs. Direct Consumer[String]
is a passing control; both inferred and explicit String lambdas fail when
passed to the real forEach(Consumer[_ >: String]).

Seven mechanisms are implemented together before broad verification:

- Pickled repeated parameters have the Scala 2.13 immutable.Seq descriptor.
  Actual pickle tracing showed the Class* catching/catchingPromiscuously
  overload rejected because its unknown erased slot could match either JVM
  method. Named-import routing was not the root: qualified calls also fail.
- Java method parameter declarations complete before lambda pretyping. SAM
  prototypes use ground bounds of existential class arguments, retaining the
  original expected type. Contravariant parameter checks reject narrow lambdas;
  the before compiler accepts Consumer[AnyRef] backed by a String lambda and
  crashes with ClassCastException on an Object argument.
- Parameter singletons in dependent method results are replaced by the actual
  argument's singleton or inferred expression type, including nested results.
- Inherited self-type checks can use the template's explicitly declared self
  requirement; subclasses must still discharge inherited requirements.
- Ordinary overriding values can hide inherited implicit methods. Blanket
  removal of the currently initializing value is not the fix.
- Value application through this/singleton types uses ordinary apply selection.
  Overload resolution already substitutes the receiver owner; doing it again
  changed FunctionK[E,F] output F[A] back into E[A].
- Function-valued lambda results preserve the inferred body type. Open nested
  input types defer their prototype until inference. Nominal FunctionN syntax
  was not the root: provisional result widening lost the actual A/B types.

Generic parent-constructor inference and generic inner-class chains remain
separate high-interaction candidates until their actual paths are established.
Slick option/join/macro families remain in the inventory and are not inferred
fixed from adjacent error wording. Standalone bidirectional probes and exact runtime comparisons establish the
roots; broader historical and composed validation is required before merging.
Consumer upper-bound existential argument capture remains a separate existing
false-acceptance: construction is valid, but accept(null) must still be rejected.


## Executed oracle matrix

The permanent `dependentadaptation` suite contains 35 independent programs
in eight groups. Valid programs execute under both compilers with
`java -Xverify:all`, compare exact stdout bytes, and check source order with
a separate symbol-warming source where relevant. Invalid programs must be
rejected by both compilers. The original 28-program suite passed all eight groups before the
broader prerequisite run; three regression controls were then added. Eighteen programs expose differences from the
immutable accepted 913fb1c0 compiler; other programs protect existing behavior.

Controls include Class[String] in Exception.catching (accepted by scalac),
Class varargs and PartialFunction overloads across related exception APIs,
primitive boxing in Consumer, non-implicit member initialization, stable and
fresh dependent results with a single evaluation, lazy Function0/Function1
results, wrong function inputs and outputs, and missing self requirements.
The before compiler's narrow Consumer example compiles and then throws
ClassCastException when called with Object; both scalac and this batch reject it.

The initial Function1 extension exposed a second provisional-type problem:
opening the nested input A to Any reverses the constraint through function
contravariance. Opening it to a wildcard inside Function1 has the same issue.
An unresolved nested function input instead defers the result prototype until
the argument supplies its actual type; fixed input prototypes stay checked.
This is independent of nominal-versus-structural FunctionN representation.

Preflight checked the four pinned source trees, all 121 dependency jar archives,
33 Java support classes byte-for-byte, and 1498 reference cache classes. No
aggregate before measurement was repeated. Full gate evidence and final
numbers belong in tests/BASELINE.md after the frozen tree is verified.


## Prerequisite regressions and corrections

The broad 142-suite prerequisite run passed 1567 CLI tests; typer tests passed
190 unit and eight integration tests. The selected 1390 pos/run corpus units
then found 14 gains and two losses, before any full gate was started:
`pos/t8310` and `run/t3619`. The initial ledger and exact comparisons remain in
/tmp/scala-rs-dependent-adaptation/regressions.tsv and regressions-compare.json.

The generic SAM regression requires inference from annotated lambda parameters
before strict contravariant adaptation. scalac's typed tree confirms String for
unbounded Comparinator[_ >: T]. A declared Number upper bound instead meets the
String input constraint as Number with String; the initially proposed negative
is legal and is now a runtime acceptance control. Explicit Integer with String
parameters is independently rejected. This corrects the initial hypothesis.

The constructor regression came from considering declarations unavailable in
a parent argument expression. Shadow filtering now starts at the effective
lexical this owner used in parent contexts, and respects the separate implicit
constructor-fill scope. The executable control checks the outer implicit's
actual value and uses -Xverify:all to catch an uninitialized receiver.

Both corrections are composed with the existing batch. Focused tests and the
selected corpus are rerun together before the full negative corpus and gate.


The first full negative prerequisite run found two further losses: neg/saito
and neg/t1623, both accepted class declarations instantiated without satisfying
their own self requirements. The declaration check had previously rejected
those classes too early. Instantiation now checks the class's actual type
against its declared self requirement after constructor inference, including
bare and applied new. A valid anonymous mixin and explicit/inferred generic
constructors run against scalac, while three independent unsatisfied forms
must reject. The intermediate negative ledger is retained in
/tmp/scala-rs-dependent-adaptation/final/neg.tsv; no full gate had started.

Final prerequisites: 520 tests in 21 affected CLI suites and 198 typer tests
pass. The selected 1390 pos/run units have zero losses and 14 gains; all 1405
negatives have zero losses and one gain (sammy_expected). Clippy preserves all
57 existing warning occurrences with none added or removed. Format and the
source/jar/cache preflight pass. Final prerequisite evidence is in
/tmp/scala-rs-dependent-adaptation/final-v2. Its immutable compiler SHA-256 is
`48f19da0ff44d9d44503fc30f1927bed2f34d5028a6a34519e0020f76d836ef2`. The full gate must use that same compiler and
the committed tree containing this record.


## Clock-independent test directories

The first full gate on 5ddf4fc9 found an actual AlreadyExists failure in
batchtypes.rs while creating its root directory. This happened before compiling
the case and is a test harness failure. Nanosecond clock readings are not unique
identifiers. The gate continues to DONE and its rejected ledger is retained.

A census found 284 timestamp expressions in 282 CLI test files; 40 files had
no independent nonce. Those clock-only helpers now share a process-local
monotonic stamp, with the process ID encoded to separate helpers whose old
path omitted it. Repeated or reversed clocks cannot reuse a stamp. Existing
file creation, compilation, rejection and exact runtime assertions stay intact.
The compiler sources and Scala fixtures are unchanged from 5ddf4fc9.

Deterministic tests allocate 3072 stamps concurrently at the same simulated
clock value and check a repeated/reversed clock sequence. All 169 tests in
41 affected suites pass, followed by all eight dependentadaptation groups in
the new worktree. Format, clippy (57 existing warnings, no new occurrences),
and source/jar/cache preflight pass. Evidence is retained under
/tmp/scala-rs-dependent-adaptation/harness. The rebuilt compiler SHA-256 is
cf35c34a0c24c0c27481720074c7a41ada3c923f135cdce6193867826a39234f; it has
the same compiler sources as the previous gate but is a separate worktree build.

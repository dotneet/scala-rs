# Lexical contexts and JVM storage boundaries

This batch starts from local main `93fcc040` composed with rejected candidate
`8dc14e22` at `8153e146`. The accepted compiler baseline remains `b969b0d1`.
No before aggregates were rerun. No subagents were used.

## Implementation

- Resolve an implementing type alias in its lexical class context. A local
  alias already has its resolved RHS; substituting through the receiver again
  confused outer Eval.A with the anonymous FlatMap.B. Value-specific path
  members remain tied to their prefixes, and inherited aliases still receive
  the appropriate generic substitution.
- Give a lambda its own method scope while typing parameters and local
  definitions. Lambda parameters in class/object val initializers then become
  real captures for nested classes. This corrects the initial hypothesis that
  capture filtering itself should simply be relaxed. Reflection owner metadata
  uses that same function symbol, and nonlocal returns keep their enclosing
  method target.
- Apply a Function0 returned by a parameterless method when the source writes
  an empty argument list. Keep explicit empty method clauses distinct from
  invoking the returned function. Side effects and repeated invocations are
  checked at runtime.
- Preserve Java Object field read identity, legal boxed writes, finality,
  declaring class, and the raw JVM field descriptor. Static writes emit
  putstatic; generic field writes use the declaration's erased storage type.
  Java Object array elements retain a distinct JavaObject type before erasure:
  reads are references and updates admit boxed values, including through an
  inferred array alias. Explicit Scala Array[AnyRef] still rejects an Int
  element. The first array hypothesis (simply narrow to AnyRef) was disproved
  by real scalac. Generic class arguments are not blanket-rewritten.
- Keep flattened constructor calls separate from partially applied ordinary
  methods when selecting parameter symbols, filling defaults, and computing
  default expectations. The existing verify_sql binary, no-forwarders,
  curried-parent regression is a mandatory first prerequisite. Three distinct
  clause-offset paths had to be corrected; fixing only argument completion
  exposed the wrong String expectation for the Int default.
- Emit erased getter and setter bridges for abstract vals/vars implemented
  with concrete primitive, array, function or value-class types. Property reads
  and stores use the same declaration/use-site distinction as method results;
  boxing and unboxing happen in erasure, while an assignment LHS remains a
  storage location rather than acquiring a read wrapper.

## Focused evidence

`lexicalcontext` contains five groups and 35 Scala programs, using a real
javac-built ObjectFields class where needed. Valid programs execute under
`java -Xverify:all`, with stdout compared byte for byte against real scalac
2.13.16 and checked-in expectations. Both acceptance directions are tested.
Twenty-six accepted programs and nine rejected programs match the oracle.
The immutable accepted before executable differs on 26 of these programs,
including eight programs it accepts but scalac rejects. The new executable
is frozen at SHA-256 `0e152377dc07fb8991a715d24f26c376fb3e9f5728c7f10e3209a0b5af40c5d5`
under `/tmp/scala-rs-lexical-context/prerequisites/`.
The 35-program matrix passes on the current implementation; aggregate gates
and merge acceptance are separate and must be recorded below when completed.

Source/jar/cache preflight passed: four pinned source trees, 121 jar archives,
33 Java classes matching the released jar, and 1498 known reference classes.
All commands use Temurin 17 through the existing with-jdk17 wrapper.

## Prerequisites

The existing constructor prerequisite passes all five tests. The following
151 related CLI suites pass 1604 tests: 1609 tests across 152 suites in total.
This includes every suite in the preceding 150-suite selection, plus
lexicalcontext and erasure3. No existing regression was removed or weakened.
The initial v1/v4 attempts exposed the constructor default-argument paths
before a full gate was launched. All 198 typer tests pass. The selected 1986 pos/run corpus identities have
zero losses and eight gains; pos/t1391 passes again. All 1405 negatives have
zero losses and 5 gains. Clippy retains 57 warning occurrences,
with none added or removed. Format and diff whitespace checks pass.
All nine prerequisite phases reached successful terminal status under one
owned execution handle; their logs and immutable executable are preserved in
`/tmp/scala-rs-lexical-context/prerequisites/`.

## Inventory retained before implementation

# Next combined repair inventory

This is a hypothesis sheet, not a diagnosis to implement blindly. Correcting
these hypotheses with measurements is the highest-value outcome. No subagents.
The current frozen candidate is 8dc14e22; accepted before remains b969b0d1.
Do not modify the gated worktree. Full-gate acceptance is already rejected by
new cats diagnostics and increased library errors; finish and record DONE.

## Selected measured families

1. Java Object field reads. All thirteen added standard-library diagnostics
   concern Object fields (Statics.pfMarker and TrieMap sentinels). marker.scala
   reads the actual library jar field: nsc/before execute true,true; candidate
   rejects with implicit ambiguity. marker_bad.scala assigns to AnyVal and
   nsc/candidate reject while before accepts. classpath.rs fill_java_members
   maps fields through jtype_to_type/parse_field_ty_java without the top-level
   java_result_obj already used for method returns. Hypothesis: extend correct
   read typing while separately preserving legal boxing on mutable Java field
   writes. Probe real javac-built Object fields (final/static/instance/mutable),
   method results/parameters, generic fields, and Scala Any/AnyRef controls;
   do not blanket-narrow every nested Object signature or accept all AnyRef.

2. Context of an implementing type alias. alias_nested.scala reproduces both
   cats Eval errors. The simpler alias_A/alias_T probes pass, and renaming the
   actual nested FlatMap parameter still fails. own_type_members currently
   applies subst_as_seen_from to the RHS of an alias declared on the receiving
   anonymous class. If that RHS already denotes outer Eval.this.A, walking
   anonymous FlatMap[B] -> Eval[B] can re-substitute the same Eval A declaration
   into B. Hypothesis: local aliases already have their lexical interpretation;
   only inherited alias declarations need that receiver substitution. Verify
   with typed symbols, not printed names. Include inherited generic alias and
   nested same-class instances; do not undo expected initializer typing.

3. Captures from val-initializer lambdas. alias_nested before compiles, prints
   3, then fails NoSuchFieldError:s; nsc prints 3,4. A reduced H{val f:()=>Int}
   inside (s:Int)=>new H{val f=()=>s} works when build is local to a method but
   fails in both before/candidate when build is an object/class val. nsc prints
   7 in both. See capture_object_val.scala/capture_class_val.scala and controls
   capture_anon_val.scala/capture_named.scala. anon_capture.rs consider accepts
   only method-owned terms; lambda parameters typed in val initializers can
   be owned by a class despite not being members. Hypothesis: actual lexical
   binders must be captured regardless of this incidental owner. Examine
   lambda_lift capture analysis too. Verify constructor fields/descriptors and
   execution, capture evaluation once, mutable captures and nested traits.

4. Nullary method returning Function0. capture_anon_def.scala should print
   value but both before/candidate print a lambda object. Compilation and JVM
   verification alone pass. Separate member method invocation from applying
   its returned function; inspect check_apply auto_apply_nullary_function.
   Include direct/selected/overridden methods, val controls, explicit empty
   method parameter lists, side-effect counts and negative extra arguments.

## Deferred / controls

- Tuple-to-AnyRef executes identically; an apparent missing subtype match arm
  did not reproduce and must not motivate a patch.
- Source-owned scala.Symbol subclass fails on before too; classpath metadata
  contamination is a separate ownership root. Basic source Symbol without
  inheritance executes identically. Do not claim it repaired by literal typing.
- Existing harder roots remain complete-context generic parents, dependent
  inner classes, Either/OptionT inference and real Slick Shape/OptionLift.

Evidence: /tmp/scala-rs-contextual-followup/next-inventory/* and its runner
scripts in the parent directory. All Java/Scala commands use with-jdk17 and
real scalac 2.13.16. Negative cases need only rejection; do not interpret the
missing-main runtime attempt in the initial marker_bad before log as evidence
of a separate runtime bug.

## Existing regression found by the complete workspace run

5. verify_sql::external_constructor_defaults_are_typed_and_companion_backed
   fails after preserving binary symbol clauses. Its real scalac -Xno-forwarders
   base contains class VSqlCurriedBase(val prefix:String)(val value:Int=42), and
   the child extends VSqlCurriedBase("curried")(). Bytecode passes String alone
   to the String,Int constructor, producing VerifyError. Keep symbol clauses
   for named placement, but flattened parent/new call handling must receive all
   constructor parameter IDs/defaults (check_args call_param_ids and parent
   call completion). Include this exact existing suite in all subsequent
   affected checks; it was mistakenly omitted from the final 30-suite list
   even though it was present in the initial 150-suite run. Do not repeat that
   selection gap. Test both source/binary parents, no-forwarders, generic
   defaults, auxiliary constructors, nested classes and named new calls.

Constructor narrowing: call_param_ids currently interprets
fun.ty.paramss.len() < symbol.paramss.len() as partially applied clauses and
returns symbol.paramss[len difference]. Binary constructors now intentionally
have a flattened Method type (one clause) but retained symbol clauses (two).
For VSqlCurriedBase this incorrectly selects the Int clause as the remaining
parameter list, despite the source parent application still carrying String.
Constructor calls need an explicit flattened-parameter contract here, while
ordinary partially applied methods retain clause subtraction. Verify before
adopting this diagnosis; first_clause_of and other constructor consumers also
need inspection.

## Terminal corpus result

Gate 8dc14e22 reached VERDICT=FAIL and DONE (exit 1, 1359.4 seconds).
Corpus has 5324 unique identities, eleven gains and one loss: pos/t1391.
Its class nA extends AB.A inside object NAnB extends AB, where TB=nB; the new
inherited expectation remains List[AB.TB], rejecting List[nB](). Add t1391
explicitly to historical prechecks. This is family 2's other lexical-context
boundary: enclosing object's type aliases must be viewed through their proper
prefix when typing members of nested classes. Same-named member lookup alone
is not a safe replacement for the actual declaration/prefix relationship.

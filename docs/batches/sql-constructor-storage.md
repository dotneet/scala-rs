# Next composed batch: preliminary inventory

No compiler changes were made during gate 73f68974. Diagnoses below are
hypotheses; measured correction of these hypotheses is the first task.

- Java String construction: `string/Probe.scala` independently reproduces the
  three String-versus-String gitbucket diagnostics, plus a missing Array[Byte]
  constructor on the first direct construction. nsc executes AB/CD/A under
  -Xverify:all; candidate73 rejects. Inventory all String constructor families,
  qualified aliases, load order and independent invalid array/arity arguments
  before implementing. Do not conflate constructor loading with result-type
  canonicalization. Likely low/medium, selected for the next batch.
- Private-this and bare constructor storage: `fields/` separately compiles an
  API and executes an nsc-built reflection inspector on both outputs. Hidden,
  Plain and Mutable should have one private term x. Candidate73 exposes an
  extra private getter x and trailing-space storage x; accepted230 already
  exposed the same incorrect getter and lacked storage entirely. Private,
  Default, Lazy and Abstract controls match after candidate73's earlier storage
  repair. This is a pre-existing unsupported distinction, not a newly observed
  loss of an accepted behavior. Inspect private/public/constructor/body/lazy
  flag families as one matrix and independently test access rejections.
- Explicit dependent macro implementation signatures: macro-transport batch
  already proves accepted230 and candidate73 cannot export some explicit
  c.Expr signatures to nsc's macro-definition shape checker. Inferred signatures
  have passing four-way fixtures. Keep this deeper type/pickle identity work
  separate if the representation change is large.
- Cats Tuple2 swap and NonEmptyList[AnyRef] diagnostics remain unchanged.
  Earlier standalone swap probes matched nsc, so do not apply a blanket prelude
  fix from message text alone. Preserve full-run provenance in any reduction.

Evidence references and compiler versions are in each probe runner. These
probes are inventory only and are not substitutes for a merge gate.

Current gitbucket diagnostic multiset adds a higher-priority concrete seam:
IssuesService.scala:494 no longer lacks GetResult[Int]. Repeated macro argument
packing advances SQL interpolation from an argument-count refusal to
`the expansion contains an empty TypeTree`. This is still a failing source
location, not a working SQL claim. Compare a standalone real Slick SQL macro
under both compilers, including statement text/runtime without a database and
invalid interpolation arguments. Hypothesis: an empty TypeTree in an inferred
ValDef/DefDef annotation should map to Empty in that annotation position;
genuine standalone unresolved type trees must still diagnose. Measure/correct
this hypothesis before touching transport. The exact old/new multiset is
/tmp/scala-rs-gate-73f68974-codex/diagnostic-comparison.json.

## Measured corrections and composed implementation

Base: accepted main2810b214 (compiler73f68974), without a new aggregate before
measurement. Worktree: codex-sql-constructor-storage-batch. Evidence root:
/tmp/scala-rs-sql-constructor-storage/.

The SQL empty-annotation hypothesis was wrong: ValDef/DefDef annotations already
support empty TypeTree. The real Slick3.4.1 sql/sqlu macro reads each argument
tree's type when constructing SetParameter[T]. The bridge supplied Expr's
Nothing weak tag but left the argument tree's type null. Argument packets now
carry the real root type separately from that weak tag. A genuine unresolved
TypeTree remains a diagnostic. Both real SQL forms now compile and run; the
expanded fixture also executes parameter binding against an assertion-backed
PreparedStatement recorder without a database.

String had two independent problems: its prelude declaration did not trigger
JDK constructor loading until another expression happened to load that class,
and a constructor result retained Class(String) where source String uses a
structural type. Java constructors are now loaded before prototype/overload
selection and an exact java.lang.String constructor produces the String type.
A user class named String is not canonicalized. The family fixture includes
bytes/chars, offsets, named/Charset encodings, copy, builders, buffers and aliases.

Private-this and bare constructor storage has no getter and uses the original
field name. Ordinary accessors retain their separate trailing-space storage.
Expanded nsc reflection disproved applying this blindly to lazy/trait vals:
these still require getters. Constructor arguments share a source symbol with
storage, so field-only access/final/accessor flags are removed from the pickled
argument. Independent nsc reflection proves those argument flags are false for
Hidden/Plain/Private/Mutable/FinalParam/ProtectedParam.

The first composed runtime probe passes String output, SQL text and the initial
storage reflection matrix, each with -Xverify:all and exact nsc stdout. Existing
related10 suites pass69 tests before the later lazy/trait/parameter refinements.
The permanent expanded batch is under validation; no full gate has started.

The independent access negatives found two further seams. Directory loading
lost JVM getter visibility and manufactured public fields from constructor
arguments. Preserve actual nullary-getter visibility on the corresponding term
and keep getter-less constructor prototypes private/local. The emitter itself
also published public getters for private-this storage and ordinary private
accessors. Eager private-this storage now has no getter/setter; ordinary
accessors use the existing method-visibility/widening rules. A public overload
with the same name remains callable. This does not claim complete preservation
of qualified/private boundaries or all lazy/trait access-control shapes.

A normal first-clause constructor default was intentionally excluded from
companion synthesis, preventing a separately compiled nsc caller from invoking
`new Default()`. The existing getter mechanism now covers defaults in every
clause of primary and auxiliary constructors. The expanded matrix covers
generic classes, a class nested in an object, explicit companions and defaults
with side effects. It also exposed a pre-existing nested-class miscompile:
the default's companion was left incomplete, and a missing argument became an
invalid JVM call. Complete external constructor defaults before selecting their
getter, and diagnose unresolved defaults instead of emitting an incomplete call.
These extra companions legitimately change generated class counts; audit them
against nsc declarations before updating a gate's exact expected count.

Batch v7 passes all three permanent tests with verifier-enabled byte-for-byte
stdout checks. Storage/defaults API producers and consumers run in all four
compiler combinations, with each inaccessible constructor field an independent
negative. The four constructor-related suites passed 39 tests at v6; final
related/corpus prerequisites use immutable binary SHA256
780ec95f4e940c2947a86144e9b09ddb06d7e492f70bb089a96b0cf0c7c082be.
Source/jar/cache preflight passes: four pinned repositories, 121 jars, 33 Java
support classes and 1498 known reference classes. No full merge gate has started.

The accepted73 binary fails the permanent String and SQL positive fixtures.
It compiles the API, but nsc rejects its consumer because constructor defaults
were not exported, and it accepts all four independent private-field reads
that nsc rejects. Before-proof results are in before-permanent/results.json
under the evidence root; this is focused regression evidence, not a new
aggregate baseline measurement. Final related24 suites pass640 tests.

The first expanded corpus prerequisite ran 613 pos/run cases and caught one
loss before the gate: macro-expand-varargs-explicit-over-varargs. Sending the
internal Repeated(Int) marker as an argument tree type was unsupported. An
independent nsc macro showed its actual Expr tree carries Int, not Int*. The
transport now sends that element type, and a permanent macro fixture compares
literal, empty and spliced argument types and rejects an invalid element type.
Batch v9 passes the four permanent tests and the four existing macro suites.
The final prerequisite binary is SHA256
df9307b75ab6f6570f36043ce0fd197d8918cefd093005338db21d4139e7f06b.

Final prerequisites pass613 pos/run and1405 neg cases with losses=0. Slick
compiles all184 sources without errors and initializes/verifies1504/1504
classes without failures or incomplete loads. Normalize only generated
anon/typecreator/treecreator numeric identifiers while preserving multiplicity:
there are exactly12 added named companions and no removed classes; all12 also
exist in the pinned nsc reference. The raw and normalized inventories are
prerequisites-v2/slick-class-delta.json and slick-class-audit.json. The gate's
exact class-count checks increase1492 to1504 for these real default getters.
Clippy stays at57 existing warnings, with no added or removed warning identities.


## Rejected gate and access integration

Candidate ee80efb0 completed the full gate with FAIL/DONE. Gitbucket reached
148/54 but Slick runtime was 0/36, six workspace tests failed and the corpus
lost run/indylambda-boxing. See the rejected candidate nineteen record in
BASELINE.md; none of this compiler batch is accepted yet. Corrections are in
the separate codex/sql-storage-access-integration worktree.

The access inventory corrected the initial diagnosis again. LOCAL includes
protected[this], whose getter must survive; the missing Slick pattern
accessors are instances of that same rule. Expanded private members also
retain their accessors. Qualified private getters use JVM-public access,
while preserving the known limitation of qualified boundary pickle export.

A missing getter does not imply a private constructor prototype: nsc can
publish an actual field without a getter (the user-written dollar-outer
fixture). Directory loading now preserves actual field metadata. This exposed
our bare constructor argument's incorrectly public storage; field emission
uses the symbol's private-this flags and widens captured constructor storage
through the existing cross-class access scan.

Value-class defaults require default getters on the special value companion.
Reloading raw JVM members must preserve already-completed Scala parents;
otherwise AnyVal becomes Object permanently. Real nsc output also shows a
private underlying value has an expanded public unboxing getter. Source
expansion now accounts for implicit boxing calls introduced after the tree
scan; binary unboxing retains the actual getter name independently of the
constructor parameter spelling. Pattern unboxing must call that getter too.
A separate method taking a value class exposed another pre-existing defect:
pickle-to-descriptor matching expected a boxed reference where the JVM uses
the underlying slot. The new four-way ABI probe keeps this case, function
boxing, collections, typed patterns and private-read rejection together.

Prerequisite revisions are recorded in access-v*.log under the evidence root.
Rejected ee80 compiles the original indylambda-boxing fixture but execution
fails; scalac compiles and runs it. See access-before/results.json. Accepted73
also rejects the new binary value-class method call (echo-before.log). These
are focused before proofs, not aggregate baseline remeasurements.

Final access prerequisites pass 41 tests in six directly affected suites and
598 tests in 18 related suites, all768 selected pos/run cases with losses=0,
and all1405 negative cases with losses=0. The original indylambda-boxing loss
is recovered. Slick passes184 sources, errors=0,1504 classes,12/12 programs
and36/36 attempts; lint_problems=0 and strong initialization verification
loads1504 with failures=0/incomplete=0. Binary SHA256:
455a89e285fe18e0ed1d8ed07badb0887a2e8b0fedbf45a5fc39f4b033da29e6.
The prerequisite orchestrator's final verifier path omitted slick_run.sh's
w- prefix and exited2. The actual retained output was separately verified
once; results.json and verify-all-actual.json record that harness correction.
No compiler, runtime or corpus stage was restarted.

The final matrix also proves widened getter privacy is reflected in the
pickle, original constructor argument names survive storage expansion, and
bare/private-this captured fields work across both producers and consumers.
A fixture initially invoked a nullary reader without explicit Function0.apply
and printed the function object under nsc; it was corrected to invoke apply
explicitly before byte-comparison. Tests retain all actual compiler failures.

Next inventory: operator-inventory demonstrates a pre-existing binary export
limitation for val/var operator names. Both compilers consume an nsc API, but
nsc rejects ours; accepted73 reproduces it too. This corrects the initial
getter-loading hypothesis. Investigate TERMNAME encoding as one family with
constructor special names, setters, fields and type names before changing it.
The private-this body/lazy and qualified-boundary inventory remains separate;
this batch does not claim all Scala access control is complete.


## Reflection parent integration after b2502886

Gate b2502886 completed FAIL/DONE on one new corpus loss,
run/var-arity-class-symbol; its workspace2728 and Slick36/36 passed. The
rejection and ledger are recorded on main; the candidate worktree is frozen.
Corrections use codex/sql-storage-reflection-integration.

The failure is not a value-class descriptor defect. Pickle conversion turns
Function1[Int, Symbol] into structural Type::Function, and attach_parents
previously discarded every non-Class parent other than AnyVal. Raw JVM
completion happened to restore an erased Function1 parent. Preserving the
Scala parent list exposed that missing parent. Convert structural functions
back to their actual FunctionN class form when attaching inheritance, retaining
the Scala argument/result types. No raw JVM parent replaces those types.

The permanent reflection fixture executes TupleClass, FunctionClass and
ProductClass at all supported arities and boundary cases, with exact nsc
stdout. A String argument is independently rejected. Rejected ee80 had
accepted that incorrect String argument; b250 refused the valid calls. Both
before results are preserved under vararity-loss/. New focused tests pass
18 cases in four suites, including all six sqlstoragebatch tests and the
existing binary/source value-class suites. Related suites, full negatives,
previous corpus losses and Slick are now checked together against immutable
binary bd028a67121980c39bea7b2f1a607ad600495154d865266991fdd7da35a66510
under reflection-prerequisites/. No new full gate is claimed yet.

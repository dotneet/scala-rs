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

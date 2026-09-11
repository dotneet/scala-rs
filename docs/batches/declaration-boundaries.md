# Declaration-boundary batch inventory

Baseline: local main 6977d6bb, compiler 96c0abb3, immutable compiler SHA256
8c98b035f42b758d7ffcdc82ffb9e923e83dde675302025276376e63b82eab65.
Every diagnosis below is a hypothesis. Correcting it by measurement is the
most valuable outcome; runtime and acceptance evidence overrides the diagnosis.

Measured with real scalac 2.13.16 and the frozen accepted compiler:

| Candidate | Evidence | Likely mechanism / dependency | Difficulty |
|---|---|---|---|
| Generic superclass value-class argument | nsc 7/7; rs stores Integer, reads unqualified 7, qualified ClassCastException | Parent call loses declaration parameter erasure; constructor PARAM read also unboxes as Integer | medium |
| Generic Unit fields and lazy values discarded in loops | nsc prints 3; rs VerifyError; ordinary def control passes | Statement reference accounting excludes fields | low/medium |
| Object-underlying value-class getter bridge collision | nsc rejects val/lazy val/constructor val; rs accepts then ClassCastException; def control already rejected | Bridge emission skips equal descriptors before checking value-class boxing | medium |
| Loaded parameterless Array method | nsc-produced parent: nsc accepts, rs rejects | Source-only getter provenance misses preserved pickle empty clause list | low |
| Exported Array signatures | rs-produced parent makes nsc crash; rs client malformed descriptor | pickle_type drops Array element entirely | low/medium; prerequisite for four producer/consumer combinations |
| Empty-clause Class result application | nsc rejects x(0) for def x():Index; rs prints 7 | Array-only clause guard misses Class result | medium; prelude provenance needs controls |
| Method result application overloads | nsc ambiguous for array and class result alongside x(Int); rs chooses x(Int) | Applicability must include result.apply; deeply interacts with overload inference | high, separate |
| Nested Seq.flatten | nsc succeeds; rs leaves implicit function method type, reproducing gitbucket diagnostics | Identity subtype view missing from undetermined inference | medium |
| Map generic Set getOrElse default | exact JDBCUtil reduction nsc succeeds, rs Set[_ <: A] | Invariant Set common-type / lower-bound inference | medium/high, trace before repair |
| Tuple.swap | simple nsc/rs generic function probe agrees | Cats full-run failure not reproduced; no code change justified | unknown |
| Object-underlying value-class def | both reject bridge collision | Existing method check correct; retain negative control | no repair |
| Generic String constructor, explicit flatMap(identity), Set.empty, wrong index/type | both agree | Retain positive/negative boundary controls | no repair |

No aggregate before remeasurement. No subagents. No implementation yet at the
moment this inventory was written. Gates will be amortized across supported
repairs, with existing regressions and input integrity checked first.

## Implemented together

- Preserve Array elements, including nested, generic and existential elements,
  in the emitted ScalaSignature.
- Record source and loaded Scala parameterless declaration metadata on the
  method symbol. Array-result application reads that metadata, and a known
  explicit empty clause requires its own call before Class-result application.
- Adapt superclass arguments against the selected constructor's erased
  declaration, and adapt inherited constructor-field reads through the same
  value-class boundary as qualified reads.
- Account for discarded generic field/lazy references instantiated at Unit.
  A field or accessor actually declared Unit already drops its value, so it
  must not be popped a second time.
- Diagnose erased value-class accessor bridge collisions for eager, lazy and
  constructor fields, preserving successful Int/String-underlying bridges.
- Complete residual implicit clauses on already typed selection qualifiers.
  Reuse identity and ordered Array wrapping witnesses during inference and
  ordinary adaptation when building function-valued implicit arguments.

The first getter hypothesis required correction: Scala field declarations of
Unit already emit a void accessor or a getfield/pop sequence. The missing pop
is for fields declared at an abstract/reference type and instantiated at Unit.
The flatten hypothesis also required correction: moving only view inference
was insufficient because the cached qualifier bypassed clause completion.
Seq then passed; Array additionally needed its existing wrapping declarations
in both inference and implicit function construction.

The declbound suite compares real scalac 2.13.16 acceptance/rejection and runs
positive clients with java -Xverify:all, comparing stdout bytes with the oracle
and checked-in expectations. Array and clause declarations cross all four
nsc/scala-rs producer/consumer combinations. The accepted before compiler
fails constructor-value-class, discarded Unit, erased accessor collision,
loaded/exported Array, explicit empty Class clause and flatten probes.
String constructors, ordinary Unit methods, correct Int/String bridges and
explicit flatMap(identity) are retained controls. Scratch evidence lives at
/tmp/scala-rs-declaration-boundaries; no aggregate before was rerun.

Map getOrElse/Set invariant inference and result-valued overload ambiguity
remain measured defects outside this batch. The simple Tuple.swap probe
agrees with scalac and does not reproduce the remaining cats full-run error.
The compiler still has outstanding gitbucket/cats/library diagnostics and
MODE=a/specialization failures; this batch is not a completion claim.

A further runtime boundary probe stores null through Box[Any] and reads it
through Box[Unit]. scalac prints null for both eager/lazy reads; the accepted
before compiler printed (). The new reference accounting preserves scalac's
output, as well as removing the loop stack leak. This is checked by unit_null.

Expanded reference-underlying value-class probes found an additional distinction:
Direct[Text] storage is an unboxed String while Generic[Text] storage contains a
boxed Text. The earlier qualified path already cast the direct String to Text;
extending it to constructor identifiers spread that bug to unqualified reads.
The final adaptation checks retained declaration metadata before unboxing,
matching the existing Apply path. Reference-backed ctor/eager/lazy/method loads
and generic superclass clients are retained as execution regressions.

The initial prerequisite corpus found three existing run losses: lazy-leaks,
t5610a and t9223. Constructor identifier adaptation had intercepted by-name
parameters before erase_ident could force or forward their Function0 thunk.
By-name symbols retain that dedicated path; ordinary constructor fields keep
the declaration/use adaptation. A counter-based execution fixture checks
repeated reads, lazy caching and forwarding into a strict superclass.

A second prerequisite run recovered those three tests but exposed t6385.
javap showed that a value-class method returning this emitted the box where
its declaration promised the underlying Object. The previous erroneous
getter unbox happened to compensate for that producer bug. This receivers
now retain their boxed representation; concrete underlying targets and
declared value-class method result positions unbox it. Any/universal-trait
result positions retain the box. The valueclass_self matrix covers generic,
String and Int underlying types, parameterless/applied/block results and Any
controls. Its applied generic call also fails on the accepted before binary.
Both prerequisite failures stopped before a full merge gate was launched.

The final selected corpus has zero losses and eight run improvements; all
1405 negative identities retain their statuses. Early library compilation
keeps 415 errors at the same locations. Exactly two SortedMap.newBuilder
selections now diagnose their missing Ordering[K] witness instead of reporting
mapResult on an unapplied implicit method. Those exact same-location diagnostic
replacements were reviewed; they are not two newly failing source locations.

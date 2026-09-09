# Binary parent prefix investigation

Base: main 57accf2b, accepted implementation 218b5340. No compiler fix yet.
This investigation began with the remaining gitbucket Shape diagnostics.
The hypothesis that a simple joinLeft would reproduce them was false: an
explicitly typed TableQuery constructor and joinLeft compile. Executing that
program instead exposed a missing enclosing constructor argument.

Lib.scala and Client.scala reproduce the failure without Slick. Compile Lib
with real scalac 2.13.16, then compile Client separately against its output.
Both compilers accept Client. Under java -Xverify:all the scalac client prints
7; the accepted scala-rs binary throws VerifyError in Child.<init>.
The constructor descriptor is (Lprobe/Owner;I)V, but the emitted stack contains
only uninitializedThis and integer 7. The required probe.O module is absent.
The binary used was .worktrees/codex-lazyzip-origin/target/release/scala-rs.
Logs are /tmp/scala-rs-shape-join/alias-{nsc,before}.{compile,run}.

The larger Slick case has the same failure in Rows.<init>: its descriptor
requires RelationalProfile before Tag and String, but only Tag and String
are emitted. The scalac version executes and generates a left outer join SQL
statement; the scala-rs version fails JVM verification before SQL generation.
Logs: /tmp/scala-rs-shape-join/{nsc,before}.{stdout,stderr}.

Relevant implementation paths: backend gen_desc::parent_super_ctor correctly
retains the JVM descriptor. The superclass call emission in gen_class separately asks
outer_field_class, whose enclosing_instance declines binary JAVA classes.
This is a mechanism hypothesis, not a finished diagnosis: a correction must
also preserve the actual imported type-alias prefix (O), not guess it from the
owner class. Two different instances of the same owner must remain distinct.
Explicit inherited member-type lookup without an alias currently fails earlier
with type Base is not a member of object O; it is a separate unclosed issue.

Next: trace parent trees and alias-prefix metadata before erasure; compare
source and binary libraries, two owner instances, static nested classes and
negative invalid-prefix cases. Execute new cases and compare stdout bytes to
scalac. Do not merge a compiler change without the full composed gate. No full
gate or new before measurement was run for this investigation.

## Parent-tree trace

A diagnostic build traced type_parent_ctor_app_in before it overwrites
fun.sym. The unqualified head is Ident("Alias"), sym NONE and NoType;
tree_to_type returns only Class(Base, []). On the second pass the same head
has already been assigned Base's symbol and class type. There is no term
prefix in either tree. Trace: /tmp/scala-rs-shape-join/parent-trace.log.
Temporary instrumentation was removed after observation.

The relevant loss precedes erasure: tree_to_type resolves the imported type
member, and resolve_type_name_completing returns its dealiased type. Parent
constructor typing then replaces the head's symbol with the underlying class.
The import binding must therefore be consulted before that replacement.
Existing object_import_prefixes are keyed by written import origin and avoid
class-wide receiver conflation; term_import_prefixes alone cannot establish
which of two imports a particular parent named. Backend load_outer_arg can
only follow lexical owners/modules, so merely removing the JAVA exclusion
would not establish the required imported instance.

No compiler change is retained, and no full gate was started. The diagnostic
binary in this worktree was built with tracing and is not an accepted binary;
use the saved lazyzip-origin binary for pre-fix acceptance/runtime comparisons.

## Initial implementation, not gated

The local candidate retains import prefixes by binding origin, materializes
the prefix as a typed expression on the parent head, and preserves the
classfile-verified enclosing descriptor on the class symbol. Superconstructor
emission uses that descriptor's class and the parent head's actual prefix.
A first attempt retained a prefix with NoType, so new_prefix_instance declined
it and codegen still loaded this. Explicitly typing the retained prefix fixes
that failure. Temporary backend tracing has been removed.

The minimal binary Alias client now executes under java -Xverify:all and its
stdout compares byte-identically with the scalac client (7 followed by LF):
/tmp/scala-rs-shape-join/{fixed,oracle}.stdout. Release build and diff checking
pass. This is only the first positive reproduction: multiple instances,
qualified/applied parent types, negative cases and the required regression
suites remain to be checked before any full gate or merge.

## Generic and distinct-instance probes

MultiLib/MultiClient reproduce two imports of the same inherited Alias[A]
through O and P. The initial implementation missed AppliedTypeTree heads;
recursive qualification fixes that. Both clients now execute under full JVM
verification and stdout bytes match scalac: 7, 11, one on separate lines.
Qualified.scala also initially failed because its explicit prefix was untyped;
qualifying that term path fixes it, with byte-identical output 7. Wildcard.scala
executes with output 11 in both compilers. Bad.scala passes an Int where String
is required, and both compilers reject it. Evidence directory:
/tmp/scala-rs-shape-join/multi.

Focused existing suites outer, innerclasses, parentcheck and libctor pass;
log /tmp/scala-rs-shape-join/related.log. The mandatory broad boundary suites,
permanent regression harness, full Slick runtime probe and composed merge gate
remain pending. This candidate is not yet accepted.

## Permanent regression and remaining API alias

crates/cli/tests/bparent.rs compiles bparent_lib.scala with real scalac, then
compares both clients under java -Xverify:all against expected/bparent.txt.
It covers distinct object imports, type arguments, qualified and wildcard
parents, and a static nested class, plus rejection of an invalid argument.
The test passes. The nine mandatory boundary suites pass 534 tests with zero
failures: /tmp/scala-rs-shape-join/boundary.log.

The actual Slick Join.scala still fails JVM verification. Its imported Table
alias is declared in RelationalAPI as type Table[T] = self.Table[T]; the
written API instance is not the Profile instance required by the constructor.
ApiLib.scala and ApiClient.scala reproduce this without Slick. Scalac executes
and prints 7; the candidate accepts but still uses uninitializedThis as the
outer argument. Logs: /tmp/scala-rs-shape-join/api/{nsc,ours}.{compile,run}.
The fix must preserve the alias RHS enclosing self path, rather than guessing
an enclosing instance by scanning the written prefix for a compatible type.
No full gate has been launched and the candidate is not merged.

## Pickle prefix preservation

Inspection of the real scalac ApiClient constructor shows it loads O directly;
Owner.API has no outer accessor at all. The previous type representation
cannot justify this choice: crates/pickle/src/sym.rs drops a TypeRef's This
prefix when producing SigType::Ref. Member now carries alias_prefix separately,
including through polymorphic and annotated alias RHS wrappers. Substitution
of refined members also transforms this metadata.

The new pickle integration test alias_prefix.rs builds a generic Owner/API
library with real scalac, reads Owner.class, and asserts Alias's retained
prefix is This(prefixprobe.Owner). It passes. Existing pickle tests also pass
(3 unit and 7 jar tests); these precede the new integration test. Logs:
/tmp/scala-rs-shape-join/{pickle-prefix,alias-prefix-test}.log.
The typer has not yet consumed this new metadata, so the API alias runtime
failure remains. The 534 boundary result above precedes this metadata change.

## Typer metadata handoff

Binary alias completion now retains the original SigType prefix in
SymbolTable.binary_alias_prefixes under the declaring owner and alias name.
This key identifies declaration metadata only; it does not record or choose a
call-site receiver. The API reproducer reaches this path and records exactly
apiouter.Owner.API#Alias -> This(apiouter.Owner), confirmed in
/tmp/scala-rs-shape-join/api/metadata.log with SCALA_RS_PICKLE_DEBUG.
The release build passes. The constructor receiver selection does not yet
consume this metadata, so the runtime failure remains. A receiver must follow
the alias's actual enclosing path; selecting any compatible object in the
written qualifier chain would not establish that it is the right instance.

## Forwarded API counterexample

ForwardApiLib/ForwardApiClient make Holder (seed 99) publish P.api (seed 11)
as other. Real scalac prints 11 and its constructor loads P directly. Choosing
Holder because it is compatible with Owner would silently return 99. This
rules out scanning written prefix trees merely for a compatible outer type.
Evidence: /tmp/scala-rs-shape-join/forward-api. Both compilers accept the
explicit-parent client; the candidate still throws VerifyError.

The variant without constructor parentheses additionally exposed that
parent_ctor_is_fillable excluded empty source parameter lists even when a
binary constructor needs a hidden outer argument. The local correction
supplies binary constructor metadata and routes that case through ordinary
parent application typing. bparent now includes an EmptyAlias parent without
parentheses to execute this path against the real scalac oracle.

## Forwarded singleton path

The pickle integration fixture now includes Holder.other = P.api. It asserts
that the accessor result retains Single(Single(This(package), P), Owner.api),
in addition to the alias RHS This(Owner). This real-scalac assertion passes;
the required P identity has not been lost in the pickle reader.
The next loss is pickle_supply::conv_at's SigType::Single { sym, .. } arm:
it resolves Owner.api's module class and returns ModuleRef, discarding the
prefix P. Preserving only alias-prefix declaration metadata cannot fix this.
The singleton conversion must preserve its per-use path before parent
receiver reconstruction can distinguish Holder from P. Evidence and test:
/tmp/scala-rs-shape-join/forward-singleton-test.log, alias_prefix.rs.

## Singleton conversion prototype

conv_at now attempts to retain a non-static member module's prefix as
Type::SingleType instead of ModuleRef when a corresponding module term exists.
The focused bparent, nestedobj and outer suites pass 17 tests. This is not yet
an effective fix for the forwarded API: a trace proves conversion is reached,
but companion_module returns None for fapi.Owner.api. It therefore still
returns ModuleRef. Evidence: forward-api/singleton-trace.log under the probe
scratch directory, and /tmp/scala-rs-shape-join/singleton-path.log.
The next step must account for the binary module-class/term representation;
do not claim the prefix survives until the actual forwarded case proves it.

## Binary module-class singleton representation

The singleton prototype now permits the module-class symbol itself when
companion_module has no term to return. singleton_underlying explicitly widens
that module-class symbol to ModuleRef instead of following an absent type to
the enclosing prefix. The actual forwarded import now displays a singleton
.type rather than a bare module type in --typer output:
/tmp/scala-rs-shape-join/forward-api/typed-moduleclass.log.
Focused bparent/nestedobj/outer tests pass 17/17; log:
/tmp/scala-rs-shape-join/moduleclass-singleton.log.
This proves the representation change reaches the reproducer, not that its
constructor is fixed. Parent receiver reconstruction and the API runtime
comparison remain outstanding. No gate has been launched.

## Parent module argument connection prototype

The typer can now record a stable outer module per subclass when a qualified
parent carries SingleType(ModuleRef(outer), member-module) and the member's
lexical owner equals the binary constructor's verified outer class. Backend
super-call emission consumes this per-subclass module before lexical fallback.
The release build passes, but the forwarded API still fails: tracing shows
its parent fun remains Ident(Alias) on both passes, so no qualifier reaches
this rule. Evidence: forward-api/parent-module-trace.log in the scratch probe
directory. Temporary tracing was removed. The next correction belongs to
lazy wildcard type binding/prefix recovery, not another backend fallback.
No runtime success is claimed for this prototype; no gate has run.

## Lazy wildcard binding origin

WildcardImport now records the written origin, and lazy binary type-member
completion uses enter_import_in_current with that origin instead of dropping
it through enter_in_current_ranked. Parent qualification completes a lazy name
before looking up its binding. Nullary aliases retain the imported name rather
than substituting the underlying class name into the qualified source path.
A typed qualified val/accessor also counts as a term type-prefix; previously
Holder.other was incorrectly looked up as a nested object after qualification.

The release build passes and the forwarded case compiles again. Its runtime
still fails verification, so the per-subclass outer-module predicate needs
further tracing. Latest evidence: forward-api/origin-fixed.{compile,stdout,stderr}
in the scratch directory. One earlier shell probe attempted java after a
compile failure and ran stale classes; that output is not validation. The
latest Python probe explicitly runs only after compilation succeeds.

## Forwarded API runtime fixed

The qualified singleton and hidden-outer owner already agreed. The remaining
predicate failed because P was only a placeholder with unloaded parents.
Completing its classfile before subtype comparison resolves the outer module
without guessing a compatible receiver. ForwardApiClient now executes under
java -Xverify:all and compares byte-identically to scalac (11 LF), evidence:
forward-api/{ours,nsc}-completed.stdout. bparent's permanent fixture now also
uses Holder(seed 99).other = ApiP(seed 17).api and asserts result 17.
Temporary parent-condition tracing has been removed.

The direct O.api example still fails: its prefix is an ordinary module
selection rather than the forwarded member's singleton result. That separate
path remains to be connected, as does the original Slick reproducer. This is
not a completed slice or a gate-ready claim.

## Direct member object runtime fixed

Direct O.api selected a classfile getter with an ordinary class result, so no
singleton return path was available. member_module_owner consults the pickle
and only accepts a genuine MemberKind::Module declaration. The parent path
then uses that member object's written receiver, checking its lexical owner
against the required binary outer type. It does not reinterpret arbitrary
getters returning the same module class as member objects.
The direct ApiClient now executes under java -Xverify:all and stdout bytes
match scalac (7 LF): api/{ours,nsc}-direct.stdout in the scratch directory.
The permanent bparent fixture includes both direct and forwarded API objects.
Focused bparent/nestedobj/outer suites pass 17/17:
/tmp/scala-rs-shape-join/direct-fixed-test.log.

The original Slick Join.scala still compiles but fails constructor verification;
its API is not this member-object declaration shape. Latest runtime evidence:
/tmp/scala-rs-shape-join/slick-direct-result.log. The remaining ordinary API
value's declared type prefix must be handled; do not broaden the member-object
rule to arbitrary getters. Temporary direct-parent tracing has been removed.

## Ordinary API values

Member.result_prefix now retains the outermost TypeRef prefix of values and
method results, following parameter clauses and annotations. Parent-owner
resolution accepts an explicit declaring-this result prefix, as well as a
member object declaration. ValueApiClient (val api: API rather than object api)
executes and stdout bytes match scalac: value-api/{ours,nsc}-fixed.stdout (19 LF).
The permanent bparent regression includes this shape.

The outer-owner compatibility check now permits a declared owner subtype of
the hidden outer type, loading its parents first. With that change the original
Slick probe fails compilation at q.result.statements instead of reaching its
constructor: value statements is not a member of StreamingProfileAction.
This is newly exposed by the additional completion and needs investigation;
no Slick runtime success is claimed. Log: /tmp/scala-rs-shape-join/slick-selftype-result.log
contains the previous runtime output only if compilation failed; the latest
compile diagnostic was observed directly and must not be confused with it.
The candidate remains unmerged and has not run the full gate.

## Slick constructor boundary reached

Loading only the declaring owner's parents (ensure_parents) instead of all
its classfile members restores q.result.statements compilation. The parent
predicate requires hierarchy facts, not eager member installation. Original
Join.scala now passes constructor verification and reaches Rows.$times, where
an existing direct cast from Rep to ProvenShape throws ClassCastException.
Evidence: /tmp/scala-rs-shape-join/slick-parent-only.{compile,run}.
Thus the original hidden-outer VerifyError is resolved, but this is not a
successful Slick query execution claim.

Writing def *: ProvenShape[Int] = id explicitly separates the next issue:
scalac accepts and executes the SQL query; scala-rs refuses Rep[Int] where
ProvenShape[Int] is required. Logs: slick-typed-{nsc,ours}.{compile,stdout,stderr}
in the same scratch directory. The unannotated version's bridge cast and
missing implicit conversion need a separate root investigation. Do not label
this as a newly introduced runtime regression; the saved pre-fix binary never
reaches this point because its constructor already fails verification.


Current broad boundary validation: bparent plus the nine mandatory suites
pass 535 tests, zero failed, on the current candidate. Log:
/tmp/scala-rs-shape-join/boundary-current.log. This includes the direct,
forwarded and ordinary API-value runtime cases. Full gate and clippy comparison
remain pending; no accepted baseline has changed.

## Candidate review before full gate

The review found and repaired explicit renaming of a nullary alias: its
imported binding contains the underlying class symbol, so recovering only
that symbol's name or the local alias name is insufficient. Original names
are now recorded with the written import origin. bparent includes
EmptyAlias => RenamedEmpty and compares execution with real scalac; it passes.
Clippy exits zero with 59 individual warnings, no additions against the saved
lazyZip baseline (59). Logs: /tmp/scala-rs-shape-join/{renamed-test,clippy-final}.log.
The prior 535-test boundary run and this focused rerun cover the current
candidate; full composed validation remains the merge requirement.


### BitSet regression reduced after the rejected 07de211b gate

The two new standard-library diagnostics reduce to a source companion class
and object in each of two packages. A nested class in the second object
extends the first object's nested class with constructor arguments. The new
parent-prefix typing calls qualified companion lookup; that helper incorrectly
entered the first package's companion as an unqualified binding in the current
scope, shadowing the enclosing companion. Thus this was lexical-scope pollution,
not missing BitSet members or binary parent completion.

`/tmp/scala-rs-bitset-regression/Proxy.scala` is accepted by the saved accepted
compiler and real scalac; 07de211b rejects its `BitSet.fromBitMaskNoCopy` call.
The before/candidate/nsc-proxy logs preserve those results. With scope insertion
restricted to the unqualified identifier caller, fixed-proxy.log shows acceptance.
The permanent `bparent_scope.scala` fixture uses independent package names and
executes the shadowed method; both ABI modes match scalac's stdout bytes under
`java -Xverify:all` (17). Both bparent tests pass. Full-gate validation remains
pending; this evidence does not replace the accepted baseline.

Validation after the scope correction: bparent 2/2 and the nine required
member-supply boundary suites 534/534 pass. `ScopePollution.scala` preserves
the exact reduced source used for the accepted/candidate/scalac comparison.

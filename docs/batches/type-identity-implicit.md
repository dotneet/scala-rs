# Type identity and binary implicit objects, based on 00718890

The previous goal turn was progress: composed compiler 23031519 passed its
complete gate, was merged, and its metadata was pushed as 00718890. This batch
starts from that local main. Aggregate before values come only from BASELINE.

All diagnoses below are hypotheses; correcting them by measurement is a primary
purpose of the inventory. No subagents are used. Existing related tests run
before editing: 43 tests in eight suites pass (unqname, nameamb, preludeshadow,
applied_collection_names, hkbound, lowbound, slickimplicit and implicit_misc).

| Candidate | Bidirectional evidence | Dependency / decision |
|---|---|---|
| Binary implicit object | nsc runs; before misses it; val/def and explicit selection work | Medium: implicit enumeration, flags, module ABI and parents together |
| Slick GetResult[Int] | Standalone summon fails before; nsc gets GetInt | Same object mechanism; verify with the real dependency jar |
| Local/qualified Array and local Function1 | nsc runs; before rejects | Medium: resolve constructor identity before intrinsic lowering |
| Qualified custom.String | nsc runs; before treats it as java.lang.String | Same qualified-name mechanism |
| Missing.String and scala.collection.String | nsc rejects; before accepts | Remove qualifier-blind String shortcut |
| Wildcard-imported custom.Int | nsc runs; before treats it as scala.Int | Respect wildcard type bindings above default imports |
| Array[Int,String] | nsc rejects; before accepts | Use ordinary kind/arity checks before lowering |
| Written upper/lower/dependent bounds and alias bounds | nsc rejects; before accepts | Medium: check written applications, not just constructor calls |
| Invalid first argument with wildcard sibling | nsc rejects; before accepts | Skip the wildcard itself, not all sibling checks |
| Box[_ <: String] for Box[A <: Number] | Both accept | Corrected hypothesis: preserve existential intersection acceptance |
| Local Tuple2 construction / local Predef.Function alias | Both run | These initial probes do not reproduce a defect; retain coverage |
| Generic/F-bounded valid application | Both run | Positive boundary regression |
| Cats Vector scanLeft/scanRight results | Isolated generic helpers run in all compilers | Harder full-run loading/inference context; do not guess a signature fix |
| Instance-prefix Manifest and TypeTag interoperation | Known representation limitations | Separate deeper batch; no erasure substitute |

Inventory source and logs are in /tmp/scala-rs-type-identity-batch/inventory/
and /tmp/scala-rs-evidence-batch-probe/next-{implicit,builtin,vector}-inventory/.
The accepted 23031519 compiler was rebuilt before current before probes; no
aggregate baseline was rerun. Source SHA ancestry, main/origin and clean status
were checked before creating the dedicated worktree.

The initial implementation resolves Array/FunctionN/TupleN constructors through
normal type lookup, checks arguments, then lowers canonical library identities.
Qualified String follows actual lookup. Written bounds reuse the existing
substitution-aware checker, including alias parameters, and one existential
argument no longer waives checks on its siblings. Binary implicit objects are
included in both implicit-name discovery paths. Their flags and parent types
are completed; actual MODULE$ fields distinguish static modules from instance
member accessors. Already loaded unflagged members are repaired rather than
mistaken for completed implicit evidence.

The first implementation passes 46 tests in ten focused suites, followed by
571 tests across 14 wider suites including e2e and the required pickle/prelude
supply cases. These checks precede the later ABI and lazy-import corrections.

The expanded fixtures exposed two additional defects in the same batch. The
ScalaSignature writer emitted only MODULE on module terms, dropping source
IMPLICIT and visibility flags; nsc could not summon an implicit object produced
by scala-rs. The eager directory classpath loader also dropped IMPLICIT from a
val (its method branch already copied it). Jar loading took a different path,
so the same generated API worked in a jar and failed from a directory. Both
metadata paths now retain the semantic flags. API fixtures cover both producer
compilers and consumer compilers, directories and jars, two Holder receivers,
inherited objects, value/method controls, ambiguity and private evidence.

The java.sql.Array case also required exposing explicit lazy wildcard types
before falling back to an existing prelude type. User-import origins and
prelude symbol identity distinguish that default binding from an explicit
source binding. Qualified missing types under known packages must not fall back
to bare names; package-object aliases still get their proper lazy lookup.

Early filtered corpus checks caught exbound, spec-Function1 and t5639 before
the full gate. Arrow syntax now has a parser-only constructor marker, separate
from a written user FunctionN. Existential binders remain quantified during
written-bound checking. Source recompilation supersedes eager classpath symbols
before imports can offer both old and new implicit vals. Interfaces are never
classified as companion static forwarder classes.

The private runtime has no Array factory companion: both qualified and bare
Array(2,3) fail there already. The type-identity fixture uses the supported
qualified array constructor and stores instead, so its runtime comparison
covers type identity in both modes without claiming factory support. Extending
the private factory ABI is outside this batch.

The remaining directory-only companion failure was not explained by the
forwarder hypothesis alone: tracing showed the real Evidence trait was already
present, but its companion had never entered the implicit-only completion path.
The jar discovery path calls supply_implicit_members; the eager directory path
had returned early because the module already existed. It now supplies only
implicit declarations for that pending binary companion. Failed requests before
a pickle becomes readable are no longer cached as completed requests.

The final focused tests pass 3 harnesses: source identity/bounds in jar and
private modes, API exchange with both compilers and both classpath formats, and
source recompilation with output 2 rather than the old binary's 1. The rebuilt
23031519 rejects the valid identity fixture in both modes and falsely accepts
all ten invalid type fixtures; nsc rejects its implicit-object API as evidence.
Before evidence: /tmp/scala-rs-type-identity-batch/before-final/results.json.

Final prerequisites pass 778 tests across 32 result rows,
including parser/backend unit tests, reification/kind-projector and all affected
CLI supply/constructor/bound suites. The 243 selected corpus identities have
losses=0 and one gain (run/toolbox_typecheck_inferimplicitvalue). Release
workspace clippy retains exactly 57 warnings. Preflight validates four pinned
source trees, 121 jars, 33 support classes and 1498 reference classes.

Slick preflight compilation caught 16 false bound rejections before any full
gate. Tracing proved the declared bounds still held provisional Named values
(NoStream/Effect) while the actual arguments already referred to their source
class symbols. Header processing now refreshes class parameter bounds in the
declaration's lexical imports before another unit's signature applies them.
The permanent two-source test runs both source orders in all three compilers,
executes the valid program and rejects an independent invalid parent. A saved
pre-header-fix binary rejects the valid program; header-before.log records it.
The follow-up bound/header suites pass 19 tests, including all four new harnesses.

After the header correction, the complete Slick precheck compiles all 184
sources with zero errors and 1492 classes. The 243 corpus prerequisites still
have zero losses and one gain; clippy remains at the same 57 warnings. The
full composed gate has not been substituted with these prerequisites.

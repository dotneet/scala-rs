# Case-copy, by-name and local-signature recovery

This composed candidate continues the rejected e5df7b09 tree and contains main
788e5695 through merge 73a6bafe. The accepted compiler remains e608c7dc until a
complete integration gate passes. No subagents are used and no aggregate before
measurement is repeated. Initial inventory: case-copy-byname-inventory.md.

Every proposed root was a hypothesis, to be corrected by measurements. Supported
repairs are implemented together; focused validation repairs the combined tree.
Full gate must wait for all prerequisites, exact identity checks and clean new
project-diagnostic locations. Do not accept improved totals that hide new errors.

## Implemented mechanisms

1. Prelude CASE/ctor_fields represent a supported copy constructor rewrite even
   without a synthetic method symbol. Require a surviving synthesized copy only
   outside that representation. Source custom/private/inherited copy suppression
   and ordinary curried method calls remain covered. Test all Tuple1..22, Some,
   Left and Right, plus separately compiled ordinary/custom/inherited cases.
2. Expected-result applicability can rescue a singleton candidate but cannot
   discard a call already ordinarily applicable. Preserve the precise Box[Unit]
   argument mismatch in cats3 and nested invariant-state inference in ctxev.
3. Add Tree.byname_thunk provenance. Source functions of every arity are values;
   a generated delaying Function0 is distinct. Argument inference presents the
   latter as ByName(result), including parent constructors, repeated/default
   applications and dependent clauses. Retyping cannot reinterpret a generated
   thunk as a source literal. Source Function0 can contribute its function type
   before by-name lower bounds are solved. Keep typed source functions distinct
   from values returned by generated thunks in overload scoring.
4. Read a callable by-name parameter as its value type, and force it exactly once
   per reference during erasure. Function-valued results keep their own arity;
   forcing cannot remove an additional Function result layer. Runtime counters
   cover fresh thunk construction, forwarding and invocation separately.
5. A provisional type argument does not erase a real outer Class, Array or
   Function constraint. String-to-Base adaptation for the t5727 shape coexists
   with Map value widening without an unwanted Int-to-String conversion.
6. Hoisted local method signatures after an unresolved stable-val import are
   provisional. Recomplete them after the actual import, before later statements
   can call forward to the method. Keep lexical import boundaries and existing
   evidence symbols; context-bound signatures must not gain duplicate clauses.
7. Separate-compile private-copy probes exposed another root: access information
   was lost when eager classpath signatures were installed/replaced. This was
   not caused by the prelude copy guard. Keep JVM private/protected on erased
   fallback methods; supply own private Scala declarations from the full pickle
   and preserve their flags and qualified boundaries on installed methods.
   Signature completion retains private method descriptors separately from the
   Java API reader, which deliberately filters private declarations. Otherwise
   full-pickle installation fails and leaves the eager public approximation.
   Protected declarations also need pickle metadata: scalac emits their JVM
   methods public. Private[this] keeps LOCAL. No fictitious copy method is added.

8. Twirl function types such as `(=> Boolean) => Html` exposed a parser root:
   `=> T` was represented as Function0[T], losing the by-name parameter meaning.
   Preserve it as a by-name type marker and mark inferred lambda parameters
   BYNAME. Counter-based execution checks forwarding, nested function results
   and function-valued by-name parameters. This corrects the earlier hypothesis
   that the two Twirl regressions were merely missing generated-thunk marks.

9. Publishing Type.ByName as its inner type loses a function parameter
   boundary across separate compilation. Write scala.<byname>[T] in the pickle.
   Compare both producer compilers against both consumer compilers, executing
   function-valued APIs, ordinary by-name methods and generic function results.
   Before this repair both consumers of our function-valued API compiled but
   threw IncompatibleClassChangeError; a same-compilation check did not see it.

10. A by-name function domain is a parameter mode, not an ordinary method type
    argument. Eta inference cannot instantiate identity with ByName[Int].
    Preserve valid by-name method eta expansion and inferred lambda parameters.
    Unit value discarding wraps a source function value in a Unit-returning
    thunk; it must not execute that function. Enable the adaptation only for
    resolved calls, not as a way to make overloaded alternatives applicable.

## Evidence and validation boundaries

All new positive fixtures are compiled by real scalac 2.13.16 and scala-rs,
executed with java -Xverify:all, and compared byte for byte with the expected
stdout. ctxev adds eight test groups (16 total). Existing test expectations are
not weakened. New negatives compare acceptance in both compilers.

Immutable accepted-before binary: /tmp/scala-rs-declaration-boundaries/
final-prerequisites/scala-rs-895222eada7b741e. New by-name, fixed-structure and
local-import positive fixtures fail there. A source Function0 passed to => Int
is wrongly accepted there. Independent Function1..3 probes compile and verify
but throw IncompatibleClassChangeError before the change; named function values
are controls. Two separately compiled private calls are accepted before and
throw IllegalAccessError when executed. Protected access from an unrelated
caller is wrongly accepted and prints x; real scalac rejects it.

Evidence: /tmp/scala-rs-copy-byname (immutable candidate binaries, source patches,
probe stdout, compiler/runtime logs and phase terminal results). Prior isolated
probe evidence remains under /tmp/scala-rs-contextual-recovery/next-probes and
next-byname-functions. Initial early measures had cats 34/23 versus accepted
83/34, library 387/107 versus 415/113, Slick 0/1504 with all1504 strong loads.
Gitbucket initially had two new Twirl locations; preserving by-name function
parameter types repaired them. Renewed measurements reached gitbucket 92/43,
cats 34/23 and library 387/107 with no new diagnostic locations, plus Slick
0/1504 and all 1504 strong loads. Later eta/Unit repairs require renewed checks.
These are intermediate results, not an accepted baseline.

Mandatory first checks: mapkey, cats3, tupletailrec, preludefidelity, ctxev and
byname_followup. After the imported-access repair the selection expands to the
classpath/pickle/default/constructor/access seam suites, all mapped copy targets,
full negatives and all historical corpus loss names. Include t5727 and t9223b.
The corpus filter is a zsh glob, not a regular expression; verify every requested
identity and all 1405 negative identities, including names with a plus sign.
Gate preflight checks pinned source revisions, jar CRCs, Java cache bytes and
reference class hashes. Monitor owned phase processes through terminal results.

A grouped probe using adjacent brace expressions on successive lines exposed an
existing statement/argument parsing difference; explicit semicolons keep this
fixture focused on local-import signatures. That separate parser root is not
claimed repaired by this batch. Import-dependent calls preceding the import
statement itself also need further lazy-completion coverage.

Integration status: work in progress. No full gate has yet run for this batch.
No compiler change has been merged to main and no push has been attempted.

The first wider prerequisite pass completed 67 CLI suites: 957 passed, one
ctorgaps failure. Private constructor descriptors belong to the dedicated
constructor completion path and are excluded from the new ordinary-private
method reader. Clippy remained 57 existing warnings, zero added. No corpus or
full gate ran on that prerequisite candidate. The combined repairs require a
new immutable binary and a renewed prerequisite pass.

The next prerequisite candidate passed 66 CLI suites (949 tests), parser 67,
backend 58 and typer 190. All 2492 selected corpus identities were present,
including all 1405 negatives and historical loss names, but neg/t7899 changed
from rejection to acceptance. No full gate ran. Normalizing expected eta
domains fixes the inferred ByName[Int] type argument responsible for that loss.
A paired Unit-discard probe found a silent error in the accepted-before binary:
scalac prints 0/0 while scala-rs prints 1/1 by executing a supplied Function0.
Both issues were repaired together; the new ctxev group also preserves valid
by-name eta expansion and rejects Unit/String overloads which scalac refuses.
Evidence directories retain both unsuccessful prerequisite runs.

Final prerequisites (checked-prerequisites) passed on the immutable binary
recorded in binary.json: 51 early CLI tests, 949 additional CLI tests, parser
67, backend 58 and typer 190; fmt clean; clippy 57 existing occurrences and no
new warning. Selected corpus: 2492 unique identities, all 1405 negatives and
all 44 historic loss names, zero losses. neg/t7899 rejects again. Preserved
gains include pos/t5727 and run/phantomValueClass, t7859 and valueclasses-pavlov;
run/t7120 also passes. Final early measures: gitbucket 92/43, cats 34/23,
library 383/107; no new diagnostic locations, no added cats diagnostics. Slick
has zero errors and 1504 classes, all 1504 loaded with full JVM verification.
Input health checks passed on four source trees, 121 jar archives, 33 cached
Java classes and 1498 reference classes. Full gate is still required.

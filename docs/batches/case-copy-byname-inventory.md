# Next composed batch inventory

This inventory is written while e5df7b09's frozen full gate runs. Do not edit
its compiler tree or restart the gate. Wait for DONE, independently audit all
results and preserve the raw rejected ledger before the next implementation.
Accepted baseline remains e608c7dc. No subagents or aggregate before run.
Every diagnosis below is a hypothesis: correcting it through measurement is
more valuable than following an incorrect brief.

| Repair candidate | Measured evidence | Mechanism and boundary to preserve | Difficulty |
|---|---|---|---|
| Prelude case-copy declaration boundary | e5df7b09 adds 53 cats diagnostics, all missing Tuple copy. Grouped Tuple1..22, Some, Left, Right: nsc and accepted-before execute; candidate refuses. Product2 copy rejects all. | The new rewrite guard requires an own SYNTHETIC copy, whereas prelude_tuple only marks CASE/ctor_fields and historically relies on the rewrite. Source and imported classes with user/private/inherited copy must still suppress synthesis. Inspect real library declarations and existing prelude case metadata, not a blanket bypass for every binary case class. | medium, mandatory regression |
| Single-candidate diagnostic regression | cats3::fixtures_c3_infer_bad_is_rejected still rejects both bad statements, but changes Box[Unit] type mismatch to no matching overload. | Contextual applicability must rescue the previously inapplicable HK call without dropping a candidate previously applicable before its precise argument adaptation. Consider ordinary applicability first and invariant-result applicability as a singleton fallback; retain actual argument checking and all c3 controls. | medium, mandatory regression |
| Local stable-val type-alias imports | val o=new Obj and val o:Obj=new Obj, wildcard and named import, all execute 7 under nsc but refuse Event's member n in accepted-before/candidate. Module-prefix control now passes candidate. | Hoisted local-def signature sees an import whose local prefix is not yet completed; Event is cached as an unresolved name. Explicit val annotation did not repair it, so do not assume simple missing val type. Trace imported alias and pending declaration scopes. | medium/unresolved |
| Literal Function0 passed by name | hold[A](a: =>A):A with declared result ()=>Int refuses Int required ()=>Int; named val function control executes7 in all. | Source Function0 and generated thunk are conflated in adaptation/re-inference. Prove source-vs-generated provenance rather than guessing from arity. | medium/high |
| By-name function parameter invocation | def twice(f: =>()=>Int)=f()+f() is accepted by nsc, prints3/2 with counter, but both rs binaries refuse apply on =>()=>Int. | Callee typing retains ByName under dummy Method expectation, then resolve_overload has no ByName path. Preserve thunk forcing and repeated evaluation in erasure, not just type acceptance. | medium |
| Wrong literal function accepted as Int by name | f(x: =>Int) accepts f(()=>7) in both rs binaries; nsc rejects. Wrong function-result-type control already rejects all. | arg_score's raw Function0 fallback and adapt's Function syntax guard accept a source function as an already generated thunk. | shared with provenance, medium/high |
| Map function-valued fallback wrong answer | Both old and candidate compile getOrElse(missing,()=>3)() but throw Integer-to-Function0 CCE; nsc prints3. | Same provenance family may also need by-name inference/unwrap paths and function-valued results checked. | medium/high |

Evidence: /tmp/scala-rs-contextual-recovery/next-copy-inventory/results.json,
next-probes/results.json, value-prototypes/results.json. Every accepted nsc
program was executed with -Xverify:all; all positive stdout is retained.
Local source reference is real v2.13.16 under /tmp/scala-rs-corpus-20260910-codex.

The later copy rewrite change expanded the scope after prerequisite selection.
Existing tupletailrec (tt_tuple/tt_tuple_bad), preludefidelity (pf_case/_bad)
and cats3 were omitted from prerequisites. This is a selection failure, not a
missing test. Enumerate fixtures that exercise every newly touched mechanism
and map them back to their test targets before finalizing the next batch.
Keep mapkey first, all previous ctxev groups and new negative controls; include
all copy-related suites from next-suite-inventory.json, the shared seam suites,
full neg and every historical corpus loss. Do not weaken expected diagnostics.
Run all four compile measures early after this failed-measure gate: they are
short and the generated cats Tuple surface must no longer wait until after
minutes of unrelated checks. Use owned paths and immutable after binary;
never remeasure aggregate before.

Do not merely spend a new full gate on the Tuple repair alone. Group supported
additional repairs above with both mandatory regressions. Keep Java Object,
source-class macro universe and full-run normalization roots separate unless
new evidence makes a bounded repair concrete.

Further source inspection: prelude_tuple.rs:83 explicitly documents that CASE
and ctor_fields back the constructor rewrite, without adding a copy member.
Requiring SYNTHETIC for this prelude representation was an invalid assumption.
Do not solve it by advertising an unimplemented private-runtime copy method;
the existing constructor rewrite is its implementation. A source-only
suppression guard or explicit copy-generation metadata may preserve that
representation, but must be tested against imported custom/private-copy classes.

For by-name provenance, consider a dedicated Tree metadata field rather than
reusing source IDs, scala_ref or stable_pat. FILLED_ARG/PRETYPED_DEFAULT IDs
already have distinct retyping semantics; a new sentinel could collide with
those or named-argument evaluation order. Track actual generated thunks through
adaptation, parameter prototypes, retyping and inference; plain Function values
must retain their own arity/result. A callable by-name parameter also needs
forcing before its returned function is applied. Existing byname_followup,
tail recursion, parent t9223b and named/default argument controls are mandatory.
No such implementation has been made in the frozen e5df7b09 tree.

The additional Function1/2/3 by-name probes are now executed, not hypothetical:
real scalac prints 7 in each; both accepted-before and e5df7b09 compile and pass
JVM verification but throw IncompatibleClassChangeError on executing hold's
Function0 call. A named Function1 value prints7 with all three compilers.
Evidence: next-byname-functions/results.json. Thus the provenance problem is
not limited to arity zero, Map or ClassCastException. The dedicated metadata
must distinguish source function literals of every arity from synthesized
zero-argument thunks without weakening rejection of f(()=>7) at f(=>Int).

Reference follow-up: real v2.13.16 UnCurry.scala:346 wraps by-name arguments
based on the formal declaration rather than treating arbitrary function
syntax as an existing thunk. scala-rs erasure.rs:1336 also strips a Function's
result when forcing BYNAME identifiers; simply changing callee typing to a
plain inner Function type may therefore lose another function layer. Keep
runtime counters and returned-function arities through the erasure boundary.

Final full-corpus comparison to accepted e608c7dc has zero losses, but the
previous rejected candidate's pos/t5727 gain is no longer present. Keep it in
the next prerequisite list. Its by-name `bar[U >: T](a: => Base[U], default)`
requires a String-to-Base implicit conversion: the outer Base constructor is
a real formal constraint, unlike Map's entirely open value parameter whose
lower bound must not force conversion. The provisional-value view suppression
may be too broad for this shape; verify through a runtime reduction with a
working Base implementation, including wrong-result and Map widening controls.
Do not call zero losses against the accepted baseline preservation of every
unmerged gain. This is an additional known boundary to recover.

Mapped existing copy-related targets: `caseabi`, `caseeq`, `gaps`, `lang`, `mismatch13`, `mismatch9`, `namedargs`, `patbind`, `pickleparams`, `preludefidelity`, `tail3`, `tail5`, `tupletailrec`, `unitbox`, `xflags`. Also retain `ctxev`, `cats3` and the shared seam suites.

Frozen gate e5df7b09 completed: VERDICT=FAIL, workspace 2786 passed/5 failed,
full corpus 5324 unique identities/0 losses/3 gains. Accepted baseline remains
e608c7dc. Full quantitative results and exact summary are in tests/BASELINE.md.

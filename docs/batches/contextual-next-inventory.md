# Remaining-family inventory after contextual/by-name integration

This inventory was prepared while the composed gate was frozen. Start the
next batch from current local main and read BASELINE first. These are
hypotheses, not measured fixes. Correcting a hypothesis through real scalac 2.13.16 probes is
more valuable than implementing the proposed diagnosis. No subagents.

Accepted measured errors are GB92/43, cats34/23, library383/107. Candidate-specific
remaining diagnostic records: cats-remaining.json and gitbucket-remaining.json.
Use the accepted compiler from BASELINE for aggregate before, never rerun it.

| Family | Current evidence | Required next investigation |
|---|---|---|
| Java Object override compatibility | Two GitBucket Migration overrides cause four errors | Previous probe proved nsc accepts both Map[String,AnyRef] and Map[String,Any] against Java Map<String,Object>; wrong key rejects. A simple Any-to-AnyRef change was removed because it failed the opposite acceptance direction. Model the Java-specific compatibility relation; preserve HeapBackend regression. |
| Curried method eta expansion | cats Apply.ifA passes ite(Boolean)(A,A) to map | Check nested eta expansion and contextual function result, runtime counters; include partial application, default and by-name controls. No new reduction measured yet. |
| Right-associative placeholder inference | cats Reducible and NonEmptyList/Seq/Vector infer NonEmptyList[AnyRef] for ior.map(c :: _) | Trace evaluation order and prototype after desugaring, with left-associated explicit-lambda controls. Existing source locations alone do not prove the cause. |
| Full-run tuple identity | Two cats swap errors compare Function[Tuple2[C,B]] with structural tuple result | Already reproduced outside full run as PASS in several older batches. symbol.rs already handles Class/Tuple subtyping in both directions. Do not add another blind tuple special case. Find first symbol-origin or owner-substitution divergence in full cats. |
| Full-run IterableOnce and SortedSet | cats Semigroup.reduceOption and NonEmptySet.toIterable | IterableOnce.reduceOption already passed isolated runtime probes. Inspect source-vs-binary completion/order; no missing-prelude stub. |
| Nested implicit extension resolution | cats function1 andThenF/composeF and generated semigroupal builders | Use actual nested syntax owner/context and inherited method evidence, then shrink while preserving failure. |
| Source-class macro universe | 31 GB diagnostics explicitly refuse mapTo on current source classes | High complexity: actual source symbols in the reflective universe are necessary. Keep separate from bounded batch unless a supported complete integration is identified. |
| Remaining HK/path dependent families | cats Parallel, ContT, EitherK/T, IorT, Nested, Kleisli | Different errors are not necessarily different defects. Compare symbol owners, constructor kinds and receiver substitutions before choosing a common repair. |

Known non-error boundaries for next probes: imported and published access modes,
function result ABI, values crossing generic/primitive storage, aliases and
shadowed declaration identities. Execute accepted probes -Xverify:all and byte
compare, check opposite rejection direction, and prove failures in the immutable
pre-change binary. Inventory multiple independently supported low/medium fixes
before editing; one combined prerequisite pass and one final composed gate.

Read-only source follow-up for Java Object: real v2.13.16 TypeComparers.scala
151 gives ObjectTpeJava special equality with Any, while its ordinary symbol
stays Object. Subtype handling at line 462 treats it as Any before erasure.
scala-rs classpath.rs:1937 explicitly leaves generic Object arguments as Any
after an earlier HeapBackend regression. The existing Type::JavaObject is
used for array elements and is_sub_type currently maps it to AnyRef on both
sides. Reusing that variant without preserving array/field semantics would
change an existing ABI boundary. Trace Java generic type origin first; an
override-only Any/AnyRef equivalence would also wrongly affect Scala-created
generic arguments unless the Java origin is retained. No implementation made.


The parser marker and quasiquote failures found at287ec595 were corrected
in712279e5 together with source-name collisions and Unicode by-name syntax.
They are not outstanding repair targets. The accepted compiler preserves all
11 corpus gains; all9 run gains agree with real scalac byte for byte. The
remaining nested by-name quasiquote reification is still unsupported. History
and failed-candidate evidence are preserved in byname-marker-inventory.md and
BASELINE; avoid repeating standalone probes already recorded as agreeing.

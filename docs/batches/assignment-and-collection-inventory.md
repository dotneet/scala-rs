# Next batch inventory: setter calls and result inference

This batch starts from `main` at `4b28fb05`. The accepted compile and corpus
measurements in `tests/BASELINE.md` are the reference; they are not rerun as a
new before measurement.

Every diagnosis below is a hypothesis. The reduced probe against real scalac
2.13.16 and `java -Xverify:all` is the deciding evidence, and correcting a
hypothesis through measurement is the main value of this slice.

| Candidate | Measured evidence | Mechanism and boundary | Difficulty |
|---|---|---|---|
| Selection assignment to a getter with a hand-written setter | `class C { val v: Int = 1; def v_=(x: Int) = println(x) }`; scalac accepts `c.v = 2`, while scala-rs reports `reassignment to val`. The same source shape for an object wildcard variable emits `putfield` instead of the ABI setter. | `setter_assign_lhs` only considers a selected `Method`. Source fields and imported object variables can resolve as `Term`, even though a `name_=` member is available. Rewrite a selected term or accessor only when the receiver type has a matching setter; preserve ordinary immutable-field reassignment diagnostics. | low, selected |
| Wildcard-imported object variable assignment | `import O._; ov = 5` must call `O$.ov_$eq(I)` under scalac. scala-rs currently writes the field directly for an object defined in the same run, which is ABI-wrong even when the generated field happens to be public. | Reuse the selected-setter rewrite after `qualify_term_import` has materialized `O.ov`; no new import-prefix heuristic is needed until a probe demonstrates one. | low, selected |
| Result-only type parameters in value applications | `val x = Ior.right(NonEmptyList.one(1))` is `Ior[Nothing, NonEmptyList[Int]]` under scalac, while scala-rs kept the unconstrained `A` and rejected an assignment to `Ior[String, ...]`. | A completed application has no later argument that can solve a parameter appearing only in its result. Instantiate it at the lower or upper bound; applications typed as a callee still receive the dummy `Method` expectation and remain open. | medium, selected |
| Generic receiver element guessing | `Ior[String, NonEmptyList[C]].map` was given `String => ...` by the collection lambda prototype, because the fallback treated every generic class's first argument as its element. | Keep the first-argument fallback only for collection JVM classes when `IterableOnce` metadata is not loaded. Ordinary generic receivers retain their declared method parameter types. | low, selected |
| Nested case-class patterns from Scala jars | `case Ior.Right(r)` was resolved to the prelude `scala.util.Right`, and the jar class's empty parent list prevented its field type from being recovered. | Resolve nested companions by their exact JVM class name and load pickled parents before deriving pattern arguments. | medium, selected |
| Cats tuple swap diagnostics | Two full cats errors compare `Tuple2[C,B]` and structural `(C,B)`, but direct and function-valued tuple probes already agree with scalac. | Not a tuple rule: the two `C`s are different type parameters printed alike. `first(fa).dimap(f)(_.swap)` solved `first`'s `C` from the first clause and the second clause was read off an unsubstituted `fun.ty`. Fixed by `agent/catsrest` (`docs/cats.md`, "The last 27", root 2). | resolved |
| Cats `NonEmptyList[AnyRef]` diagnostics | The reduced `nonEmptyPartition` probe initially reproduced the `AnyRef` and `Ior[... , B]` errors. After the three selected fixes above, all four `NonEmpty*` diagnostics disappeared from the cats measure (31→28 total errors). | Keep the probe in `cats3.rs`; the remaining cats errors are separate higher-kinded/type-class roots and are deferred. | medium, resolved |
| GitBucket `value _1 is not a member of A` | Six errors are concentrated in `IssuesService.scala` after a query result loses its tuple shape. No standalone source reduction has yet separated Shape inference from tuple projection. | Likely downstream of unresolved Slick `Shape`/query types. Fixing tuple members without preserving the query result type could accept a wrong program, so keep it out of this low-risk batch. | high, deferred |

The selected rows were implemented together. The existing `varassign`,
`mapkey`, `preludelb`, and cats inference suites are the focused regression gate
before one full `tests/verify_merge.sh` run.

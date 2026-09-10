# Evidence and default imports, based on 75f86f78

The starting diagnostics were hypotheses, not a repair specification. Inventory
probes use real Scala 2.13.16 and the rebuilt, accepted 3343f368 compiler.
Aggregate before figures remain those in tests/BASELINE.md.

| Candidate | Measured result | Decision |
|---|---|---|
| Inferred toMap in an ordinary callback | Both compile and run | Excluded |
| JDBC result values / Java String pair keys | Both compile and run | Excluded |
| Lazy val signature completion | Both compile | Excluded |
| Inferred toMap val in a by-name block | nsc runs; accepted compiler rejects | Isolate the val inference context |
| Bare Predef Manifest/OptManifest aliases | nsc accepts; accepted compiler misses names | Complete default Predef imports lazily |
| Primitive and phantom Manifest values | nsc runs; accepted compiler rejects | Use canonical factory values |
| Class Manifest with nested type arguments | nsc runs; accepted compiler rejects | Preserve recursive evidence |
| Array Manifest and generic element witness | nsc runs; accepted compiler rejects | Use arrayType with recursively found evidence |
| Generic List Manifest with a context witness | nsc runs; accepted compiler rejects | Search scope before synthesizing children |
| Singleton and intersection manifests | nsc runs; accepted compiler rejects | Use runtime factories |
| Wildcard type arguments | nsc uses upper bounds | Match measured nsc trees |
| Abstract, bounded and nested abstract type without evidence | Both reject | Keep negative tests |
| Generic OptManifest without evidence | nsc supplies NoManifest | Use the real library fallback |
| Bare ClassManifest alias | Both reject in 2.13.16 | Excluded; it is not a Predef alias |
| Non-tuple case class accepted as toMap pair | Both reject | Excluded |
| Simple Java Map Consumer override | Both compile and run | Does not reproduce the full-run override failures |

Evidence is in /tmp/scala-rs-evidence-batch-probe/inventory/results.json,
byname-results.json, and adjacent source, compiler and runtime logs. The trace
of the full gitbucket source set found two delayed toMap applications: no
implicit search was attempted while typing those val initializers. The cause
was the outer typing_call_args state, not a missing collection member or
failure to find reflexive conformance evidence.

Full manifests for path-dependent inner classes need preserved type prefixes;
TypeTag/Manifest interoperability also needs a separate investigation. These
are deeper representation changes and are not approximated by an erased tag.

The recursive probes also exposed premature implicit application of a TypeApply
callee. A local Manifest[A] could settle m's still-open parameter before the
written m[List[A]] was read. A one-expression flag now keeps that reference
unapplied until its explicit type arguments have been substituted. The normal
Apply path and computations inside a selected receiver retain their behavior.

The default-import probe corrected another hypothesis: loading Predef alone was
insufficient. Alias installation tried to resolve a module as a non-module
class. The alias now belongs to the declaring owner already identified by the
pickle lookup. This also exposes the higher-kinded Predef.Function alias.

Boundary probes distinguish partial array manifests with and without ClassTag
and partial intersections. A missing abstract array element ClassTag produces
NoManifest, while a supplied tag produces the matching array manifest. Full
instance-member singleton manifests remain unsupported because source singleton
types lose the instance path; they are diagnosed, not constructed on this.

Pre-gate checks: 549 tests in 15 related suites passed, including the required
pickle/prelude supply regressions and e2e. After the partial-array/intersection
boundary correction, evidencebatch, classtag_inference and macrotag passed
again (6 tests). The 18 recorded corpus regression identities are unchanged,
with losses=0. Release workspace clippy retains exactly the same 57 warnings.
Preflight passed all four pinned source trees, 121 jar CRC checks, 33 Java class
byte comparisons and 1498 known-good scalac reference classes. Four runnable
fixtures match real scalac stdout byte for byte; the three repaired behavior
fixtures fail compilation with the rebuilt accepted 3343f368 binary. The fourth
fixture preserves explicit-import shadowing and passes both before and after.

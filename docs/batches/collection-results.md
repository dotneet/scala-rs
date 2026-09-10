# Collection result and Java member batch inventory

Base: main ca84fa4f; accepted compiler 9cc076f6, immutable binary SHA256
f92a0f1db75db9a61f9ec978b5adcbf6557fc157ad93da3ffc31f66b4d1d031a.
No aggregate before measurement is repeated. No subagents are used. Every
mechanism below is a hypothesis to correct from measurement; reproductions
and real scalac 2.13.16 runtime logs live under /tmp/scala-rs-collection-results.
Commands use actual Temurin 17 and UTF-8, matching the recorded baseline.

| Family | Independent evidence | Mechanism to investigate | Difficulty |
| --- | --- | --- | --- |
| Vector/LazyList grouped and sliding | Alone matches nsc; ArraySeq warming rejects valid Iterator[receiver] results | Receiver substitution for inherited C/CC, declaration identity and JVM origin | Medium/high |
| List/Vector scanLeft and scanRight | Same warm-only loss; wrong result independently rejected | Same inherited result family | Medium/high |
| Vector tails / LazyList inits | Same warm-only loss with actual runtime oracle | Same inherited result family | Medium/high |
| SortedMap.keySet | Refuses valid SortedSet result alone and warmed; wrong key rejected | Covariant result override suppressed by parameter-only guard | Medium |
| SortedSet .to factory | Both alone/warmed refuse valid conversion; nsc executes | EvidenceIterableFactory parent edge and extra implicit clause | Medium |
| IterableOnce.reduceOption | Compiles but ClassCastException on both input orders | Library value classes excluded from recovered AnyVal representation | Medium/high |
| LazyList.mkString overloads | Alone matches; warming compiles then VerifyError for bare mkString | Supplied overload identity and descriptor selection | Medium |
| String getBytes/getChars/codePointAt | Real nsc executes; accepted compiler refuses | Canonical String Java members deliberately not completed | Medium |
| String.lines precedence | nsc accepts Java Stream.count and rejects .toList; ours does the reverse | Standing comment about preserving StringOps is wrong on pinned JDK17 | Medium/high |
| Gitbucket Java source supply | All three actual Java sources compile with real javac | Measure omits Java compilation; candidate input correction must be separated from compiler gains | Low/medium |
| Excluded PullRequestsController | Actual source parses with both compilers; value/guard runtime probe matches | Exclusion rationale is obsolete; full source coverage must be restored | Low |

BitSet union/intersection/difference and SortedSet union/intersection/flatMap
are passing controls in this specific warm context. Their real-cats messages
are not yet reproduced and must not be claimed as fixed. The deeper repeated
nested-variable constraint solver remains separate. Prior unmerged fullrunonly
code is reference only: its HashMap.collect, Map ++ and List.length failures
must remain prerequisite regressions, with no blind cherry-pick.

The PatchUtil runtime probe reached a distinct String.getBytes missing-member
error after Java compilation; it has not yet established full scala-rs runtime
parity. This corrects the initial source-only diagnosis. The independent
IterableOnce and mkString runtime failures do not appear in error counts.

Select the connected collection result/overload/value-class and factory
mechanisms together, alongside String Java supply and verified input coverage
corrections when ready. Preserve declaration ownership rather than merely
changing casts or expected types. Every valid fixture must run with -Xverify:all
and byte-identical output; independent invalid programs and immutable before
failures accompany them. Implement the batch before broad verification.

Before a full gate: affected existing suites and the entire supply-seam list,
all historical corpus losses and full negatives, source/jar/cache preflight,
Slick runtime/strong verification, and clippy warning comparison. A full gate
runs once on a frozen composed tree and is followed through DONE; preserve its
full ledger and record even on rejection. Changing gitbucket measurement inputs
requires reporting the comparable old-input measure separately from expanded
coverage and updating the source-count gate from actual evidence.

## Implemented composition and corrections from measurement

- Read receiver-substituted collection declarations without companion or
  ancestor fallback; accept only the inherited declaration or a more-derived
  origin for an existing erased shape. Preserve modeled prelude candidates.
  Grouped/sliding/scans/tails/inits now execute in alone, warm-first and
  warm-last order. HashMap.collect, Map concatenation and Set.toSeq/List.length
  remain mandatory neighboring regressions.
- Resolve duplicate nullary overload types before choosing the actual symbol;
  warmed LazyList.mkString no longer invokes the three-argument descriptor.
- Recover AnyVal for unmodeled scala-library classes as for external libraries.
  IterableOnce.reduceOption now executes. The first prerequisite detected
  Duration syntax double boxing: these three lazy NewWrapper models are created
  after prelude_end, contrary to the initial assumption. Preserve their explicit
  boxed representation; do not exclude all scala-library value classes.
- Model SortedMap.keySet's actual refined result and JVM SortedSet descriptor.
- Complete Java members for canonical String, including literal types, before
  implicit views. getBytes/getChars/codePointAt and Java Stream lines execute.
  The old string_ops4 fixture used lines.next(), which actual JDK17 scalac
  rejects. Preserve this as a negative probe; the positive fixture now uses
  linesIterator with unchanged output verified by scalac. Its historical
  'dual run' helper only ran scala-rs output against the jar, not scalac.
- Link six sorted collection companions to their evidence-bearing factory
  traits and read the actual pickled conversions. The conversion's result
  already solves element types; use that solution for implicit arguments
  absent from the receiver. SortedSet/TreeSet and SortedMap/TreeMap factories
  execute, including mutable TreeSet/TreeMap. Missing Ordering and wrong result
  types reject independently. Ordinary result-constrained conversions remain
  passing controls. ArraySeq's lazy ClassTag factory edge is still unresolved
  and excluded from this composition; no dummy Factory or ClassTag is installed.
- Restore all 354 gitbucket Scala/Twirl sources and compile its three real Java
  helpers with javac per invocation. PatchUtil now executes with real JGit and
  exact scalac stdout. Historical input comparison uses the explicit 353-source,
  Java-disabled switches on the candidate binary, never a repeated before run.

Raw oracle and immutable-before evidence: inventory-results.json,
string-inventory/results.json, factory-inventory/results.json and
string-legacy-original.log under /tmp/scala-rs-collection-results. The original
14-family matrix covers 56 compilations and all successful outputs execute;
additional Java/String/factory probes are separate. Accepted compiler and its
binary hash are recorded at the top. A full gate has not started yet.

Prerequisites on the composed compiler: 684 tests in 32 related suites pass;
1288 selected pos/run corpus rows and all 1405 negatives have zero losses and
zero status changes against 9cc076f6. Clippy retains exactly the same 57 warning
identities. Source/jar/Java/reference preflight passes. Candidate gitbucket under
historical inputs remains 115 errors / 54 files (353 sources, one excluded,
Java disabled). This is distinct from the expanded input measure in the gate.
The first failed duration prerequisite was repaired before this successful set;
no full gate was spent on it. Full-gate results belong in BASELINE.md and the
owned gate directory; no success is inferred from these prerequisites alone.

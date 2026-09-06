# Scala 2.13 compatibility: development and acceptance

The target is observable Scala 2.13.16 behavior: source acceptance, useful
diagnostics, program output, and interoperability with separately compiled
Scala and Java code. A falling error count is evidence to investigate, not
the definition of correctness. Scala 3 and compiler plugins remain separate
scope decisions; their absence must not be confused with a Scala 2.13 pass.

## Starting point

`main` at `46a66d0` has the same implementation as the measured `902da04`.
Only `HANDOFF.md` and `tests/BASELINE.md` differ. Use that table as the
reference; do not rerun the unchanged baseline. Existing unmerged work is
preserved on `agent/implicitmemo`, `worktree-agent-a44905be6f76c9f6a`, and
`agent/catstail3`.

## Order of work

1. Recover and review those three branches in their existing worktrees.
   Preserve uncommitted work, merge local `main`, then run release workspace
   tests, relevant compile and execution checks, and the full corpus.
   Investigate losses individually, including a rejection for the wrong
   reason becoming a different, correct diagnostic.
2. Independently review and validate the proposed integration before moving
   `main`. Record the tested commit/tree, process exit status, complete logs,
   and any differences from the table. Aggregate pass counts cannot substitute
   for comparison by test identity when a complete reference log is available.
3. Fix silently wrong behavior first. Tail recursion must actually eliminate
   stack growth; cached implicit searches must preserve the selected witness
   and its receiver as well as the inferred type. Check output against scalac.
4. After implicit search is both correct and fast, revisit the deferred
   gitbucket import-resolution change with a bounded performance measurement.
   A fast cache alone does not demonstrate that the additional search is viable.
5. Implement specialization as a compiler phase with explicit source, JVM
   descriptor, emitted-name, and separate-compilation tests. Emitting names
   without the matching methods, constructors, or dispatch behavior is not a
   completed stage. Keep the specialization ledger red until its actual
   obligations are met.
6. Design current-run macro symbol/type queries before expanding `mapTo`.
   A placeholder class cannot answer case-class members, accessor types, and
   companion methods truthfully. Confirm the protocol and symbol lifecycle
   before changing macro expansion.
7. Reclassify the remaining corpus failures using current diagnostics and
   execution traces. Separate unsupported harness inputs, missing language
   behavior, erroneous rejection, and wrong runtime output. Implement one
   reproduced cause per slice; do not predict yield from test counts.

## Execution rules for Codex

These rules supersede conflicting historical process advice in
`.agent-brief.md`; its semantic and regression lessons still apply.

- The coordinator assigns an explicit worktree and branch before delegating.
  Every command uses that worktree. Agents never change the parent checkout.
- Existing worktrees retain their branches. New branches start at local
  `main` and use `codex/`. No shared `git stash` and no in-place A/B revert.
- While several branches run, use `CARGO_BUILD_JOBS=2`,
  `RUST_TEST_THREADS=2`, and `CORPUS_JOBS=2` per branch. Adjust total workload
  centrally. Compilation and targeted investigation can proceed in parallel;
  unnecessary duplicate full suites cannot.
- Track long commands through their tool session IDs and exit statuses.
  Do not detach a command and wait indefinitely for a sentinel. The existence
  of a log is not evidence of completion.
- Put all writable measurement outputs under a branch-specific directory.
  Keep the slick class output outside a measurement script's temporary run
  directory if it will be verified afterwards.
- Rebuild before directly invoking a compiler binary. Record any use of an
  explicitly supplied `SCALA_RS` binary and which tree produced it.
- Commit the final tested tree. An edit made after testing invalidates the
  affected evidence. The coordinator independently checks it before merging.
- Update the baseline only from completed integration measurements. Keep
  unmeasured or deliberately red checks explicit; do not infer their results.

## Acceptance gates

- Release workspace tests and focused positive/negative regressions pass.
  Formatting and lint introduce no new warnings.
- A source fix rejects invalid programs at the intended construct and accepts
  the corresponding valid form. Merely rejecting a negative test is not enough.
- ABI/code-generation fixes compare scalac-only execution with scala-rs-only
  execution and both directions of separate compilation where applicable.
  Exercise methods; class loading does not prove dispatch or access correctness.
- Slick has nonempty expected class output, passes structural checks and JVM
  verification, and all differential execution attempts agree. Report classes
  whose loading could not complete separately from successful verification.
- Corpus logs are complete for the selected population before comparing them.
  Investigate changed test identities and output, not just net totals.
- Performance fixes report the same source set, settings, result, and timing.
  A compiler crash, timeout, or empty output is never `errors=0` success.

The project is not complete while the documented Scala 2.13 language, macro,
specialization, ABI, and execution gaps remain. Progress reports must state
which gates were reached and which obligations remain open.

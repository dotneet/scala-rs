# Pattern validation and lint followup

The independent e521f47 audit confirmed five additional positive corpus cases
compile with both compilers and two run cases have identical strict JVM results.
A driver for t11406 calls both update functions and prints 9/9 with both
compilers. Observable controls also agree for the mixed collection comprehension,
dropWhile's result and predicate visits (1,2,3), and every pair of a 100-element
HashMap merge. Evidence: /tmp/scala-rs-codex/integration/gain-audit-e521f47.

Clippy at e521f47 reported 60 warning messages against the accepted baseline's
58. Two new warnings were clones used only to construct borrowed slices.
The extractor helper also had nine arguments where the previous warning was
for eight: its fun and args already reside in its UnApply pattern tree.

The followup borrows the two type slices and derives extractor inputs from the
single pattern argument. The helper is now private with one caller, guarded by
the caller's UnApply match arm. Its arity falls to seven, removing the existing
warning instead of suppressing it. lint-repaired-audit.json reports 57 warnings,
no added messages, and removal of the old eight-argument warning.

The earlier e521f47 worktree stays frozen for its running full validation. This
followup is validated in its own worktree and must not be represented as the
same commit. lint-focused.log is the focused sequence/binder/Singleton check;
full final acceptance remains pending. A separate copied Slick execution cache
keeps the validation runners from writing to the same directory.

# Override Checking for Universal Methods from External Definitions

The full test for candidate e9e17219 failed at
`inherited_binary_members_match_scalac`. `Ops.hashCode(): Int` from the full
pickle and `Any.hashCode: Int` from the handwritten prelude remained as
separate candidates, so `println(c.hashCode)` reached code generation with an
unresolved overload type. The JVM rejected the point where an `Int` was passed
as an `Object` with a `VerifyError`.

`drop_overridden` compared owner relationships only as class symbols, while
parent types use `Type::Any`, `AnyRef`, and `AnyVal`. The comparison now maps
those three owners to their corresponding type representations and removes a
parent member with the same signature from the candidate set. The rule uses
the prelude symbol IDs rather than simple names, so user classes with the same
names are not affected.

The existing ifacebridge tests compare a class and an anonymous class that
inherit traits compiled by scalac, and also inspect bridge descriptors for
covariant return types. Both tests passed after the fix. Evidence is in
`/tmp/scala-rs-codex/integration/inherited-boxing-audit/repaired.log`。
The related 52 tests also passed (`related.log`). Strict JVM verification is
recorded in `strict.out` and `strict.err`. The library measurements were 346
Cats diagnostics, 895 GitBucket diagnostics, 0 Slick diagnostics across 1490
classes, and 1557 Scala-library diagnostics. The four diagnostics removed from
GitBucket were all false reports caused by `toString` remaining as an
unresolved overload after override checking. Full-gate acceptance was still
required separately.

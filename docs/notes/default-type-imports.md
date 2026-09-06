# Default type imports after term completion

Parent candidate based on fd3a4ff8, not accepted into main.

Loading a qualified Stream companion before a later unqualified Stream type
made type lookup return the module instead of scala.package's polymorphic type
alias. `expose_unqualified` stopped at the existing term; the type-only fallback
searched explicit wildcard imports and enclosing source packages but omitted
default scala/java.lang imports. Raw traces showed a TypeMember with one type
parameter before completion, and a ModuleRef with no type parameters afterward.

`expose_unqualified_type` now completes the default packages only after nearer
type bindings have been searched, and enters only real type symbols. Existing
term bindings remain intact. The source span is forwarded by both callers.
Temporary tracing was removed before the focused tests and measurements.

Validation: Temurin 17, UTF-8, Cargo/test jobs 2. Five tests pass: aliaslookup 2,
applied_collection_names 1, default_type_imports 1, late_factory_inference 1.
The new fixture compiles and runs with both nsc and scala-rs under -Xverify:all;
Int and String outputs match, and an invalid generic result remains rejected.

Against fd3a4ff8: cats 350/81, GitBucket 902/112 and Slick 0 errors/1490 classes
are unchanged. Scala library 1856/203 -> 1855/203: only the missing Runtime type
at scala/sys/package.scala:48 is removed, with no added diagnostic. Evidence is
/tmp/scala-rs-codex/integration/stream-prepend-probe/default-import/ and
../default-import-focused.log, ../warm-results.json, ../alias-forms-results.json.
Full workspace, corpus and execution gates for this candidate remain pending.

A separate cold-completion issue remains. With a return annotation Stream[A],
Stream(a) fails while Stream.apply(a), fully qualified Stream(a), and explicit
Stream.apply[A](a) all compile. The warmed Stream(a) fixture now works. Do not
interpret this repair as complete implementation of companion application.

## Cold module application follow-up

The remaining Stream(a) failure was a nullary accessor whose return type is
ModuleRef. `insert_apply_on_nullary` calls `ensure_apply_supplied`, but the
latter only completed Class types. It therefore declined before reaching the
normal Select path that explicit .apply uses. Completing ModuleRef receivers
as well repairs the cold case without special-casing Stream.

The permanent test now includes both cold and warmed provider completion in
separate compiler invocations. default_type_imports, late_factory_inference
and qualifier_retry pass (4 tests), including matching nsc/scala-rs JVM output
for both Int and String. Evidence: cold-repaired-results.json and
cold-focused.log under the same probe directory. Full gates remain pending.

The earlier d2efbcbc full run has finished with 5 corpus losses. It must not be
merged: pos/t10272, pos/t12077 and pos/t6275 incorrectly reject valid patterns;
neg/t10073 and neg/t10073b accept unresolved ClassTag inference. The next
integration work must resolve and independently validate these cases.

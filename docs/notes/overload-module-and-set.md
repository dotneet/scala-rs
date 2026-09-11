# Overload Ambiguity and Set Combination

When a method and an object have the same name, the object's `apply` is also a
candidate. A `def f(Int)` and `object f { def apply(Int) }` at the same priority
are not collapsed merely because their argument and result types match. When
the object wins, code generation retains the original receiver and emits
`receiver.f.apply(...)`.

Unconditionally preferring a non-generic candidate would erase the ambiguity
between `f(1)` and `f[A <: 1](A)`. Candidates are compared by their types and
the inheritance relationship of their owners. Even in the compact standard
library declarations, `SetOps.++` and `IterableOps.++` retain their distinct
trait owners.

`SetOps.++` preserves the original set type. The widening `IterableOps.++`
normally returns a plain `Set`, so a `SortedSet` receiver must not narrow that
result back to `SortedSet`. Generated code also preserves the selected
overload instead of replacing both candidates with one JVM call. A
`SortedMap` combination that widens only the value type retains the key type,
the existing `Ordering`, and the `SortedMap` result.

`cargo test --release -p scala-rs-cli --test overload_module` compares the
behavior with scalac 2.13.16. The test checks rejection of ambiguous calls,
selection by specificity, single evaluation of the receiver, and the exit
status and stdout of `java -Xverify:all` for ordinary and element-widening set
combinations. It also requires ambiguity at the four call sites covered by
`neg/t11866` in the existing corpus.

This does not establish full Scala 2.13 overload compatibility.

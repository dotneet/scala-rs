# Implicit conversion inference acceptance repair

The full e0c92b19 corpus introduced two losses besides Singleton: pos/t11174b
and run/t2788. Both still failed at 750acebe.

For t11174b, conv[T](x: From): x.To[T] correctly keeps T open, but the synthetic
conversion application skipped type_apply and never registered its escaping
variables in undet_tvars. fill_conv_implicits now records them so the following
member arguments can constrain them. The first repair accepted from.foo(23),
but also wrongly accepted it for T <: CharSequence. Receiver-variable solutions
now undergo their declaration's type-parameter bounds check before substitution.
The compiler must not regain acceptance by treating an open parameter as AnyRef.

For t2788, ArrayOps.flatten requires both a function-valued implicit view and a
ClassTag. The ordinary solver finds Option.option2Iterable and binds B = Int,
but the later ClassTag requires materialization rather than an implicit value.
The ClassTag partial-solution continuation only searched implicit values, so it
lost the view binding. It now uses view_undet_bindings on a missing value, keeping
ambiguity excluded and preserving the binding for the later tag.

The original unannotated Array(...).flatten.toList still failed after that
first continuation repair: the test's explicit Array[Int] expected type had
pinned B independently. The actual implicit-only selection uses undet_solution
through adapt_implicit_apply. That solver now accepts a materializable ClassTag
only after previous constraints fix its element type, without using the tag to
solve open variables. The test now includes the unannotated original shape.

Evidence: /tmp/scala-rs-codex/integration/conversion-parent. first-results.json
records the positive repair and the bounded negative that exposed the missing
bounds check. unannotated-focused.log passed all 21 related tests, including
ClassTag's negative cases and the conversion oracle. originals-repaired/results.json
confirms the unmodified t11174b source compiles and the unmodified t2788 source
compiles and prints List(1, 2) under strict JVM verification.

complete/measures has cats 346/81, gitbucket 899/112, Slick 0 errors/1490 classes,
and scala library 1558/169. Exact error-message/location counters are unchanged
from the preceding Singleton measurements (diagnostic-delta.json). The failed
first unannotated reproduction remains in originals/results.json as evidence
that the initial annotated test was insufficient.


Full workspace, corpus and interop acceptance remain required before merging.

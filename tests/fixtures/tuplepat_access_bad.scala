// `private[this]` is still object-private after `agent/tuplepat` widened the
// prefix test from the *tree* to the prefix *type*. A different instance is
// not `this`, whether the selection is written in the trait itself or in a
// class nested inside it.
//
// Two errors, one per selection, under both compilers. The wording differs:
// nsc drops an object-private member from a non-`this` prefix's member set
// entirely ("value gen is not a member of Ops2") where this compiler finds it
// and refuses the access; the test asserts the count and the rejection.
trait Ops2 { self =>
  private[this] def gen: Int = 1
  def f(o: Ops2): Int = o.gen
  private class In { def g(o: Ops2): Int = o.gen }
}

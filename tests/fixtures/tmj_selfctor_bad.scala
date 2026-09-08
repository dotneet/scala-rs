// What believing a *written* self-instantiating type-argument list must not
// let through. Same shape as `tmj_selfctor.scala` -- no constructor parameter
// mentions `K` or `V` -- so these go through the very code path the fix
// added. Real scalac 2.13.16 reports exactly these two, at these two lines.
package tmj

final class Cell[K, V](val bits: Int, payload: AnyRef) {
  def swapped = new Cell[V, K](bits, payload)

  // (1) `swapped`'s inferred result really is `Cell[V, K]`, and `Cell` is
  // invariant, so asking for `Cell[K, V]` is a mismatch. The fix has to give
  // the honest type here, not merely one that conforms -- and this is also
  // what says the written list is read in the order written rather than the
  // class's own parameters being substituted back in declaration order.
  def wrong: Cell[K, V] = swapped

}

final class Pair[K, V](val k: K, val v: V) {
  // (2) Believing the written list is exactly what makes this constructor's
  // first parameter be `String` here, so passing `k: K` has to be rejected.
  // Discarding the list instead -- what the compiler used to do -- inferred
  // `K` from the argument and let it through.
  def mistyped = new Pair[String, V](k, v)
}

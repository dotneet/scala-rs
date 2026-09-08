// A class that instantiates *itself*, writing out its own type parameters, in
// a method whose result type is inferred. This is the shape every one of
// `CNode`'s copying methods uses in `scala/collection/concurrent/TrieMap.scala`
// (`updatedAt` / `removedAt` / `insertedAt` / `renewed`, each ending in
// `new CNode[K, V](...)` with no declared result type).
//
// The written arguments *are* the instantiated class's own type-parameter
// symbols, which -- judged on the type alone -- is indistinguishable from the
// placeholders an un-applied `new C` carries, so the constructor path threw
// the whole list away and re-inferred it from the value arguments.
//
// Note what makes that fatal here: **no constructor parameter mentions `K` or
// `V`**. `CNode` is generic but stores its children erased, in an
// `Array[BasicNode]`; `bits`/`payload`/`tag` below are the same arrangement.
// So there is nothing for inference to read the parameters off, both solved to
// `Nothing`, and every one of these methods got the inferred result type
// `Cell[Nothing, Nothing]`. (A class whose constructor *does* take a `K`
// recovers by accident, which is why the defect hid.)
//
// Nothing Java-specific about any of this.
package tmj

final class Cell[K, V](val bits: Int, payload: AnyRef, val tag: String) {
  def key: K = payload.asInstanceOf[K]
  def value: V = payload.asInstanceOf[V]

  // Inferred result type; the arguments carry `K` and `V` verbatim.
  def withTag(t: String) = new Cell[K, V](bits, payload, t)

  // Mixed: `K` is the class's own parameter, `String` is not. The whole list
  // was dropped, not only the parameters, so `String` was lost too.
  def erased = new Cell[K, String](bits, payload, tag)

  // Swapped, so believing the written list is not the same as substituting
  // the class's parameters back in declaration order.
  def swapped = new Cell[V, K](bits, payload, tag)

  // A local `val` between the `new` and the method's result changes nothing.
  def viaLocal = { val c = new Cell[K, V](bits, payload, tag); c }
}

object Main {
  // Invariant in both parameters, so a `Cell[Nothing, Nothing]` argument
  // cannot be quietly absorbed here.
  def tagOf[K, V](c: Cell[K, V]): String = c.tag

  def main(args: Array[String]): Unit = {
    val c = new Cell[String, Int](7, "one", "t0")

    // Each of these annotations is the check: it is the *declared* type of
    // the val that the inferred result type has to conform to.
    val a: Cell[String, Int] = c.withTag("t1")
    println(a.key)
    println(a.bits)
    println(a.tag)

    val e: Cell[String, String] = c.erased
    println(e.value)

    val s: Cell[Int, String] = c.swapped
    println(s.value)
    println(s.tag)

    val v: Cell[String, Int] = c.viaLocal
    println(v.tag)

    println(tagOf(c.withTag("t2")))
  }
}

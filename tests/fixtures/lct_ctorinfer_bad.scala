package lct

class ContraBox[-A](val x: AnyRef)

final class Node[A, B](_key: A, _value: AnyRef, _next: AnyRef) {
  def key: A = _key
  def value: B = _value.asInstanceOf[B]
  def next: Node[A, B] = _next.asInstanceOf[Node[A, B]]
}
object Node {
  def cons[A, B](k: A, v: AnyRef, tail: Node[A, B]): Node[A, B] = new Node(k, v, tail)
  def lookup[A, B](n: Node[A, B], k: A): B =
    if (n eq null) null.asInstanceOf[B]
    else if (k == n.key) n.value
    else lookup(n.next, k)
}

object Bad {
  // `ContraBox` is contravariant: `ContraBox[Int]` is a *super*type of
  // `ContraBox[Any]`, never a subtype of it. This is ordinary variance
  // conformance, unrelated to how an unconstrained type parameter defaults,
  // and it must still be rejected.
  val bad1: ContraBox[Any] = new ContraBox[Int]("x")

  val n1: Node[Int, String] = Node.cons(1, "one", null)
  // The self-recursive `lookup`'s own `A` is `Int` here (from `n1`); an
  // explicit type argument that disagrees still has to be checked.
  val bad2: String = Node.lookup[Int, String](n1, "not an int")
}

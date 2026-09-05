// A constructor call whose type argument no argument mentions (`new C(args)`
// where the class's own type parameter appears nowhere in the value
// parameters), and a generic method that calls itself with an argument whose
// type is written in terms of the very type parameters being solved for.
// Both are the shapes `docs/scala-library.md` records from `src/library`
// itself: `Vector.scala`'s `private[this] def copy(...) = new Vector2(...)`
// (no argument mentions the element type) and
// `RedBlackTree.scala`'s `def lookup[A, B](tree: Tree[A, B], x: A) = ...
// lookup(tree.left, x)` (a self-recursive call).
package lct

// (1) Constructor type-argument inference for an unconstrained parameter.
// nsc's default is variance-driven (`Infer.solvedTypes`), not a flat `Any`:
// the parameter's own lower bound (`Nothing` when unbounded) for a covariant
// or invariant parameter, its upper bound (`Any` when unbounded) for a
// contravariant one -- confirmed against real scalac with `-Xprint:typer`.
class CoBox[+A](val x: AnyRef) {
  // No declared return type: its type is inferred from the body alone, and
  // nothing here mentions `A`.
  private[this] def copy(y: AnyRef = x) = new CoBox(y)
  def updated[B >: A](y: AnyRef): CoBox[B] = copy(y)
  def get: AnyRef = x
}

class InvBox[A](val x: AnyRef) {
  private[this] def copy(y: AnyRef = x) = new InvBox(y)
  def get: AnyRef = x
}

class ContraBox[-A](val x: AnyRef) {
  private[this] def copy(y: AnyRef = x) = new ContraBox(y)
  // `copy`'s own inferred type is `ContraBox[Any]` (the contravariant
  // default), which is exactly what this signature asks for.
  def widened: ContraBox[Any] = copy()
}

// (2) A self-recursive generic method. `next` has no declared return type of
// its own either, and its body reads back the class's own `A`/`B` through a
// cast -- exactly `RedBlackTree.Tree.left`'s shape.
final class Node[A, B](_key: A, _value: AnyRef, _next: AnyRef) {
  def key: A = _key
  def value: B = _value.asInstanceOf[B]
  def next: Node[A, B] = _next.asInstanceOf[Node[A, B]]
}
object Node {
  def cons[A, B](k: A, v: AnyRef, tail: Node[A, B]): Node[A, B] = new Node(k, v, tail)

  // Self-recursive: the argument `n.next` has type `Node[A, B]`, written in
  // terms of *this call's own* `A`/`B` -- the fixed point `A := A, B := B`
  // is the correct solution, not a failure to solve.
  def lookup[A, B](n: Node[A, B], k: A): B =
    if (n eq null) null.asInstanceOf[B]
    else if (k == n.key) n.value
    else lookup(n.next, k)
}

object Main {
  def main(args: Array[String]): Unit = {
    val b = new CoBox[Int]("hi")
    val b2: CoBox[Any] = b.updated("bye")
    println(b2.get)

    val ib = new InvBox[String]("iv")
    println(ib.get)

    val cb = new ContraBox[Int]("cv")
    val w: ContraBox[Any] = cb.widened
    println(w.x)

    val n3: Node[Int, String] = Node.cons(3, "three", null)
    val n2: Node[Int, String] = Node.cons(2, "two", n3)
    val n1: Node[Int, String] = Node.cons(1, "one", n2)
    println(Node.lookup(n1, 1))
    println(Node.lookup(n1, 2))
    println(Node.lookup(n1, 3))
  }
}

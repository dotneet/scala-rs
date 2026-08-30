// A case class with a `Unit` field: the companion's `apply`, the erased
// `apply(Object, Object)` bridge, `copy`, `toString`, `equals`, `hashCode` and
// `productElement` all read a `BoxedUnit`, never an int-sorted primitive.
case class K(k: Unit, n: Int)
case class U(u: Unit)

object Main {
  def main(args: Array[String]): Unit = {
    val a = K((), 3)
    println(a)
    println(a.k)
    println(a.n)
    println(a.copy(n = 4))
    println(a == K((), 3))
    println(a == K((), 4))
    println(a.productArity)
    println(a.productElement(0))
    println(a.productElement(1))
    println(a.hashCode == K((), 3).hashCode)
    println(U(()))
    a match {
      case K(u, n) => println("matched"); println(u); println(n)
    }
  }
}

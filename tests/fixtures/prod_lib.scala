// The half of `Product` that only the real scala-library backs: the `Product`
// *type*, `productIterator` / `productElementNames` (which come from
// `scala.Product` itself), and `tupled` / `curried` on the synthetic companion
// (which come from `scala.runtime.AbstractFunctionN`).
object Main {
  case class P(x: Int, y: String)
  case class One(a: Int)
  case class Zero()
  case class Meters(n: Int)
  case object Solo
  sealed trait T
  case class L(a: Int) extends T
  case object R extends T

  case class Big22(
      a1: Int, a2: Int, a3: Int, a4: Int, a5: Int, a6: Int, a7: Int, a8: Int,
      a9: Int, a10: Int, a11: Int, a12: Int, a13: Int, a14: Int, a15: Int,
      a16: Int, a17: Int, a18: Int, a19: Int, a20: Int, a21: Int, a22: Int)

  def arity(p: Product): String = p.productPrefix + "/" + p.productArity

  def main(args: Array[String]): Unit = {
    val p = P(1, "h")

    // The type itself: a case class conforms to Product, and to Serializable.
    println("as Product = " + arity(p))
    println("case object as Product = " + arity(Solo))
    val ps: List[Product] = List(p, One(2), Zero(), Solo)
    println("list = " + ps.map(_.productPrefix))
    val ser: java.io.Serializable = p
    println("as Serializable = " + ser)

    // productIterator / productElementNames come from `scala.Product`.
    println("iterator = " + p.productIterator.toList)
    println("names = " + p.productElementNames.toList)
    println("empty iterator = " + Zero().productIterator.toList)
    println("object iterator = " + Solo.productIterator.toList)
    println("object names = " + Solo.productElementNames.toList)

    // The companion is an AbstractFunctionN.
    println("tupled = " + P.tupled((5, "z")))
    println("curried = " + P.curried(5)("w"))
    val f: (Int, String) => P = P
    println("as function = " + f(6, "q"))
    println("mapped = " + List(1, 2, 3).map(One).map(_.a))
    println("one tupled = " + List(9).map(One.apply))

    // Arity 22 is the last one AbstractFunctionN covers.
    val big = Big22(1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17,
      18, 19, 20, 21, 22)
    println("big arity = " + big.productArity)
    println("big last = " + big.productElement(21))
    println("big names = " + big.productElementNames.toList.last)
    val bf: (Int, Int, Int, Int, Int, Int, Int, Int, Int, Int, Int, Int, Int,
      Int, Int, Int, Int, Int, Int, Int, Int, Int) => Big22 = Big22
    println("big applied = " + bf(1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13,
      14, 15, 16, 17, 18, 19, 20, 21, 22).productElement(0))

    // A sealed hierarchy's members are all Products.
    val ts: List[T] = List(L(1), R)
    println("sealed = " + ts.map { case t: Product => arity(t) })
  }
}

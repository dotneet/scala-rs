// `case class` / `case object` as a real `scala.Product`, and the synthetic
// companion as a real `scala.runtime.AbstractFunctionN`. Everything here needs
// only `java.lang` plus the four `Product` members a case class overrides
// itself, so it runs under `--no-scala-library` too; `prod_lib.scala` covers
// the parts that need the jar.
object Main {
  case class P(x: Int, y: String)
  case class One(a: Int)
  case class Zero()
  case class Sub(a: Int) extends Base(a)
  class Base(val n: Int)
  case object Solo

  def show(name: String, thunk: => Any): Unit =
    println(name + " = " + thunk)

  def oob(thunk: => Any): String =
    try { "no throw: " + thunk }
    catch { case e: IndexOutOfBoundsException => "IndexOutOfBoundsException " + e.getMessage }

  def main(args: Array[String]): Unit = {
    val p = P(1, "h")
    show("productPrefix", p.productPrefix)
    show("productArity", p.productArity)
    show("productElement 0", p.productElement(0))
    show("productElement 1", p.productElement(1))
    show("productElementName 0", p.productElementName(0))
    show("productElementName 1", p.productElementName(1))

    // Out of range, both directions, on both accessors.
    println(oob(p.productElement(9)))
    println(oob(p.productElement(-1)))
    println(oob(p.productElementName(9)))
    println(oob(p.productElementName(-1)))

    // A single field still switches, and a zero-field case class has no
    // in-range index at all.
    val one = One(7)
    show("One arity", one.productArity)
    show("One element", one.productElement(0))
    show("One name", one.productElementName(0))
    println(oob(one.productElement(1)))
    val zero = Zero()
    show("Zero arity", zero.productArity)
    println(oob(zero.productElement(0)))
    println(oob(zero.productElementName(0)))

    // A case class with a superclass is still a Product.
    val sub = Sub(4)
    show("Sub prefix", sub.productPrefix)
    show("Sub element", sub.productElement(0))
    show("Sub n", sub.n)

    // A `case object` is a zero-arity Product.
    show("Solo prefix", Solo.productPrefix)
    show("Solo arity", Solo.productArity)
    println(oob(Solo.productElement(0)))
    println(oob(Solo.productElementName(0)))
  }
}

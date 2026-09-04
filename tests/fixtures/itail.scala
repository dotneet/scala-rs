// Leftover implicit cases and the holes in the prelude.
//
// 1. A call whose companion-object implicit is already filled in survives a
//    second typing by the tupling retry (`LiteralNode(1)`'s `intType`).
// 2. `Numeric[T]` is an `Ordering[T]` (slick `ScalaNumericType`).
// 3. A type parameter no value argument mentions is decided by implicit search
//    (slick `SimpleFunction.nullary`).
// 4. `apply` on a function value is the function itself.
// 5. An implicit clause left in argument position is filled in once the
//    parameter type is known (`take(Array.empty)`).
// 6. `copy` defaults for a case class with a varargs parameter.

import scala.reflect.ClassTag

class Tagged[T](val name: String) {
  override def toString = "Tagged(" + name + ")"
}
class NumTagged[T](val zero: T)(implicit ord: Ordering[T]) extends Tagged[T]("num") {
  def below(x: T): Boolean = ord.lt(x, zero)
}

object Tagged {
  implicit val intTag: NumTagged[Int] = new NumTagged[Int](0)
  implicit val strTag: Tagged[String] = new Tagged[String]("str")
  implicit val boolTag: Tagged[Boolean] = new Tagged[Boolean]("bool")
}

class Lit(val tag: Tagged[?], val value: Any, val vol: Boolean) {
  override def toString = "Lit(" + value + ", " + tag + ", " + vol + ")"
}
object Lit {
  def apply(tag: Tagged[?], v: Any, vol: Boolean = false): Lit = new Lit(tag, v, vol)
  def apply[T](v: T)(implicit tag: Tagged[T]): Lit = apply(tag, v)
}

final case class Pair(fst: Lit, snd: Lit)

/** `Numeric[T]` extends `Ordering[T]`. scala-rs's prelude did not wire up that
  * parent, so this super call did not go through. */
class OrdBox[T](implicit val ct: ClassTag[T], val ord: Ordering[T]) {
  def name: String = ct.toString
}
class NumBox[T](val fromDouble: Double => T)(implicit tag: ClassTag[T], val num: Numeric[T])
  extends OrdBox[T]()(tag, num) {
  def twice(x: T): T = num.plus(x, x)
}

/** No value argument mentions `T`, so only implicit search can decide it. */
object Build {
  def rows[T](prefix: String)(implicit tag: Tagged[T]): (Seq[Int] => String) =
    (xs: Seq[Int]) => prefix + tag.name + xs.size
  def nullary[R: Tagged](prefix: String): String = rows(prefix).apply(Seq())
  def unary[A, R: Tagged](prefix: String): (Int => String) = {
    val f = rows(prefix);
    { (n: Int) => f(Seq(n)) }
  }
}

/** A case class with a varargs parameter. nsc generates no `copy` for this
  * shape but scala-rs does, so unless its `copy$default$n` is typed as `Seq[T]`
  * rather than `T*` we reported diagnostics against a tree nobody wrote. Here
  * we exercise only the uses that agree with nsc (`apply` / field / `equals`). */
final case class Row(name: String, cells: Int*) {
  def total: Int = cells.sum
}

object Main {
  def takeStrings(a: Array[String]): Int = a.length
  def takeInts(a: Array[Int]): Int = a.length

  def main(args: Array[String]): Unit = {
    // 1. The tupling retry re-types a call carrying implicit arguments without breaking it.
    println(Pair(Lit(1), Lit("x")))
    println(Lit(true))

    // 2. Numeric is an Ordering.
    val nb = new NumBox[Int](_.toInt)
    println(nb.name + " " + nb.twice(21) + " " + nb.ord.lt(1, 2))
    val ordering: Ordering[Int] = implicitly[Numeric[Int]]
    println(ordering.compare(3, 4))
    println(Tagged.intTag.below(-1))

    // 3. A type parameter only implicit search can decide.
    println(Build.nullary[String]("a:"))
    val u = Build.unary[Int, Boolean]("b:")
    println(u(7))

    // 4. apply on a function value.
    val f: Seq[Int] => String = xs => "n=" + xs.size
    println(f.apply(Seq(1, 2)) + " " + f(Seq()))

    // 5. A residual implicit clause in argument position.
    println(takeStrings(Array.empty) + takeInts(Array.empty))

    // 6. A varargs case class and copy.
    val r = Row("r", 1, 2, 3)
    println(r.name + " " + r.total + " " + r.cells.size)
    println(r == Row("r", 1, 2, 3))
    println(Row("t").total)
  }
}

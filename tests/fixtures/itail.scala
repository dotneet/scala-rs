// implicit の残件と prelude の穴。
//
// 1. companion object の implicit を埋めた呼び出しを、タプル化リトライが
//    もう一度型付けしても壊れない（`LiteralNode(1)` の `intType`）。
// 2. `Numeric[T]` は `Ordering[T]` である（slick `ScalaNumericType`）。
// 3. 値引数がどれも触れない型パラメータは implicit 探索が決める
//    （slick `SimpleFunction.nullary`）。
// 4. 関数値の `apply` は関数そのもの。
// 5. 引数位置に残った implicit 節を、パラメータ型が決まってから埋める
//    （`take(Array.empty)`）。
// 6. 可変長引数を持つ case class の `copy` デフォルト。

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

/** `Numeric[T]` は `Ordering[T]` を継承する。scala-rs の prelude はその親を
  * 張っていなかったので、この super 呼び出しが通らなかった。 */
class OrdBox[T](implicit val ct: ClassTag[T], val ord: Ordering[T]) {
  def name: String = ct.toString
}
class NumBox[T](val fromDouble: Double => T)(implicit tag: ClassTag[T], val num: Numeric[T])
  extends OrdBox[T]()(tag, num) {
  def twice(x: T): T = num.plus(x, x)
}

/** 値引数が `T` に触れないので、`T` を決められるのは implicit 探索だけ。 */
object Build {
  def rows[T](prefix: String)(implicit tag: Tagged[T]): (Seq[Int] => String) =
    (xs: Seq[Int]) => prefix + tag.name + xs.size
  def nullary[R: Tagged](prefix: String): String = rows(prefix).apply(Seq())
  def unary[A, R: Tagged](prefix: String): (Int => String) = {
    val f = rows(prefix);
    { (n: Int) => f(Seq(n)) }
  }
}

/** 可変長引数を持つ case class。nsc はこの形に `copy` を作らないが、
  * scala-rs は作るので、その `copy$default$n` を `T*` ではなく `Seq[T]` として
  * 型付けしないと、書かれてもいないツリーに対する診断が出ていた。ここでは
  * nsc と一致する使い方（`apply` / フィールド / `equals`）だけを確かめる。 */
final case class Row(name: String, cells: Int*) {
  def total: Int = cells.sum
}

object Main {
  def takeStrings(a: Array[String]): Int = a.length
  def takeInts(a: Array[Int]): Int = a.length

  def main(args: Array[String]): Unit = {
    // 1. タプル化リトライが implicit 引数入りの呼び出しを再型付けしても壊れない。
    println(Pair(Lit(1), Lit("x")))
    println(Lit(true))

    // 2. Numeric は Ordering。
    val nb = new NumBox[Int](_.toInt)
    println(nb.name + " " + nb.twice(21) + " " + nb.ord.lt(1, 2))
    val ordering: Ordering[Int] = implicitly[Numeric[Int]]
    println(ordering.compare(3, 4))
    println(Tagged.intTag.below(-1))

    // 3. implicit 探索だけが決められる型パラメータ。
    println(Build.nullary[String]("a:"))
    val u = Build.unary[Int, Boolean]("b:")
    println(u(7))

    // 4. 関数値の apply。
    val f: Seq[Int] => String = xs => "n=" + xs.size
    println(f.apply(Seq(1, 2)) + " " + f(Seq()))

    // 5. 引数位置の残余 implicit 節。
    println(takeStrings(Array.empty) + takeInts(Array.empty))

    // 6. 可変長引数の case class と copy。
    val r = Row("r", 1, 2, 3)
    println(r.name + " " + r.total + " " + r.cells.size)
    println(r == Row("r", 1, 2, 3))
    println(Row("t").total)
  }
}

// Parser corners the 2.13 standard library needs and this subset lacked:
// multi-importer clauses, `using` argument clauses, meta-annotated
// annotations, and interpolation holes holding more than one statement.
// See docs/scala-library.md.
object Boxes {
  object Deep { def deep: String = "deep" }
  def top: String = "top"
}

class tag(msg: String) extends scala.annotation.StaticAnnotation
class onGetter extends scala.annotation.StaticAnnotation

object Main {
  // One `import` clause, two importers (`scala/collection/immutable/HashMap`
  // writes `import scala.collection.mutable, mutable.ReusableBuilder`).
  import Boxes.top, Boxes.Deep.deep

  // `@(T @meta)(args)`: the meta-annotation says which member of the
  // definition receives the annotation, and only the base one is kept here.
  // `scala/collection/immutable/RedBlackTree` writes
  // ``@(`inline` @getter @setter) private var _key: A``.
  @(tag @onGetter)("kept") private val tagged: Int = 41

  def label(n: Int)(implicit s: String): String = s + n

  // `${ ... }` is a block, so it may hold several statements — including a
  // nested triple-quoted string (`scala/StringContext`).
  def hole: String = s"""a ${
    val inner = s"""[b]"""
    val n = tagged + 1
    inner + n
  } c"""

  def main(args: Array[String]): Unit = {
    println(top + "/" + deep)
    // `f(using x)` is an ordinary argument clause; 2.13 takes it with no flag.
    println(label(1)(using "s="))
    println(hole)
    // `using` is still an ordinary identifier: with no expression after it,
    // it is the argument itself.
    val using = "id"
    println(label(2)(using))
  }
}

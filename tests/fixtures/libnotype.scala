// `agent/libnotype`: three shapes whose type was lost rather than reported,
// every one of them taken from `src/library` of scala/scala.
package libnt

class Node(val v: Int)

class Holder {
  // A *term* member of the enclosing class with the same name as a top-level
  // class -- `List` declares `def ::` beside the top-level `class ::`, and
  // `new ::(x, xs)` in `List.scala` has to reach the class past it.
  def Node(v: Int): Int = -v
  def make(v: Int): Node = new Node(v)
}

object Main {
  // A blank line ends the expression. `scala/collection/immutable/HashMap.scala`
  // and `HashSet.scala` write `var newCachedHashCode = 0`, a blank line, and
  // then a bare block; read as `0 { ... }` the `var` took the failed
  // application's type and every later `+=` on it reported on `<notype>`.
  def blankLineBlock(): Int = {
    var acc = 0

    {
      var i = 0
      while (i < 4) { acc += i; i += 1 }
    }
    acc
  }

  def g(f: => Int): Int = f + 1

  // A *single* line break before `{` is still an application, and must stay one.
  def singleNewlineApply(): Int =
    g
    { 40 }

  // `val accum = new accum`, where `class accum` is declared in an enclosing
  // block: the `val` being defined is the nearer binding of the name, but it
  // is a term and `new` names a type. `HashMap.concat` is written this way.
  def valShadowsLocalClass(that: Any): Int = {
    class accum {
      var current: Int = 0
      def add(n: Int): Unit = current += n
    }
    that match {
      case _: String =>
        val accum = new accum
        accum.add(3)
        accum.current
      case _ =>
        val accum = new accum
        accum.add(4)
        accum.current
    }
  }

  def main(args: Array[String]): Unit = {
    println(blankLineBlock())
    println(singleNewlineApply())
    println(valShadowsLocalClass("s"))
    println(valShadowsLocalClass(0))
    println(new Holder().make(7).v)
    println(new Holder().Node(7))
  }
}

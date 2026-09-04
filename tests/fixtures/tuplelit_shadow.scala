// A tuple literal is `scala.TupleN`, whatever `TupleN` means here.
//
// nsc's `gen.mkTuple` builds a fully qualified `scala.TupleN` tree, so the
// name is never looked up. This compiler lowers `(a, b)` to
// `Apply(Ident("Tuple2"), …)` in the parser, so inside a scope that declares a
// *term* of that name the literal called it: `scala.math.Ordering` declares
// `implicit def Tuple2[T1, T2](…)` and writes tuple literals in its own body.
//
// The synthesized `Ident` now carries `Tree::scala_ref` and is resolved as a
// member of package `scala`. An *explicit* `Tuple2(…)` is still an ordinary
// name and still finds the method, which is what scalac 2.13.16 does.
object Shadowed {
  def Tuple2(n: Int): String = "method Tuple2(" + n + ")"
  def Tuple3(n: Int): String = "method Tuple3(" + n + ")"
  def Seq(n: Int): String = "method Seq(" + n + ")"

  // Expression position: still the tuple.
  def pair[A, B](a: A, b: B): (A, B) = (a, b)
  def triple[A, B, C](a: A, b: B, c: C): (A, B, C) = (a, b, c)

  // Pattern position: still the tuple's extractor.
  def sum(p: (Int, Int)): Int = p match { case (a, b) => a + b }

  // The `for` desugaring binds the generator pattern the same way.
  def sums(ps: List[(Int, Int)]): List[Int] = for ((a, b) <- ps) yield a + b

  // A repeated parameter is `Seq[T]`, and a `def Seq` in scope must not stop
  // the widening: `xs.length` used to be `value length is not a member of
  // Int*`.
  def count(xs: Int*): Int = xs.length

  // Written out by hand, the name means what the source says it means.
  def explicit: String = Tuple2(1)
  def explicitSeq: String = Seq(2)
}

object Main {
  def main(args: Array[String]): Unit = {
    println(Shadowed.pair("a", 3))
    println(Shadowed.triple(1, "b", 2.0))
    println(Shadowed.sum((4, 5)))
    println(Shadowed.sums(List((1, 2), (3, 4))))
    println(Shadowed.count(7, 8, 9))
    println(Shadowed.explicit)
    println(Shadowed.explicitSeq)
    println(Shadowed.Tuple3(6))
  }
}

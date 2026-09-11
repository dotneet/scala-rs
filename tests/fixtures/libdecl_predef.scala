// A source `scala.Predef`'s type aliases are open in *signatures*.
//
// Compiling scala/scala's `src/library` compiles `Predef.scala` in the same
// run, and `Predef._` then has to mean that object. Its members were imported
// only after the signature pass, so a signature naming one of its aliases --
// `val byName: Map[String, Int]` in `Enumeration.scala`, `def env:
// Map[String, String]` in `sys/package.scala` -- read `not found: type Map`
// while the same name in a body resolved. `Pairs` plays `Map` here.
//
// `type String = java.lang.String` is there because real scalac 2.13.16
// refuses a `Predef` without it (see `libmaxmin_predef.scala`); `println` is
// written with `append` for the reason recorded there.
package scala {
  object Predef {
    type String = java.lang.String
    type Pairs[K] = List[(K, K)]
    def println(x: Any): Unit = {
      java.lang.System.out.append(x.toString)
      java.lang.System.out.append("\n")
      ()
    }
  }
}

object Main {
  val pairs: Pairs[Int] = List((1, 2), (3, 4))
  def swap(p: Pairs[Int]): Pairs[Int] = p.map(t => (t._2, t._1))
  def main(args: Array[String]): Unit = {
    println(swap(pairs))
  }
}

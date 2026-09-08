// The Java half: `jvarargs.JVar`, compiled by javac and read back as a class
// file. `java.lang.reflect.Array` is here too, because it is the member the
// standard library actually calls and the one the twelve `ambiguous overload`
// errors named.
//
// Needs the real `scala-library` jar: the `_*` splice below is a `Seq`.
package jvarargs

object JMain {
  def g(x: Int): String = "s-fixed(" + x + ")"
  def g(x: Int*): String = "s-varargs(" + x.length + ")"

  def main(args: Array[String]): Unit = {
    // Both alternatives accept this; `pick(int)` wins and allocates nothing.
    println(JVar.pick(1))
    // Only `pick(int...)` accepts two.
    println(JVar.pick(1, 2))
    // Varargs-only, with a primitive element type: the array built for the
    // call has to be an `int[]`, not the boxed `Object[]` this used to emit.
    println(JVar.only(3, 4))
    println(JVar.refs("a", "b"))
    // A widening element type: `5` is an `Int` literal reaching a `long[]`.
    println(JVar.wide(5))
    // A fixed-arity formal that is a supertype of the sequence its varargs
    // sibling stands for still wins.
    println(new JVar().inst("z"))
    // The library's own call. `newInstance(Class[_], Int)` returns a
    // one-dimensional array; the varargs alternative, reached with two
    // lengths, returns a two-dimensional one -- so the class name printed
    // says which alternative ran.
    println(java.lang.reflect.Array.newInstance(classOf[Int], 3).getClass.getName)
    println(java.lang.reflect.Array.newInstance(classOf[Int], 2, 3).getClass.getName)
    // A `_*` splice is a repeated argument, and only a repeated parameter
    // list takes one. Handing it to `g(x: Int)` as a single element would
    // compile and be wrong -- and did, until applicability learned the rule.
    println(g(Seq(4, 5, 6): _*))
  }
}

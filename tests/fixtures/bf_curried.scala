// A method with three parameter lists: each clause's own declared parameter
// types are what its arguments are inferred against. Reading the *first*
// clause for every clause solved `K` twice and never `B`, which is what made
// `xs.groupMapReduce(key)(f)(reduce)` report
// "no matching overload for (String)String with arguments (Any)".
//
// Only unary functions here: the private runtime has no `scala.Function2`.
object Main {
  def three[K, B](key: Int => K)(f: Int => B)(g: B => B): B = g(f(1))

  def four[A, B, C](a: Int => A)(b: Int => B)(c: Int => C)(join: C => String): String =
    join(c(3)) + "|" + a(1) + "|" + b(2)

  def main(args: Array[String]): Unit = {
    println(three(_.toString)(_ + 1)(_ * 10))
    println(three(_ + 1)(_.toString)(_ + "!"))
    println(four(_.toString)(_ * 2)(_ > 0)(b => if (b) "yes" else "no"))
    // Explicit type arguments still work, and still agree.
    println(three[String, Int](_.toString)(_ + 1)(_ * 10))
  }
}

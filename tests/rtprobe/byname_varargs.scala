// By-name parameters are evaluated once per use; varargs, `: _*` splices,
// primitive varargs, and by-name class parameters.
object Main {
  var evals = 0
  def v(): Int = { evals += 1; evals }
  def twice(x: => Int): Int = x + x
  def never(x: => Int): Int = 0
  def passAlong(x: => Int): Int = twice(x)
  def unless(c: Boolean)(body: => Unit): Unit = if (!c) body
  def lazyOr(a: Boolean, b: => Boolean): Boolean = a || b

  def sum(xs: Int*): Int = xs.sum
  def strs(prefix: String, xs: String*): String = prefix + xs.mkString("|") + "#" + xs.length
  def dbls(xs: Double*): Double = xs.foldLeft(0.0)(_ + _)
  def anys(xs: Any*): String = xs.map(x => if (x == null) "null" else x.getClass.getSimpleName).mkString(",")
  def seqOf[T](xs: T*): Seq[T] = xs

  class Lazy(x: => String) { def get = x + x }

  def main(args: Array[String]): Unit = {
    println(twice(v()) + " evals=" + evals)
    println(never(v()) + " evals=" + evals)
    println(passAlong(v()) + " evals=" + evals)
    unless(false) { println("ran") }
    unless(true) { println("should not run") }
    println(lazyOr(true, { println("rhs evaluated"); false }))
    println(lazyOr(false, { println("rhs evaluated"); true }))
    println(sum()); println(sum(1, 2, 3)); println(sum(List(4, 5): _*)); println(sum(Array(6, 7): _*))
    println(strs("p:")); println(strs("p:", "a")); println(strs("p:", Seq("x", "y", "z"): _*))
    println(dbls(1.5, 2.5))
    println(anys(1, "s", 2.0, 'c', null, List(1)))
    println(seqOf(1, 2, 3).getClass.getSimpleName.nonEmpty)
    println(seqOf("a", "b").map(_.toUpperCase))
    var n = 0
    val l = new Lazy({ n += 1; "v" + n })
    println(l.get + " " + l.get)
  }
}

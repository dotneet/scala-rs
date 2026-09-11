// Scala 2.13 (no -Xsource:3): an unapplied method with parameters is
// converted to a function only where a function type is expected, or with
// `m _`. Every marked line is scalac's "missing argument list for method …";
// scala-rs used to accept most of them (eta-expanding in uncurry).
class K {
  def m(a: Int): Int = a
  def useM = m                                    // err
}
object Main {
  def add(a: Int, b: Int): Int = a + b
  def curried(a: Int)(b: Int): Int = a * b
  def poly[A](a: A): List[A] = List(a)
  def two(a: Int)(implicit b: Int): Int = a + b
  def foo[F](f: F): F = f
  def by(x: => Any): Unit = ()
  def main(args: Array[String]): Unit = {
    def local(a: Int): Int = a
    val f = add                                   // err
    println(add.tupled((3, 4)))                   // err
    val c = curried                               // err
    val p = poly[Int]                             // err
    def g = add                                   // err
    println(add)                                  // err
    val x: Any = add                              // err
    val l = List(add)                             // err
    val b = { add }                               // err
    val i = if (args.isEmpty) add else add        // err
    val s = "abc".charAt                          // err
    val t = two                                   // err
    var v = add                                   // err
    val z = Main.add                              // err
    val a1 = foo(add)                             // err
    by(add)                                       // err
    val a2 = (add, 1)                             // err
    val a4 = poly                                 // err
    val a5 = local                                // err
    val a7 = add == null                          // err
    val a8 = Option(add)                          // err
    val lam = (y: Int) => curried                 // err
    val c1 = curried(1)                           // err
  }
}

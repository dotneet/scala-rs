// Warnings from phases after refchecks: constructors (uninitialized reads),
// specialization (unused specialized type parameters), and the view-bound
// deprecation the parser reports.
class Init {
  val early = late // warn
  val late = 3
  var v = w + 1 // warn
  var w = 2
  lazy val l = m
  val m = 4
  def d = n
  val n = 5
  println(q) // warn
  val q = 6
  val byName = Option(1).getOrElse(bb)
  val bb = 7
  val fn = () => u
  val u = 8
}

object InitObj {
  val a = b // warn
  val b = "b"
}

class Spec[@specialized(Int) T] {
  def used[@specialized(Int) A](x: A): A = x
  def inList[@specialized(Int) A](xs: List[A]): Int = xs.length // warn
  def none[@specialized(Int) A, @specialized(Int) B](x: Int): Int = x // warn
  def fun[@specialized(Int) A](f: A => Int): Int = 0
  def triple[@specialized(Int) A](p: (A, Int, Int)): Int = 0 // warn
}

object WarnLater {
  def view[A <% Int](a: A): Int = a

  def main(args: Array[String]): Unit = {
    val i = new Init
    println(i.early)
    println(i.v)
    println(InitObj.a)
    println(new Spec[Int].used(1))
    println(view(2))
  }
}

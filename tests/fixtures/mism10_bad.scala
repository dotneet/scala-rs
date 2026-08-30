// The errors these fixes must keep reporting.
//
// Dropping the *signature* pass's diagnostics for a parent constructor's
// arguments must not drop a real one: the body pass walks the same tree and
// raises it. And a defaulted constructor parameter is still checked against
// its declared type once the class's own type parameters are bound.

class Base(val n: Int)

class WrongParentArg extends Base("not an int")

class WrongDefault[A](val one: Chain[A] = Chain.of("a string"))

class Chain[A](val head: A)
object Chain {
  def of[A](a: A): Chain[A] = new Chain[A](a)
}

object Main {
  def main(args: Array[String]): Unit = {
    println(new WrongParentArg().n)
    println(new WrongDefault[Int]().one.head)
  }
}

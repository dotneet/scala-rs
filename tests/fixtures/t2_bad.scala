// Reading a view at the value it was imported from must *narrow* it, not
// widen it: `Box[Int]#mkOps` converts an `Int`, and nothing else.

class Ops[T](lhs: T) {
  def dbl(rhs: T): String = "(" + lhs + "," + rhs + ")"
}

class Box[T] {
  implicit def mkOps(lhs: T): Ops[T] = new Ops(lhs)
}

object Main {
  def wrongReceiver(b: Box[Int]): String = {
    import b._
    // `mkOps` converts an `Int` here, so a `String` has no `dbl`.
    "s".dbl("t")
  }

  def wrongArgument(b: Box[Int]): String = {
    import b._
    // `Ops[Int]#dbl` takes an `Int`.
    3.dbl("t")
  }

  def outOfScope[T](x: T, y: T): String = {
    // The import was in another method; `x` has no `dbl` here.
    x.dbl(y)
  }

  def main(args: Array[String]): Unit = {
    println(wrongReceiver(new Box[Int]) + wrongArgument(new Box[Int]) + outOfScope(1, 2))
  }
}

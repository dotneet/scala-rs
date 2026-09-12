object ResBad {
  // `new` still needs a class type: a *deferred* member has no right-hand side
  // to construct.
  trait HasT { type T; def make: T = new T }

  // A case class extractor still has the arity its constructor has.
  final case class Pair[A](a: A, b: A)
  def bad(p: Pair[Int]): Int = p match {
    case Pair(x, y, z) => x
    case _             => 0
  }

  // `super.m` read at the declaring class is still checked against the
  // override's declared result.
  trait Ops[A, +CC[_]] { def zip[B](that: List[B]): CC[(A, B)] = ??? }
  class WideBox[X]
  class NarrowBox[X] extends WideBox[X]
  class Wrong extends Ops[Int, WideBox] {
    override def zip[B](that: List[B]): WideBox[(Int, B)] = {
      val narrowed: NarrowBox[(Int, B)] = super.zip(that)
      narrowed
    }
  }
}

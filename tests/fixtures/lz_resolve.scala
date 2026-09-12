// Candidate fixture: name- and member-resolution roots.

// 1. A case class whose companion is hidden, at the point where the class is
//    typed, by a `val` of the same name in an *enclosing* package: the
//    companion is still the module in the same owner, so its synthesized
//    `unapply` is given a signature.
package object lz0 {
  val Cons = lz0.deep.Cons
  type Cons[A] = lz0.deep.Cons[A]
}

package lz0 {
  package deep {
    final case class Cons[A](head: A, private[lz0] var next: List[A])
    class Box[+A](val a: A) {
      override def toString = s"Box($a)"
    }
    object Use {
      def headOf(c: Cons[String]): String = c match {
        case Cons(h, _) => h
        case _          => "?"
      }
    }
  }

  package object inner {
    // 2. `new B(x)` for a parameterized alias constructs the class it renames.
    type Box2[+A] = lz0.deep.Box[A]
  }

  package inner {
    object Boxes {
      def boxed[A](a: A): Box2[A] = new Box2(a)
    }
  }

  // 3. An unqualified call to a sibling `->` of the same value class is typed
  //    from that method's own signature, not from the receiver.
  object Arrow {
    implicit final class AA[A](private val self: A) extends AnyVal {
      def ->[B](y: B): (A, B) = (self, y)
      def arrow[B](y: B): (A, B) = ->(y)
    }
  }

  // 4. An abstract receiver widened to its bound for member lookup is still
  //    what the conversion is applied to: the conversion's `X` is the type
  //    parameter, not its bound.
  object Narrowed {
    implicit final class Tilde[X](private val self: X) extends AnyVal {
      def ~>[Y](y: Y): (X, Y) = (self, y)
    }
    def pairOf[C <: AnyRef](c: C): (C, Int) = c ~> 1
  }

  // 5. `super.m` where the syntactically last parent only *inherits* `m`: the
  //    declaration is read at this class's own arguments for the class that
  //    declares it.
  trait Ops[A, +CC[_], +C] {
    def zip[B](that: List[B]): CC[(A, B)] = null.asInstanceOf[CC[(A, B)]]
  }
  trait StrictOps[A, +CC[_], +C] extends Ops[A, CC, C] {
    override def zip[B](that: List[B]): CC[(A, B)] = null.asInstanceOf[CC[(A, B)]]
  }
  class WideBox[X] { override def toString = "WideBox" }
  class NarrowBox[X] extends WideBox[X] { override def toString = "NarrowBox" }
  trait WideOps[+C] extends Ops[Int, WideBox, C]

  class Narrow
      extends Ops[Int, NarrowBox, Narrow]
      with StrictOps[Int, NarrowBox, Narrow]
      with WideOps[Narrow] {
    override def zip[B](that: List[B]): NarrowBox[(Int, B)] = {
      val inherited: NarrowBox[(Int, B)] = super.zip(that)
      new NarrowBox[(Int, B)]
    }
  }
}

object Main {
  def main(args: Array[String]): Unit = {
    println(lz0.deep.Use.headOf(lz0.deep.Cons("a", Nil)))
    println(lz0.inner.Boxes.boxed(7))
    println(lz0.Arrow.AA("k").arrow(1))
    println(lz0.Narrowed.pairOf("s"))
    println(new lz0.Narrow().zip(List("x")))
  }
}

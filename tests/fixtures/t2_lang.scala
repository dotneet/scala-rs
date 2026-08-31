// A view brought into scope by `import <a value>._`.
//
//  * The conversion is declared by a *generic* class, so it only says what it
//    converts once the value's type arguments are filled in.
//  * It is an instance member, so the call needs that value as its receiver.
//  * A subclass that overrides it is one candidate, not two.
//  * Its result may be a class nested in the generic one, whose members are
//    written at the *outer* class's parameters.

class Ops[T](lhs: T) {
  def dbl(rhs: T): String = "(" + lhs + "," + rhs + ")"
}

class Pair[T](lhs: T) {
  def pair(rhs: T): String = "[" + lhs + "|" + rhs + "]"
}

class Sharper[T](lhs: T) extends Pair[T](lhs) {
  override def pair(rhs: T): String = "<" + lhs + "|" + rhs + ">"
}

class Box[T] {
  // Nested in the generic class: `join` is written at `Box`'s `T`.
  class Inner(lhs: T) {
    def join(rhs: T): String = "{" + lhs + "/" + rhs + "}"
  }
  implicit def mkOps(lhs: T): Ops[T] = new Ops(lhs)
  implicit def mkInner(lhs: T): Inner = new Inner(lhs)
  implicit def mkPair(lhs: T): Pair[T] = new Pair(lhs)
}

class SubBox[T] extends Box[T] {
  override implicit def mkPair(lhs: T): Sharper[T] = new Sharper(lhs)
}

class Plain {
  implicit def mkOps(lhs: Int): Ops[Int] = new Ops(lhs)
}

object Main {
  // The value's argument is concrete.
  def known(b: Box[Int]): String = {
    import b._
    3.dbl(4) + 3.join(4)
  }

  // The value's argument is the method's own type parameter.
  def unknown[T](b: Box[T], x: T, y: T): String = {
    import b._
    x.dbl(y) + x.join(y)
  }

  // `mkPair` is overridden; the derived one is the only candidate.
  def overridden[T](b: SubBox[T], x: T, y: T): String = {
    import b._
    x.pair(y)
  }

  // No type parameters: only the receiver matters here.
  def plain(p: Plain): String = {
    import p._
    3.dbl(4)
  }

  def main(args: Array[String]): Unit = {
    println(known(new Box[Int]))
    println(unknown(new Box[String], "a", "b"))
    println(overridden(new SubBox[String], "a", "b"))
    println(plain(new Plain))
  }
}

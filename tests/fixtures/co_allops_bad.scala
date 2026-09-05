// Reading the inherited bound at the overriding trait's own type parameters
// must not turn into accepting bounds that do not narrow. Every declaration
// below is one nsc rejects with "incompatible type in overriding".

trait Invariant[F[_]]
trait Functor[F[_]] extends Invariant[F]

class Box[A](val value: A)
class Cell[A](val value: A)

class Animal
class Dog extends Animal

object Widened {
  trait Ops[F[_]] { type T <: Functor[F] }
  // `Invariant[F]` is wider than the inherited `Functor[F]`.
  trait Sub[F[_]] extends Ops[F] { type T <: Invariant[F] }
}

object WrongArgument {
  trait Ops[F[_]] { type T <: Functor[F] }
  // The parent is applied at `Box`, so the inherited bound is `Functor[Box]`;
  // `Functor[Cell]` is unrelated to it.
  trait Sub extends Ops[Box] { type T <: Functor[Cell] }
}

object NarrowedLowerBound {
  trait Ops[A] { type T >: Animal }
  // A lower bound may only be widened by an override.
  trait Sub[A] extends Ops[A] { type T >: Dog }
}

object BadAlias {
  trait Ops[F[_]] { type T <: Functor[F] }
  trait Sub[F[_]] extends Ops[F] { type T = Invariant[F] }
}

object Main {
  def main(args: Array[String]): Unit = println("unreachable")
}

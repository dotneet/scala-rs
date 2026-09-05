// simulacrum's `AllOps` encoding, as cats ships it in generated source: an
// abstract type member restated at every level of a *generic* trait hierarchy,
// each restatement narrowing its upper bound.
//
//   trait Ops[F[_], A]    { type TypeClassType <: Functor[F] }
//   trait AllOps[F[_], A] extends Ops[F, A] with Invariant.AllOps[F, A] {
//     type TypeClassType <: Functor[F]
//   }
//
// The inherited bound is written in the *parent's* type parameters, so it has
// to be read at the overriding trait's own parameters (`Functor[F_Ops]` ->
// `Functor[F_AllOps]`) before the two can be compared. Without that step every
// such restatement was rejected as "incompatible type in overriding".

trait Invariant[F[_]] {
  def imap[A, B](fa: F[A])(f: A => B)(g: B => A): F[B]
}
trait Functor[F[_]] extends Invariant[F] {
  def map[A, B](fa: F[A])(f: A => B): F[B]
  def imap[A, B](fa: F[A])(f: A => B)(g: B => A): F[B] = map(fa)(f)
}

object Invariant {
  trait Ops[F[_], A] {
    type TypeClassType <: Invariant[F]
    def self: F[A]
    val typeClassInstance: TypeClassType
    def imap[B](f: A => B)(g: B => A): F[B] = typeClassInstance.imap[A, B](self)(f)(g)
  }
  trait AllOps[F[_], A] extends Ops[F, A]
}

object Functor {
  trait Ops[F[_], A] {
    type TypeClassType <: Functor[F]
    def self: F[A]
    val typeClassInstance: TypeClassType
    def map[B](f: A => B): F[B] = typeClassInstance.map[A, B](self)(f)
  }
  // The diamond: `TypeClassType` arrives from `Ops` bounded by `Functor[F]`
  // and from `Invariant.AllOps` bounded by `Invariant[F]`, and the
  // restatement here has to conform to both.
  trait AllOps[F[_], A] extends Ops[F, A] with Invariant.AllOps[F, A] {
    type TypeClassType <: Functor[F]
  }
  object ops {
    implicit def toAllFunctorOps[F[_], A](target: F[A])(implicit tc: Functor[F]): AllOps[F, A] {
      type TypeClassType = Functor[F]
    } =
      new AllOps[F, A] {
        type TypeClassType = Functor[F]
        val self: F[A] = target
        val typeClassInstance: TypeClassType = tc
      }
  }
}

class Box[A](val value: A) {
  override def toString: String = "Box(" + value + ")"
}

class Pair[A, B](val a: A, val b: B) {
  override def toString: String = "Pair(" + a + "," + b + ")"
}

class Animal { override def toString: String = "animal" }
class Dog extends Animal { override def toString: String = "dog" }

// A parent applied at a concrete argument: the inherited bound reads
// `Functor[Box]`, and restating it that way is legal.
object AppliedParent {
  trait Ops[F[_]] { type T <: Functor[F] }
  trait Sub extends Ops[Box] { type T <: Functor[Box] }
  // Narrowing all the way to an alias is legal too.
  trait Fixed extends Ops[Box] { type T = Functor[Box] }
}

// Lower bounds go the other way: the override may only *widen* them.
object Lower {
  trait Ops[A] { type T >: Dog <: Animal }
  trait Sub[A] extends Ops[A] { type T >: Animal <: Animal }
}

// A type member that takes parameters of its own, inside a generic trait: both
// substitutions have to happen, the member's and the enclosing class's.
object Higher {
  trait Ops[A] { type M[B] <: Pair[A, B] }
  trait Sub[A] extends Ops[A] { type M[B] <: Pair[A, B] }
}

object Main {
  implicit val boxFunctor: Functor[Box] = new Functor[Box] {
    def map[A, B](fa: Box[A])(f: A => B): Box[B] = new Box(f(fa.value))
  }

  // The concrete `AllOps` cats' `toAllFunctorOps` builds.
  final class BoxOps[A](val self: Box[A], val typeClassInstance: Functor[Box])
      extends Functor.AllOps[Box, A] {
    type TypeClassType = Functor[Box]
  }

  def main(args: Array[String]): Unit = {
    val ops = new BoxOps[Int](new Box(1), boxFunctor)
    // `map` comes from `Functor.Ops`, `imap` from `Invariant.Ops` through the
    // other side of the diamond; both read `typeClassInstance` at the narrowed
    // bound.
    println(ops.map(_ + 1))
    println(ops.imap(_ + 10)(_ - 10))
    println(ops.self)

    // The same thing through the implicit conversion, whose result type is a
    // refinement fixing `TypeClassType`.
    println(Functor.ops.toAllFunctorOps(new Box("a")).map(_ + "b"))

    // A `Sub` whose `T` is fixed by the anonymous class.
    val f = new AppliedParent.Fixed {}
    val tc: f.T = boxFunctor
    println(tc.map(new Box(7))(_ * 2))

    val h = new Higher.Sub[String] { type M[B] = Pair[String, B] }
    val m: h.M[Int] = new Pair("k", 3)
    println(m)

    val l = new Lower.Sub[Int] { type T = Animal }
    val an: l.T = new Dog
    println(an)
  }
}

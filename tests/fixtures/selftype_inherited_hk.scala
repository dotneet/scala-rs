// An inherited self type has to be read with the linking parent's type
// arguments substituted into it.
//
// This is cats' `NTupleMonadInstances` shape. `class FlatMapBox extends
// FlatMap[Box]` inherits `self: FlatMap[F] =>` from `FlatMapArity[F]` through
// `trait FlatMap[F[_]] extends Apply[F] with FlatMapArity[F]`, so the inherited
// requirement means `FlatMap[Box]` here -- which the class plainly satisfies.
//
// **The declaration order is the whole test**, and it has to be a three-way
// order, which is why the natural hand-written hierarchy does not reproduce it:
//
//   1. `FlatMapArity`, which *declares* the self type, comes first -- so by the
//      time the class is reached its `self_type` is already bound and the
//      check has something to test.
//   2. `FlatMapBox`, the class, comes second.
//   3. `FlatMap`, which *supplies the argument* `F := Box`, comes last -- so
//      during the signature pass its own parents are still the unapplied
//      `Apply` / `FlatMapArity` that the header pass installs.
//
// With no arguments to substitute, `F` stays `F` and the class is rejected
// against `FlatMap[F]`. Put `FlatMap` before the class, or `FlatMapArity`
// after it, and the bug vanishes.
//
// In cats the same three-way order falls out of the file names: the generated
// `FlatMapArityFunctions.scala` sorts before `instances/NTupleMonadInstances.scala`,
// and hand-written `src/main/scala/cats/FlatMap.scala` sorts after both.
trait ApplyArity[F[_]] { self: Apply[F] =>
  // Curried rather than `(A, B) => Z` only so the fixture stays on `Function1`
  // and runs against the private runtime like its neighbours.
  def map2[A, B, Z](fa: F[A], fb: F[B])(f: A => B => Z): F[Z] =
    map(product(fa, fb))(p => f(p._1)(p._2))
}

trait FlatMapArity[F[_]] { self: FlatMap[F] =>
  def flatMap2[A, B, Z](fa: F[A], fb: F[B])(f: A => B => F[Z]): F[Z] =
    flatMap(fa)(a => flatMap(fb)(b => f(a)(b)))
}

case class Box[A](a: A)

class FlatMapBox extends FlatMap[Box] {
  def map[A, B](fa: Box[A])(f: A => B): Box[B] = Box(f(fa.a))
  def product[A, B](fa: Box[A], fb: Box[B]): Box[(A, B)] = Box((fa.a, fb.a))
  def flatMap[A, B](fa: Box[A])(f: A => Box[B]): Box[B] = f(fa.a)
}

trait Functor[F[_]] {
  def map[A, B](fa: F[A])(f: A => B): F[B]
}

trait Apply[F[_]] extends Functor[F] with ApplyArity[F] {
  def product[A, B](fa: F[A], fb: F[B]): F[(A, B)]
}

trait FlatMap[F[_]] extends Apply[F] with FlatMapArity[F] {
  def flatMap[A, B](fa: F[A])(f: A => F[B]): F[B]
}

object Main {
  def main(args: Array[String]): Unit = {
    val M = new FlatMapBox
    // Both of these are contributed *through* the inherited self types, so
    // they also prove the members those self types bring in are still found.
    println(M.map2(Box(2), Box(3))(a => b => a + b).a)
    println(M.flatMap2(Box(4), Box(5))(a => b => Box(a * b)).a)
    println(M.flatMap(Box(6))(x => Box(x + 1)).a)
  }
}

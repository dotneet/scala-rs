// The negative half of `selftype_inherited_hk.scala`.
//
// Holding the self-type conformance check back to the body pass must not make
// it stop rejecting anything. The declaration order here is the same three-way
// order that makes the positive fixture bite -- `FlatMapArity` first, the
// classes second, `FlatMap` last -- so both classes below are checked in the
// pass where the linking parent's arguments are finally known.
//
// Real scalac 2.13.16 rejects both, at the lines marked.
trait FlatMapArity[F[_]] { self: FlatMap[F] =>
  def flatMap2[A, B](fa: F[A], fb: F[B]): F[B] = fb
}

case class Box[A](a: A)
case class Cup[A](a: A)

// Inherits `self: FlatMap[F] =>` with `F := Box`, and is not a `FlatMap[Box]`.
// Substituting the argument in is exactly what turns this into a real error
// rather than a vacuous one: the requirement really is `FlatMap[Box]`.
class NotAFlatMap extends FlatMapArity[Box]

// Satisfies the *shape* but at the wrong argument: it is a `FlatMap[Cup]`,
// and the inherited requirement reads `FlatMap[Box]`. A check that compared
// against the unsubstituted `FlatMap[F]` would wave this through.
class WrongArg extends FlatMap[Cup] with FlatMapArity[Box] {
  def flatMap[A, B](fa: Cup[A])(f: A => Cup[B]): Cup[B] = f(fa.a)
}

trait FlatMap[F[_]] extends FlatMapArity[F] {
  def flatMap[A, B](fa: F[A])(f: A => F[B]): F[B]
}

// The tags scala-rs will not build, each named (`docs/macros.md` §7.10,
// §7.12).
//
// A materialiser that guessed would be discovered at *run* time, as a `Type`
// that is not the type the program asked about -- so every shape the creator
// cannot compose is refused here, saying which shape it was.
//
// A type constructor at its arguments is *not* on this list any more: it is
// built with `appliedType`, and `tt_tags.scala` runs the result against real
// scalac. Neither are tuples, function types and arrays (§7.13), which name
// `scala.TupleN` / `scala.FunctionN` / `scala.Array` and compose the same
// way. What is still refused is a constructor one of whose *arguments*
// cannot be built, and a type whose shape has no `staticClass` at all.

import scala.reflect.runtime.universe._

class Foo

object Nest {
  class Inner
}

object Main {
  // A type constructor whose *argument* cannot be built. The composition
  // recurses, so the reason names the argument rather than the constructor.
  val a = typeOf[List[Nest.Inner]]

  // A tuple whose *element* cannot be built. The tuple itself is composed
  // now -- `scala.Tuple2` at its arguments, `tt_tags.scala` runs it against
  // real scalac -- so what is left is the element that has no body.
  val b = weakTypeOf[(Int, Nest.Inner)]

  // A class nested in an object: `staticClass` walks packages only; nsc
  // reaches this one with `selectType` on the module class.
  val c = typeOf[Nest.Inner]

  // `AnyRef` is an alias for `java.lang.Object`, not a class.
  val d = typeOf[AnyRef]

  // An abstract type with no tag in scope. nsc's `WeakTypeTag` reifies it as
  // a *free* type; there is no `TypeTag` for it at all.
  def f[T]: Type = typeOf[T]
  def g[T]: Type = weakTypeOf[T]

  // A singleton type.
  val e = typeOf[Main.type]
}

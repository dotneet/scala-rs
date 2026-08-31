// The tags scala-rs will not build, each named (`docs/macros.md` §7.10).
//
// A materialiser that guessed would be discovered at *run* time, as a `Type`
// that is not the type the program asked about -- so every shape one
// `staticClass` call cannot rebuild is refused here, saying which shape it
// was.

import scala.reflect.runtime.universe._

class Foo

object Nest {
  class Inner
}

object Main {
  // A type constructor applied to arguments: nsc builds a `TypeRef` out of a
  // prefix, a symbol and the reified arguments.
  val a = typeOf[List[Int]]
  val b = weakTypeOf[Option[Foo]]

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

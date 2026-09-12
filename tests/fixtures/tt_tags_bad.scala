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
object Main {
  // A `TypeTag` for a type parameter with no tag in scope: nsc refuses it too
  // ("No TypeTag available for T"). A `WeakTypeTag` for the same is built,
  // with a free type -- `reify2_tags.scala`.
  def f[T]: Type = typeOf[T]
  // A refinement needs nsc's `newNestedSymbol` scope reification.
  val s = typeOf[{ def foo: Int }]
}

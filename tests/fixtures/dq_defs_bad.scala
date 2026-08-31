// The definition forms reification still refuses, each named.
//
// A quasiquote that silently built the wrong tree would be far worse than one
// that does not compile, so every gap in `crates/typer/src/reify_defs.rs` is
// an error saying *which* form is missing -- not a definition quietly reified
// as some other one. `docs/macros.md` §7.8.
import scala.reflect.runtime.universe._

object DqDefsBad {
  val parents: List[Tree] = List(tq"D")

  // The parser keeps no trace of the braces around an empty template body, and
  // a self type is exactly where nsc's own tree depends on them.
  val selfType = q"class C { self => def g = 1 }"

  // Early definitions carry nsc's `PRESUPER`, which is bit 37 -- outside the
  // parser's flag word entirely.
  val earlyDefs = q"class C extends { val x = 1 } with D"

  // `private[foo]` is a name in the `Modifiers`, not a flag.
  val qualified = q"private[foo] val x = 1"

  // A by-name parameter's type is `_root_.scala.<byname>[T]` in nsc, which the
  // parser records as a flag on the parameter instead.
  val byNameParam = q"def f(x: => Int) = x"

  // The same for a repeated one (`_root_.scala.<repeated>[T]`).
  val repeatedParam = q"def f(x: Int*) = x"

  // Procedure syntax has no result type in the source and nsc supplies
  // `_root_.scala.Unit`; the parser leaves the type empty.
  val procedure = q"def f() { 1 }"

  // The same gap with no body at all.
  val noTypeNoBody = q"def f()"

  // A pattern definition is one `SyntacticPatDef` in nsc and three definitions
  // after our parser has desugared it.
  val patternDef = q"{ val (a, b) = e; a }"

  // A higher-kinded type parameter has type parameters of its own.
  val higherKinded = q"class C[F[_]]"

  // Bounds that are not `<:` / `>:` are desugared by nsc's typer, not its
  // parser, and are not part of the reified tree's shape here.
  val contextBound = q"def f[T: Ordering] = 1"

  // A `case` class's parents are the written ones plus `Product with
  // Serializable`; concatenating those onto a splice needs a static type that
  // is right on both sides.
  val caseSplice = q"case class C(x: Int) extends ..$parents"

  // nsc passes a trailing implicit clause separately, as `ImplicitParams`;
  // there is no shape for one in the middle.
  val implicitFirst = q"def f(implicit x: Int)(y: Int) = y"

  // A macro definition's right-hand side is a reference to an implementation,
  // not an expression.
  val macroDef = q"def f = macro Impl.f"

  // A `_` type argument is an existential whose bound type parameters nsc
  // names with `freshTypeName`, in a block around the call -- the same reason
  // the `_` placeholder lambda is refused.
  val existential = q"def f(x: Bar[_]) = x"
}

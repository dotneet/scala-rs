// Macro *implementations*: the path-dependent types their signatures are
// written in, and quasiquotes in their bodies. `docs/macros.md` §7.6.
//
// `scala.reflect.macros.blackbox.Context` inherits, from
// `scala.reflect.macros.Aliases`,
//
//     type Tree           = universe.Tree
//     type Expr[T]        = universe.Expr[T]
//     type WeakTypeTag[T] = universe.WeakTypeTag[T]
//
// so `c.Tree` / `c.Expr[T]` / `c.WeakTypeTag[T]` cannot be named at all until
// a jar class's *type* members are read from its pickle. Until they were,
// nothing in this file typechecked -- and `import c.universe._` did not put a
// universe in scope either, so every `q"..."` below reported the generic
// "cannot expand quasiquote" rather than being reified.
//
// Compile-only, and compiled by real scalac 2.13.16 too: expanding these
// needs the JVM bridge (`docs/macros.md` §2.2), which is not built. What is
// checked here is that the implementation itself compiles, and that the class
// file it produces loads and verifies.
import scala.reflect.macros.blackbox

object QqCtx {
  // `c.Tree` in both an argument and the result.
  def dup(c: blackbox.Context)(x: c.Tree): c.Tree = {
    import c.universe._
    q"scala.List($x, $x)"
  }

  // `c.Expr[T]`: a *parameterised* alias, which keeps type parameters of its
  // own so each use substitutes. And the universe's names in type position.
  def viaExpr(c: blackbox.Context)(x: c.Expr[Int]): c.Tree = {
    import c.universe._
    val n: TermName = TermName("f")
    val t: TypeName = TypeName("T")
    val lit: Tree = Literal(Constant(1))
    q"g($lit)"
  }

  // `c.WeakTypeTag[T]`, in the implicit clause a macro implementation's type
  // parameters arrive through.
  def tagged[T](c: blackbox.Context)(implicit t: c.WeakTypeTag[T]): c.Tree = {
    import c.universe._
    val s: TypeSymbol = t.tpe.typeSymbol.asType
    val e: Tree = EmptyTree
    q"h"
  }

  // The shape slick's `ShapedValue.mapToImpl` is written in: the context is a
  // *refinement*, and the aliases still have to be found through it.
  def prefixed(c: blackbox.Context { type PrefixType = List[Int] }): c.Tree = {
    import c.universe._
    val trees: List[Tree] = List(q"k", q"m")
    q"n(..$trees)"
  }
}

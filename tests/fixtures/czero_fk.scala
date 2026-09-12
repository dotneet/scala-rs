// cats' one macro, self-contained: `core/src/main/scala-2/cats/arrow/
// FunctionKMacros.scala` plus the two traits it needs. It was the last file
// held out of `tests/cats_measure.sh` (`CATS_EXCLUDE=FunctionKMacros.scala`),
// and it put four different gaps in the way at once:
//
//  * a quasiquote *pattern*, `case q"($param) => $trans[..$typeArgs]($arg)"`
//    (`crates/typer/src/quasi_pattern.rs`);
//  * a macro definition whose implementation asks for tags over *applied* type
//    constructors, `c.WeakTypeTag[F[Any]]` (see `czero_macrotag.scala`);
//  * `for (typeArg @ TypeTree() <- typeArgs)`, a pattern-filtered `for` with no
//    `yield` over the `List[Tree]` a rank-1 hole binds;
//  * `q"${reify(FunctionK)}.liftFunction[$F, $G]($trans(_))"` as the result.
//
// The package clause is the reason every name `quasi_pattern` builds is
// qualified: `cats` declares a `cats.Apply`, which outranks the wildcard
// `import c.universe._` (SLS 2), so a bare `Apply(…)` in the deconstruction
// would have bound to the type class.
//
// Real scalac 2.13.16 compiles this file. The macro is not applied here --
// cats does not apply it either, the one mention of `FunctionK.lift` in its
// sources being in a scaladoc -- so this is a compile-only fixture.
package cats
package arrow

import scala.language.experimental.macros
import scala.reflect.macros.blackbox

trait FunctionK[F[_], G[_]] { self =>
  def apply[A](fa: F[A]): G[A]
}

private[arrow] trait FunctionKLift {
  def liftFunction[F[_], G[_]](f: F[Any] => G[Any]): FunctionK[F, G] =
    new FunctionK[F, G] {
      def apply[A](fa: F[A]): G[A] = f(fa.asInstanceOf[F[Any]]).asInstanceOf[G[A]]
    }
}

private[arrow] class FunctionKMacroMethods extends FunctionKLift {
  def lift[F[_], G[_]](f: (F[α] => G[α]) forSome { type α }): FunctionK[F, G] =
    macro FunctionKMacros.lift[F, G]
}

object FunctionK extends FunctionKMacroMethods

private[arrow] object FunctionKMacros {

  def lift[F[_], G[_]](c: blackbox.Context)(
    f: c.Expr[(F[α] => G[α]) forSome { type α }]
  )(implicit evF: c.WeakTypeTag[F[Any]], evG: c.WeakTypeTag[G[Any]]): c.Expr[FunctionK[F, G]] =
    c.Expr[FunctionK[F, G]](new Lifter[c.type](c).lift[F, G](f.tree))

  private class Lifter[C <: blackbox.Context](val c: C) {
    import c.universe._

    def lift[F[_], G[_]](tree: Tree)(implicit evF: c.WeakTypeTag[F[Any]],
                                     evG: c.WeakTypeTag[G[Any]]): Tree = {
      def liftFunction(function: Tree): Tree =
        function match {
          case q"($param) => $trans[..$typeArgs]($arg)" if param.symbol == arg.symbol =>
            for (typeArg @ TypeTree() <- typeArgs) if (typeArg.original != null) {
              c.abort(
                typeArg.pos,
                s"type parameter $typeArg must not be supplied when lifting function $trans to FunctionK"
              )
            }

            val F = typeConstructorOf[F[Any]]
            val G = typeConstructorOf[G[Any]]
            q"${reify(FunctionK)}.liftFunction[$F, $G]($trans(_))"

          case other =>
            c.abort(other.pos, s"Unexpected tree $other when lifting to FunctionK")
        }

      tree match {
        case Block(Nil, expr)   => liftFunction(expr)
        case Block(stats, expr) => Block(stats, liftFunction(expr))
        case other              => liftFunction(other)
      }
    }

    private def typeConstructorOf[A: WeakTypeTag]: Type =
      weakTypeOf[A].typeConstructor.etaExpand
  }
}

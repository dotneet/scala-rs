// The shape `reify { … }` expands into, written out by hand and expanded for
// real. `docs/macros.md` §7.13.4, §7.14.
//
// nsc's `-Xprint:typer` for `reify { 42 }` inside a macro implementation is
//
//     { val $u: c.universe.type = c.universe
//       val $m: $u.Mirror = c.universe.rootMirror
//       $u.Expr.apply[Int]($m, new $treecreator1())($u.TypeTag.apply[Int](…)) }
//
// with the tree built inside `$treecreator1.apply[U](m: Mirror[U]): U#Tree`.
// scala-rs does not *build* that shape yet -- `reify` is still the §7.8
// diagnostic -- but every piece of it is here, so the machinery is what this
// file pins: the nested `object Expr` and its `apply` (whose pickled
// signature says `Mirror[Universe.this.type]` and cannot be converted, so it
// is written out the way `TypeTag.apply` is), `Mirror[c.universe.type]` as a
// stable path, and the two things a reified body has to do with names --
// resolve a static symbol through `staticModule` and splice an `Expr` in
// through `in`.
//
// Compiled on its own so `rd_use.scala` can expand against it, the split nsc
// requires (§1.3).
import scala.reflect.macros.blackbox.Context

/** A static module the reified body refers to, the way `reify` writes a
  * static symbol as `_root_`-qualified rather than by the name it was written
  * with. */
object RdHelper {
  def twice(n: Int): Int = n * 2
}

object RdImpl {
  /** `reify { 42 }`: a literal, and nothing else. */
  def fortyTwo(c: Context): c.Expr[Int] = {
    import c.universe._
    final class rdtreecreator1 extends scala.reflect.api.TreeCreator {
      def apply[U <: scala.reflect.api.Universe with Singleton](
          m: scala.reflect.api.Mirror[U]
      ): U#Tree = {
        val u = m.universe
        u.Literal(u.Constant(42))
      }
    }
    c.universe.Expr.apply[Int](
      c.universe.rootMirror.asInstanceOf[scala.reflect.api.Mirror[c.universe.type]],
      new rdtreecreator1()
    )
  }

  /** `reify { RdHelper.twice(x.splice) }`: a static symbol and a splice.
    *
    * `RdHelper` is reached through the *mirror the creator is handed*, not
    * through the name as written -- that is reify's hygiene. `x.in(mm).tree`
    * is what `.splice` becomes: the argument's tree, rebased into that same
    * mirror.
    *
    * `mm` is cast to `scala.reflect.api.Mirror[u.type]` rather than to
    * `u.Mirror` (which is how nsc writes it): `u.Mirror` is an abstract type
    * member whose bound `api.Mirror[self.type]` scala-rs drops when it reads
    * the pickle, so `x.in[u.type](mm)` would not typecheck. Both compilers
    * accept the spelling below and the two programs print the same thing.
    */
  def twice(c: Context)(x: c.Expr[Int]): c.Expr[Int] = {
    import c.universe._
    final class rdtreecreator2 extends scala.reflect.api.TreeCreator {
      def apply[U <: scala.reflect.api.Universe with Singleton](
          m: scala.reflect.api.Mirror[U]
      ): U#Tree = {
        val u = m.universe
        val mm = m.asInstanceOf[scala.reflect.api.Mirror[u.type]]
        u.Apply(
          u.Select(
            u.internal.reificationSupport.mkIdent(mm.staticModule("RdHelper")),
            u.TermName("twice")
          ),
          scala.collection.immutable.List(x.in[u.type](mm).tree)
        )
      }
    }
    c.universe.Expr.apply[Int](
      c.universe.rootMirror.asInstanceOf[scala.reflect.api.Mirror[c.universe.type]],
      new rdtreecreator2()
    )
  }

  /** The two gaps §7.13.4 names, in the position they are needed: the nested
    * `object Expr` reached through the path *and* through the wildcard
    * import, and `c.universe` written as a stable identifier in a type
    * argument. Both were diagnostics ("value Expr is not a member of
    * Universe", "not found: value Expr", "stable identifier required, but
    * c.universe found").
    *
    * The answer is carried out to the call site rather than merely compiled,
    * so a wrong receiver -- which compiles either way -- shows up as a
    * different line of output.
    */
  def probes(c: Context): c.Expr[Boolean] = {
    import c.universe._
    val viaPath = c.universe.Expr
    val viaImport = Expr
    val m: scala.reflect.api.Mirror[c.universe.type] =
      c.universe.rootMirror.asInstanceOf[scala.reflect.api.Mirror[c.universe.type]]
    val ok = viaPath.eq(viaImport) && m.staticClass("scala.Int").fullName == "scala.Int"
    final class rdtreecreator3 extends scala.reflect.api.TreeCreator {
      def apply[U <: scala.reflect.api.Universe with Singleton](
          mm: scala.reflect.api.Mirror[U]
      ): U#Tree = {
        val u = mm.universe
        u.Literal(u.Constant(ok))
      }
    }
    c.universe.Expr.apply[Boolean](
      c.universe.rootMirror.asInstanceOf[scala.reflect.api.Mirror[c.universe.type]],
      new rdtreecreator3()
    )
  }
}

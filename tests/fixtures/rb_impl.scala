// `reify { … }` expanded by scala-rs itself (`docs/macros.md` §7.14).
//
// `tests/fixtures/rd_impl.scala` writes out, by hand, the tree `reify` has to
// build; this file writes `reify` and lets the compiler build it. The two
// files are the same experiment from opposite ends, and `rb_use.scala` runs
// the expansions the way `rd_use.scala` runs the hand-written ones.
//
// Everything is in one file on purpose: real scalac costs 1.8 seconds a run
// and the dual run compiles this twice.
//
// Compiled on its own so `rb_use.scala` can expand against it, the split nsc
// requires (§1.3).
import scala.reflect.macros.blackbox.Context

/** The static module the reified bodies refer to. `reify` writes a static
  * symbol as `mirror.staticModule("RbHelper")`, resolved in the universe the
  * expansion lands in -- not as the name it was written with. */
object RbHelper {
  def twice(n: Int): Int = n * 2
  def join(a: String, b: String): String = a + "-" + b
  def flip(b: Boolean): Boolean = !b
  def id[T](x: T): T = x
  def pair[A, B](a: A, b: B): String = a.toString + "/" + b.toString
}

object RbImpl {
  /** A literal, and nothing else: no mirror, no names. */
  def fortyTwo(c: Context): c.Expr[Int] = {
    import c.universe._
    reify { 42 }
  }

  /** Each literal kind that reaches `Constant`. */
  def hello(c: Context): c.Expr[String] = {
    import c.universe._
    reify { "hello" }
  }

  def yes(c: Context): c.Expr[Boolean] = {
    import c.universe._
    reify { true }
  }

  def big(c: Context): c.Expr[Long] = {
    import c.universe._
    reify { 9000000000L }
  }

  /** A static symbol, reached through the mirror the creator is handed. */
  def helper(c: Context): c.Expr[Int] = {
    import c.universe._
    reify { RbHelper.twice(21) }
  }

  /** A splice: the argument's own tree, rebased into that same mirror. */
  def twice(c: Context)(x: c.Expr[Int]): c.Expr[Int] = {
    import c.universe._
    reify { RbHelper.twice(x.splice) }
  }

  /** Two splices, and an operator whose name has to be encoded (`$plus`). */
  def sum(c: Context)(a: c.Expr[Int], b: c.Expr[Int]): c.Expr[Int] = {
    import c.universe._
    reify { a.splice + b.splice }
  }

  /** A splice under a static symbol, at a reference type. */
  def join(c: Context)(a: c.Expr[String]): c.Expr[String] = {
    import c.universe._
    reify { RbHelper.join(a.splice, "tail") }
  }

  /** A selection on a spliced expression, and a static symbol around it. */
  def flipped(c: Context)(b: c.Expr[Boolean]): c.Expr[Boolean] = {
    import c.universe._
    reify { RbHelper.flip(b.splice) }
  }

  /** `reify` written on the universe rather than imported from it. */
  def qualified(c: Context): c.Expr[Int] = c.universe.reify { 7 }

  /** A *type argument* built from a monomorphic class: one `staticClass`. */
  def three(c: Context): c.Expr[Int] = {
    import c.universe._
    reify { RbHelper.id[Int](3) }
  }

  /** A type argument that is a type *parameter*, knowable only through the
    * tag in scope -- `tag.in($m).tpe`. This is the shape slick's
    * `TableQueryMacroImpl` needs: `reify { TableQuery.apply[E](cons.splice) }`.
    */
  def idOf[T](c: Context)(x: c.Expr[T])(implicit t: c.WeakTypeTag[T]): c.Expr[T] = {
    import c.universe._
    reify { RbHelper.id[T](x.splice) }
  }

  /** Two of them, so the tags are told apart rather than reused. */
  def pair[A, B](c: Context)(a: c.Expr[A], b: c.Expr[B])(implicit
      ta: c.WeakTypeTag[A],
      tb: c.WeakTypeTag[B]
  ): c.Expr[String] = {
    import c.universe._
    reify { RbHelper.pair[A, B](a.splice, b.splice) }
  }
}

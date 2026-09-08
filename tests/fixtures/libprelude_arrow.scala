// `->` when the run's own sources define `scala.Predef`.
//
// The prelude spells `Predef`'s `->` conversion `any2ArrowAssoc`, which is
// 2.10's name for it -- `javap -p scala.Predef$` on scala-library-2.13.16.jar
// has `public final <A> A ArrowAssoc(A)` and no `any2ArrowAssoc` at all --
// while the `implicit final class ArrowAssoc` below synthesizes `ArrowAssoc`.
// `predef_reimport` supersedes the prelude's snapshot by *name*, the two
// names never met, both conversions stayed in scope offering `->` for the
// same source type, and `search_extension` could separate neither: every
// `a -> b` in `src/library` was `value -> is not a member` (28 errors,
// 3 files).
//
// Run rather than compiled: the conversion has to be *called*, on the source
// `ArrowAssoc` and not on the prelude's stand-in, and the pair it builds has
// to come out with the operands in the right order.
package scala {
  // Defining `scala.Predef` replaces the real one for the whole run -- that
  // is the point of the fixture -- so nothing the real one supplies is in
  // scope below: no `println`, no `any2stringadd`.
  object Predef {
    // `AnyVal.getClass` names `Predef.Class`, so a value class cannot be
    // declared without it (real scalac 2.13.16 says so).
    type Class[T] = java.lang.Class[T]
    type String = java.lang.String

    implicit final class ArrowAssoc[A](private val self: A) extends AnyVal {
      def ->[B](y: B): (A, B) = (self, y)
    }
  }
}

object Main {
  // A generic caller: the conversion is applied to a bare type parameter, not
  // to a type the tie-break could special-case.
  def pair[A, B](a: A, b: B): (A, B) = a -> b

  // The components rather than the pair itself: the private runtime's
  // `Tuple2` has no `toString` override, so printing the pair would differ
  // between the two modes for a reason that has nothing to do with `->`.
  // Everything spelled out through `java.lang`: `println` and
  // `any2stringadd` are members of the *real* `Predef`, which the source one
  // above supersedes.
  def show(a: Any, b: Any): Unit =
    java.lang.System.out.println(
      java.lang.String.valueOf(a) + "|" + java.lang.String.valueOf(b)
    )

  def main(args: Array[String]): Unit = {
    val p = 1 -> "a"
    show(p._1, p._2)
    val q = "x" -> 2
    show(q._1, q._2)
    val r = pair(true, 'c')
    show(r._1, r._2)
    // Chained: `->` is left-associative and its result is an ordinary pair.
    val s = (1 -> 2) -> 3
    show(s._1._1, s._1._2)
    show(s._2, "end")
  }
}

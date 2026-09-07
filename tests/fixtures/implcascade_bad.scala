// An implicit search that fails must report once and stop.
//
// Two shapes, both taken from gitbucket:
//
//   * the wanted type is itself erroneous, so the search must not run at all
//     (`extractFromJsonBody[A](implicit request: HttpServletRequest,
//     mf: Manifest[A])` in a compiler whose `Predef` has no `Manifest`);
//   * the witness that was not found was the only thing that could have said
//     what one of the callee's type parameters is, so the application is an
//     error tree and nothing may be typed against its result (slick's
//     `map[F, T, G](f)(implicit shape: Shape[_, F, T, G]): Query[G, T, C]`).
//
// scalac 2.13.16 reports exactly two errors here, at lines 22 and 38, and
// nothing at 25, 26 or anywhere else.
object Cascade {

  implicit val anInt: Int = 1
  implicit val aString: String = "s"

  // The parameter type does not exist. scalac reports that, once, and says
  // nothing at either call site below -- an erroneous type is not a search.
  def tagged[A](implicit tag: NoSuchTag[A]): List[A] = Nil

  def useTagged(): Unit = {
    println(tagged[Long].head)
    println(tagged[String].head.length)
  }

  // Only the witness can say what `T` is.
  trait Shape[F, T]
  class Rows[E] {
    def project[F, T](f: E => F)(implicit shape: Shape[F, T]): List[T] = Nil
  }

  def useProject(rows: Rows[Int]): Unit = {
    // No `Shape` in scope: scalac reports the missing implicit and nothing
    // about `_1`, which it never types.
    println(rows.project(x => x.toString).head._1)
  }
}

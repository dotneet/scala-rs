// The other half of `implcascade_bad.scala`: when the witness *is* found, the
// type parameter it determines has to reach the program, and everything
// selected on the result still typechecks and runs.
//
// Suppressing the cascade after a failed implicit search is only correct if a
// successful one is untouched, and the shape that matters is the one slick's
// `map[F, T, G](f)(implicit shape: Shape[_, F, T, G]): Query[G, T, C]` has:
// `T` appears nowhere in the value arguments, so the witness is the only thing
// that says what it is. Guarding that with a compile-only test would not show
// a wrong `T`; this one prints the values, so a wrong one is visible.
object Main {

  trait Tag[A] { def name: String }
  implicit val longTag: Tag[Long] = new Tag[Long] { def name = "Long" }
  implicit val stringTag: Tag[String] = new Tag[String] { def name = "String" }

  def tagged[A](xs: List[A])(implicit tag: Tag[A]): String =
    tag.name + ":" + xs.mkString(",")

  trait Shape[F, T] { def unpack(f: F): T }
  implicit val pairShape: Shape[(String, Int), (String, Int)] =
    new Shape[(String, Int), (String, Int)] {
      def unpack(f: (String, Int)): (String, Int) = f
    }
  implicit val singleShape: Shape[Int, String] = new Shape[Int, String] {
    def unpack(f: Int): String = "#" + f
  }

  class Rows[E](es: List[E]) {
    def project[F, T](f: E => F)(implicit shape: Shape[F, T]): List[T] =
      es.map(e => shape.unpack(f(e)))
  }

  // An implicit parameter whose type mentions no type parameter at all still
  // has to be found the ordinary way.
  def greet(implicit tag: Tag[String]): String = "greet-" + tag.name

  def main(args: Array[String]): Unit = {
    println(tagged(List(1L, 2L)))
    println(tagged(List("a", "b")))
    println(greet)
    val rows = new Rows(List(1, 2, 3))
    val pairs = rows.project(x => (x.toString, x * 10))
    println(pairs.map(_._1).mkString(","))
    println(pairs.head._2)
    val singles = rows.project(x => x + 1)
    println(singles.mkString(","))
    println(singles.head.length)
  }
}

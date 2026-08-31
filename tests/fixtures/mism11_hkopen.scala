// An undetermined *type constructor* in an argument's expected type.
//
// `flatMap[F, T, D[_]](f: E => Qry[F, T, D])` leaves `D` for the call to
// solve, and the argument position reached the lambda with `D` opened up to
// its bound. A type constructor has no bound that is a type: `Any` is not one
// of its inhabitants and not even the same kind, so the body's
// `Qry[G, T, Box]` was `found: Qry[G, T, Box]  required: Qry[G, T, Any]`.
// This is slick's `Query.map` (`Query.scala:37`), which is written exactly
// this way.

class Box[A](val a: A)

trait Shp[E, U, R] {
  def repack(e: E): R
}

class Qry[E, U, C[_]](val value: E) {
  def flatMap[F, T, D[_]](f: E => Qry[F, T, D]): Qry[F, T, C] =
    new Qry[F, T, C](f(value).value)

  def map[F, G, T](f: E => F)(implicit shape: Shp[F, T, G]): Qry[G, T, C] =
    flatMap(v => Qry[F, T, G](f(v)))
}

object Qry {
  def apply[E, U, R](value: E)(implicit shape: Shp[E, U, R]): Qry[R, U, Box] =
    new Qry[R, U, Box](shape.repack(value))
}

object Main {
  implicit val intShape: Shp[Int, Int, Int] = new Shp[Int, Int, Int] {
    def repack(e: Int): Int = e
  }
  implicit val strShape: Shp[String, String, String] = new Shp[String, String, String] {
    def repack(e: String): String = e
  }

  def main(args: Array[String]): Unit = {
    val q = new Qry[Int, Int, Box](1)
    println(q.map(_ + 1).value)
    println(q.map(i => "n" + i).value)
    // A proper (kind-0) parameter still reaches the lambda opened to its
    // bound, which is what lets the body use `String`'s members.
    println(bounded(x => x.length))
  }

  def bounded[A >: String](f: A => Int): Int = f("abcd")
}

object Syntax {
  implicit class Ops[A, B](val f: A => Option[B]) {
    def andThenF(g: B => Option[String]): A => Option[String] =
      a => f(a).flatMap(g)
  }
}
object Main {
  import Syntax._
  def main(args: Array[String]): Unit = {
    val f: Int => Option[String] = x => Some(x.toString)
    val a = f.andThenF(s => Some(s.length.toString))
    println(a(4).get)
  }
}

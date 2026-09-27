import scala.language.implicitConversions

// A by-name parameter whose type still has the callee's type parameter in it
// takes an argument through a view like a strict one does. The thunk used to
// hand back the unconverted argument (`ClassCastException`).
class P[+T](val tag: String) {
  def <~[U](q: => P[U]): P[T] = new P[T](tag + " <~ " + q.tag)
  def |[U >: T](q: => P[U]): P[U] = new P[U](tag + " | " + q.tag)
  def ~[U](q: => P[U]): P[(T, U)] = new P[(T, U)](tag + " ~ " + q.tag)
  def strict[U](q: P[U]): P[T] = new P[T](tag + " strict " + q.tag)
}

object Main {
  implicit def lit(s: String): P[String] = new P[String]("lit(" + s + ")")
  def main(args: Array[String]): Unit = {
    val p = new P[Int]("p")
    println((p <~ ")").tag)
    val any: P[Any] = p | ")"
    println(any.tag)
    println((p ~ ")").tag)
    println((p strict ")").tag)
  }
}

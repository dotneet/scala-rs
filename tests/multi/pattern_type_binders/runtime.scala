sealed trait A[T]
final class B[T](val value: T) extends A[T]
object Main {
  type t = String
  def exact(a: A[Int]): Int = a match {
    case b: B[t] => val x: t = b.value; val n: Int = x; n
  }
  def unknown(a: Any): Any = a match {
    case b: B[t] => val x: t = b.value; x
    case _ => "other"
  }
  def main(args: Array[String]): Unit = {
    println(exact(new B[Int](42)))
    println(unknown(new B[String]("bound")))
    println(unknown(7))
  }
}

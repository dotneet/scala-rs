// A bound that declares no `apply` must stay a rejection: scalac says
// "T does not take parameters".
object Main {
  class Box[A](val a: A)
  def a[T](x: T): Int = x(1)
  def b[T <: AnyRef](x: T): Int = x(1)
  def c[T <: Box[Int]](x: T): Int = x(1)
  def main(args: Array[String]): Unit = ()
}

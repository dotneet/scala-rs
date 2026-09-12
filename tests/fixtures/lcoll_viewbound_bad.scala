// No view makes these members reachable, and scalac rejects all three.
object Main {
  class Box[A](val a: A)
  def a[T](x: T): Int = x.iterator.size
  def b[T](x: T)(implicit w: T <:< Int): Int = x.iterator.size
  def c[T](x: T)(implicit w: Box[T]): Int = x.iterator.size
  def main(args: Array[String]): Unit = ()
}

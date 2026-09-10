trait Box[A]
class IntBox extends Box[Int]
object Main {
  def accept[M[_ <: A], A](x: M[A]): M[A] = x
  def lower[M[_ >: A], A](x: M[A]): M[A] = x
  def main(args: Array[String]): Unit = {
    println(accept[Box, Int](new IntBox).isInstanceOf[IntBox])
    println(lower[Box, Int](new IntBox).isInstanceOf[IntBox])
  }
}

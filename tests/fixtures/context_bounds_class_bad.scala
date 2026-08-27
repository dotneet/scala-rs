trait Show[T]
class C[T: Show](val x: T)
object Main {
  def main(args: Array[String]): Unit = {
    new C(1)
  }
}

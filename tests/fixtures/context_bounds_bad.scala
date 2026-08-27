trait Show[T]
object Main {
  def show[T: Show](x: T): T = x
  def main(args: Array[String]): Unit = {
    show(1)
  }
}

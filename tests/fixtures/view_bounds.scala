class Box(val n: Int) extends Ordered[Box] {
  def compare(that: Box): Int = n - that.n
}
object Main {
  def lt[T <% Ordered[T]](a: T, b: T): Boolean = a < b
  def main(args: Array[String]): Unit = {
    println(lt(new Box(1), new Box(2)))
    println(lt(new Box(3), new Box(1)))
  }
}

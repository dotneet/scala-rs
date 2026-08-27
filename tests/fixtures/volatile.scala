class Box {
  @volatile var x: Int = 0
  @transient var y: Int = 7
}
object Main {
  def main(args: Array[String]): Unit = {
    val b = new Box
    b.x = 3
    println(b.x)
    println(b.y)
    b.y = 9
    println(b.y)
  }
}

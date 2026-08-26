class Box[+A](val value: A) {
  def get: A = value
}
object Main {
  def main(args: Array[String]): Unit = {
    val b: Box[Int] = new Box(41)
    println(b.get + 1)
  }
}

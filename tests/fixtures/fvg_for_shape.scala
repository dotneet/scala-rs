class Box[A](val value: A) {
  def map[B](f: A => B): Box[B] = {
    val next = f(value)
    println(next.toString)
    new Box[B](next)
  }
}

object Main {
  def main(args: Array[String]): Unit = {
    val result = for {
      x <- new Box[Int](1)
      y = x + 1
      z = y + 1
    } yield x + y + z
    println(result.value)
  }
}

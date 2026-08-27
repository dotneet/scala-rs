object Main {
  def main(args: Array[String]): Unit = {
    println(Array(1, 2, 3).foldLeft(0)(_ + _))
    println(Array(1, 2, 3).fold(0)(_ + _))
    println(Array(1, 2, 3).foldRight(0)(_ + _))
  }
}

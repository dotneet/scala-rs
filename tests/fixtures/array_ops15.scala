object Main {
  def main(args: Array[String]): Unit = {
    Array(1, 2, 3).scanLeft(0)(_ + _).foreach(x => println(x))
    println(Array(1, 2, 3).count(_ > 1))
    println(Array(1, 2, 3).forall(_ > 0))
    println(Array(1, 2, 3).forall(_ > 1))
  }
}

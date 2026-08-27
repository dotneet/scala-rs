object Main {
  def main(args: Array[String]): Unit = {
    val xs = List(1, 2, 3).view.map(_ + 1).toList
    println(xs)
    val ys = scala.collection.View.fill(3)(7).toList
    println(ys)
    val zs = scala.collection.View.iterate(1, 4)(_ + 1).toList
    println(zs)
  }
}

object Main {
  def main(a: Array[String]): Unit = {
    val xs = for { i <- 1 to 3; j <- 1 to 2 if i != j } yield (i, j)
    println(xs.toList)
    val m = Map(1 -> "a", 2 -> "b")
    for ((k, v) <- m.toList.sorted) println(k + "=" + v)
  }
}

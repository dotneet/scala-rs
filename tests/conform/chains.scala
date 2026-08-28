object Main {
  def main(args: Array[String]): Unit = {
    val xs = List(1, 2, 3, 4)
    val r = xs
      .map(_ + 1)
      .filter(_ > 2)
      .take(2)
    println(r)
    val s = xs
      .mkString(
        "[",
        ",",
        "]"
      )
    println(s)
    val t =
      if (xs.nonEmpty) "yes"
      else "no"
    println(t)
    val u = xs.map { v =>
      v * 2
    }
    println(u)
  }
}

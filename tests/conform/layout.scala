object Main {
  def add(
    a: Int,
    b: Int,
  ): Int = a + b
  def main(args: Array[String]): Unit = {
    println(add(
      1,
      2,
    ))
    val xs = List(
      1,
      2,
      3,
    )
    println(xs)
    val long = xs.map(_ * 2)
      .filter(_ > 2)
      .sum
    println(long)
    println(Seq
      (1, 2).sum)
  }
}

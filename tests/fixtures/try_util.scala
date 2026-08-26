object Main {
  def main(args: Array[String]): Unit = {
    val t = Try(1)
    println(t.map((x: Int) => x + 1).getOrElse(0))
    val s = Success(2)
    println(s.getOrElse(0))
    val f = Failure(new RuntimeException())
    println(f.getOrElse(0))
  }
}

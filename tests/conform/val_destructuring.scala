object Main {
  def main(a: Array[String]): Unit = {
    val (x, y) = (1, "a")
    println(x, y)
    val h :: t = List(1, 2, 3): @unchecked
    println(h, t)
    val Some(v) = Option(42): @unchecked
    println(v)
    val (p, (q, r)) = (1, (2, 3))
    println(p + q + r)
    val Array(first, rest @ _*) = "a b c".split(" "): @unchecked
    println(first, rest.toList)
    var (m, n) = (10, 20)
    m += 1
    println(m, n)
  }
}

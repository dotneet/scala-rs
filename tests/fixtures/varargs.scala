object Main {
  def count(xs: Int*): Int = xs.length
  def total(xs: Int*): Int = { var t = 0; xs.foreach(x => t += x); t }
  def main(args: Array[String]): Unit = {
    println(count(1, 2, 3))
    println(count())
    println(total(1, 2, 3))
    val s = Seq(4, 5)
    println(count(s: _*))
    println(total(s: _*))
  }
}

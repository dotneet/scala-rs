object Main {
  def main(args: Array[String]): Unit = {
    val xs = List(1, 2, 3, 4)
    println(xs.foldLeft(0)((a, b) => a + b))
    println(xs.foldLeft(100)((a, b) => a - b))
    println(xs.foldRight(0)((a, b) => a - b))
    val joined: String = xs.foldLeft("")((a, b) => a + "<" + b)
    println(joined)
    println(xs.reduce((a, b) => a + b))
    println(xs.reduceLeft((a, b) => a - b))
    println(xs.reduceRight((a, b) => a - b))
    println(xs.scanLeft(0)((a, b) => a + b).mkString(","))
    val words = List("a", "bb", "ccc")
    println(words.foldLeft(0)((n, w) => n + w.length))
    println(words.reduce((a, b) => a + b))
  }
}

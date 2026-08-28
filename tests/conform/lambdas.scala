object Main {
  def apply2(f: Int => Int): Int = f(2)
  def main(args: Array[String]): Unit = {
    val xs = List(1, 2, 3)
    println(xs.map { x =>
      val y = x + 1
      y * 2
    })
    println(apply2 { x =>
      val a = x * 10
      val b = a + 1
      b
    })
    println(xs.map { (x: Int) =>
      val y = x - 1
      y
    })
    println(xs.foldLeft(0) { (acc, x) =>
      val s = acc + x
      s
    })
    println(xs.map(x => x + 1))
  }
}

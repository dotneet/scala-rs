object Main {
  def f(n: Int): Int = {
    var i = 0
    var acc = 0
    while (i < n) {
      val x = i * 2
      if (x > 2) { val y = x + 1; acc += y } else acc += x
      i += 1
    }
    acc
  }
  def main(args: Array[String]): Unit = println(f(4))
}
